//! Wiring of live language data into the cascade's `LanguageProfile`.
//!
//! Shared by every command that has to make a real decision, so that the CLI
//! and the daemon cannot drift into judging words by different rules.

use std::path::PathBuf;

use pulinto_core::cascade::LanguageProfile;
use pulinto_core::layout::Layout;
use pulinto_core::ngram_bitmap::NgramBitmap;
use pulinto_core::pack::{self, Language, LanguagePack};
use crate::{LanguageData, LanguageDataLoader, WordList};

/// Our own language data, in preference order.
const LANGUAGE_DIRS: [&str; 4] = [
    "data/languages",
    "$XDG_DATA_HOME/pulinto/languages",
    "/usr/share/pulinto/languages",
    "/usr/local/share/pulinto/languages",
];
const DICT_DIRS: [&str; 4] = [
    "data/dict",
    "$XDG_DATA_HOME/pulinto/dict",
    "/usr/share/pulinto/dict",
    "/usr/local/share/pulinto/dict",
];

pub struct Profile {
    layout: &'static Layout,
    rules: Option<LanguageData>,
    ngrams: NgramBitmap,
    words: Option<WordList>,
}

impl LanguageProfile for Profile {
    fn layout(&self) -> &'static Layout {
        self.layout
    }

    fn matches_rule(&self, word: &str) -> bool {
        self.rules
            .as_ref()
            .is_some_and(|data| data.matches_rule(word))
    }

    fn in_dictionary(&self, word: &str) -> bool {
        // Stage 3 never judges one-character words; short_words owns those.
        word.chars().count() >= 2
            && self
                .words
                .as_ref()
                .is_some_and(|list| list.contains(&word.to_lowercase()))
    }

    fn starts_a_word(&self, prefix: &str) -> bool {
        !prefix.is_empty()
            && self
                .words
                .as_ref()
                .is_some_and(|list| list.has_prefix(prefix))
    }

    fn word_frequency(&self, word: &str) -> Option<u64> {
        if word.chars().count() < 2 {
            return None;
        }
        self.words.as_ref()?.frequency(&word.to_lowercase())
    }

    fn ngrams(&self) -> &NgramBitmap {
        &self.ngrams
    }
}

impl Profile {
    pub fn layout_id(&self) -> &'static str {
        self.layout.id
    }

    pub fn layout(&self) -> &'static Layout {
        self.layout
    }

    pub fn has_word_list(&self) -> bool {
        self.words.is_some()
    }
}

fn read_lines(path: &str) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Where a data directory may be. Relative ones (`data/dict`) are tried from
/// the working directory and then from every directory above the executable:
/// a daemon started at login runs from `$HOME`, yet the binary still sits in
/// `<checkout>/target/release`, two levels below the data. A leading
/// `$XDG_DATA_HOME/` stands for the per-user data directory (`~/.local/share`
/// when the variable is unset), where `install.sh` puts a user install.
fn places(base: &str) -> Vec<PathBuf> {
    if let Some(rest) = base.strip_prefix("$XDG_DATA_HOME/") {
        let home = std::env::var_os("XDG_DATA_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            });
        return home.map(|dir| dir.join(rest)).into_iter().collect();
    }
    let mut found = vec![PathBuf::from(base)];
    if PathBuf::from(base).is_relative() {
        if let Ok(exe) = std::env::current_exe() {
            found.extend(exe.ancestors().skip(1).map(|dir| dir.join(base)));
        }
    }
    found
}

/// Word lists sit next to the binary in a real install; in a source checkout
/// they are under `data/dict`.
fn word_list(lang: &str, pack_dir: Option<&PathBuf>) -> Option<WordList> {
    // A pack may carry its own word list beside `language.conf`.
    let beside = pack_dir.into_iter().cloned();
    for base in beside.chain(DICT_DIRS.iter().flat_map(|base| places(base))) {
        let path = base.join(format!("{lang}.fst"));
        if path.exists() {
            match WordList::load(&path) {
                Ok(list) => return Some(list),
                Err(err) => eprintln!("could not read {}: {err}", path.display()),
            }
        }
    }
    None
}

/// First directory that actually holds data for this layout.
fn language_dir(layout_id: &str) -> Option<PathBuf> {
    LANGUAGE_DIRS
        .iter()
        .flat_map(|base| places(base))
        .map(|base| base.join(layout_id))
        .find(|dir| dir.join("language.conf").exists() || dir.join("proto3").exists() || dir.join("proto").exists())
}

/// Find the language packs on disk and hand them to the core, once. A pack in
/// the first directory that has it wins; a pack with the id of a built-in one
/// replaces it, any other id adds a language. Call it before anything looks at a
/// layout (the daemon does so first thing): the tables are built once.
pub fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut found: Vec<LanguagePack> = Vec::new();
        for dir in LANGUAGE_DIRS.iter().flat_map(|base| places(base)) {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path().join("language.conf")).collect();
            paths.sort();
            for path in paths {
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                match LanguagePack::parse(&text) {
                    Ok(pack) if found.iter().any(|have| have.id == pack.id) => drop(pack),
                    Ok(pack) => found.push(pack),
                    Err(problems) => eprintln!("language pack {} rejected: {}", path.display(), problems.join("; ")),
                }
            }
        }
        // The built-in ids keep their places; the others follow in a fixed order.
        found.sort_by(|a, b| a.id.cmp(&b.id));
        if !pack::install(found) {
            eprintln!("language packs not loaded: the layout tables are already built");
        }
    });
}

