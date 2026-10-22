
//! Loader and parser for language datasets (`proto`, `proto3`, `dictionary`).
//!
//! The `dictionary` file does not contain plain words to compare by exact
//! string equality. It contains regular expressions, e.g.
//! `(?i)^а$`, `(?i)^http`, `^[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}$`.
//! Loading it as literal strings for exact-match comparison would mean stage 1
//! of the cascade never fires. `LanguageData` below
//! compiles the file into a `regex::RegexSet` and exposes `matches_rule` for
//! that stage.

pub mod profile;
pub mod wordlist;
pub use wordlist::WordList;

use std::fs::File;
use std::io::{self, BufRead, BufReader, Result};
use std::path::{Path, PathBuf};

use regex::RegexSet;

pub const DEFAULT_LANGUAGE_PATH: &str = "/usr/share/pulinto/languages";

/// Loader for language models from disk.
pub struct LanguageDataLoader {
    base_dir: PathBuf,
}

impl Default for LanguageDataLoader {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from(DEFAULT_LANGUAGE_PATH),
        }
    }
}

impl LanguageDataLoader {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Load language model for a given language code (e.g., "ru", "us").
    ///
    /// Compiles the `dictionary` file into the regex rules of cascade stage 1.
    pub fn load_language_data(&self, lang_id: &str) -> Result<LanguageData> {
        let dict_path = self.base_dir.join(lang_id).join("dictionary");
        LanguageData::from_dictionary_file(lang_id, &dict_path)
    }
}

/// Compiled per-language regex rule set from the `dictionary` file.
///
/// A word matching one of these rules is treated as belonging to this
/// language regardless of exact spelling: short frequent words (`(?i)^а$`),
/// URL/IP/MAC-shaped tokens meant to be left alone, etc. This is cascade
/// stage 1, the highest-priority stage after stop
/// rules.
#[derive(Debug)]
pub struct LanguageData {
    pub lang_id: String,
    rule_set: RegexSet,
}

impl LanguageData {
    /// Compile a `dictionary` file into regex rules.
    ///
    /// Empty lines and lines starting with `#` are skipped. Every other line
    /// must be a valid regex; a broken pattern is a hard load error naming
    /// the file and line number, not a silent skip — a mistyped rule quietly
    /// disabling stage 1 of the cascade is exactly the failure mode this
    /// loader replaces.
    fn from_dictionary_file(lang_id: &str, path: &Path) -> Result<Self> {
        let mut patterns: Vec<String> = Vec::new();

        if path.exists() {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            for (idx, line) in reader.lines().enumerate() {
                let line_no = idx + 1;
                let line = line?;
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }

                // Validate each pattern individually so a broken rule reports
                // its own line number instead of an opaque RegexSet-wide error.
                if let Err(err) = regex::Regex::new(trimmed) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "invalid regex in {}:{}: `{}` ({})",
                            path.display(),
                            line_no,
                            trimmed,
                            err
                        ),
                    ));
                }
                patterns.push(trimmed.to_string());
            }
        }

        let rule_set = RegexSet::new(&patterns).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to build rule set for {}: {}", path.display(), err),
            )
        })?;

        Ok(Self {
            lang_id: lang_id.to_string(),
            rule_set,
        })
    }

    /// Word matches one of this language's regex rules (cascade stage 1).
    ///
    /// Checks all compiled rules in a single pass via `RegexSet::is_match`,
    /// compiled once at load time rather than per call.
    pub fn matches_rule(&self, word: &str) -> bool {
        self.rule_set.is_match(word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pulinto-data-test-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn matches_rule_uses_compiled_regexes_not_literal_equality() {
        let base = unique_temp_dir("regex-basic");
        let lang_dir = base.join("xx");
        fs::create_dir_all(&lang_dir).unwrap();
        fs::write(lang_dir.join("dictionary"), "(?i)^а$\n(?i)^http\n").unwrap();

        let loader = LanguageDataLoader::new(&base);
        let data = loader.load_language_data("xx").unwrap();

        assert!(data.matches_rule("а"), "single-letter rule should match");
        assert!(
            data.matches_rule("HTTP://x"),
            "case-insensitive prefix rule should match"
        );
        assert!(
            !data.matches_rule("привет"),
            "word not covered by any rule should not match"
        );

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn broken_regex_is_a_load_error_with_line_number() {
        let base = unique_temp_dir("regex-broken");
        let lang_dir = base.join("xx");
        fs::create_dir_all(&lang_dir).unwrap();
        // Line 1 is valid, line 2 has an unbalanced group - broken regex.
        fs::write(lang_dir.join("dictionary"), "(?i)^ok$\n(?i)^bad(\n").unwrap();

        let loader = LanguageDataLoader::new(&base);
        let err = loader
            .load_language_data("xx")
            .expect_err("broken regex must fail to load");
        let msg = err.to_string();
        assert!(msg.contains(":2:"), "error should mention line 2, got: {}", msg);

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn empty_and_comment_lines_are_skipped() {
        let base = unique_temp_dir("regex-comments");
        let lang_dir = base.join("xx");
        fs::create_dir_all(&lang_dir).unwrap();
        fs::write(
            lang_dir.join("dictionary"),
            "# a comment\n\n(?i)^ok$\n   \n",
        )
        .unwrap();

        let loader = LanguageDataLoader::new(&base);
        let data = loader.load_language_data("xx").unwrap();
        assert!(data.matches_rule("ok"));

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_real_ru_dictionary_regex_rules() {
        let loader = LanguageDataLoader::default();
        if !Path::new(DEFAULT_LANGUAGE_PATH).join("ru").exists() {
            // System language data not installed; skip without failing.
            return;
        }
        let ru_data = loader.load_language_data("ru").unwrap();
        assert!(
            ru_data.matches_rule("я"),
            "real ru/dictionary has an `(?i)^я$` rule"
        );
        assert!(
            !ru_data.matches_rule("табуретка"),
            "an ordinary word should not match any stage-1 rule"
        );
    }
}
