//! The keyboard's XKB configuration as the user would write it — rules, model,
//! layouts, variants, options — and the one change PuLinTo makes to it: the
//! Apple arrangement of the letters and punctuation.
//!
//! The change goes through `setxkbmap`, the same tool the desktop's own keyboard
//! settings use. It rebuilds the keymap on the running server and updates
//! `_XKB_RULES_NAMES`, which is what the layout watcher and every indicator
//! read; compiling a keymap ourselves would mean carrying xkbcomp along.

use std::process::Command;

/// Layouts for which XKB has a `mac` variant that matters to us. Other layouts
/// are left exactly as the user configured them: `de(mac)` exists too, but
/// nobody asked for it, and we have no table for it.
const APPLE_CAPABLE: [&str; 2] = ["us", "ru"];

/// The Apple variant's name in XKB.
pub const APPLE_VARIANT: &str = "mac";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rmlvo {
    pub rules: String,
    pub model: String,
    pub layouts: Vec<String>,
    /// One entry per layout, empty for the standard arrangement.
    pub variants: Vec<String>,
    pub options: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("setxkbmap not found (package x11-xkb-utils): {0}")]
    ToolMissing(String),
    #[error("setxkbmap reported an error: {0}")]
    Failed(String),
    #[error("setxkbmap did not say which layout is loaded")]
    Unreadable,
}

