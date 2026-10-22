//! Layout controller traits and implementations.
//!
//! The unit the rest of the daemon works in is the **layout id** (`us`, `ru`),
//! not the XKB group index. Those are not the same thing and must not be
//! confused: the group is a slot in whatever keymap the server happens to hold
//! right now, and on an IBus session that keymap is rebuilt on every engine
//! switch with the active layout placed in group 0. Treating group 0 as "the
//! first profile we know about" is what made the daemon read Russian typing as
//! Latin. Ask for a name; use a group only to address the server.

pub mod gnome;
pub mod wayland;
pub mod x11;
pub mod xkb_config;

use std::env;

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("X11 layout error: {0}")]
    X11Error(String),
    #[error("Unsupported platform or desktop environment")]
    Unsupported,
}

/// Keys that move the keymap to the next layout, for backends that cannot set
/// a group from outside the key stream (GNOME on Wayland).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutChord {
    /// Evdev key codes, pressed in order and released in reverse.
    pub keys: Vec<u16>,
    /// True when the compositor applies the switch in order with the keys that
    /// follow it (an XKB group option), so they need no wait. False for a
    /// desktop shortcut handled elsewhere, which has to be confirmed first.
    pub ordered: bool,
}

pub trait LayoutController: Send + Sync {
    fn name(&self) -> &'static str;
    fn get_current_group(&self) -> Result<u8, LayoutError>;
    fn set_group(&self, group: u8) -> Result<(), LayoutError>;

    /// Layout ids the server currently has loaded, in group order — the
    /// mapping the daemon needs and the only one that survives a keymap swap.
    /// Empty when the backend cannot tell, and callers must cope with that
    /// rather than assume an order.
    fn layout_ids(&self) -> Vec<String> {
        Vec::new()
    }

    /// XKB variant of each loaded layout, aligned with [`layout_ids`]: empty for
    /// the standard arrangement, `mac` for the Apple one. Empty when the backend
    /// cannot tell — callers then assume the standard arrangement.
    ///
    /// [`layout_ids`]: LayoutController::layout_ids
    fn layout_variants(&self) -> Vec<String> {
        Vec::new()
    }

    /// Id of the layout in effect right now, if known.
    fn current_layout_id(&self) -> Option<String> {
        let group = self.get_current_group().ok()? as usize;
        self.layout_ids().get(group).cloned()
    }

    /// Group that produces `layout_id`, if the server has that layout loaded.
    fn group_of(&self, layout_id: &str) -> Option<u8> {
        self.layout_ids()
            .iter()
            .position(|id| id == layout_id)
            .and_then(|index| u8::try_from(index).ok())
    }

    /// `Some` when the layout can only be changed by typing a hotkey through the
    /// daemon's own keyboard; `None` when [`LayoutController::set_group`] works.
    fn layout_chord(&self) -> Option<LayoutChord> {
        None
    }

    /// The daemon has just typed a switch to `group`. Backends that learn of
    /// switches late record it at once, so the next read is not stale.
    fn note_group(&self, _group: u8) {}

    /// The group as the desktop itself last reported it — never a guess made
    /// from a chord we typed. `None` when the controller has no such source
    /// and the current group is the only answer.
    fn confirmed_group(&self) -> Option<u8> {
        None
    }

    /// Move to the next group. Kept for the manual hotkey and the D-Bus method,
    /// where there is no target layout to aim at — a replacement should use
    /// [`LayoutController::group_of`] instead and say where it wants to go.
    fn toggle_next(&self) -> Result<(), LayoutError> {
        let current = self.get_current_group()?;
        let groups = u8::try_from(self.layout_ids().len()).unwrap_or(2).max(2);
        self.set_group((current + 1) % groups)
    }
}

