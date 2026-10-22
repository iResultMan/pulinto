//! The replacement mechanic, once, for both input loops.
//!
//! There used to be two of these — one per engine — and the second one was
//! wrong: it typed target characters straight after `toggle_next()` with no
//! pause, so the keys arrived before the layout did and were swallowed
//! (`cktlbnm` → `cktlbn`). Two copies of a mechanic this delicate will drift
//! again, so there is now one copy and the engines differ only in what they
//! pass in.
//!
//! Everything here is synchronous. Grab mode must inject inline — it is holding
//! the delimiter while this runs, and that ordering is the whole reason grab
//! mode exists. Observe mode has no such constraint and calls it on a thread of
//! its own so the capture reader keeps up.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pulinto_core::layout::Layout;
use pulinto_data::profile::Profile;
use pulinto_input::injector::Injector;
use pulinto_layout::{LayoutChord, LayoutController};

/// The last finished word and everything needed for a safe Alt cycle.
#[derive(Debug, Clone)]
pub struct Fix {
    pub strokes: Vec<(u16, bool)>,
    pub original: String,
    /// Whose reading the word on the screen is in — a profile index. From it
    /// the Alt cycle computes the next step for an already finished word.
    pub shown: usize,
    /// The delimiter typed right after the word. Alt erases it together with
    /// the word and types it again, without moving the cursor with arrows.
    /// `None` means the word is not finished yet, or the delimiter is unsafe
    /// for the cycle (Enter/Tab: a Backspace across the line edge eats someone else's text).
    pub delimiter: Option<WordDelimiter>,
}

/// A delimiter of a finished word that is safe for the Alt cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordDelimiter {
    /// The fallback uinput path needs the physical key.
    pub key: (u16, bool),
    /// The XTest path needs the character already shown: it is kept literally and
    /// does not depend on which layout is active during the next step.
    pub ch: char,
}

/// A fully computed, safe Alt step for a finished word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordCycle {
    pub to: usize,
    pub current_text: String,
    pub target_text: String,
    /// The word plus exactly one already typed delimiter.
    pub erase: usize,
    pub delimiter: WordDelimiter,
}

pub fn plan_word_cycle(fix: &Fix, profiles: &[Profile]) -> Option<WordCycle> {
    if profiles.len() < 2 || fix.shown >= profiles.len() {
        return None;
    }
    let delimiter = fix.delimiter?;
    let to = (fix.shown + 1) % profiles.len();
    let current_text =
        crate::scanword::ScanWord::render_keys(&fix.strokes, profiles[fix.shown].layout());
    let target_text = crate::scanword::ScanWord::render_keys(&fix.strokes, profiles[to].layout());
    Some(WordCycle {
        to,
        erase: current_text.chars().count() + 1,
        current_text,
        target_text,
        delimiter,
    })
}

/// The last selection replacement — so that a single Alt can cycle it round.
#[derive(Debug, Clone)]
pub struct SelectionFix {
    /// The text now on the screen in place of the selection.
    pub text: String,
    /// Whose profile it is — an index.
    pub shown: usize,
}

/// What the daemon has learned this session and the last word Alt may still
/// edit. Both engines must share it.
///
/// It used to live only in observe mode, which meant a word the user had
/// reverted came straight back the moment they switched to grab — the
/// "looping" complaint that makes people uninstall Punto Switcher.
#[derive(Debug, Default)]
pub struct Learning {
    reverted: Mutex<HashSet<String>>,
    last_fix: Mutex<Option<Fix>>,
    last_selection: Mutex<Option<SelectionFix>>,
    /// When the last word or selection was remembered.
    remembered_at: Mutex<Option<std::time::Instant>>,
}

impl Learning {
    /// A word the user has already rejected once. Auto-conversion must not
    /// touch it again this session.
    pub fn is_reverted(&self, word: &str) -> bool {
        self.reverted
            .lock()
            .map(|set| set.contains(&word.to_lowercase()))
            .unwrap_or(false)
    }

