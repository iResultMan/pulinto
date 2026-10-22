//! Does a hotkey collide with one GNOME already owns?
//!
//! Only a warning, never a refusal: the user may well want PuLinTo's key over
//! GNOME's, and it is GNOME that decides who gets a key it grabs. Read-only —
//! `gsettings` is asked, nothing is ever written.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::hotkeys::{Binding, Chord, Modifier};
use crate::i18n::tr;

/// The schemas whose keys are shortcuts: the system ones and the window
/// manager's.
const SCHEMAS: [&str; 2] = [
    "org.gnome.settings-daemon.plugins.media-keys",
    "org.gnome.desktop.wm.keybindings",
];

const CUSTOM_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";

/// Warnings, in words, for a binding that GNOME has taken for itself.
pub fn conflicts(binding: &Binding) -> Vec<String> {
    match binding {
        Binding::Chord(chord) => taken_shortcuts()
            .into_iter()
            .filter(|(taken, _)| taken == chord)
            .map(|(taken, what)| tr("{key} is already taken in GNOME: {what}.").replace("{key}", &taken.label()).replace("{what}", &what))
            .collect(),
        Binding::Tap(tap) if tap.modifier == Modifier::Meta => {
            let overlay =
                gsettings(&["get", "org.gnome.mutter", "overlay-key"]).unwrap_or_default();
            if overlay.contains("Super") {
                vec![
                    tr("A single press of Super opens the window overview in GNOME; PuLinTo will take it for itself, and the overview will open only with Super plus another key.")
                        .to_string(),
                ]
            } else {
                Vec::new()
            }
        }
        Binding::Tap(_) => Vec::new(),
    }
}

/// Every shortcut GNOME has, as a chord and a name for it.
fn taken_shortcuts() -> Vec<(Chord, String)> {
    let mut taken = Vec::new();
    for schema in SCHEMAS {
        let Some(listing) = gsettings(&["list-recursively", schema]) else {
            continue;
        };
        for line in listing.lines() {
            // "<schema> <key> <value>"
            let mut parts = line.splitn(3, ' ');
            let (_, Some(key), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            for text in quoted_strings(value) {
                if let Some(chord) = chord_of_accelerator(&text) {
                    taken.push((chord, format!("«{key}»")));
                }
            }
        }
    }
    // The shortcuts the user made themselves.
    if let Some(list) = gsettings(&["get", MEDIA_KEYS, "custom-keybindings"]) {
        for path in quoted_strings(&list) {
            let schema = format!("{CUSTOM_SCHEMA}:{path}");
            let binding = gsettings(&["get", &schema, "binding"]).unwrap_or_default();
            let name = gsettings(&["get", &schema, "name"])
                .and_then(|text| quoted_strings(&text).into_iter().next())
                .unwrap_or_else(|| tr("a custom shortcut").into());
            for text in quoted_strings(&binding) {
                if let Some(chord) = chord_of_accelerator(&text) {
                    taken.push((chord, format!("«{name}»")));
                }
            }
        }
    }
    taken
}

/// The single-quoted strings of a GVariant value: `['<Super>l', 'XF86Lock']`.
fn quoted_strings(value: &str) -> Vec<String> {
    value
        .split('\'')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, text)| text.to_string())
        .collect()
}

/// A GTK accelerator (`<Control><Alt>t`) as a chord, when its key is one the
/// daemon can bind. Keys it cannot (XF86 media keys, say) are not conflicts
/// with anything it could be asked to record.
fn chord_of_accelerator(text: &str) -> Option<Chord> {
    let mut chord = Chord {
        code: 0,
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };
    let mut rest = text.trim();
    while let Some(after) = rest.strip_prefix('<') {
        let (name, tail) = after.split_once('>')?;
        match name.to_ascii_lowercase().as_str() {
            "control" | "primary" | "ctrl" => chord.ctrl = true,
            "alt" | "mod1" => chord.alt = true,
            "shift" => chord.shift = true,
            "super" | "mod4" | "meta" => chord.meta = true,
            _ => return None,
        }
        rest = tail;
    }
    let key = match rest {
        "Escape" => "esc",
        "Return" => "enter",
        "BackSpace" => "backspace",
        "Scroll_Lock" => "scrolllock",
        "Page_Up" | "Prior" => "pageup",
        "Page_Down" | "Next" => "pagedown",
        "Print" | "Sys_Req" => "print",
        "Pause" => "pause",
        "Tab" | "ISO_Left_Tab" => "tab",
        "grave" => "`",
        "minus" => "-",
        "equal" => "=",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        "backslash" => "\\",
        "semicolon" => ";",
        "apostrophe" => "'",
        "bracketleft" => "[",
        "bracketright" => "]",
        other => other,
    };
    // Reuse the config's own reader for the key itself.
    chord.code = Chord::parse(&key.to_ascii_lowercase())?.code;
    Some(chord)
}

/// `gsettings` with a deadline: a broken dconf must not hold the daemon's
/// D-Bus handler for longer than a couple of seconds.
fn gsettings(args: &[&str]) -> Option<String> {
    let mut child = Command::new("gsettings")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut out = String::new();
                std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
                return Some(out);
            }
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_become_chords() {
        assert_eq!(
            chord_of_accelerator("<Control><Alt>t"),
            Some(Chord::parse("ctrl+alt+t").unwrap())
        );
        assert_eq!(
            chord_of_accelerator("<Super>l"),
            Some(Chord::parse("meta+l").unwrap())
        );
        assert_eq!(
            chord_of_accelerator("<Primary><Shift>Print"),
            Some(Chord::parse("ctrl+shift+print").unwrap())
        );
        assert_eq!(
            chord_of_accelerator("<Control><Alt>backslash"),
            Some(Chord::REPORT)
        );
        assert_eq!(
            chord_of_accelerator("<Alt>F4"),
            Some(Chord::parse("alt+f4").unwrap())
        );
        assert_eq!(
            chord_of_accelerator("<Alt>Tab"),
            Some(Chord::parse("alt+tab").unwrap())
        );
    }

    #[test]
    fn what_the_daemon_cannot_bind_is_no_conflict() {
        assert_eq!(chord_of_accelerator("XF86AudioMute"), None);
        assert_eq!(chord_of_accelerator("<Unknown>x"), None);
        assert_eq!(chord_of_accelerator(""), None);
    }

    #[test]
    fn a_gvariant_list_gives_its_strings() {
        assert_eq!(
            quoted_strings("['<Super>l', 'XF86ScreenSaver']"),
            ["<Super>l", "XF86ScreenSaver"]
        );
        assert!(quoted_strings("@as []").is_empty());
        assert_eq!(quoted_strings("'Terminal'"), ["Terminal"]);
    }
}
