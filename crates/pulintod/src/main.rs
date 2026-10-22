//! PuLinTo daemon — entry point.
//!
//! Orchestrates D-Bus, system tray, and the key-event processing engine.

mod config;
mod dbus;
mod debug;
mod engine;
mod extensions;
mod gnome_keys;
mod grab_engine;
mod hotkeys;
mod i18n;
mod input_monitor;
mod instance_lock;
mod keyboard;
mod replace;
mod report;
mod scanword;
mod shell_focus;
mod tray;
mod xwindow;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use std::{collections::HashSet, path::PathBuf};

use tokio::sync::Mutex;
use zbus::connection::Builder;

use pulinto_core::autocorrect::AutoCorrectEngine;
use pulinto_core::exceptions::ExceptionManager;

/// Shared daemon state, accessible from D-Bus, tray, and the event loop.
///
/// The word being typed is deliberately *not* here: it belongs to whichever
/// input loop is running, and each keeps it as scan codes of its own.
pub struct SwitcherState {
    /// Index into `Mode::ALL`. Which mechanism is doing the correcting — and,
    /// just as importantly, which ones are therefore standing down.
    pub mode: AtomicU8,
    pub auto_enabled: AtomicBool,
    pub conversion_count: AtomicU64,
    pub exceptions: Mutex<ExceptionManager>,
    pub autocorrect: Mutex<AutoCorrectEngine>,
    /// One policy instance for every input mode and every control surface.
    pub app_policy: RwLock<xwindow::AppPolicy>,
    /// The hotkey sets, as the input loops and the settings window see them.
    pub hotkeys: hotkeys::HotkeyStore,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `pulintod --version` must not start a second daemon.
    if std::env::args().skip(1).any(|arg| arg == "--version" || arg == "-V") {
        println!("pulintod {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    stamp_output();
    println!("Starting PuLinTo daemon (pulintod)...");
    debug::forget_stale();
    std::thread::spawn(|| {
        // The tray menu counts the minutes left: it is redrawn once a minute, not more.
        let mut minutes_shown = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if debug::tick() {
                tray::refresh();
                keyboard::notify(i18n::tr(
                    "Debug mode stopped by the timer. The log is kept: save a copy or delete it from the tray menu.",
                ));
            } else if debug::recording() {
                let minutes = debug::seconds_left().div_ceil(60);
                if minutes != minutes_shown {
                    minutes_shown = minutes;
                    tray::refresh();
                }
            }
        }
    });
    // Before anything looks at a layout: the language packs on disk join the
    // built-in ones, and the tables are built once.
    pulinto_data::profile::init();

    let settings = config::Config::load();
    i18n::set_choice(i18n::Lang::parse(&settings.language));
    println!(
        "Mode: {} ({}), auto-replacement: {}",
        settings.mode.label(),
        settings.mode,
        if settings.auto_enabled {
            "on"
        } else {
            "off"
        }
    );

    // D-Bus uniqueness is session-local, but the physical keyboards are not.
    // Take ownership before publishing another tray/D-Bus service, otherwise a
    // stale daemon on another DISPLAY can keep the devices grabbed while this
    // process looks healthy but handles no keys.
    let _input_instance_lock =
        if settings.mode.daemon_handles_input() && input_session_may_capture() {
            match instance_lock::InputInstanceLock::acquire() {
                Ok(lock) => Some(lock),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    eprintln!(
                        "PuLinTo is already handling the physical keyboard in another instance; \
                         the second daemon has exited"
                    );
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            None
        };

    let app_policy = xwindow::AppPolicy::new();
    println!(
        "Application modes: {} system rules, {} auto, {} Alt only, {} off",
        xwindow::AppPolicy::default_disabled_apps().len(),
        app_policy.enabled_apps().len(),
        app_policy.manual_apps().len(),
        app_policy.disabled_apps().len()
    );
    println!(
        "Auto: {}; Alt only: {}; off: {}",
        xwindow::enabled_apps_path().display(),
        xwindow::manual_apps_path().display(),
        xwindow::excluded_apps_path().display()
    );

    let state = Arc::new(SwitcherState {
        mode: AtomicU8::new(mode_index(settings.mode)),
        auto_enabled: AtomicBool::new(settings.auto_enabled),
        conversion_count: AtomicU64::new(0),
        exceptions: Mutex::new(ExceptionManager::new()),
        autocorrect: Mutex::new(AutoCorrectEngine::with_user_dictionary()),
        app_policy: RwLock::new(app_policy),
        hotkeys: hotkeys::HotkeyStore::new(&settings.hotkeys),
    });

    // The keyboard's arrangement decides which position tables the profiles are
    // built with, so the controller comes first and `prepare` settles the
    // arrangement before them.
    let layout = Arc::new(pulinto_layout::create_default_layout_controller());
    println!("Layout controller: {}", layout.name());
    keyboard::prepare(&settings, layout.as_ref().as_ref());

    // ── D-Bus ────────────────────────────────────────────────────────────
    // Loaded once and shared: the decision engine and the D-Bus `Judge` method
    // read the same word lists.
    let mut profiles = pulinto_data::profile::all();
    pulinto_data::profile::retain_enabled(&mut profiles, &layout.layout_ids());
    let profiles = Arc::new(profiles);
    pulinto_data::profile::warn_if_incomplete(&profiles);

    // Reverted words and the last correction. Created here, not inside a mode,
    // so the two input loops cannot end up with separate copies — a word the
    // user takes back must stay taken back whichever loop is running.
    let learning = Arc::new(replace::Learning::default());

    let dbus_service = dbus::DaemonDBusService {
        state: Arc::clone(&state),
        profiles: Arc::clone(&profiles),
        layout: Arc::clone(&layout),
    };
    let _conn = Builder::session()?
        .name("ru.pulinto.Daemon1")?
        .serve_at("/ru/pulinto/Daemon1", dbus_service)?
        .build()
        .await?;
    println!("D-Bus interface ru.pulinto.Daemon1 registered.");

    // ── System tray ──────────────────────────────────────────────────────
    let tray_svc = ksni::TrayService::new(tray::SwitcherTray {
        state: Arc::clone(&state),
    });
    tray::register(tray_svc.handle());
    tray_svc.spawn();
    tokio::spawn(tray::keep_registered(_conn.clone()));
    // One step to install: the GNOME Shell extensions are switched on for the user,
    // once, from here (see `extensions`).
    std::thread::spawn(extensions::ensure_enabled_once);
    println!("Tray icon initialized via StatusNotifierItem.");

    // ── Input ────────────────────────────────────────────────────────────
    //
    // Exactly one mechanism runs; `Off` deliberately starts nothing.
    match settings.mode {
        mode if mode.daemon_handles_input() && !input_session_may_capture() => {
            eprintln!(
                "Input capture is disabled in a VNC session: otherwise a background DISPLAY might \
                 grab the physical keyboard before the active desktop does. \
                 To start on purpose, set PULINTO_ALLOW_REMOTE_INPUT=1."
            );
        }
        config::Mode::Off => {
            println!("Input is not captured. The daemon only answers requests.");
        }
        config::Mode::Grab => {
            if !start_grab_mode(&state, &profiles, &layout, &learning, &settings) {
                eprintln!(
                    "Grab mode did not start: no keyboard was grabbed. \
                     Check whether another pulintod instance is running."
                );
            }
        }
        config::Mode::Observe => {
            start_observe_mode(&state, &profiles, &layout, &learning, &settings, false);
        }
        config::Mode::Watch => {
            println!(
                "DIAGNOSTICS: reads everything, decides everything, TOUCHES NOTHING. \
                 For every word the layout it was read under is printed."
            );
            start_observe_mode(&state, &profiles, &layout, &learning, &settings, true);
        }
    }

    // ── Graceful shutdown ────────────────────────────────────────────────
    // SIGTERM is how the desktop and `kill` stop us; it must end the same way
    // as Ctrl-C, with the log drained.
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result?,
        _ = terminate.recv() => {}
    }
    println!("Shutting down pulintod daemon...");
    flush_output();

    Ok(())
}

/// Wayland session — decided once, at start, from the session itself rather
/// than from whether an X display answers: Xwayland always does.
pub fn is_wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|kind| kind.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// TigerVNC brings up a separate X session before the local desktop and runs
/// the same autostart. `/dev/input` is shared by both sessions: the first
/// daemon grabs the physical keyboard but controls the layout of someone
/// else's DISPLAY. By default such a session may show the tray and D-Bus but
/// has no right to read or change the system input.
/// Put the time, in milliseconds since the epoch, in front of every line the
/// daemon prints — whichever thread or crate prints it — when the output is a
/// log file or a pipe rather than a terminal.
///
/// Lines without a time («Hotkey», «Selection is stale») could not be
/// matched to the replacements around them. Both stdout and stderr go through
/// one pipe, so their order is kept; a thread copies lines to the original
/// descriptor and keeps the last of them in memory ([`debug::record`]).
///
/// There is no log file of our own: under systemd or the desktop's autostart the
/// output goes to the journal, and the lines about single words (`pulinto_core::diag!`)
/// are kept out of it and stay in memory.
/// The descriptor the output really goes to and the thread copying lines to
/// it, kept so [`flush_output`] can put the descriptor back and wait for the
/// last lines before the process exits or execs.
static OUTPUT_RELAY: std::sync::Mutex<Option<(i32, std::thread::JoinHandle<()>)>> =
    std::sync::Mutex::new(None);

/// Put stdout and stderr back onto the original descriptor and wait for the
/// copying thread to drain the pipe. Without this the last lines — the reason
/// for a shutdown, the restart announcement — were lost with the process, and
/// an `exec` would inherit a pipe nobody reads any more.
pub(crate) fn flush_output() {
    let taken = OUTPUT_RELAY.lock().ok().and_then(|mut slot| slot.take());
    if let Some((original, relay)) = taken {
        // SAFETY: the descriptors are ours; the pipe's write end closes with
        // the last dup2 and the relay thread sees end of file.
        unsafe {
            libc::dup2(original, 1);
            libc::dup2(original, 2);
        }
        // A child that inherited the pipe (one spawned before `child_output`
        // existed, or by a crate that does not know about it) keeps it open,
        // and the relay would never see end of file. The exit must not hang
        // on that: whatever reached the pipe by now is drained, the rest is
        // the child's own output.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while !relay.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if relay.is_finished() {
            let _ = relay.join();
        } else {
            eprintln!("Log not fully read: the output pipe is held by a child process");
        }
    }
}

