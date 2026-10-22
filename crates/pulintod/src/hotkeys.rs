//! Hotkeys: what they are, which set a keyboard uses, how one is recorded.
//!
//! A **binding** is either a [`Chord`] (a key pressed while an exact set of
//! modifiers is held) or a [`Tap`] (one modifier pressed and released with
//! nothing in between). Each [`Action`] owns a list of bindings; the list is
//! empty when the action is switched off.
//!
//! There are two ready-made sets, Apple and PC, and a keyboard picks its set by
//! itself: an Apple keyboard (vendor id `0x05ac`) lives by the Apple one, any
//! other by the PC one, so a laptop and an external Apple keyboard each keep
//! their own habits at the same time. The user can override the choice and edit
//! every binding of every set.
//!
//! "Bare" is the whole difficulty of a tap. Alt is a modifier before it is a
//! hotkey, so it counts only when pressed and released with nothing in between;
//! otherwise Alt+Tab, Alt+F4 and every other combination would fire a
//! conversion on the way out.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use crate::i18n::tr;
use pulinto_input::capture::APPLE_VENDOR_ID;
use tokio::sync::oneshot;

const SHIFT: [u16; 2] = [42, 54];
const CTRL: [u16; 2] = [29, 97];
const ALT: [u16; 2] = [56, 100];
const META: [u16; 2] = [125, 126];

const KEY_ESC: u16 = 1;
const KEY_PAUSE: u16 = 119;

// ── Modifiers and taps ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Meta,
}

impl Modifier {
    fn codes(self) -> [u16; 2] {
        match self {
            Modifier::Shift => SHIFT,
            Modifier::Ctrl => CTRL,
            Modifier::Alt => ALT,
            Modifier::Meta => META,
        }
    }

    pub fn of_code(code: u16) -> Option<Modifier> {
        [
            Modifier::Shift,
            Modifier::Ctrl,
            Modifier::Alt,
            Modifier::Meta,
        ]
        .into_iter()
        .find(|modifier| modifier.codes().contains(&code))
    }

    /// Whether a bare press of it has to be held back until we know it is not
    /// part of a combination. Alt opens application menus and Super opens the
    /// GNOME overview when tapped alone, so those two would reach the
    /// application before our hotkey did. Ctrl and Shift do nothing on their
    /// own, and holding them back would hide them from mouse clicks
    /// (Ctrl+click).
    pub fn withholds_press(self) -> bool {
        matches!(self, Modifier::Alt | Modifier::Meta)
    }

    fn name(self) -> &'static str {
        match self {
            Modifier::Shift => "shift",
            Modifier::Ctrl => "ctrl",
            Modifier::Alt => "alt",
            Modifier::Meta => "meta",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Modifier::Shift => "Shift",
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Meta => "Meta",
        }
    }

    fn from_name(name: &str) -> Option<Modifier> {
        match name {
            "shift" => Some(Modifier::Shift),
            "ctrl" | "control" => Some(Modifier::Ctrl),
            "alt" => Some(Modifier::Alt),
            "meta" | "super" | "win" => Some(Modifier::Meta),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Any,
    Left,
    Right,
}

/// A modifier pressed and released on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tap {
    pub modifier: Modifier,
    pub side: Side,
}

impl Tap {
    pub const ANY_ALT: Tap = Tap {
        modifier: Modifier::Alt,
        side: Side::Any,
    };
    pub const RIGHT_CTRL: Tap = Tap {
        modifier: Modifier::Ctrl,
        side: Side::Right,
    };

    pub fn matches_code(&self, code: u16) -> bool {
        let [left, right] = self.modifier.codes();
        match self.side {
            Side::Any => code == left || code == right,
            Side::Left => code == left,
            Side::Right => code == right,
        }
    }

    /// For a physical key: the tap that names exactly that key.
    fn of_exact_code(code: u16) -> Option<Tap> {
        let modifier = Modifier::of_code(code)?;
        let side = if modifier.codes()[0] == code {
            Side::Left
        } else {
            Side::Right
        };
        Some(Tap { modifier, side })
    }

    pub fn withholds_press(&self) -> bool {
        self.modifier.withholds_press()
    }

    pub fn parse(text: &str) -> Option<Tap> {
        let lower = text.trim().to_ascii_lowercase();
        let name = lower.strip_prefix("tap:")?.trim();
        if let Some(modifier) = Modifier::from_name(name) {
            return Some(Tap {
                modifier,
                side: Side::Any,
            });
        }
        let (side, rest) = match name.chars().next()? {
            'l' => (Side::Left, &name[1..]),
            'r' => (Side::Right, &name[1..]),
            _ => return None,
        };
        Modifier::from_name(rest).map(|modifier| Tap { modifier, side })
    }

    pub fn written(&self) -> String {
        let side = match self.side {
            Side::Any => "",
            Side::Left => "l",
            Side::Right => "r",
        };
        format!("tap:{side}{}", self.modifier.name())
    }

    pub fn label(&self) -> String {
        match self.side {
            Side::Any => tr("{modifier} (single press)").replace("{modifier}", self.modifier.title()),
            Side::Left => tr("Left {modifier} (single press)").replace("{modifier}", self.modifier.title()),
            Side::Right => tr("Right {modifier} (single press)").replace("{modifier}", self.modifier.title()),
        }
    }
}

// ── Chords ──────────────────────────────────────────────────────────────

/// A key pressed while an exact set of modifiers is held.
///
/// Observe mode cannot swallow the combination — it holds no grab, so the
/// application sees it too. That is an argument for a default nobody else
/// binds, not for a comfortable one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub code: u16,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

/// Keys with a name of their own in the config. Printable keys are written as
/// raw codes: a code round-trips exactly, whatever the character on the keycap.
const NAMED_KEYS: &[(&str, u16, &str)] = &[
    ("pause", KEY_PAUSE, "Pause"),
    ("scrolllock", 70, "Scroll Lock"),
    ("print", 99, "Print Screen"),
    ("menu", 127, "Menu"),
    ("space", 57, "Space"),
    ("tab", 15, "Tab"),
    ("enter", 28, "Enter"),
    ("backspace", 14, "Backspace"),
    ("esc", KEY_ESC, "Esc"),
    ("capslock", 58, "Caps Lock"),
    ("insert", 110, "Insert"),
    ("delete", 111, "Delete"),
    ("home", 102, "Home"),
    ("end", 107, "End"),
    ("pageup", 104, "Page Up"),
    ("pagedown", 109, "Page Down"),
    ("up", 103, "↑"),
    ("down", 108, "↓"),
    ("left", 105, "←"),
    ("right", 106, "→"),
];

fn function_key_code(number: u16) -> Option<u16> {
    match number {
        1..=10 => Some(58 + number),
        11 => Some(87),
        12 => Some(88),
        13..=24 => Some(170 + number),
        _ => None,
    }
}

fn function_key_number(code: u16) -> Option<u16> {
    (1..=24).find(|number| function_key_code(*number) == Some(code))
}

/// Keys that do nothing when typed, so a binding on them is safe without a
/// modifier. Every other key is typing, and a binding on it would swallow text.
fn is_free_standing(code: u16) -> bool {
    matches!(code, KEY_PAUSE | 70 | 99 | 127) || function_key_number(code).is_some()
}

