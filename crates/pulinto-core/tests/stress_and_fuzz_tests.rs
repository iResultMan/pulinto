//! Stress, Chaos, and Edge-Case Fuzzing Tests for pulinto-core.

use pulinto_core::autocorrect::AutoCorrectEngine;
use pulinto_core::case::{apply, CaseMode, fix_incidental_caps, fix_two_capitals};
use pulinto_core::exceptions::ExceptionManager;
use pulinto_core::layout::{convert, guess_layout, Layout};
use pulinto_core::cascade::{self, LanguageProfile};
use pulinto_core::ngram_bitmap::NgramBitmap;
use pulinto_core::pack::alphabet;
use pulinto_core::translit::{lat_to_ru, ru_to_lat, translit};
use pulinto_core::word_buffer::WordBuffer;

#[test]
fn test_fuzz_layout_conversion_no_panics() {
    let us = Layout::by_id("us").unwrap();
    let ru = Layout::by_id("ru").unwrap();

    let chaos_inputs: Vec<String> = vec![
        "".into(),
        "\0".into(),
        "\0\x01\x02\x03\x7F".into(),
        "🚀🔥🎉✨👑".into(),
        "中文测试".into(),
        "тест 123 !@#$%^&*()_+".into(),
        "äöüßéèêâà".into(),
        "ﷺ".into(),
        "a".repeat(100_000),
        "ghbdtn".repeat(10_000),
        "\n\r\t\u{200B}\u{FEFF}".into(),
    ];

    for input in &chaos_inputs {
        let res1 = convert(input, us, ru);
        let res2 = convert(input, ru, us);
        let _guessed = guess_layout(input);

        assert_eq!(res1.chars().count(), input.chars().count());
        assert_eq!(res2.chars().count(), input.chars().count());
    }
}

#[test]
fn test_fuzz_cascade_no_panics() {
    struct Fuzz {
        layout: &'static Layout,
        ngrams: NgramBitmap,
    }
    impl LanguageProfile for Fuzz {
        fn layout(&self) -> &'static Layout {
            self.layout
        }
        fn matches_rule(&self, _word: &str) -> bool {
            false
        }
        fn in_dictionary(&self, word: &str) -> bool {
            word.chars().count() > 3
        }
        fn ngrams(&self) -> &NgramBitmap {
            &self.ngrams
        }
    }

    let us = Fuzz {
        layout: Layout::by_id("us").unwrap(),
        ngrams: NgramBitmap::build(alphabet("us"), vec!["gh".into()], vec![]),
    };
    let ru = Fuzz {
        layout: Layout::by_id("ru").unwrap(),
        ngrams: NgramBitmap::build(alphabet("ru"), vec!["ыъ".into()], vec![]),
    };
    let profiles = [&us, &ru];

    let chaos: Vec<String> = vec![
        String::new(),
        "a".into(),
        "b".into(),
        "ghbdtn".into(),
        "привет".into(),
        "Qw3rty!x".into(),
        "std::vector".into(),
        "🚀🔥".into(),
        "\0\x01".into(),
        "тест 123 !@#".into(),
        "a".repeat(5_000),
    ];

    for input in &chaos {
        for current in 0..profiles.len() {
            for context in [&[][..], &["ru"][..], &["us", "ru"][..]] {
                let _ = cascade::decide(input, &profiles, current, context);
            }
        }
    }
}

#[test]
fn test_fuzz_case_transforms_no_panics() {
    let chaos_inputs: Vec<String> = vec![
        "".into(),
        "a".into(),
        "A".into(),
        "i".into(),
        "İ".into(),
        "ß".into(),
        "σ".into(),
        "ς".into(),
        "ПРивет".into(),
        "пРИВЕТ".into(),
        "123!@#".into(),
        "🚀🔥".into(),
        "a1B2c3D4".repeat(1_000),
    ];

    let modes = vec![
        CaseMode::Upper,
        CaseMode::Lower,
        CaseMode::Title,
        CaseMode::Sentence,
    ];

    for input in &chaos_inputs {
        for mode in &modes {
            let _out = apply(input, *mode);
        }
        let _two_caps = fix_two_capitals(input);
        let _inc_caps = fix_incidental_caps(input);
    }
}

#[test]
fn test_fuzz_transliteration_no_panics() {
    let chaos_inputs: Vec<String> = vec![
        "".into(),
        "привет мир".into(),
        "privet mir".into(),
        "Щука".into(),
        "Shchuka".into(),
        "12345".into(),
        "\n\t".into(),
        "🎉🚀".into(),
        "Транслитерация".repeat(500),
    ];

    for input in &chaos_inputs {
        let _t1 = translit(input);
        let _t2 = ru_to_lat(input);
        let _t3 = lat_to_ru(input);
    }
}

#[test]
fn test_stress_word_buffer_100k_ops() {
    let mut wb = WordBuffer::new();

    for i in 0..100_000 {
        match i % 5 {
            0 => wb.push_char(((i % 26) as u8 + b'a') as char),
            1 => wb.pop_char(),
            2 => {
                if !wb.is_empty() {
                    let w = wb.current_word().to_string();
                    wb.record_conversion(w.clone(), "conv".into(), "us".into(), "ru".into());
                }
            }
            3 => {
                let _undo = wb.pop_undo();
            }
            _ => wb.clear(),
        }
    }
}

#[test]
fn test_stress_exceptions_and_autocorrect() {
    // `default()`, not `new()`: this test fuzzes the in-memory exclusion
    // logic 10,000 times a run, and `new()` now persists every mutation to
    // disk — fine for real usage where the list is
    // small, but it would hammer the real XDG data directory here and make
    // the test needlessly slow.
    let mut exceptions = ExceptionManager::default();
    let mut autocorrect = AutoCorrectEngine::new();

    for i in 0..10_000 {
        let word = format!("word_{}", i);
        let _ = exceptions.add_word_exception(&word);
        autocorrect.add_abbreviation(&word, format!("expansion_{}", i));

        assert!(exceptions.is_word_excluded(&word));
        assert_eq!(
            autocorrect.check_abbreviation(&word).as_deref(),
            Some(format!("expansion_{}", i).as_str())
        );

        if i % 2 == 0 {
            exceptions.remove_word_exception(&word);
            autocorrect.remove_abbreviation(&word);
            assert!(!exceptions.is_word_excluded(&word));
        }
    }
}