/// Where a child the daemon starts should write: the daemon's own output as it
/// was before [`stamp_output`] put the relay in front of it. Inheriting the
/// relay's pipe instead would keep it open for as long as the child lives,
/// and the daemon's exit would wait on the child.
pub(crate) fn child_output() -> (std::process::Stdio, std::process::Stdio) {
    use std::os::unix::io::FromRawFd;
    let original = OUTPUT_RELAY
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|(fd, _)| *fd));
    let Some(fd) = original else {
        return (
            std::process::Stdio::inherit(),
            std::process::Stdio::inherit(),
        );
    };
    // SAFETY: duplicates of a descriptor we own, each handed to exactly one
    // `Stdio`, which closes it.
    unsafe {
        let out = libc::dup(fd);
        let err = libc::dup(fd);
        if out < 0 || err < 0 {
            if out >= 0 {
                libc::close(out);
            }
            if err >= 0 {
                libc::close(err);
            }
            return (
                std::process::Stdio::inherit(),
                std::process::Stdio::inherit(),
            );
        }
        (
            std::process::Stdio::from_raw_fd(out),
            std::process::Stdio::from_raw_fd(err),
        )
    }
}

fn stamp_output() {
    use std::io::{BufRead, Write};
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::io::FromRawFd;

    // SAFETY: plain descriptor calls on our own stdout/stderr; the pipe ends
    // and the duplicate are owned exactly once, by the File values below.
    unsafe {
        if libc::isatty(1) == 1 {
            return;
        }
        let mut pipe = [0; 2];
        if libc::pipe(pipe.as_mut_ptr()) != 0 {
            return;
        }
        let original = libc::dup(1);
        if original < 0 || libc::dup2(pipe[1], 1) < 0 || libc::dup2(pipe[1], 2) < 0 {
            return;
        }
        libc::close(pipe[1]);
        // A panic is written straight to the original descriptor: the
        // message must not depend on the thread it would otherwise go through.
        let panic_fd = libc::dup(original);
        if panic_fd >= 0 {
            // Only the place of the panic: its message may quote the text being
            // handled (a failed slice of a word prints the word), and the log
            // must never hold what the user typed. The default hook is not called
            // for the same reason.
            std::panic::set_hook(Box::new(move |info| {
                let place = info
                    .location()
                    .map(|at| format!("{}:{}", at.file(), at.line()))
                    .unwrap_or_else(|| "?".into());
                let line = format!("{} PANIC: {place}\n", report::now_ms());
                // A plain write to a descriptor we own for this purpose.
                libc::write(panic_fd, line.as_ptr().cast(), line.len());
                debug::panic_detail(info);
            }));
        }
        let reader = std::io::BufReader::new(std::fs::File::from_raw_fd(pipe[0]));
        let mut sink = std::fs::File::from_raw_fd(original);
        let chosen_by_caller = sink
            .metadata()
            .map(|meta| meta.file_type().is_file() || meta.file_type().is_char_device())
            .unwrap_or(true);
        if !chosen_by_caller {
            // The output goes to the journal: the lines about single words stay in memory.
            pulinto_core::diag::set_sink(debug::event_line);
        }
        let relay = std::thread::spawn(move || {
            for line in reader.split(b'\n') {
                let Ok(line) = line else { return };
                let mut stamped = format!("{} ", report::now_ms()).into_bytes();
                stamped.extend_from_slice(&line);
                debug::record(&String::from_utf8_lossy(&stamped));
                stamped.push(b'\n');
                // One write per line, so lines of the relay and of the sink never mix.
                if sink.write_all(&stamped).is_err() {
                    return;
                }
            }
        });
        if let Ok(mut slot) = OUTPUT_RELAY.lock() {
            *slot = Some((original, relay));
        }
    }
}

