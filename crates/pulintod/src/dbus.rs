//! D-Bus interface for pulintod.

use crate::hotkeys::{Action, Binding, Choice, HotkeyConfig, Outcome, Scheme};
use crate::i18n::tr;
use crate::SwitcherState;
use pulinto_core::cascade;
use pulinto_data::profile::{self, Profile};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use zbus::interface;

/// How long the daemon waits for a combination to record.
const RECORD_SECONDS: u64 = 10;

pub struct DaemonDBusService {
    pub state: Arc<SwitcherState>,
    /// Needed because switching the XKB group is a display-server operation
    /// that only the daemon can do for its callers.
    pub layout: Arc<Box<dyn pulinto_layout::LayoutController>>,
    /// The same profiles the daemon decides with. Callers of `Judge` ask here
    /// instead of loading their own copy of the dictionaries: two megabytes of
    /// word lists per process, and worse, two implementations that could drift
    /// apart in what they decide.
    pub profiles: Arc<Vec<Profile>>,
}

/// zbus checks the body signature of a method that takes arguments and runs
/// one that takes none whatever the body carries. A body on such a call is a
/// malformed client, and a call that changes state is refused before it does.
fn body_is_unexpected(header: &zbus::message::Header<'_>, method: &str) -> bool {
    let extra = header
        .signature()
        .is_some_and(|signature| !signature.as_str().is_empty());
    if extra {
        eprintln!("D-Bus: {method} takes no arguments, the call with a body was rejected");
    }
    extra
}

#[interface(name = "ru.pulinto.Daemon1")]
impl DaemonDBusService {
    async fn status(&self) -> String {
        let enabled = self.state.auto_enabled.load(Ordering::SeqCst);
        let count = self.state.conversion_count.load(Ordering::SeqCst);
        let mode = crate::mode_from_index(self.state.mode.load(Ordering::SeqCst));
        format!(
            "{{\"status\":\"running\",\"mode\":\"{}\",\"auto_enabled\":{},\"conversions\":{},\"language\":\"{}\",\"language_choice\":\"{}\"}}",
            mode,
            enabled,
            count,
            crate::i18n::language().code(),
            crate::i18n::choice().map_or("auto", |lang| lang.code())
        )
    }

    /// The interface language the user picked: `ru`, `en`, `it`, or `auto` to
    /// follow the system. The tray and the settings window both follow it.
    async fn set_language(&self, choice: String) -> bool {
        let lang = match choice.as_str() {
            "auto" => None,
            other => match crate::i18n::Lang::parse(other) {
                Some(lang) => Some(lang),
                None => return false,
            },
        };
        crate::i18n::choose(lang);
        true
    }

    /// Which mechanism is currently in charge. Front-ends ask before doing
    /// anything: two correctors editing the same word is what mangled the
    /// user's text, and the fix is that only the mode named here acts.
    async fn get_mode(&self) -> String {
        crate::mode_from_index(self.state.mode.load(Ordering::SeqCst))
            .as_str()
            .to_string()
    }

