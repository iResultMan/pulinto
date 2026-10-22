//! Keyboard capture at the kernel level.
//!
//! The X11 path (`rdev`, backed by XRecord) observes events only *after* the
//! application has already received them, and its synthetic replies race with
//! the real key stream — which is why injected corrections arrived garbled.
//! Reading `/dev/input/event*` puts us on the same level as the compositor, so
//! capture and injection share one clock and one ordering, and the same code
//! works under X11, Wayland and a bare TTY.
//!
//! We deliberately do **not** take an exclusive grab (`EVIOCGRAB`). A grab
//! would let us delay a keystroke and decide before the application sees it,
//! but it also means that if this process wedges, the keyboard dies for the
//! whole machine. Our design never needs to delay a key (Enter is
//! passed through untouched), so the risk buys nothing.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub use evdev::Key;
use evdev::{Device, InputEventKind};

/// One key transition, already reduced to what the engine cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub pressed: bool,
    /// When the kernel handed us this keystroke.
    ///
    /// Not when it was *processed*: the daemon can be several hundred
    /// milliseconds behind while a correction is being injected, and anything
    /// that measures the user's typing by processing time ends up measuring the
    /// daemon instead. That is not hypothetical — it is why the guard against
    /// correcting over live typing only worked half the time.
    pub at: std::time::Instant,
    /// USB/Bluetooth vendor id of the keyboard it came from, which decides
    /// the hotkey set (Apple or PC) the key is read by.
    pub vendor: u16,
}

/// Name of the uinput device we inject through — see `evdev::EvdevInputBackend`.
/// It must never be captured: reading our own synthetic keystrokes back feeds
/// them into the word buffer and can trigger a second conversion of text we
/// just wrote. The `is_injecting` flag guards the same hazard, but only for as
/// long as it happens to be set, which is a race; excluding the device by name
/// removes the hazard instead of timing around it.
pub const VIRTUAL_DEVICE_NAME: &str = "PuLinTo Virtual Keyboard";

/// USB/Bluetooth vendor id of Apple. A real Apple keyboard may carry any name —
/// this machine's reports itself as "System Administrator's Keyboard" — but its
/// vendor id is always this one.
pub const APPLE_VENDOR_ID: u16 = 0x05ac;

/// Is an Apple keyboard among the connected ones?
pub fn apple_keyboard_present() -> bool {
    keyboards()
        .iter()
        .any(|(_, device)| device.input_id().vendor() == APPLE_VENDOR_ID)
}

/// Every keyboard-like device on the system, minus our own.
///
/// Machines routinely expose several: the built-in keyboard, a "video bus" that
/// emits brightness keys, a power button. Anything advertising the letter `A`
/// is a real keyboard; the rest would only add noise.
pub fn keyboards() -> Vec<(PathBuf, Device)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("event"))
        {
            continue;
        }
        let Ok(device) = Device::open(&path) else {
            continue;
        };
        if device.name() == Some(VIRTUAL_DEVICE_NAME) {
            continue;
        }
        let is_keyboard = device
            .supported_keys()
            .is_some_and(|keys| keys.contains(Key::KEY_A) && keys.contains(Key::KEY_SPACE));
        if is_keyboard {
            found.push((path, device));
        }
    }
    found
}

/// Start reading every keyboard, delivering key transitions on one channel.
///
/// Returns the receiver and the names of the devices being watched. An empty
/// name list means nothing could be opened — almost always missing membership
/// in the `input` group.
pub fn start() -> (Receiver<KeyEvent>, Vec<String>) {
    let (sender, receiver) = mpsc::channel();
    let mut names = Vec::new();

    for (path, mut device) in keyboards() {
        let vendor = device.input_id().vendor();
        names.push(
            device
                .name()
                .unwrap_or(&path.to_string_lossy())
                .to_string(),
        );
        let sender = sender.clone();
        thread::spawn(move || loop {
            match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        if let InputEventKind::Key(key) = event.kind() {
                            // The kernel emits value 2 for a held key. Letting
                            // those through duplicates letters in the word
                            // buffer — the "прррррр" symptom found in testing.
                            // The grab path has always dropped them; this one
                            // did not, which is why only observe mode was
                            // affected.
                            let pressed = match event.value() {
                                0 => false,
                                1 => true,
                                _ => continue,
                            };
                            if sender
                                .send(KeyEvent {
                                    key,
                                    pressed,
                                    at: std::time::Instant::now(),
                                    vendor,
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                // The device disappeared (unplugged, suspend). Stop this
                // thread; the others keep working.
                Err(_) => return,
            }
        });
    }

    (receiver, names)
}

/// Base US-layout character for a physical key, or `None` for non-character
/// keys. Mirrors `rdev_key_to_char`, but keyed on the kernel's own codes so no
/// X11 round-trip is involved.
pub fn key_to_char(key: Key) -> Option<char> {
    let ch = match key {
        Key::KEY_A => 'a',
        Key::KEY_B => 'b',
        Key::KEY_C => 'c',
        Key::KEY_D => 'd',
        Key::KEY_E => 'e',
        Key::KEY_F => 'f',
        Key::KEY_G => 'g',
        Key::KEY_H => 'h',
        Key::KEY_I => 'i',
        Key::KEY_J => 'j',
        Key::KEY_K => 'k',
        Key::KEY_L => 'l',
        Key::KEY_M => 'm',
        Key::KEY_N => 'n',
        Key::KEY_O => 'o',
        Key::KEY_P => 'p',
        Key::KEY_Q => 'q',
        Key::KEY_R => 'r',
        Key::KEY_S => 's',
        Key::KEY_T => 't',
        Key::KEY_U => 'u',
        Key::KEY_V => 'v',
        Key::KEY_W => 'w',
        Key::KEY_X => 'x',
        Key::KEY_Y => 'y',
        Key::KEY_Z => 'z',
        Key::KEY_0 => '0',
        Key::KEY_1 => '1',
        Key::KEY_2 => '2',
        Key::KEY_3 => '3',
        Key::KEY_4 => '4',
        Key::KEY_5 => '5',
        Key::KEY_6 => '6',
        Key::KEY_7 => '7',
        Key::KEY_8 => '8',
        Key::KEY_9 => '9',
        Key::KEY_GRAVE => '`',
        Key::KEY_MINUS => '-',
        Key::KEY_EQUAL => '=',
        Key::KEY_LEFTBRACE => '[',
        Key::KEY_RIGHTBRACE => ']',
        Key::KEY_BACKSLASH => '\\',
        Key::KEY_SEMICOLON => ';',
        Key::KEY_APOSTROPHE => '\'',
        Key::KEY_COMMA => ',',
        Key::KEY_DOT => '.',
        Key::KEY_SLASH => '/',
        _ => return None,
    };
    Some(ch)
}

pub fn is_alt(key: Key) -> bool {
    matches!(key, Key::KEY_LEFTALT | Key::KEY_RIGHTALT)
}

pub fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::KEY_LEFTSHIFT
            | Key::KEY_RIGHTSHIFT
            | Key::KEY_LEFTCTRL
            | Key::KEY_RIGHTCTRL
            | Key::KEY_LEFTALT
            | Key::KEY_RIGHTALT
            | Key::KEY_LEFTMETA
            | Key::KEY_RIGHTMETA
            | Key::KEY_CAPSLOCK
    )
}
