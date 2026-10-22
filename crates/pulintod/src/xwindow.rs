//! Which application has the keyboard.
//!
//! Needed for the one protection this daemon does not have. macOS turns event
//! taps off by itself when a password field is focused — Secure Input — and the
//! switchers there additionally skip terminals, editors and password managers
//! by hand. On Linux nothing of the sort exists: in grab mode we read every
//! keystroke in every window, password fields included.
//!
//! A switcher that has met real applications ends up keeping four separate
//! application lists: excluded outright, needing a delay before injection,
//! unable to cope with a synthetic key release, and remembering their own
//! layout. That is not over-engineering, it is what contact with real
//! applications does. This is the first of those lists.
//!
//! `WM_CLASS` is the identifier used, because it is the one a user can look up
//! (`xprop WM_CLASS`) and the one other switchers' lists are written in.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xinput;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, CreateWindowAux, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

/// Monotonic notification that the mouse may have moved the caret or focus.
///
/// Keyboard capture cannot see pointer clicks. Without this side channel a
/// partial word typed in one field survives a click into another field, and
/// the next delimiter can make PuLinTo erase or replace text against a stale
/// cursor position. XI2 raw events observe clicks globally without grabbing or
/// modifying them.
pub struct PointerClicks {
    generation: Arc<AtomicU64>,
    /// When the last click happened, taken as it arrives: a selection is live
    /// for Alt only if PRIMARY changed after this.
    last: Arc<Mutex<Option<Instant>>>,
}

fn stamp(generation: &AtomicU64, last: &Mutex<Option<Instant>>) {
    generation.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut slot) = last.lock() {
        *slot = Some(Instant::now());
    }
}

impl PointerClicks {
    pub fn start() -> Option<Self> {
        let (conn, screen_index) = x11rb::connect(None).ok()?;
        xinput::xi_query_version(&conn, 2, 0).ok()?.reply().ok()?;
        let root = conn.setup().roots.get(screen_index)?.root;
        let mask = xinput::EventMask {
            deviceid: xinput::Device::ALL.into(),
            mask: vec![xinput::XIEventMask::RAW_BUTTON_PRESS],
        };
        xinput::xi_select_events(&conn, root, &[mask])
            .ok()?
            .check()
            .ok()?;
        conn.flush().ok()?;

        let generation = Arc::new(AtomicU64::new(0));
        let last = Arc::new(Mutex::new(None));
        let watched = Arc::clone(&generation);
        let stamped = Arc::clone(&last);
        println!("XInput: mouse clicks reset the word buffer");
        std::thread::spawn(move || loop {
            match conn.wait_for_event() {
                Ok(Event::XinputRawButtonPress(event)) => {
                    stamp(&watched, &stamped);
                    if event.detail == 2 {
                        println!(
                            "XInput: middle mouse button — the application may paste X11 PRIMARY"
                        );
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("XInput: click watching stopped ({error})");
                    return;
                }
            }
        });
        Some(Self { generation, last })
    }

    /// The same notification from the kernel's own pointer devices, for
    /// Wayland: there the X server is Xwayland, which hears only clicks that
    /// land in X windows, and a click into any Wayland window would go unseen.
    /// Read-only, no grab. Devices plugged in later (Bluetooth mice that wake
    /// up) are picked up from the kernel's device announcements.
    pub fn start_evdev() -> Option<Self> {
        let generation = Arc::new(AtomicU64::new(0));
        let last: Arc<Mutex<Option<Instant>>> = Arc::default();
        let watched: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
        let found = attach_pointers(&generation, &last, &watched);
        println!("evdev: mouse clicks reset the word buffer (devices: {found})");

        let monitor_generation = Arc::clone(&generation);
        let monitor_last = Arc::clone(&last);
        if let Ok(mut monitor) = crate::input_monitor::InputMonitor::new() {
            std::thread::spawn(move || loop {
                if monitor.wait().is_err() {
                    return;
                }
                // udev needs a moment to set permissions on the new node.
                std::thread::sleep(Duration::from_millis(300));
                attach_pointers(&monitor_generation, &monitor_last, &watched);
            });
        }
        Some(Self { generation, last })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// When the last click happened; `None` before the first.
    pub fn last_click(&self) -> Option<Instant> {
        self.last.lock().ok().and_then(|slot| *slot)
    }
}

/// Start a reader for every pointer device not yet watched. Returns how many
/// are watched now.
fn attach_pointers(
    generation: &Arc<AtomicU64>,
    last: &Arc<Mutex<Option<Instant>>>,
    watched: &Arc<Mutex<HashSet<PathBuf>>>,
) -> usize {
    use evdev::{InputEventKind, Key};
    let clicks = [
        Key::BTN_LEFT,
        Key::BTN_RIGHT,
        Key::BTN_MIDDLE,
        Key::BTN_TOUCH,
    ];
    let Ok(entries) = fs::read_dir("/dev/input") else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_event = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("event"));
        if !is_event || watched.lock().map_or(true, |set| set.contains(&path)) {
            continue;
        }
        let Ok(mut device) = evdev::Device::open(&path) else {
            continue;
        };
        let has_buttons = device
            .supported_keys()
            .is_some_and(|keys| clicks.iter().any(|key| keys.contains(*key)));
        if !has_buttons {
            continue;
        }
        if let Ok(mut set) = watched.lock() {
            set.insert(path.clone());
        }
        let generation = Arc::clone(generation);
        let last = Arc::clone(last);
        let watched = Arc::clone(watched);
        std::thread::spawn(move || {
            while let Ok(events) = device.fetch_events() {
                for event in events {
                    if let InputEventKind::Key(key) = event.kind() {
                        if event.value() == 1 && clicks.contains(&key) {
                            stamp(&generation, &last);
                        }
                    }
                }
            }
            if let Ok(mut set) = watched.lock() {
                set.remove(&path);
            }
        });
    }
    watched.lock().map(|set| set.len()).unwrap_or(0)
}