    pub fn mark_reverted(&self, word: &str) {
        if let Ok(mut set) = self.reverted.lock() {
            set.insert(word.to_lowercase());
        }
    }

    pub fn remember(&self, fix: Fix) {
        self.stamp(Some(std::time::Instant::now()));
        if let Ok(mut slot) = self.last_selection.lock() {
            *slot = None;
        }
        if let Ok(mut slot) = self.last_fix.lock() {
            *slot = Some(fix);
        }
    }

    /// Take the last word for one Alt step. The caller puts the updated snapshot
    /// back only after the replacement succeeds.
    pub fn take_last(&self) -> Option<Fix> {
        self.last_fix.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Is there a last word or selection an Alt could still cycle?
    pub fn has_last(&self) -> bool {
        self.last_fix.lock().is_ok_and(|slot| slot.is_some())
            || self.last_selection.lock().is_ok_and(|slot| slot.is_some())
    }

    /// The word is no longer "the last": the user typed something else and
    /// the cursor moved away. A cycle over a stale snapshot would erase text in
    /// the wrong place — exactly how Alt once turned `merge.` into `mergepfntv`.
    pub fn invalidate_last(&self) {
        self.stamp(None);
        if let Ok(mut slot) = self.last_fix.lock() {
            *slot = None;
        }
        if let Ok(mut slot) = self.last_selection.lock() {
            *slot = None;
        }
    }

    pub fn unmark_reverted(&self, word: &str) {
        if let Ok(mut set) = self.reverted.lock() {
            set.remove(&word.to_lowercase());
        }
    }

    pub fn remember_selection(&self, fix: SelectionFix) {
        // A selection and a word cannot both own the next Alt. Keeping the old
        // word here made a freshly highlighted word lose to an unrelated
        // snapshot and sent Backspace to the wrong range.
        self.stamp(Some(std::time::Instant::now()));
        if let Ok(mut slot) = self.last_fix.lock() {
            *slot = None;
        }
        if let Ok(mut slot) = self.last_selection.lock() {
            *slot = Some(fix);
        }
    }

    fn stamp(&self, at: Option<std::time::Instant>) {
        if let Ok(mut slot) = self.remembered_at.lock() {
            *slot = at;
        }
    }

    /// When the last word or selection that an Alt could cycle was remembered.
    pub fn remembered_at(&self) -> Option<std::time::Instant> {
        self.remembered_at.lock().ok().and_then(|slot| *slot)
    }

    pub fn take_selection(&self) -> Option<SelectionFix> {
        self.last_selection
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }
}

/// Where the cursor sits relative to the word being replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// Right after the word — the manual hotkey, and everything in grab mode,
    /// where the delimiter is still being withheld.
    AtWord,
    /// One character past the word, because a delimiter has already reached
    /// the application. Only observe mode, and only because it cannot hold a
    /// key back. Carries the key that produced the delimiter, so it can be
    /// erased along with the word and typed again afterwards.
    ///
    /// The obvious alternative — step left over the delimiter, edit, step
    /// right — is what this replaces, and it was a mistake. Arrow keys are
    /// interpreted by the application, not by the text: a browser input, a
    /// list with keyboard navigation or an autocomplete popup will each do
    /// something different with a Left, and the backspaces that follow then
    /// land wherever the cursor ended up. The user watched corrected words
    /// appear at the top of a chat instead of under the cursor. Erasing one
    /// character more and retyping it needs no cursor movement at all.
    PastDelimiter { key: (u16, bool) },
    /// One character past a delimiter that belongs to the typo itself. It is
    /// erased with the source and deliberately not typed back (`ещ\\` → `ещё`).
    PastConsumedDelimiter,
}

