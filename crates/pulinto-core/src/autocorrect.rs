//! Auto-correction for common typos and custom abbreviations.
//!
//! Exact user replacements deliberately live outside the language dictionary.
//! An FST word list and its n-grams answer "which language is this?"; teaching
//! either of them that one literal spelling must become another would leak a
//! personal typing habit into every language decision. User replacements are
//! instead persisted as exact `source<TAB>replacement` pairs in
//! `~/.local/share/pulinto/replacements.tsv`.

use std::collections::HashMap;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use crate::case::{self, fix_incidental_caps, fix_two_capitals};
use crate::scope::AppScope;

const USER_REPLACEMENTS_FILE: &str = "replacements.tsv";

#[derive(Debug, Clone, Default)]
pub struct AutoCorrectEngine {
    abbreviations: HashMap<String, String>,
    user_replacements: HashMap<String, ReplacementRule>,
    dictionary_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ReplacementRule {
    replacement: String,
    scope: AppScope,
}

impl AutoCorrectEngine {
    /// In-memory engine, used by library callers and tests that need no I/O.
    pub fn new() -> Self {
        let mut engine = Self::default();
        // Default sample abbreviation. Abbreviations are queried explicitly;
        // they are not silently expanded by the typo path.
        engine.add_abbreviation("др", "с днём рождения");
        engine
    }