fn input_session_may_capture() -> bool {
    session_flags_may_capture(
        std::env::var_os("VNCDESKTOP").is_some(),
        std::env::var_os("PULINTO_ALLOW_REMOTE_INPUT").as_deref()
            == Some(std::ffi::OsStr::new("1")),
    )
}

fn session_flags_may_capture(vnc: bool, explicit_override: bool) -> bool {
    !vnc || explicit_override
}

pub fn mode_index(mode: config::Mode) -> u8 {
    config::Mode::ALL
        .iter()
        .position(|candidate| *candidate == mode)
        .unwrap_or(0) as u8
}

pub fn mode_from_index(index: u8) -> config::Mode {
    config::Mode::ALL
        .get(index as usize)
        .copied()
        .unwrap_or_default()
}

/// Effective policy for the window that owns keyboard focus. Failure to ask
/// X11 is not treated as a blanket ban (that would disable every Wayland
/// session), but it is kept explicit so callers can fall back deliberately.
pub fn focused_app_decision(
    state: &SwitcherState,
    windows: Option<&xwindow::XWindows>,
) -> Option<(String, xwindow::AppDecision)> {
    let class = windows?.focused_class()?;
    let decision = state.app_policy.read().ok()?.decision(&class);
    Some((class, decision))
}

pub fn focused_app_is_enabled(state: &SwitcherState, windows: Option<&xwindow::XWindows>) -> bool {
    focused_app_allows_automatic(state, windows)
}