impl Chord {
    pub const PAUSE: Chord = Chord {
        code: KEY_PAUSE,
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    pub const SHIFT_PAUSE: Chord = Chord {
        code: KEY_PAUSE,
        ctrl: false,
        alt: false,
        shift: true,
        meta: false,
    };

    /// `Ctrl+Alt+\` — reachable one-handed, present on every keyboard, and
    /// bound by almost nothing.
    pub const REPORT: Chord = Chord {
        code: 43,
        ctrl: true,
        alt: true,
        shift: false,
        meta: false,
    };

    /// `Ctrl+Alt+P` — toggle PuLinTo for the focused application. The decision
    /// is persisted by WM_CLASS, so the hotkey also teaches the safety policy.
    pub const TOGGLE_APP: Chord = Chord {
        code: 25,
        ctrl: true,
        alt: true,
        shift: false,
        meta: false,
    };

    /// `Ctrl+Pause` — switch the Apple arrangement of the system layout on or
    /// off. With an Apple and an ordinary keyboard on one machine the Apple
    /// arrangement is wrong for the ordinary one, so the way back has to be a
    /// key, not a trip to the settings window. The key is taken in every
    /// application, so it is one no application uses: `Ctrl+Alt+M` is a command
    /// of IDEs, and chords with Alt+Shift or Ctrl+Shift would switch the layout
    /// on the way where those switch it.
    pub const APPLE_LAYOUT: Chord = Chord {
        code: KEY_PAUSE,
        ctrl: true,
        alt: false,
        shift: false,
        meta: false,
    };

    /// `Ctrl+Alt+'` — the same on an Apple keyboard, which has no Pause.
    pub const APPLE_LAYOUT_ON_APPLE: Chord = Chord {
        code: 40,
        ctrl: true,
        alt: true,
        shift: false,
        meta: false,
    };

    /// Parse `ctrl+alt+\`, `ctrl+alt+43` or `shift+pause`. The key is a name
    /// from the table above, a raw evdev keycode, or a single character looked
    /// up on the US layout.
    pub fn parse(text: &str) -> Option<Chord> {
        let mut chord = Chord {
            code: 0,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        };
        let mut parts = text
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .peekable();
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            if last {
                chord.code = keycode_of(part)?;
                continue;
            }
            match Modifier::from_name(&part.to_ascii_lowercase())? {
                Modifier::Ctrl => chord.ctrl = true,
                Modifier::Alt => chord.alt = true,
                Modifier::Shift => chord.shift = true,
                Modifier::Meta => chord.meta = true,
            }
        }
        (chord.code != 0).then_some(chord)
    }

    /// The binding as written back to the config.
    pub fn written(&self) -> String {
        let mut out = String::new();
        for (held, name) in [
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
            (self.meta, "meta"),
        ] {
            if held {
                out.push_str(name);
                out.push('+');
            }
        }
        match key_name(self.code) {
            Some((name, _)) => out.push_str(&name),
            None => out.push_str(&self.code.to_string()),
        }
        out
    }

    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        if self.meta {
            parts.push("Meta".into());
        }
        let key = key_name(self.code)
            .map(|(_, label)| label)
            .unwrap_or_else(|| {
                pulinto_input::injector::position_of_keycode(self.code)
                    .and_then(|position| {
                        pulinto_core::layout::Layout::by_id("us")?
                            .char_at(position, pulinto_core::layout::Level::Unshifted)
                    })
                    .map(|ch| ch.to_ascii_uppercase().to_string())
                    .unwrap_or_else(|| self.code.to_string())
            });
        parts.push(key);
        parts.join("+")
    }

    /// Does this press complete the chord? `held` is what the user is holding,
    /// which the engines already track for injection.
    pub fn matches(&self, code: u16, held: &[u16]) -> bool {
        let down = |group: [u16; 2]| held.iter().any(|key| group.contains(key));
        code == self.code
            && down(CTRL) == self.ctrl
            && down(ALT) == self.alt
            && down(SHIFT) == self.shift
            && down(META) == self.meta
    }

    fn from_held(code: u16, held: &[u16]) -> Chord {
        let down = |group: [u16; 2]| held.iter().any(|key| group.contains(key));
        Chord {
            code,
            ctrl: down(CTRL),
            alt: down(ALT),
            shift: down(SHIFT),
            meta: down(META),
        }
    }
}

/// Name and label of a key that has one: the table, then F1–F24.
fn key_name(code: u16) -> Option<(String, String)> {
    if let Some((name, _, label)) = NAMED_KEYS.iter().find(|(_, key, _)| *key == code) {
        return Some(((*name).to_string(), (*label).to_string()));
    }
    function_key_number(code).map(|number| (format!("f{number}"), format!("F{number}")))
}

/// Evdev keycode for a written key: a name, a decimal code, or a character
/// located on the US layout — which is where every keyboard agrees.
fn keycode_of(text: &str) -> Option<u16> {
    let lower = text.to_ascii_lowercase();
    if let Some((_, code, _)) = NAMED_KEYS.iter().find(|(name, _, _)| *name == lower) {
        return Some(*code);
    }
    if lower == "backslash" {
        return Some(43);
    }
    if let Some(number) = lower
        .strip_prefix('f')
        .and_then(|rest| rest.parse::<u16>().ok())
    {
        return function_key_code(number);
    }
    // One character names the key that types it on the US layout: «1» is
    // the digit key (code 2), not keycode 1, which is Esc. A bare number is
    // a keycode only when it is longer than one character.
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_none() {
        let layout = pulinto_core::layout::Layout::by_id("us")?;
        let (position, _) = layout.position_of(ch)?;
        return pulinto_input::evdev::POSITION_TO_EVDEV_KEYCODE
            .get(position)
            .copied();
    }
    // A raw keycode lives in the evdev key range; anything else is a typo.
    text.parse::<u16>()
        .ok()
        .filter(|code| (1..=248).contains(code))
}

// ── Bindings ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Chord(Chord),
    Tap(Tap),
}

impl Binding {
    pub fn parse(text: &str) -> Option<Binding> {
        let text = text.trim();
        if text.to_ascii_lowercase().starts_with("tap:") {
            Tap::parse(text).map(Binding::Tap)
        } else {
            Chord::parse(text).map(Binding::Chord)
        }
    }