pub struct XWindows {
    conn: RustConnection,
    /// Unmapped window of our own. A selection transfer has to be delivered
    /// somewhere, and it may not be a window belonging to somebody else.
    owner: Window,
    utf8: Atom,
    transfer: Atom,
    /// Under Wayland the focus comes from GNOME Shell (the PuLinTo focus
    /// extension); X knows only X windows there.
    shell: Option<crate::shell_focus::ShellFocus>,
    /// Focus changes under X11, counted and stamped from `_NET_ACTIVE_WINDOW`
    /// on the root window; under Wayland the Shell extension keeps the same
    /// pair inside `shell`.
    focus: FocusChanges,
}

/// A monotonic count of focus changes and when the last one happened.
///
/// Keyboard capture does not see the window change under Alt+Tab or a launcher
/// any more than it sees a click: without this, the word being typed, the last
/// correction and the selection of the old window all survive into the new one,
/// and the first Alt there rewrites text that is no longer under the caret.
#[derive(Clone, Default)]
pub struct FocusChanges {
    generation: Arc<AtomicU64>,
    last: Arc<Mutex<Option<Instant>>>,
    /// Whether somebody is watching at all: false under Wayland while the
    /// PuLinTo extension is off, and wherever the X watch could not start.
    watching: Arc<AtomicBool>,
}

impl FocusChanges {
    pub fn note(&self) {
        stamp(&self.generation, &self.last);
    }

    /// A window change would be seen. When it would not, nothing remembered
    /// about a window (a selection, the last replacement) can be trusted for
    /// long: the user may already be typing somewhere else.
    pub fn watching(&self) -> bool {
        self.watching.load(Ordering::SeqCst)
    }

    pub fn set_watching(&self, on: bool) {
        self.watching.store(on, Ordering::SeqCst);
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn last(&self) -> Option<Instant> {
        self.last.lock().ok().and_then(|slot| *slot)
    }

    /// Count every change of `_NET_ACTIVE_WINDOW`, on a connection of its own.
    fn follow_x11(&self) {
        let (conn, screen_index) = match x11rb::connect(None) {
            Ok(connected) => connected,
            Err(_) => return,
        };
        let Some(root) = conn
            .setup()
            .roots
            .get(screen_index)
            .map(|screen| screen.root)
        else {
            return;
        };
        let Some(active) = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.atom)
        else {
            return;
        };
        let selected = conn
            .change_window_attributes(
                root,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(x11rb::protocol::xproto::EventMask::PROPERTY_CHANGE),
            )
            .ok()
            .and_then(|cookie| cookie.check().ok())
            .is_some()
            && conn.flush().is_ok();
        if !selected {
            return;
        }
        let changes = self.clone();
        changes.set_watching(true);
        println!("X11: a change of the active window resets the word buffer");
        std::thread::spawn(move || loop {
            match conn.wait_for_event() {
                Ok(Event::PropertyNotify(event)) if event.atom == active => changes.note(),
                Ok(_) => {}
                Err(error) => {
                    changes.set_watching(false);
                    eprintln!("X11: active window watching stopped ({error})");
                    return;
                }
            }
        });
    }
}

