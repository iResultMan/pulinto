//! `pulinto pack check <dir>` and `pulinto pack list`: the tools of a person who
//! writes a language pack (see docs/LANGUAGES.md).

use pulinto_core::pack::{self, LanguagePack};
use std::path::{Path, PathBuf};
use std::process;

pub fn run(args: &[String]) {
    match args.first().map(String::as_str) {
        Some("check") if args.len() > 1 => {
            let failed = args[1..].iter().filter(|path| !check(Path::new(path))).count();
            process::exit(if failed == 0 { 0 } else { 1 });
        }
        Some("list") | None => list(),
        _ => {
            eprintln!("Usage: pulinto pack check <pack directory>...\n       pulinto pack list");
            process::exit(2);
        }
    }
}

fn list() {
    pulinto_data::profile::init();
    for language in pack::languages() {
        let source = &language.source;
        println!(
            "{:<8} {:<24} dictionary: {:<8} layouts: {}",
            source.id,
            source.name,
            source.dictionary,
            source.layouts.iter().map(|table| if table.variant.is_empty() { "standard" } else { table.variant.as_str() }).collect::<Vec<_>>().join(", ")
        );
    }
}

/// Check one pack directory (or its `language.conf`). Returns whether it is usable.
fn check(path: &Path) -> bool {
    let file: PathBuf = if path.is_dir() { path.join("language.conf") } else { path.to_path_buf() };
    let dir = file.parent().unwrap_or(Path::new("."));
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) => {
            println!("{}: cannot be read: {error}", file.display());
            return false;
        }
    };
    let parsed = match LanguagePack::parse(&text) {
        Ok(parsed) => parsed,
        Err(problems) => {
            println!("{}: {} problem(s)", file.display(), problems.len());
            for problem in problems {
                println!("  - {problem}");
            }
            return false;
        }
    };
    println!(
        "{}: the format is right. {} ({}): {} one-letter and {} two-letter words, {} abbreviations, {} layout table(s).",
        file.display(),
        parsed.name,
        parsed.id,
        parsed.short1.len(),
        parsed.short2.len(),
        parsed.abbreviations.len() + parsed.abbreviations_after_context.len(),
        parsed.layouts.len()
    );
    // What a pack needs next to language.conf to take part in every stage.
    let dictionary = format!("{}.fst", parsed.dictionary);
    let beside = |name: &str| dir.join(name).exists();
    let data_dict = dir.parent().and_then(Path::parent).map(|root| root.join("dict").join(&dictionary));
    for (what, present, effect) in [
        ("proto", beside("proto"), "the bigram stage will not work"),
        ("proto3", beside("proto3"), "the trigram stage will not work"),
        (
            dictionary.as_str(),
            beside(&dictionary) || data_dict.is_some_and(|path| path.exists()),
            "the dictionary stage (three quarters of all decisions) will not work",
        ),
    ] {
        if !present {
            println!("  note: no `{what}` next to the pack: {effect}");
        }
    }
    true
}