/// Erase the word and replay its keys under `target_layout_id`.
///
/// Returns the layout that was in effect beforehand, which is also the layout
/// left in effect afterwards: switching is a means, not an end. The letters
/// have to come out of the keymap, so the keymap has to be on the target
/// layout while the keys are replayed — and then it goes back.
///
/// **Both the switch and the way back go by name.** Saving the group number
/// and setting it again looks equivalent and is not: on an IBus session the
/// keymap can be rebuilt between the two, and the number then names a
/// different layout. That is how a correction ended with the user in Russian
/// mid-English-sentence, typing words the daemon never even saw — it ignores
/// input while injecting, and it holds no grab in observe mode.
pub fn replay_under(
    injector: &Injector,
    layout: &dyn LayoutController,
    strokes: &[(u16, bool)],
    target_layout_id: &str,
    cursor: Cursor,
    held_modifiers: &[u16],
) -> std::io::Result<Option<String>> {
    let restore = layout.current_layout_id();

    injector.release_modifiers(held_modifiers)?;

    // GNOME on Wayland: the switch is a hotkey typed through this same
    // keyboard. With an XKB group option the compositor applies it in order
    // with the keys that follow, so there is nothing to wait for — before the
    // replay or after it.
    if let (Some(chord), Some(target)) = (layout.layout_chord(), layout.group_of(target_layout_id))
    {
        let result = (|| {
            switch_by_chord(injector, layout, &chord, target)?;
            injector.backspaces(strokes.len() + extra_erase(cursor))?;
            injector.replay(strokes)?;
            if let Cursor::PastDelimiter { key } = cursor {
                injector.tap(key.0, key.1)?;
            }
            Ok(())
        })();
        let restored = injector.restore_modifiers(held_modifiers);
        return result.and(restored).map(|()| restore);
    }

    // Establish a durable group through the same virtual keyboard that will
    // replay the letters. A bare XkbLockGroup is overwritten by the first
    // uinput event on this Xorg setup, which can split one corrected word
    // between two layouts.
    let aimed = match layout.group_of(target_layout_id) {
        Some(target) => {
            layout.set_group(0).map_err(std::io::Error::other)?;
            for _ in 0..target {
                injector.advance_layout_group(false)?;
            }
            Some(target)
        }
        None => {
            layout.toggle_next().map_err(std::io::Error::other)?;
            None
        }
    };
    wait_for_group(injector, layout, aimed);
    injector.backspaces(strokes.len() + extra_erase(cursor))?;
    injector.replay(strokes)?;
    // The keys are still in flight. This one is *not* a layout barrier and
    // cannot become one: nothing to confirm, we are waiting for keys already
    // sent to be consumed before the keymap changes under them.
    injector.wait_for_layout();

    // We do NOT restore the layout back to `restore`. The entire point of the
    // correction is that the user was in the wrong layout and wants to continue
    // typing in the new one.
    if let Cursor::PastDelimiter { key } = cursor {
        injector.tap(key.0, key.1)?;
    }
    injector.restore_modifiers(held_modifiers)?;

    Ok(restore)
}

/// Type the layout hotkey until the keymap is on `target`.
///
/// Each press moves to the next group, so the count is the distance from the
/// current one. An ordered chord (XKB option) is noted at once — the
/// compositor will apply it before our next key, but GNOME reports it only
/// ~35 ms later. A desktop shortcut is not ordered and is confirmed instead;
/// unconfirmed, the replacement is refused before anything is erased.
pub fn switch_by_chord(
    injector: &Injector,
    layout: &dyn LayoutController,
    chord: &LayoutChord,
    target: u8,
) -> std::io::Result<()> {
    let count = layout.layout_ids().len().max(2);
    let current = usize::from(layout.get_current_group().map_err(std::io::Error::other)?);
    let steps = (usize::from(target) + count - current % count) % count;
    if steps == 0 {
        return Ok(());
    }
    // Said in the log: when the screen ends up in the wrong layout, the
    // question is always which group the daemon believed it was in.
    pulinto_core::diag!("Layout by chord: group {current} → {target}, presses: {steps}");
    for _ in 0..steps {
        injector.chord(&chord.keys)?;
    }
    if chord.ordered {
        layout.note_group(target);
        return Ok(());
    }
    if confirm_group(layout, target, Duration::from_millis(CHORD_BARRIER_MS)) {
        std::thread::sleep(Duration::from_millis(SETTLE_MS));
        Ok(())
    } else {
        Err(std::io::Error::other(
            "the layout did not switch by the desktop chord",
        ))
    }
}

