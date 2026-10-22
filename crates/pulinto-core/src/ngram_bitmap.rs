//! Forbidden bi-/trigram bitmaps, radix-indexed by alphabet position.
//!
//! Rationale: the resident daemon has a ≤6 MB RSS
//! budget, and a `HashSet<String>` of n-grams costs megabytes once you count
//! allocator overhead per entry. A bitmap addressed by alphabet radix needs
//! exactly `N^2` bits for bigrams and `N^3` bits for trigrams — for Russian
//! (33 letters) that is ~4.6 KB total, for English (26 letters) ~2.3 KB.
//!
//! The cascade uses this as a binary veto:
//! a word is rejected for a language the moment it contains *any* forbidden
//! n-gram, regardless of how many. Incomplete prefixes are checked too
//! while the user is still typing, so the lookup must be allocation-
//! free — it runs on every keystroke.

use std::collections::HashMap;

/// Up to two alphabet positions a single character can stand for while
/// querying. Only ever holds more than one entry for `е`/`ё`, which corpora
/// use interchangeably (see module docs and `build`'s caller contract).
#[derive(Clone, Copy)]
struct Candidates {
    values: [u8; 2],
    len: u8,
}

impl Candidates {
    const EMPTY: Candidates = Candidates { values: [0, 0], len: 0 };

    fn push(&mut self, position: u8) {
        // Both slots are only ever filled by distinct alphabet positions
        // (the literal character and its е/ё counterpart), so no dedup needed.
        self.values[self.len as usize] = position;
        self.len += 1;
    }