    pub fn written(&self) -> String {
        match self {
            Binding::Chord(chord) => chord.written(),
            Binding::Tap(tap) => tap.written(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Binding::Chord(chord) => chord.label(),
            Binding::Tap(tap) => tap.label(),
        }
    }

    /// Would one physical action trigger both? Two chords overlap only when
    /// equal; two taps overlap when they share a key (`Alt` covers both Alts).
    fn overlaps(&self, other: &Binding) -> bool {
        match (self, other) {
            (Binding::Chord(a), Binding::Chord(b)) => a == b,
            (Binding::Tap(a), Binding::Tap(b)) => {
                a.modifier == b.modifier
                    && (a.side == Side::Any || b.side == Side::Any || a.side == b.side)
            }
            _ => false,
        }
    }

    /// Rules that hold whatever else is bound.
    ///
    /// A bare printable key is refused because the daemon swallows a matching
    /// key: bound to `A`, it would eat every letter. The emergency combination
    /// Backspace+Esc+Enter cannot be bound at all — it has three keys and a
    /// binding has one, so [`Chord::parse`] rejects it by construction.
    pub fn validate(&self) -> Result<(), String> {
        let Binding::Chord(chord) = self else {
            return Ok(());
        };
        if Modifier::of_code(chord.code).is_some() {
            return Err(tr("A modifier cannot be assigned as the key of a combination. For a single press, choose the modifier's “single press”.").into());
        }
        let has_real_modifier = chord.ctrl || chord.alt || chord.meta;
        if !has_real_modifier && !is_free_standing(chord.code) {
            return Err(tr("“{key}” without Ctrl, Alt or Super is an ordinary typing key: assigning it would stop it from typing. Add a modifier or choose Pause, Scroll Lock, Print Screen, Menu or F1–F24.")
                .replace("{key}", &chord.label()));
        }
        Ok(())
    }
}

// ── Actions and sets ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Convert the word being typed — or take back the last correction.
    Word,
    /// Convert the highlighted text.
    Selection,
    /// Turn PuLinTo on or off for the focused application.
    ToggleApp,
    /// File a report about the last correction.
    Report,
    /// Switch the Apple arrangement of the system layout on or off.
    AppleLayout,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::Word,
        Action::Selection,
        Action::ToggleApp,
        Action::Report,
        Action::AppleLayout,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Action::Word => "word",
            Action::Selection => "selection",
            Action::ToggleApp => "toggle_app",
            Action::Report => "report",
            Action::AppleLayout => "apple_layout",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Action::Word => tr("Change the layout of the word"),
            Action::Selection => tr("Convert the selected text"),
            Action::ToggleApp => tr("Cycle the mode for the application"),
            Action::Report => tr("Report a wrong correction"),
            Action::AppleLayout => tr("Turn the Apple layout on or off"),
        }
    }

    pub fn parse(text: &str) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|action| action.id() == text.trim())
    }

    fn index(self) -> usize {
        self as usize
    }

    /// Word and selection conversion stand down where PuLinTo is switched off
    /// for the focused application; the other two are the way back in.
    pub fn respects_app_policy(self) -> bool {
        matches!(self, Action::Word | Action::Selection)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySet {
    bindings: [Vec<Binding>; 5],
}

impl HotkeySet {
    pub fn get(&self, action: Action) -> &[Binding] {
        &self.bindings[action.index()]
    }

    pub fn set(&mut self, action: Action, bindings: Vec<Binding>) {
        self.bindings[action.index()] = bindings;
    }

    /// The action a key press completes, by its chord.
    pub fn chord_action(&self, code: u16, held: &[u16]) -> Option<Action> {
        Action::ALL.into_iter().find(|action| {
            self.get(*action).iter().any(
                |binding| matches!(binding, Binding::Chord(chord) if chord.matches(code, held)),
            )
        })
    }

    /// The action a bare tap of this modifier key triggers.
    pub fn tap_action(&self, code: u16) -> Option<Action> {
        Action::ALL.into_iter().find(|action| {
            self.get(*action)
                .iter()
                .any(|binding| matches!(binding, Binding::Tap(tap) if tap.matches_code(code)))
        })
    }

    pub fn first_label(&self, action: Action) -> Option<String> {
        self.get(action).first().map(Binding::label)
    }

    /// The other action of this set that already owns the binding, if any.
    pub fn taken_by_other(&self, action: Action, binding: &Binding) -> Option<Action> {
        Action::ALL
            .into_iter()
            .filter(|other| *other != action)
            .find(|other| self.get(*other).iter().any(|taken| taken.overlaps(binding)))
    }

    /// Replace the bindings of one action — or refuse, with a reason in words.
    pub fn try_set(&mut self, action: Action, bindings: Vec<Binding>) -> Result<(), String> {
        for (index, binding) in bindings.iter().enumerate() {
            binding.validate()?;
            if bindings[..index]
                .iter()
                .any(|earlier| earlier.overlaps(binding))
            {
                return Err(tr("“{key}” is already in this action.").replace("{key}", &binding.label()));
            }
            if let Some(other) = self.taken_by_other(action, binding) {
                return Err(tr("“{key}” is already assigned to the action “{action}” in this set.")
                    .replace("{key}", &binding.label())
                    .replace("{action}", other.title()));
            }
        }
        self.set(action, bindings);
        Ok(())
    }
}

/// Which of the two ready-made sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Apple,
    Pc,
}

impl Scheme {
    pub const ALL: [Scheme; 2] = [Scheme::Apple, Scheme::Pc];

    pub fn id(self) -> &'static str {
        match self {
            Scheme::Apple => "apple",
            Scheme::Pc => "pc",
        }
    }

    pub fn parse(text: &str) -> Option<Scheme> {
        Scheme::ALL
            .into_iter()
            .find(|scheme| scheme.id() == text.trim())
    }

    /// The factory settings of the set.
    ///
    /// Apple keeps the bare Alt tap: that keyboard has no menu-on-Alt habit to
    /// protect. PC leaves Alt alone — GTK, Qt, LibreOffice and Firefox open
    /// their menus on a bare Alt, and the daemon holds Alt back until release,
    /// which breaks exactly that — and takes Pause/Break plus a tap of the
    /// right Ctrl, which a laptop without Pause still has.
    pub fn defaults(self) -> HotkeySet {
        let chord = Binding::Chord;
        let (word, apple_layout) = match self {
            Scheme::Apple => (
                vec![Binding::Tap(Tap::ANY_ALT), chord(Chord::PAUSE)],
                vec![chord(Chord::APPLE_LAYOUT_ON_APPLE)],
            ),
            Scheme::Pc => (
                vec![chord(Chord::PAUSE), Binding::Tap(Tap::RIGHT_CTRL)],
                vec![chord(Chord::APPLE_LAYOUT)],
            ),
        };
        HotkeySet {
            bindings: [
                word,
                vec![chord(Chord::SHIFT_PAUSE)],
                vec![chord(Chord::TOGGLE_APP)],
                vec![chord(Chord::REPORT)],
                apple_layout,
            ],
        }
    }
}

/// The user's choice of set: by keyboard, or one set for all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Choice {
    #[default]
    Auto,
    Apple,
    Pc,
}

impl Choice {
    pub fn id(self) -> &'static str {
        match self {
            Choice::Auto => "auto",
            Choice::Apple => "apple",
            Choice::Pc => "pc",
        }
    }

    pub fn parse(text: &str) -> Option<Choice> {
        [Choice::Auto, Choice::Apple, Choice::Pc]
            .into_iter()
            .find(|choice| choice.id() == text.trim())
    }

    pub fn scheme_for(self, vendor: u16) -> Scheme {
        match self {
            Choice::Apple => Scheme::Apple,
            Choice::Pc => Scheme::Pc,
            Choice::Auto if vendor == APPLE_VENDOR_ID => Scheme::Apple,
            Choice::Auto => Scheme::Pc,
        }
    }
}

/// Everything about hotkeys that the config file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyConfig {
    pub choice: Choice,
    pub apple: HotkeySet,
    pub pc: HotkeySet,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            choice: Choice::Auto,
            apple: Scheme::Apple.defaults(),
            pc: Scheme::Pc.defaults(),
        }
    }
}

impl HotkeyConfig {
    pub fn set(&self, scheme: Scheme) -> &HotkeySet {
        match scheme {
            Scheme::Apple => &self.apple,
            Scheme::Pc => &self.pc,
        }
    }

    pub fn set_mut(&mut self, scheme: Scheme) -> &mut HotkeySet {
        match scheme {
            Scheme::Apple => &mut self.apple,
            Scheme::Pc => &mut self.pc,
        }
    }

    /// Read one config line if it is about hotkeys. Returns whether it was.
    ///
    /// Lenient on purpose: a hand-edited line must never stop the daemon from
    /// starting or leave it without a hotkey. A binding that fails the rules
    /// (a bare letter, say) is dropped with a note; a line with nothing usable
    /// keeps the default. `hotkey.<set>.<action> =` with nothing after it is a
    /// deliberate "off". Conflicts between actions are settled afterwards, by
    /// [`settle`](Self::settle), once every line has been read.
    pub fn apply_line(&mut self, key: &str, value: &str) -> bool {
        if key == "hotkey_set" {
            if let Some(choice) = Choice::parse(value) {
                self.choice = choice;
            }
            return true;
        }
        // Before the editor existed: one binding, shared by both sets.
        if let Some(action) = match key {
            "report_hotkey" => Some(Action::Report),
            "toggle_hotkey" => Some(Action::ToggleApp),
            _ => None,
        } {
            if let Some(binding) = Binding::parse(value).filter(|b| b.validate().is_ok()) {
                self.apple.set(action, vec![binding]);
                self.pc.set(action, vec![binding]);
            }
            return true;
        }
        let Some(rest) = key.strip_prefix("hotkey.") else {
            return false;
        };
        let Some((scheme, action)) = rest.split_once('.') else {
            return true;
        };
        let (Some(scheme), Some(action)) = (Scheme::parse(scheme), Action::parse(action)) else {
            return true;
        };
        let items: Vec<&str> = value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .collect();
        let mut bindings = Vec::new();
        for item in &items {
            match Binding::parse(item).map(|binding| (binding, binding.validate())) {
                Some((binding, Ok(()))) => bindings.push(binding),
                Some((_, Err(reason))) => eprintln!("Hotkey «{item}» dropped: {reason}"),
                None => eprintln!("Hotkey «{item}» not parsed"),
            }
        }
        if items.is_empty() || !bindings.is_empty() {
            self.set_mut(scheme).set(action, bindings);
        }
        true
    }

