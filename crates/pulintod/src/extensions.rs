//! Turns the GNOME Shell extensions on for the user, once, so that installing
//! the program is a single step.
//!
//! The extension `pulinto@pulinto.ru` reports the focused window (per-application
//! rules under Wayland); the AppIndicator extension shows the tray icon on a GNOME
//! that does not ship it. A new extension is only noticed by GNOME at the next
//! login, so `gnome-extensions enable` cannot be used: the list of enabled
//! extensions is a plain setting and GNOME reads it at login. Corrections do not
//! wait for any of this; they work from the first key.
//!
//! The only place that does it: install.sh and the packages leave it to the first
//! start of the daemon for each user. Done once per user (a marker file in the
//! state directory), once our extension is installed: a user who turns an
//! extension off later is not overruled at the next start.

use std::path::PathBuf;
use std::process::Command;

const OURS: &str = "pulinto@pulinto.ru";
const APPINDICATORS: [&str; 2] = [
    "appindicatorsupport@rgcjonas.gmail.com",
    "ubuntu-appindicators@ubuntu.com",
];

/// The names in a `gsettings` string list (`['a', 'b']` or `@as []`).
fn parse_list(text: &str) -> Vec<String> {
    let inner = text.trim();
    let inner = inner.strip_prefix("@as").unwrap_or(inner).trim();
    let inner = inner.trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|item| item.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn format_list(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|name| format!("'{name}'")).collect();
    format!("[{}]", quoted.join(", "))
}

/// `current` with every name of `wanted` added, or `None` when nothing is missing.
/// A name the user has switched off (`disabled`) is left alone.
fn with_missing(current: &str, disabled: &str, wanted: &[&str]) -> Option<String> {
    let mut names = parse_list(current);
    let off = parse_list(disabled);
    let before = names.len();
    for name in wanted {
        if !names.iter().any(|n| n == name) && !off.iter().any(|n| n == name) {
            names.push((*name).to_string());
        }
    }
    (names.len() != before).then(|| format_list(&names))
}

fn installed(uuid: &str) -> bool {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    [
        home,
        Some(PathBuf::from("/usr/local/share")),
        Some(PathBuf::from("/usr/share")),
    ]
    .into_iter()
    .flatten()
    .any(|base| base.join("gnome-shell/extensions").join(uuid).is_dir())
}

fn gsettings(args: &[&str]) -> Option<String> {
    let output = Command::new("gsettings").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn marker() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(state.join("pulinto").join("extensions-enabled"))
}

pub fn ensure_enabled_once() {
    let on_gnome =
        std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| desktop.contains("GNOME"));
    let Some(marker) = marker() else { return };
    if !on_gnome || marker.exists() {
        return;
    }
    let wanted: Vec<&str> = std::iter::once(OURS)
        .chain(APPINDICATORS)
        .filter(|uuid| installed(uuid))
        .collect();
    // Without our extension there is nothing to settle yet: the next start tries again.
    if !wanted.contains(&OURS) {
        return;
    }
    let (Some(current), disabled) = (
        gsettings(&["get", "org.gnome.shell", "enabled-extensions"]),
        gsettings(&["get", "org.gnome.shell", "disabled-extensions"]).unwrap_or_default(),
    ) else {
        return;
    };
    if let Some(updated) = with_missing(&current, &disabled, &wanted) {
        if gsettings(&["set", "org.gnome.shell", "enabled-extensions", &updated]).is_some() {
            println!(
                "GNOME Shell extensions enabled ({}); they start working after the next login",
                wanted.join(", ")
            );
        } else {
            return;
        }
    }
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&marker, "");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_gsettings_lists() {
        assert_eq!(parse_list("['a@b', 'c']"), ["a@b", "c"]);
        assert_eq!(parse_list("@as []"), Vec::<String>::new());
        assert_eq!(format_list(&["a".into(), "b".into()]), "['a', 'b']");
    }

    #[test]
    fn adds_only_what_is_missing_and_respects_a_disabled_extension() {
        let wanted = [OURS, "ubuntu-appindicators@ubuntu.com"];
        assert_eq!(
            with_missing("['user-theme@x']", "@as []", &wanted).as_deref(),
            Some("['user-theme@x', 'pulinto@pulinto.ru', 'ubuntu-appindicators@ubuntu.com']")
        );
        assert_eq!(
            with_missing("@as []", "@as []", &wanted).as_deref(),
            Some("['pulinto@pulinto.ru', 'ubuntu-appindicators@ubuntu.com']")
        );
        // Already there: nothing to write.
        assert_eq!(
            with_missing(
                "['pulinto@pulinto.ru', 'ubuntu-appindicators@ubuntu.com']",
                "@as []",
                &wanted
            ),
            None
        );
        // The user switched ours off: it stays off.
        assert_eq!(
            with_missing("['a']", "['pulinto@pulinto.ru']", &wanted).as_deref(),
            Some("['a', 'ubuntu-appindicators@ubuntu.com']")
        );
    }
}
