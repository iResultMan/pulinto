//! Lines about one event of the user's typing: a word replaced, a layout switched, a hotkey
//! pressed. Their number and times follow the user's rhythm, so the daemon keeps them out of
//! the system journal: it installs a sink that holds them in memory (and in the debug log while
//! the debug mode is on). Without a sink — tests, the stands, a daemon started from a terminal
//! or into a file — they are printed like any other line.

use std::sync::OnceLock;

static SINK: OnceLock<fn(&str)> = OnceLock::new();

/// Called once by the daemon at start.
pub fn set_sink(sink: fn(&str)) {
    let _ = SINK.set(sink);
}

pub fn emit(line: &str) {
    match SINK.get() {
        Some(sink) => sink(line),
        None => println!("{line}"),
    }
}

/// `println!` for event lines, see the module description. Never put the typed text in one:
/// the debug mode has its own channel for that.
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {
        $crate::diag::emit(&format!($($arg)*))
    };
}
