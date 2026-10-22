//! Which application has the keyboard under GNOME on Wayland.
//!
//! Xwayland knows only X windows, so `WM_CLASS` of the focus is unavailable for
//! every Wayland client — and with it every per-application rule: password
//! managers would be corrected, terminals rewritten automatically. The small
//! Shell extension in `gnome-extension/` publishes the focused window's class
//! (its app id for Wayland clients) and announces each change.
//!
//! The answer is kept in a cache that a thread of its own updates from the
//! extension's signal. The keyboard loop only ever reads the cache: a D-Bus
//! call there could hang behind a busy Shell, and with the keyboard grabbed
//! that would be a dead keyboard.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

const BUS_NAME: &str = "ru.pulinto.Shell1";
const OBJECT_PATH: &str = "/ru/pulinto/Shell1";
const INTERFACE: &str = "ru.pulinto.Shell1";

#[derive(Clone, Default)]
pub struct ShellFocus {
    /// `None` while the extension has not answered at all — then the caller
    /// falls back to X; `Some("")` when nothing has focus.
    focused: Arc<RwLock<Option<String>>>,
    /// The screen is locked. Known from GNOME itself, extension or not: the
    /// password typed to unlock must never pass for a word to correct.
    locked: Arc<AtomicBool>,
    /// Every `FocusChanged` the extension announces, counted and stamped.
    changes: crate::xwindow::FocusChanges,
}

impl ShellFocus {
    pub fn start() -> Self {
        let focus = Self::default();
        let locked = Arc::clone(&focus.locked);
        std::thread::spawn(move || {
            if let Err(error) = follow_lock(&locked) {
                eprintln!("GNOME: the screen lock is not visible ({error})");
            }
        });
        // The extension can come and go: it is usually not up yet when the
        // daemon starts at login, and the Shell may be restarted. Follow it
        // while it is there; while it is not, wait for it to appear.
        let cache = Arc::clone(&focus.focused);
        let changes = focus.changes.clone();
        std::thread::spawn(move || {
            let mut said = false;
            loop {
                match follow(&cache, &changes) {
                    Ok(()) => {}
                    Err(error) if !said => {
                        said = true;
                        eprintln!(
                            "GNOME: the PuLinTo focus extension is not available yet ({error}) — \
                             application rules apply to X windows only, waiting for the extension"
                        );
                    }
                    Err(_) => {}
                }
                changes.set_watching(false);
                if let Ok(mut slot) = cache.write() {
                    *slot = None;
                }
                if wait_for_owner().is_err() {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                }
            }
        });
        focus
    }

    pub fn focus_changes(&self) -> &crate::xwindow::FocusChanges {
        &self.changes
    }

    /// Class of the focused window as the extension last reported it.
    pub fn focused_class(&self) -> Option<Option<String>> {
        if self.locked.load(Ordering::SeqCst) {
            return Some(Some("gnome-shell".to_string()));
        }
        let known = self.focused.read().ok()?.clone()?;
        Some((!known.is_empty()).then_some(known))
    }
}

/// Ask the Shell once and wait for the answer. `None` when the extension is not
/// there; `Some(None)` when nothing has focus.
pub fn ask() -> Option<Option<String>> {
    let connection = zbus::blocking::Connection::session().ok()?;
    let reply = connection
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "FocusedApp",
            &(),
        )
        .ok()?;
    let app: String = reply.body().deserialize().ok()?;
    Some((!app.is_empty()).then_some(app))
}

fn follow_lock(locked: &AtomicBool) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::session()?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.gnome.ScreenSaver")?
        .member("ActiveChanged")?
        .build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &connection, Some(16))?;
    let reply = connection.call_method(
        Some("org.gnome.ScreenSaver"),
        "/org/gnome/ScreenSaver",
        Some("org.gnome.ScreenSaver"),
        "GetActive",
        &(),
    )?;
    locked.store(reply.body().deserialize::<bool>()?, Ordering::SeqCst);
    for message in signals {
        if let Ok(active) = message?.body().deserialize::<bool>() {
            locked.store(active, Ordering::SeqCst);
        }
    }
    Ok(())
}

/// Block until the extension takes its bus name.
fn wait_for_owner() -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::session()?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .add_arg(BUS_NAME)?
        .build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &connection, Some(16))?;
    // Subscribed; it may already be there.
    if ask().is_some() {
        return Ok(());
    }
    for message in signals {
        let message = message?;
        if let Ok((_, _, owner)) = message.body().deserialize::<(String, String, String)>() {
            if !owner.is_empty() {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn follow(
    cache: &RwLock<Option<String>>,
    changes: &crate::xwindow::FocusChanges,
) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::session()?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(INTERFACE)?
        .member("FocusChanged")?
        .path(OBJECT_PATH)?
        .build();
    // Subscribe first, then ask: a change between the two is not lost.
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &connection, Some(64))?;
    let reply = connection.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        "FocusedApp",
        &(),
    )?;
    let app: String = reply.body().deserialize()?;
    if let Ok(mut slot) = cache.write() {
        *slot = Some(app);
    }
    println!("GNOME: the focused window is reported by the PuLinTo focus extension");
    changes.set_watching(true);
    for message in signals {
        let message = message?;
        if let Ok(app) = message.body().deserialize::<String>() {
            if let Ok(mut slot) = cache.write() {
                *slot = Some(app);
            }
            changes.note();
        }
    }
    Ok(())
}
