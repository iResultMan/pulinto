//! The registry of real mistakes.
//!
//! Every measurement this project trusts came from live use, not from a corpus:
//! the lone comma that became `б`, the `ТФЫФ` that had to become `NASA`, the
//! group number that meant a different layout after IBus rebuilt the keymap.
//! None of them were in the 21 300-word news corpus, because a corpus contains
//! what a corpus contains. So the user needs a way to say **"that one was
//! wrong"** at the moment it happens, and the daemon needs to already be
//! holding everything required to work out why.
//!
//! Two things make this harder than logging the last decision.
//!
//! First, the user notices *afterwards*. At speed, the word that went wrong is
//! one or two words back by the time a hand reaches the hotkey — so the daemon
//! keeps a short history and reports the lot, newest first, rather than
//! guessing which one was meant.
//!
//! Second, a decision that looks wrong on screen may have been right in the
//! cascade and wrong in the mechanics — that is what a whole day of debugging
//! once turned out to be. A report that says only "`ghbdtn` was not corrected" does
//! not separate the two. So each entry carries the decision *and* the state it
//! was made against: every reading of the word, the layout in effect, the
//! layouts loaded, the stage that answered, and whether the replacement was
//! actually carried out.

use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// How many decisions back the report reaches.
///
/// The customer asked for one or two; this keeps eight. The cost is a few
/// hundred bytes, and the case it covers is real — noticing a bad correction
/// only after finishing the sentence.
const HISTORY: usize = 8;

/// One decision, with the state it was taken against.
#[derive(Debug, Clone)]
pub struct Observed {
    pub at: u64,
    /// The word as it stood on screen, under the layout in effect.
    pub typed: String,
    pub strokes: Vec<(u16, bool)>,
    /// Every reading of those keys: `(layout id, text)`. The alternative the
    /// cascade weighed is in here, whether or not it was chosen.
    pub readings: Vec<(String, String)>,
    pub current_layout: String,
    pub loaded_layouts: Vec<String>,
    pub context: Vec<String>,
    pub stage: String,
    pub converted_to: Option<String>,
    /// False when the cascade said convert and the mechanics did not: the
    /// distinction that took a fortnight to learn to ask about.
    pub applied: bool,
}

#[derive(Debug, Default)]
pub struct Registry {
    recent: Mutex<VecDeque<Observed>>,
}

impl Registry {
    /// Remember a decision. Called for every word the daemon judges, including
    /// the ones it decides to leave alone — "it should have corrected this and
    /// did not" is a report we want just as much.
    pub fn note(&self, entry: Observed) {
        if let Ok(mut recent) = self.recent.lock() {
            if recent.len() == HISTORY {
                recent.pop_front();
            }
            recent.push_back(entry);
        }
    }

    /// Write everything held to the registry file and return its path.
    ///
    /// `selection` is the text the user had highlighted, when there was any: it
    /// names the offending word outright and saves the guessing. `window` is the
    /// `WM_CLASS` of whatever had focus, because several defects so far were
    /// specific to one kind of application.
    pub fn report(
        &self,
        selection: Option<&str>,
        window: Option<&str>,
    ) -> std::io::Result<PathBuf> {
        let entries: Vec<Observed> = self
            .recent
            .lock()
            .map(|recent| recent.iter().rev().cloned().collect())
            .unwrap_or_default();

        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)?;
        // `mode` only applies when the file is first created. Also repair
        // permissions left by older PuLinTo versions.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;

        let mut line = String::from("{");
        push_field(&mut line, "at", &json_string(&now_iso()));
        line.push(',');
        push_field(&mut line, "selection", &opt_string(selection));
        line.push(',');
        push_field(&mut line, "window", &opt_string(window));
        line.push(',');
        line.push_str("\"recent\":[");
        for (index, entry) in entries.iter().enumerate() {
            if index > 0 {
                line.push(',');
            }
            line.push_str(&entry.to_json());
        }
        line.push_str("]}");

