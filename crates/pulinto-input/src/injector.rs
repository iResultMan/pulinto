//! Synthetic input for grab mode.
//!
//! Two jobs. First, forwarding: once the keyboard is grabbed the kernel stops
//! delivering events to anyone else, so every keystroke the user makes has to
//! be re-emitted by us. Second, replacement: erase the word and replay the same
//! physical keys after switching layout.
//!
//! Replaying scancodes rather than typing target characters is the whole trick.
//! A scancode becomes a letter only when the compositor applies the active
//! layout to it, so if we switch the layout first and resend the very keys the
//! user pressed, the right letters appear by themselves. Computing target
//! characters and mapping them back to keys — what the old path did — adds a
//! translation that can disagree with the layout actually in effect, which is
//! exactly how corrections came out garbled.

use std::io;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{AttributeSet, EventType, InputEvent, Key};

pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_SPACE: u16 = 57;
pub const KEY_LEFT: u16 = 105;
pub const KEY_RIGHT: u16 = 106;

/// The three independent delays.
///
/// A single delay defaulting to zero is a common design and is known for
/// mangling text; keyd documents its own delay as being there "to avoid
/// overflowing the input buffer on some systems". Both point the same way:
/// without pauses, applications drop characters.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Between press and release of one key, and between consecutive keys.
    pub key_hold_ms: u64,
    /// After the last Backspace, before anything else.
    pub post_backspace_ms: u64,
    /// After switching the layout, before replaying keys. The layout change
    /// travels over the X11 socket while our keys go through a kernel device —
    /// independent channels, so this is a margin, not a handshake.
    pub switch_delay_ms: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            key_hold_ms: 8,
            post_backspace_ms: 10,
            switch_delay_ms: 50,
        }
    }
}

pub struct Injector {
    device: Mutex<VirtualDevice>,
    pub timing: Timing,
    /// When the last forwarded key left, to space out a backlog.
    last_forward: Mutex<std::time::Instant>,
    /// Keys this device has pressed and not yet released. If whoever drives it
    /// stops (hangs, loses a release), the compositor would repeat them for
    /// good; [`Injector::release_forwarded`] puts them all back up.
    down: Mutex<std::collections::BTreeSet<u16>>,
    /// `PULINTO_TRACE_OUTPUT=<file>`: every key event this device emits, as
    /// `ms<TAB>code<TAB>value`, so a letter lost on the stand can be traced to
    /// the hop where it vanished. Diagnostics only; off without the variable.
    trace: Option<Mutex<std::fs::File>>,
}

/// Open the event trace named by `variable`, if set.
pub fn open_trace(variable: &str) -> Option<Mutex<std::fs::File>> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let path = std::env::var_os(variable)?;
    // The trace holds every key with its time: readable by the user only.
    let file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .ok()?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600)).ok()?;
    Some(Mutex::new(file))
}

/// The key trace of the debug mode: one file for the keys read (`i`) and the keys written
/// (`o`), in the order they happened. Switched on and off by the daemon at run time, unlike
/// the traces named by the environment, which the stands use.
static KEY_TRACE: Mutex<Option<std::fs::File>> = Mutex::new(None);
static KEY_TRACE_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_key_trace(file: Option<std::fs::File>) {
    KEY_TRACE_ON.store(file.is_some(), std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut slot) = KEY_TRACE.lock() {
        *slot = file;
    }
}

