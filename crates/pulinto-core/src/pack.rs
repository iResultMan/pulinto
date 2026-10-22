//! Language packs: everything the cascade knows about one language that is data
//! and not an algorithm.
//!
//! A pack is one text file, `language.conf`, in a directory named after the XKB
//! layout id (`data/languages/ru/language.conf`), next to the dictionary and the
//! n-gram tables of that language. It holds the alphabet, the keys of the
//! keyboard layout (and its variants, such as Apple's), the words of one and two
//! letters, the abbreviations and the word endings the cascade needs. The format
//! is described for contributors in `docs/LANGUAGES.md`; [`LanguagePack::parse`]
//! is the reference and reports every problem it finds, with the line.
//!
//! Russian and English are built into the program from the same files, so a
//! program without data files still works; a pack found on disk replaces the
//! built-in one of the same id, and any other id adds a language.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::layout::{Layout, KEY_COUNT};

const BUILT_IN: [&str; 2] = [
    include_str!("../../../data/languages/us/language.conf"),
    include_str!("../../../data/languages/ru/language.conf"),
];

/// The keys of one arrangement: what 47 keys give unshifted and shifted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutTable {
    /// Empty for the standard arrangement, `mac` for Apple's, and so on.
    pub variant: String,
    pub name: String,
    pub lower: String,
    pub upper: String,
}

/// One word ending of a paradigm; `key` marks an ending that finishes on a letter
/// the other layout keeps on a punctuation key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ending {
    pub text: String,
    pub key: bool,
}

/// A language pack as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguagePack {
    /// The XKB layout id; also the name of the directory.
    pub id: String,
    pub name: String,
    /// The word list is `dict/<dictionary>.fst`; the id when not given.
    pub dictionary: String,
    pub alphabet: String,
    /// Letters looked up as another letter in the short-word tables (`ё` as `е`).
    pub fold: Vec<(char, char)>,
    pub short1: Vec<String>,
    pub short2: Vec<String>,
    pub keep: Vec<String>,
    pub abbreviations: Vec<String>,
    pub abbreviations_after_context: Vec<String>,
    pub dotted: Vec<String>,
    pub paradigms: Vec<Vec<Ending>>,
    pub layouts: Vec<LayoutTable>,
}

/// A layout while it is being read: variant, name, lower, upper.
type Table = (String, Option<String>, Option<String>, Option<String>);

fn words(value: &str) -> impl Iterator<Item = String> + '_ {
    value.split_whitespace().map(str::to_string)
}