impl XWindows {
    /// Connect, or report that we cannot — Wayland, no display, a server that
    /// refuses us. Every caller has to keep working without this.
    pub fn connect() -> Option<Self> {
        let (conn, screen_index) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_index)?.root;

        let owner = conn.generate_id().ok()?;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            owner,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .ok()?;

        let utf8 = conn
            .intern_atom(false, b"UTF8_STRING")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let transfer = conn
            .intern_atom(false, b"PULINTO_SELECTION")
            .ok()?
            .reply()
            .ok()?
            .atom;
        conn.flush().ok()?;
        Some(Self {
            conn,
            owner,
            utf8,
            transfer,
            shell: None,
            focus: FocusChanges::default(),
        })
    }

    /// For a long-lived input loop under Wayland: keep the focus current from
    /// the Shell's announcements, so asking for it never waits on D-Bus.
    pub fn following_focus(mut self) -> Self {
        if crate::is_wayland_session() {
            self.shell = Some(crate::shell_focus::ShellFocus::start());
        } else {
            self.focus.follow_x11();
        }
        self
    }

    /// How many times the focus has moved to another window, and when it last
    /// did. Zero and `None` until a change is seen.
    pub fn focus_changes(&self) -> &FocusChanges {
        match &self.shell {
            Some(shell) => shell.focus_changes(),
            None => &self.focus,
        }
    }

    /// The text the user has highlighted, if any.
    ///
    /// This is the X PRIMARY selection — what highlighting with the mouse or
    /// Shift+arrows puts there — and **not** the clipboard. Reading the
    /// clipboard instead would mean overwriting whatever the user had copied,
    /// which is how RuSwitcher on macOS ended up restoring the pasteboard two
    /// seconds later and hoping nobody noticed.
    ///
    /// The owning application answers this, so it can refuse, answer in a
    /// format we did not ask for, or not answer at all; a fifth of a second is
    /// as long as we are prepared to wait for text nobody is required to give
    /// us.
    pub fn selection(&self) -> Option<String> {
        self.conn.delete_property(self.owner, self.transfer).ok()?;
        self.conn
            .convert_selection(
                self.owner,
                AtomEnum::PRIMARY.into(),
                self.utf8,
                self.transfer,
                x11rb::CURRENT_TIME,
            )
            .ok()?;
        self.conn.flush().ok()?;

        let deadline = Instant::now() + Duration::from_millis(200);
        loop {
            match self.conn.poll_for_event() {
                Ok(Some(Event::SelectionNotify(notify))) => {
                    // A null property is the owner saying "not in that format",
                    // or nobody owning a selection at all.
                    if notify.property == x11rb::NONE {
                        return None;
                    }
                    break;
                }
                Ok(Some(_)) => continue,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return None,
            }
        }

        let reply = self
            .conn
            .get_property(true, self.owner, self.transfer, self.utf8, 0, 1 << 16)
            .ok()?
            .reply()
            .ok()?;
        if reply.value.is_empty() {
            return None;
        }
        String::from_utf8(reply.value).ok()
    }

    /// `WM_CLASS` of the window holding the keyboard focus.
    ///
    /// The focused window is often an input-only child that carries no class of
    /// its own, so the tree is walked upwards until one appears — the same
    /// thing `xprop` does when you click on a window.
    pub fn focused_class(&self) -> Option<String> {
        let from_shell = match &self.shell {
            Some(shell) => shell.focused_class(),
            // A one-off question (tray, D-Bus) may wait for the Shell.
            None if crate::is_wayland_session() => crate::shell_focus::ask(),
            None => None,
        };
        if let Some(answer) = from_shell {
            return answer;
        }
        let focus = self.conn.get_input_focus().ok()?.reply().ok()?.focus;
        let mut window = focus;
        for _ in 0..8 {
            if let Some(class) = self.class_of(window) {
                return Some(class);
            }
            let tree = self.conn.query_tree(window).ok()?.reply().ok()?;
            if tree.parent == x11rb::NONE || tree.parent == window {
                return None;
            }
            window = tree.parent;
        }
        None
    }

    fn class_of(&self, window: Window) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        if reply.value.is_empty() {
            return None;
        }
        // WM_CLASS is two NUL-terminated strings: instance, then class. The
        // class is the stable one — `Navigator` stays `Navigator` however the
        // binary is invoked — so it is the one users write in their lists.
        let mut parts = reply
            .value
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty());
        let instance = parts.next();
        let class = parts.next().or(instance)?;
        Some(String::from_utf8_lossy(class).into_owned())
    }
}

