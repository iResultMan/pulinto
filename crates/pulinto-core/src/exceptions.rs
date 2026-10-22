//! Exception list manager for words and application classes.
//!
//! Two word lists are persisted here, not just held in memory:
//!
//! - `never`  — words the cascade must never convert, whatever it decides;
//! - `always` — words that convert on sight, unless a stop rule fires first.
//!
//! Losing either list on restart is the same defect class that once cost the
//! user real text: settings that do not survive a restart are not settings,
//! and learning that is forgotten is not learning. Both lists grow by two
//! routes — an explicit call (`add_word_exception` / `add_always_word`) and
//! automatic promotion once the user repeats the same correction often enough
//! in a session (`record_revert` / `record_manual_conversion`).
//!
//! One guard is categorical and must never be relaxed: single letters and
//! frequent short words in exceptions poison the language model. So words
//! under [`MIN_WORD_LEN`] characters are refused everywhere in this file,
//! including the learning counters that would otherwise collect them —
//! there is no path around the check, by design.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::scope::AppScope;
use sha2::{Digest, Sha256};

/// Shortest word length user lists may hold. See the module docs —
/// this is not a tunable, it is a correctness rule.
pub const MIN_WORD_LEN: usize = 3;

const NEVER_FILE: &str = "never.txt";
const ALWAYS_FILE: &str = "always.txt";
const FORGOTTEN_FILE: &str = "forgotten.sha256";
const FORGET_KEY_FILE: &str = "learning.key";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleOrigin {
    Manual,
    Learned,
    Legacy,
}

impl RuleOrigin {
    fn parse(value: &str) -> Self {
        match value {
            "manual" => Self::Manual,
            "learned" => Self::Learned,
            _ => Self::Legacy,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Learned => "learned",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone)]
struct WordRule {
    scope: AppScope,
    origin: RuleOrigin,
}

/// Which persisted list an operation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum List {
    Never,
    Always,
}

impl List {
    fn file_name(self) -> &'static str {
        match self {
            List::Never => NEVER_FILE,
            List::Always => ALWAYS_FILE,
        }
    }

    fn header(self) -> &'static str {
        match self {
            List::Never => "# PuLinTo — words that are never converted\n",
            List::Always => "# PuLinTo — words that are always converted\n",
        }
    }
}

/// Why a word was refused, so callers (D-Bus in particular) can explain
/// themselves instead of returning a bare `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// Shorter than [`MIN_WORD_LEN`] — see the module docs.
    TooShort,
    /// Contains a line break, a tab or another control character: one entry
    /// would become two lines, or a shifted field, in the list file.
    Malformed,
}

impl RejectReason {
    pub fn message(self) -> &'static str {
        match self {
            RejectReason::Malformed => "a word must not contain line breaks or tabs",
            RejectReason::TooShort => {
                "a word shorter than 3 characters cannot be added to the user dictionary \
                 (short words in the exceptions wreck the language model)"
            }
        }
    }
}

/// What came of a learning event (a revert or a manual conversion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnOutcome {
    /// Below [`MIN_WORD_LEN`]: the counter is not even kept.
    TooShort,
    /// Counted, but not yet at threshold. Carries the count so far.
    Counted(u32),
    /// This event pushed the word over the threshold and it graduated into
    /// the corresponding list.
    Graduated,
    /// Already in the target list; nothing left to learn.
    AlreadyListed,
    /// The user explicitly deleted and forgot this word. No counter is kept.
    Forgotten,
}

/// Threshold for automatic learning, kept as its own type so it reads as a
/// setting rather than a magic number buried in `ExceptionManager`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LearnSettings {
    /// How many times the same word must be reverted (or manually converted)
    /// in a session before it graduates on its own. "Several times per
    /// session" is taken as a starting point —
    /// there is no corpus for "how many corrections feel like a pattern to a
    /// human", only a judgement call, so it is a setting and not a constant.
    pub threshold: u32,
}