    /// Change the mode. The daemon persists it and restarts into it, so this
    /// call does not return normally.
    async fn set_mode(&self, mode: String) -> bool {
        match crate::config::Mode::parse(&mode) {
            Some(mode) => {
                let state = Arc::clone(&self.state);
                // Re-exec must not happen inside the D-Bus handler, or the
                // caller never gets its reply.
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    crate::switch_mode(&state, mode);
                });
                true
            }
            None => false,
        }
    }

    /// Re-exec the daemon without changing its settings.  This deliberately
    /// rebuilds the input engine from scratch, so keyboards that appeared
    /// after desktop autostart (Bluetooth is the usual case) are enumerated.
    async fn restart_input(&self, #[zbus(header)] header: zbus::message::Header<'_>) -> bool {
        if body_is_unexpected(&header, "RestartInput") {
            return false;
        }
        let state = Arc::clone(&self.state);
        let mode = crate::mode_from_index(state.mode.load(Ordering::SeqCst));
        // Let D-Bus deliver the successful reply before exec closes the
        // connection.  The settings UI then waits for the name to return.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            crate::switch_mode(&state, mode);
        });
        true
    }

    /// What the settings window needs to know about the Apple keyboard
    /// arrangement, as JSON: whether an Apple keyboard is connected, whether the
    /// server runs `ru(mac)`, whether the daemon's tables agree with it.
    async fn keyboard_info(&self) -> String {
        let settings = crate::config::Config::load();
        crate::keyboard::info(self.layout.as_ref().as_ref(), &settings)
    }

    /// Switch the Apple arrangement of the keyboard on or off and restart, so
    /// the position tables are rebuilt for the new keymap. Returns whether it
    /// worked and, when it did not, why — in words the window can show.
    async fn set_apple_layout(&self, enable: bool) -> (bool, String) {
        // Called directly: zbus runs handlers on its own executor, where there is
        // no Tokio runtime to hand a blocking task to. `setxkbmap` takes a
        // fraction of a second.
        match crate::keyboard::set_apple_layout(enable) {
            Ok(()) => {
                let state = Arc::clone(&self.state);
                let mode = crate::mode_from_index(state.mode.load(Ordering::SeqCst));
                // Let D-Bus deliver the reply before exec closes the connection.
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    crate::switch_mode(&state, mode);
                });
                (true, String::new())
            }
            Err(reason) => (false, reason),
        }
    }

    /// Move the system layout to the next group.
    ///
    /// For a caller that has just corrected a word outside the daemon: without
    /// this the text is fixed but the indicator still shows the old language,
    /// and the user types the next word wrong again.
    async fn toggle_layout(&self, #[zbus(header)] header: zbus::message::Header<'_>) -> bool {
        if body_is_unexpected(&header, "ToggleLayout") {
            return false;
        }
        match self.layout.toggle_next() {
            Ok(()) => true,
            Err(error) => {
                eprintln!("Could not switch the layout: {error}");
                false
            }
        }
    }

    async fn toggle_auto(&self, #[zbus(header)] header: zbus::message::Header<'_>) -> bool {
        if body_is_unexpected(&header, "ToggleAuto") {
            return self.state.auto_enabled.load(Ordering::SeqCst);
        }
        let prev = self.state.auto_enabled.fetch_xor(true, Ordering::SeqCst);
        let enabled = !prev;
        // Persist immediately: a setting that does not survive a restart is not
        // a setting, and losing this one silently re-enabled auto-correction
        // the user had turned off.
        if let Err(error) = crate::config::Config::update(|settings| settings.auto_enabled = enabled) {
            eprintln!("Could not save the settings: {error}");
        }
        enabled
    }

    async fn is_auto_enabled(&self) -> bool {
        self.state.auto_enabled.load(Ordering::SeqCst)
    }

    /// Persistently toggle PuLinTo for the application holding X11 focus.
    async fn toggle_current_app(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> String {
        if body_is_unexpected(&header, "ToggleCurrentApp") {
            return format!(
                "{{\"error\":{}}}",
                json_string(tr("The method takes no arguments."))
            );
        }
        let windows = crate::xwindow::XWindows::connect();
        match crate::toggle_current_application(&self.state, windows.as_ref()) {
            Ok((class, mode)) => format!(
                "{{\"class\":{},\"mode\":{},\"enabled\":{},\"automatic\":{}}}",
                json_string(&class),
                json_string(mode.as_str()),
                mode != crate::xwindow::AppMode::Off,
                mode == crate::xwindow::AppMode::Auto,
            ),
            Err(error) => format!("{{\"error\":{}}}", json_string(&error)),
        }
    }

    async fn set_app_enabled(&self, class: String, enabled: bool) -> bool {
        self.state
            .app_policy
            .write()
            .map_err(|_| ())
            .and_then(|mut policy| {
                policy.set_enabled(&class, enabled).map_err(|error| {
                    eprintln!("D-Bus: SetAppEnabled for «{class}» rejected: {error}")
                })
            })
            .is_ok()
    }

    async fn set_app_mode(&self, class: String, mode: String) -> bool {
        self.state
            .app_policy
            .write()
            .map_err(|_| ())
            .and_then(|mut policy| {
                policy
                    .set_mode_text(&class, &mode)
                    .map_err(|error| eprintln!("D-Bus: SetAppMode for «{class}» rejected: {error}"))
            })
            .is_ok()
    }

    async fn get_app_mode(&self, class: String) -> String {
        self.state
            .app_policy
            .read()
            .map(|policy| policy.mode(&class).as_str().to_string())
            .unwrap_or_else(|_| "auto".into())
    }

    async fn reset_app_policy(&self, class: String) -> bool {
        self.state
            .app_policy
            .write()
            .ok()
            .and_then(|mut policy| policy.reset(&class).ok())
            .unwrap_or(false)
    }

    async fn list_enabled_apps(&self) -> Vec<String> {
        self.state
            .app_policy
            .read()
            .map(|policy| policy.enabled_apps())
            .unwrap_or_default()
    }

    async fn list_disabled_apps(&self) -> Vec<String> {
        self.state
            .app_policy
            .read()
            .map(|policy| policy.disabled_apps())
            .unwrap_or_default()
    }

    async fn list_manual_apps(&self) -> Vec<String> {
        self.state
            .app_policy
            .read()
            .map(|policy| policy.manual_apps())
            .unwrap_or_default()
    }

    async fn list_default_disabled_apps(&self) -> Vec<String> {
        crate::xwindow::AppPolicy::default_disabled_apps()
    }

    async fn list_default_manual_apps(&self) -> Vec<String> {
        crate::xwindow::AppPolicy::default_manual_apps()
    }

    async fn list_default_off_apps(&self) -> Vec<String> {
        crate::xwindow::AppPolicy::default_off_apps()
    }

    async fn exclude_word(&self, word: String) -> bool {
        let mut exceptions = self.state.exceptions.lock().await;
        report_reject(exceptions.add_word_exception(word))
    }

    async fn exclude_word_scoped(&self, word: String, applications: String) -> bool {
        let mut exceptions = self.state.exceptions.lock().await;
        report_reject(exceptions.add_word_exception_scoped(word, applications))
    }

    async fn remove_exclusion(&self, word: String) -> bool {
        let mut exceptions = self.state.exceptions.lock().await;
        exceptions.remove_word_exception(&word)
    }

    async fn list_exclusions(&self) -> Vec<String> {
        let exceptions = self.state.exceptions.lock().await;
        exceptions.list_words()
    }

    async fn list_exclusion_rules(&self) -> Vec<String> {
        let exceptions = self.state.exceptions.lock().await;
        exceptions.list_exclusion_rules()
    }

    /// Symmetric counterpart of `exclude_word`: a word that should convert on
    /// sight, unless a stop rule (password, URL, ...) fires first.
    async fn include_word(&self, word: String) -> bool {
        let mut exceptions = self.state.exceptions.lock().await;
        report_reject(exceptions.add_always_word(word))
    }

    async fn include_word_scoped(&self, word: String, applications: String) -> bool {
        let mut exceptions = self.state.exceptions.lock().await;
        report_reject(exceptions.add_always_word_scoped(word, applications))
    }

    async fn remove_inclusion(&self, word: String) -> bool {
        let mut exceptions = self.state.exceptions.lock().await;
        exceptions.remove_always_word(&word)
    }

    /// Remove a word from both lists and suppress future automatic learning.
    /// The manager persists only a keyed fingerprint, never the word itself.
    async fn forget_word(&self, word: String) -> bool {
        let mut exceptions = self.state.exceptions.lock().await;
        exceptions.forget_word(&word)
    }

    async fn forgotten_word_count(&self) -> u64 {
        let exceptions = self.state.exceptions.lock().await;
        exceptions.forgotten_word_count() as u64
    }

    async fn clear_forgotten_words(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> bool {
        if body_is_unexpected(&header, "ClearForgottenWords") {
            return false;
        }
        let mut exceptions = self.state.exceptions.lock().await;
        exceptions.clear_forgotten_words()
    }

    async fn list_inclusions(&self) -> Vec<String> {
        let exceptions = self.state.exceptions.lock().await;
        exceptions.list_always_words()
    }

    async fn list_inclusion_rules(&self) -> Vec<String> {
        let exceptions = self.state.exceptions.lock().await;
        exceptions.list_inclusion_rules()
    }

    // ── Hotkeys ──────────────────────────────────────────────────────────

    /// Everything the hotkey page shows, as JSON: the choice of set, the
    /// actions, the bindings of both sets with labels, and which set each
    /// attached keyboard is using.
    async fn get_hotkeys(&self) -> String {
        hotkeys_json(&self.state.hotkeys.config())
    }

    /// Replace the bindings of one action in one set. `bindings` are in the
    /// config's own spelling (`ctrl+alt+p`, `pause`, `tap:rctrl`); an empty
    /// list switches the action off. Answers `{"ok":…,"error":…,"warnings":[…]}`;
    /// a warning is a clash with a GNOME shortcut and does not stop the save.
    async fn set_hotkey(&self, scheme: String, action: String, bindings: Vec<String>) -> String {
        let (Some(scheme), Some(action)) = (Scheme::parse(&scheme), Action::parse(&action)) else {
            return hotkey_failure(tr("Unknown set or action."));
        };
        let mut parsed = Vec::new();
        for text in &bindings {
            match Binding::parse(text) {
                Some(binding) => parsed.push(binding),
                None => return hotkey_failure(&tr("Could not parse “{text}”.").replace("{text}", text)),
            }
        }
        let result = self
            .state
            .hotkeys
            .update(|config| config.set_mut(scheme).try_set(action, parsed.clone()));
        self.finish_hotkey_change(result, &parsed)
    }

    /// Put one action — or, with an empty `action`, the whole set — back to
    /// the factory bindings.
    async fn reset_hotkeys(&self, scheme: String, action: String) -> String {
        let Some(scheme) = Scheme::parse(&scheme) else {
            return hotkey_failure(tr("Unknown set."));
        };
        let result = self.state.hotkeys.update(|config| {
            let defaults = scheme.defaults();
            if action.is_empty() {
                *config.set_mut(scheme) = defaults;
                return Ok(());
            }
            let action = Action::parse(&action).ok_or(tr("Unknown action."))?;
            config
                .set_mut(scheme)
                .try_set(action, defaults.get(action).to_vec())
        });
        self.finish_hotkey_change(result, &[])
    }

    /// Which set the keyboards use: `auto` (by keyboard), `apple` or `pc`.
    async fn set_hotkey_choice(&self, choice: String) -> String {
        let Some(choice) = Choice::parse(&choice) else {
            return hotkey_failure(tr("Unknown choice of set."));
        };
        let result = self.state.hotkeys.update(|config| {
            config.choice = choice;
            Ok(())
        });
        self.finish_hotkey_change(result, &[])
    }

    /// The modifiers held so far while a combination is being recorded
    /// (`Ctrl+Alt`), empty when nothing is recorded or nothing is held yet.
    async fn hotkey_recording_progress(&self) -> String {
        self.state.hotkeys.recorder.progress()
    }

    /// Stop a recording at once; the keyboard is the user's again.
    async fn cancel_hotkey_recording(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> bool {
        if body_is_unexpected(&header, "CancelHotkeyRecording") {
            return false;
        }
        self.state.hotkeys.recorder.cancel();
        true
    }

    /// Wait for one combination and answer with it. The daemon swallows the
    /// next combination — or the next tap of a lone modifier — from the
    /// keyboard, so nothing typed meanwhile reaches an application and no
    /// replacement fires; `Esc` cancels, and so does the end of ten seconds.
    /// The answer is JSON with a `status` of `recorded`, `cancelled`,
    /// `timeout`, `rejected` or `unavailable`. Nothing is saved: the window
    /// saves what it chooses with `SetHotkey`.
    async fn record_hotkey(&self, scheme: String, action: String) -> String {
        let (Some(scheme), Some(action)) = (Scheme::parse(&scheme), Action::parse(&action)) else {
            return record_status("unavailable", tr("Unknown set or action."));
        };
        if crate::mode_from_index(self.state.mode.load(Ordering::SeqCst))
            != crate::config::Mode::Grab
        {
            return record_status(
                "unavailable",
                tr("Shortcuts can be recorded only in keyboard grab mode: only then can the daemon swallow a key press."),
            );
        }
        let set = self.state.hotkeys.config().set(scheme).clone();
        let validate =
            Box::new(
                move |binding: &Binding| match set.taken_by_other(action, binding) {
                    Some(other) => Err(tr("“{key}” is already assigned to the action “{action}” in this set.")
                        .replace("{key}", &binding.label())
                        .replace("{action}", other.title())),
                    None => Ok(()),
                },
            );
        let receiver = match self
            .state
            .hotkeys
            .recorder
            .start(validate, Duration::from_secs(RECORD_SECONDS))
        {
            Ok(receiver) => receiver,
            Err(reason) => return record_status("unavailable", &reason),
        };
        match receiver.await {
            Ok(Outcome::Recorded(binding)) => format!(
                "{{\"status\":\"recorded\",\"binding\":{},\"label\":{},\"warnings\":{}}}",
                json_string(&binding.written()),
                json_string(&binding.label()),
                json_list(&crate::gnome_keys::conflicts(&binding)),
            ),
            Ok(Outcome::Cancelled) => record_status("cancelled", ""),
            Ok(Outcome::Timeout) | Err(_) => record_status("timeout", ""),
            Ok(Outcome::Rejected(reason)) => record_status("rejected", &reason),
        }
    }

    /// Preview the positional reading of `text` in the next configured
    /// layout. The settings UI uses the exact same layout tables as the input
    /// engine instead of maintaining a second keyboard map of its own.
    async fn preview_layout_conversion(&self, text: String) -> String {
        if text.is_empty() || self.profiles.len() < 2 {
            return text;
        }
        let from = profile::assume_layout(&self.profiles, &text, None);
        let to = (from + 1) % self.profiles.len();
        pulinto_core::layout::convert(
            &text,
            self.profiles[from].layout(),
            self.profiles[to].layout(),
        )
    }

    /// Drop the in-progress revert/manual-conversion counters without
    /// touching the persisted `never`/`always` lists — for a user who wants
    /// to start the count over rather than have a word graduate by mistake.
    async fn reset_learning(&self, #[zbus(header)] header: zbus::message::Header<'_>) -> bool {
        if body_is_unexpected(&header, "ResetLearning") {
            return false;
        }
        let mut exceptions = self.state.exceptions.lock().await;
        exceptions.reset_learning();
        true
    }

    /// Switches the debug mode on with exactly these options (`panic`, `text`, `decisions`,
    /// `keys`) and starts their timers again, so calling it again extends the recording.
    /// The tray icon carries a red dot while it records.
    async fn start_debug(&self, options: Vec<String>) -> bool {
        let parsed: Option<Vec<crate::debug::Opt>> = options
            .iter()
            .map(|id| crate::debug::Opt::parse(id))
            .collect();
        let Some(parsed) = parsed else {
            eprintln!("D-Bus: StartDebug with an unknown option was rejected");
            return false;
        };
        match crate::debug::start(&parsed) {
            Ok(()) => {
                let minutes = if parsed == [crate::debug::Opt::Keys] { 15 } else { 60 };
                crate::keyboard::notify(
                    &crate::i18n::tr("Debug mode is on: PuLinTo is recording. A red dot on the icon; it stops by itself in {minutes} min.")
                        .replace("{minutes}", &minutes.to_string()),
                );
                crate::tray::refresh();
                true
            }
            Err(error) => {
                eprintln!("Debug mode not started: {error}");
                false
            }
        }
    }

    /// Switches the debug mode off and deletes its logs. With `save`, a copy is put in the
    /// home directory first; its path is returned (an empty string otherwise).
    async fn stop_debug(&self, save: bool) -> String {
        match crate::debug::finish(save) {
            Ok(path) => {
                crate::tray::refresh();
                path.map(|path| path.display().to_string()).unwrap_or_default()
            }
            Err(error) => {
                eprintln!("The debug log was not closed: {error}");
                String::new()
            }
        }
    }

    /// The debug mode as JSON: `recording`, `options`, `seconds_left`, `bytes`, `leftover`
    /// (a finished recording whose log waits for a decision).
    async fn get_debug(&self) -> String {
        crate::debug::status_json()
    }

    /// The last lines of the daemon's output, kept in memory; what to attach to a report.
    async fn get_diagnostics(&self) -> String {
        crate::debug::diagnostics()
    }

    /// Add one exact, persistent personal auto-replacement. These rules are
    /// intentionally separate from the language dictionary and n-grams.
    async fn add_replacement(&self, source: String, replacement: String) -> bool {
        let mut autocorrect = self.state.autocorrect.lock().await;
        match autocorrect.add_user_replacement(source, replacement) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("Auto-replacement not added: {error}");
                false
            }
        }
    }

    async fn add_scoped_replacement(
        &self,
        source: String,
        replacement: String,
        applications: String,
    ) -> bool {
        let mut autocorrect = self.state.autocorrect.lock().await;
        match autocorrect.add_user_replacement_scoped(source, replacement, applications) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("Auto-replacement not added: {error}");
                false
            }
        }
    }

    async fn remove_replacement(&self, source: String) -> bool {
        let mut autocorrect = self.state.autocorrect.lock().await;
        match autocorrect.remove_user_replacement(&source) {
            Ok(removed) => removed,
            Err(error) => {
                eprintln!("Auto-replacement not removed: {error}");
                false
            }
        }
    }

    async fn list_replacements(&self) -> Vec<String> {
        self.state.autocorrect.lock().await.list_user_replacements()
    }

    async fn list_replacement_rules(&self) -> Vec<String> {
        self.state
            .autocorrect
            .lock()
            .await
            .list_user_replacement_rules()
    }

    /// Run the cascade over one word and report the verdict as JSON.
    ///
    /// `context` is the preceding text, from which the languages of the last
    /// two words are derived — stage 2 cannot judge short words without it.
    /// `layout_hint` may be empty, in which case the layout is guessed from the
    /// alphabet of the word itself.
    async fn judge(&self, word: String, context: String, layout_hint: String) -> String {
        let refs = profile::refs(&self.profiles);
        let hint = (!layout_hint.is_empty()).then_some(layout_hint.as_str());
        let current = profile::assume_layout(&self.profiles, &word, hint);
        let context_langs = profile::context_langs(&context);
        let decision = cascade::decide(&word, &refs, current, &context_langs);

        format!(
            "{{\"convert\":{},\"text\":{},\"layout\":\"{}\",\"stage\":\"{:?}\"}}",
            decision.convert,
            json_string(&decision.text),
            decision.layout_id,
            decision.stage
        )
    }
}