fn build(language: &Language) -> Profile {
    let layout_id = language.id();
    let dir = language_dir(layout_id);
    let (bigrams, trigrams, rules) = match &dir {
        Some(dir) => (
            read_lines(&dir.join("proto").to_string_lossy()),
            read_lines(&dir.join("proto3").to_string_lossy()),
            LanguageDataLoader::new(dir.parent().unwrap_or(dir))
                .load_language_data(layout_id)
                .ok(),
        ),
        None => (Vec::new(), Vec::new(), None),
    };
    Profile {
        layout: Layout::by_id(layout_id).expect("a language pack has a standard layout"),
        rules,
        ngrams: NgramBitmap::build(&language.source.alphabet, bigrams, trigrams),
        words: word_list(&language.source.dictionary, dir.as_ref()),
    }
}

/// The layouts the switcher arbitrates between: every language pack found.
pub fn all() -> Vec<Profile> {
    init();
    pack::languages().iter().map(build).collect()
}

/// Leave out the languages the user has not set up as a layout. The two built-in
/// ones always stay (the daemon has always judged between them); a pack added
/// from disk takes part only when its layout is among those of the keyboard, or
/// its words would be offered to people who never type that language.
pub fn retain_enabled(profiles: &mut Vec<Profile>, enabled: &[String]) {
    const BUILT_IN: [&str; 2] = ["us", "ru"];
    profiles.retain(|profile| BUILT_IN.contains(&profile.layout_id()) || enabled.iter().any(|id| id == profile.layout_id()));
}

pub fn is_cyrillic(text: &str) -> bool {
    text.chars().any(|ch| ('\u{0400}'..='\u{04FF}').contains(&ch))
}

/// The id of the language whose alphabet holds most letters of the text; on a
/// tie the later pack, so a Cyrillic letter beside a Latin one reads as Russian.
/// A text with no letter of any alphabet is the first language's. Languages that
/// share a script cannot be told apart by letters: the daemon knows the layout
/// that is really active and does not ask.
pub fn language_of(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    let mut best: Option<(&'static str, usize)> = None;
    for language in pack::languages() {
        let score = lower.chars().filter(|ch| language.source.alphabet.contains(*ch)).count();
        if score > 0 && best.is_none_or(|(_, seen)| score >= seen) {
            best = Some((language.source.id.as_str(), score));
        }
    }
    best.map(|(id, _)| id).or_else(|| pack::languages().first().map(|language| language.source.id.as_str())).unwrap_or("us")
}

/// Index of the layout the text was most likely typed on.
pub fn assume_layout(profiles: &[Profile], text: &str, forced: Option<&str>) -> usize {
    let wanted = forced.map(str::to_string).unwrap_or_else(|| language_of(text).to_string());
    profiles
        .iter()
        .position(|profile| profile.layout.id == wanted)
        .unwrap_or(0)
}

/// Layouts of up to two preceding words, most recent first — what stage 2 needs.
pub fn context_langs(context: &str) -> Vec<&'static str> {
    context
        .split_whitespace()
        .rev()
        .take(2)
        .map(language_of)
        .collect()
}

/// Warn once when the word lists are missing, since without them stage 3 —
/// three quarters of all decisions — silently degrades to n-grams only.
pub fn warn_if_incomplete(profiles: &[Profile]) {
    if profiles.iter().any(|profile| !profile.has_word_list()) {
        eprintln!(
            "warning: dictionaries not found in {}; the dictionary stage is disabled",
            DICT_DIRS.join(", ")
        );
    }
}

pub fn refs(profiles: &[Profile]) -> Vec<&Profile> {
    profiles.iter().collect()
}
