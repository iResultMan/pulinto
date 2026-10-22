//! Persistent settings.
//!
//! Exists because of a concrete failure: the daemon used to keep its state only
//! in memory, so anything the user turned off came back on at the next start.
//! During testing that silently re-enabled auto-correction the user had
//! turned off. Settings that do not survive a restart are not settings.

use std::fmt;
use std::fs;
use std::path::PathBuf;

/// Which mechanism does the correcting. Exactly one, never two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Nothing touches the keyboard. The daemon still answers `Judge`, so the
    /// CLI keeps working.
    Off,
    Observe,
    /// Hold the keyboard exclusively and withhold the delimiter until the
    /// cascade has decided. Corrects before Enter reaches the application;
    /// costs an exclusive grab and everything that implies.
    #[default]
    Grab,
    /// Read everything, decide everything, **change nothing**. Every word is
    /// logged with the layout it was read under, the text that produced and
    /// the verdict — and no key is ever injected.
    ///
    /// This exists because a wrong layout reading is invisible from the
    /// outside: the daemon stays silent on exactly the words it misreads, so
    /// the log of what it *did* tells you nothing about what it *saw*. Live
    /// testing with real typing needs an instrument that cannot damage the
    /// text it is measuring.
    Watch,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Observe => "observe",
            Mode::Grab => "grab",
            Mode::Watch => "watch",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "off" => Some(Mode::Off),
            "observe" => Some(Mode::Observe),
            "grab" => Some(Mode::Grab),
            "watch" => Some(Mode::Watch),
            _ => None,
        }
    }

    /// Human-readable label for the tray menu.
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Off => "Off",
            Mode::Observe => "Observe (no grab)",
            Mode::Grab => "Keyboard grab (holds Enter)",
            Mode::Watch => "Diagnostics (only watches, changes nothing)",
        }
    }

    // Appended, never reordered: the index is what `SwitcherState.mode` holds
    // and what the tray and D-Bus pass around.
    pub const ALL: [Mode; 4] = [Mode::Off, Mode::Observe, Mode::Grab, Mode::Watch];

    /// Whether the daemon itself should be touching the key stream.
    pub fn daemon_handles_input(&self) -> bool {
        matches!(self, Mode::Observe | Mode::Grab | Mode::Watch)
    }

    /// Whether the daemon may inject keystrokes. `Watch` reads and decides
    /// like `Observe` and differs in exactly this.
    pub fn corrects_text(&self) -> bool {
        matches!(self, Mode::Observe | Mode::Grab)
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub mode: Mode,
    pub auto_enabled: bool,
    pub key_hold_ms: u64,
    pub post_backspace_ms: u64,
    pub switch_delay_ms: u64,
    /// Hotkeys: the Apple and PC sets and which keyboard uses which.
    pub hotkeys: crate::hotkeys::HotkeyConfig,
    /// The user asked for the Apple arrangement of the keyboard (`ru(mac)`:
    /// comma and full stop on Shift+6/7, ё on `\`). The daemon applies it at
    /// start, because the desktop forgets a layout change made by someone else
    /// as soon as the session ends.
    pub apple_layout: bool,
    /// Interface language the user picked (`ru`, `en`, `it`); empty follows the
    /// system. Shared by the tray menu and the settings window.
    pub language: String,
}

impl Default for Config {
    fn default() -> Self {
        let timing = pulinto_input::injector::Timing::default();
        Self {
            mode: Mode::default(),
            auto_enabled: true,
            key_hold_ms: timing.key_hold_ms,
            post_backspace_ms: timing.post_backspace_ms,
            switch_delay_ms: timing.switch_delay_ms,
            hotkeys: crate::hotkeys::HotkeyConfig::default(),
            apple_layout: false,
            language: String::new(),
        }
    }
}

impl Config {
    pub fn daemon_executable() -> PathBuf {
        std::env::current_exe()
            .ok()
            .map(|path| {
                // After a rebuild the kernel reports the running binary as
                // «…/pulintod (deleted)»; the new build sits at the same path.
                match path
                    .to_str()
                    .and_then(|text| text.strip_suffix(" (deleted)"))
                {
                    Some(fresh) => PathBuf::from(fresh),
                    None => path,
                }
            })
            .filter(|path| path.exists())
            .or_else(|| {
                let installed = PathBuf::from("/usr/local/bin/pulintod");
                installed.exists().then_some(installed)
            })
            .unwrap_or_else(|| PathBuf::from("pulintod"))
    }

