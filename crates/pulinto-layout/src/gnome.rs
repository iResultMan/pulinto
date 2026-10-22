//! GNOME on Wayland: which layout is in effect, and how to move to another.
//!
//! **No out-of-band switch exists here.** The X11 controller sets the group
//! over the X socket; under Wayland the keymap belongs to the compositor, and
//! GNOME Shell offers no D-Bus call to change it (`Eval` is closed, IBus
//! engines are followed by the Shell, not the other way round — measured). So
//! the switch is made the way the user makes it: with the layout hotkey, typed
//! through the daemon's own virtual keyboard. [`LayoutController::layout_chord`]
//! tells the caller which keys those are.
//!
//! **Why that is better than it sounds.** When the hotkey is an XKB option
//! (`grp:alt_shift_toggle` and friends), mutter applies it inside the same
//! input queue the following letters travel through, and every Wayland client
//! receives the new group in `wl_keyboard.modifiers` before the next key.
//! Ordering is guaranteed by the protocol, so no confirmation wait is needed.
//! Measured on GNOME 50: 0 of 450 words lost with no pause at all after the
//! switch, both in a Wayland-native and in an Xwayland window. The Shell's own
//! binding (`<Super>space`) is not ordered that way — it goes through the
//! Shell's main loop — and is used only as a fallback, with a wait.
//!
//! **Reading the current layout.** GNOME Shell writes `mru-sources` on every
//! switch, the active source first. A `gsettings monitor` child reports it about
//! 35 ms after the switch; the daemon's own switches are noted immediately
//! ([`LayoutController::note_group`]) so the next read does not lag behind.

use crate::{LayoutChord, LayoutController, LayoutError};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock};

const SCHEMA: &str = "org.gnome.desktop.input-sources";

// Evdev key codes of the modifiers a group-toggle option can name.
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_LEFTALT: u16 = 56;
const KEY_CAPSLOCK: u16 = 58;
const KEY_SPACE: u16 = 57;
const KEY_LEFTMETA: u16 = 125;

#[derive(Debug, Default, Clone)]
struct State {
    /// `(layout id, variant)` per input source, in the order GNOME numbers them.
    sources: Vec<(String, String)>,
    /// Index into `sources` of the active one.
    current: usize,
    /// What `mru-sources` last said, untouched by `note_group`: the daemon's
    /// own belief after a chord is checked against this.
    confirmed: Option<usize>,
    /// Heads of `mru-sources` the daemon is still owed for chords it has seen
    /// typed — its own and the user's — oldest first. GNOME reports a switch
    /// about 35 ms after it, so a second chord can be noted before the first
    /// is reported; taking that late report as the truth would roll `current`
    /// back to a group the keyboard has already left.
    expected: VecDeque<usize>,
    chord: Option<LayoutChord>,
}

/// More chords in flight than this and the reports are not coming: `gsettings
/// monitor` is gone, or the chord does nothing here. Forget the oldest.
const EXPECTED_LIMIT: usize = 8;

impl State {
    /// A chord to `group` has been typed (by the daemon or the user).
    fn note_group(&mut self, group: usize) {
        if group >= self.sources.len() {
            return;
        }
        self.current = group;
        if self.expected.len() >= EXPECTED_LIMIT {
            self.expected.pop_front();
        }
        self.expected.push_back(group);
    }

    /// GNOME has written `mru-sources` with `head` first. Returns whether the
    /// report was the one expected next; an unexpected one is a switch made
    /// elsewhere (the panel, the lock screen) and resets the bookkeeping.
    /// GNOME also rewrites the list without switching (a change to `sources`,
    /// the settings panel): while chords are still unreported, a report of the
    /// group we are already on says nothing and is let through.
    fn mru_event(&mut self, head: usize) -> bool {
        self.confirmed = Some(head);
        if self.expected.front() == Some(&head) {
            self.expected.pop_front();
            true
        } else if !self.expected.is_empty() && head == self.current {
            true
        } else {
            self.expected.clear();
            self.current = head;
            false
        }
    }