/// Places where PuLinTo must not inspect or alter text at all. The class names
/// are matched case-insensitively; a trailing `*` is a prefix rule.
const DEFAULT_OFF: &[&str] = &[
    // Password stores.
    "1password",
    "bitwarden",
    "keepassxc",
    "org.gnome.seahorse.application",
    // The same applications as Wayland clients report themselves (app id).
    "com.bitwarden.desktop",
    "org.keepassxc.keepassxc",
    // GNOME Shell's own input under Wayland — the lock screen, polkit and
    // keyring password prompts, the overview search — as the PuLinTo focus
    // extension reports it. Unlocking the screen must not be "corrected".
    "gnome-shell",
    // PuLinTo's own editor must never rewrite a rule while it is being typed.
    "pulinto-settings",
    "pulinto settings",
    "ru.pulinto.settings",
    // Remote desktops, VMs and games: forwarding a manual rewrite into a
    // second input stack is unsafe, so these are fully off by default.
    "freerdp",
    "org.remmina.remmina",
    "qemu-system-*",
    "remmina",
    "steam",
    "virt-manager",
    "virtualbox machine",
    "vncviewer",
    "wlfreerdp",
    "xfreerdp",
];

/// Applications where automatic rewriting is surprising, but the user's
/// explicit Alt action remains useful.
const DEFAULT_MANUAL: &[&str] = &[
    // Terminals and shells.
    "alacritty",
    "com.mitchellh.ghostty",
    "foot",
    "gnome-terminal-server",
    "kitty",
    "konsole",
    "org.gnome.terminal",
    "org.gnome.console",
    "org.gnome.ptyxis",
    "org.wezfurlong.wezterm",
    "terminator",
    "tilix",
    "wezterm",
    "xfce4-terminal",
    "xterm",
    // Editors and IDEs.
    "android-studio",
    "code",
    "code-oss",
    "codium",
    "emacs",
    "jetbrains-*",
    "sublime_text",
];

const DISABLED_FILE: &str = "excluded-apps.txt";
const ENABLED_FILE: &str = "enabled-apps.txt";
const MANUAL_FILE: &str = "manual-apps.txt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Auto,
    Manual,
    Off,
}

impl AppMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "auto" | "automatic" => Some(Self::Auto),
            "manual" | "alt" => Some(Self::Manual),
            "off" | "disabled" => Some(Self::Off),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
            Self::Off => "off",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => crate::i18n::tr("Automatic"),
            Self::Manual => crate::i18n::tr("Hotkey only"),
            Self::Off => crate::i18n::tr("Completely off"),
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Auto => Self::Manual,
            Self::Manual => Self::Off,
            Self::Off => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySource {
    DefaultAuto,
    DefaultManual,
    DefaultOff,
    UserAuto,
    UserManual,
    UserOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppDecision {
    pub mode: AppMode,
    pub source: PolicySource,
}

impl AppDecision {
    pub fn reason(self) -> &'static str {
        match self.source {
            PolicySource::DefaultAuto => "an ordinary application",
            PolicySource::DefaultManual => "automatic correction is safely off by default",
            PolicySource::DefaultOff => "fully off by default",
            PolicySource::UserAuto => "automatic mode chosen by the user",
            PolicySource::UserManual => "manual mode chosen by the user",
            PolicySource::UserOff => "turned off by the user",
        }
    }

    pub fn allows_automatic(self) -> bool {
        self.mode == AppMode::Auto
    }

    pub fn allows_manual(self) -> bool {
        self.mode != AppMode::Off
    }
}

