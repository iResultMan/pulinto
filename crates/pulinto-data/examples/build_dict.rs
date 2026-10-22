//! Compile `data/dict/<lang>.tsv` frequency lists into `.fst` word lists.
//!
//! Usage: cargo run --release --example build_dict -p pulinto-data [min_count]

use std::path::Path;

use pulinto_data::WordList;

fn main() {
    let min_count: u64 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(3);

    let dir = Path::new("data/dict");
    let mut total = 0usize;
    for lang in ["ru", "en"] {
        let source = dir.join(format!("{lang}.tsv"));
        let out = dir.join(format!("{lang}.fst"));
        match WordList::build_from_tsv(&source, &out, min_count) {
            Ok(written) => {
                let list = WordList::load(&out).expect("just written");
                total += list.memory_bytes();
                println!(
                    "{lang}: {written} words -> {} ({} KB)",
                    out.display(),
                    list.memory_bytes() / 1024
                );
            }
            Err(err) => eprintln!("{lang}: {err}"),
        }
    }
    println!("total resident size: {} KB", total / 1024);
}
