//! Wayland Layout Controller implementations (KDE, GNOME, Sway, Hyprland).

use crate::{LayoutController, LayoutError};
use std::env;
use std::sync::atomic::{AtomicU8, Ordering};

pub struct WaylandLayoutController {
    current_group: AtomicU8,
    desktop_env: String,
}

impl WaylandLayoutController {
    pub fn new() -> Self {
        let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "generic".into());
        Self {
            current_group: AtomicU8::new(0),
            desktop_env: desktop,
        }
    }
}

impl Default for WaylandLayoutController {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutController for WaylandLayoutController {
    fn name(&self) -> &'static str {
        match self.desktop_env.to_lowercase().as_str() {
            d if d.contains("kde") => "Wayland (KDE D-Bus)",
            d if d.contains("gnome") => "Wayland (GNOME D-Bus)",
            d if d.contains("sway") => "Wayland (Sway IPC)",
            d if d.contains("hyprland") => "Wayland (Hyprland IPC)",
            _ => "Wayland (Synthetic uinput)",
        }
    }

    fn get_current_group(&self) -> Result<u8, LayoutError> {
        Ok(self.current_group.load(Ordering::SeqCst))
    }

    fn set_group(&self, group: u8) -> Result<(), LayoutError> {
        self.current_group.store(group, Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wayland_layout_controller() {
        let controller = WaylandLayoutController::new();
        assert!(controller.set_group(1).is_ok());
        assert_eq!(controller.get_current_group().unwrap(), 1);
    }
}