pub fn focused_app_allows_automatic(
    state: &SwitcherState,
    windows: Option<&xwindow::XWindows>,
) -> bool {
    focused_app_decision(state, windows)
        .map(|(_, decision)| decision.allows_automatic())
        .unwrap_or(true)
}

pub fn focused_app_allows_manual(
    state: &SwitcherState,
    windows: Option<&xwindow::XWindows>,
) -> bool {
    focused_app_decision(state, windows)
        .map(|(_, decision)| decision.allows_manual())
        .unwrap_or(true)
}

/// Toggle the focused application persistently. Without a readable WM_CLASS
/// (notably a pure Wayland session), fall back to the global auto-correction
/// switch so the hotkey always remains useful.
pub fn toggle_current_application(
    state: &SwitcherState,
    windows: Option<&xwindow::XWindows>,
) -> Result<(String, xwindow::AppMode), String> {
    if let Some(class) = windows.and_then(xwindow::XWindows::focused_class) {
        let mode = state
            .app_policy
            .write()
            .map_err(|_| i18n::tr("The application policy is locked").to_string())?
            .cycle(&class)
            .map_err(|error| error.to_string())?;
        println!("PuLinTo for {class}: {}", mode.label());
        return Ok((class, mode));
    }

    let enabled = !state.auto_enabled.fetch_xor(true, Ordering::SeqCst);
    config::Config::update(|settings| settings.auto_enabled = enabled)
        .map_err(|error| i18n::tr("Could not save the global state: {error}").replace("{error}", &error.to_string()))?;
    println!(
        "PuLinTo globally {}: WM_CLASS of the current window is unavailable",
        if enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    Ok((
        i18n::tr("all applications").into(),
        if enabled {
            xwindow::AppMode::Auto
        } else {
            xwindow::AppMode::Off
        },
    ))
}

/// Persist the new mode and restart into it.
///
/// The input loop cannot be swapped while it runs: grab mode owns file
/// descriptors and observe mode owns reader threads, and unwinding either
/// safely mid-flight is more risk than it is worth for a setting changed once
/// in a while. Re-exec is honest and total — the kernel drops any grab when the
/// descriptors close, so we always come back up clean.
pub fn switch_mode(state: &SwitcherState, mode: config::Mode) -> ! {
    let auto_enabled = state.auto_enabled.load(Ordering::SeqCst);
    if let Err(error) = config::Config::update(|settings| {
        settings.mode = mode;
        settings.auto_enabled = auto_enabled;
    }) {
        eprintln!("Could not save the settings: {error}");
    }
    println!("Switching to mode «{}», restarting…", mode.label());

    use std::os::unix::process::CommandExt;
    let executable = config::Config::daemon_executable();
    // The new image must inherit the real log, not our pipe: its reader dies
    // with this image's threads.
    flush_output();
    let error = std::process::Command::new(executable).exec();
    eprintln!("Restart failed: {error}");
    std::process::exit(1);
}

fn start_observe_mode(
    state: &Arc<SwitcherState>,
    profiles: &Arc<Vec<pulinto_data::profile::Profile>>,
    layout: &Arc<Box<dyn pulinto_layout::LayoutController>>,
    learning: &Arc<replace::Learning>,
    settings: &config::Config,
    dry_run: bool,
) {
    // The same injector grab mode uses. Observe mode used to have an injection
    // path of its own, and that second path is where the swallowed keystrokes
    // came from: one mechanic, one set of delays, one place to fix.
    let injector = match pulinto_input::injector::Injector::new(settings.timing()) {
        Ok(injector) => Arc::new(injector),
        Err(error) => {
            eprintln!("Observe mode unavailable: cannot open /dev/uinput ({error}).");
            return;
        }
    };

    let ctx = Arc::new(engine::EventContext {
        state: Arc::clone(state),
        injector,
        layout: Arc::clone(layout),
        profiles: Arc::clone(profiles),
        word: std::sync::Mutex::new(scanword::ScanWord::default()),
        held_modifiers: std::sync::Mutex::new(Vec::new()),
        shift_down: AtomicBool::new(false),
        recent_langs: std::sync::Mutex::new(Vec::new()),
        learning: Arc::clone(learning),
        pending: Arc::new(engine::Pending::default()),
        tap: std::sync::Mutex::new(hotkeys::TapTracker::default()),
        reports: Arc::new(report::Registry::default()),
        windows: xwindow::XWindows::connect().map(xwindow::XWindows::following_focus),
        rhythm: std::sync::Mutex::new(engine::Rhythm::default()),
        dry_run,
    });

    let (events, devices) = pulinto_input::capture::start();
    if devices.is_empty() {
        eprintln!("Could not open keyboards in /dev/input (the input group is required).");
        return;
    }
    println!("Capture: evdev, devices: {}", devices.join(", "));

    // Two threads, and the split is the point. The reader does nothing that
    // can block, so no keystroke is ever missed while a correction is being
    // typed; the processor takes its time and catches up afterwards.
    let pending = Arc::clone(&ctx.pending);
    std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            pending.push(event);
        }
        eprintln!("The keyboard thread has closed.");
    });
    std::thread::spawn(move || engine::process_pending(&ctx));
}

