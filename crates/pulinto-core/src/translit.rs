//! Cyrillic <-> Latin transliteration, Punto-Switcher style.
//!
//! Conversion is table-driven so callers can swap in a user-supplied table
//! later; the direction that reads multi-character Latin sequences (`sch`,
//! `zh`, ...) needs a greedy longest-match search, since a naive left-to-right
//! single-character scan would read `sch` as `s` + `ch` and produce `сч`
//! instead of `щ`.

/// (Cyrillic letter, Latin transcription) pairs, lower-case on both sides.
/// A `static` slice rather than a built-in `match` so the table can later be
/// replaced by one loaded from user configuration.
pub static TABLE: &[(char, &str)] = &[
    ('а', "a"),
    ('б', "b"),
    ('в', "v"),
    ('г', "g"),
    ('д', "d"),
    ('е', "e"),
    ('ё', "yo"),
    ('ж', "zh"),
    ('з', "z"),
    ('и', "i"),
    ('й', "y"),
    ('к', "k"),
    ('л', "l"),
    ('м', "m"),
    ('н', "n"),
    ('о', "o"),
    ('п', "p"),
    ('р', "r"),
    ('с', "s"),
    ('т', "t"),
    ('у', "u"),
    ('ф', "f"),
    ('х', "h"),
    ('ц', "ts"),
    ('ч', "ch"),
    ('ш', "sh"),
    ('щ', "sch"),
    ('ъ', ""),
    ('ы', "y"),
    ('ь', ""),
    ('э', "e"),
    ('ю', "yu"),
    ('я', "ya"),
];

/// Convert Russian text to its Latin transliteration.
pub fn ru_to_lat(text: &str) -> String {
    for_each_word(text, |word| {
        word.iter()
            .map(|ch| ru_char_to_lat(lower(*ch)))
            .collect()
    })
}

/// Convert a Latin transliteration back to Russian.
pub fn lat_to_ru(text: &str) -> String {
    for_each_word(text, |word| {
        let lower: Vec<char> = word.iter().map(|ch| lower(*ch)).collect();
        lat_to_ru_lower(&lower)
    })
}

/// Transliterate in whichever direction the input calls for: Cyrillic input
/// goes to Latin, anything else is treated as Latin going to Cyrillic.
pub fn translit(text: &str) -> String {
    if text.chars().any(is_cyrillic) {
        ru_to_lat(text)
    } else {
        lat_to_ru(text)
    }
}

fn lower(ch: char) -> char {
    ch.to_lowercase().next().unwrap_or(ch)
}

fn is_cyrillic(ch: char) -> bool {
    let ch = lower(ch);
    TABLE.iter().any(|&(ru, _)| ru == ch)
}

fn ru_char_to_lat(ch: char) -> String {
    TABLE
        .iter()
        .find(|&&(ru, _)| ru == ch)
        .map(|&(_, lat)| lat.to_string())
        .unwrap_or_else(|| ch.to_string())
}

/// Greedy longest-match: try 3-, then 2-, then 1-character Latin sequences so
/// `sch` is claimed by щ before `sh`/`ch` get a chance at it.
fn lat_to_ru_lower(word: &[char]) -> String {
    let mut out = String::with_capacity(word.len());
    let mut i = 0;
    while i < word.len() {
        let mut matched = false;
        for len in [3usize, 2, 1] {
            if i + len > word.len() {
                continue;
            }
            if let Some(&(ru, _)) = TABLE.iter().find(|&&(_, lat)| {
                !lat.is_empty() && lat.chars().count() == len && lat.chars().eq(word[i..i + len].iter().copied())
            }) {
                out.push(ru);
                i += len;
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(word[i]);
            i += 1;
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordCase {
    Lower,
    Capitalized,
    Upper,
}

/// A single capitalised letter is ambiguous between "capitalised" and
/// "all caps" — treated as capitalised, since that is the far more common
/// case (start of a sentence) and matches what a user expects from `Щ` alone.
fn word_case(word: &[char]) -> WordCase {
    let alpha: Vec<char> = word.iter().copied().filter(|ch| ch.is_alphabetic()).collect();
    match alpha.as_slice() {
        [] => WordCase::Lower,
        [only] => {
            if only.is_uppercase() {
                WordCase::Capitalized
            } else {
                WordCase::Lower
            }
        }
        [first, rest @ ..] => {
            if rest.iter().all(|ch| ch.is_uppercase()) && first.is_uppercase() {
                WordCase::Upper
            } else if first.is_uppercase() {
                WordCase::Capitalized
            } else {
                WordCase::Lower
            }
        }
    }
}

fn apply_case(base: &str, case: WordCase) -> String {
    match case {
        WordCase::Lower => base.to_string(),
        WordCase::Upper => base.to_uppercase(),
        WordCase::Capitalized => {
            let mut chars = base.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

/// Splits `text` into alphabetic words and non-alphabetic separators,
/// converts each word through `convert_lower` (which sees a lower-cased
/// `Vec<char>`), then re-applies the word's original casing to the result.
fn for_each_word(text: &str, mut convert_lower: impl FnMut(&[char]) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word: Vec<char> = Vec::new();
    for ch in text.chars() {
        if ch.is_alphabetic() {
            word.push(ch);
            continue;
        }
        if !word.is_empty() {
            let case = word_case(&word);
            out.push_str(&apply_case(&convert_lower(&word), case));
            word.clear();
        }
        out.push(ch);
    }
    if !word.is_empty() {
        let case = word_case(&word);
        out.push_str(&apply_case(&convert_lower(&word), case));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_ru_to_lat() {
        assert_eq!(ru_to_lat("привет"), "privet");
    }

    #[test]
    fn capitalises_multi_letter_transliterations() {
        assert_eq!(ru_to_lat("Щука"), "Schuka");
    }

    #[test]
    fn uppercases_whole_words() {
        assert_eq!(ru_to_lat("ЩУКА"), "SCHUKA");
    }

    #[test]
    fn prefers_the_longest_latin_match() {
        assert_eq!(lat_to_ru("schuka"), "щука");
        assert_eq!(lat_to_ru("Schuka"), "Щука");
    }

    #[test]
    fn round_trips_common_words() {
        for word in ["привет", "щука", "чашка", "мама"] {
            assert_eq!(lat_to_ru(&ru_to_lat(word)), word);
        }
    }

    #[test]
    fn leaves_untabled_characters_alone() {
        assert_eq!(ru_to_lat("привет, world!"), "privet, world!");
    }

    #[test]
    fn picks_direction_automatically() {
        assert_eq!(translit("привет"), "privet");
        assert_eq!(translit("privet"), "привет");
    }
}
