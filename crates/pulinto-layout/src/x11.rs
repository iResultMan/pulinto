//! X11 layout controller: which layout is in effect, by name, kept current.
//!
//! **Why not just the group index.** The group is a slot in the keymap the
//! server holds at this instant, and nothing guarantees slot order. Under IBus
//! — the setup on the development machine, XFCE with `xkb:us::eng` and
//! `xkb:ru::rus` — every engine switch reloads the keymap with the layout it
//! just activated in group 0. Watch `_XKB_RULES_NAMES` across a switch and the
//! layout list goes from `us` to `ru,us`: the group number stayed 0 while the
//! meaning of 0 changed. The old code mapped the group index straight onto the
//! daemon's own profile order (`[us, ru]`), so it believed every keystroke was
//! Latin, read Russian words as gibberish and "corrected" them into Latin.
//!
//! **Why a watcher instead of a question at word boundaries.** Asking once per
//! word only ever sees our own last move; the changes that matter are the ones
//! we did not make — the user's hotkey, IBus reloading the keymap. So the group
//! and the layout names are pushed to us by the server: XKB `StateNotify` for a
//! group change, `NewKeyboardNotify`/`MapNotify` plus a `PropertyNotify` on
//! `_XKB_RULES_NAMES` for a keymap change.
//!
//! The watcher runs on its own connection. `wait_for_event` blocks, and the
//! request connection has to stay usable for `set_group` while it does.

use crate::{LayoutController, LayoutError};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};

use x11rb::connection::Connection;
use x11rb::protocol::xkb;
use x11rb::protocol::xproto::{
    self, Atom, AtomEnum, ChangeWindowAttributesAux, EventMask, Window,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

/// Root-window property listing the loaded layouts, in group order. Set by
/// whoever last configured the keymap — `setxkbmap`, IBus, the desktop's own
/// keyboard applet — and the same source every layout indicator reads.
const RULES_NAMES: &str = "_XKB_RULES_NAMES";

/// What the daemon reads; what the watcher writes.
#[derive(Debug, Default)]
struct Shared {
    group: AtomicU8,
    layouts: RwLock<Vec<String>>,
    variants: RwLock<Vec<String>>,
    /// False when no watcher is running, in which case reads fall back to
    /// asking the server. A stale cache would be worse than a round trip.
    watching: AtomicBool,
}

pub struct X11LayoutController {
    conn: RustConnection,
    shared: Arc<Shared>,
}

impl X11LayoutController {
    pub fn new() -> Result<Self, LayoutError> {
        let (conn, screen) = x11rb::connect(None).map_err(x11_error)?;
        // The reply must be read: until it is, the server has not confirmed the
        // XKB extension and later xkb requests can fail as protocol errors.
        xkb::use_extension(&conn, 1, 0)
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?;

        let root = conn.setup().roots[screen].root;
        let rules_atom = intern(&conn, RULES_NAMES)?;

        let shared = Arc::new(Shared::default());
        shared
            .group
            .store(query_group(&conn).unwrap_or(0), Ordering::SeqCst);
        *shared.layouts.write().expect("layout names lock") =
            read_layout_ids(&conn, root, rules_atom);
        *shared.variants.write().expect("layout variants lock") =
            read_variants(&conn, root, rules_atom);

        watch(Arc::clone(&shared), root);

        Ok(Self { conn, shared })
    }
}

impl LayoutController for X11LayoutController {
    fn name(&self) -> &'static str {
        "X11 (XKB)"
    }

    fn get_current_group(&self) -> Result<u8, LayoutError> {
        if self.shared.watching.load(Ordering::SeqCst) {
            return Ok(self.shared.group.load(Ordering::SeqCst));
        }
        match query_group(&self.conn) {
            Some(group) => {
                self.shared.group.store(group, Ordering::SeqCst);
                Ok(group)
            }
            None => Ok(self.shared.group.load(Ordering::SeqCst)),
        }
    }

    fn set_group(&self, group: u8) -> Result<(), LayoutError> {
        xkb::latch_lock_state(
            &self.conn,
            xkb::ID::USE_CORE_KBD.into(),
            0u16.into(),
            0u16.into(),
            true,
            group.into(),
            0u16.into(),
            false,
            0,
        )
        .map_err(x11_error)?;

        // The core group lives until the first event from a slave device: in X
        // every keyboard has its own XKB group, and the master follows the one
        // that typed last. The daemon grabs the physical keyboards at the evdev
        // level — the X server hears only the virtual uinput device, and its
        // group pulled the state back with the very first key press.
        // Measured: set_group answered Ok, the layout watcher saw
        // no change at all. So the group is locked on every keyboard device;
        // errors of individual devices do not matter — not every one answers
        // XKB requests.
        if let Ok(cookie) = x11rb::protocol::xinput::list_input_devices(&self.conn) {
            if let Ok(reply) = cookie.reply() {
                use x11rb::protocol::xinput::DeviceUse;
                for info in reply.devices {
                    if matches!(
                        info.device_use,
                        DeviceUse::IS_X_KEYBOARD | DeviceUse::IS_X_EXTENSION_KEYBOARD
                    ) {
                        if let Ok(c) = xkb::latch_lock_state(
                            &self.conn,
                            u16::from(info.device_id),
                            0u16.into(),
                            0u16.into(),
                            true,
                            group.into(),
                            0u16.into(),
                            false,
                            0,
                        ) {
                            let _ = c.check();
                        }
                    }
                }
            }
        }

        self.conn.flush().map_err(x11_error)?;

        // Record it ourselves rather than waiting for the notification to come
        // back: the next read may happen microseconds from now, and the reply
        // travels the socket on its own schedule.
        self.shared.group.store(group, Ordering::SeqCst);
        Ok(())
    }

    fn layout_ids(&self) -> Vec<String> {
        self.shared
            .layouts
            .read()
            .map(|layouts| layouts.clone())
            .unwrap_or_default()
    }

    fn layout_variants(&self) -> Vec<String> {
        self.shared
            .variants
            .read()
            .map(|variants| variants.clone())
            .unwrap_or_default()
    }
}

