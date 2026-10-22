//! System-tray (StatusNotifierItem) integration.

use crate::config::{Config, Mode};
use crate::i18n::{tr, Lang};
use crate::SwitcherState;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn current_mode(state: &SwitcherState) -> Mode {
    let index = state.mode.load(Ordering::SeqCst) as usize;
    Mode::ALL.get(index).copied().unwrap_or_default()
}

/// Where the settings window may live, most specific first: an explicit
/// override, next to the daemon binary (installed layout), the source tree
/// when the daemon runs straight from `target/<profile>` (the customer's
/// setup), the default install prefix, and finally `PATH`.
fn settings_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(configured) = std::env::var_os("PULINTO_SETTINGS") {
        candidates.push(PathBuf::from(configured));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join("pulinto-settings"));
        candidates.push(exe_dir.join("../../gui/pulinto-settings"));
    }
    candidates.push(PathBuf::from("/usr/local/bin/pulinto-settings"));
    candidates.push(PathBuf::from("pulinto-settings"));
    candidates
}

fn open_settings(origin_app: Option<&str>) {
    let mut failures = Vec::new();
    for program in settings_candidates() {
        let mut command = Command::new(&program);
        let (stdout, stderr) = crate::child_output();
        command.stdout(stdout).stderr(stderr);
        if let Some(class) = origin_app {
            command.env("PULINTO_ORIGIN_APP", class);
        }
        match command.spawn() {
            Ok(_) => {
                eprintln!("Settings window opened: {}", program.display());
                return;
            }
            Err(error) => failures.push(format!("{}: {error}", program.display())),
        }
    }
    eprintln!(
        "Could not start the pulinto-settings window ({})",
        failures.join("; ")
    );
}

fn set_application_mode(state: &SwitcherState, class: &str, mode: crate::xwindow::AppMode) {
    if let Ok(mut policy) = state.app_policy.write() {
        if let Err(error) = policy.set_mode(class, mode) {
            eprintln!("Cannot change the mode of {class}: {error}");
        }
    }
}

/// Keeps the tray item registered with the shell. After the daemon re-executes
/// itself (a mode switch, "Restart") the process id, and with it the name
/// `org.kde.StatusNotifierItem-<pid>-1`, is the same as before, and the shell's
/// watcher has been seen to drop the item as the old connection closed, leaving
/// the new one unregistered: no icon. So the watcher is asked now and then, and
/// the item is registered again when it is missing.
pub async fn keep_registered(connection: zbus::Connection) {
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    for delay in [1u64, 2, 4, 8, 16] {
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        let listed = connection
            .call_method(
                Some("org.kde.StatusNotifierWatcher"),
                "/StatusNotifierWatcher",
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(
                    "org.kde.StatusNotifierWatcher",
                    "RegisteredStatusNotifierItems",
                ),
            )
            .await
            .ok()
            .and_then(|reply| {
                reply
                    .body()
                    .deserialize::<zbus::zvariant::OwnedValue>()
                    .ok()
            })
            .and_then(|value| Vec::<String>::try_from(value).ok());
        let Some(items) = listed else { continue };
        if items.iter().any(|item| item.contains(&name)) {
            continue;
        }
        let result = connection
            .call_method(
                Some("org.kde.StatusNotifierWatcher"),
                "/StatusNotifierWatcher",
                Some("org.kde.StatusNotifierWatcher"),
                "RegisterStatusNotifierItem",
                &(name.as_str(),),
            )
            .await;
        match result {
            Ok(_) => println!("The tray icon was registered again"),
            Err(error) => eprintln!("The tray icon was not registered: {error}"),
        }
    }
}

pub struct SwitcherTray {
    pub state: Arc<SwitcherState>,
}

static HANDLE: std::sync::OnceLock<ksni::Handle<SwitcherTray>> = std::sync::OnceLock::new();

/// Keeps the handle for [`refresh`].
pub fn register(handle: ksni::Handle<SwitcherTray>) {
    let _ = HANDLE.set(handle);
}

/// Asks the shell to read the icon, the tooltip and the menu again.
pub fn refresh() {
    if let Some(handle) = HANDLE.get() {
        handle.update(|_| {});
    }
}