/// Effective per-application policy, shared by grab, observe, tray and D-Bus.
///
/// User decisions always win over shipped defaults. Thus an IDE can be
/// safe-off on first launch and explicitly enabled afterwards, while a user can
/// also disable PuLinTo in any ordinary application that was not pre-listed.
#[derive(Debug, Clone)]
pub struct AppPolicy {
    default_manual: HashSet<String>,
    default_off: HashSet<String>,
    disabled: HashSet<String>,
    manual: HashSet<String>,
    enabled: HashSet<String>,
    data_dir: Option<PathBuf>,
}

impl Default for AppPolicy {
    fn default() -> Self {
        Self {
            default_manual: DEFAULT_MANUAL
                .iter()
                .map(|class| class.to_string())
                .collect(),
            default_off: DEFAULT_OFF.iter().map(|class| class.to_string()).collect(),
            disabled: HashSet::new(),
            manual: HashSet::new(),
            enabled: HashSet::new(),
            data_dir: None,
        }
    }
}

impl AppPolicy {
    pub fn new() -> Self {
        Self::with_data_dir(app_policy_data_dir())
    }

    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        let mut policy = Self {
            data_dir: Some(data_dir.clone()),
            ..Self::default()
        };
        policy.disabled = load_rules(&data_dir.join(DISABLED_FILE));
        policy.manual = load_rules(&data_dir.join(MANUAL_FILE));
        policy.enabled = load_rules(&data_dir.join(ENABLED_FILE));
        // Resolve hand-edited conflicts deterministically. The safest mode
        // wins: off, then manual, then automatic.
        policy
            .manual
            .retain(|class| !policy.disabled.contains(class));
        policy
            .enabled
            .retain(|class| !policy.disabled.contains(class) && !policy.manual.contains(class));
        policy
    }

    pub fn decision(&self, class: &str) -> AppDecision {
        let class = normalise(class);
        if let Some((mode, source)) = self.best_user_mode(&class) {
            return AppDecision { mode, source };
        }
        if matches_rules(&self.default_off, &class) {
            return AppDecision {
                mode: AppMode::Off,
                source: PolicySource::DefaultOff,
            };
        }
        if matches_rules(&self.default_manual, &class) {
            return AppDecision {
                mode: AppMode::Manual,
                source: PolicySource::DefaultManual,
            };
        }
        AppDecision {
            mode: AppMode::Auto,
            source: PolicySource::DefaultAuto,
        }
    }

    /// Compatibility helper: historically "enabled" meant automatic mode.
    pub fn is_enabled(&self, class: &str) -> bool {
        self.decision(class).allows_automatic()
    }

    /// Cycle Auto → Manual → Off → Auto and persist the exact focused class.
    pub fn cycle(&mut self, class: &str) -> std::io::Result<AppMode> {
        let class = normalise(class);
        if class.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                crate::i18n::tr("empty WM_CLASS"),
            ));
        }
        let mode = self.decision(&class).mode.next();
        self.set_mode(&class, mode)?;
        Ok(mode)
    }

    pub fn set_enabled(&mut self, class: &str, enable: bool) -> std::io::Result<()> {
        self.set_mode(class, if enable { AppMode::Auto } else { AppMode::Off })
    }

    pub fn set_mode(&mut self, class: &str, mode: AppMode) -> std::io::Result<()> {
        let class = normalise(class);
        if class.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                crate::i18n::tr("empty WM_CLASS"),
            ));
        }
        // The Shell's own input is the lock screen and the password prompts:
        // the toggle hotkey pressed on a locked screen must not enable
        // correction in the password field.
        if class == "gnome-shell" && mode != AppMode::Off {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                crate::i18n::tr("gnome-shell is the lock screen and the system dialogs; PuLinTo stays off there"),
            ));
        }
        self.enabled.remove(&class);
        self.manual.remove(&class);
        self.disabled.remove(&class);
        match mode {
            AppMode::Auto => self.enabled.insert(class),
            AppMode::Manual => self.manual.insert(class),
            AppMode::Off => self.disabled.insert(class),
        };
        self.save()
    }

    pub fn set_mode_text(&mut self, class: &str, mode: &str) -> std::io::Result<()> {
        let mode = AppMode::parse(mode).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unknown application mode",
            )
        })?;
        self.set_mode(class, mode)
    }

    pub fn mode(&self, class: &str) -> AppMode {
        self.decision(class).mode
    }

    pub fn allows_automatic(&self, class: &str) -> bool {
        self.decision(class).allows_automatic()
    }

    pub fn allows_manual(&self, class: &str) -> bool {
        self.decision(class).allows_manual()
    }

    pub fn explicit_mode(&self, class: &str) -> Option<AppMode> {
        let class = normalise(class);
        self.best_user_mode(&class).map(|(mode, _)| mode)
    }

    /// Remove an explicit choice and return to the shipped default.
    pub fn reset(&mut self, class: &str) -> std::io::Result<bool> {
        let class = normalise(class);
        let changed =
            self.enabled.remove(&class) | self.manual.remove(&class) | self.disabled.remove(&class);
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn enabled_apps(&self) -> Vec<String> {
        sorted(&self.enabled)
    }

    pub fn disabled_apps(&self) -> Vec<String> {
        sorted(&self.disabled)
    }

    pub fn manual_apps(&self) -> Vec<String> {
        sorted(&self.manual)
    }

    pub fn default_disabled_apps() -> Vec<String> {
        DEFAULT_MANUAL
            .iter()
            .chain(DEFAULT_OFF.iter())
            .map(|class| class.to_string())
            .collect()
    }

    pub fn default_manual_apps() -> Vec<String> {
        DEFAULT_MANUAL
            .iter()
            .map(|class| class.to_string())
            .collect()
    }

    pub fn default_off_apps() -> Vec<String> {
        DEFAULT_OFF.iter().map(|class| class.to_string()).collect()
    }

    fn best_user_mode(&self, class: &str) -> Option<(AppMode, PolicySource)> {
        let sets = [
            (&self.enabled, AppMode::Auto, PolicySource::UserAuto, 0usize),
            (&self.manual, AppMode::Manual, PolicySource::UserManual, 1),
            (&self.disabled, AppMode::Off, PolicySource::UserOff, 2),
        ];
        sets.into_iter()
            .flat_map(|(rules, mode, source, safety)| {
                rules.iter().filter_map(move |rule| {
                    matching_specificity(rule, class)
                        .map(|specificity| (specificity, safety, mode, source))
                })
            })
            .max_by_key(|(specificity, safety, _, _)| (*specificity, *safety))
            .map(|(_, _, mode, source)| (mode, source))
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(dir) = &self.data_dir else {
            return Ok(());
        };
        fs::create_dir_all(dir)?;
        write_rules(
            &dir.join(DISABLED_FILE),
            "# PuLinTo — applications where the service is explicitly turned off",
            &self.disabled,
        )?;
        write_rules(
            &dir.join(MANUAL_FILE),
            "# PuLinTo — applications where only the manual Alt works",
            &self.manual,
        )?;
        write_rules(
            &dir.join(ENABLED_FILE),
            "# PuLinTo — applications allowed despite the safe defaults",
            &self.enabled,
        )
    }
}

