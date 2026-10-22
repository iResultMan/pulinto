//! Exclusive keyboard capture with pass-through.
//!
//! With `EVIOCGRAB` the kernel stops delivering events from a device to anyone
//! else, so we become the keyboard: every keystroke must be forwarded by us.
//! That is the price of being able to *hold* a keystroke — which is the whole
//! point: the delimiter waits until the cascade has
//! decided, so a correction lands before Enter reaches the application instead
//! of racing it.
//!
//! The danger is real and specific. A crash is safe: the kernel drops the grab
//! when the file descriptor closes. A *hang* is not — the keyboard would go
//! dead for the whole machine. Everything here exists to bound that risk:
//! a watchdog that flushes held input when a decision overruns, and an
//! emergency combination that releases the grab outright.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

pub use evdev::{EventType, InputEvent, Key};
use evdev::{Device, InputEventKind};

/// Held simultaneously, this releases the grab no matter what state the daemon
/// is in. Mirrors keyd's `backspace+escape+enter` — a combination no normal
/// typing produces, reachable even when everything else is wedged.
pub const PANIC_KEYS: [Key; 3] = [Key::KEY_BACKSPACE, Key::KEY_ESC, Key::KEY_ENTER];

/// Longest the engine may hold a keystroke before the watchdog gives up and
/// releases it unchanged. A missed correction is a nuisance; a keyboard that
/// stops responding is a broken machine.
pub const DECISION_BUDGET: Duration = Duration::from_millis(400);

pub struct GrabbedKeyboard {
    device: Device,
    grabbed: bool,
    /// A second handle on the same device node, never read from, for asking
    /// the kernel which keys are down. `EVIOCGKEY` on a handle also flushes
    /// that handle's queue of unread key events (`evdev_handle_get_val` →
    /// `__evdev_flush_queue`), so asked through the grabbed handle it ate the
    /// first key typed after a pause — press and release both, with no
    /// `SYN_DROPPED` to show for it. This handle's queue is always empty: the
    /// grab on the other one keeps events from reaching it.
    state_probe: Option<Device>,
}

/// How long one attempt to grab waits for the user to let go of every key. A
/// longer hold is the caller's to retry: waiting here would stall the other
/// keyboards queued behind this one.
const ALL_KEYS_UP_WAIT: Duration = Duration::from_secs(2);

/// Open the device node of `device` a second time for state queries; the
/// node is found through `/proc/self/fd`, since the handle came from an
/// enumeration that did not keep its path.
fn open_state_probe(device: &Device) -> Option<Device> {
    let path = std::fs::read_link(format!("/proc/self/fd/{}", device.as_raw_fd())).ok()?;
    match Device::open(&path) {
        Ok(probe) => Some(probe),
        Err(error) => {
            eprintln!(
                "Second descriptor {} not opened ({error}): the held-key check is disabled",
                path.display()
            );
            None
        }
    }
}

fn any_key_down(device: &Device) -> bool {
    device
        .get_key_state()
        .map(|keys| keys.iter().next().is_some())
        .unwrap_or(false)
}

