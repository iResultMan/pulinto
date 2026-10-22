//! Test fixture for closed-loop integration testing of pulintod.
//!
//! Not part of the product. Used to simulate physical keyboard input at the
//! kernel level (via its own uinput virtual keyboard, distinct from the
//! daemon's own "PuLinTo Virtual Keyboard" device) and to observe what the
//! daemon emits on its output device.
//!
//! Usage:
//!   kbharness send <script>     create a virtual keyboard named
//!                               "PuLinTo Test Fixture Keyboard" and play a
//!                               scripted sequence of key events through it.
//!   kbharness sniff <ms>        open the daemon's output device
//!                               ("PuLinTo Virtual Keyboard") read-only and
//!                               print every event for <ms> milliseconds,
//!                               with a best-effort decode to text under both
//!                               the "us" and "ru" layouts.
//!
//! Script mini-language (space separated tokens):
//!   type:TEXT        press+release each character in TEXT (lowercase
//!                    letters/digits only), ~35ms between keys
//!   key:NAME         single press+release of a named key
//!   hold:NAME:N:MS   press NAME, then emit N autorepeat (value=2) events MS
//!                    apart, then release
//!   combo:A+B+C      press A, then B, then C (10ms apart), hold 100ms, then
//!                    release C, B, A (mirrors a real chorded press)
//!   sleep:MS         pause
//!
//! Named keys: SPACE ENTER BACKSPACE ESC TAB LEFT RIGHT UP DOWN HOME END
//! LSHIFT

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use evdev::uinput::VirtualDeviceBuilder;
use evdev::{AttributeSet, Device, EventType, InputEvent, InputEventKind, Key};

const FIXTURE_NAME: &str = "PuLinTo Test Fixture Keyboard";

/// Wall-clock milliseconds since the Unix epoch. Used (instead of `Instant`)
/// specifically because it is comparable across the two separate `kbharness`
/// processes (`send` and `sniff`) — `CLOCK_MONOTONIC` is shared system-wide on
/// Linux but `Instant`'s API gives no portable way to read it as a number, so
/// wall-clock time is what lets us line up a keypress in one process's log
/// with its effect in the other's and measure real end-to-end latency.
fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

fn keycode_for_char(ch: char) -> Option<u16> {
    Some(match ch.to_ascii_lowercase() {
        'q' => 16, 'w' => 17, 'e' => 18, 'r' => 19, 't' => 20, 'y' => 21, 'u' => 22,
        'i' => 23, 'o' => 24, 'p' => 25,
        'a' => 30, 's' => 31, 'd' => 32, 'f' => 33, 'g' => 34, 'h' => 35, 'j' => 36,
        'k' => 37, 'l' => 38,
        'z' => 44, 'x' => 45, 'c' => 46, 'v' => 47, 'b' => 48, 'n' => 49, 'm' => 50,
        '1' => 2, '2' => 3, '3' => 4, '4' => 5, '5' => 6, '6' => 7, '7' => 8, '8' => 9,
        '9' => 10, '0' => 11,
        _ => return None,
    })
}

/// Keycode plus whether Shift must be held, for characters that need a
/// distinct physical key or a shift level: uppercase letters and a handful
/// of punctuation marks needed by the stop-rule test words (URL, password).
fn key_and_shift_for_char(ch: char) -> Option<(u16, bool)> {
    if ch.is_ascii_uppercase() {
        return keycode_for_char(ch).map(|code| (code, true));
    }
    if let Some(code) = keycode_for_char(ch) {
        return Some((code, false));
    }
    Some(match ch {
        ':' => (39, true),  // Shift+;
        '/' => (53, false),
        '.' => (52, false),
        '@' => (3, true),   // Shift+2
        '-' => (12, false),
        '_' => (12, true),
        _ => return None,
    })
}

fn named_key(name: &str) -> Option<u16> {
    Some(match name.to_uppercase().as_str() {
        "SPACE" => 57,
        "ENTER" => 28,
        "BACKSPACE" => 14,
        "ESC" => 1,
        "TAB" => 15,
        "LEFT" => 105,
        "RIGHT" => 106,
        "UP" => 103,
        "DOWN" => 108,
        "HOME" => 102,
        "END" => 107,
        "LSHIFT" => 42,
        _ => return None,
    })
}

fn syn() -> InputEvent {
    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0)
}

fn press(code: u16) -> InputEvent {
    InputEvent::new(EventType::KEY, code, 1)
}
fn release(code: u16) -> InputEvent {
    InputEvent::new(EventType::KEY, code, 0)
}
fn repeat(code: u16) -> InputEvent {
    InputEvent::new(EventType::KEY, code, 2)
}