impl Rmlvo {
    /// What `setxkbmap -query` reports for the running server.
    pub fn query() -> Result<Self, ConfigError> {
        let output = Command::new("setxkbmap")
            .arg("-query")
            .output()
            .map_err(|error| ConfigError::ToolMissing(error.to_string()))?;
        if !output.status.success() {
            return Err(ConfigError::Failed(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Self::parse_query(&String::from_utf8_lossy(&output.stdout)).ok_or(ConfigError::Unreadable)
    }

    pub fn parse_query(text: &str) -> Option<Self> {
        let field = |name: &str| -> String {
            text.lines()
                .find_map(|line| line.strip_prefix(name))
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        };
        let layouts: Vec<String> = split_list(&field("layout:"));
        if layouts.is_empty() {
            return None;
        }
        let mut variants = split_list_keeping_empty(&field("variant:"));
        variants.resize(layouts.len(), String::new());
        Some(Self {
            rules: field("rules:"),
            model: field("model:"),
            layouts,
            variants,
            options: field("options:"),
        })
    }

    /// Does the Russian layout — the one whose keys actually move — run the
    /// Apple arrangement?
    pub fn is_apple(&self) -> bool {
        self.layouts
            .iter()
            .zip(&self.variants)
            .any(|(layout, variant)| layout == "ru" && variant == APPLE_VARIANT)
    }

    /// The same configuration with the Apple arrangement switched on or off for
    /// the layouts that have one. A variant the user chose themselves — Russian
    /// `phonetic`, say — is never overwritten: switching on leaves that layout
    /// as it was, and [`Rmlvo::is_apple`] then says it did not take.
    pub fn with_apple(&self, enabled: bool) -> Self {
        let mut next = self.clone();
        for (layout, variant) in next.layouts.iter().zip(next.variants.iter_mut()) {
            if !APPLE_CAPABLE.contains(&layout.as_str()) {
                continue;
            }
            if enabled && variant.is_empty() {
                *variant = APPLE_VARIANT.to_string();
            } else if !enabled && variant == APPLE_VARIANT {
                variant.clear();
            }
        }
        next
    }

    /// The variant standing in the way of the Apple arrangement for Russian, if
    /// the user has chosen another one.
    pub fn foreign_russian_variant(&self) -> Option<&str> {
        self.layouts
            .iter()
            .zip(&self.variants)
            .find(|(layout, variant)| {
                *layout == "ru" && !variant.is_empty() && *variant != APPLE_VARIANT
            })
            .map(|(_, variant)| variant.as_str())
    }

    /// The arguments that make `setxkbmap` load exactly this configuration.
    /// `-option ""` first: without it the options would be added to whatever
    /// the server already had, and a switch off would leave the old ones behind.
    pub fn setxkbmap_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if !self.rules.is_empty() {
            args.extend(["-rules".to_string(), self.rules.clone()]);
        }
        if !self.model.is_empty() {
            args.extend(["-model".to_string(), self.model.clone()]);
        }
        args.extend(["-layout".to_string(), self.layouts.join(",")]);
        args.extend(["-variant".to_string(), self.variants.join(",")]);
        args.extend(["-option".to_string(), String::new()]);
        if !self.options.is_empty() {
            args.extend(["-option".to_string(), self.options.clone()]);
        }
        args
    }

    /// Load this configuration on the running X server.
    pub fn apply(&self) -> Result<(), ConfigError> {
        let output = Command::new("setxkbmap")
            .args(self.setxkbmap_args())
            .output()
            .map_err(|error| ConfigError::ToolMissing(error.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(ConfigError::Failed(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
    }
}

/// Whether `setxkbmap` can be run at all — the settings window greys the switch
/// out, with the reason, when it cannot.
pub fn tool_available() -> bool {
    Command::new("setxkbmap").arg("-version").output().is_ok()
}

fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// Variants are positional, so an empty entry is meaningful: `,mac` is "standard
/// first, Apple second".
fn split_list_keeping_empty(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split(',').map(|part| part.trim().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUERY: &str = "rules:      evdev\nmodel:      pc105\nlayout:     us,ru\nvariant:    ,\noptions:    grp:alt_shift_toggle,grp_led:scroll\n";

    #[test]
    fn reads_what_setxkbmap_reports() {
        let config = Rmlvo::parse_query(QUERY).unwrap();
        assert_eq!(config.layouts, vec!["us", "ru"]);
        assert_eq!(config.variants, vec!["", ""]);
        assert_eq!(config.options, "grp:alt_shift_toggle,grp_led:scroll");
        assert!(!config.is_apple());
    }

    #[test]
    fn a_report_without_a_layout_is_not_a_configuration() {
        assert!(Rmlvo::parse_query("rules: evdev\n").is_none());
    }

    #[test]
    fn switching_on_sets_the_apple_variant_for_both_layouts() {
        let apple = Rmlvo::parse_query(QUERY).unwrap().with_apple(true);
        assert_eq!(apple.variants, vec!["mac", "mac"]);
        assert!(apple.is_apple());
        assert_eq!(apple.options, "grp:alt_shift_toggle,grp_led:scroll");
    }

    #[test]
    fn switching_off_restores_the_standard_arrangement() {
        let plain = Rmlvo::parse_query(QUERY)
            .unwrap()
            .with_apple(true)
            .with_apple(false);
        assert_eq!(plain.variants, vec!["", ""]);
        assert!(!plain.is_apple());
    }

    /// The user's own choices are not ours to touch: a Ukrainian layout stays as
    /// it was, and a Russian variant they picked is not overwritten either way.
    #[test]
    fn layouts_and_variants_the_user_chose_are_left_alone() {
        let text = "rules: evdev\nmodel: pc105\nlayout: us,ru,ua\nvariant: ,phonetic,winkeys\noptions: grp:caps_toggle\n";
        let config = Rmlvo::parse_query(text).unwrap();
        let on = config.with_apple(true);
        assert_eq!(on.variants, vec!["mac", "phonetic", "winkeys"]);
        assert!(!on.is_apple(), "Russian stayed phonetic, so Apple did not take");
        assert_eq!(on.foreign_russian_variant(), Some("phonetic"));
        assert_eq!(config.with_apple(false).variants, vec!["", "phonetic", "winkeys"]);
        // And one that is ours comes off cleanly.
        assert_eq!(on.with_apple(false).variants, vec!["", "phonetic", "winkeys"]);
    }

    #[test]
    fn the_command_replaces_the_options_instead_of_adding_to_them() {
        let args = Rmlvo::parse_query(QUERY).unwrap().with_apple(true).setxkbmap_args();
        assert_eq!(
            args,
            [
                "-rules", "evdev", "-model", "pc105", "-layout", "us,ru", "-variant", "mac,mac",
                "-option", "", "-option", "grp:alt_shift_toggle,grp_led:scroll"
            ]
        );
    }
}