impl GrabbedKeyboard {
    /// Take the keyboard — but never while a key is held.
    ///
    /// A key that is down when the grab starts is down in the compositor too,
    /// and its release then goes to us instead: the compositor never sees it
    /// and keeps the key pressed for good, repeating it until someone presses
    /// and releases the same key again. That is how a brightness key held
    /// across a daemon restart turned into a screen that kept dimming. keyd
    /// and kmonad wait for all keys up for the same reason.
    ///
    /// The check is made again after the grab, since a key can go down between
    /// the first look and the grab itself; then the grab is given back and the
    /// wait starts over.
    pub fn open(mut device: Device) -> std::io::Result<Self> {
        // The probe comes first: after `grab()` events reach only our
        // handle, and asking it for the key state would throw away whatever
        // arrived between the grab and the ioctl — under a typing user that
        // is a lost press or release, the very thing this check guards.
        let state_probe = open_state_probe(&device);
        let key_down = |device: &Device, probe: &Option<Device>| any_key_down(probe.as_ref().unwrap_or(device));
        let deadline = Instant::now() + ALL_KEYS_UP_WAIT;
        loop {
            if !key_down(&device, &state_probe) {
                device.grab()?;
                if !key_down(&device, &state_probe) {
                    return Ok(Self {
                        device,
                        grabbed: true,
                        state_probe,
                    });
                }
                device.ungrab()?;
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "a key is held down, grab postponed: it would stick in the system otherwise",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Keys the hardware reports as down right now — asked through the probe
    /// handle, never the grabbed one (see [`GrabbedKeyboard::state_probe`]).
    /// `None` when there is no probe: then the question must not be asked.
    pub fn keys_down(&self) -> Option<Vec<u16>> {
        self.state_probe
            .as_ref()?
            .get_key_state()
            .ok()
            .map(|keys| keys.iter().map(|key| key.code()).collect())
    }

    pub fn name(&self) -> String {
        self.device.name().unwrap_or("(unnamed)").to_string()
    }

    /// Vendor id of the keyboard: tells an Apple one from the rest.
    pub fn vendor_id(&self) -> u16 {
        self.device.input_id().vendor()
    }

    pub fn release(&mut self) {
        if self.grabbed {
            let _ = self.device.ungrab();
            self.grabbed = false;
        }
    }

    pub fn device_mut(&mut self) -> &mut Device {
        &mut self.device
    }
}

impl Drop for GrabbedKeyboard {
    fn drop(&mut self) {
        self.release();
    }
}

/// Tracks the emergency combination across events.
#[derive(Default)]
pub struct PanicDetector {
    held: [bool; PANIC_KEYS.len()],
}

impl PanicDetector {
    /// Returns true once all panic keys are down together.
    pub fn observe(&mut self, key: Key, pressed: bool) -> bool {
        if let Some(index) = PANIC_KEYS.iter().position(|candidate| *candidate == key) {
            self.held[index] = pressed;
        }
        self.held.iter().all(|down| *down)
    }
}

/// Keystrokes withheld from the application while a decision is pending.
///
/// Order is preserved: whatever the user typed during the pause is delivered
/// afterwards exactly as typed. Nothing is dropped — dropping input would be a
/// far worse failure than a late correction.
#[derive(Default)]
pub struct HeldInput {
    events: Vec<InputEvent>,
    since: Option<Instant>,
}

impl HeldInput {
    pub fn hold(&mut self, event: InputEvent) {
        if self.since.is_none() {
            self.since = Some(Instant::now());
        }
        self.events.push(event);
    }

    pub fn is_holding(&self) -> bool {
        self.since.is_some()
    }

    /// True when the decision has taken longer than the budget allows.
    pub fn overran(&self) -> bool {
        self.since
            .is_some_and(|start| start.elapsed() > DECISION_BUDGET)
    }

    /// Take everything held, clearing the buffer.
    pub fn drain(&mut self) -> Vec<InputEvent> {
        self.since = None;
        std::mem::take(&mut self.events)
    }
}

/// Whether this key ends a word without any correction being applicable —
/// the cursor moved, so what we believe sits to its left is no longer true.
pub fn is_navigation(key: Key) -> bool {
    matches!(
        key,
        Key::KEY_LEFT
            | Key::KEY_RIGHT
            | Key::KEY_UP
            | Key::KEY_DOWN
            | Key::KEY_HOME
            | Key::KEY_END
            | Key::KEY_PAGEUP
            | Key::KEY_PAGEDOWN
            | Key::KEY_INSERT
            | Key::KEY_DELETE
    )
}

/// A shared switch the watchdog thread can flip to force a release.
#[derive(Clone, Default)]
pub struct EmergencyRelease(Arc<AtomicBool>);

impl EmergencyRelease {
    pub fn trigger(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn taken(&self) -> bool {
        self.0.swap(false, Ordering::SeqCst)
    }
}

/// Keys the daemon has seen go down on one keyboard and not yet come up.
///
/// Whatever was forwarded as pressed has to be released again, or the virtual
/// keyboard keeps it down and the compositor repeats it. Normally the physical
/// release does that; it does not when the kernel dropped the event, when the
/// keyboard disappears mid-press, or when the grab is given up while keys are
/// still held (the emergency combination is exactly that). This is the list of
/// what to release in those cases.
#[derive(Debug, Default)]
pub struct HeldKeys {
    down: std::collections::BTreeSet<u16>,
}

impl HeldKeys {
    pub fn observe(&mut self, event: &InputEvent) {
        if event.event_type() != EventType::KEY {
            return;
        }
        match event.value() {
            1 => {
                self.down.insert(event.code());
            }
            0 => {
                self.down.remove(&event.code());
            }
            _ => {}
        }
    }

    /// Keys believed down that the hardware says are up: their release was
    /// lost. They are forgotten here, so each is reported once.
    pub fn lost(&mut self, hardware_down: &[u16]) -> Vec<u16> {
        let lost: Vec<u16> = self
            .down
            .iter()
            .copied()
            .filter(|code| !hardware_down.contains(code))
            .collect();
        for code in &lost {
            self.down.remove(code);
        }
        lost
    }

    /// Everything still held, forgotten: for when the keyboard is let go.
    pub fn take_all(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.down).into_iter().collect()
    }
}

/// A release event for `code`, as the reader would have produced it.
pub fn release_event(code: u16) -> InputEvent {
    InputEvent::new(EventType::KEY, code, 0)
}

/// Key transitions, ignoring auto-repeat.
///
/// The kernel emits `value == 2` for a held key. Feeding those into the word
/// buffer duplicates letters, which is exactly the "letters get doubled"
/// symptom users report. keyd drops them on input for the same reason.
pub fn key_transition(event: &InputEvent) -> Option<(Key, bool)> {
    match event.kind() {
        InputEventKind::Key(key) => match event.value() {
            0 => Some((key, false)),
            1 => Some((key, true)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_combination_needs_every_key() {
        let mut detector = PanicDetector::default();
        assert!(!detector.observe(Key::KEY_BACKSPACE, true));
        assert!(!detector.observe(Key::KEY_ESC, true));
        assert!(detector.observe(Key::KEY_ENTER, true));
        // Releasing one breaks it again.
        assert!(!detector.observe(Key::KEY_ESC, false));
    }

    #[test]
    fn held_input_preserves_order_and_clears() {
        let mut held = HeldInput::default();
        assert!(!held.is_holding());
        for code in [30u16, 31, 32] {
            held.hold(InputEvent::new(evdev::EventType::KEY, code, 1));
        }
        assert!(held.is_holding());
        let drained = held.drain();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].code(), 30);
        assert_eq!(drained[2].code(), 32);
        assert!(!held.is_holding());
    }

    #[test]
    fn a_lost_release_is_found_once() {
        let mut held = HeldKeys::default();
        let press = |code| InputEvent::new(EventType::KEY, code, 1);
        held.observe(&press(224));
        held.observe(&press(30));
        held.observe(&release_event(30));
        // 224 is down for the daemon, up for the hardware: its release was lost.
        assert_eq!(held.lost(&[]), vec![224]);
        assert!(held.lost(&[]).is_empty(), "reported once");
    }

    #[test]
    fn a_key_still_held_is_not_lost_and_is_released_on_exit() {
        let mut held = HeldKeys::default();
        let press = |code| InputEvent::new(EventType::KEY, code, 1);
        held.observe(&press(14));
        held.observe(&press(1));
        // Repeats (value 2) change nothing.
        held.observe(&InputEvent::new(EventType::KEY, 14, 2));
        assert!(held.lost(&[14, 1]).is_empty());
        assert_eq!(held.take_all(), vec![1, 14]);
        assert!(held.take_all().is_empty());
    }

    #[test]
    fn auto_repeat_is_not_a_transition() {
        let repeat = InputEvent::new(evdev::EventType::KEY, Key::KEY_A.code(), 2);
        assert_eq!(key_transition(&repeat), None);

        let press = InputEvent::new(evdev::EventType::KEY, Key::KEY_A.code(), 1);
        assert_eq!(key_transition(&press), Some((Key::KEY_A, true)));
    }

    #[test]
    fn navigation_cancels_but_letters_do_not() {
        assert!(is_navigation(Key::KEY_LEFT));
        assert!(is_navigation(Key::KEY_HOME));
        assert!(!is_navigation(Key::KEY_A));
        assert!(!is_navigation(Key::KEY_SPACE));
    }
}
