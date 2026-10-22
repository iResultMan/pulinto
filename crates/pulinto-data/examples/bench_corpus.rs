//! Runs the PuLinTo cascade over a labelled corpus and writes per-row
//! predictions, which `docs/research/corpus2/validate.py` then scores.
//!
//!   cargo run --release --example bench_corpus -p pulinto-data -- <corpus dir> [corpus file]
//!
//! The corpus file is a TSV with the columns `id context input expect correct
//! category` (the default is `golden.tsv`; the held-out corpus is
//! `heldout.tsv`). It uses the very same profiles as the daemon: word lists,
//! n-gram tables and language packs are loaded from the project's own data. A
//! new language pack is measured the same way: add rows for it and run this.
//!
//! The layout the word is assumed to have been typed on is the one whose
//! alphabet the input is written in, and the cascade is given the layout of the
//! two preceding words as context, as the daemon has it.

use std::collections::HashMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use pulinto_core::cascade::{self, Decision, Stage, Thresholds};
use pulinto_data::profile::{self, Profile};

/// Split a TSV line, keeping empty fields.
fn fields(line: &str) -> Vec<&str> {
    line.split('\t').collect()
}

fn column_index(header: &str, name: &str) -> usize {
    fields(header)
        .iter()
        .position(|field| *field == name)
        .unwrap_or_else(|| panic!("column {name} missing"))
}

fn context_langs(context: &str) -> Vec<&'static str> {
    profile::context_langs(context)
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::StopRule(_) => "stop_rule",
        Stage::Initial => "initial",
        Stage::Abbreviation => "abbreviation",
        Stage::Compound => "compound",
        Stage::Rule => "rule",
        Stage::ShortWord => "short_word",
        Stage::Dictionary => "dictionary",
        Stage::Bigram => "bigram",
        Stage::Trigram => "trigram",
        Stage::Undecided => "undecided",
    }
}

fn main() {
    let base = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "docs/research/corpus".to_string()),
    );

    // The very same profiles the daemon uses. The bench used to build its own
    // from a hardcoded xneur path, which meant it silently kept measuring data
    // the product had already stopped using.
    let profiles = profile::all();
    profile::warn_if_incomplete(&profiles);
    for entry in &profiles {
        eprintln!("{}: словарь {}", entry.layout_id(), entry.has_word_list());
    }
    let profile_refs: Vec<&Profile> = profile::refs(&profiles);

    let corpus_name = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "golden.tsv".to_string());
    let corpus = fs::read_to_string(base.join(&corpus_name)).expect("corpus file");
    let mut lines = corpus.lines();
    let header = lines.next().expect("header");
    let columns: HashMap<&str, usize> = ["id", "context", "input", "expect", "correct", "category"]
        .iter()
        .map(|name| (*name, column_index(header, name)))
        .collect();

    let output = fs::File::create(base.join("predictions-pulinto.tsv")).expect("output file");
    let mut out = BufWriter::new(output);
    writeln!(
        out,
        "id\tcontext\tinput\texpect\tcorrect\tcategory\tassumed_layout\tpredicted\tpredicted_text\tstage"
    )
    .unwrap();

    // PULINTO_THRESHOLDS="f2,f3,f4,margin" overrides stage-3 tuning for sweeps.
    let thresholds = std::env::var("PULINTO_THRESHOLDS")
        .ok()
        .and_then(|raw| {
            let parts: Vec<u64> = raw.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            (parts.len() == 4).then(|| Thresholds {
                min_frequency: [parts[0], parts[1], parts[2]],
                margin: parts[3],
                ..Thresholds::DEFAULT
            })
        })
        .unwrap_or(Thresholds::DEFAULT);

    // PULINTO_NGRAM=legacy puts stages 4 and 5 back on the pre-2026-08 rule,
    // where "no forbidden n-gram in the current language" ended the cascade.
    // Kept so the shipped rule can be measured against the one it replaced.
    let thresholds = match std::env::var("PULINTO_NGRAM").as_deref() {
        Ok("legacy") => Thresholds { ngram_discrimination: false, ..thresholds },
        Ok("discriminate") => Thresholds { ngram_discrimination: true, ..thresholds },
        _ => thresholds,
    };

    let mut unmatched_layout = 0usize;
    for line in lines {
        let row = fields(line);
        if row.len() < columns.len() {
            continue;
        }
        let id = row[columns["id"]];
        let input = row[columns["input"]];
        let context = row[columns["context"]];

        let assumed = profile::language_of(input);
        let current = match profiles.iter().position(|p| p.layout_id() == assumed) {
            Some(index) => index,
            None => {
                unmatched_layout += 1;
                0
            }
        };

        let decision: Decision =
            cascade::decide_with(input, &profile_refs, current, &context_langs(context), &thresholds);

        writeln!(
            out,
            "{id}\t{context}\t{input}\t{}\t{}\t{}\t{assumed}\t{}\t{}\t{}",
            row[columns["expect"]],
            row[columns["correct"]],
            row[columns["category"]],
            if decision.convert { "CONVERT" } else { "KEEP" },
            decision.text,
            stage_name(decision.stage),
        )
        .unwrap();
    }

    if unmatched_layout > 0 {
        eprintln!("WARN: {unmatched_layout} rows had an unrecognised assumed layout");
    }
    eprintln!("wrote {}", base.join("predictions-pulinto.tsv").display());
}
