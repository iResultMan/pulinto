//! What the daemon keeps of its own work, and the debug mode.
//!
//! In normal work nothing is written to a file: the last lines of the daemon's output sit in a
//! ring in memory (for the "copy diagnostics" button and the bug report), and the system
//! journal gets only the lines about the daemon's own state, never one per word.
//!
//! The debug mode is for finding a fault together with the developer. It records more,
//! including, on request, the typed words and every key, so it is:
//!   * off at every start, never saved in the settings;
//!   * written to `$XDG_RUNTIME_DIR` (memory, erased at logout), never to the disk;
//!   * limited in time (an hour, a quarter of an hour for the key trace);
//!   * shown by a red dot on the tray icon and a notification while it records;
//!   * deleted when the user switches it off, unless they ask for a copy to send.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RING_LINES: usize = 1000;
/// The log is cut in two when it grows past this; the older half is kept as `debug.log.1`.
const LOG_LIMIT: u64 = 32 << 20;
/// Files left by a run that ended without the user's decision are removed after this.
const LEFTOVER_MAX_AGE: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    /// The full text and a backtrace of a panic (otherwise only where it happened).
    Panic,
    /// The words typed and what they became. Only words that passed the stop rules
    /// (password fields, switched-off applications) reach the log.
    Text,
    /// A line for every decision, including the words left alone, with the stage and the time.
    Decisions,
    /// Every key read and written, with the time. Records passwords too.
    Keys,
}

const ALL: [Opt; 4] = [Opt::Panic, Opt::Text, Opt::Decisions, Opt::Keys];

impl Opt {
    pub fn id(self) -> &'static str {
        match self {
            Opt::Panic => "panic",
            Opt::Text => "text",
            Opt::Decisions => "decisions",
            Opt::Keys => "keys",
        }
    }

    pub fn parse(id: &str) -> Option<Opt> {
        ALL.into_iter().find(|opt| opt.id() == id)
    }

    fn index(self) -> usize {
        ALL.iter().position(|opt| *opt == self).unwrap_or(0)
    }

    fn limit(self) -> Duration {
        match self {
            Opt::Keys => Duration::from_secs(15 * 60),
            _ => Duration::from_secs(3600),
        }
    }
}

static RING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static RECORDING: AtomicBool = AtomicBool::new(false);
static FLAGS: [AtomicBool; 4] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

struct Session {
    log: File,
    log_bytes: u64,
    until: [Option<Instant>; 4],
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn flag(opt: Opt) -> bool {
    FLAGS[opt.index()].load(Ordering::Relaxed)
}

pub fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// The tests of this module point the directories at a temporary one instead of changing the
/// environment, which the other tests of the process share. Always empty in the daemon.
static ROOT_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

fn dir() -> Option<PathBuf> {
    if let Some(root) = ROOT_OVERRIDE.lock().ok().and_then(|root| root.clone()) {
        return Some(root.join("pulinto-debug"));
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(value).join("pulinto-debug"))
}

fn log_path(dir: &std::path::Path) -> PathBuf {
    dir.join("debug.log")
}

fn keys_path(dir: &std::path::Path) -> PathBuf {
    dir.join("keys.log")
}

fn open_private(path: &std::path::Path) -> std::io::Result<File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

fn make_dir() -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let dir = dir().ok_or("no $XDG_RUNTIME_DIR: the debug log has nowhere to live in memory")?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|error| error.to_string())?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    Ok(dir)
}

// ── The ring ─────────────────────────────────────────────────────────────

/// One line of the daemon's output (already stamped with the time).
pub fn record(line: &str) {
    if let Ok(mut ring) = RING.lock() {
        if ring.len() >= RING_LINES {
            ring.pop_front();
        }
        ring.push_back(line.to_string());
    }
    if recording() {
        write_log(line);
    }
}

/// The sink of [`pulinto_core::diag`] when the output goes to the journal: the line is kept in
/// memory and in the debug log, not sent to the journal.
pub fn event_line(line: &str) {
    record(&format!("{} {line}", crate::report::now_ms()));
}