        writeln!(file, "{line}")?;
        Ok(path)
    }

    pub fn path() -> PathBuf {
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                    .join(".local")
                    .join("share")
            });
        base.join("pulinto").join("reports.jsonl")
    }
}

impl Observed {
    fn to_json(&self) -> String {
        let mut out = String::from("{");
        push_field(&mut out, "at_ms", &self.at.to_string());
        out.push(',');
        push_field(&mut out, "typed", &json_string(&self.typed));
        out.push(',');
        push_field(&mut out, "stage", &json_string(&self.stage));
        out.push(',');
        push_field(
            &mut out,
            "converted_to",
            &opt_string(self.converted_to.as_deref()),
        );
        out.push(',');
        push_field(
            &mut out,
            "applied",
            if self.applied { "true" } else { "false" },
        );
        out.push(',');
        push_field(&mut out, "layout", &json_string(&self.current_layout));
        out.push(',');
        push_field(&mut out, "loaded", &json_array(&self.loaded_layouts));
        out.push(',');
        push_field(&mut out, "context", &json_array(&self.context));
        out.push(',');
        let readings: Vec<String> = self
            .readings
            .iter()
            .map(|(id, text)| format!("{}:{}", id, text))
            .collect();
        push_field(&mut out, "readings", &json_array(&readings));
        out.push(',');
        let codes: Vec<String> = self
            .strokes
            .iter()
            .map(|(code, shift)| {
                if *shift {
                    format!("+{code}")
                } else {
                    code.to_string()
                }
            })
            .collect();
        push_field(&mut out, "codes", &json_array(&codes));
        out.push('}');
        out
    }
}

fn push_field(out: &mut String, name: &str, raw_value: &str) {
    out.push('"');
    out.push_str(name);
    out.push_str("\":");
    out.push_str(raw_value);
}

fn json_array(items: &[String]) -> String {
    let mut out = String::from("[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(item));
    }
    out.push(']');
    out
}

fn opt_string(value: Option<&str>) -> String {
    match value {
        Some(text) => json_string(text),
        None => "null".into(),
    }
}

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

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

fn now_iso() -> String {
    // No date library in the dependency list, and one is not worth adding for a
    // timestamp: milliseconds since the epoch are unambiguous and sort right.
    now_ms().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str) -> Observed {
        Observed {
            at: 1,
            typed: word.into(),
            strokes: vec![(34, false), (35, true)],
            readings: vec![("us".into(), word.into()), ("ru".into(), "привет".into())],
            current_layout: "us".into(),
            loaded_layouts: vec!["us".into(), "ru".into()],
            context: vec!["он".into(), "сказал".into()],
            stage: "Dictionary".into(),
            converted_to: Some("привет".into()),
            applied: true,
        }
    }

    #[test]
    fn the_history_keeps_the_newest_and_drops_the_oldest() {
        let registry = Registry::default();
        for index in 0..HISTORY + 3 {
            registry.note(entry(&format!("слово{index}")));
        }
        let recent = registry.recent.lock().unwrap();
        assert_eq!(recent.len(), HISTORY);
        assert_eq!(recent.front().unwrap().typed, "слово3");
        assert_eq!(
            recent.back().unwrap().typed,
            format!("слово{}", HISTORY + 2)
        );
    }

    /// The report is read by a machine before it is read by a person, so a word
    /// containing a quote must not end the JSON early.
    #[test]
    fn text_is_escaped() {
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("строка\nвторая"), "\"строка\\nвторая\"");
    }

    #[test]
    fn an_entry_carries_both_readings_and_the_keys() {
        let json = entry("ghbdtn").to_json();
        assert!(json.contains("\"typed\":\"ghbdtn\""));
        assert!(json.contains("us:ghbdtn"));
        assert!(json.contains("ru:привет"));
        assert!(json.contains("\"codes\":[\"34\",\"+35\"]"));
        assert!(json.contains("\"applied\":true"));
    }
}