/// The debug-mode indicator: the usual icon with a red dot in its lower right corner, steady,
/// so the shell has nothing to redraw while the recording runs. Drawn here, not shipped as a
/// file: ARGB32, network byte order, like the other pixmaps.
fn recording_pixmaps(enabled: bool) -> Vec<ksni::Icon> {
    let (red, green, blue) = (229.0, 37.0, 29.0);
    tray_pixmaps(enabled)
        .into_iter()
        .map(|mut icon| {
            let size = icon.width as f32;
            let radius = size * 0.24;
            let centre = size - radius - 0.5;
            for y in 0..icon.height {
                for x in 0..icon.width {
                    let distance = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt();
                    // One pixel of soft edge, laid over the icon.
                    let cover = (radius + 0.5 - distance).clamp(0.0, 1.0);
                    if cover == 0.0 {
                        continue;
                    }
                    let at = ((y * icon.width + x) * 4) as usize;
                    let pixel = &mut icon.data[at..at + 4];
                    let under = 1.0 - cover;
                    pixel[0] = (255.0 * cover + f32::from(pixel[0]) * under) as u8;
                    pixel[1] = (red * cover + f32::from(pixel[1]) * under) as u8;
                    pixel[2] = (green * cover + f32::from(pixel[2]) * under) as u8;
                    pixel[3] = (blue * cover + f32::from(pixel[3]) * under) as u8;
                }
            }
            icon
        })
        .collect()
}

fn current_pixmaps(state: &SwitcherState) -> Vec<ksni::Icon> {
    let enabled = current_mode(state) != Mode::Off;
    if crate::debug::recording() {
        recording_pixmaps(enabled)
    } else {
        tray_pixmaps(enabled)
    }
}