impl LanguagePack {
    /// Read a pack. Every problem is collected, each with its line, so the author
    /// sees them all at once.
    pub fn parse(text: &str) -> Result<LanguagePack, Vec<String>> {
        let mut pack = LanguagePack::default();
        let mut problems = Vec::new();
        // Layout parts by variant: (name, lower, upper).
        let mut tables: Vec<Table> = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let number = index + 1;
            let Some((key, value)) = line.split_once('=') else {
                problems.push(format!("line {number}: expected `key = value`, got `{line}`"));
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "id" => pack.id = value.to_string(),
                "name" => pack.name = value.to_string(),
                "dictionary" => pack.dictionary = value.to_string(),
                "alphabet" => pack.alphabet = value.to_string(),
                "fold" => {
                    for pair in value.split_whitespace() {
                        let mut chars = pair.split('=');
                        match (chars.next(), chars.next(), chars.next()) {
                            (Some(from), Some(to), None) if from.chars().count() == 1 && to.chars().count() == 1 => {
                                pack.fold.push((from.chars().next().unwrap(), to.chars().next().unwrap()))
                            }
                            _ => problems.push(format!("line {number}: `fold` takes pairs like `ё=е`, got `{pair}`")),
                        }
                    }
                }
                "short1" => pack.short1.extend(words(value)),
                "short2" => pack.short2.extend(words(value)),
                "keep" => pack.keep.extend(words(value)),
                "abbreviations" => pack.abbreviations.extend(words(value)),
                "abbreviations.after_context" => pack.abbreviations_after_context.extend(words(value)),
                "dotted" => pack.dotted.extend(words(value)),
                "paradigm" => pack.paradigms.push(
                    words(value)
                        .map(|item| {
                            let (text, key) = match item.strip_suffix('*') {
                                Some(text) => (text, true),
                                None => (item.as_str(), false),
                            };
                            Ending { text: if text == "-" { String::new() } else { text.to_string() }, key }
                        })
                        .collect(),
                ),
                other => {
                    let parts: Vec<&str> = other.split('.').collect();
                    match parts.as_slice() {
                        ["layout", field @ ("name" | "lower" | "upper")] | ["layout", _, field @ ("name" | "lower" | "upper")] => {
                            let variant = if parts.len() == 3 { parts[1] } else { "" };
                            let at = match tables.iter().position(|table| table.0 == variant) {
                                Some(at) => at,
                                None => {
                                    tables.push((variant.to_string(), None, None, None));
                                    tables.len() - 1
                                }
                            };
                            let slot = match *field {
                                "name" => &mut tables[at].1,
                                "lower" => &mut tables[at].2,
                                _ => &mut tables[at].3,
                            };
                            if slot.is_some() {
                                problems.push(format!("line {number}: `{other}` is given twice"));
                            }
                            *slot = Some(value.to_string());
                        }
                        _ => problems.push(format!("line {number}: unknown key `{other}`")),
                    }
                }
            }
        }
        for (variant, name, lower, upper) in tables {
            let label = if variant.is_empty() { "layout".to_string() } else { format!("layout.{variant}") };
            match (lower, upper) {
                (Some(lower), Some(upper)) => pack.layouts.push(LayoutTable {
                    name: name.unwrap_or_else(|| format!("{} ({variant})", pack.name)),
                    variant,
                    lower,
                    upper,
                }),
                _ => problems.push(format!("`{label}` needs both `{label}.lower` and `{label}.upper`")),
            }
        }
        if pack.dictionary.is_empty() {
            pack.dictionary = pack.id.clone();
        }
        problems.extend(pack.problems());
        if problems.is_empty() {
            Ok(pack)
        } else {
            Err(problems)
        }
    }

    /// What is wrong with the contents, in words an author can act on.
    pub fn problems(&self) -> Vec<String> {
        let mut found = Vec::new();
        let in_alphabet = |ch: char| self.alphabet.contains(ch) || self.fold.iter().any(|(from, _)| *from == ch);
        if self.id.is_empty() || !self.id.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
            found.push(format!("`id` must be the XKB layout id in lower case ASCII (`ru`, `de`, `ua`), got `{}`", self.id));
        }
        if self.name.is_empty() {
            found.push("`name` is missing".into());
        }
        if self.alphabet.is_empty() {
            found.push("`alphabet` is missing: the lower case letters of the language".into());
        }
        let mut seen = HashSet::new();
        for ch in self.alphabet.chars() {
            if !ch.is_alphabetic() || ch.is_uppercase() {
                found.push(format!("`alphabet` holds `{ch}`, which is not a lower case letter"));
            } else if !seen.insert(ch) {
                found.push(format!("`alphabet` holds `{ch}` twice"));
            }
        }
        match self.layouts.iter().find(|table| table.variant.is_empty()) {
            None => found.push("the standard layout is missing: `layout.lower` and `layout.upper`".into()),
            Some(standard) => {
                for ch in self.alphabet.chars() {
                    if !standard.lower.contains(ch) {
                        found.push(format!("the letter `{ch}` of the alphabet is on no key of `layout.lower`"));
                    }
                }
            }
        }
        for table in &self.layouts {
            let label = if table.variant.is_empty() { "layout".to_string() } else { format!("layout.{}", table.variant) };
            for (what, text) in [("lower", &table.lower), ("upper", &table.upper)] {
                let count = text.chars().count();
                if count != KEY_COUNT {
                    found.push(format!("`{label}.{what}` has {count} characters, the 47 keys of the main block need exactly {KEY_COUNT}"));
                }
            }
        }
        let list = |key: &str, items: &[String], found: &mut Vec<String>, check: &dyn Fn(&str) -> Option<String>| {
            let mut seen = HashSet::new();
            for item in items {
                if !seen.insert(item) {
                    found.push(format!("`{key}` holds `{item}` twice"));
                }
                if let Some(reason) = check(item) {
                    found.push(format!("`{key}`: `{item}` {reason}"));
                }
            }
        };
        let letters = |item: &str| -> Option<String> {
            item.chars().find(|ch| !in_alphabet(*ch)).map(|ch| format!("has `{ch}`, which is not in the alphabet"))
        };
        list("short1", &self.short1, &mut found, &|item| {
            (item.chars().count() != 1).then(|| "is not one letter".to_string()).or_else(|| letters(item))
        });
        list("short2", &self.short2, &mut found, &|item| {
            (item.chars().count() != 2).then(|| "is not two letters".to_string()).or_else(|| letters(item))
        });
        list("keep", &self.keep, &mut found, &|item| {
            (item.chars().count() != 2)
                .then(|| "is not two letters".to_string())
                .or_else(|| self.short2.iter().any(|word| word == item).then(|| "is already in `short2`".to_string()))
                .or_else(|| letters(item))
        });
        list("dotted", &self.dotted, &mut found, &|item| {
            (!(1..=8).contains(&item.chars().count())).then(|| "is not 1 to 8 letters".to_string()).or_else(|| letters(item))
        });
        let abbreviation = |item: &str| -> Option<String> {
            (!(2..=6).contains(&item.chars().count()) || !item.chars().all(|ch| ch.is_alphanumeric() && !ch.is_uppercase()))
                .then(|| "is not 2 to 6 lower case letters or digits".to_string())
        };
        list("abbreviations", &self.abbreviations, &mut found, &abbreviation);
        list("abbreviations.after_context", &self.abbreviations_after_context, &mut found, &|item| {
            abbreviation(item).or_else(|| self.abbreviations.iter().any(|word| word == item).then(|| "is also in `abbreviations`".to_string()))
        });
        for (index, paradigm) in self.paradigms.iter().enumerate() {
            let number = index + 1;
            if !paradigm.iter().any(|ending| ending.key) {
                found.push(format!("`paradigm` number {number} has no ending marked with `*`, so it is never used"));
            }
            for ending in paradigm {
                if let Some(ch) = ending.text.chars().find(|ch| !in_alphabet(*ch)) {
                    found.push(format!("`paradigm` number {number}: `{}` has `{ch}`, which is not in the alphabet", ending.text));
                }
            }
        }
        found
    }
}