fn start_grab_mode(
    state: &Arc<SwitcherState>,
    profiles: &Arc<Vec<pulinto_data::profile::Profile>>,
    layout: &Arc<Box<dyn pulinto_layout::LayoutController>>,
    learning: &Arc<replace::Learning>,
    settings: &config::Config,
) -> bool {
    let injector = match pulinto_input::injector::Injector::new(settings.timing()) {
        Ok(injector) => Arc::new(injector),
        Err(error) => {
            eprintln!("Grab mode unavailable: cannot open /dev/uinput ({error}).");
            return false;
        }
    };

    // Under Wayland the X server is Xwayland: XTest typing would reach only
    // X windows, and XInput sees clicks only over them. There the replacement
    // goes by scan codes with the layout switch in the same key stream, and
    // clicks are read from the kernel. The X11 path does not change.
    let wayland = is_wayland_session();
    let xinject = if wayland {
        println!(
            "Wayland session: replacement by scan codes, layout through {}",
            layout.name()
        );
        None
    } else {
        match pulinto_input::xtest_unicode::XTestUnicodeBackend::new() {
            Ok(backend) => Some(Arc::new(backend)),
            Err(error) => {
                eprintln!("XTest delivery unavailable ({error}), replacement will go by scan codes");
                None
            }
        }
    };
    let selection = if wayland {
        pulinto_input::xtest_unicode::XTestUnicodeBackend::selection_only()
            .map(Arc::new)
            .map_err(|error| eprintln!("Selection unavailable ({error}): Alt on a selection is off"))
            .ok()
    } else {
        xinject.clone()
    };
    // The virtual stand (`PULINTO_NO_POINTER=1`) watches no pointers: the
    // person at the machine clicking during a run would otherwise reset the
    // word buffer mid-word and spoil the count. The Xvfb harness, which also
    // grabs by name, keeps its pointers — it tests the click reset itself.
    let pointer_clicks = if std::env::var_os("PULINTO_NO_POINTER").is_some() {
        None
    } else if wayland {
        xwindow::PointerClicks::start_evdev()
    } else {
        xwindow::PointerClicks::start()
    };

    let ctx = Arc::new(grab_engine::GrabContext {
        state: Arc::clone(state),
        profiles: Arc::clone(profiles),
        injector,
        layout: Arc::clone(layout),
        learning: Arc::clone(learning),
        xinject,
        selection,
        reports: Arc::new(report::Registry::default()),
        pointer_clicks,
        windows: xwindow::XWindows::connect().map(xwindow::XWindows::following_focus),
        heartbeats: std::sync::Mutex::new(Vec::new()),
    });
    grab_engine::watchdog(Arc::clone(&ctx));

    let active = Arc::new(std::sync::Mutex::new(HashSet::<PathBuf>::new()));
    let grabbed = attach_new_keyboards(&ctx, &active);
    if grabbed == 0 {
        eprintln!("Grab mode: no keyboards found.");
    }

    // Bluetooth HID devices routinely disappear while the computer is idle
    // and return as a fresh /dev/input/event* device on the next key press.
    // Wait for the kernel's device notification: unlike periodic scanning,
    // this thread sleeps in recv(2) and consumes no CPU while nothing changes.
    let monitor_ctx = Arc::clone(&ctx);
    let monitor_active = Arc::clone(&active);
    match input_monitor::InputMonitor::new() {
        Ok(mut monitor) => {
            std::thread::spawn(move || loop {
                if let Err(error) = monitor.wait() {
                    eprintln!("Keyboard monitor stopped: {error}");
                    return;
                }
                // inotify reports creation at the VFS boundary. Give udev a
                // moment to finish permissions and HID capabilities, then do
                // a few bounded retries only for this actual device event.
                for _ in 0..5 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if attach_new_keyboards(&monitor_ctx, &monitor_active) > 0 {
                        break;
                    }
                }
            });
        }
        Err(error) => eprintln!(
            "Auto-pickup of connected keyboards is unavailable: {error}. \
             Use «Re-check keyboards» in the settings."
        ),
    }

    if grabbed > 0 {
        println!(
            "WARNING: keyboards grabbed: {grabbed}. \
             Emergency release — Backspace+Esc+Enter at once."
        );
    }
    // Even with no keyboard at this instant the hotplug monitor is healthy and
    // will attach one when Bluetooth reconnects.
    true
}