    /// After reading the file: where two actions of one set ended up on the
    /// same key, the earlier action keeps it and the later one loses it.
    pub fn settle(&mut self) {
        for scheme in Scheme::ALL {
            let set = self.set_mut(scheme);
            let mut kept: Vec<Binding> = Vec::new();
            for action in Action::ALL {
                let mut mine: Vec<Binding> = Vec::new();
                for binding in set.get(action).to_vec() {
                    if kept
                        .iter()
                        .chain(mine.iter())
                        .any(|taken| taken.overlaps(&binding))
                    {
                        eprintln!(
                            "Hotkey «{}» is already taken in set «{}»; dropped for «{}»",
                            binding.label(),
                            scheme.id(),
                            action.title()
                        );
                    } else {
                        mine.push(binding);
                    }
                }
                kept.extend(mine.iter().copied());
                set.set(action, mine);
            }
        }
    }

    /// The config lines that differ from the factory settings. Only those are
    /// written, so a default improved in a later version reaches everyone who
    /// never changed it.
    pub fn to_lines(&self) -> String {
        let mut out = String::new();
        if self.choice != Choice::Auto {
            out.push_str(&format!("hotkey_set = {}\n", self.choice.id()));
        }
        for scheme in Scheme::ALL {
            let defaults = scheme.defaults();
            for action in Action::ALL {
                let current = self.set(scheme).get(action);
                if current != defaults.get(action) {
                    let value: Vec<String> = current.iter().map(Binding::written).collect();
                    out.push_str(&format!(
                        "hotkey.{}.{} = {}\n",
                        scheme.id(),
                        action.id(),
                        value.join(", ")
                    ));
                }
            }
        }
        out
    }
}

/// The live copy the input loops read on every key, and the recorder.
pub struct HotkeyStore {
    inner: RwLock<StoreInner>,
    pub recorder: Arc<Recorder>,
}

struct StoreInner {
    choice: Choice,
    apple: Arc<HotkeySet>,
    pc: Arc<HotkeySet>,
}

impl HotkeyStore {
    pub fn new(config: &HotkeyConfig) -> Self {
        Self {
            inner: RwLock::new(StoreInner {
                choice: config.choice,
                apple: Arc::new(config.apple.clone()),
                pc: Arc::new(config.pc.clone()),
            }),
            recorder: Arc::new(Recorder::default()),
        }
    }

    pub fn scheme_for(&self, vendor: u16) -> Scheme {
        self.inner.read().unwrap().choice.scheme_for(vendor)
    }

    /// The set a keyboard of this vendor lives by right now.
    pub fn set_for(&self, vendor: u16) -> Arc<HotkeySet> {
        let inner = self.inner.read().unwrap();
        match inner.choice.scheme_for(vendor) {
            Scheme::Apple => Arc::clone(&inner.apple),
            Scheme::Pc => Arc::clone(&inner.pc),
        }
    }

    /// How the tray names the per-application toggle. The tray belongs to no
    /// keyboard, so for "auto" it goes by whether an Apple keyboard is attached.
    pub fn toggle_label(&self) -> Option<String> {
        let vendor = match self.inner.read().unwrap().choice {
            Choice::Auto if pulinto_input::capture::apple_keyboard_present() => APPLE_VENDOR_ID,
            _ => 0,
        };
        self.set_for(vendor).first_label(Action::ToggleApp)
    }

    pub fn config(&self) -> HotkeyConfig {
        let inner = self.inner.read().unwrap();
        HotkeyConfig {
            choice: inner.choice,
            apple: (*inner.apple).clone(),
            pc: (*inner.pc).clone(),
        }
    }

    /// Change the live sets in one step and hand back what to write to the
    /// config file. Nothing changes when `change` refuses.
    pub fn update<T>(
        &self,
        change: impl FnOnce(&mut HotkeyConfig) -> Result<T, String>,
    ) -> Result<(T, HotkeyConfig), String> {
        let mut config = self.config();
        let value = change(&mut config)?;
        self.replace(&config);
        Ok((value, config))
    }

    pub fn replace(&self, config: &HotkeyConfig) {
        let mut inner = self.inner.write().unwrap();
        inner.choice = config.choice;
        inner.apple = Arc::new(config.apple.clone());
        inner.pc = Arc::new(config.pc.clone());
    }
}

// ── Detecting a tap ─────────────────────────────────────────────────────

/// Tracks whether one modifier was tapped on its own.
#[derive(Debug, Default)]
pub struct TapTracker {
    down: Option<u16>,
    used: bool,
    /// Mouse clicks counted when the candidate went down.
    clicks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapRelease {
    /// The candidate was pressed and released with nothing in between.
    Tap,
    /// The candidate was part of a combination.
    Combination,
    /// Not the candidate at all.
    Other,
}

impl TapTracker {
    pub fn is_down(&self) -> bool {
        self.down.is_some()
    }

    /// Feed the press of a modifier that has a tap binding. Returns whether it
    /// became the candidate.
    ///
    /// `others_held` says whether any other modifier was already down. It has
    /// to be asked, because "nothing happened in between" is not the same as
    /// "nothing was held": pressing **Shift and then Alt** — which is how half
    /// the world switches layout — puts Shift down first, so nothing happens
    /// between Alt's press and release and the combination reads as a bare tap.
    /// The fuzzing campaign switched layout with Alt+Shift and the daemon
    /// answered with an undo on almost every switch: 55 undos in a 200-word
    /// run, each also adding the word to the "never touch again" list.
    ///
    /// `clicks` is the running count of mouse clicks (0 where it is not known):
    /// a click while the modifier is held — Ctrl+click, Alt+drag — makes it a
    /// combination although no other key was pressed.
    pub fn press(&mut self, code: u16, others_held: bool, clicks: u64) -> bool {
        if self.down.is_some() || others_held {
            self.other_key();
            return false;
        }
        self.down = Some(code);
        self.used = false;
        self.clicks = clicks;
        true
    }

    /// Any other key went down. Once something else happens while the
    /// candidate is held it is part of a combination, not a hotkey.
    pub fn other_key(&mut self) {
        if self.down.is_some() {
            self.used = true;
        }
    }

    pub fn release(&mut self, code: u16, clicks: u64) -> TapRelease {
        if self.down != Some(code) {
            return TapRelease::Other;
        }
        let tapped = !self.used && clicks == self.clicks;
        self.down = None;
        self.used = false;
        if tapped {
            TapRelease::Tap
        } else {
            TapRelease::Combination
        }
    }

    /// The candidate was consumed by a chord that swallowed the whole
    /// combination: forget it, and say which key it was so its release can be
    /// swallowed too.
    pub fn consume(&mut self) -> Option<u16> {
        self.used = false;
        self.down.take()
    }
}

// ── Recording ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Recorded(Binding),
    Cancelled,
    Timeout,
    Rejected(String),
}

type Validator = Box<dyn Fn(&Binding) -> Result<(), String> + Send>;

struct Session {
    id: u64,
    validate: Validator,
    tx: Option<oneshot::Sender<Outcome>>,
    /// Keys swallowed during the recording and not yet released.
    down: Vec<u16>,
    /// Modifiers swallowed during the recording and not yet released.
    modifiers: Vec<u16>,
    /// Something besides one lone modifier happened: no tap any more.
    used: bool,
}

/// Swallows the next combination so the daemon, not the window, can say what
/// the user pressed: the window never sees Super or a bare modifier, and under
/// Wayland the compositor takes part of the keys for itself.
///
/// The rule that keeps keys from sticking: a *press* made during the recording
/// is swallowed and so is its release; a release of a key pressed *before* the
/// recording passes through to the application that saw its press.
#[derive(Default)]
pub struct Recorder {
    active: AtomicBool,
    next_id: AtomicU64,
    session: Mutex<Option<Session>>,
}