fn x11_error(error: impl std::fmt::Display) -> LayoutError {
    LayoutError::X11Error(error.to_string())
}

fn intern(conn: &RustConnection, name: &str) -> Result<Atom, LayoutError> {
    Ok(xproto::intern_atom(conn, false, name.as_bytes())
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?
        .atom)
}

fn query_group(conn: &RustConnection) -> Option<u8> {
    let reply = xkb::get_state(conn, xkb::ID::USE_CORE_KBD.into())
        .ok()?
        .reply()
        .ok()?;
    Some(reply.group.into())
}

/// Layout ids from `_XKB_RULES_NAMES`, in group order.
fn read_layout_ids(conn: &RustConnection, root: Window, rules_atom: Atom) -> Vec<String> {
    read_rules_property(conn, root, rules_atom)
        .map(|property| parse_layout_ids(&property))
        .unwrap_or_default()
}

/// Variants from `_XKB_RULES_NAMES`, one per loaded layout.
fn read_variants(conn: &RustConnection, root: Window, rules_atom: Atom) -> Vec<String> {
    read_rules_property(conn, root, rules_atom)
        .map(|property| parse_variants(&property))
        .unwrap_or_default()
}

fn read_rules_property(conn: &RustConnection, root: Window, rules_atom: Atom) -> Option<Vec<u8>> {
    let Ok(cookie) = xproto::get_property(
        conn,
        false,
        root,
        rules_atom,
        AtomEnum::STRING,
        0,
        // Five NUL-terminated strings; the options field is the long one and
        // never approaches this.
        1024,
    ) else {
        return None;
    };
    cookie.reply().ok().map(|reply| reply.value)
}