impl DaemonDBusService {
    /// Save what a hotkey change produced and answer for the window.
    fn finish_hotkey_change<T>(
        &self,
        result: Result<(T, HotkeyConfig), String>,
        changed: &[Binding],
    ) -> String {
        let (_, config) = match result {
            Ok(done) => done,
            Err(reason) => return hotkey_failure(&reason),
        };
        if let Err(error) = crate::config::Config::update(|settings| settings.hotkeys = config) {
            eprintln!("Could not save the settings: {error}");
            return hotkey_failure(&tr("Could not save the settings: {error}").replace("{error}", &error.to_string()));
        }
        let warnings: Vec<String> = changed
            .iter()
            .flat_map(crate::gnome_keys::conflicts)
            .collect();
        format!(
            "{{\"ok\":true,\"error\":\"\",\"warnings\":{}}}",
            json_list(&warnings)
        )
    }
}

fn hotkey_failure(reason: &str) -> String {
    format!(
        "{{\"ok\":false,\"error\":{},\"warnings\":[]}}",
        json_string(reason)
    )
}

fn record_status(status: &str, error: &str) -> String {
    format!(
        "{{\"status\":\"{status}\",\"error\":{},\"warnings\":[]}}",
        json_string(error)
    )
}

fn json_list(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| json_string(item)).collect();
    format!("[{}]", items.join(","))
}