/// A pack with what the cascade looks up by word, ready for lookup.
#[derive(Debug)]
pub struct Language {
    pub source: LanguagePack,
    short1: HashSet<String>,
    short2: HashSet<String>,
    keep: HashSet<String>,
    abbreviations: HashSet<String>,
    after_context: HashSet<String>,
    dotted: HashSet<String>,
    /// (ending, the other endings of its paradigm), for every ending marked `*`.
    families: Vec<(String, Vec<String>)>,
}

impl Language {
    fn new(source: LanguagePack) -> Language {
        let set = |items: &[String]| items.iter().cloned().collect::<HashSet<_>>();
        let fold = |items: &[String]| {
            items.iter().map(|item| fold_with(&source.fold, item)).collect::<HashSet<_>>()
        };
        let families = source
            .paradigms
            .iter()
            .flat_map(|paradigm| {
                paradigm.iter().filter(|ending| ending.key).map(|ending| {
                    (ending.text.clone(), paradigm.iter().map(|other| other.text.clone()).collect())
                })
            })
            .collect();
        Language {
            short1: fold(&source.short1),
            short2: fold(&source.short2),
            keep: fold(&source.keep),
            abbreviations: set(&source.abbreviations),
            after_context: set(&source.abbreviations_after_context),
            dotted: set(&source.dotted),
            families,
            source,
        }
    }

    pub fn id(&self) -> &str {
        &self.source.id
    }

    /// Lower case, with the pack's letters folded (`ё` as `е`).
    fn normalise(&self, word: &str) -> String {
        fold_with(&self.source.fold, word)
    }

    /// A word of one or two letters that is a real word of the language.
    pub fn is_short_word(&self, word: &str) -> bool {
        let word = self.normalise(word);
        match word.chars().count() {
            1 => self.short1.contains(&word),
            2 => self.short2.contains(&word),
            _ => false,
        }
    }