impl Default for LearnSettings {
    fn default() -> Self {
        Self { threshold: 3 }
    }
}

impl LearnSettings {
    /// Zero would graduate a word on its very first occurrence, which is not
    /// learning from repetition — it is a different feature with a confusing
    /// name. Floor it at one.
    pub fn set_threshold(&mut self, threshold: u32) {
        self.threshold = threshold.max(1);
    }
}

#[derive(Debug, Clone)]
pub struct ExceptionManager {
    never: HashMap<String, WordRule>,
    always: HashMap<String, WordRule>,
    /// Keyed fingerprints only: a forgotten password is never written here in
    /// plaintext, but the learner can still recognise and ignore it.
    forgotten: HashSet<String>,
    forget_key: [u8; 32],
    excluded_apps: HashSet<String>,
    /// Per-word revert counts feeding `never`. Session-scoped: cleared by
    /// `reset_learning`, and never persisted itself — only the lists it
    /// eventually feeds are.
    revert_counts: HashMap<String, u32>,
    /// Per-word manual-conversion counts feeding `always`, the mirror image
    /// of `revert_counts`.
    manual_counts: HashMap<String, u32>,
    pub learn: LearnSettings,
    /// Directory the two lists live in. `None` means "do not touch disk" —
    /// used only where a test needs a manager with no I/O at all; production
    /// code always goes through `new()`, which sets this.
    data_dir: Option<PathBuf>,
}

impl Default for ExceptionManager {
    fn default() -> Self {
        let mut mgr = Self {
            never: HashMap::new(),
            always: HashMap::new(),
            forgotten: HashSet::new(),
            forget_key: [0; 32],
            excluded_apps: HashSet::new(),
            revert_counts: HashMap::new(),
            manual_counts: HashMap::new(),
            learn: LearnSettings::default(),
            data_dir: None,
        };
        // Default application exclusions (terminals, VMs, games, RDP).
        for app in [
            "alacritty",
            "kitty",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "virtualbox",
            "freerdp",
            "steam",
        ] {
            mgr.add_app_exception(app);
        }
        mgr
    }
}

impl ExceptionManager {
    /// Build a manager backed by the standard XDG data directory
    /// (`$XDG_DATA_HOME/pulinto` or `~/.local/share/pulinto`), loading
    /// whatever lists are already there.
    pub fn new() -> Self {
        Self::with_data_dir(Self::default_data_dir())
    }