    pub fn path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config")
            });
        base.join("pulinto").join("config.conf")
    }

    /// Read the config, falling back to defaults for anything missing or
    /// unparseable. A broken line must never stop the daemon from starting.
    pub fn load() -> Self {
        let text = fs::read_to_string(Self::path()).unwrap_or_default();
        Self::from_text(&text)
    }

    /// Change the saved settings: read them afresh, apply `change`, write them
    /// back. One lock covers the whole read-change-write, so two changes made
    /// at once (a hotkey from the settings window, the language from the tray)
    /// do not overwrite each other. A config that exists but cannot be read is
    /// left alone instead of being replaced by the defaults.
    pub fn update(change: impl FnOnce(&mut Config)) -> std::io::Result<()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let text = match fs::read_to_string(Self::path()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let mut config = Self::from_text(&text);
        change(&mut config);
        config.save()
    }

    pub fn from_text(text: &str) -> Self {
        let mut config = Config::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if config.hotkeys.apply_line(key, value) {
                continue;
            }
            match key {
                "mode" => match Mode::parse(value) {
                    Some(mode) => config.mode = mode,
                    // The IBus engine is gone; a config written for it falls
                    // back to the default mode and is rewritten on the next save.
                    None if value.eq_ignore_ascii_case("ibus") => {
                        static TOLD: std::sync::Once = std::sync::Once::new();
                        TOLD.call_once(|| {
                            eprintln!("Mode «ibus» in the settings is no longer supported, using «grab»")
                        });
                    }
                    None => {}
                },
                "auto_enabled" => config.auto_enabled = value == "true" || value == "yes",
                "apple_layout" => config.apple_layout = value == "true" || value == "yes",
                "language" => {
                    config.language = crate::i18n::Lang::parse(value)
                        .map(|lang| lang.code().to_string())
                        .unwrap_or_default()
                }
                "key_hold_ms" => {
                    if let Ok(parsed) = value.parse() {
                        config.key_hold_ms = parsed;
                    }
                }
                "post_backspace_ms" => {
                    if let Ok(parsed) = value.parse() {
                        config.post_backspace_ms = parsed;
                    }
                }
                "switch_delay_ms" => {
                    if let Ok(parsed) = value.parse() {
                        config.switch_delay_ms = parsed;
                    }
                }
                _ => {}
            }
        }
        config.hotkeys.settle();
        config
    }

    pub fn to_text(&self) -> String {
        format!(
            "# PuLinTo\n\
             # mode: off | observe | grab | watch\n\
             mode = {}\n\
             auto_enabled = {}\n\
             \n\
             # Injection delays, ms\n\
             key_hold_ms = {}\n\
             post_backspace_ms = {}\n\
             switch_delay_ms = {}\n\
             \n\
             # Apple layout: comma and full stop on Shift+6 and Shift+7, ё on the \\ key\n\
             apple_layout = {}\n\
             \n\
             # Interface language: auto | ru | en | it\n\
             language = {}\n\
             \n\
             # Hotkeys. The set is chosen by keyboard: Apple for Apple keyboards,\n\
             # PC for the others (hotkey_set = auto | apple | pc). Only changes are\n\
             # written here; an empty value turns the action off. Actions: word,\n\
             # selection, toggle_app, report, apple_layout. A binding is a chord (ctrl+alt+p, pause)\n\
             # or a single press of a modifier (tap:alt, tap:rctrl).\n\
             {}",
            self.mode,
            self.auto_enabled,
            self.key_hold_ms,
            self.post_backspace_ms,
            self.switch_delay_ms,
            self.apple_layout,
            if self.language.is_empty() {
                "auto"
            } else {
                &self.language
            },
            self.hotkeys.to_lines()
        )
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, self.to_text())
    }

    pub fn timing(&self) -> pulinto_input::injector::Timing {
        pulinto_input::injector::Timing {
            key_hold_ms: self.key_hold_ms,
            post_backspace_ms: self.post_backspace_ms,
            switch_delay_ms: self.switch_delay_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip_through_text() {
        for mode in Mode::ALL {
            assert_eq!(Mode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(Mode::parse("чепуха"), None);
    }

    #[test]
    fn the_removed_ibus_mode_reads_as_the_default() {
        let config = Config::from_text("mode = ibus\nauto_enabled = false\n");
        assert_eq!(config.mode, Mode::Grab);
        // The rest of the file is still read.
        assert!(!config.auto_enabled);
        assert_eq!(Mode::parse("ibus"), None);
    }

    #[test]
    fn only_the_daemon_modes_touch_input() {
        assert!(Mode::Observe.daemon_handles_input());
        assert!(Mode::Grab.daemon_handles_input());
        assert!(!Mode::Off.daemon_handles_input());

        // Diagnostics reads the key stream but must never write to it.
        assert!(Mode::Watch.daemon_handles_input());
        assert!(!Mode::Watch.corrects_text());
        assert!(Mode::Observe.corrects_text());
        assert!(Mode::Grab.corrects_text());
    }

    #[test]
    fn unreadable_config_yields_defaults() {
        // Never a startup failure: a missing file is the normal first run.
        let config = Config::default();
        assert_eq!(config.mode, Mode::Grab);
        assert!(config.auto_enabled);
    }

    #[test]
    fn save_and_load_preserve_everything() {
        let dir = std::env::temp_dir().join("pulinto-config-test");
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: single-threaded test setup.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };

        let written = Config {
            mode: Mode::Grab,
            auto_enabled: false,
            key_hold_ms: 12,
            post_backspace_ms: 25,
            switch_delay_ms: 70,
            hotkeys: {
                let mut hotkeys = crate::hotkeys::HotkeyConfig {
                    choice: crate::hotkeys::Choice::Pc,
                    ..Default::default()
                };
                hotkeys
                    .pc
                    .try_set(
                        crate::hotkeys::Action::Report,
                        vec![crate::hotkeys::Binding::parse("shift+meta+q").unwrap()],
                    )
                    .unwrap();
                hotkeys
            },
            apple_layout: true,
            language: "it".into(),
        };
        written.save().unwrap();
        assert_eq!(Config::load(), written);

        // A change of one setting keeps the others and leaves the autostart
        // entry, which belongs to the installer, where it is.
        let entry = dir.join("autostart").join("pulinto.desktop");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "[Desktop Entry]\n").unwrap();
        Config::update(|config| config.language = "ru".into()).unwrap();
        assert_eq!(
            Config::load(),
            Config {
                language: "ru".into(),
                ..written
            }
        );
        assert!(entry.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A `config.conf` as the previous version wrote it: the two hotkeys are
    /// single bindings that applied to every keyboard.
    const OLD_CONFIG: &str = "# PuLinTo\n\
        mode = grab\n\
        auto_enabled = false\n\
        key_hold_ms = 12\n\
        post_backspace_ms = 25\n\
        switch_delay_ms = 70\n\
        report_hotkey = ctrl+alt+43\n\
        toggle_hotkey = ctrl+shift+meta+25\n\
        autostart = false\n\
        apple_layout = true\n";

    #[test]
    fn a_config_from_before_the_editor_still_reads() {
        use crate::hotkeys::{Action, Binding, Scheme};
        let config = Config::from_text(OLD_CONFIG);
        assert_eq!(config.mode, Mode::Grab);
        assert!(!config.auto_enabled);
        assert!(config.apple_layout);
        assert_eq!(config.key_hold_ms, 12);
        let toggle = Binding::parse("ctrl+shift+meta+25").unwrap();
        for scheme in Scheme::ALL {
            let set = config.hotkeys.set(scheme);
            // The default report key was never changed, the toggle was.
            assert_eq!(
                set.get(Action::Report),
                Scheme::Pc.defaults().get(Action::Report)
            );
            assert_eq!(set.get(Action::ToggleApp), [toggle]);
        }
        // The new defaults of each set are untouched by the old file.
        assert_eq!(
            config.hotkeys.apple.get(Action::Word),
            Scheme::Apple.defaults().get(Action::Word)
        );
        assert_eq!(
            config.hotkeys.pc.get(Action::Word),
            Scheme::Pc.defaults().get(Action::Word)
        );
    }

    #[test]
    fn rewriting_an_old_config_moves_the_hotkeys_to_the_new_lines() {
        let config = Config::from_text(OLD_CONFIG);
        let text = config.to_text();
        assert!(!text.contains("toggle_hotkey"), "{text}");
        assert!(
            text.contains("hotkey.apple.toggle_app = ctrl+shift+meta+25"),
            "{text}"
        );
        assert!(
            text.contains("hotkey.pc.toggle_app = ctrl+shift+meta+25"),
            "{text}"
        );
        assert!(
            !text.contains("hotkey.pc.report"),
            "the default is not written: {text}"
        );
        assert_eq!(Config::from_text(&text), config);
    }

    #[test]
    fn a_default_config_writes_no_hotkey_lines() {
        let text = Config::default().to_text();
        assert!(
            !text.lines().any(|line| line.starts_with("hotkey")),
            "{text}"
        );
        assert_eq!(Config::from_text(&text), Config::default());
    }

    #[test]
    fn the_hotkey_lines_do_not_disturb_the_other_settings() {
        let mut config = Config {
            apple_layout: true,
            ..Config::default()
        };
        config.hotkeys.choice = crate::hotkeys::Choice::Apple;
        config.hotkeys.pc.set(crate::hotkeys::Action::Word, vec![]);
        assert_eq!(Config::from_text(&config.to_text()), config);
    }
}
