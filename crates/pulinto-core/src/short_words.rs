//! Stage 2 of the cascade: words of 1–2
//! characters, where n-grams carry no information at all (a one-letter word
//! has zero bigrams). Handled with explicit whitelists instead.
//!
//! The whitelists are data of the language pack (`short1`, `short2` and `keep`
//! in `data/languages/<id>/language.conf`), not code.

use crate::layout::{self, Layout};
use crate::pack;

/// Verdict for words of 1–2 characters, where n-grams carry no information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortVerdict {
    Keep,
    Convert,
    Undecided,
}

/// Is `word` one of the closed list of short forms of the layout's language
/// that the context-free two-letter rule must not touch?
pub fn is_kept_short_form(word: &str, layout_id: &str) -> bool {
    pack::language(layout_id).is_some_and(|language| language.is_kept(word))
}

/// Whether a 1–2 character word is a real word in the given layout's language.
pub fn is_valid_short_word(word: &str, layout_id: &str) -> bool {
    pack::language(layout_id).is_some_and(|language| language.is_short_word(word))
}

/// `context_langs` — layout ids of up to two preceding words, most recent
/// first; empty when the word starts a line.
pub fn judge(
    word: &str,
    current_layout_id: &str,
    target_layout_id: &str,
    context_langs: &[&str],
) -> ShortVerdict {
    let char_len = word.chars().count();
    if char_len == 0 || char_len > 2 {
        return ShortVerdict::Undecided;
    }

    if is_valid_short_word(word, current_layout_id) {
        return ShortVerdict::Keep;
    }

    // A single letter in an empty context is never converted — there is
    // no signal at all to break the tie, and a wrong guess here is the exact
    // false positive the whole design is trying to avoid.
    if char_len == 1 && context_langs.is_empty() {
        return ShortVerdict::Undecided;
    }

    let (Some(from), Some(to)) = (
        Layout::by_id(current_layout_id),
        Layout::by_id(target_layout_id),
    ) else {
        return ShortVerdict::Undecided;
    };
    let converted = layout::convert(word, from, to);

    if is_valid_short_word(&converted, target_layout_id)
        && context_langs.contains(&target_layout_id)
    {
        return ShortVerdict::Convert;
    }

    ShortVerdict::Undecided
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_valid_short_words() {
        assert!(is_valid_short_word("я", "ru"));
        assert!(is_valid_short_word("Я", "ru"));
        assert!(is_valid_short_word("а", "ru"));
        assert!(is_valid_short_word("на", "ru"));
        assert!(is_valid_short_word("a", "us"));
        assert!(is_valid_short_word("to", "us"));
        assert!(!is_valid_short_word("b", "us"));
        assert!(!is_valid_short_word("c", "us"));
        assert!(!is_valid_short_word("xyz", "ru"));
    }

    #[test]
    fn yo_folds_to_ye() {
        assert!(is_valid_short_word("её", "ru"));
    }

    #[test]
    fn converts_b_to_cyrillic_i_with_agreeing_context() {
        assert_eq!(judge("b", "us", "ru", &["ru"]), ShortVerdict::Convert);
    }

    #[test]
    fn empty_context_never_converts_a_single_letter() {
        assert_eq!(judge("b", "us", "ru", &[]), ShortVerdict::Undecided);
    }

    #[test]
    fn c_in_english_context_stays_undecided_the_source_code_trap() {
        assert_eq!(judge("c", "us", "ru", &["us"]), ShortVerdict::Undecided);
    }

    #[test]
    fn valid_english_word_is_kept() {
        assert_eq!(judge("a", "us", "ru", &["ru"]), ShortVerdict::Keep);
    }

    #[test]
    fn valid_russian_word_is_kept() {
        assert_eq!(judge("я", "ru", "us", &["ru"]), ShortVerdict::Keep);
    }

    #[test]
    fn out_of_range_length_is_not_this_stages_business() {
        assert_eq!(judge("hello", "us", "ru", &["ru"]), ShortVerdict::Undecided);
        assert_eq!(judge("", "us", "ru", &["ru"]), ShortVerdict::Undecided);
    }

    #[test]
    fn two_letter_word_can_convert_without_context_agreement_failing() {
        // Context disagreement still blocks conversion even for 2-letter words.
        assert_eq!(judge("dj", "us", "ru", &["us"]), ShortVerdict::Undecided);
    }
}