    /// Production engine backed by the user's exact-replacement dictionary.
    pub fn with_user_dictionary() -> Self {
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                    .join(".local")
                    .join("share")
            });
        Self::with_dictionary_file(base.join("pulinto").join(USER_REPLACEMENTS_FILE))
    }

    /// Same engine with an explicit file, primarily so persistence is testable
    /// without changing process-wide environment variables.
    pub fn with_dictionary_file(path: PathBuf) -> Self {
        let mut engine = Self::new();
        engine.dictionary_file = Some(path);
        engine.load_user_replacements();
        engine
    }

    pub fn add_abbreviation(&mut self, shortcut: impl Into<String>, expansion: impl Into<String>) {
        self.abbreviations
            .insert(shortcut.into().to_lowercase(), expansion.into());
    }

    pub fn remove_abbreviation(&mut self, shortcut: &str) -> bool {
        self.abbreviations.remove(&shortcut.to_lowercase()).is_some()
    }

    pub fn check_abbreviation(&self, word: &str) -> Option<String> {
        self.abbreviations.get(&word.to_lowercase()).cloned()
    }

    /// Add or replace one persistent, exact user correction.
    pub fn add_user_replacement(
        &mut self,
        source: impl Into<String>,
        replacement: impl Into<String>,
    ) -> io::Result<()> {
        self.add_user_replacement_scoped(source, replacement, "")
    }

    /// Add or replace an exact correction limited to a comma-separated list
    /// of application classes. An empty scope applies everywhere.
    pub fn add_user_replacement_scoped(
        &mut self,
        source: impl Into<String>,
        replacement: impl Into<String>,
        scope: impl AsRef<str>,
    ) -> io::Result<()> {
        let source = source.into();
        let replacement = replacement.into();
        let scope_text = scope.as_ref();
        validate_rule(&source, &replacement, scope_text)?;
        self.user_replacements.insert(
            source.to_lowercase(),
            ReplacementRule {
                replacement,
                scope: AppScope::parse(scope_text),
            },
        );
        self.save_user_replacements()
    }

    pub fn remove_user_replacement(&mut self, source: &str) -> io::Result<bool> {
        let removed = self
            .user_replacements
            .remove(&source.to_lowercase())
            .is_some();
        if removed {
            self.save_user_replacements()?;
        }
        Ok(removed)
    }

    pub fn list_user_replacements(&self) -> Vec<String> {
        let mut pairs: Vec<String> = self
            .user_replacements
            .iter()
            .map(|(source, rule)| format!("{source}\t{}", rule.replacement))
            .collect();
        pairs.sort();
        pairs
    }

    /// Machine-readable rows for the settings UI: source, replacement, scope.
    pub fn list_user_replacement_rules(&self) -> Vec<String> {
        let mut rules: Vec<String> = self
            .user_replacements
            .iter()
            .map(|(source, rule)| {
                format!("{source}\t{}\t{}", rule.replacement, rule.scope.as_text())
            })
            .collect();
        rules.sort();
        rules
    }

    /// Check a replacement whose source is an ordinary word.
    pub fn fix_typo(&self, word: &str) -> Option<String> {
        self.fix_typo_in(word, None)
    }

    pub fn fix_typo_in(&self, word: &str, app_class: Option<&str>) -> Option<String> {
        if let Some(fixed) = self.fix_user_word_in(word, app_class) {
            return Some(fixed);
        }
        if let Some(fixed) = fix_two_capitals(word) {
            return Some(fixed);
        }
        if let Some(fixed) = fix_incidental_caps(word) {
            return Some(fixed);
        }
        None
    }

    /// Only user-created exact rules, without the incidental-capital fixes.
    pub fn fix_user_word_in(&self, word: &str, app_class: Option<&str>) -> Option<String> {
        self.user_replacements
            .get(&word.to_lowercase())
            .filter(|rule| rule.scope.matches(app_class))
            .map(|rule| preserve_case(word, &rule.replacement))
    }

    /// Check an exact source that ends in a character normally treated as the
    /// word delimiter. The caller consumes that delimiter when this matches.
    /// This makes a user rule such as `ещ\ → ещё` possible without teaching
    /// the word scanner that a backslash is a Russian letter.
    pub fn fix_delimited_typo(&self, word: &str, delimiter: char) -> Option<String> {
        self.fix_delimited_typo_in(word, delimiter, None)
    }

    pub fn fix_delimited_typo_in(
        &self,
        word: &str,
        delimiter: char,
        app_class: Option<&str>,
    ) -> Option<String> {
        let mut source = String::with_capacity(word.len() + delimiter.len_utf8());
        source.push_str(word);
        source.push(delimiter);
        self.user_replacements
            .get(&source.to_lowercase())
            .filter(|rule| rule.scope.matches(app_class))
            .map(|rule| preserve_case(word, &rule.replacement))
    }

    fn load_user_replacements(&mut self) {
        let Some(path) = &self.dictionary_file else {
            return;
        };
        if path.exists() {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };
        for line in contents.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.splitn(3, '\t');
            let Some(source) = fields.next() else {
                continue;
            };
            let Some(replacement) = fields.next() else {
                eprintln!("Skipping a malformed line in {}", path.display());
                continue;
            };
            let scope = fields.next().unwrap_or("");
            if validate_rule(source, replacement, scope).is_ok() {
                self.user_replacements.insert(
                    source.to_lowercase(),
                    ReplacementRule {
                        replacement: replacement.to_string(),
                        scope: AppScope::parse(scope),
                    },
                );
            }
        }
    }

    fn save_user_replacements(&self) -> io::Result<()> {
        let Some(path) = &self.dictionary_file else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut pairs: Vec<_> = self.user_replacements.iter().collect();
        pairs.sort_by(|left, right| left.0.cmp(right.0));
        let mut contents = String::from(
            "# PuLinTo — exact auto-replacements: source<TAB>replacement<TAB>applications\n",
        );
        for (source, rule) in pairs {
            contents.push_str(source);
            contents.push('\t');
            contents.push_str(&rule.replacement);
            contents.push('\t');
            contents.push_str(&rule.scope.as_text());
            contents.push('\n');
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(contents.as_bytes())
    }
}

fn validate_rule(source: &str, replacement: &str, scope: &str) -> io::Result<()> {
    if source.is_empty() || replacement.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the source and the replacement must not be empty",
        ));
    }
    if source.contains(['\t', '\n', '\r'])
        || replacement.contains(['\t', '\n', '\r'])
        || scope.contains(['\t', '\n', '\r'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an auto-replacement must not contain a TAB or a line break",
        ));
    }
    Ok(())
}