    /// The group as GNOME last reported it, once every chord typed so far has
    /// been reported; `None` while reports are still on their way, because a
    /// report that arrives then may be about an earlier chord.
    fn confirmed_group(&self) -> Option<usize> {
        self.expected.is_empty().then_some(self.confirmed).flatten()
    }
}

pub struct GnomeLayoutController {
    state: Arc<RwLock<State>>,
}

impl GnomeLayoutController {
    /// Refuses when this is not a GNOME session with at least one input source
    /// GNOME will tell us about.
    pub fn new() -> Result<Self, LayoutError> {
        let sources = parse_sources(&gsettings_get("sources")?);
        if sources.is_empty() {
            return Err(LayoutError::Unsupported);
        }
        let mut state = State {
            sources,
            ..State::default()
        };
        state.current = current_index(&state.sources, &gsettings_get("mru-sources")?);
        state.confirmed = Some(state.current);
        state.chord = pick_chord(
            &parse_strings(&gsettings_get("xkb-options").unwrap_or_default()),
            state.sources.len(),
        );
        let state = Arc::new(RwLock::new(state));
        watch(Arc::clone(&state));
        Ok(Self { state })
    }

    fn read(&self) -> State {
        self.state.read().map(|state| state.clone()).unwrap_or_default()
    }
}

impl LayoutController for GnomeLayoutController {
    fn name(&self) -> &'static str {
        "Wayland (GNOME)"
    }

    fn get_current_group(&self) -> Result<u8, LayoutError> {
        u8::try_from(self.read().current).map_err(|_| LayoutError::Unsupported)
    }

    /// There is no way to set a group from outside the key stream on GNOME;
    /// the caller types [`LayoutController::layout_chord`] instead.
    fn set_group(&self, _group: u8) -> Result<(), LayoutError> {
        Err(LayoutError::Unsupported)
    }

    fn layout_ids(&self) -> Vec<String> {
        self.read().sources.into_iter().map(|(id, _)| id).collect()
    }

    fn layout_variants(&self) -> Vec<String> {
        self.read()
            .sources
            .into_iter()
            .map(|(_, variant)| variant)
            .collect()
    }

    fn layout_chord(&self) -> Option<LayoutChord> {
        self.read().chord
    }

    fn note_group(&self, group: u8) {
        if let Ok(mut state) = self.state.write() {
            state.note_group(usize::from(group));
        }
    }

    fn confirmed_group(&self) -> Option<u8> {
        self.read()
            .confirmed_group()
            .and_then(|group| u8::try_from(group).ok())
    }
}

fn gsettings_get(key: &str) -> Result<String, LayoutError> {
    let output = Command::new("gsettings")
        .args(["get", SCHEMA, key])
        .stderr(Stdio::null())
        .output()
        .map_err(|_| LayoutError::Unsupported)?;
    if !output.status.success() {
        return Err(LayoutError::Unsupported);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Keep `state` current from `gsettings monitor`. The child prints one line per
/// change, `key: value`; it lives as long as the daemon does.
fn watch(state: Arc<RwLock<State>>) {
    let child = Command::new("gsettings")
        .args(["monitor", SCHEMA])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("GNOME: layout watching not started, the current layout will go stale");
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        return;
    };
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Some((key, value)) = line.split_once(": ") else {
                continue;
            };
            let Ok(mut state) = state.write() else { break };
            match key.trim() {
                "sources" => {
                    let sources = parse_sources(value);
                    if !sources.is_empty() {
                        state.sources = sources;
                        state.chord = pick_chord(
                            &parse_strings(&gsettings_get("xkb-options").unwrap_or_default()),
                            state.sources.len(),
                        );
                    }
                }
                "mru-sources" => {
                    let head = current_index(&state.sources, value);
                    let expected = state.mru_event(head);
                    pulinto_core::diag!(
                        "GNOME: layout from mru-sources — group {head}{}",
                        if expected { "" } else { " (switched not by us)" }
                    );
                }
                "xkb-options" => {
                    state.chord = pick_chord(&parse_strings(value), state.sources.len());
                }
                _ => {}
            }
        }
        let _ = child.wait();
        eprintln!("GNOME: layout watching stopped");
    });
}