    /// Build a manager backed by an explicit directory instead of the XDG
    /// default. Production code has no reason to call this — it exists so
    /// tests can point at a temp directory without mutating process-wide
    /// environment variables, which would race across parallel test threads.
    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        let mut mgr = Self {
            data_dir: Some(data_dir),
            ..Self::default()
        };
        mgr.load();
        mgr
    }

    fn default_data_dir() -> PathBuf {
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                    .join(".local")
                    .join("share")
            });
        base.join("pulinto")
    }

    // ── Persistence ──────────────────────────────────────────────────────

    /// Load both lists from disk, if a data directory is configured. Missing
    /// files are the normal first run and yield an empty list; a corrupted
    /// file must never stop the daemon from starting, so unreadable lines
    /// are skipped rather than propagated as an error.
    fn load(&mut self) {
        let Some(dir) = self.data_dir.clone() else {
            return;
        };
        self.forget_key = load_or_create_forget_key(&dir);
        self.forgotten = load_fingerprints(&dir.join(FORGOTTEN_FILE));
        for path in [
            dir.join(NEVER_FILE),
            dir.join(ALWAYS_FILE),
            dir.join(FORGOTTEN_FILE),
            dir.join(FORGET_KEY_FILE),
        ] {
            if path.exists() {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
            }
        }
        self.never = Self::load_list(&dir.join(NEVER_FILE));
        self.always = Self::load_list(&dir.join(ALWAYS_FILE));
        // A hand-edited or legacy data directory may contain the same word in
        // both lists. Prefer the safe "never" rule so the UI and engine expose
        // one unambiguous decision.
        self.always.retain(|word, _| !self.never.contains_key(word));
    }

    fn load_list(path: &std::path::Path) -> HashMap<String, WordRule> {
        let Ok(text) = fs::read_to_string(path) else {
            return HashMap::new();
        };
        text.lines()
            .map(|line| line.trim_end_matches('\r'))
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| {
                let mut fields = line.splitn(3, '\t');
                let word = fields.next().unwrap_or_default();
                let scope = fields.next().unwrap_or_default();
                let origin = RuleOrigin::parse(fields.next().unwrap_or_default());
                let word = word.trim().to_lowercase();
                (word.chars().count() >= MIN_WORD_LEN)
                    .then(|| {
                        (
                            word,
                            WordRule {
                                scope: AppScope::parse(scope),
                                origin,
                            },
                        )
                    })
            })
            // A line shorter than the minimum (e.g. from manual editing, or from a
            // half-written file) is dropped rather than let back in — the
            // guarantee has to hold for what is on disk too, not only for
            // what goes through the API.
            .collect()
    }

    /// Write one list to disk. Failure is reported to stderr and otherwise
    /// swallowed: the lists are small enough that a missed write costs at
    /// most one learned word, and a daemon that dies over a full disk is a
    /// worse outcome than a stale exception list.
    fn save(&self, list: List) {
        let Some(dir) = &self.data_dir else {
            return;
        };
        if let Err(error) = fs::create_dir_all(dir) {
            eprintln!("Could not create {}: {error}", dir.display());
            return;
        }
        let words = match list {
            List::Never => &self.never,
            List::Always => &self.always,
        };
        let mut sorted: Vec<_> = words.iter().collect();
        sorted.sort_by(|left, right| left.0.cmp(right.0));
        let mut contents = String::from(list.header());
        for (word, rule) in sorted {
            contents.push_str(word);
            contents.push('\t');
            contents.push_str(&rule.scope.as_text());
            contents.push('\t');
            contents.push_str(rule.origin.as_str());
            contents.push('\n');
        }
        let path = dir.join(list.file_name());
        if let Err(error) = write_private(&path, contents.as_bytes()) {
            eprintln!("Could not save {}: {error}", path.display());
        }
    }

    fn save_forgotten(&self) {
        let Some(dir) = &self.data_dir else {
            return;
        };
        if let Err(error) = fs::create_dir_all(dir) {
            eprintln!("Could not create {}: {error}", dir.display());
            return;
        }
        let mut fingerprints: Vec<&String> = self.forgotten.iter().collect();
        fingerprints.sort();
        let mut contents = String::from(
            "# PuLinTo — keyed SHA-256 words excluded from automatic learning\n",
        );
        for fingerprint in fingerprints {
            contents.push_str(fingerprint);
            contents.push('\n');
        }
        let path = dir.join(FORGOTTEN_FILE);
        if let Err(error) = write_private(&path, contents.as_bytes()) {
            eprintln!("Could not save {}: {error}", path.display());
        }
    }

    // ── Word lists: never / always ───────────────────────────────────────

    fn normalise(word: &str) -> Result<String, RejectReason> {
        let lower = word.trim().to_lowercase();
        if lower.chars().any(char::is_control) {
            return Err(RejectReason::Malformed);
        }
        if lower.chars().count() < MIN_WORD_LEN {
            return Err(RejectReason::TooShort);
        }
        Ok(lower)
    }

    pub fn add_word_exception(&mut self, word: impl Into<String>) -> Result<(), RejectReason> {
        self.add_word_exception_scoped(word, "")
    }

    pub fn add_word_exception_scoped(
        &mut self,
        word: impl Into<String>,
        scope: impl AsRef<str>,
    ) -> Result<(), RejectReason> {
        let word = Self::normalise(&word.into())?;
        self.allow_learning_again(&word);
        self.always.remove(&word);
        self.never.insert(
            word,
            WordRule {
                scope: AppScope::parse(scope.as_ref()),
                origin: RuleOrigin::Manual,
            },
        );
        self.save(List::Never);
        self.save(List::Always);
        Ok(())
    }

    pub fn remove_word_exception(&mut self, word: &str) -> bool {
        let removed = self.never.remove(&word.to_lowercase()).is_some();
        if removed {
            self.save(List::Never);
        }
        removed
    }

    pub fn is_word_excluded(&self, word: &str) -> bool {
        self.is_word_excluded_in(word, None)
    }

    pub fn is_word_excluded_in(&self, word: &str, app_class: Option<&str>) -> bool {
        let lower = word.to_lowercase();
        self.never
            .get(&lower)
            .is_some_and(|rule| rule.scope.matches(app_class))
            || lower.starts_with("http://")
            || lower.starts_with("https://")
        // Domains (`yandex.ru`, `vk.com`) used to be excluded here as well, for
        // the whole token and before anything else; that also froze `cfqn.ru`
        // typed on the wrong layout. They are a stop rule now
        // (`stop_rules::looks_like_domain`), which the cascade applies to a
        // token it judges whole — a token whose left part is a word of its
        // own is split at the stop first, and `сайт.ru` comes out right.
    }

    pub fn add_always_word(&mut self, word: impl Into<String>) -> Result<(), RejectReason> {
        self.add_always_word_scoped(word, "")
    }

    pub fn add_always_word_scoped(
        &mut self,
        word: impl Into<String>,
        scope: impl AsRef<str>,
    ) -> Result<(), RejectReason> {
        let word = Self::normalise(&word.into())?;
        self.allow_learning_again(&word);
        self.never.remove(&word);
        self.always.insert(
            word,
            WordRule {
                scope: AppScope::parse(scope.as_ref()),
                origin: RuleOrigin::Manual,
            },
        );
        self.save(List::Always);
        self.save(List::Never);
        Ok(())
    }

    pub fn remove_always_word(&mut self, word: &str) -> bool {
        let removed = self.always.remove(&word.to_lowercase()).is_some();
        if removed {
            self.save(List::Always);
        }
        removed
    }

    /// Whether the cascade's verdict should be overridden to convert. Callers
    /// must still run stop rules first — this list is a standing preference,
    /// not a bypass of the password/URL/abbreviation guards.
    pub fn is_always_word(&self, word: &str) -> bool {
        self.is_always_word_in(word, None)
    }

    pub fn is_always_word_in(&self, word: &str, app_class: Option<&str>) -> bool {
        self.always
            .get(&word.to_lowercase())
            .is_some_and(|rule| rule.scope.matches(app_class))
    }

    pub fn list_words(&self) -> Vec<String> {
        let mut words: Vec<String> = self.never.keys().cloned().collect();
        words.sort();
        words
    }

    pub fn list_always_words(&self) -> Vec<String> {
        let mut words: Vec<String> = self.always.keys().cloned().collect();
        words.sort();
        words
    }

    pub fn list_exclusion_rules(&self) -> Vec<String> {
        Self::list_rule_rows(&self.never)
    }

    pub fn list_inclusion_rules(&self) -> Vec<String> {
        Self::list_rule_rows(&self.always)
    }

    fn list_rule_rows(rules: &HashMap<String, WordRule>) -> Vec<String> {
        let mut rows: Vec<String> = rules
            .iter()
            .map(|(word, rule)| {
                format!(
                    "{word}\t{}\t{}",
                    rule.scope.as_text(),
                    rule.origin.as_str()
                )
            })
            .collect();
        rows.sort();
        rows
    }

    // ── Automatic learning ───────────────────────────────────────────────

    /// The user reverted an automatic conversion of `word` by hand. Counts
    /// towards `never`; graduates once `learn.threshold` reverts of the same
    /// word have been seen.
    pub fn record_revert(&mut self, word: &str) -> LearnOutcome {
        self.record(word, List::Never)
    }

    /// The user manually converted `word` even though the cascade left it
    /// alone. Counts towards `always`, mirroring `record_revert`.
    pub fn record_manual_conversion(&mut self, word: &str) -> LearnOutcome {
        self.record(word, List::Always)
    }

    fn record(&mut self, word: &str, list: List) -> LearnOutcome {
        let lower = word.to_lowercase();
        // The same guard: short words never enter the counters, so there is no way
        // for automatic learning to do what manual entry is forbidden from
        // doing.
        if lower.chars().count() < MIN_WORD_LEN {
            return LearnOutcome::TooShort;
        }
        if self.is_forgotten_normalised(&lower) {
            return LearnOutcome::Forgotten;
        }

        let already_listed = match list {
            List::Never => self.never.contains_key(&lower),
            List::Always => self.always.contains_key(&lower),
        };
        if already_listed {
            return LearnOutcome::AlreadyListed;
        }

        let counts = match list {
            List::Never => &mut self.revert_counts,
            List::Always => &mut self.manual_counts,
        };
        let count = counts.entry(lower.clone()).or_insert(0);
        *count += 1;
        let count = *count;

        if count >= self.learn.threshold {
            counts.remove(&lower);
            match list {
                List::Never => {
                    self.always.remove(&lower);
                    self.never.insert(
                        lower,
                        WordRule {
                            scope: AppScope::global(),
                            origin: RuleOrigin::Learned,
                        },
                    );
                }
                List::Always => {
                    self.never.remove(&lower);
                    self.always.insert(
                        lower,
                        WordRule {
                            scope: AppScope::global(),
                            origin: RuleOrigin::Learned,
                        },
                    );
                }
            }
            self.save(list);
            self.save(match list {
                List::Never => List::Always,
                List::Always => List::Never,
            });
            LearnOutcome::Graduated
        } else {
            LearnOutcome::Counted(count)
        }
    }

    /// Drop all in-progress counters without touching the persisted lists —
    /// exposed to D-Bus so a user who mis-corrected a few times in a row can
    /// start the count over instead of watching a word graduate by mistake.
    pub fn reset_learning(&mut self) {
        self.revert_counts.clear();
        self.manual_counts.clear();
    }

    /// Remove a word from both learned lists and prevent future automatic
    /// learning. Only a keyed fingerprint survives on disk.
    pub fn forget_word(&mut self, word: &str) -> bool {
        let lower = word.trim().to_lowercase();
        if lower.chars().count() < MIN_WORD_LEN {
            return false;
        }
        let removed = self.never.remove(&lower).is_some() | self.always.remove(&lower).is_some();
        self.revert_counts.remove(&lower);
        self.manual_counts.remove(&lower);
        let fingerprint = self.fingerprint(&lower);
        let inserted = self.forgotten.insert(fingerprint);
        if removed {
            self.save(List::Never);
            self.save(List::Always);
        }
        if inserted {
            self.save_forgotten();
        }
        removed || inserted
    }

    pub fn forgotten_word_count(&self) -> usize {
        self.forgotten.len()
    }

    pub fn clear_forgotten_words(&mut self) -> bool {
        if self.forgotten.is_empty() {
            return false;
        }
        self.forgotten.clear();
        self.save_forgotten();
        true
    }

    fn allow_learning_again(&mut self, word: &str) {
        let fingerprint = self.fingerprint(word);
        if self.forgotten.remove(&fingerprint) {
            self.save_forgotten();
        }
    }

    fn is_forgotten_normalised(&self, word: &str) -> bool {
        self.forgotten.contains(&self.fingerprint(word))
    }

    fn fingerprint(&self, word: &str) -> String {
        let mut digest = Sha256::new();
        digest.update(b"PuLinTo forgotten word v1\0");
        digest.update(self.forget_key);
        digest.update(word.as_bytes());
        hex(&digest.finalize())
    }

    // ── Application exclusions ───────────────────────────────────────────

    pub fn add_app_exception(&mut self, app_class: impl Into<String>) {
        self.excluded_apps.insert(app_class.into().to_lowercase());
    }

    pub fn remove_app_exception(&mut self, app_class: &str) -> bool {
        self.excluded_apps.remove(&app_class.to_lowercase())
    }

    pub fn is_app_excluded(&self, app_class: &str) -> bool {
        self.excluded_apps.contains(&app_class.to_lowercase())
    }

    pub fn list_apps(&self) -> Vec<String> {
        let mut apps: Vec<String> = self.excluded_apps.iter().cloned().collect();
        apps.sort();
        apps
    }
}