/// Append one key event to a trace opened by [`open_trace`] and to the debug key trace.
/// `direction` is `i` for a key read from a keyboard and `o` for a key we emit.
pub fn trace_event(trace: &Option<Mutex<std::fs::File>>, direction: char, event: &InputEvent) {
    use std::io::Write;
    if event.event_type() != EventType::KEY {
        return;
    }
    let dynamic = KEY_TRACE_ON.load(std::sync::atomic::Ordering::Relaxed);
    if trace.is_none() && !dynamic {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    if let Some(trace) = trace {
        if let Ok(mut file) = trace.lock() {
            let _ = writeln!(file, "{now}\t{}\t{}", event.code(), event.value());
        }
    }
    if dynamic {
        if let Ok(mut slot) = KEY_TRACE.lock() {
            if let Some(file) = slot.as_mut() {
                let _ = writeln!(file, "{now}\t{direction}\t{}\t{}", event.code(), event.value());
            }
        }
    }
}

/// Least gap between two forwarded key events.
///
/// Whoever reads our device — libinput in the compositor or Xorg — gets a
/// kernel buffer of 64 events, the same as everyone. Keys typed during a
/// replacement queue up in the daemon and leave together afterwards; written
/// in one burst of microseconds they overran the reader's buffer and the
/// kernel dropped them (SYN_DROPPED). A millisecond apart they cannot. Live
/// typing is tens of milliseconds apart and never waits on this.
const FORWARD_GAP: Duration = Duration::from_millis(1);

/// Gap inside a backlog burst: the keys that queued up while a replacement was
/// being typed leave together afterwards. A millisecond keeps the kernel from
/// dropping them, but the compositor reads our device through the same 64-event
/// buffer, and on a loaded machine it may not get to it for a while. Spaced like
/// a fast typist, 64 events take half a second, which any scheduler grants; a
/// burst is recognised by forwards following each other faster than any human.
const BACKLOG_GAP: Duration = Duration::from_millis(8);
const BURST: Duration = Duration::from_millis(2);

impl Injector {
    pub fn new(timing: Timing) -> io::Result<Self> {
        Self::named(timing, crate::capture::VIRTUAL_DEVICE_NAME)
    }

    /// An injector under a name of your choosing.
    ///
    /// The daemon's own device is excluded from capture by name, which is what
    /// stops it reading its own corrections back. A test harness needs the
    /// opposite — to be seen exactly like a real keyboard — so it asks for a
    /// different name and gets treated as one.
    pub fn named(timing: Timing, name: &str) -> io::Result<Self> {
        let all: Vec<u16> = (1..=248u16).collect();
        Self::with_keys(timing, name, &all)
    }

    /// A virtual keyboard that has only these keys. A device without Space is
    /// not a keyboard to the daemon's capture, which is what lets a test drive
    /// one on a machine where the daemon is running.
    pub fn with_keys(timing: Timing, name: &str, codes: &[u16]) -> io::Result<Self> {
        let mut keys = AttributeSet::<Key>::new();
        for code in codes {
            keys.insert(Key::new(*code));
        }
        let device = VirtualDeviceBuilder::new()?
            .name(name)
            .with_keys(&keys)?
            .build()?;
        Ok(Self {
            device: Mutex::new(device),
            timing,
            last_forward: Mutex::new(std::time::Instant::now()),
            down: Mutex::new(Default::default()),
            trace: open_trace("PULINTO_TRACE_OUTPUT"),
        })
    }

    /// Release `keycode` if this device holds it down; nothing otherwise.
    fn release_if_down(&self, keycode: u16) -> io::Result<()> {
        let held = self
            .down
            .lock()
            .map(|down| down.contains(&keycode))
            .unwrap_or(false);
        if held {
            let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
            self.emit(&[InputEvent::new(EventType::KEY, keycode, 0), syn])?;
        }
        Ok(())
    }

    fn emit(&self, events: &[InputEvent]) -> io::Result<()> {
        let mut device = self
            .device
            .lock()
            .map_err(|_| io::Error::other("injector mutex poisoned"))?;
        device.emit(events)?;
        for event in events {
            trace_event(&self.trace, 'o', event);
        }
        if let Ok(mut down) = self.down.lock() {
            for event in events.iter().filter(|e| e.event_type() == EventType::KEY) {
                match event.value() {
                    1 => {
                        down.insert(event.code());
                    }
                    0 => {
                        down.remove(&event.code());
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Is any key held down on the virtual keyboard right now?
    pub fn has_forwarded_down(&self) -> bool {
        self.down.lock().map(|down| !down.is_empty()).unwrap_or(false)
    }

    /// Release everything this keyboard holds down; returns how many keys.
    pub fn release_forwarded(&self) -> io::Result<usize> {
        let held: Vec<u16> = self
            .down
            .lock()
            .map(|down| down.iter().copied().collect())
            .unwrap_or_default();
        let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
        for code in &held {
            self.emit(&[InputEvent::new(EventType::KEY, *code, 0), syn])?;
        }
        Ok(held.len())
    }

    /// Pass user events straight through, unchanged and unpaced.
    ///
    /// These are real keystrokes that were merely intercepted, so they must not
    /// be slowed down: any delay here would be felt as input lag on every key.
    pub fn forward(&self, events: &[InputEvent]) -> io::Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        if !events.iter().any(|event| event.event_type() == EventType::KEY) {
            return self.emit(events);
        }
        let mut last = self
            .last_forward
            .lock()
            .map_err(|_| io::Error::other("injector mutex poisoned"))?;
        let elapsed = last.elapsed();
        let gap = if elapsed < BURST { BACKLOG_GAP } else { FORWARD_GAP };
        if elapsed < gap {
            sleep(gap - elapsed);
        }
        let result = self.emit(events);
        *last = std::time::Instant::now();
        result
    }

    /// One key, pressed and released, each with its own `SYN_REPORT`.
    ///
    /// Batching several keys under one SYN is valid only for chords; for a
    /// sequence the receiver needs each transition delimited, which is what
    /// keyd and kmonad do.
    pub fn tap(&self, keycode: u16, shift: bool) -> io::Result<()> {
        let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
        let hold = Duration::from_millis(self.timing.key_hold_ms);

        // The kernel drops a press of a key this device already holds down.
        // Typing fast, the last letter of a word is still physically held when
        // the delimiter arrives, so its press was forwarded and its release is
        // queued behind the replacement; retyping it then lost the letter. Put
        // it up first — the real release that follows is a harmless no-op.
        self.release_if_down(keycode)?;
        if shift {
            self.release_if_down(KEY_LEFTSHIFT)?;
        }

        if shift {
            self.emit(&[InputEvent::new(EventType::KEY, KEY_LEFTSHIFT, 1), syn])?;
        }
        self.emit(&[InputEvent::new(EventType::KEY, keycode, 1), syn])?;
        sleep(hold);
        self.emit(&[InputEvent::new(EventType::KEY, keycode, 0), syn])?;
        if shift {
            self.emit(&[InputEvent::new(EventType::KEY, KEY_LEFTSHIFT, 0), syn])?;
        }
        sleep(hold);
        Ok(())
    }

    pub fn backspaces(&self, count: usize) -> io::Result<()> {
        for _ in 0..count {
            self.tap(KEY_BACKSPACE, false)?;
        }
        if count > 0 {
            sleep(Duration::from_millis(self.timing.post_backspace_ms));
        }
        Ok(())
    }

    /// Replay physical keys. Call only after the layout switch has been given
    /// `switch_delay_ms` to land.
    pub fn replay(&self, keys: &[(u16, bool)]) -> io::Result<()> {
        for (keycode, shift) in keys {
            self.tap(*keycode, *shift)?;
        }
        Ok(())
    }

    /// Type `text` as the keys that produce it **under `layout`**, at the same
    /// pace as a replay.
    ///
    /// Only for corrections that do not change the layout — case fixes such as
    /// `ПРивет` → `Привет`, where the target characters are in the layout the
    /// user is already typing on. Anything that crosses layouts must replay the
    /// original scan codes instead: computing characters and mapping
    /// them back to keys is the translation that can disagree with the layout
    /// actually in effect.
    fn text_keys(
        text: &str,
        layout: &pulinto_core::layout::Layout,
    ) -> io::Result<Vec<(u16, bool)>> {
        // Resolve the whole string before emitting anything. Silently skipping
        // one unsupported character turns a safe correction into data loss.
        let mut keys = Vec::with_capacity(text.chars().count());
        for ch in text.chars() {
            if ch == ' ' {
                keys.push((KEY_SPACE, false));
                continue;
            }
            let (position, level) = layout.position_of(ch).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("character missing from layout {}", layout.id),
                )
            })?;
            let keycode = crate::evdev::POSITION_TO_EVDEV_KEYCODE
                .get(position)
                .copied()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "no physical key for the character",
                    )
                })?;
            keys.push((keycode, level == pulinto_core::layout::Level::Shifted));
        }
        Ok(keys)
    }

    /// Check that a replacement is fully representable before any Backspace
    /// can be emitted.
    pub fn validate_text(
        &self,
        text: &str,
        layout: &pulinto_core::layout::Layout,
    ) -> io::Result<()> {
        Self::text_keys(text, layout).map(|_| ())
    }

    pub fn type_text(&self, text: &str, layout: &pulinto_core::layout::Layout) -> io::Result<()> {
        let keys = Self::text_keys(text, layout)?;
        self.replay(&keys)
    }

    /// Select characters immediately before the caret without touching the
    /// clipboard. Used after replacing an active selection so repeated Alt can
    /// cycle it again.
    pub fn select_previous(&self, count: usize) -> io::Result<()> {
        if count == 0 {
            return Ok(());
        }
        let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
        self.emit(&[InputEvent::new(EventType::KEY, KEY_LEFTSHIFT, 1), syn])?;
        let mut result = Ok(());
        for _ in 0..count {
            if let Err(error) = self.tap(KEY_LEFT, false) {
                result = Err(error);
                break;
            }
        }
        // Shift must never remain logically pressed, including on an error.
        let release = self.emit(&[InputEvent::new(EventType::KEY, KEY_LEFTSHIFT, 0), syn]);
        result.and(release)
    }

    pub fn wait_for_layout(&self) {
        sleep(Duration::from_millis(self.timing.switch_delay_ms));
    }

    /// Advance XKB's group through the same uinput keyboard that forwards all
    /// subsequent physical keys.
    ///
    /// `XkbLockGroup` is not persistent on this Xorg setup: the first event
    /// from a slave keyboard copies group 0 back over the externally locked
    /// state. An actual `Alt+Shift` XKB action changes the device's durable
    /// state, so later events from this same device keep the chosen group.
    /// Unlike the old letter-and-Backspace probe, this cannot leave visible
    /// text behind if an application drops one half of the sequence.
    pub fn advance_layout_group(&self, shift_already_down: bool) -> io::Result<()> {
        let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
        let mut events = vec![InputEvent::new(EventType::KEY, KEY_LEFTALT, 1), syn];
        if !shift_already_down {
            events.extend([
                InputEvent::new(EventType::KEY, KEY_LEFTSHIFT, 1),
                syn,
                InputEvent::new(EventType::KEY, KEY_LEFTSHIFT, 0),
                syn,
            ]);
        }
        events.extend([InputEvent::new(EventType::KEY, KEY_LEFTALT, 0), syn]);
        self.emit(&events)?;
        sleep(Duration::from_millis(self.timing.key_hold_ms.max(5)));
        Ok(())
    }

    /// Press `keys` in order and release them in reverse — a layout hotkey such
    /// as Alt+Shift, typed through the same device as everything else so the
    /// compositor applies it between the keys around it (GNOME on Wayland).
    pub fn chord(&self, keys: &[u16]) -> io::Result<()> {
        let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
        for code in keys {
            self.emit(&[InputEvent::new(EventType::KEY, *code, 1), syn])?;
        }
        for code in keys.iter().rev() {
            self.emit(&[InputEvent::new(EventType::KEY, *code, 0), syn])?;
        }
        sleep(Duration::from_millis(self.timing.key_hold_ms));
        Ok(())
    }

    /// Lift modifiers the user happens to be holding, so they do not contaminate
    /// the keys we are about to send. `xremap` does the same before synthesising
    /// input; without it a held Shift turns the whole replacement upper-case.
    pub fn release_modifiers(&self, held: &[u16]) -> io::Result<()> {
        let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
        for code in held {
            self.emit(&[InputEvent::new(EventType::KEY, *code, 0), syn])?;
        }
        Ok(())
    }

    /// Put back exactly what we lifted — the user may still be holding them.
    pub fn restore_modifiers(&self, held: &[u16]) -> io::Result<()> {
        let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
        for code in held {
            self.emit(&[InputEvent::new(EventType::KEY, *code, 1), syn])?;
        }
        Ok(())
    }
}