/// `_XKB_RULES_NAMES` is five NUL-separated strings — rules, model, layouts,
/// variants, options — of which the third is `us,ru` in group order.
fn parse_layout_ids(property: &[u8]) -> Vec<String> {
    property
        .split(|byte| *byte == 0)
        .nth(2)
        .map(|layouts| {
            String::from_utf8_lossy(layouts)
                .split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The fourth field of `_XKB_RULES_NAMES`: variants, comma-separated and aligned
/// by position with the layouts. A short list means "standard" for the rest, so
/// the result is padded to the layout count — `us,ru` with `,mac` is `["", "mac"]`.
fn parse_variants(property: &[u8]) -> Vec<String> {
    let layouts = parse_layout_ids(property).len();
    let mut variants: Vec<String> = property
        .split(|byte| *byte == 0)
        .nth(3)
        .map(|field| {
            String::from_utf8_lossy(field)
                .split(',')
                .map(|variant| variant.trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    variants.resize(layouts, String::new());
    variants
}

/// Start the watcher, or leave `watching` false so reads fall back to polling.
fn watch(shared: Arc<Shared>, root: Window) {
    let (conn, _screen) = match x11rb::connect(None) {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("XKB: cannot open a second connection ({error}), layout watching is off");
            return;
        }
    };

    if let Err(error) = subscribe(&conn, root) {
        eprintln!("XKB: cannot subscribe to layout changes ({error}), watching is off");
        return;
    }

    let rules_atom = match intern_on(&conn, RULES_NAMES) {
        Some(atom) => atom,
        None => return,
    };

    // Re-read through the watcher's own connection, so what the loop sees and
    // what it stored can never come from two different keymaps.
    refresh(&conn, &shared, root, rules_atom);
    shared.watching.store(true, Ordering::SeqCst);

    std::thread::spawn(move || loop {
        match conn.wait_for_event() {
            Ok(Event::XkbStateNotify(event)) => {
                shared.group.store(event.group.into(), Ordering::SeqCst);
            }
            // The keymap itself was replaced — which is how IBus switches
            // layouts. Both the names and the group have to be re-read.
            Ok(Event::XkbNewKeyboardNotify(_)) | Ok(Event::XkbMapNotify(_)) => {
                refresh(&conn, &shared, root, rules_atom);
            }
            Ok(Event::PropertyNotify(event)) if event.atom == rules_atom => {
                refresh(&conn, &shared, root, rules_atom);
            }
            Ok(_) => {}
            Err(error) => {
                // The connection died. Say so instead of serving a frozen
                // group forever: a wrong layout is the defect this file exists
                // to fix.
                shared.watching.store(false, Ordering::SeqCst);
                eprintln!("XKB: layout watching interrupted ({error}), falling back to polling");
                return;
            }
        }
    });
}

fn subscribe(conn: &RustConnection, root: Window) -> Result<(), LayoutError> {
    xkb::use_extension(conn, 1, 0)
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?;

    let wanted = xkb::EventType::NEW_KEYBOARD_NOTIFY
        | xkb::EventType::MAP_NOTIFY
        | xkb::EventType::STATE_NOTIFY;
    xkb::select_events(
        conn,
        xkb::ID::USE_CORE_KBD.into(),
        0u16.into(), // clear nothing
        wanted,      // and take every detail of what we do want
        0u16.into(),
        0u16.into(),
        &xkb::SelectEventsAux::new(),
    )
    .map_err(x11_error)?;

    // Belt and braces: a client that swaps the keymap without going through
    // XKB still updates the root property, and that we would otherwise miss.
    xproto::change_window_attributes(
        conn,
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .map_err(x11_error)?;

    conn.flush().map_err(x11_error)?;
    Ok(())
}

fn intern_on(conn: &RustConnection, name: &str) -> Option<Atom> {
    Some(
        xproto::intern_atom(conn, false, name.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom,
    )
}

fn refresh(conn: &RustConnection, shared: &Shared, root: Window, rules_atom: Atom) {
    let layouts = read_layout_ids(conn, root, rules_atom);
    if let Ok(mut stored) = shared.layouts.write() {
        *stored = layouts;
    }
    let variants = read_variants(conn, root, rules_atom);
    if let Ok(mut stored) = shared.variants.write() {
        *stored = variants;
    }
    if let Some(group) = query_group(conn) {
        shared.group.store(group, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_layout_ids, parse_variants};

    /// Exactly the bytes read from this machine while IBus had the Russian
    /// engine active: two layouts, Russian first, because IBus rebuilt the
    /// keymap around the engine it had just switched to.
    #[test]
    fn layouts_come_out_in_group_order() {
        let property = b"evdev\0pc105\0ru,us\0,\0grp:alt_shift_toggle\0";
        assert_eq!(parse_layout_ids(property), vec!["ru", "us"]);
    }

    /// The same machine a moment earlier, with the English engine active: one
    /// layout, and group 0 means something else entirely.
    #[test]
    fn a_single_layout_is_read_as_one_group() {
        let property = b"evdev\0pc105\0us\0\0grp:alt_shift_toggle\0";
        assert_eq!(parse_layout_ids(property), vec!["us"]);
    }

    #[test]
    fn variants_line_up_with_layouts_by_position() {
        let property = b"evdev\0pc105\0us,ru\0,mac\0grp:alt_shift_toggle\0";
        assert_eq!(parse_variants(property), vec!["", "mac"]);
        let both = b"evdev\0pc105\0us,ru\0mac,mac\0\0";
        assert_eq!(parse_variants(both), vec!["mac", "mac"]);
    }

    /// `setxkbmap -layout us,ru -variant mac` names a variant for the first layout
    /// only; the second one is standard, not missing.
    #[test]
    fn a_short_variant_list_is_padded_with_the_standard_arrangement() {
        let property = b"evdev\0pc105\0us,ru\0mac\0\0";
        assert_eq!(parse_variants(property), vec!["mac", ""]);
        let none = b"evdev\0pc105\0us,ru\0\0\0";
        assert_eq!(parse_variants(none), vec!["", ""]);
    }

    #[test]
    fn nothing_is_invented_from_a_missing_or_empty_property() {
        assert!(parse_variants(b"").is_empty());
        assert!(parse_layout_ids(b"").is_empty());
        assert!(parse_layout_ids(b"evdev\0pc105\0").is_empty());
        assert!(parse_layout_ids(b"evdev\0pc105\0\0\0\0").is_empty());
    }
}