    fn slice(&self) -> &[u8] {
        &self.values[..self.len as usize]
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

fn bit_at(bits: &[u8], idx: usize) -> bool {
    bits[idx / 8] & (1 << (idx % 8)) != 0
}

fn set_bit(bits: &mut [u8], idx: usize) {
    bits[idx / 8] |= 1 << (idx % 8);
}

/// Compact forbidden-n-gram membership test for one language's alphabet.
pub struct NgramBitmap {
    /// Character -> position in the alphabet, built once so lookups never
    /// rescan the alphabet string.
    positions: HashMap<char, u8>,
    alphabet_len: usize,
    /// Bit `i(c1) * N + i(c2)` set means that bigram is forbidden.
    bigram_bits: Box<[u8]>,
    /// Bit `(i(c1) * N + i(c2)) * N + i(c3)` set means that trigram is forbidden.
    trigram_bits: Box<[u8]>,
    /// True when nothing was actually inserted (empty input, or every
    /// n-gram was rejected for containing out-of-alphabet characters).
    is_empty: bool,
}

impl NgramBitmap {
    /// `alphabet` — the language's letters in lower case, e.g. "абвгд...я".
    /// N-grams containing characters outside the alphabet are ignored.
    pub fn build<I, J>(alphabet: &str, forbidden_bigrams: I, forbidden_trigrams: J) -> Self
    where
        I: IntoIterator<Item = String>,
        J: IntoIterator<Item = String>,
    {
        let letters: Vec<char> = alphabet.chars().collect();
        let n = letters.len();

        let mut positions = HashMap::with_capacity(n);
        for (i, &ch) in letters.iter().enumerate() {
            positions.entry(ch).or_insert(i as u8);
        }

        let bigram_bit_count = n * n;
        let trigram_bit_count = n * n * n;
        let mut bigram_bits = vec![0u8; bigram_bit_count.div_ceil(8)].into_boxed_slice();
        let mut trigram_bits = vec![0u8; trigram_bit_count.div_ceil(8)].into_boxed_slice();
        let mut any = false;

        for bigram in forbidden_bigrams {
            let chars: Vec<char> = bigram.chars().flat_map(|c| c.to_lowercase()).collect();
            let [c0, c1] = match chars[..] {
                [c0, c1] => [c0, c1],
                _ => continue,
            };
            let (Some(&a), Some(&b)) = (positions.get(&c0), positions.get(&c1)) else {
                continue;
            };
            set_bit(&mut bigram_bits, a as usize * n + b as usize);
            any = true;
        }

        for trigram in forbidden_trigrams {
            let chars: Vec<char> = trigram.chars().flat_map(|c| c.to_lowercase()).collect();
            let [c0, c1, c2] = match chars[..] {
                [c0, c1, c2] => [c0, c1, c2],
                _ => continue,
            };
            let (Some(&a), Some(&b), Some(&c)) =
                (positions.get(&c0), positions.get(&c1), positions.get(&c2))
            else {
                continue;
            };
            set_bit(&mut trigram_bits, (a as usize * n + b as usize) * n + c as usize);
            any = true;
        }

        Self {
            positions,
            alphabet_len: n,
            bigram_bits,
            trigram_bits,
            is_empty: !any,
        }
    }

    fn bigram_index(&self, a: u8, b: u8) -> usize {
        a as usize * self.alphabet_len + b as usize
    }

    fn trigram_index(&self, a: u8, b: u8, c: u8) -> usize {
        (a as usize * self.alphabet_len + b as usize) * self.alphabet_len + c as usize
    }

    /// Alphabet positions a query character can match. Besides the literal
    /// position this also returns the е/ё counterpart's position, since
    /// corpora normalize the two inconsistently (see module docs).
    fn query_candidates(&self, ch: char) -> Candidates {
        let mut out = Candidates::EMPTY;
        if let Some(&p) = self.positions.get(&ch) {
            out.push(p);
        }
        // `ё` folds to `е` but not the other way round. Treating them as fully
        // interchangeable meant a sequence banned only for `ё` also condemned
        // every word spelled with `е`, which wrongly converted real Russian
        // words that simply are not in the dictionary — хейтер, херня,
        // балдежные. The source n-gram data draws the distinction, so we keep it.
        if ch == 'ё' {
            if let Some(&p) = self.positions.get(&'е') {
                out.push(p);
            }
        }
        out
    }

    /// Scan `s`, reporting bigram and trigram hits separately. Shared by all
    /// four public predicates — a "word" and an in-progress "prefix" are
    /// both just character sequences to slide a window over. Characters
    /// outside the alphabet break the sequence: no n-gram is built across
    /// them. No allocation: state is two `Candidates` values (stack,
    /// `Copy`) carried across a plain `char` iteration. Stops early once
    /// both kinds have hit, since nothing further can change the answer.
    ///
    /// Bigram and trigram are kept separate (rather than a single
    /// "any forbidden n-gram" bool) because the cascade runs them as
    /// distinct stages: stage 4 (bigrams) decides the language outright
    /// when it comes up clean, and stage 5 (trigrams) only runs at all if
    /// stage 4 hit. Collapsing them would flag a bigram-clean,
    /// trigram-dirty word as suspicious when the staged check would not —
    /// stricter than intended, which is the wrong direction: a
    /// false positive costs more than a miss.
    fn scan_both(&self, s: &str) -> (bool, bool) {
        if self.is_empty {
            return (false, false);
        }
        let mut prev2: Option<Candidates> = None;
        let mut prev1: Option<Candidates> = None;
        let mut bigram_hit = false;
        let mut trigram_hit = false;
        for ch in s.chars() {
            if bigram_hit && trigram_hit {
                break;
            }
            let lower = ch.to_lowercase().next().unwrap_or(ch);
            let cur = self.query_candidates(lower);
            if cur.is_empty() {
                prev1 = None;
                prev2 = None;
                continue;
            }

            if !bigram_hit {
                if let Some(p1) = prev1 {
                    'bigram: for &a in p1.slice() {
                        for &b in cur.slice() {
                            if bit_at(&self.bigram_bits, self.bigram_index(a, b)) {
                                bigram_hit = true;
                                break 'bigram;
                            }
                        }
                    }
                }
            }
            if !trigram_hit {
                if let (Some(p2), Some(p1)) = (prev2, prev1) {
                    'trigram: for &a in p2.slice() {
                        for &b in p1.slice() {
                            for &c in cur.slice() {
                                if bit_at(&self.trigram_bits, self.trigram_index(a, b, c)) {
                                    trigram_hit = true;
                                    break 'trigram;
                                }
                            }
                        }
                    }
                }
            }

            prev2 = prev1;
            prev1 = Some(cur);
        }
        (bigram_hit, trigram_hit)
    }

    /// Whether `word` contains at least one forbidden bigram, ignoring
    /// trigrams. This is cascade stage 4: a hit here is what makes
    /// stage 5 relevant at all.
    pub fn has_forbidden_bigram(&self, word: &str) -> bool {
        self.scan_both(word).0
    }

    /// Whether `word` contains at least one forbidden trigram, ignoring
    /// bigrams. Cascade stage 5 — only meaningful to consult after
    /// stage 4 has already hit.
    pub fn has_forbidden_trigram(&self, word: &str) -> bool {
        self.scan_both(word).1
    }

    /// Whether `word` contains at least one forbidden bigram or trigram.
    /// Binary veto: the fact matters, not the count. Useful
    /// for the incremental prefix mode, where only "is this prefix
    /// already tainted" matters, not which stage would have caught it.
    pub fn has_forbidden(&self, word: &str) -> bool {
        let (bigram, trigram) = self.scan_both(word);
        bigram || trigram
    }

    /// Same check for an unfinished word prefix, for mid-typing correction.
    pub fn prefix_has_forbidden(&self, prefix: &str) -> bool {
        let (bigram, trigram) = self.scan_both(prefix);
        bigram || trigram
    }

    /// Actual size of the bitmaps in bytes, for verifying the memory budget.
    pub fn memory_bytes(&self) -> usize {
        self.bigram_bits.len() + self.trigram_bits.len()
    }