fn load_fingerprints(path: &std::path::Path) -> HashSet<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| line.len() == 64 && line.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_lowercase)
        .collect()
}

fn load_or_create_forget_key(dir: &std::path::Path) -> [u8; 32] {
    let path = dir.join(FORGET_KEY_FILE);
    if let Ok(bytes) = fs::read(&path) {
        if let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice()) {
            return key;
        }
    }

    let mut key = [0u8; 32];
    let random = fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut key))
        .is_ok();
    if !random {
        // Last-resort uniqueness for a severely restricted Linux sandbox.
        // This is not intended to defend against a process running as the
        // same user; its purpose is to keep forgotten text out of the file.
        let mut digest = Sha256::new();
        digest.update(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
                .to_le_bytes(),
        );
        digest.update(std::process::id().to_le_bytes());
        digest.update(dir.to_string_lossy().as_bytes());
        key.copy_from_slice(&digest.finalize());
    }

    if fs::create_dir_all(dir).is_ok() {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(mut file) => {
                let _ = file.write_all(&key);
                let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if let Ok(bytes) = fs::read(&path) {
                    if let Ok(existing) = <[u8; 32]>::try_from(bytes.as_slice()) {
                        return existing;
                    }
                }
            }
            Err(_) => {}
        }
    }
    key
}

fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(contents)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Each test gets its own throwaway directory under the OS temp dir —
    /// no shared env var, so tests can run in parallel without racing.
    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pulinto-exceptions-test-{tag}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_word_exceptions() {
        let mut mgr = ExceptionManager::default();
        mgr.add_word_exception("code").unwrap();

        assert!(mgr.is_word_excluded("code"));
        assert!(mgr.is_word_excluded("https://github.com"));
        assert!(!mgr.is_word_excluded("привет"));
    }

    #[test]
    fn test_app_exceptions() {
        let mgr = ExceptionManager::default();
        assert!(mgr.is_app_excluded("alacritty"));
        assert!(mgr.is_app_excluded("Konsole"));
        assert!(!mgr.is_app_excluded("firefox"));
    }

    #[test]
    fn short_words_are_rejected_with_a_reason() {
        let mut mgr = ExceptionManager::default();
        assert_eq!(
            mgr.add_word_exception("б"),
            Err(RejectReason::TooShort)
        );
        assert_eq!(
            mgr.add_word_exception("на"),
            Err(RejectReason::TooShort)
        );
        assert!(!mgr.is_word_excluded("б"));

        assert_eq!(
            mgr.add_always_word("ох"),
            Err(RejectReason::TooShort)
        );
        assert!(!mgr.is_always_word("ох"));
    }

    #[test]
    fn save_and_load_round_trip_through_disk() {
        let dir = temp_dir("roundtrip");

        let mut mgr = ExceptionManager::with_data_dir(dir.clone());
        mgr.add_word_exception("код").unwrap();
        mgr.add_always_word("превед").unwrap();
        drop(mgr);

        let reloaded = ExceptionManager::with_data_dir(dir.clone());
        assert!(reloaded.is_word_excluded("код"));
        assert!(reloaded.is_always_word("превед"));

        assert!(dir.join(NEVER_FILE).exists());
        assert!(dir.join(ALWAYS_FILE).exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_file_does_not_crash_load() {
        let dir = temp_dir("corrupt");
        // Garbage: binary-ish bytes, blank lines, comments, and a short word
        // that should never have made it into the file in the first place.
        let mut contents = b"# comment\n\n\xff\xfe not-utf8-safe-ish\n".to_vec();
        contents.extend_from_slice("б\nok\n".as_bytes());
        std::fs::write(dir.join(NEVER_FILE), contents).unwrap();

        let mgr = ExceptionManager::with_data_dir(dir.clone());
        // The file wasn't valid UTF-8 as a whole, so `read_to_string` fails
        // and we fall back to empty — the important thing is that this does
        // not panic and the daemon would still start.
        assert!(!mgr.is_word_excluded("ok"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_but_valid_utf8_file_filters_bad_lines() {
        let dir = temp_dir("filter");
        std::fs::write(
            dir.join(NEVER_FILE),
            "# comment\n\nб\nна\nok\nvalid\n",
        )
        .unwrap();

        let mgr = ExceptionManager::with_data_dir(dir.clone());
        // "б" and "на" are below MIN_WORD_LEN and must not survive the load,
        // even though they made it into the file somehow (hand-edited, say).
        assert!(!mgr.is_word_excluded("б"));
        assert!(!mgr.is_word_excluded("на"));
        // "ok" is two characters and must be dropped for the same reason.
        assert!(!mgr.is_word_excluded("ok"));
        // "valid" is a well-formed three-plus-letter entry and must load.
        assert!(mgr.is_word_excluded("valid"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn revert_threshold_fires_exactly_at_the_configured_count() {
        let dir = temp_dir("threshold");
        let mut mgr = ExceptionManager::with_data_dir(dir.clone());
        mgr.learn.set_threshold(3);

        assert_eq!(mgr.record_revert("привет"), LearnOutcome::Counted(1));
        assert!(!mgr.is_word_excluded("привет"));
        assert_eq!(mgr.record_revert("привет"), LearnOutcome::Counted(2));
        assert!(!mgr.is_word_excluded("привет"));
        assert_eq!(mgr.record_revert("привет"), LearnOutcome::Graduated);
        assert!(mgr.is_word_excluded("привет"));

        // Reloading from disk must see the graduated word too.
        let reloaded = ExceptionManager::with_data_dir(dir.clone());
        assert!(reloaded.is_word_excluded("привет"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manual_conversion_threshold_feeds_always_list() {
        let dir = temp_dir("manual-threshold");
        let mut mgr = ExceptionManager::with_data_dir(dir.clone());
        mgr.learn.set_threshold(2);

        assert_eq!(
            mgr.record_manual_conversion("превед"),
            LearnOutcome::Counted(1)
        );
        assert!(!mgr.is_always_word("превед"));
        assert_eq!(
            mgr.record_manual_conversion("превед"),
            LearnOutcome::Graduated
        );
        assert!(mgr.is_always_word("превед"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn learning_never_counts_short_words() {
        let mut mgr = ExceptionManager::default();
        mgr.learn.set_threshold(1);
        assert_eq!(mgr.record_revert("б"), LearnOutcome::TooShort);
        assert_eq!(mgr.record_manual_conversion("на"), LearnOutcome::TooShort);
        assert!(!mgr.is_word_excluded("б"));
        assert!(!mgr.is_always_word("на"));
    }

    #[test]
    fn reset_learning_clears_counters_but_not_lists() {
        let mut mgr = ExceptionManager::default();
        mgr.learn.set_threshold(5);
        mgr.record_revert("привет");
        mgr.add_word_exception("готово").unwrap();

        mgr.reset_learning();

        // The counter is gone (four more reverts would be needed again)...
        assert_eq!(mgr.record_revert("привет"), LearnOutcome::Counted(1));
        // ...but an explicitly-added word is untouched by a counter reset.
        assert!(mgr.is_word_excluded("готово"));
    }

    #[test]
    fn always_and_never_are_mutually_exclusive() {
        let mut mgr = ExceptionManager::default();
        mgr.add_word_exception("слово").unwrap();
        assert!(mgr.is_word_excluded("слово"));
        assert!(!mgr.is_always_word("слово"));

        mgr.add_always_word("слово").unwrap();
        assert!(mgr.is_always_word("слово"));
        assert!(!mgr.is_word_excluded("слово"));
    }

    #[test]
    fn word_rules_can_be_scoped_to_applications() {
        let dir = temp_dir("scoped-rules");
        let mut mgr = ExceptionManager::with_data_dir(dir.clone());
        mgr.add_word_exception_scoped("cargo", "Navigator").unwrap();
        mgr.add_always_word_scoped("ghbdtn", "code,jetbrains-*")
            .unwrap();

        assert!(mgr.is_word_excluded_in("cargo", Some("navigator")));
        assert!(!mgr.is_word_excluded_in("cargo", Some("code")));
        assert!(!mgr.is_word_excluded_in("cargo", None));
        assert!(mgr.is_always_word_in("ghbdtn", Some("jetbrains-idea")));
        assert!(!mgr.is_always_word_in("ghbdtn", Some("navigator")));

        let reloaded = ExceptionManager::with_data_dir(dir.clone());
        assert_eq!(
            reloaded.list_exclusion_rules(),
            vec!["cargo\tnavigator\tmanual"]
        );
        assert_eq!(
            reloaded.list_inclusion_rules(),
            vec!["ghbdtn\tcode,jetbrains-*\tmanual"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_legacy_conflict_is_resolved_in_favour_of_never() {
        let dir = temp_dir("legacy-conflict");
        std::fs::write(dir.join(NEVER_FILE), "слово\n").unwrap();
        std::fs::write(dir.join(ALWAYS_FILE), "слово\n").unwrap();
        let mgr = ExceptionManager::with_data_dir(dir.clone());
        assert!(mgr.is_word_excluded("слово"));
        assert!(!mgr.is_always_word("слово"));
        assert!(mgr.list_inclusion_rules().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn learned_origin_survives_restart() {
        let dir = temp_dir("origin");
        let mut mgr = ExceptionManager::with_data_dir(dir.clone());
        mgr.learn.set_threshold(1);
        assert_eq!(mgr.record_manual_conversion("ghbdtn"), LearnOutcome::Graduated);
        let reloaded = ExceptionManager::with_data_dir(dir.clone());
        assert_eq!(
            reloaded.list_inclusion_rules(),
            vec!["ghbdtn\t\tlearned"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn forget_blocks_learning_without_persisting_plaintext() {
        let dir = temp_dir("forget");
        let secret = "correct-horse-battery-staple";
        let mut mgr = ExceptionManager::with_data_dir(dir.clone());
        mgr.learn.set_threshold(1);
        assert_eq!(mgr.record_manual_conversion(secret), LearnOutcome::Graduated);
        assert!(mgr.forget_word(secret));
        assert_eq!(mgr.record_manual_conversion(secret), LearnOutcome::Forgotten);
        assert_eq!(mgr.forgotten_word_count(), 1);

        let stored = std::fs::read_to_string(dir.join(FORGOTTEN_FILE)).unwrap();
        assert!(!stored.contains(secret));
        assert_eq!(
            std::fs::metadata(dir.join(FORGOTTEN_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let mut reloaded = ExceptionManager::with_data_dir(dir.clone());
        reloaded.learn.set_threshold(1);
        assert_eq!(
            reloaded.record_manual_conversion(secret),
            LearnOutcome::Forgotten
        );
        // Explicit user intent reverses "forget" without exposing the
        // fingerprint in the UI.
        reloaded.add_always_word(secret).unwrap();
        assert!(reloaded.is_always_word(secret));
        assert_eq!(reloaded.forgotten_word_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