fn bindings_json(bindings: &[Binding]) -> String {
    let items: Vec<String> = bindings
        .iter()
        .map(|binding| {
            format!(
                "{{\"text\":{},\"label\":{}}}",
                json_string(&binding.written()),
                json_string(&binding.label())
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn hotkeys_json(config: &HotkeyConfig) -> String {
    let actions: Vec<String> = Action::ALL
        .iter()
        .map(|action| {
            format!(
                "{{\"id\":{},\"title\":{}}}",
                json_string(action.id()),
                json_string(action.title())
            )
        })
        .collect();
    let sets: Vec<String> = Scheme::ALL
        .iter()
        .map(|scheme| {
            let defaults = scheme.defaults();
            let entries: Vec<String> = Action::ALL
                .iter()
                .map(|action| {
                    let current = config.set(*scheme).get(*action);
                    format!(
                        "{}:{{\"bindings\":{},\"is_default\":{}}}",
                        json_string(action.id()),
                        bindings_json(current),
                        current == defaults.get(*action)
                    )
                })
                .collect();
            format!("{}:{{{}}}", json_string(scheme.id()), entries.join(","))
        })
        .collect();
    let keyboards: Vec<String> = pulinto_input::capture::keyboards()
        .iter()
        .map(|(_, device)| {
            let vendor = device.input_id().vendor();
            format!(
                "{{\"name\":{},\"scheme\":{}}}",
                json_string(device.name().unwrap_or(tr("(unnamed)"))),
                json_string(config.choice.scheme_for(vendor).id())
            )
        })
        .collect();
    format!(
        "{{\"choice\":{},\"actions\":[{}],\"sets\":{{{}}},\"keyboards\":[{}]}}",
        json_string(config.choice.id()),
        actions.join(","),
        sets.join(","),
        keyboards.join(",")
    )
}

/// D-Bus methods here return a bare `bool`, so a rejection (currently only
/// "too short") has nowhere to carry its reason except
/// stderr — same convention as `toggle_auto`'s save failures just above.
fn report_reject(result: Result<(), pulinto_core::exceptions::RejectReason>) -> bool {
    match result {
        Ok(()) => true,
        Err(reason) => {
            eprintln!("Word not added to the dictionary: {}", reason.message());
            false
        }
    }
}

/// Minimal JSON string escaping — enough for word content, and it keeps the
/// daemon free of a serialisation dependency it would otherwise need only here.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