    /// A two-letter form the context-free rule must leave alone.
    pub fn is_kept(&self, word: &str) -> bool {
        self.keep.contains(&self.normalise(word))
    }

    /// How a reading (`IP`, `ip`; one case only) stands in the abbreviation lists.
    pub fn abbreviation(&self, reading: &str) -> Abbreviation {
        let count = reading.chars().count();
        if !(2..=6).contains(&count) || !reading.chars().all(char::is_alphanumeric) {
            return Abbreviation::No;
        }
        let lower = reading.to_lowercase();
        // One case: `IP`, `ip`; not `Ip`, which is a name or a typo.
        if reading != lower && reading != reading.to_uppercase() {
            return Abbreviation::No;
        }
        if self.abbreviations.contains(&lower) {
            Abbreviation::Anywhere
        } else if self.after_context.contains(&lower) && reading == reading.to_uppercase() {
            // Capitals only: another language may write the same keys in lower
            // case as an abbreviation of its own.
            Abbreviation::AfterContext
        } else {
            Abbreviation::No
        }
    }

    /// An abbreviation the language writes with a full stop.
    pub fn is_dotted(&self, lower: &str) -> bool {
        self.dotted.contains(lower)
    }

    /// For each ending that finishes on a letter the other layout puts on a
    /// punctuation key, the endings of its paradigm.
    pub fn families(&self) -> &[(String, Vec<String>)] {
        &self.families
    }
}

fn fold_with(fold: &[(char, char)], word: &str) -> String {
    word.chars()
        .flat_map(|ch| ch.to_lowercase())
        .map(|ch| fold.iter().find(|(from, _)| *from == ch).map_or(ch, |(_, to)| *to))
        .collect()
}

/// How a reading relates to the abbreviation lists of the language it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abbreviation {
    /// Not listed.
    No,
    /// Listed: convertible anywhere.
    Anywhere,
    /// Listed: convertible only after text in that language.
    AfterContext,
}

struct Registry {
    languages: Vec<Language>,
    layouts: Vec<Layout>,
    by_id: HashMap<String, usize>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn build(extra: Vec<LanguagePack>) -> Registry {
    let mut sources: Vec<LanguagePack> = BUILT_IN
        .iter()
        .map(|text| LanguagePack::parse(text).unwrap_or_else(|problems| panic!("a built-in language pack is broken: {problems:?}")))
        .collect();
    for pack in extra {
        match sources.iter().position(|have| have.id == pack.id) {
            Some(at) => sources[at] = pack,
            None => sources.push(pack),
        }
    }
    let leak = |text: &str| -> &'static str { Box::leak(text.to_string().into_boxed_str()) };
    let mut layouts = Vec::new();
    for source in &sources {
        let id = leak(&source.id);
        // The standard arrangement first: it is the fallback of every variant.
        let mut tables: Vec<&LayoutTable> = source.layouts.iter().collect();
        tables.sort_by_key(|table| !table.variant.is_empty());
        for table in tables {
            layouts.push(Layout::from_pack(id, leak(&table.variant), leak(&table.name), leak(&table.lower), leak(&table.upper)));
        }
    }
    let languages: Vec<Language> = sources.into_iter().map(Language::new).collect();
    let by_id = languages.iter().enumerate().map(|(at, language)| (language.source.id.clone(), at)).collect();
    Registry { languages, layouts, by_id }
}

/// Add the packs found on disk to the two built in. Only possible before the first
/// lookup of anything: the tables are built once. Returns whether they were taken.
pub fn install(extra: Vec<LanguagePack>) -> bool {
    REGISTRY.set(build(extra)).is_ok()
}

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| build(Vec::new()))
}

/// Every pack, the built-in ones first.
pub fn languages() -> &'static [Language] {
    &registry().languages
}

pub fn language(id: &str) -> Option<&'static Language> {
    let registry = registry();
    registry.by_id.get(id).map(|at| &registry.languages[*at])
}

/// The alphabet of a language; empty when there is no such pack.
pub fn alphabet(id: &str) -> &'static str {
    language(id).map_or("", |language| language.source.alphabet.as_str())
}

