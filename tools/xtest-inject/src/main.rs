//! PoC: type arbitrary Unicode text via XTest + scratch-keycode remapping.
//!
//! The technique (character-at-a-time, xdotool-style, WITH WAKE-UP ZWS):
//! 1. Find ONE unused keycode in the X11 keyboard map.
//! 2. Prepend a Zero-Width Space (ZWS, U+200B) to the text.
//!    GTK/Qt drop the FIRST keypress after idleness because they evaluate it
//!    using their old keymap cache while fetching the new one asynchronously.
//!    Sacrificing a ZWS perfectly bypasses this bug.
//! 3. For each character:
//!    a. Bind the scratch keycode to the Unicode keysym.
//!    b. XSync.
//!    c. Settle delay (e.g., 5-10ms) for clients to process MappingNotify.
//!    d. XTestFakeKeyEvent press/release.
//!    e. Restore the keycode to NoSymbol.
//!    f. XSync.
//!
//! Usage:
//!   cargo run -p xtest-inject -- "Привет мир"

use std::thread::sleep;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, Keysym, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt;

const DEFAULT_SETTLE_MS: u64 = 10;

fn char_to_keysym(ch: char) -> Keysym {
    let cp = ch as u32;
    if cp <= 0xFF {
        cp
    } else {
        0x0100_0000 | cp
    }
}

fn find_one_free_keycode(
    conn: &RustConnection,
    min_kc: u8,
    max_kc: u8,
) -> Result<Option<u8>, Box<dyn std::error::Error>> {
    let count = max_kc - min_kc + 1;
    let reply = xproto::get_keyboard_mapping(conn, min_kc, count)?.reply()?;
    let width = reply.keysyms_per_keycode as usize;

    for i in 0..count as usize {
        let base = i * width;
        let slice = &reply.keysyms[base..base + width];
        if slice.iter().all(|&sym| sym == 0) {
            return Ok(Some(min_kc + i as u8));
        }
    }
    Ok(None)
}

fn type_one_char(
    conn: &RustConnection,
    ch: char,
    scratch_kc: u8,
    settle_ms: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let keysym = char_to_keysym(ch);

    xproto::change_keyboard_mapping(conn, 1, scratch_kc, 1, &[keysym])?;
    conn.sync()?;
    if settle_ms > 0 {
        sleep(Duration::from_millis(settle_ms));
    }

    xtest::fake_input(conn, KEY_PRESS_EVENT, scratch_kc, 0, x11rb::NONE, 0, 0, 0)?;
    xtest::fake_input(conn, KEY_RELEASE_EVENT, scratch_kc, 0, x11rb::NONE, 0, 0, 0)?;

    xproto::change_keyboard_mapping(conn, 1, scratch_kc, 1, &[0u32])?;
    conn.sync()?;

    Ok(())
}

fn type_unicode(
    conn: &RustConnection,
    text: &str,
    scratch_kc: u8,
    settle_ms: u64,
) -> Result<Duration, Box<dyn std::error::Error>> {
    if text.is_empty() {
        return Ok(Duration::from_millis(0));
    }
    
    let start = Instant::now();
    // Prepend ZWS to sacrifice it to the GTK lazy-fetch bug
    let text_with_zws = format!("\u{200B}{}", text);

    for ch in text_with_zws.chars() {
        type_one_char(conn, ch, scratch_kc, settle_ms)?;
    }
    Ok(start.elapsed())
}

fn send_backspaces(conn: &RustConnection, count: usize) -> Result<(), Box<dyn std::error::Error>> {
    const BACKSPACE: u8 = 22;
    for _ in 0..count {
        xtest::fake_input(conn, KEY_PRESS_EVENT, BACKSPACE, 0, x11rb::NONE, 0, 0, 0)?;
        xtest::fake_input(conn, KEY_RELEASE_EVENT, BACKSPACE, 0, x11rb::NONE, 0, 0, 0)?;
    }
    conn.flush()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let mut backspaces = 0;
    let mut delay_secs = 0;
    let mut settle_ms = DEFAULT_SETTLE_MS;
    let mut text = String::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-b" | "--backspaces" => {
                i += 1;
                backspaces = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            "-d" | "--delay" => {
                i += 1;
                delay_secs = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            "-s" | "--settle" => {
                i += 1;
                settle_ms = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_SETTLE_MS);
            }
            _ if !args[i].starts_with("-") => {
                text = args[i].to_string();
            }
            _ => {}
        }
        i += 1;
    }

    if text.is_empty() {
        return Ok(());
    }

    let (conn, _screen) = x11rb::connect(None)?;
    let setup = conn.setup();
    let min_kc = setup.min_keycode;
    let max_kc = setup.max_keycode;

    let scratch_kc = find_one_free_keycode(&conn, min_kc, max_kc)?
        .ok_or("No free keycodes available")?;

    if delay_secs > 0 {
        sleep(Duration::from_secs(delay_secs));
    }

    if backspaces > 0 {
        send_backspaces(&conn, backspaces)?;
    }

    type_unicode(&conn, &text, scratch_kc, settle_ms)?;

    Ok(())
}