/// How long a desktop shortcut gets to be confirmed. GNOME reports a switch in
/// about 35 ms; this is that with a wide margin.
const CHORD_BARRIER_MS: u64 = 400;

/// One extra character to erase when a delimiter has already been typed.
fn extra_erase(cursor: Cursor) -> usize {
    match cursor {
        Cursor::AtWord => 0,
        Cursor::PastDelimiter { .. } | Cursor::PastConsumedDelimiter => 1,
    }
}

/// Erase `erase` characters and type `text` in the layout already in effect.
///
/// Only for corrections that stay inside one layout — the case fixes
/// (`ПРивет` → `Привет`). Anything crossing layouts must replay scan codes.
pub fn retype_text(
    injector: &Injector,
    layout: &'static Layout,
    erase: usize,
    text: &str,
    cursor: Cursor,
    held_modifiers: &[u16],
) -> std::io::Result<()> {
    // Fail closed before the first destructive key. Previously type_text
    // silently skipped an unsupported symbol after the source was erased.
    injector.validate_text(text, layout)?;
    injector.release_modifiers(held_modifiers)?;
    let result = (|| {
        injector.backspaces(erase + extra_erase(cursor))?;
        injector.type_text(text, layout)?;
        if let Cursor::PastDelimiter { key } = cursor {
            injector.tap(key.0, key.1)?;
        }
        Ok(())
    })();
    let restore = injector.restore_modifiers(held_modifiers);
    result.and(restore)
}

/// How long to wait for a switch to be confirmed before giving up on the
/// confirmation and falling back to the blind margin.
const BARRIER_MS: u64 = 150;
/// How often to ask. With the watcher running this is an atomic read.
const POLL_MS: u64 = 2;
/// Margin kept *after* the switch is confirmed.
///
/// Confirmation says the X server has applied the new group, which is what our
/// scan codes will be mapped through. It says nothing about the application
/// receiving them: GTK and Qt clients translate keys themselves against their
/// own cached keymap, updated by an XKB event of their own. That is a second
/// independent channel, and this is the margin for it.
/// Raised from 15 ms after a run came back with `jrtfys` corrected to `jкеаны`:
/// the first replayed key arrived before the receiving application had applied
/// the new keymap, so it drew the old letter while the rest drew the new ones.
/// Confirmation covers the server; this covers the client, and the client is
/// the one that decides what the letter looks like.
const SETTLE_MS: u64 = 30;

/// Wait until the keymap reports `group`, rather than guessing how long the
/// switch takes.
///
/// What this replaces was 50 ms of blind sleep, three times per replacement,
/// against a group change that lands in single-digit milliseconds — so the
/// margin was both most of the cost of a correction and, when the machine was
/// busy, still not always enough. The daemon already watches XKB `StateNotify`,
/// so the switch can be *observed*: `get_current_group` is then an atomic read.
///
/// Without a watcher every poll would be an X round trip, and on Wayland the
/// group cannot be read at all — there the blind margin is still the answer.
fn wait_for_group(injector: &Injector, layout: &dyn LayoutController, group: Option<u8>) {
    match group {
        Some(group) if confirm_group(layout, group, Duration::from_millis(BARRIER_MS)) => {
            std::thread::sleep(Duration::from_millis(SETTLE_MS));
        }
        // Nothing to confirm, or never confirmed and the switch may still be in
        // flight: fall back to the margin we used to take unconditionally
        // rather than charge ahead.
        _ => injector.wait_for_layout(),
    }
}