/// A key was held when the grab was due. Try again every couple of seconds —
/// for ten minutes at most — until it is let go; one retrier at a time.
fn retry_attach(
    ctx: &Arc<grab_engine::GrabContext>,
    active: &Arc<std::sync::Mutex<HashSet<PathBuf>>>,
) {
    static RETRYING: AtomicBool = AtomicBool::new(false);
    if RETRYING.swap(true, Ordering::SeqCst) {
        return;
    }
    let (ctx, active) = (Arc::clone(ctx), Arc::clone(active));
    std::thread::spawn(move || {
        for _ in 0..300 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if attach_new_keyboards(&ctx, &active) > 0 {
                break;
            }
        }
        RETRYING.store(false, Ordering::SeqCst);
    });
}

fn attach_new_keyboards(
    ctx: &Arc<grab_engine::GrabContext>,
    active: &Arc<std::sync::Mutex<HashSet<PathBuf>>>,
) -> usize {
    let mut grabbed = 0usize;
    // Test-stand lever: grab only the typist's keyboard, so that a run does
    // not take the keyboard from a human (who must be able to at least unlock
    // the screen). Not set on a live session.
    let only = std::env::var("PULINTO_GRAB_ONLY").ok();
    for (path, device) in pulinto_input::capture::keyboards() {
        if only
            .as_deref()
            .is_some_and(|name| device.name() != Some(name))
        {
            continue;
        }
        let already_active = active
            .lock()
            .map(|paths| paths.contains(&path))
            .unwrap_or(true);
        if already_active {
            continue;
        }
        match pulinto_input::grab::GrabbedKeyboard::open(device) {
            Ok(keyboard) => {
                grabbed += 1;
                if let Ok(mut paths) = active.lock() {
                    paths.insert(path.clone());
                }
                let ctx = Arc::clone(ctx);
                let active = Arc::clone(active);
                std::thread::spawn(move || {
                    let exit = grab_engine::run(ctx, keyboard);
                    if exit == grab_engine::RunExit::Disconnected {
                        if let Ok(mut paths) = active.lock() {
                            paths.remove(&path);
                        }
                    }
                });
            }
            Err(error) => {
                eprintln!("Could not grab {}: {error}", path.display());
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    retry_attach(ctx, active);
                }
            }
        }
    }
    grabbed
}

#[cfg(test)]
mod session_tests {
    use super::session_flags_may_capture;

    #[test]
    fn vnc_cannot_steal_the_physical_keyboard_by_default() {
        assert!(!session_flags_may_capture(true, false));
        assert!(session_flags_may_capture(true, true));
        assert!(session_flags_may_capture(false, false));
    }
}