/// A class fit for the list files: one line, not a comment. Anything else
/// becomes empty, which the callers reject as “empty WM_CLASS”.
fn normalise(class: &str) -> String {
    let class = class.trim().to_lowercase();
    if class.starts_with('#') || class.chars().any(|ch| ch.is_control()) {
        return String::new();
    }
    class
}

fn matches_rules(rules: &HashSet<String>, class: &str) -> bool {
    rules
        .iter()
        .any(|rule| matching_specificity(rule, class).is_some())
}

fn matching_specificity(rule: &str, class: &str) -> Option<usize> {
    if let Some(prefix) = rule.strip_suffix('*') {
        class.starts_with(prefix).then_some(prefix.len())
    } else {
        (rule == class).then_some(usize::MAX)
    }
}

fn load_rules(path: &Path) -> HashSet<String> {
    if path.exists() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(normalise)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

fn sorted(rules: &HashSet<String>) -> Vec<String> {
    let mut rules: Vec<String> = rules.iter().cloned().collect();
    rules.sort();
    rules
}

fn write_rules(path: &Path, header: &str, rules: &HashSet<String>) -> std::io::Result<()> {
    let mut contents = String::from(header);
    contents.push('\n');
    for rule in sorted(rules) {
        contents.push_str(&rule);
        contents.push('\n');
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents.as_bytes())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

fn app_policy_data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                .join(".local")
                .join("share")
        });
    base.join("pulinto")
}