pub(crate) fn layouts() -> &'static [Layout] {
    &registry().layouts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_packs_are_clean() {
        for text in BUILT_IN {
            assert_eq!(LanguagePack::parse(text).map(|pack| pack.problems()), Ok(Vec::new()));
        }
        assert_eq!(languages().iter().map(Language::id).take(2).collect::<Vec<_>>(), ["us", "ru"]);
    }

    #[test]
    fn short_words_fold_and_ignore_case() {
        let ru = language("ru").unwrap();
        assert!(ru.is_short_word("Я") && ru.is_short_word("её") && ru.is_short_word("на"));
        assert!(!ru.is_short_word("xyz") && !ru.is_short_word(""));
        assert!(ru.is_kept("ща") && !ru.is_kept("на"));
        let us = language("us").unwrap();
        assert!(us.is_short_word("a") && us.is_short_word("To") && !us.is_short_word("b"));
    }

    #[test]
    fn the_abbreviation_lists_say_what_they_claim() {
        let us = language("us").unwrap();
        assert_eq!(us.abbreviation("IP"), Abbreviation::Anywhere);
        assert_eq!(us.abbreviation("ip"), Abbreviation::Anywhere);
        assert_eq!(us.abbreviation("Ip"), Abbreviation::No, "one case only");
        assert_eq!(us.abbreviation("PC"), Abbreviation::AfterContext);
        assert_eq!(us.abbreviation("uk"), Abbreviation::No, "capitals only for the look-alikes");
        assert_eq!(us.abbreviation("CV"), Abbreviation::No);
        assert_eq!(us.abbreviation("ram"), Abbreviation::No, "an ordinary word: the dictionary decides");
        assert_eq!(us.abbreviation("NOTLISTED"), Abbreviation::No);
    }

    #[test]
    fn the_stem_families_come_from_the_paradigms() {
        let ru = language("ru").unwrap();
        let (_, siblings) = ru.families().iter().find(|(ending, _)| ending == "ую").unwrap();
        assert!(siblings.iter().any(|ending| ending == "ая"));
        assert!(ru.families().iter().any(|(ending, siblings)| ending == "ах" && siblings.iter().any(String::is_empty)));
        assert!(ru.families().iter().all(|(ending, _)| !ending.is_empty() && !ending.ends_with('*')));
    }

    #[test]
    fn a_broken_pack_names_every_fault_with_its_line() {
        let text = "id = xx\nname = Test\nalphabet = ab\nshort1 = a bb\nshort2 = ab ab zz\nfroggle = 1\nlayout.lower = abc\nlayout.upper = ABC\nabbreviations = IP x1\nparadigm = a b\nnonsense\n";
        let problems = LanguagePack::parse(text).unwrap_err().join("\n");
        for expected in [
            "line 6: unknown key `froggle`",
            "line 11: expected `key = value`",
            "`short1`: `bb` is not one letter",
            "`short2` holds `ab` twice",
            "`short2`: `zz` has `z`, which is not in the alphabet",
            "`layout.lower` has 3 characters",
            "`abbreviations`: `IP` is not 2 to 6 lower case letters or digits",
            "`paradigm` number 1 has no ending marked with `*`",
        ] {
            assert!(problems.contains(expected), "missing `{expected}` in:\n{problems}");
        }
    }

    #[test]
    fn a_pack_without_the_standard_layout_or_with_a_letter_off_the_keyboard_is_refused() {
        let base = |layout: &str| {
            format!("id = xx\nname = T\nalphabet = abz\n{layout}\n")
        };
        let lower: String = ('a'..='z').chain('a'..='u').take(KEY_COUNT).collect();
        let upper = lower.to_uppercase();
        let missing = LanguagePack::parse(&base("")).unwrap_err().join("\n");
        assert!(missing.contains("the standard layout is missing"), "{missing}");
        let off = LanguagePack::parse(&base(&format!("layout.lower = {}\nlayout.upper = {upper}", lower.replace('z', "y")))).unwrap_err().join("\n");
        assert!(off.contains("the letter `z` of the alphabet is on no key"), "{off}");
        assert!(LanguagePack::parse(&base(&format!("layout.lower = {lower}\nlayout.upper = {upper}"))).is_ok());
    }
}
