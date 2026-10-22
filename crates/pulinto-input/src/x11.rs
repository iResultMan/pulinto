//! X11 input backend using XTest extension for synthesis.

use crate::{InputBackend, InputError};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

const BACKSPACE_KEYCODE: u8 = 22;
const SHIFT_KEYCODE: u8 = 50;
const SPACE_KEYCODE: u8 = 65;

/// Maps key-position index (0..46) to X11 keycode.
pub static POSITION_TO_X11_KEYCODE: [u8; 47] = [
    49, // 0: `
    10, 11, 12, 13, 14, 15, 16, 17, 18, 19, // 1..10: 1..0
    20, // 11: -
    21, // 12: =
    24, 25, 26, 27, 28, 29, 30, 31, 32, 33, // 13..22: q..p
    34, // 23: [
    35, // 24: ]
    51, // 25: backslash
    38, 39, 40, 41, 42, 43, 44, 45, 46, // 26..34: a..l
    47, // 35: ;
    48, // 36: '
    52, 53, 54, 55, 56, 57, 58, // 37..43: z..m
    59, // 44: ,
    60, // 45: .
    61, // 46: /
];

pub struct X11InputBackend {
    conn: RustConnection,
}

impl X11InputBackend {
    pub fn new() -> Result<Self, InputError> {
        let (conn, _) = x11rb::connect(None).map_err(|e| InputError::X11Error(e.to_string()))?;
        Ok(Self { conn })
    }

    /// Send a single XTest key event (press or release).
    fn fake_key(&self, event_type: u8, keycode: u8) -> Result<(), InputError> {
        xtest::fake_input(&self.conn, event_type, keycode, 0, x11rb::NONE, 0, 0, 0)
            .map_err(|e| InputError::InjectionError(e.to_string()))?;
        Ok(())
    }

    /// Press and immediately release a key, with optional Shift modifier.
    fn tap_key(&self, keycode: u8, shifted: bool) -> Result<(), InputError> {
        if shifted {
            self.fake_key(KEY_PRESS_EVENT, SHIFT_KEYCODE)?;
        }
        self.fake_key(KEY_PRESS_EVENT, keycode)?;
        self.fake_key(KEY_RELEASE_EVENT, keycode)?;
        if shifted {
            self.fake_key(KEY_RELEASE_EVENT, SHIFT_KEYCODE)?;
        }
        self.conn
            .flush()
            .map_err(|e| InputError::InjectionError(e.to_string()))?;
        Ok(())
    }
}

impl InputBackend for X11InputBackend {
    fn name(&self) -> &'static str {
        "X11 (XTest)"
    }

    fn send_key_combination(&self, keycode: u8, modifiers: u16) -> Result<(), InputError> {
        self.tap_key(keycode, (modifiers & 1) != 0)
    }

    fn send_backspaces(&self, count: usize) -> Result<(), InputError> {
        for _ in 0..count {
            self.tap_key(BACKSPACE_KEYCODE, false)?;
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
                self.tap_key(SPACE_KEYCODE, false)?;
            } else if let Some((pos, level)) = layout.position_of(ch) {
                if pos < POSITION_TO_X11_KEYCODE.len() {
                    let keycode = POSITION_TO_X11_KEYCODE[pos];
                    let shifted = level == pulinto_core::layout::Level::Shifted;
                    self.tap_key(keycode, shifted)?;
                }
            }
        }
        Ok(())
    }

    fn send_left(&self) -> Result<(), InputError> {
        self.tap_key(113, false) // 113 is X11 Left Arrow
    }

    fn send_right(&self) -> Result<(), InputError> {
        self.tap_key(114, false) // 114 is X11 Right Arrow
    }
}
