//! Linux evdev and uinput input backend for Wayland sessions.

use crate::{InputBackend, InputError};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Key};
use std::sync::Mutex;

// Re-export for use by crates that depend on pulinto-input.
pub use evdev::{EventType, InputEvent};

pub const KEY_BACKSPACE_CODE: u16 = 14; // Linux input event KEY_BACKSPACE
pub const EVDEV_SHIFT_KEYCODE: u16 = 42; // KEY_LEFTSHIFT
pub const EVDEV_SPACE_KEYCODE: u16 = 57; // KEY_SPACE

pub static POSITION_TO_EVDEV_KEYCODE: [u16; 47] = [
    41, // 0: `
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, // 1..10: 1..0
    12, // 11: -
    13, // 12: =
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, // 13..22: q..p
    26, // 23: [
    27, // 24: ]
    43, // 25: \
    30, 31, 32, 33, 34, 35, 36, 37, 38, // 26..34: a..l
    39, // 35: ;
    40, // 36: '
    44, 45, 46, 47, 48, 49, 50, // 37..43: z..m
    51, // 44: ,
    52, // 45: .
    53, // 46: /
];

pub struct EvdevInputBackend {
    virtual_device: Option<Mutex<VirtualDevice>>,
}

impl EvdevInputBackend {
    pub fn new() -> Result<Self, InputError> {
        let mut keys = AttributeSet::<Key>::new();
        // Register all common keys for uinput injection
        for code in 1..255 {
            keys.insert(Key::new(code));
        }

        let virtual_device = evdev::uinput::VirtualDeviceBuilder::new()
            .map_err(|e| InputError::InjectionError(e.to_string()))?
            .name(crate::capture::VIRTUAL_DEVICE_NAME)
            .with_keys(&keys)
            .map_err(|e| InputError::InjectionError(e.to_string()))?
            .build()
            .map_err(|e| InputError::InjectionError(e.to_string()))?;

        Ok(Self {
            virtual_device: Some(Mutex::new(virtual_device)),
        })
    }

    pub fn new_mock() -> Self {
        Self {
            virtual_device: None,
        }
    }
}

impl InputBackend for EvdevInputBackend {
    fn name(&self) -> &'static str {
        "Wayland (evdev/uinput)"
    }

    fn send_key_combination(&self, keycode: u8, modifiers: u16) -> Result<(), InputError> {
        if let Some(ref dev_mutex) = self.virtual_device {
            let mut dev = dev_mutex
                .lock()
                .map_err(|e| InputError::InjectionError(e.to_string()))?;
            let key = Key::new(keycode as u16);
            let shift_key = Key::new(EVDEV_SHIFT_KEYCODE);
            let syn_ev = evdev::InputEvent::new(evdev::EventType::SYNCHRONIZATION, 0, 0);

            let need_shift = (modifiers & 1) != 0;
            if need_shift {
                let down = evdev::InputEvent::new(evdev::EventType::KEY, shift_key.code(), 1);
                dev.emit(&[down, syn_ev])
                    .map_err(|e| InputError::InjectionError(e.to_string()))?;
            }

            let down_ev = evdev::InputEvent::new(evdev::EventType::KEY, key.code(), 1);
            let up_ev = evdev::InputEvent::new(evdev::EventType::KEY, key.code(), 0);

            dev.emit(&[down_ev, syn_ev])
                .map_err(|e| InputError::InjectionError(e.to_string()))?;
            dev.emit(&[up_ev, syn_ev])
                .map_err(|e| InputError::InjectionError(e.to_string()))?;

            if need_shift {
                let up = evdev::InputEvent::new(evdev::EventType::KEY, shift_key.code(), 0);
                dev.emit(&[up, syn_ev])
                    .map_err(|e| InputError::InjectionError(e.to_string()))?;
            }
        }
        Ok(())
    }

    fn send_backspaces(&self, count: usize) -> Result<(), InputError> {
        for _ in 0..count {
            self.send_key_combination(KEY_BACKSPACE_CODE as u8, 0)?;
        }
        Ok(())
    }

    fn send_text_for_layout(
        &self,
        text: &str,
        layout: &pulinto_core::layout::Layout,
    ) -> Result<(), InputError> {
        for ch in text.chars() {
            if ch == ' ' {
                self.send_key_combination(EVDEV_SPACE_KEYCODE as u8, 0)?;
            } else if let Some((pos, level)) = layout.position_of(ch) {
                if pos < POSITION_TO_EVDEV_KEYCODE.len() {
                    let keycode = POSITION_TO_EVDEV_KEYCODE[pos] as u8;
                    let modifiers = if level == pulinto_core::layout::Level::Shifted { 1 } else { 0 };
                    self.send_key_combination(keycode, modifiers)?;
                }
            }
        }
        Ok(())
    }

    fn send_left(&self) -> Result<(), InputError> {
        self.send_key_combination(105, 0) // 105 is evdev KEY_LEFT
    }

    fn send_right(&self) -> Result<(), InputError> {
        self.send_key_combination(106, 0) // 106 is evdev KEY_RIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evdev_mock_backend() {
        let backend = EvdevInputBackend::new_mock();
        assert_eq!(backend.name(), "Wayland (evdev/uinput)");
        assert!(backend.send_backspaces(3).is_ok());
    }
}