struct Fixture {
    device: evdev::uinput::VirtualDevice,
}

impl Fixture {
    fn new() -> std::io::Result<Self> {
        let mut keys = AttributeSet::<Key>::new();
        for code in 1..=248u16 {
            keys.insert(Key::new(code));
        }
        let device = VirtualDeviceBuilder::new()?
            .name(FIXTURE_NAME)
            .with_keys(&keys)?
            .build()?;
        Ok(Self { device })
    }

    fn emit(&mut self, events: &[InputEvent]) {
        if let Err(error) = self.device.emit(events) {
            eprintln!("emit failed: {error}");
        }
    }

    fn tap(&mut self, code: u16) {
        self.tap_shift(code, false);
    }

    fn tap_shift(&mut self, code: u16, shift: bool) {
        if shift {
            self.emit(&[press(42), syn()]);
            std::thread::sleep(Duration::from_millis(5));
        }
        self.emit(&[press(code), syn()]);
        std::thread::sleep(Duration::from_millis(12));
        self.emit(&[release(code), syn()]);
        if shift {
            std::thread::sleep(Duration::from_millis(5));
            self.emit(&[release(42), syn()]);
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    fn run_script(&mut self, script: &str) {
        println!("[fixture] device ready, running script");
        // Give the daemon's hotplug monitor time to notice this fresh device.
        for token in script.split_whitespace() {
            self.run_token(token);
        }
        println!("[fixture] script done");
    }

    fn run_token(&mut self, token: &str) {
        let mut parts = token.splitn(2, ':');
        let cmd = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        match cmd {
            "type" => {
                for ch in rest.chars() {
                    if let Some((code, shift)) = key_and_shift_for_char(ch) {
                        self.tap_shift(code, shift);
                    } else {
                        eprintln!("[fixture] skip unmapped char {ch:?}");
                    }
                }
            }
            "key" => {
                if let Some(code) = named_key(rest) {
                    // Wall-clock mark for delimiters specifically: this is
                    // the timestamp the latency measurement in the report is
                    // computed from (paired against the sniff process' own
                    // wall-clock log of when the same code re-appears on the
                    // daemon's output device).
                    if matches!(rest.to_uppercase().as_str(), "SPACE" | "ENTER") {
                        println!("[fixture] wall={} pressing delimiter {rest}", now_ms());
                    }
                    self.tap(code);
                } else {
                    eprintln!("[fixture] unknown key {rest}");
                }
            }
            "hold" => {
                let mut fields = rest.splitn(3, ':');
                let name = fields.next().unwrap_or("");
                let n: u32 = fields.next().unwrap_or("5").parse().unwrap_or(5);
                let ms: u64 = fields.next().unwrap_or("30").parse().unwrap_or(30);
                if let Some(code) = named_key(name).or_else(|| {
                    name.chars().next().and_then(keycode_for_char)
                }) {
                    println!("[fixture] hold {name} x{n} every {ms}ms (autorepeat)");
                    self.emit(&[press(code), syn()]);
                    for _ in 0..n {
                        std::thread::sleep(Duration::from_millis(ms));
                        self.emit(&[repeat(code), syn()]);
                    }
                    std::thread::sleep(Duration::from_millis(ms));
                    self.emit(&[release(code), syn()]);
                } else {
                    eprintln!("[fixture] unknown hold key {name}");
                }
            }
            "combo" => {
                let codes: Vec<u16> = rest
                    .split('+')
                    .filter_map(|name| named_key(name).or_else(|| name.chars().next().and_then(keycode_for_char)))
                    .collect();
                println!("[fixture] combo {rest} -> pressing {codes:?}");
                for code in &codes {
                    self.emit(&[press(*code), syn()]);
                    std::thread::sleep(Duration::from_millis(10));
                }
                std::thread::sleep(Duration::from_millis(100));
                for code in codes.iter().rev() {
                    self.emit(&[release(*code), syn()]);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            "sleep" => {
                let ms: u64 = rest.parse().unwrap_or(0);
                std::thread::sleep(Duration::from_millis(ms));
            }
            "" => {}
            other => eprintln!("[fixture] unknown token {other}"),
        }
    }
}

fn find_device_by_name(name: &str) -> Option<Device> {
    let entries = std::fs::read_dir("/dev/input").ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("event"))
        {
            continue;
        }
        if let Ok(device) = Device::open(&path) {
            if device.name() == Some(name) {
                return Some(device);
            }
        }
    }
    None
}

fn decode(codes: &[(u16, bool)]) -> (String, String) {
    let us = pulinto_core::layout::Layout::by_id("us").unwrap();
    let ru = pulinto_core::layout::Layout::by_id("ru").unwrap();
    let decode_one = |layout: &pulinto_core::layout::Layout| -> String {
        codes
            .iter()
            .filter_map(|(code, shift)| {
                let pos = pulinto_input::injector::position_of_keycode(*code)?;
                let level = if *shift {
                    pulinto_core::layout::Level::Shifted
                } else {
                    pulinto_core::layout::Level::Unshifted
                };
                layout.char_at(pos, level)
            })
            .collect()
    };
    (decode_one(us), decode_one(ru))
}

fn sniff(duration_ms: u64, name_override: Option<&str>) {
    let name = name_override.unwrap_or(pulinto_input::capture::VIRTUAL_DEVICE_NAME);
    println!("[sniff] looking for device named {name:?}");
    let mut device = None;
    for _ in 0..40 {
        if let Some(found) = find_device_by_name(name) {
            device = Some(found);
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let Some(mut device) = device else {
        println!("[sniff] device not found after retrying (daemon not in grab/observe mode with injector active?)");
        return;
    };
    println!("[sniff] found, reading for {duration_ms}ms");
    let deadline = std::time::Instant::now() + Duration::from_millis(duration_ms);
    let mut run: Vec<(u16, bool)> = Vec::new();
    let mut shift_down = false;
    let start = std::time::Instant::now();
    let mut last_event_at = start;
    // A gap this long between key events closes the current "segment" and
    // starts decoding a fresh one — lets one long sniff session cover many
    // scenarios (separated by sleep: tokens in the send script) while still
    // reporting each one legibly instead of one giant concatenated string.
    const SEGMENT_GAP: Duration = Duration::from_millis(300);

    let flush_segment = |run: &mut Vec<(u16, bool)>| {
        if run.is_empty() {
            return;
        }
        let (as_us, as_ru) = decode(run);
        println!(
            "[sniff] --- segment: {} presses, as-us={as_us:?} as-ru={as_ru:?} ---",
            run.len()
        );
        run.clear();
    };

    // fetch_events blocks; poll in a loop with short reads via a background
    // thread so we can respect the deadline.
    let (tx, rx) = std::sync::mpsc::channel::<InputEvent>();
    std::thread::spawn(move || loop {
        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    if tx.send(event).is_err() {
                        return;
                    }
                }
            }
            Err(_) => return,
        }
    });

    while std::time::Instant::now() < deadline {
        let timeout = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(timeout.min(Duration::from_millis(50))) {
            Ok(event) => {
                let now = std::time::Instant::now();
                if now.duration_since(last_event_at) > SEGMENT_GAP {
                    flush_segment(&mut run);
                }
                last_event_at = now;
                let t = start.elapsed().as_millis();
                if let InputEventKind::Key(key) = event.kind() {
                    let code = key.code();
                    let value = event.value();
                    println!("[sniff] t={t}ms wall={} code={code} value={value}", now_ms());
                    if code == 42 || code == 54 {
                        shift_down = value != 0;
                    }
                    if value == 1 {
                        run.push((code, shift_down));
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    flush_segment(&mut run);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("send") => {
            let script = args.get(2).cloned().unwrap_or_default();
            let mut fixture = Fixture::new().expect("create virtual fixture keyboard");
            // Let the daemon's device enumeration see us if it starts fresh;
            // harmless if it was already running.
            std::thread::sleep(Duration::from_millis(300));
            fixture.run_script(&script);
            // Keep the device alive briefly so trailing events are not lost
            // to a device teardown race.
            std::thread::sleep(Duration::from_millis(500));
        }
        Some("sniff") => {
            let ms: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3000);
            let name_override = args.get(3).map(String::as_str);
            sniff(ms, name_override);
        }
        Some("grabcheck") => {
            // Proves the daemon actually released EVIOCGRAB on the named
            // device (rather than merely printing that it did): if the
            // daemon still holds the grab, our own grab() call fails with
            // EBUSY; if released, it succeeds and we immediately let go.
            let name = args.get(2).cloned().unwrap_or_else(|| FIXTURE_NAME.to_string());
            match find_device_by_name(&name) {
                Some(mut device) => match device.grab() {
                    Ok(()) => {
                        println!("[grabcheck] grab succeeded: device {name:?} is NOT held exclusively by anyone else");
                        let _ = device.ungrab();
                    }
                    Err(error) => {
                        println!("[grabcheck] grab FAILED ({error}): device {name:?} is still exclusively held by another process");
                    }
                },
                None => println!("[grabcheck] device {name:?} not found"),
            }
        }
        _ => {
            eprintln!("usage: kbharness send <script> | kbharness sniff <ms> [device] | kbharness grabcheck [device]");
            std::process::exit(2);
        }
    }
}