impl Recorder {
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Start waiting for one combination. Whatever happens first — a
    /// combination, `Esc`, or the end of `timeout` — is delivered on the
    /// returned channel, and nothing is stored anywhere.
    pub fn start(
        self: &Arc<Self>,
        validate: Validator,
        timeout: Duration,
    ) -> Result<oneshot::Receiver<Outcome>, String> {
        let mut session = self.session.lock().unwrap();
        if session.is_some() {
            return Err(tr("A shortcut is already being recorded.").into());
        }
        let (tx, rx) = oneshot::channel();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        *session = Some(Session {
            id,
            validate,
            tx: Some(tx),
            down: Vec::new(),
            modifiers: Vec::new(),
            used: false,
        });
        self.active.store(true, Ordering::Relaxed);
        drop(session);

        let recorder = Arc::clone(self);
        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            recorder.finish_if(id, Outcome::Timeout);
        });
        Ok(rx)
    }

    /// End the recording now, as if `Esc` had been pressed. The window calls
    /// this when its dialog is closed, so the keyboard is the user's again at
    /// once instead of after the timeout.
    pub fn cancel(&self) {
        let mut guard = self.session.lock().unwrap();
        if guard.is_some() {
            Self::finish(&self.active, &mut guard, Outcome::Cancelled);
        }
    }

    /// The modifiers held so far in the recording, for the window to show
    /// while the combination is still being made (`Ctrl+Alt`).
    pub fn progress(&self) -> String {
        let guard = self.session.lock().unwrap();
        let Some(session) = guard.as_ref() else {
            return String::new();
        };
        [
            Modifier::Ctrl,
            Modifier::Alt,
            Modifier::Shift,
            Modifier::Meta,
        ]
        .into_iter()
        .filter(|modifier| {
            session
                .modifiers
                .iter()
                .any(|code| Modifier::of_code(*code) == Some(*modifier))
        })
        .map(Modifier::title)
        .collect::<Vec<_>>()
        .join("+")
    }

    fn finish_if(&self, id: u64, outcome: Outcome) {
        let mut guard = self.session.lock().unwrap();
        if guard.as_ref().is_some_and(|session| session.id == id) {
            Self::finish(&self.active, &mut guard, outcome);
        }
    }

    fn finish(active: &AtomicBool, guard: &mut Option<Session>, outcome: Outcome) {
        active.store(false, Ordering::Relaxed);
        if let Some(mut session) = guard.take() {
            if let Some(tx) = session.tx.take() {
                let _ = tx.send(outcome);
            }
        }
    }

    /// One key transition. Returns whether the recorder swallowed it; the
    /// caller then must not forward it, and must swallow its release if this
    /// was a press.
    ///
    /// `held` is what the caller already knows to be held, pressed before the
    /// recording: it counts as part of the combination.
    pub fn feed(&self, code: u16, pressed: bool, held: &[u16]) -> bool {
        if !self.is_active() {
            return false;
        }
        let mut guard = self.session.lock().unwrap();
        let Some(session) = guard.as_mut() else {
            return false;
        };
        let modifier = Modifier::of_code(code);

        if pressed {
            if !session.down.contains(&code) {
                session.down.push(code);
            }
            if modifier.is_some() {
                if !held.is_empty() || !session.modifiers.is_empty() {
                    session.used = true;
                }
                session.modifiers.push(code);
                return true;
            }
            let mut mods: Vec<u16> = held.to_vec();
            mods.extend(session.modifiers.iter().copied());
            let outcome = if code == KEY_ESC && mods.is_empty() {
                Outcome::Cancelled
            } else {
                let binding = Binding::Chord(Chord::from_held(code, &mods));
                match binding
                    .validate()
                    .and_then(|()| (session.validate)(&binding))
                {
                    Ok(()) => Outcome::Recorded(binding),
                    Err(reason) => Outcome::Rejected(reason),
                }
            };
            Self::finish(&self.active, &mut guard, outcome);
            return true;
        }

        let Some(position) = session.down.iter().position(|key| *key == code) else {
            return false;
        };
        session.down.remove(position);
        if modifier.is_some() {
            if !session.used && session.modifiers == [code] {
                let binding = Tap::of_exact_code(code).map(Binding::Tap);
                if let Some(binding) = binding {
                    let outcome = match (session.validate)(&binding) {
                        Ok(()) => Outcome::Recorded(binding),
                        Err(reason) => Outcome::Rejected(reason),
                    };
                    Self::finish(&self.active, &mut guard, outcome);
                }
            } else {
                session.modifiers.retain(|key| *key != code);
                if session.modifiers.is_empty() {
                    session.used = false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The texts are translated into the interface language; the tests that read them pin English.
    fn english() {
        crate::i18n::set_choice(Some(crate::i18n::Lang::En));
    }

    #[test]
    fn a_digit_names_the_digit_key_not_a_keycode() {
        assert_eq!(Chord::parse("ctrl+1").unwrap().code, 2);
        assert_eq!(Chord::parse("ctrl+2").unwrap().code, 3);
        assert_eq!(keycode_of("119"), Some(119));
        assert_eq!(keycode_of("0"), Some(11));
        assert_eq!(keycode_of("65535"), None);
        assert_eq!(keycode_of("249"), None);
    }

    fn chord(text: &str) -> Chord {
        Chord::parse(text).unwrap()
    }

    #[test]
    fn a_chord_reads_both_spellings_of_its_key() {
        assert_eq!(Chord::parse("ctrl+alt+43"), Some(Chord::REPORT));
        assert_eq!(Chord::parse("ctrl+alt+\\"), Some(Chord::REPORT));
        assert_eq!(Chord::parse("CTRL+Alt+\\"), Some(Chord::REPORT));
        assert_eq!(Chord::parse("ctrl+alt+backslash"), Some(Chord::REPORT));
    }

    #[test]
    fn named_keys_and_function_keys_are_read() {
        assert_eq!(Chord::parse("pause"), Some(Chord::PAUSE));
        assert_eq!(Chord::parse("Shift+Pause"), Some(Chord::SHIFT_PAUSE));
        assert_eq!(chord("f1").code, 59);
        assert_eq!(chord("f11").code, 87);
        assert_eq!(chord("f12").code, 88);
        assert_eq!(chord("ctrl+f13").code, 183);
        assert_eq!(Chord::parse("f25"), None);
    }

    #[test]
    fn the_application_toggle_has_a_stable_human_label() {
        assert_eq!(Chord::TOGGLE_APP, Chord::parse("ctrl+alt+p").unwrap());
        assert_eq!(Chord::TOGGLE_APP.label(), "Ctrl+Alt+P");
        assert_eq!(Chord::SHIFT_PAUSE.label(), "Shift+Pause");
        assert_eq!(chord("meta+space").label(), "Meta+Space");
    }

    /// The config is rewritten whenever anything changes, so a binding that
    /// does not survive the round trip is a binding the user loses.
    #[test]
    fn every_kind_of_binding_survives_being_written_and_read_back() {
        for text in [
            "ctrl+alt+43",
            "ctrl+alt+p",
            "shift+meta+q",
            "alt+53",
            "pause",
            "shift+pause",
            "ctrl+f5",
            "tap:alt",
            "tap:lalt",
            "tap:rctrl",
            "tap:meta",
            "tap:shift",
        ] {
            let binding = Binding::parse(text).unwrap_or_else(|| panic!("{text} does not parse"));
            assert_eq!(
                Binding::parse(&binding.written()),
                Some(binding),
                "{text} → {}",
                binding.written()
            );
        }
    }

    #[test]
    fn a_chord_without_a_key_is_not_a_chord() {
        assert_eq!(Chord::parse("ctrl+alt"), None);
        assert_eq!(Chord::parse(""), None);
        assert_eq!(Chord::parse("ctrl+alt+никогда"), None);
        assert_eq!(Binding::parse("tap:"), None);
        assert_eq!(Binding::parse("tap:capslock"), None);
    }

    /// Backspace+Esc+Enter releases the keyboard. It has three keys, a binding
    /// has one, so there is no way to write it as a binding.
    #[test]
    fn the_emergency_combination_cannot_be_a_binding() {
        assert_eq!(Binding::parse("backspace+esc+enter"), None);
        assert_eq!(Binding::parse("14+1+28"), None);
    }

    /// Exactly those modifiers and no others: `Ctrl+Alt+\` must not fire when
    /// the user is holding Shift as well, or every accidental combination would
    /// file a bug report.
    #[test]
    fn a_chord_needs_exactly_its_own_modifiers() {
        let chord = Chord::REPORT;
        assert!(chord.matches(43, &[29, 56]));
        assert!(chord.matches(43, &[97, 100])); // right Ctrl and Alt
        assert!(!chord.matches(43, &[29]));
        assert!(!chord.matches(43, &[29, 56, 42])); // extra Shift
        assert!(!chord.matches(44, &[29, 56]));
        assert!(Chord::PAUSE.matches(119, &[]));
        assert!(
            !Chord::PAUSE.matches(119, &[42]),
            "Shift+Pause is another action"
        );
        assert!(Chord::SHIFT_PAUSE.matches(119, &[54]));
    }

    #[test]
    fn a_tap_names_its_keys() {
        english();
        assert!(Tap::ANY_ALT.matches_code(56) && Tap::ANY_ALT.matches_code(100));
        assert!(Tap::RIGHT_CTRL.matches_code(97) && !Tap::RIGHT_CTRL.matches_code(29));
        assert!(!Tap::ANY_ALT.matches_code(29));
        assert_eq!(Tap::ANY_ALT.label(), "Alt (single press)");
        assert_eq!(Tap::RIGHT_CTRL.label(), "Right Ctrl (single press)");
    }

    #[test]
    fn only_alt_and_super_are_held_back() {
        assert!(Tap::ANY_ALT.withholds_press());
        assert!(Binding::parse("tap:meta")
            .map(|b| matches!(b, Binding::Tap(t) if t.withholds_press()))
            .unwrap());
        assert!(!Tap::RIGHT_CTRL.withholds_press());
    }

    // ── sets ────────────────────────────────────────────────────────────

    #[test]
    fn apple_keeps_the_alt_tap_and_pc_does_not() {
        let apple = Scheme::Apple.defaults();
        let pc = Scheme::Pc.defaults();
        assert_eq!(apple.tap_action(56), Some(Action::Word));
        assert_eq!(apple.tap_action(100), Some(Action::Word));
        assert_eq!(
            pc.tap_action(56),
            None,
            "a bare Alt belongs to the menus on a PC"
        );
        assert_eq!(pc.tap_action(100), None);
        assert_eq!(
            pc.tap_action(97),
            Some(Action::Word),
            "right Ctrl is the PC tap"
        );
        assert_eq!(apple.tap_action(97), None);
        for set in [&apple, &pc] {
            assert_eq!(set.chord_action(119, &[]), Some(Action::Word));
            assert_eq!(set.chord_action(119, &[42]), Some(Action::Selection));
            assert_eq!(set.chord_action(25, &[29, 56]), Some(Action::ToggleApp));
            assert_eq!(set.chord_action(43, &[29, 56]), Some(Action::Report));
            assert_eq!(set.chord_action(30, &[]), None);
        }
        // The Apple layout key: Ctrl+Pause on a PC keyboard, Ctrl+Alt+' on an
        // Apple one, which has no Pause.
        assert_eq!(pc.chord_action(119, &[29]), Some(Action::AppleLayout));
        assert_eq!(apple.chord_action(40, &[29, 56]), Some(Action::AppleLayout));
        assert_eq!(apple.chord_action(119, &[29]), None);
        assert_eq!(Chord::APPLE_LAYOUT_ON_APPLE.label(), "Ctrl+Alt+'");
    }

    #[test]
    fn the_default_sets_obey_their_own_rules() {
        for scheme in Scheme::ALL {
            let mut set = scheme.defaults();
            for action in Action::ALL {
                let own = set.get(action).to_vec();
                assert_eq!(set.try_set(action, own), Ok(()), "{scheme:?} {action:?}");
            }
        }
    }

    #[test]
    fn a_keyboard_picks_its_set_by_vendor_unless_told_otherwise() {
        assert_eq!(Choice::Auto.scheme_for(APPLE_VENDOR_ID), Scheme::Apple);
        assert_eq!(Choice::Auto.scheme_for(0x046d), Scheme::Pc);
        assert_eq!(Choice::Apple.scheme_for(0x046d), Scheme::Apple);
        assert_eq!(Choice::Pc.scheme_for(APPLE_VENDOR_ID), Scheme::Pc);
    }

    #[test]
    fn two_keyboards_live_by_different_sets_at_once() {
        let store = HotkeyStore::new(&HotkeyConfig::default());
        let apple = store.set_for(APPLE_VENDOR_ID);
        let pc = store.set_for(0x1234);
        assert_eq!(apple.tap_action(56), Some(Action::Word));
        assert_eq!(pc.tap_action(56), None);
    }

    #[test]
    fn the_store_follows_an_edit_without_a_restart() {
        let store = HotkeyStore::new(&HotkeyConfig::default());
        let mut config = store.config();
        config
            .pc
            .try_set(Action::Word, vec![Binding::Tap(Tap::ANY_ALT)])
            .unwrap();
        store.replace(&config);
        assert_eq!(store.set_for(0x1234).tap_action(56), Some(Action::Word));
    }

    // ── rules ───────────────────────────────────────────────────────────

    #[test]
    fn a_bare_printable_key_is_refused() {
        english();
        let mut set = Scheme::Pc.defaults();
        for text in [
            "a",
            "30",
            "space",
            "enter",
            "backspace",
            "esc",
            "left",
            "shift+a",
            "shift+space",
        ] {
            let binding = Binding::parse(text).unwrap();
            let error = set.try_set(Action::Report, vec![binding]).unwrap_err();
            assert!(error.contains("ordinary typing key"), "{text}: {error}");
        }
        assert_eq!(
            set.get(Action::Report),
            Scheme::Pc.defaults().get(Action::Report)
        );
    }

    #[test]
    fn keys_that_type_nothing_may_stand_alone() {
        let mut set = Scheme::Pc.defaults();
        // Pause and Shift+Pause are taken by the defaults; free them first.
        set.set(Action::Word, vec![]);
        set.set(Action::Selection, vec![]);
        for text in [
            "f1",
            "f12",
            "f24",
            "pause",
            "scrolllock",
            "print",
            "menu",
            "shift+f5",
        ] {
            let binding = Binding::parse(text).unwrap();
            assert_eq!(set.try_set(Action::Report, vec![binding]), Ok(()), "{text}");
        }
    }

    #[test]
    fn a_printable_key_is_fine_with_a_real_modifier() {
        let mut set = Scheme::Pc.defaults();
        for text in ["ctrl+q", "alt+space", "meta+a", "ctrl+shift+f"] {
            let binding = Binding::parse(text).unwrap();
            assert_eq!(set.try_set(Action::Report, vec![binding]), Ok(()), "{text}");
        }
    }

    #[test]
    fn a_modifier_is_not_a_chord_key() {
        english();
        let mut set = Scheme::Pc.defaults();
        let error = set
            .try_set(Action::Report, vec![Binding::parse("ctrl+56").unwrap()])
            .unwrap_err();
        assert!(error.contains("single press"), "{error}");
    }

    #[test]
    fn two_actions_of_one_set_cannot_share_a_binding() {
        english();
        let mut set = Scheme::Pc.defaults();
        let error = set
            .try_set(Action::Report, vec![Binding::Chord(Chord::TOGGLE_APP)])
            .unwrap_err();
        assert!(error.contains("Cycle the mode for the application"), "{error}");
        // The same binding in the other set is another matter.
        assert_eq!(
            Scheme::Apple
                .defaults()
                .try_set(Action::Word, vec![Binding::Chord(Chord::REPORT)]),
            Err("“Ctrl+Alt+\\” is already assigned to the action “Report a wrong correction” in this set.".to_string())
        );
        // Taps overlap through the side they share.
        let mut apple = Scheme::Apple.defaults();
        let left_alt = Binding::parse("tap:lalt").unwrap();
        assert!(apple.try_set(Action::Selection, vec![left_alt]).is_err());
        let mut pc = Scheme::Pc.defaults();
        assert!(pc
            .try_set(Action::Selection, vec![Binding::Tap(Tap::ANY_ALT)])
            .is_ok());
        assert!(pc.try_set(Action::Report, vec![left_alt]).is_err());
    }

    #[test]
    fn an_action_cannot_list_a_binding_twice() {
        english();
        let mut set = Scheme::Pc.defaults();
        let both = vec![Binding::Chord(Chord::PAUSE), Binding::Chord(Chord::PAUSE)];
        assert!(set
            .try_set(Action::Word, both)
            .unwrap_err()
            .contains("already in this action"));
    }

    #[test]
    fn an_action_can_be_switched_off() {
        let mut set = Scheme::Pc.defaults();
        set.try_set(Action::Word, vec![]).unwrap();
        assert_eq!(set.chord_action(119, &[]), None);
        assert_eq!(set.tap_action(97), None);
    }

    // ── config lines ────────────────────────────────────────────────────

    fn read(lines: &[(&str, &str)]) -> HotkeyConfig {
        let mut config = HotkeyConfig::default();
        for (key, value) in lines {
            config.apply_line(key, value);
        }
        config.settle();
        config
    }

    #[test]
    fn an_untouched_config_writes_nothing() {
        assert_eq!(HotkeyConfig::default().to_lines(), "");
    }

    #[test]
    fn only_what_differs_from_the_defaults_is_written() {
        let mut config = HotkeyConfig {
            choice: Choice::Pc,
            ..HotkeyConfig::default()
        };
        config
            .pc
            .try_set(
                Action::Word,
                vec![Binding::Tap(Tap::ANY_ALT), Binding::Chord(Chord::PAUSE)],
            )
            .unwrap();
        config.apple.try_set(Action::Selection, vec![]).unwrap();
        let text = config.to_lines();
        assert_eq!(
            text,
            "hotkey_set = pc\nhotkey.apple.selection = \nhotkey.pc.word = tap:alt, pause\n"
        );
        let mut again = HotkeyConfig::default();
        for line in text.lines() {
            let (key, value) = line.split_once('=').unwrap();
            assert!(again.apply_line(key.trim(), value.trim()));
        }
        again.settle();
        assert_eq!(again, config);
    }

    /// The format before the editor: one binding per action, for every keyboard.
    #[test]
    fn the_old_config_keys_set_both_sets() {
        let config = read(&[
            ("report_hotkey", "ctrl+alt+r"),
            ("toggle_hotkey", "ctrl+shift+meta+t"),
        ]);
        for set in [&config.apple, &config.pc] {
            assert_eq!(
                set.get(Action::Report),
                [Binding::Chord(chord("ctrl+alt+r"))]
            );
            assert_eq!(
                set.get(Action::ToggleApp),
                [Binding::Chord(chord("ctrl+shift+meta+t"))]
            );
        }
        // The rest is as it was shipped.
        assert_eq!(
            config.pc.get(Action::Word),
            Scheme::Pc.defaults().get(Action::Word)
        );
        assert_eq!(
            config.apple.get(Action::Word),
            Scheme::Apple.defaults().get(Action::Word)
        );
    }

    #[test]
    fn the_old_default_report_key_reads_back_as_the_default() {
        let config = read(&[
            ("report_hotkey", "ctrl+alt+43"),
            ("toggle_hotkey", "ctrl+alt+25"),
        ]);
        assert_eq!(config, HotkeyConfig::default());
        assert_eq!(config.to_lines(), "");
    }

    #[test]
    fn a_broken_line_keeps_the_default() {
        let config = read(&[
            ("hotkey.pc.report", "ctrl+alt+никогда"),
            ("hotkey.pc.nonsense", "ctrl+q"),
            ("hotkey.nobody.report", "ctrl+q"),
            ("report_hotkey", "???"),
            ("hotkey_set", "somehow"),
        ]);
        assert_eq!(config, HotkeyConfig::default());
    }

    /// A hand-edited bare letter would swallow that letter everywhere.
    #[test]
    fn a_bare_letter_in_the_file_is_dropped() {
        let config = read(&[
            ("hotkey.pc.report", "a"),
            ("hotkey.pc.selection", "a, ctrl+q"),
        ]);
        assert_eq!(
            config.pc.get(Action::Report),
            Scheme::Pc.defaults().get(Action::Report)
        );
        assert_eq!(
            config.pc.get(Action::Selection),
            [Binding::Chord(chord("ctrl+q"))]
        );
    }

    #[test]
    fn an_empty_value_switches_the_action_off() {
        let config = read(&[("hotkey.pc.word", "")]);
        assert!(config.pc.get(Action::Word).is_empty());
        assert!(!config.apple.get(Action::Word).is_empty());
    }

    #[test]
    fn a_swap_between_two_actions_is_read_whole() {
        let config = read(&[
            ("hotkey.pc.report", "ctrl+alt+p"),
            ("hotkey.pc.toggle_app", "ctrl+alt+backslash"),
        ]);
        assert_eq!(
            config.pc.get(Action::Report),
            [Binding::Chord(Chord::TOGGLE_APP)]
        );
        assert_eq!(
            config.pc.get(Action::ToggleApp),
            [Binding::Chord(Chord::REPORT)]
        );
    }

    #[test]
    fn a_clash_in_the_file_costs_the_later_action_its_binding() {
        let config = read(&[("hotkey.pc.report", "ctrl+alt+p")]);
        assert_eq!(
            config.pc.get(Action::ToggleApp),
            [Binding::Chord(Chord::TOGGLE_APP)]
        );
        assert!(config.pc.get(Action::Report).is_empty());
    }

    // ── taps ────────────────────────────────────────────────────────────

    #[test]
    fn a_bare_tap_fires_on_release() {
        let mut tap = TapTracker::default();
        assert!(tap.press(56, false, 0));
        assert_eq!(tap.release(56, 0), TapRelease::Tap);
        assert!(!tap.is_down());
    }

    /// The case that makes this a state machine rather than a boolean: losing
    /// it would fire a conversion every time the user switched windows.
    #[test]
    fn alt_tab_is_not_a_hotkey() {
        let mut tap = TapTracker::default();
        tap.press(56, false, 0);
        tap.other_key(); // Tab
        assert_eq!(tap.release(56, 0), TapRelease::Combination);
    }

    /// Alt+Shift is the layout-switch hotkey on the development machine. If a
    /// modifier did not count as "something else happened", every layout
    /// switch would end with a conversion nobody asked for.
    #[test]
    fn alt_shift_is_not_a_hotkey_either() {
        let mut tap = TapTracker::default();
        tap.press(56, false, 0);
        assert!(
            !tap.press(42, true, 0),
            "Shift pressed while Alt is the candidate"
        );
        assert_eq!(tap.release(56, 0), TapRelease::Combination);
    }

    /// Shift held *before* Alt — the layout-switch hotkey as half the world
    /// presses it. Nothing happens between Alt's press and release, so only
    /// the state of the other modifiers can tell this apart from a bare tap.
    #[test]
    fn shift_held_first_then_alt_is_not_a_hotkey() {
        let mut tap = TapTracker::default();
        assert!(!tap.press(56, true, 0));
        assert_eq!(tap.release(56, 0), TapRelease::Other);
    }

    #[test]
    fn the_second_alt_does_not_replace_the_first() {
        let mut tap = TapTracker::default();
        assert!(tap.press(56, false, 0));
        assert!(!tap.press(100, true, 0));
        assert!(tap.is_down());
        assert_eq!(tap.release(100, 0), TapRelease::Other);
        assert_eq!(tap.release(56, 0), TapRelease::Combination);
    }

    #[test]
    fn keys_pressed_outside_a_tap_do_not_arm_anything() {
        let mut tap = TapTracker::default();
        tap.other_key();
        tap.other_key();
        assert!(tap.press(97, false, 0));
        assert_eq!(tap.release(97, 0), TapRelease::Tap);
    }

    #[test]
    fn a_release_without_a_press_is_not_a_tap() {
        // Happens for real: Alt held while the daemon starts, or the press
        // consumed by whoever had the keyboard before us.
        let mut tap = TapTracker::default();
        assert_eq!(tap.release(56, 0), TapRelease::Other);
    }

    /// Ctrl+click and Alt+drag press nothing on the keyboard, so only the mouse
    /// can tell them from a tap.
    #[test]
    fn a_click_while_the_modifier_is_held_is_not_a_tap() {
        let mut tap = TapTracker::default();
        assert!(tap.press(97, false, 7));
        assert_eq!(tap.release(97, 8), TapRelease::Combination);
        assert!(tap.press(97, false, 8));
        assert_eq!(tap.release(97, 8), TapRelease::Tap, "no click, still a tap");
    }

    #[test]
    fn a_chord_that_swallows_the_candidate_takes_its_release_too() {
        let mut tap = TapTracker::default();
        tap.press(56, false, 0);
        assert_eq!(tap.consume(), Some(56));
        assert!(!tap.is_down());
        assert_eq!(tap.consume(), None);
    }

    // ── recording ───────────────────────────────────────────────────────

    fn open(recorder: &Arc<Recorder>) -> oneshot::Receiver<Outcome> {
        recorder
            .start(Box::new(|_| Ok(())), Duration::from_secs(60))
            .unwrap()
    }

    fn recorded(mut rx: oneshot::Receiver<Outcome>) -> Outcome {
        rx.try_recv().expect("the recording did not finish")
    }

    #[test]
    fn a_chord_is_recorded_with_its_modifiers() {
        let recorder = Arc::new(Recorder::default());
        let rx = open(&recorder);
        assert!(recorder.feed(29, true, &[])); // Ctrl
        assert!(recorder.feed(56, true, &[])); // Alt
        assert!(recorder.feed(25, true, &[])); // P
        assert!(!recorder.is_active());
        assert_eq!(
            recorded(rx),
            Outcome::Recorded(Binding::Chord(Chord::TOGGLE_APP))
        );
        // The keys that were swallowed still come up, and are swallowed by the
        // caller's own list; the recorder no longer takes part.
        assert!(!recorder.feed(25, false, &[]));
    }

    #[test]
    fn a_lone_modifier_is_recorded_as_a_tap_of_that_key() {
        let recorder = Arc::new(Recorder::default());
        let rx = open(&recorder);
        assert!(recorder.feed(97, true, &[]));
        assert!(recorder.is_active(), "waits for the release");
        assert!(recorder.feed(97, false, &[]));
        assert_eq!(
            recorded(rx),
            Outcome::Recorded(Binding::Tap(Tap::RIGHT_CTRL))
        );
    }

    #[test]
    fn two_modifiers_without_a_key_are_not_a_tap() {
        let recorder = Arc::new(Recorder::default());
        let rx = open(&recorder);
        for (code, pressed) in [(29, true), (42, true), (42, false), (29, false)] {
            assert!(recorder.feed(code, pressed, &[]));
        }
        assert!(recorder.is_active(), "still waiting");
        // And a clean tap afterwards still counts.
        recorder.feed(56, true, &[]);
        recorder.feed(56, false, &[]);
        assert_eq!(
            recorded(rx),
            Outcome::Recorded(Binding::Tap(Tap {
                modifier: Modifier::Alt,
                side: Side::Left
            }))
        );
    }

    #[test]
    fn escape_cancels_and_is_not_recorded() {
        let recorder = Arc::new(Recorder::default());
        let rx = open(&recorder);
        assert!(recorder.feed(KEY_ESC, true, &[]));
        assert_eq!(recorded(rx), Outcome::Cancelled);
    }

    #[test]
    fn escape_with_a_modifier_is_an_ordinary_chord() {
        let recorder = Arc::new(Recorder::default());
        let rx = open(&recorder);
        recorder.feed(29, true, &[]);
        recorder.feed(KEY_ESC, true, &[]);
        assert_eq!(
            recorded(rx),
            Outcome::Recorded(Binding::Chord(chord("ctrl+esc")))
        );
    }

    #[test]
    fn a_forbidden_combination_is_rejected_in_words() {
        english();
        let recorder = Arc::new(Recorder::default());
        let rx = open(&recorder);
        assert!(recorder.feed(30, true, &[])); // bare A
        match recorded(rx) {
            Outcome::Rejected(reason) => {
                assert!(reason.contains("ordinary typing key"), "{reason}")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_caller_can_reject_too() {
        let recorder = Arc::new(Recorder::default());
        let mut rx = recorder
            .start(Box::new(|_| Err("занято".into())), Duration::from_secs(60))
            .unwrap();
        recorder.feed(29, true, &[]);
        recorder.feed(25, true, &[]);
        assert_eq!(rx.try_recv().unwrap(), Outcome::Rejected("занято".into()));
    }

    /// The release of a key pressed *before* the recording belongs to the
    /// application that saw its press; swallowing it would leave the key down.
    #[test]
    fn a_key_held_from_before_is_not_swallowed_on_release() {
        let recorder = Arc::new(Recorder::default());
        let _rx = open(&recorder);
        assert!(!recorder.feed(29, false, &[29]), "Ctrl came up: not ours");
        assert!(recorder.is_active());
    }

    #[test]
    fn a_modifier_held_from_before_counts_toward_the_combination() {
        let recorder = Arc::new(Recorder::default());
        let rx = open(&recorder);
        assert!(recorder.feed(25, true, &[29, 56]));
        assert_eq!(
            recorded(rx),
            Outcome::Recorded(Binding::Chord(Chord::TOGGLE_APP))
        );
    }

    #[test]
    fn a_second_recording_cannot_start_over_the_first() {
        let recorder = Arc::new(Recorder::default());
        let _rx = open(&recorder);
        assert!(recorder
            .start(Box::new(|_| Ok(())), Duration::from_secs(60))
            .is_err());
    }

    #[test]
    fn the_end_of_time_cancels_the_recording() {
        let recorder = Arc::new(Recorder::default());
        let mut rx = recorder
            .start(Box::new(|_| Ok(())), Duration::from_millis(30))
            .unwrap();
        assert!(recorder.is_active());
        std::thread::sleep(Duration::from_millis(200));
        assert!(!recorder.is_active());
        assert_eq!(rx.try_recv().unwrap(), Outcome::Timeout);
        // Keys are the application's again.
        assert!(!recorder.feed(30, true, &[]));
    }

    #[test]
    fn the_window_can_follow_and_cancel_a_recording() {
        let recorder = Arc::new(Recorder::default());
        assert_eq!(recorder.progress(), "");
        let rx = open(&recorder);
        assert_eq!(recorder.progress(), "");
        recorder.feed(56, true, &[]);
        recorder.feed(29, true, &[]);
        assert_eq!(recorder.progress(), "Ctrl+Alt");
        recorder.cancel();
        assert!(!recorder.is_active());
        assert_eq!(recorded(rx), Outcome::Cancelled);
        assert_eq!(recorder.progress(), "");
        recorder.cancel(); // nothing to cancel: harmless
    }

    #[test]
    fn an_idle_recorder_swallows_nothing() {
        let recorder = Recorder::default();
        assert!(!recorder.feed(30, true, &[]));
        assert!(!recorder.feed(30, false, &[]));
    }
}
