//! Tap raw scan codes through a uinput device — the same path the daemon uses.
//!
//! For settling questions that cannot be answered by reading configuration:
//! what does a physical key actually produce right now, in the application that
//! has focus? XTEST tools (`xdotool`) cannot answer it — they inject at a
//! different level and are translated by different machinery.
//!
//! Usage: `tapkeys [--delay MS] <keycode>[+] ...`, where a trailing `+` means
//! the key is sent with Shift held. Waits two seconds first so the window you
//! are aiming at can take focus.
//!
//! Example — the keys of "word" on a US keyboard:
//!   cargo run --example tapkeys -p pulinto-input -- 17 24 19 32

use std::time::Duration;

use pulinto_input::injector::{Injector, Timing};

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let mut timing = Timing::default();
    let mut keys: Vec<(u16, bool)> = Vec::new();
    // Not the daemon's own device name, or the daemon would ignore us — it
    // excludes its injector from capture by name.
    let mut name = "PuLinTo Test Typist".to_string();
    // The daemon enumerates keyboards once, at startup. A device created after
    // it started is invisible to it — so the harness creates the device first
    // and waits here while the daemon comes up.
    let mut wait = Duration::from_secs(2);

    while let Some(arg) = args.next() {
        if arg == "--name" {
            if let Some(value) = args.next() {
                name = value;
            }
            continue;
        }
        if arg == "--wait" {
            if let Some(value) = args.next().and_then(|value| value.parse().ok()) {
                wait = Duration::from_secs(value);
            }
            continue;
        }
        if arg == "--delay" {
            if let Some(value) = args.next().and_then(|value| value.parse().ok()) {
                timing.key_hold_ms = value;
            }
            continue;
        }
        let shift = arg.ends_with('+');
        let code = arg.trim_end_matches('+');
        match code.parse::<u16>() {
            Ok(code) => keys.push((code, shift)),
            Err(_) => eprintln!("не скан-код: {arg}"),
        }
    }

    if keys.is_empty() {
        eprintln!("нечего печатать. Пример: tapkeys 17 24 19 32   # w o r d");
        std::process::exit(1);
    }

    let injector = match Injector::named(timing, &name) {
        Ok(injector) => injector,
        Err(error) => {
            eprintln!("не открыть /dev/uinput: {error}");
            std::process::exit(1);
        }
    };

    eprintln!("жду {} с — фокус и запуск демона…", wait.as_secs());
    std::thread::sleep(wait);

    if let Err(error) = injector.replay(&keys) {
        eprintln!("не отправить коды: {error}");
        std::process::exit(1);
    }
    eprintln!("отправлено кодов: {}", keys.len());
    // The device must outlive the events; closing it immediately can drop them.
    std::thread::sleep(Duration::from_millis(300));
}