/// What `set_apple` answers when `gsettings` refuses the new list; the daemon
/// recognises it to show the message in the interface language.
pub const REFUSED: &str = "gsettings did not accept the new list of layouts";

/// Switch the Apple arrangement (`us+mac`, `ru+mac`) on or off in GNOME's own
/// input sources — the setting GNOME applies to the compositor and keeps across
/// sessions. `setxkbmap` would only reach Xwayland. A variant the user picked
/// themselves (Russian `phonetic`) is left alone, as on X11.
pub fn set_apple(enabled: bool) -> Result<(), String> {
    let current = gsettings_get("sources").map_err(|error| error.to_string())?;
    let strings = parse_strings(&current);
    let mut items = Vec::new();
    for pair in strings.chunks(2).filter(|pair| pair.len() == 2) {
        let (kind, name) = (&pair[0], &pair[1]);
        let mut name = name.clone();
        if kind == "xkb" {
            let (id, variant) = name.split_once('+').unwrap_or((name.as_str(), ""));
            if id == "us" || id == "ru" {
                if enabled && variant.is_empty() {
                    name = format!("{id}+mac");
                } else if !enabled && variant == "mac" {
                    name = id.to_string();
                }
            }
        }
        items.push(format!("('{kind}', '{name}')"));
    }
    let value = format!("[{}]", items.join(", "));
    if value == current {
        return Ok(());
    }
    let status = Command::new("gsettings")
        .args(["set", SCHEMA, "sources", &value])
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(REFUSED.into())
    }
}

/// `[('xkb', 'us'), ('xkb', 'ru+mac')]` → `[("us", ""), ("ru", "mac")]`.
/// Non-XKB sources (IBus engines) keep their name as the id; no profile will
/// match it, which is the honest answer.
fn parse_sources(value: &str) -> Vec<(String, String)> {
    let strings = parse_strings(value);
    strings
        .chunks(2)
        .filter(|pair| pair.len() == 2)
        .map(|pair| {
            let name = &pair[1];
            if pair[0] == "xkb" {
                match name.split_once('+') {
                    Some((id, variant)) => (id.to_string(), variant.to_string()),
                    None => (name.clone(), String::new()),
                }
            } else {
                (name.clone(), String::new())
            }
        })
        .collect()
}

/// Every single-quoted string in a GVariant text, in order.
fn parse_strings(value: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('\'') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('\'') else { break };
        strings.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    strings
}

/// Position of the first MRU source among `sources`; 0 when GNOME has not
/// written the list yet (a fresh profile), which is also what it means then.
fn current_index(sources: &[(String, String)], mru: &str) -> usize {
    let Some(first) = parse_sources(mru).into_iter().next() else {
        return 0;
    };
    sources.iter().position(|source| *source == first).unwrap_or(0)
}