pub fn excluded_apps_path() -> PathBuf {
    app_policy_data_dir().join(DISABLED_FILE)
}

pub fn enabled_apps_path() -> PathBuf {
    app_policy_data_dir().join(ENABLED_FILE)
}

pub fn manual_apps_path() -> PathBuf {
    app_policy_data_dir().join(MANUAL_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe_and_case_insensitive() {
        let policy = AppPolicy::default();
        assert_eq!(policy.mode("KeePassXC"), AppMode::Off);
        assert_eq!(policy.mode("Alacritty"), AppMode::Manual);
        assert_eq!(policy.mode("jetbrains-idea"), AppMode::Manual);
        assert_eq!(policy.mode("Navigator"), AppMode::Auto);
        assert!(!policy.allows_manual("KeePassXC"));
        assert!(policy.allows_manual("Alacritty"));
        assert!(!policy.allows_automatic("Alacritty"));
    }

    /// Under Wayland the Shell extension reports app ids, not WM_CLASS; the
    /// safety defaults have to cover those names as well.
    #[test]
    fn wayland_app_ids_get_the_same_defaults() {
        let policy = AppPolicy::default();
        assert_eq!(policy.mode("org.keepassxc.KeePassXC"), AppMode::Off);
        assert_eq!(policy.mode("org.gnome.Ptyxis"), AppMode::Manual);
        assert_eq!(policy.mode("org.gnome.Console"), AppMode::Manual);
        assert_eq!(policy.mode("org.gnome.TextEditor"), AppMode::Auto);
        assert_eq!(policy.mode("gnome-shell"), AppMode::Off);
    }

    #[test]
    fn user_choices_override_defaults_and_persist() {
        let dir = std::env::temp_dir().join(format!(
            "pulinto-app-policy-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let mut policy = AppPolicy::with_data_dir(dir.clone());
        policy.set_enabled("Alacritty", true).unwrap();
        policy.set_mode("Navigator", AppMode::Manual).unwrap();
        policy.set_enabled("Steam", false).unwrap();

        let loaded = AppPolicy::with_data_dir(dir);
        assert_eq!(loaded.decision("alacritty").source, PolicySource::UserAuto);
        assert_eq!(loaded.mode("ALACRITTY"), AppMode::Auto);
        assert_eq!(
            loaded.decision("navigator").source,
            PolicySource::UserManual
        );
        assert_eq!(loaded.mode("Navigator"), AppMode::Manual);
        assert_eq!(loaded.decision("steam").source, PolicySource::UserOff);
        assert_eq!(loaded.manual_apps(), vec!["navigator"]);
    }

    #[test]
    fn cycle_walks_through_all_three_modes() {
        let mut policy = AppPolicy::default();
        assert_eq!(policy.mode("Navigator"), AppMode::Auto);
        assert_eq!(policy.cycle("Navigator").unwrap(), AppMode::Manual);
        assert_eq!(policy.cycle("Navigator").unwrap(), AppMode::Off);
        assert_eq!(policy.cycle("Navigator").unwrap(), AppMode::Auto);
    }

    #[test]
    fn an_exact_user_rule_beats_a_broader_wildcard() {
        let mut policy = AppPolicy::default();
        policy.set_mode("jetbrains-*", AppMode::Off).unwrap();
        policy.set_mode("jetbrains-idea", AppMode::Auto).unwrap();
        assert_eq!(policy.mode("jetbrains-rider"), AppMode::Off);
        assert_eq!(policy.mode("jetbrains-idea"), AppMode::Auto);
    }
}