    /// True when no forbidden n-gram was registered at all.
    pub fn is_empty(&self) -> bool {
        self.is_empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::alphabet;
    use std::fs;

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn detects_a_registered_bigram_and_trigram() {
        let map = NgramBitmap::build(alphabet("ru"), strings(&["аъ"]), strings(&["йцу"]));
        assert!(map.has_forbidden("баъв"));
        assert!(map.has_forbidden("койцух"));
    }

    #[test]
    fn word_without_forbidden_ngrams_is_clean() {
        let map = NgramBitmap::build(alphabet("ru"), strings(&["аъ"]), strings(&["йцу"]));
        assert!(!map.has_forbidden("привет"));
    }

    #[test]
    fn reference_case_forbidden_trigram_prefix() {
        // "йцу" is a forbidden Russian trigram; the English
        // map must not flag "qwe" — it occurs in "request", "sequel".
        let ru = NgramBitmap::build(alphabet("ru"), strings(&[]), strings(&["йцу"]));
        assert!(ru.prefix_has_forbidden("йцу"));

        let en = NgramBitmap::build(alphabet("us"), strings(&[]), strings(&["abc"]));
        assert!(!en.prefix_has_forbidden("qwe"));
    }

    #[test]
    fn out_of_alphabet_character_breaks_the_sequence() {
        // "а-ъ" must not be read as the bigram "аъ": the hyphen isn't in the
        // alphabet and severs the window.
        let map = NgramBitmap::build(alphabet("ru"), strings(&["аъ"]), strings(&[]));
        assert!(!map.has_forbidden("а-ъ"));
        assert!(map.has_forbidden("аъ"));
    }

    #[test]
    fn yo_folds_to_ye_but_not_the_reverse() {
        // Text written with `ё` must still match rules recorded with `е`,
        // because corpora spell the same word both ways.
        let built_with_e = NgramBitmap::build(alphabet("ru"), strings(&[]), strings(&["сел"]));
        assert!(built_with_e.has_forbidden("сел"));
        assert!(built_with_e.has_forbidden("сёл"));

        // The reverse must not hold: a sequence banned only in its `ё` spelling
        // says nothing about the far more common `е` spelling. Folding both
        // ways condemned ordinary words like `хейтер` that are merely absent
        // from the dictionary.
        let built_with_yo = NgramBitmap::build(alphabet("ru"), strings(&[]), strings(&["ёлк"]));
        assert!(built_with_yo.has_forbidden("ёлка"));
        assert!(!built_with_yo.has_forbidden("елка"));
    }

    #[test]
    fn bigram_and_trigram_stages_are_independent() {
        // Stage 4 (bigrams) and stage 5 (trigrams) run separately, with
        // stage 5 only consulted once stage 4 already hit. A word whose
        // bigrams are clean but that contains a forbidden trigram must not
        // be flagged by the bigram-only predicate, even though the combined
        // `has_forbidden` does flag it.
        let map = NgramBitmap::build(alphabet("ru"), strings(&[]), strings(&["йцу"]));
        assert!(!map.has_forbidden_bigram("койцух"));
        assert!(map.has_forbidden_trigram("койцух"));
        assert!(map.has_forbidden("койцух"));
    }

    #[test]
    fn empty_map_flags_nothing() {
        let map = NgramBitmap::build(alphabet("ru"), strings(&[]), strings(&[]));
        assert!(map.is_empty());
        assert!(!map.has_forbidden("привет"));
        assert!(!map.prefix_has_forbidden("пр"));
    }

    #[test]
    fn memory_budget_holds_for_full_alphabets() {
        // Bitmaps for both languages combined must stay well under the
        // ≤6 MB daemon RSS budget — this is a regression guard on that.
        let ru = NgramBitmap::build(alphabet("ru"), strings(&["аъ"]), strings(&["йцу"]));
        assert!(
            ru.memory_bytes() < 6000,
            "ru bitmap is {} bytes",
            ru.memory_bytes()
        );

        let en = NgramBitmap::build(alphabet("us"), strings(&["qz"]), strings(&["xyz"]));
        assert!(
            en.memory_bytes() < 3000,
            "en bitmap is {} bytes",
            en.memory_bytes()
        );
    }

    #[test]
    fn builds_from_the_real_trigram_data() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/languages/ru/proto3");
        let contents = fs::read_to_string(path).expect("the project's own Russian trigram table");
        let trigrams = contents.lines().map(|l| l.to_string());
        let map = NgramBitmap::build(alphabet("ru"), std::iter::empty(), trigrams);

        assert!(
            map.memory_bytes() < 6000,
            "ru bitmap from real data is {} bytes",
            map.memory_bytes()
        );
        assert!(!map.has_forbidden("привет"));
    }
}
