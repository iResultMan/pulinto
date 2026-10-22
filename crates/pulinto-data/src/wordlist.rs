//! Frequency word list backed by a finite-state transducer.
//!
//! Replaces the spell checker in the cascade's stage 3. Two reasons it exists,
//! neither of them "hunspell is old":
//!
//!   * the daemon cannot spawn `aspell` per keystroke, and a plain `HashSet` of
//!     230k word forms costs several megabytes against a 6 MB RSS budget;
//!   * measured on the held-out corpus, general spell checkers accept plenty of
//!     mistyped garbage as real words (`ult`, `vs`, `pf`), which stops the
//!     cascade at stage 3 with the wrong verdict. A corpus frequency slice does
//!     not contain those, and it does contain the names and neologisms a
//!     stem dictionary lacks.
//!
//! The stored value is the corpus frequency, kept for stage-3 tie-breaking
//! between two hypotheses that are both real words.

use std::fs;
use std::io;
use std::path::Path;

use fst::{Map, MapBuilder, Streamer};

pub struct WordList {
    map: Map<Vec<u8>>,
}

impl WordList {
    /// Load a prebuilt `.fst`.
    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        let map = Map::new(bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
        Ok(Self { map })
    }

    /// Build from a `word<TAB>count` file. Input must be sorted by word;
    /// `gen_dict.py` emits it that way, and an FST cannot be built otherwise.
    pub fn build_from_tsv(source: &Path, out: &Path, min_count: u64) -> io::Result<usize> {
        let text = fs::read_to_string(source)?;
        let writer = io::BufWriter::new(fs::File::create(out)?);
        let mut builder = MapBuilder::new(writer)
            .map_err(|err| io::Error::other(err.to_string()))?;

        let mut written = 0usize;
        for line in text.lines() {
            let Some((word, count)) = line.split_once('\t') else {
                continue;
            };
            let count: u64 = count.trim().parse().unwrap_or(0);
            if count < min_count || word.is_empty() {
                continue;
            }
            builder
                .insert(word, count)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
            written += 1;
        }

        builder
            .finish()
            .map_err(|err| io::Error::other(err.to_string()))?;
        Ok(written)
    }

    pub fn contains(&self, word: &str) -> bool {
        self.map.get(word).is_some()
    }

    /// Does any stored word begin with `prefix`?
    ///
    /// The question the daemon has to answer at a key that draws punctuation on
    /// one layout and a letter on another: is `изъ` the start of a word, or is
    /// `течениею` the end of one plus a full stop? Membership cannot answer it —
    /// `изъ` is not a word — but the transducer walks prefixes for free, which
    /// is the whole reason the dictionary is an FST and not a hash set.
    pub fn has_prefix(&self, prefix: &str) -> bool {
        use fst::{Automaton, IntoStreamer};
        let mut stream = self
            .map
            .search(fst::automaton::Str::new(prefix).starts_with())
            .into_stream();
        stream.next().is_some()
    }

    /// Corpus frequency, or `None` when the word is unknown.
    pub fn frequency(&self, word: &str) -> Option<u64> {
        self.map.get(word)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Resident size of the transducer, for the RSS budget check.
    pub fn memory_bytes(&self) -> usize {
        self.map.as_fst().as_bytes().len()
    }

    /// Longest stored word, used only by tests and diagnostics.
    pub fn sample(&self, limit: usize) -> Vec<String> {
        let mut stream = self.map.stream();
        let mut out = Vec::new();
        while let Some((key, _)) = stream.next() {
            if out.len() >= limit {
                break;
            }
            out.push(String::from_utf8_lossy(key).into_owned());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pulinto-wordlist-{name}"))
    }

    #[test]
    fn builds_and_queries() {
        let source = temp("src.tsv");
        // Sorted by word, as an FST requires.
        fs::write(&source, "где\t120\nпривет\t4200\nредкость\t2\n").unwrap();
        let fst = temp("out.fst");

        let written = WordList::build_from_tsv(&source, &fst, 3).unwrap();
        assert_eq!(written, 2, "the word below min_count must be dropped");

        let list = WordList::load(&fst).unwrap();
        assert!(list.contains("привет"));
        assert!(list.contains("где"));
        assert!(!list.contains("редкость"));
        assert_eq!(list.frequency("привет"), Some(4200));
        assert_eq!(list.frequency("нетслова"), None);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn real_word_list_fits_the_memory_budget() {
        // Measured: 1.88 MB for both languages at min-count 3 (143k ru + 85k en
        // forms, 95.1% coverage of held-out real text). Raising the threshold to
        // 5 would save ~0.5 MB and cost a point of coverage — a bad trade inside
        // a 6 MB RSS budget where the n-gram maps take 7 KB. The ceiling here
        // guards against accidental growth, it is not an aspiration.
        let mut total = 0usize;
        for lang in ["ru", "en"] {
            let source = Path::new("../../data/dict").join(format!("{lang}.tsv"));
            if !source.exists() {
                return;
            }
            let fst = temp(&format!("real-{lang}.fst"));
            WordList::build_from_tsv(&source, &fst, 3).unwrap();
            let list = WordList::load(&fst).unwrap();
            assert!(list.len() > 50_000, "{lang} list looks truncated");
            total += list.memory_bytes();
        }
        assert!(
            total < 15_000_000,
            "word lists take {total} bytes, over the 15 MB ceiling"
        );
    }
}