/// The debug-mode lines of the menu: the state, and what to do about it.
fn debug_items() -> Vec<ksni::MenuItem<SwitcherTray>> {
    use ksni::menu::*;
    let mut items: Vec<ksni::MenuItem<SwitcherTray>> = Vec::new();
    let finish = |save: bool| {
        move |_: &mut SwitcherTray| {
            match crate::debug::finish(save) {
                Ok(Some(path)) => crate::keyboard::notify(
                    &tr("Debug log copy saved: {path}. Look through it before sending: it may hold your texts.")
                        .replace("{path}", &path.display().to_string()),
                ),
                Ok(None) => crate::keyboard::notify(tr("Debug mode is off, the logs are deleted.")),
                Err(error) => eprintln!("The debug log was not closed: {error}"),
            }
            refresh();
        }
    };
    if crate::debug::recording() {
        let minutes = crate::debug::seconds_left().div_ceil(60);
        items.push(
            StandardItem {
                label: tr("● Debug mode is recording · {minutes} min left")
                    .replace("{minutes}", &minutes.to_string()),
                enabled: false,
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: tr("Stop debug mode and delete the logs").into(),
                activate: Box::new(finish(false)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: tr("Stop debug mode and save a copy").into(),
                activate: Box::new(finish(true)),
                ..Default::default()
            }
            .into(),
        );
    } else if crate::debug::leftover() {
        items.push(
            StandardItem {
                label: tr("A debug log is kept").into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: tr("Save a copy of the debug log").into(),
                activate: Box::new(finish(true)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: tr("Delete the debug log").into(),
                activate: Box::new(finish(false)),
                ..Default::default()
            }
            .into(),
        );
    }
    if !items.is_empty() {
        items.push(MenuItem::Separator);
    }
    items
}

/// The tray icon as embedded pixmaps (ARGB32, network byte order), drawn
/// from `data/icons/pulinto-tray-*.svg`. A themed name
/// such as `input-keyboard` is drawn by the shell's theme: a white outline on
/// most dark panels, which is no more than a white rectangle at 22 px, and
/// missing altogether where the theme lacks it. Pixmaps look the same
/// everywhere and need no installed files.
macro_rules! pixmap {
    ($state:literal, $size:literal) => {
        ksni::Icon {
            width: $size,
            height: $size,
            data: include_bytes!(concat!("../assets/tray-", $state, "-", $size, ".argb")).to_vec(),
        }
    };
}

fn tray_pixmaps(enabled: bool) -> Vec<ksni::Icon> {
    if enabled {
        vec![
            pixmap!("on", 16),
            pixmap!("on", 22),
            pixmap!("on", 24),
            pixmap!("on", 32),
            pixmap!("on", 48),
        ]
    } else {
        vec![
            pixmap!("off", 16),
            pixmap!("off", 22),
            pixmap!("off", 24),
            pixmap!("off", 32),
            pixmap!("off", 48),
        ]
    }
}

impl ksni::Tray for SwitcherTray {
    fn id(&self) -> String {
        "pulinto-switcher".into()
    }

    fn title(&self) -> String {
        "PuLinTo".into()
    }

    // No themed name: the pixmaps below are the icon (see `tray_pixmaps`).
    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        current_pixmaps(&self.state)
    }

    // While the debug mode records the item asks for attention and carries the red dot as its
    // attention icon too. Shells re-read the whole item on a change of status, so the plain
    // icon comes back at once when the recording ends, not only when the pixmap is fetched again.
    fn status(&self) -> ksni::Status {
        if crate::debug::recording() {
            ksni::Status::NeedsAttention
        } else {
            ksni::Status::Active
        }
    }

    fn attention_icon_pixmap(&self) -> Vec<ksni::Icon> {
        recording_pixmaps(current_mode(&self.state) != Mode::Off)
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let auto_status = if current_mode(&self.state) == Mode::Off {
            tr("off")
        } else if self.state.auto_enabled.load(Ordering::SeqCst) {
            tr("fixes automatically")
        } else {
            tr("hotkey only")
        };
        let app_status =
            crate::focused_app_decision(&self.state, crate::xwindow::XWindows::connect().as_ref())
                .map(|(class, decision)| format!("{class}: {}", decision.mode.label()))
                .unwrap_or_else(|| tr("current application: unknown").into());
        let recording = if crate::debug::recording() {
            format!("{} · ", tr("debug mode is recording"))
        } else {
            String::new()
        };
        ksni::ToolTip {
            title: "PuLinTo".into(),
            description: format!("{recording}{auto_status}; {app_status}"),
            icon_name: String::new(),
            icon_pixmap: current_pixmaps(&self.state),
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let enabled = current_mode(&self.state) != Mode::Off;
        let auto_on = self.state.auto_enabled.load(Ordering::SeqCst);
        let count = self.state.conversion_count.load(Ordering::SeqCst);
        let focused =
            crate::focused_app_decision(&self.state, crate::xwindow::XWindows::connect().as_ref());
        let settings_origin = focused.as_ref().map(|(class, _)| class.clone());
        let app_class = settings_origin.clone();
        let app_mode = focused.as_ref().map(|(_, decision)| decision.mode);
        let toggle_label = self.state.hotkeys.toggle_label();

        // The first line is the state, the second the one thing most people
        // come here to do. Everything else sits below a separator.
        let status = match (enabled, auto_on) {
            (false, _) => tr("PuLinTo is off").to_string(),
            (true, false) => tr("PuLinTo: hotkey only").to_string(),
            (true, true) => tr("PuLinTo is working · words fixed: {count}")
                .replace("{count}", &count.to_string()),
        };

        let mut items: Vec<MenuItem<Self>> = vec![
            StandardItem {
                label: status,
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if enabled {
                    tr("Pause").into()
                } else {
                    tr("Turn on").into()
                },
                icon_name: if enabled {
                    "media-playback-pause-symbolic".into()
                } else {
                    "media-playback-start-symbolic".into()
                },
                activate: Box::new(|tray: &mut Self| {
                    let next = if current_mode(&tray.state) == Mode::Off {
                        Mode::Grab
                    } else {
                        Mode::Off
                    };
                    crate::switch_mode(&tray.state, next);
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: tr("Fix automatically").into(),
                checked: auto_on,
                enabled,
                activate: Box::new(|tray: &mut Self| {
                    tray.state.auto_enabled.fetch_xor(true, Ordering::SeqCst);
                    let enabled = tray.state.auto_enabled.load(Ordering::SeqCst);
                    let _ = Config::update(|settings| settings.auto_enabled = enabled);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tr("Restart").into(),
                icon_name: "view-refresh-symbolic".into(),
                activate: Box::new(|tray: &mut Self| {
                    let mode = current_mode(&tray.state);
                    crate::switch_mode(&tray.state, mode);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
        ];
        items.splice(1..1, debug_items());

        // The focused application: a radio group, so the current mode is
        // visible at a glance and changing it is one click.
        if let Some((class, _)) = &focused {
            let modes = [
                crate::xwindow::AppMode::Auto,
                crate::xwindow::AppMode::Manual,
                crate::xwindow::AppMode::Off,
            ];
            let selected = modes
                .iter()
                .position(|mode| Some(*mode) == app_mode)
                .unwrap_or(0);
            let select_class = app_class.clone();
            items.push(
                SubMenu {
                    label: tr("In the application \"{class}\"").replace("{class}", class),
                    icon_name: "view-app-grid-symbolic".into(),
                    submenu: vec![
                        RadioGroup {
                            selected,
                            select: Box::new(move |tray: &mut Self, index| {
                                if let (Some(class), Some(mode)) =
                                    (select_class.as_deref(), modes.get(index))
                                {
                                    set_application_mode(&tray.state, class, *mode);
                                }
                            }),
                            options: vec![
                                RadioItem {
                                    label: tr("Fix automatically").into(),
                                    ..Default::default()
                                },
                                RadioItem {
                                    label: tr("Hotkey only").into(),
                                    ..Default::default()
                                },
                                RadioItem {
                                    label: tr("Leave alone").into(),
                                    ..Default::default()
                                },
                            ],
                        }
                        .into(),
                        MenuItem::Separator,
                        StandardItem {
                            label: tr("Use default").into(),
                            activate: Box::new(move |tray: &mut Self| {
                                if let Some(class) = app_class.as_deref() {
                                    if let Ok(mut policy) = tray.state.app_policy.write() {
                                        if let Err(error) = policy.reset(class) {
                                            eprintln!("Cannot reset the policy of {class}: {error}");
                                        }
                                    }
                                }
                            }),
                            ..Default::default()
                        }
                        .into(),
                        StandardItem {
                            label: match &toggle_label {
                                Some(label) => tr("{label}: switch without opening the menu")
                                    .replace("{label}", label),
                                None => tr("No shortcut is assigned for switching").into(),
                            },
                            enabled: false,
                            ..Default::default()
                        }
                        .into(),
                    ],
                    ..Default::default()
                }
                .into(),
            );
        }

        items.extend([
            StandardItem {
                label: tr("Settings…").into(),
                icon_name: "preferences-system-symbolic".into(),
                activate: Box::new(move |_| open_settings(settings_origin.as_deref())),
                ..Default::default()
            }
            .into(),
            language_menu(),
            SubMenu {
                label: tr("Hints").into(),
                icon_name: "help-about-symbolic".into(),
                submenu: vec![
                    StandardItem {
                        label: tr("The correction key: fix the word; again: the next reading").into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: tr("Shift+Pause/Break: convert the selected text").into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: tr("Ctrl+Alt+\\: report the last fix").into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: tr("Backspace+Esc+Enter together: release the keyboard").into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    MenuItem::Separator,
                    StandardItem {
                        label: format!("PuLinTo {}", env!("CARGO_PKG_VERSION")),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: tr("Quit").into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|_| {
                    crate::flush_output();
                    std::process::exit(0)
                }),
                ..Default::default()
            }
            .into(),
        ]);
        items
    }
}

/// The interface language: system, Russian, English, Italian. The names of the
/// languages are their own and never translated, so a person who cannot read the
/// current one still finds theirs.
fn language_menu() -> ksni::MenuItem<SwitcherTray> {
    use ksni::menu::*;
    let languages = [None, Some(Lang::Ru), Some(Lang::En), Some(Lang::It)];
    let selected = languages
        .iter()
        .position(|lang| *lang == crate::i18n::choice())
        .unwrap_or(0);
    SubMenu {
        label: tr("Interface language").into(),
        icon_name: "preferences-desktop-locale-symbolic".into(),
        submenu: vec![RadioGroup {
            selected,
            select: Box::new(move |_: &mut SwitcherTray, index| {
                if let Some(lang) = languages.get(index) {
                    crate::i18n::choose(*lang);
                }
            }),
            options: vec![
                RadioItem {
                    label: tr("System language").into(),
                    ..Default::default()
                },
                RadioItem {
                    label: "Русский".into(),
                    ..Default::default()
                },
                RadioItem {
                    label: "English".into(),
                    ..Default::default()
                },
                RadioItem {
                    label: "Italiano".into(),
                    ..Default::default()
                },
            ],
        }
        .into()],
        ..Default::default()
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recording_indicator_is_the_icon_with_a_red_dot() {
        for enabled in [true, false] {
            for (icon, plain) in recording_pixmaps(enabled).iter().zip(tray_pixmaps(enabled)) {
                let side = icon.width as usize;
                let pixel = |x: usize, y: usize| &icon.data[(y * side + x) * 4..(y * side + x) * 4 + 4];
                let dot = pixel(side * 3 / 4, side * 3 / 4);
                assert_eq!(dot[0], 255, "the dot is opaque");
                assert!(dot[1] > 200 && dot[2] < 60 && dot[3] < 60, "and red");
                assert_eq!(pixel(0, 0), &plain.data[0..4], "the rest is the usual icon");
            }
        }
    }

    #[test]
    fn settings_are_looked_for_next_to_the_daemon_and_in_the_source_tree() {
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let candidates = settings_candidates();
        assert!(candidates.contains(&exe_dir.join("pulinto-settings")));
        assert!(candidates.contains(&exe_dir.join("../../gui/pulinto-settings")));
        assert_eq!(
            candidates.last().map(PathBuf::as_path),
            Some(Path::new("pulinto-settings"))
        );
    }

    #[test]
    fn tray_pixmaps_match_their_declared_size_and_are_not_blank() {
        for enabled in [true, false] {
            let icons = tray_pixmaps(enabled);
            assert_eq!(icons.len(), 5);
            for icon in &icons {
                let side = icon.width as usize;
                assert_eq!(icon.width, icon.height);
                assert_eq!(
                    icon.data.len(),
                    side * side * 4,
                    "{}x{}",
                    icon.width,
                    icon.height
                );
                // ARGB, network byte order: the first byte of each pixel is the alpha.
                let opaque = icon.data.chunks(4).filter(|px| px[0] == 255).count();
                assert!(
                    opaque * 2 > side * side,
                    "{side} px icon is mostly transparent"
                );
            }
        }
        assert_ne!(tray_pixmaps(true)[0].data, tray_pixmaps(false)[0].data);
    }
}
