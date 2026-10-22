//! `pulinto judge` — run the real decision cascade
//! against live data, showing which stage produced the verdict.

use pulinto_core::cascade::{self, Stage};

use pulinto_data::profile as profiles;

pub fn run(args: &[String]) {
    let mut context = String::new();
    let mut layout_id: Option<String> = None;
    let mut words: Vec<String> = Vec::new();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--context" if index + 1 < args.len() => {
                context = args[index + 1].clone();
                index += 2;
            }
            "--layout" if index + 1 < args.len() => {
                layout_id = Some(args[index + 1].clone());
                index += 2;
            }
            other => {
                words.push(other.to_string());
                index += 1;
            }
        }
    }

    if words.is_empty() {
        eprintln!("usage: pulinto judge [--context \"prev words\"] [--layout ru|us] <word>...");
        std::process::exit(1);
    }

    let all = profiles::all();
    profiles::warn_if_incomplete(&all);
    let refs = profiles::refs(&all);
    let context_langs = profiles::context_langs(&context);

    for word in &words {
        let current = profiles::assume_layout(&all, word, layout_id.as_deref());
        let assumed = all[current].layout_id();

        let decision = cascade::decide(word, &refs, current, &context_langs);
        let stage = match decision.stage {
            Stage::StopRule(reason) => format!("stop rule ({reason:?})"),
            Stage::Initial => "initial".into(),
            Stage::Abbreviation => "Latin abbreviation".into(),
            Stage::Compound => "compound: abbreviation-hyphen-word".into(),
            Stage::Rule => "regex rule".into(),
            Stage::ShortWord => "short word".into(),
            Stage::Dictionary => "dictionary".into(),
            Stage::Bigram => "bigrams".into(),
            Stage::Trigram => "trigrams".into(),
            Stage::Undecided => "undecided".into(),
        };
        let verdict = if decision.convert { "CONVERT" } else { "keep" };
        println!(
            "{word}  [layout: {assumed}, context: {}]\n  {verdict} -> {}\n  stage: {stage}",
            if context_langs.is_empty() {
                "empty".to_string()
            } else {
                context_langs.join(",")
            },
            decision.text
        );
    }
}