/// Poll until the keymap reports `group`. False means it never did.
fn confirm_group(layout: &dyn LayoutController, group: u8, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if layout.get_current_group().is_ok_and(|now| now == group) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};

    /// Reports `target` only after `delay` questions, so a barrier can be told
    /// apart from a sleep: a sleep takes the same time either way.
    struct SlowLayout {
        target: u8,
        asked: AtomicU8,
        delay: u8,
    }

    impl LayoutController for SlowLayout {
        fn name(&self) -> &'static str {
            "slow"
        }
        fn get_current_group(&self) -> Result<u8, pulinto_layout::LayoutError> {
            let asked = self.asked.fetch_add(1, Ordering::SeqCst);
            Ok(if asked >= self.delay { self.target } else { 9 })
        }
        fn set_group(&self, _group: u8) -> Result<(), pulinto_layout::LayoutError> {
            Ok(())
        }
    }

    #[test]
    fn a_confirmed_switch_is_not_waited_out() {
        let layout = SlowLayout {
            target: 1,
            asked: AtomicU8::new(0),
            delay: 3,
        };
        let started = Instant::now();
        assert!(confirm_group(&layout, 1, Duration::from_millis(BARRIER_MS)));
        // Three polls at POLL_MS, not the 50 ms margin this replaces.
        assert!(
            started.elapsed() < Duration::from_millis(40),
            "барьер ждал {:?} — это уже не подтверждение, а пауза",
            started.elapsed()
        );
    }

    /// A switch that never lands must not be declared done: the caller falls
    /// back to the blind margin, which is what kept `Русская.` from coming out
    /// `Русскаяю`.
    #[test]
    fn a_switch_that_never_arrives_is_not_confirmed() {
        let layout = SlowLayout {
            target: 1,
            asked: AtomicU8::new(0),
            delay: u8::MAX,
        };
        assert!(!confirm_group(&layout, 1, Duration::from_millis(20)));
    }

    /// Observe mode erases the delimiter too, so it never has to move the
    /// cursor; grab mode still holds the delimiter and erases only the word.
    #[test]
    fn the_delimiter_is_erased_only_when_it_has_already_been_typed() {
        assert_eq!(extra_erase(Cursor::AtWord), 0);
        assert_eq!(extra_erase(Cursor::PastDelimiter { key: (57, false) }), 1);
        assert_eq!(extra_erase(Cursor::PastConsumedDelimiter), 1);
    }

    #[test]
    fn alt_after_space_erases_the_word_and_exactly_one_space() {
        let profiles = pulinto_data::profile::all();
        let fix = Fix {
            // G,H: `gh` under US and `пр` under Russian.
            strokes: vec![(34, false), (35, false)],
            original: "gh".into(),
            shown: 0,
            delimiter: Some(WordDelimiter {
                key: (57, false),
                ch: ' ',
            }),
        };

        let cycle = plan_word_cycle(&fix, &profiles).expect("пробел допускает цикл");
        assert_eq!(cycle.current_text, "gh");
        assert_eq!(cycle.target_text, "пр");
        assert_eq!(cycle.erase, 3, "два символа слова плюс один пробел");
        assert_eq!(cycle.delimiter.ch, ' ');
    }

    #[test]
    fn a_word_without_a_safe_delimiter_cannot_erase_backwards() {
        let profiles = pulinto_data::profile::all();
        let fix = Fix {
            strokes: vec![(34, false), (35, false)],
            original: "gh".into(),
            shown: 0,
            delimiter: None,
        };
        assert!(plan_word_cycle(&fix, &profiles).is_none());
    }

    #[test]
    fn a_new_short_word_replaces_a_stale_long_snapshot() {
        let profiles = pulinto_data::profile::all();
        let learning = Learning::default();
        learning.remember(Fix {
            strokes: vec![(34, false); 12],
            original: "gggggggggggg".into(),
            shown: 0,
            delimiter: Some(WordDelimiter {
                key: (57, false),
                ch: ' ',
            }),
        });

        // The first key of a new word must clear the old range.
        learning.invalidate_last();
        learning.remember(Fix {
            strokes: vec![(34, false), (35, false)],
            original: "gh".into(),
            shown: 0,
            delimiter: Some(WordDelimiter {
                key: (57, false),
                ch: ' ',
            }),
        });

        let fix = learning.take_last().expect("сохранено только новое слово");
        let cycle = plan_word_cycle(&fix, &profiles).unwrap();
        assert_eq!(cycle.current_text, "gh");
        assert_eq!(cycle.erase, 3);
    }

    #[test]
    fn a_reverted_word_is_remembered_case_insensitively() {
        let learning = Learning::default();
        learning.mark_reverted("Привет");
        assert!(learning.is_reverted("привет"));
        assert!(learning.is_reverted("ПРИВЕТ"));
        assert!(!learning.is_reverted("пока"));
    }

    #[test]
    fn the_last_word_can_be_taken_exactly_once() {
        let learning = Learning::default();
        assert!(learning.take_last().is_none());

        learning.remember(Fix {
            strokes: vec![(34, false), (35, false)],
            original: "gh".into(),
            shown: 1,
            delimiter: Some(WordDelimiter {
                key: (57, false),
                ch: ' ',
            }),
        });
        let taken = learning.take_last().expect("одна отмена есть");
        assert_eq!(taken.original, "gh");
        // Tapping the undo hotkey twice must not undo the same thing twice.
        assert!(learning.take_last().is_none());
    }

    #[test]
    fn a_new_keystroke_invalidates_the_last_fix_and_selection() {
        // The right to an Alt cycle lasts exactly until the next key press: erasing
        // by a stale replacement lands in the wrong place (`merge.` → `mergepfntv`).
        let learning = Learning::default();
        learning.remember(Fix {
            strokes: vec![(34, false)],
            original: "g".into(),
            shown: 0,
            delimiter: Some(WordDelimiter {
                key: (57, false),
                ch: ' ',
            }),
        });
        learning.remember_selection(SelectionFix {
            text: "привет".into(),
            shown: 1,
        });

        learning.invalidate_last();
        assert!(learning.take_last().is_none());
        assert!(learning.take_selection().is_none());
    }

    #[test]
    fn a_fresh_selection_supersedes_the_internal_last_word() {
        let learning = Learning::default();
        learning.remember(Fix {
            strokes: vec![(34, false)],
            original: "g".into(),
            shown: 0,
            delimiter: Some(WordDelimiter {
                key: (57, false),
                ch: ' ',
            }),
        });
        learning.remember_selection(SelectionFix {
            text: "раскладки".into(),
            shown: 1,
        });

        assert!(learning.take_last().is_none());
        assert_eq!(
            learning.take_selection().map(|selection| selection.text),
            Some("раскладки".into())
        );
    }

    #[test]
    fn a_remembered_replacement_knows_its_age() {
        let learning = Learning::default();
        assert!(learning.remembered_at().is_none());
        let before = std::time::Instant::now();
        learning.remember_selection(SelectionFix {
            text: "раскладки".into(),
            shown: 1,
        });
        assert!(learning.remembered_at().is_some_and(|at| at >= before));
        learning.invalidate_last();
        assert!(learning.remembered_at().is_none() && !learning.has_last());
    }

    #[test]
    fn typing_a_new_word_ends_an_old_selection_cycle() {
        let learning = Learning::default();
        learning.remember_selection(SelectionFix {
            text: "раскладки".into(),
            shown: 1,
        });
        learning.remember(Fix {
            strokes: vec![(34, false)],
            original: "g".into(),
            shown: 0,
            delimiter: None,
        });

        assert!(learning.take_selection().is_none());
        assert!(learning.take_last().is_some());
    }

    #[test]
    fn reverted_words_can_be_unmarked_by_cycling_forward() {
        let learning = Learning::default();
        learning.mark_reverted("ghbdtn");
        assert!(learning.is_reverted("ghbdtn"));
        learning.unmark_reverted("ghbdtn");
        assert!(!learning.is_reverted("ghbdtn"));
    }
}