/// Pick whoever actually governs the layout on this session.
///
/// Under Wayland that is the desktop: GNOME's own input sources, or the blind
/// toggle when GNOME cannot be reached. On X11 it is XKB, which the controller
/// reads by layout name rather than group number, so an IBus engine switch that
/// rebuilds the keymap does not confuse it. The blind toggle is the last resort.
pub fn create_default_layout_controller() -> Box<dyn LayoutController> {

    // The stand runs against a dummy X server where `latch_lock_state` is
    // accepted and never applied, so every switch times out unconfirmed. The
    // blind toggle works there. This is a property of the sandbox, not of the
    // product: never set it on a live session.
    let forced_blind = env::var_os("PULINTO_FORCE_WAYLAND_CONTROLLER").is_some();
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    if forced_blind {
        return Box::new(wayland::WaylandLayoutController::new());
    }
    if session_type.eq_ignore_ascii_case("wayland") || env::var_os("WAYLAND_DISPLAY").is_some() {
        // Under Wayland the X server is Xwayland, whose keymap follows the
        // compositor only for X windows — reading it would be wrong whenever a
        // Wayland window has focus. Ask the desktop instead.
        if let Ok(gnome) = gnome::GnomeLayoutController::new() {
            return Box::new(gnome);
        }
        return Box::new(wayland::WaylandLayoutController::new());
    }
    if let Ok(x11) = x11::X11LayoutController::new() {
        return Box::new(x11);
    }
    Box::new(wayland::WaylandLayoutController::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};

    /// A controller with a keymap we choose, to pin down the group ↔ name
    /// mapping without an X server.
    struct Fake {
        group: AtomicU8,
        layouts: Vec<String>,
    }

    impl Fake {
        fn new(group: u8, layouts: &[&str]) -> Self {
            Self {
                group: AtomicU8::new(group),
                layouts: layouts.iter().map(|id| (*id).to_string()).collect(),
            }
        }
    }

    impl LayoutController for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn get_current_group(&self) -> Result<u8, LayoutError> {
            Ok(self.group.load(Ordering::SeqCst))
        }
        fn set_group(&self, group: u8) -> Result<(), LayoutError> {
            self.group.store(group, Ordering::SeqCst);
            Ok(())
        }
        fn layout_ids(&self) -> Vec<String> {
            self.layouts.clone()
        }
    }

    /// The regression this whole change exists for: with IBus the active layout
    /// sits in group 0, so group 0 is `ru` here and `us` in the next test. Only
    /// the name distinguishes them.
    #[test]
    fn group_zero_is_whatever_the_keymap_says_it_is() {
        let ibus_on_russian = Fake::new(0, &["ru", "us"]);
        assert_eq!(ibus_on_russian.current_layout_id().as_deref(), Some("ru"));

        let ibus_on_english = Fake::new(0, &["us", "ru"]);
        assert_eq!(ibus_on_english.current_layout_id().as_deref(), Some("us"));
    }

    #[test]
    fn a_layout_is_addressed_by_name_and_found_wherever_it_sits() {
        let controller = Fake::new(0, &["ru", "us"]);
        assert_eq!(controller.group_of("us"), Some(1));
        assert_eq!(controller.group_of("ru"), Some(0));
        // A layout the user has not loaded: the caller must not get a group
        // number it would then switch to blindly.
        assert_eq!(controller.group_of("de"), None);
    }

    #[test]
    fn without_names_nothing_is_claimed() {
        struct Nameless;
        impl LayoutController for Nameless {
            fn name(&self) -> &'static str {
                "nameless"
            }
            fn get_current_group(&self) -> Result<u8, LayoutError> {
                Ok(0)
            }
            fn set_group(&self, _group: u8) -> Result<(), LayoutError> {
                Ok(())
            }
        }
        assert_eq!(Nameless.current_layout_id(), None);
        assert_eq!(Nameless.group_of("us"), None);
    }

    #[test]
    fn toggle_wraps_over_the_layouts_actually_loaded() {
        let three = Fake::new(2, &["us", "ru", "de"]);
        three.toggle_next().unwrap();
        assert_eq!(three.get_current_group().unwrap(), 0);

        // No names: two layouts is the historical assumption and stays.
        let two = Fake::new(1, &[]);
        two.toggle_next().unwrap();
        assert_eq!(two.get_current_group().unwrap(), 0);
    }
}