/// The hotkey that moves to the next group, from the XKB options when one is
/// set there — those are applied in order with the keys that follow. Without
/// one, GNOME's own `<Super>space`, which only toggles when there are exactly
/// two sources and is not ordered with the key stream.
fn pick_chord(options: &[String], sources: usize) -> Option<LayoutChord> {
    let in_band = |keys: Vec<u16>| {
        Some(LayoutChord {
            keys,
            ordered: true,
        })
    };
    for option in options {
        match option.as_str() {
            "grp:alt_shift_toggle" => return in_band(vec![KEY_LEFTALT, KEY_LEFTSHIFT]),
            "grp:ctrl_shift_toggle" => return in_band(vec![KEY_LEFTCTRL, KEY_LEFTSHIFT]),
            "grp:ctrl_alt_toggle" => return in_band(vec![KEY_LEFTCTRL, KEY_LEFTALT]),
            "grp:caps_toggle" => return in_band(vec![KEY_CAPSLOCK]),
            "grp:win_space_toggle" => return in_band(vec![KEY_LEFTMETA, KEY_SPACE]),
            _ => {}
        }
    }
    (sources == 2).then(|| LayoutChord {
        keys: vec![KEY_LEFTMETA, KEY_SPACE],
        ordered: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_read_with_their_variants() {
        let sources = parse_sources("[('xkb', 'us'), ('xkb', 'ru+mac'), ('ibus', 'mozc-jp')]");
        assert_eq!(
            sources,
            vec![
                ("us".to_string(), String::new()),
                ("ru".to_string(), "mac".to_string()),
                ("mozc-jp".to_string(), String::new()),
            ]
        );
        assert!(parse_sources("@a(ss) []").is_empty());
    }

    #[test]
    fn the_current_layout_is_the_first_recently_used_source() {
        let sources = parse_sources("[('xkb', 'us'), ('xkb', 'ru')]");
        assert_eq!(current_index(&sources, "[('xkb', 'ru'), ('xkb', 'us')]"), 1);
        assert_eq!(current_index(&sources, "[('xkb', 'us'), ('xkb', 'ru')]"), 0);
        assert_eq!(current_index(&sources, "@a(ss) []"), 0);
    }

    fn two_sources() -> State {
        State {
            sources: parse_sources("[('xkb', 'us'), ('xkb', 'ru')]"),
            confirmed: Some(0),
            ..State::default()
        }
    }

    #[test]
    fn a_late_report_of_our_own_chord_does_not_roll_the_group_back() {
        // The daemon switches 0 → 1, the user taps the chord back 1 → 0 before
        // GNOME has reported the first switch; then both reports arrive.
        let mut state = two_sources();
        state.note_group(1);
        state.note_group(0);
        assert_eq!(state.current, 0);
        assert!(state.mru_event(1));
        assert_eq!(state.current, 0, "the first report is history, not news");
        assert_eq!(state.confirmed_group(), None, "one report still owed");
        assert!(state.mru_event(0));
        assert_eq!(state.current, 0);
        assert_eq!(state.confirmed_group(), Some(0));
        assert!(state.expected.is_empty());
    }

    #[test]
    fn a_rewrite_without_a_switch_does_not_disown_the_chords_in_flight() {
        let mut state = two_sources();
        state.note_group(1);
        state.note_group(0);
        // GNOME rewrote the list for another reason, head unchanged.
        assert!(state.mru_event(0));
        assert_eq!(state.expected.len(), 2);
        assert_eq!(state.current, 0);
        assert!(state.mru_event(1));
        assert!(state.mru_event(0));
        assert_eq!(state.confirmed_group(), Some(0));
    }

    #[test]
    fn a_switch_made_elsewhere_is_taken_as_the_truth() {
        let mut state = two_sources();
        state.note_group(1);
        // Our chord never got reported; the user picks a source in the panel.
        assert!(!state.mru_event(0));
        assert_eq!(state.current, 0);
        assert_eq!(state.confirmed_group(), Some(0));
        assert!(state.expected.is_empty());
        // Nothing is owed any more, so the next report is news as well.
        assert!(!state.mru_event(1));
        assert_eq!((state.current, state.confirmed), (1, Some(1)));
    }

    #[test]
    fn unreported_chords_do_not_pile_up() {
        let mut state = two_sources();
        for _ in 0..3 * EXPECTED_LIMIT {
            state.note_group(1);
        }
        assert_eq!(state.expected.len(), EXPECTED_LIMIT);
        state.note_group(5);
        assert_eq!(state.current, 1, "a group GNOME does not have is ignored");
    }

    #[test]
    fn an_xkb_toggle_is_preferred_and_is_ordered() {
        let chord = pick_chord(&["grp:alt_shift_toggle".to_string()], 2).unwrap();
        assert_eq!(chord.keys, vec![KEY_LEFTALT, KEY_LEFTSHIFT]);
        assert!(chord.ordered);
    }

    #[test]
    fn without_an_xkb_toggle_the_shell_binding_works_only_for_two_layouts() {
        let chord = pick_chord(&[], 2).unwrap();
        assert_eq!(chord.keys, vec![KEY_LEFTMETA, KEY_SPACE]);
        assert!(!chord.ordered);
        assert!(pick_chord(&[], 3).is_none());
    }
}