/// The last lines of the daemon's output, for the "copy diagnostics" button.
pub fn diagnostics() -> String {
    RING.lock()
        .map(|ring| ring.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn write_log(line: &str) {
    let mut slot = SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    append(&mut slot, line);
}

fn append(slot: &mut Option<Session>, line: &str) {
    let Some(session) = slot.as_mut() else { return };
    if writeln!(session.log, "{line}").is_ok() {
        session.log_bytes += line.len() as u64 + 1;
    }
    if session.log_bytes > LOG_LIMIT {
        if let Some(dir) = dir() {
            let _ = std::fs::rename(log_path(&dir), dir.join("debug.log.1"));
            if let Ok(file) = open_private(&log_path(&dir)) {
                session.log = file;
                session.log_bytes = 0;
            }
        }
    }
}

/// A typed word or its result, written only when the "text" option is on. The closure is not
/// called otherwise, so the text is never even formatted in normal work.
pub fn text(make: impl FnOnce() -> String) {
    if flag(Opt::Text) {
        write_log(&format!("{} TEXT {}", crate::report::now_ms(), make()));
    }
}

/// The same for text the cascade has not judged: the auto-replacements, the split tokens, the
/// Alt cycles and the selections. Nothing is written when a word of any of `texts` is one the
/// stop rules keep away (a password, an address, code), as on the cascade's own path.
pub fn text_checked(texts: &[&str], make: impl FnOnce() -> String) {
    if flag(Opt::Text)
        && !texts
            .iter()
            .flat_map(|text| text.split_whitespace())
            .any(|word| pulinto_core::stop_rules::check(word).is_some())
    {
        text(make);
    }
}

/// A line for the "decisions" option; the closure is called only when it is on.
pub fn decision(make: impl FnOnce() -> String) {
    if flag(Opt::Decisions) {
        write_log(&format!("{} DECISION {}", crate::report::now_ms(), make()));
    }
}

/// Called from the panic hook, which has already written the place of the panic.
pub fn panic_detail(info: &std::panic::PanicHookInfo<'_>) {
    if flag(Opt::Panic) {
        let backtrace = std::backtrace::Backtrace::force_capture();
        // try_lock: the panicking thread may be the one holding the log.
        if let Ok(mut slot) = SESSION.try_lock() {
            append(
                &mut slot,
                &format!("{} PANIC (detailed): {info}\n{backtrace}", crate::report::now_ms()),
            );
        }
    }
}

// ── The mode ─────────────────────────────────────────────────────────────

/// Switches the recording on with exactly these options (an empty list is refused) and starts
/// their timers anew.
pub fn start(options: &[Opt]) -> Result<(), String> {
    if options.is_empty() {
        return Err("no debug option selected".into());
    }
    let dir = make_dir()?;
    // Every file is opened before any option is switched on: a failure leaves the
    // recording exactly as it was.
    let keys = if options.contains(&Opt::Keys) {
        Some(open_private(&keys_path(&dir)).map_err(|error| error.to_string())?)
    } else {
        None
    };
    let mut slot = SESSION.lock().map_err(|_| "the debug log is busy")?;
    if slot.is_none() {
        let log = open_private(&log_path(&dir)).map_err(|error| error.to_string())?;
        let log_bytes = log.metadata().map(|meta| meta.len()).unwrap_or(0);
        *slot = Some(Session {
            log,
            log_bytes,
            until: [None; 4],
        });
    }
    let session = slot.as_mut().ok_or("the debug log is not open")?;
    let now = Instant::now();
    for opt in ALL {
        let on = options.contains(&opt);
        session.until[opt.index()] = on.then(|| now + opt.limit());
        FLAGS[opt.index()].store(on, Ordering::SeqCst);
    }
    pulinto_input::injector::set_key_trace(keys);
    RECORDING.store(true, Ordering::SeqCst);
    let ids: Vec<&str> = options.iter().map(|opt| opt.id()).collect();
    let line = format!(
        "{} Debug mode on: {}",
        crate::report::now_ms(),
        ids.join(", ")
    );
    let _ = writeln!(session.log, "{line}");
    drop(slot);
    if let Ok(mut ring) = RING.lock() {
        ring.push_back(line);
    }
    Ok(())
}

/// Stops recording, keeps the files.
fn end_recording() {
    pulinto_input::injector::set_key_trace(None);
    for flag in &FLAGS {
        flag.store(false, Ordering::SeqCst);
    }
    RECORDING.store(false, Ordering::SeqCst);
    if let Ok(mut slot) = SESSION.lock() {
        *slot = None;
    }
}

/// Once a second: options whose time is up are switched off; when none is left the recording
/// ends and the files wait for the user's decision. Returns true when the state changed.
pub fn tick() -> bool {
    if !recording() {
        return false;
    }
    let now = Instant::now();
    let mut remaining = false;
    if let Ok(mut slot) = SESSION.lock() {
        if let Some(session) = slot.as_mut() {
            for opt in ALL {
                let slot = &mut session.until[opt.index()];
                match slot {
                    Some(until) if *until <= now => {
                        *slot = None;
                        FLAGS[opt.index()].store(false, Ordering::SeqCst);
                        if opt == Opt::Keys {
                            pulinto_input::injector::set_key_trace(None);
                        }
                    }
                    Some(_) => remaining = true,
                    None => {}
                }
            }
        }
    }
    if !remaining {
        end_recording();
        return true;
    }
    false
}

fn files() -> Vec<PathBuf> {
    let Some(dir) = dir() else { return Vec::new() };
    ["debug.log.1", "debug.log", "keys.log"]
        .iter()
        .map(|name| dir.join(name))
        .filter(|path| path.is_file())
        .collect()
}

/// Files of a finished or interrupted recording wait for a decision.
pub fn leftover() -> bool {
    !recording() && !files().is_empty()
}

/// Removes files nobody decided about for a day.
pub fn forget_stale() {
    let stale = files().iter().all(|path| {
        path.metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > LEFTOVER_MAX_AGE)
    });
    if stale {
        delete_files();
    }
}

fn delete_files() {
    for path in files() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(dir) = dir() {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Seconds until the last running option stops.
pub fn seconds_left() -> u64 {
    let now = Instant::now();
    SESSION
        .lock()
        .ok()
        .and_then(|slot| {
            slot.as_ref().and_then(|session| {
                session
                    .until
                    .iter()
                    .flatten()
                    .map(|until| until.saturating_duration_since(now).as_secs())
                    .max()
            })
        })
        .unwrap_or(0)
}

fn civil_stamp(seconds: u64) -> String {
    let days = (seconds / 86400) as i64;
    let rest = seconds % 86400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Ends the recording, if it runs, and takes the files away: with `save`, after putting a copy
/// in the home directory for the user to read and send. Returns the path of the copy.
pub fn finish(save: bool) -> Result<Option<PathBuf>, String> {
    let home = ROOT_OVERRIDE.lock().ok().and_then(|root| root.clone());
    finish_into(save, home)
}

fn finish_into(save: bool, home: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    end_recording();
    let mut copy = None;
    if save && !files().is_empty() {
        let home = home
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .ok_or("no $HOME")?;
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or(0);
        let path = home.join(format!("pulinto-debug-{}.log", civil_stamp(seconds)));
        let mut out = open_private(&path).map_err(|error| error.to_string())?;
        for source in files() {
            let name = source.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            writeln!(out, "===== {name} =====").map_err(|error| error.to_string())?;
            let bytes = std::fs::read(&source).map_err(|error| error.to_string())?;
            out.write_all(&bytes).map_err(|error| error.to_string())?;
        }
        copy = Some(path);
    }
    delete_files();
    Ok(copy)
}

/// For the settings window, as JSON.
pub fn status_json() -> String {
    let ids: Vec<String> = ALL
        .into_iter()
        .filter(|opt| flag(*opt))
        .map(|opt| format!("\"{}\"", opt.id()))
        .collect();
    let bytes: u64 = files()
        .iter()
        .filter_map(|path| path.metadata().ok())
        .map(|meta| meta.len())
        .sum();
    format!(
        "{{\"recording\":{},\"options\":[{}],\"seconds_left\":{},\"bytes\":{},\"leftover\":{}}}",
        recording(),
        ids.join(","),
        seconds_left(),
        bytes,
        leftover()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_come_out_right() {
        assert_eq!(civil_stamp(0), "19700101-000000Z");
        assert_eq!(civil_stamp(1_792_567_500), "20261021-072500Z");
        assert_eq!(civil_stamp(951_782_400 + 3 * 3600 + 5), "20000229-030005Z");
    }

    #[test]
    fn options_round_trip_through_their_ids() {
        for opt in ALL {
            assert_eq!(Opt::parse(opt.id()), Some(opt));
        }
        assert_eq!(Opt::parse("keystrokes"), None);
    }

    /// One test for the whole life of a recording, since the mode is process-wide state.
    #[test]
    fn a_recording_lives_in_memory_and_leaves_nothing_behind() {
        use std::os::unix::fs::PermissionsExt;
        let runtime = std::env::temp_dir().join(format!("pulinto-debug-test-{}", std::process::id()));
        std::fs::create_dir_all(&runtime).unwrap();
        *ROOT_OVERRIDE.lock().unwrap() = Some(runtime.clone());

        assert!(start(&[]).is_err());
        text(|| panic!("the text is formatted although the option is off"));

        start(&[Opt::Text, Opt::Decisions]).unwrap();
        assert!(recording());
        text(|| "слово → word".into());
        decision(|| "оставить".into());
        record("1 строка вывода");
        // Text the cascade did not judge is held to the stop rules here.
        text_checked(&["привет Xk9#mQ2!vL"], || "пароль Xk9#mQ2!vL".into());
        text_checked(&["ghbdtn"], || "Alt: ghbdtn".into());
        let dir = runtime.join("pulinto-debug");
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(
            std::fs::metadata(dir.join("debug.log")).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let log = std::fs::read_to_string(dir.join("debug.log")).unwrap();
        assert!(log.contains("слово → word") && log.contains("оставить") && log.contains("1 строка вывода"));
        assert!(log.contains("Alt: ghbdtn") && !log.contains("Xk9#mQ2!vL"));
        assert!(status_json().contains("\"text\""));

        // Without the text option the words stay out of the file.
        start(&[Opt::Decisions]).unwrap();
        text(|| "секрет".into());
        assert!(!std::fs::read_to_string(dir.join("debug.log")).unwrap().contains("секрет"));

        let copy = finish(true).unwrap().unwrap();
        assert!(!recording() && !dir.exists());
        let saved = std::fs::read_to_string(&copy).unwrap();
        assert!(saved.contains("слово → word"));
        assert_eq!(std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777, 0o600);

        // A timer that runs out ends the recording but keeps the files for the user's decision.
        start(&[Opt::Text]).unwrap();
        text(|| "слово".into());
        SESSION.lock().unwrap().as_mut().unwrap().until[Opt::Text.index()] =
            Some(Instant::now() - Duration::from_secs(1));
        assert!(tick());
        assert!(!recording() && leftover());
        text(|| "после остановки".into());
        assert!(std::fs::read_to_string(dir.join("debug.log")).unwrap().contains("слово"));
        assert!(!std::fs::read_to_string(dir.join("debug.log")).unwrap().contains("после остановки"));
        assert_eq!(finish(false).unwrap(), None);
        assert!(!dir.exists() && !leftover());

        // A key trace that cannot be opened switches nothing on.
        std::fs::create_dir_all(dir.join("keys.log")).unwrap();
        assert!(start(&[Opt::Text, Opt::Keys]).is_err());
        assert!(!recording() && !flag(Opt::Text) && !flag(Opt::Keys));
        text(|| panic!("the text is formatted after a failed start"));
        let _ = std::fs::remove_dir_all(&runtime);
    }
}