fn preserve_case(source_word: &str, replacement: &str) -> String {
    match case::observe(source_word) {
        Some(case::CaseMode::Upper) => case::apply(replacement, case::CaseMode::Upper),
        Some(case::CaseMode::Title) | Some(case::CaseMode::Sentence) => {
            case::apply(replacement, case::CaseMode::Sentence)
        }
        _ => replacement.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn dictionary_file(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("pulinto-autocorrect-{tag}-{}-{n}", std::process::id()))
            .join(USER_REPLACEMENTS_FILE)
    }

    #[test]
    fn test_autocorrect_typos() {
        let engine = AutoCorrectEngine::new();
        assert_eq!(engine.fix_typo("ПРивет").as_deref(), Some("Привет"));
        assert_eq!(engine.fix_typo("пРИВЕТ").as_deref(), Some("Привет"));
        assert_eq!(engine.fix_typo("Привет"), None);
    }

    #[test]
    fn test_abbreviations() {
        let mut engine = AutoCorrectEngine::new();
        engine.add_abbreviation("спс", "спасибо");

        assert_eq!(engine.check_abbreviation("спс").as_deref(), Some("спасибо"));
        assert_eq!(
            engine.check_abbreviation("др").as_deref(),
            Some("с днём рождения")
        );
        assert_eq!(engine.check_abbreviation("hello"), None);
    }

    #[test]
    fn exact_delimited_replacement_is_persistent_and_preserves_case() {
        let path = dictionary_file("delimited");
        let mut engine = AutoCorrectEngine::with_dictionary_file(path.clone());
        engine.add_user_replacement("ещ\\", "ещё").unwrap();

        let reloaded = AutoCorrectEngine::with_dictionary_file(path);
        assert_eq!(
            reloaded.fix_delimited_typo("ещ", '\\').as_deref(),
            Some("ещё")
        );
        assert_eq!(
            reloaded.fix_delimited_typo("Ещ", '\\').as_deref(),
            Some("Ещё")
        );
        assert_eq!(
            reloaded.fix_delimited_typo("ЕЩ", '\\').as_deref(),
            Some("ЕЩЁ")
        );
        assert_eq!(reloaded.fix_delimited_typo("вещ", '\\'), None);
        assert_eq!(reloaded.fix_delimited_typo("ещ", '/'), None);
    }

    #[test]
    fn invalid_dictionary_lines_do_not_become_rules() {
        let path = dictionary_file("invalid");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "broken line\n\tmissing-source\nvalid\tготово\n").unwrap();
        let engine = AutoCorrectEngine::with_dictionary_file(path);
        assert_eq!(engine.fix_typo("valid").as_deref(), Some("готово"));
        assert_eq!(engine.fix_typo("broken line"), None);
    }

    #[test]
    fn scoped_replacement_only_matches_the_selected_application() {
        let path = dictionary_file("scope");
        let mut engine = AutoCorrectEngine::with_dictionary_file(path.clone());
        engine
            .add_user_replacement_scoped("omw", "уже еду", "Navigator, jetbrains-*")
            .unwrap();

        let reloaded = AutoCorrectEngine::with_dictionary_file(path);
        assert_eq!(
            reloaded
                .fix_user_word_in("omw", Some("navigator"))
                .as_deref(),
            Some("уже еду")
        );
        assert!(reloaded
            .fix_user_word_in("omw", Some("jetbrains-idea"))
            .is_some());
        assert!(reloaded.fix_user_word_in("omw", Some("code")).is_none());
        assert!(reloaded.fix_user_word_in("omw", None).is_none());
        assert_eq!(
            reloaded.list_user_replacement_rules(),
            vec!["omw\tуже еду\tjetbrains-*,navigator"]
        );
    }

    #[test]
    fn old_two_column_dictionary_stays_global() {
        let path = dictionary_file("migration");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "hello\tпривет\n").unwrap();
        let engine = AutoCorrectEngine::with_dictionary_file(path);
        assert_eq!(
            engine.fix_user_word_in("hello", Some("any-app")).as_deref(),
            Some("привет")
        );
    }
}