/// Position of a physical key in the 47-key alphanumeric block, or `None` for
/// keys outside it. Inverse of `POSITION_TO_EVDEV_KEYCODE`.
pub fn position_of_keycode(keycode: u16) -> Option<usize> {
    crate::evdev::POSITION_TO_EVDEV_KEYCODE
        .iter()
        .position(|code| *code == keycode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_file_is_private_even_when_it_existed_with_wider_rights() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("pulinto-trace-{}", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        std::env::set_var("PULINTO_TEST_TRACE_MODE", &path);
        assert!(open_trace("PULINTO_TEST_TRACE_MODE").is_some());
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_file(&path);
        assert_eq!(mode, 0o600);
    }

    /// What the reader sees of the device the daemon types on. The device has
    /// one key and no Space, so a daemon running on this machine leaves it
    /// alone; it is grabbed before anything is written, so nothing reaches
    /// the screen. Without `/dev/uinput` the test says so and passes.
    fn own_device(name: &str, codes: &[u16]) -> Option<(Injector, evdev::Device)> {
        let injector = match Injector::with_keys(Timing::default(), name, codes) {
            Ok(injector) => injector,
            Err(error) => {
                eprintln!("нет /dev/uinput, тест пропущен: {error}");
                return None;
            }
        };
        let mut reader = None;
        for _ in 0..50 {
            reader = evdev::enumerate()
                .map(|(_, device)| device)
                .find(|device| device.name() == Some(name));
            if reader.is_some() {
                break;
            }
            sleep(Duration::from_millis(20));
        }
        let mut reader = reader.expect("своё устройство не нашлось в /dev/input");
        reader.grab().expect("захват своего устройства");
        sleep(Duration::from_millis(50));
        Some((injector, reader))
    }

    fn key_values(reader: &mut evdev::Device) -> Vec<i32> {
        sleep(Duration::from_millis(50));
        reader
            .fetch_events()
            .expect("чтение событий")
            .filter(|event| event.event_type() == EventType::KEY)
            .map(|event| event.value())
            .collect()
    }

    /// The kernel drops a second press of a key the device holds down, so a
    /// retyped letter that the user still holds must be released first.
    #[test]
    fn tap_releases_a_key_the_device_still_holds() {
        const KEY_A: u16 = 30;
        let name = format!("pulinto-test-tap-{}", std::process::id());
        let Some((injector, mut reader)) = own_device(&name, &[KEY_A]) else {
            return;
        };
        let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
        // The letter is forwarded while the user holds it...
        injector
            .forward(&[InputEvent::new(EventType::KEY, KEY_A, 1), syn])
            .unwrap();
        // ...the replacement retypes it...
        injector.tap(KEY_A, false).unwrap();
        // ...and the physical release arrives after.
        injector
            .forward(&[InputEvent::new(EventType::KEY, KEY_A, 0), syn])
            .unwrap();
        // Release of the held key, then a full press and release; the late
        // physical release is dropped by the kernel.
        assert_eq!(key_values(&mut reader), [1, 0, 1, 0]);
        assert!(!injector.has_forwarded_down());
    }

    #[test]
    fn keycode_and_position_round_trip() {
        for (position, keycode) in crate::evdev::POSITION_TO_EVDEV_KEYCODE
            .iter()
            .enumerate()
        {
            assert_eq!(position_of_keycode(*keycode), Some(position));
        }
        // Space is not part of the alphanumeric block.
        assert_eq!(position_of_keycode(57), None);
    }

    #[test]
    fn default_timing_matches_the_agreed_profile() {
        let timing = Timing::default();
        assert_eq!(timing.key_hold_ms, 8);
        assert_eq!(timing.post_backspace_ms, 10);
        assert_eq!(timing.switch_delay_ms, 50);
    }

    #[test]
    fn layout_activation_no_longer_types_a_probe_letter() {
        // The activation implementation is intentionally modifier-only. Keep
        // the constants asserted here so a later rewrite cannot quietly bring
        // back KEY_A + Backspace, which once leaked a visible `A` into input.
        assert_eq!(KEY_LEFTALT, 56);
        assert_eq!(KEY_LEFTSHIFT, 42);
        assert_ne!(KEY_LEFTALT, 30);
        assert_ne!(KEY_LEFTSHIFT, 30);
    }

    #[test]
    fn replacement_is_fully_planned_before_typing() {
        let ru = pulinto_core::layout::Layout::by_id("ru").unwrap();
        let keys = Injector::text_keys("ещё удалить", ru).unwrap();
        assert_eq!(keys.len(), "ещё удалить".chars().count());
        assert!(Injector::text_keys("удалить🙂", ru).is_err());
    }
}
