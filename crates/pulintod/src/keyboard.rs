//! The Apple arrangement of the keyboard: what the machine has, what the user
//! asked for, and the switch between them.
//!
//! The arrangement is a fact about the X server's keymap — `ru(mac)` puts the
//! comma on Shift+6, the full stop on Shift+7 and ё on `\` — so the daemon reads
//! it from there and builds its position tables to match. It also keeps the
//! user's wish in its own config and applies it at start: nothing else in a
//! session remembers a layout change made behind the desktop's back.

use crate::config::Config;
use crate::i18n::tr;
use crate::SwitcherState;
use pulinto_layout::xkb_config::{self, Rmlvo};
use pulinto_layout::LayoutController;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Run before the language profiles are built, because it decides which
/// position tables they get: apply the Apple arrangement if the user asked for
/// it and the server does not have it yet, then tell the tables what the server
/// really runs.
pub fn prepare(settings: &Config, layout: &dyn LayoutController) {
    if settings.apple_layout && !is_apple(layout) && crate::is_wayland_session() {
        // GNOME keeps its input sources itself; the change goes there, and
        // the controller's watcher reports the rebuilt list.
        match pulinto_layout::gnome::set_apple(true) {
            Ok(()) => wait_for_apple(layout, true),
            Err(error) => eprintln!("Apple layout not applied: {error}"),
        }
    } else if settings.apple_layout && !is_apple(layout) {
        match Rmlvo::query().and_then(|current| current.with_apple(true).apply()) {
            Ok(()) => wait_for_apple(layout, true),
            Err(error) => eprintln!("Apple layout not applied: {error}"),
        }
    }
    use_server_variants(layout);
}

/// The position tables follow the keymap, never the other way round.
fn use_server_variants(layout: &dyn LayoutController) {
    let ids = layout.layout_ids();
    let variants = layout.layout_variants();
    pulinto_core::layout::use_variants(
        ids.iter()
            .zip(variants.iter())
            .map(|(id, variant)| (id.as_str(), variant.as_str())),
    );
}

/// Does the server run the Apple arrangement for Russian right now?
pub fn is_apple(layout: &dyn LayoutController) -> bool {
    layout
        .layout_ids()
        .iter()
        .zip(layout.layout_variants().iter())
        .any(|(id, variant)| id == "ru" && variant == xkb_config::APPLE_VARIANT)
}

/// Do the tables the daemon is running with match the server's keymap?
pub fn tables_are_apple() -> bool {
    pulinto_core::layout::Layout::by_id("ru")
        .is_some_and(|ru| ru.variant == xkb_config::APPLE_VARIANT)
}

/// The watcher learns of a keymap change a moment after `setxkbmap` returns.
fn wait_for_apple(layout: &dyn LayoutController, wanted: bool) {
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        if is_apple(layout) == wanted {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The hotkey: flip the Apple arrangement and start the input again, exactly as
/// the switch in the settings window does. The keymap changes on the fly; only
/// the daemon's own position tables need the restart. A desktop notification
/// says what happened, because nothing else on screen does.
pub fn toggle_apple_layout(layout: &dyn LayoutController, state: &Arc<SwitcherState>) {
    let enable = !is_apple(layout);
    match set_apple_layout(enable) {
        Ok(()) => {
            notify(if enable {
                tr("Apple layout is on")
            } else {
                tr("Standard layout restored")
            });
            let state = Arc::clone(state);
            // The chord is still down: give the keys time to be let go before
            // the input is taken again.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(400));
                let mode =
                    crate::mode_from_index(state.mode.load(std::sync::atomic::Ordering::SeqCst));
                crate::switch_mode(&state, mode);
            });
        }
        Err(reason) => {
            eprintln!("Apple layout not switched: {reason}");
            notify(&reason);
        }
    }
}

pub(crate) fn notify(text: &str) {
    let text = text.to_string();
    std::thread::spawn(move || {
        let _ = std::process::Command::new("notify-send")
            .args([
                "-a",
                "PuLinTo",
                "-i",
                "input-keyboard-symbolic",
                "PuLinTo",
                &text,
            ])
            .status();
    });
}

/// What the settings window shows, as JSON.
pub fn info(layout: &dyn LayoutController, settings: &Config) -> String {
    let apple_layout = is_apple(layout);
    let foreign = if crate::is_wayland_session() {
        layout
            .layout_ids()
            .iter()
            .zip(layout.layout_variants().iter())
            .find(|(id, variant)| {
                *id == "ru" && !variant.is_empty() && *variant != xkb_config::APPLE_VARIANT
            })
            .map(|(_, variant)| variant.clone())
    } else {
        Rmlvo::query()
            .ok()
            .and_then(|current| current.foreign_russian_variant().map(str::to_string))
    };
    let can_apply = crate::is_wayland_session() || xkb_config::tool_available();
    let list = |items: Vec<String>| {
        items
            .iter()
            .map(|item| format!("\"{}\"", item.replace('"', "")))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{{\"apple_keyboard\":{},\"apple_layout\":{},\"wanted\":{},\"restart_needed\":{},\
         \"can_apply\":{},\"foreign_variant\":{},\"layouts\":[{}],\"variants\":[{}]}}",
        pulinto_input::capture::apple_keyboard_present(),
        apple_layout,
        settings.apple_layout,
        apple_layout != tables_are_apple(),
        can_apply,
        foreign
            .map(|variant| format!("\"{}\"", variant.replace('"', "")))
            .unwrap_or_else(|| "null".to_string()),
        list(layout.layout_ids()),
        list(layout.layout_variants()),
    )
}

/// Switch the Apple arrangement on or off and remember the choice. The caller
/// restarts the daemon afterwards, so the position tables are rebuilt for the
/// keymap that is now in effect.
pub fn set_apple_layout(enable: bool) -> Result<(), String> {
    if crate::is_wayland_session() {
        pulinto_layout::gnome::set_apple(enable).map_err(|error| {
            if error == pulinto_layout::gnome::REFUSED {
                tr("gsettings did not accept the new list of layouts").to_string()
            } else {
                error
            }
        })?;
        return Config::update(|settings| settings.apple_layout = enable).map_err(choice_not_saved);
    }
    let current = Rmlvo::query().map_err(config_error)?;
    let wanted = current.with_apple(enable);
    if enable && !wanted.is_apple() {
        return Err(match current.foreign_russian_variant() {
            Some(variant) => tr("The Russian layout uses the “{variant}” variant; the Apple layout is not applied over it, so your choice is kept.")
                .replace("{variant}", variant),
            None => tr("There is no Russian layout in the settings to apply the Apple variant to.").to_string(),
        });
    }
    if wanted != current {
        wanted.apply().map_err(config_error)?;
    }
    Config::update(|settings| settings.apple_layout = enable).map_err(choice_not_saved)
}

fn choice_not_saved(error: impl std::fmt::Display) -> String {
    tr("Choice not saved: {error}").replace("{error}", &error.to_string())
}

/// The `setxkbmap` failures in the interface language; the layout crate's own
/// texts are English.
fn config_error(error: xkb_config::ConfigError) -> String {
    use xkb_config::ConfigError;
    match error {
        ConfigError::ToolMissing(detail) => {
            tr("setxkbmap not found (package x11-xkb-utils): {detail}").replace("{detail}", &detail)
        }
        ConfigError::Failed(detail) => {
            tr("setxkbmap reported an error: {detail}").replace("{detail}", &detail)
        }
        ConfigError::Unreadable => tr("setxkbmap did not say which layout is loaded").to_string(),
    }
}
