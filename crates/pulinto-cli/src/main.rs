mod judge;
mod pack;

use pulinto_data::profile as profiles;
use std::env;
use std::process;
use pulinto_core::case::{apply, CaseMode};
use pulinto_core::layout::{convert, guess_layout, Layout};
use pulinto_core::translit::{lat_to_ru, ru_to_lat, translit};

fn print_usage() {
    println!("PuLinTo CLI v{}", env!("CARGO_PKG_VERSION"));
    println!("Usage:");
    println!("  pulinto convert [--from LANG] [--to LANG] <text>");
    println!("  pulinto detect [--json] <text>");
    println!("  pulinto judge [--context \"prev words\"] [--layout ru|us] <word>...");
    println!("  pulinto case --mode <upper|lower|title|sentence> <text>");
    println!("  pulinto translit [--dir ru2lat|lat2ru] <text>");
    println!("  pulinto exclude <add|remove|list> [word]");
    println!("  pulinto layout [--watch]");
    println!("  pulinto pack check <language pack directory>...");
    println!("  pulinto pack list");
    println!("  pulinto version");
    println!("  pulinto help");
}

fn main() {
    // The language packs on disk join the built-in ones before anything reads a layout.
    profiles::init();
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "convert" => handle_convert(&args[2..]),
        "judge" => judge::run(&args[2..]),
        "pack" => pack::run(&args[2..]),
        "detect" => handle_detect(&args[2..]),
        "case" => handle_case(&args[2..]),
        "translit" => handle_translit(&args[2..]),
        "exclude" => handle_exclude(&args[2..]),
        "layout" => handle_layout(&args[2..]),
        "build-fst" => handle_build_fst(&args[2..]),
        "help" | "-h" | "--help" => print_usage(),
        "version" | "-V" | "--version" => println!("pulinto {}", env!("CARGO_PKG_VERSION")),
        cmd => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            process::exit(1);
        }
    }
}

/// Report what the daemon believes about the keyboard, without running it.
///
/// This exists because the answer used to be wrong and nothing showed it. The
/// group number is printed next to the layout name on purpose: on an IBus
/// session those disagree — the engine switch rebuilds the keymap and the
/// active layout ends up in group 0 — and seeing group 0 named `ru` is the
/// whole point. `--watch` follows changes, which is how you check that the
/// daemon notices a layout switch it did not make.
fn handle_layout(args: &[String]) {
    let controller = pulinto_layout::create_default_layout_controller();
    println!("Controller: {}", controller.name());

    // The raw XKB view, printed next to the chosen controller's view. When
    // those two disagree, the session is desynchronised — and that
    // disagreement, invisible until you put the two lines side by side, is
    // what corrupted the user's text.
    let xkb = pulinto_layout::x11::X11LayoutController::new().ok();

    let describe = |controller: &dyn pulinto_layout::LayoutController| {
        let group = controller
            .get_current_group()
            .map(|group| group.to_string())
            .unwrap_or_else(|_| "?".into());
        let layouts = controller.layout_ids();
        format!(
            "layout: {}   group: {group}   loaded: [{}]",
            controller
                .current_layout_id()
                .unwrap_or_else(|| "unknown".into()),
            if layouts.is_empty() {
                "names unavailable".to_string()
            } else {
                layouts.join(", ")
            }
        )
    };

    let report = |previous: &mut String| {
        let mut line = describe(controller.as_ref());
        if let Some(xkb) = xkb.as_ref() {
            let raw = describe(xkb as &dyn pulinto_layout::LayoutController);
            if raw != line {
                line = format!("{line}\n   XKB's own view:  {raw}");
            }
        }
        if line != *previous {
            println!("{line}");
            *previous = line;
        }
    };

    let mut last = String::new();
    report(&mut last);

    // `--set` exercises the write path, which is the half that broke the
    // session: on an IBus desktop a layout change has to go through IBus, and
    // the only honest check is to ask for one and look at what the system says
    // afterwards.
    if let Some(position) = args.iter().position(|arg| arg == "--set") {
        match args.get(position + 1) {
            Some(wanted) => match controller.group_of(wanted) {
                Some(group) => match controller.set_group(group) {
                    Ok(()) => {
                        println!("switched to «{wanted}» (group {group})");
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        report(&mut last);
                    }
                    Err(error) => eprintln!("cannot switch to «{wanted}»: {error}"),
                },
                None => eprintln!(
                    "no layout «{wanted}»; loaded: {}",
                    controller.layout_ids().join(", ")
                ),
            },
            None => eprintln!("--set needs a layout, for example: --set ru"),
        }
    }

    if args.iter().any(|arg| arg == "--watch") {
        println!("Watching the layout, Ctrl+C to exit.");
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            report(&mut last);
        }
    }
}

fn handle_build_fst(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: pulinto build-fst <input.tsv> <output.fst> [min_count]");
        process::exit(1);
    }
    let input = std::path::Path::new(&args[0]);
    let output = std::path::Path::new(&args[1]);
    let min_count = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    println!("Building FST from {:?} -> {:?} (min_count={})", input, output, min_count);
    match pulinto_data::WordList::build_from_tsv(input, output, min_count) {
        Ok(count) => println!("Successfully compiled {} words into {:?}", count, output),
        Err(err) => {
            eprintln!("Error building FST: {}", err);
            process::exit(1);
        }
    }
}

fn handle_convert(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: Missing text to convert.");
        process::exit(1);
    }

    let mut from_id: Option<&str> = None;
    let mut to_id: Option<&str> = None;
    let mut text_parts: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--from" if i + 1 < args.len() => {
                from_id = Some(&args[i + 1]);
                i += 2;
            }
            "--to" if i + 1 < args.len() => {
                to_id = Some(&args[i + 1]);
                i += 2;
            }
            arg => {
                text_parts.push(arg);
                i += 1;
            }
        }
    }

    let text = text_parts.join(" ");
    let from_layout = from_id
        .and_then(Layout::by_id)
        .or_else(|| guess_layout(&text))
        .unwrap_or_else(|| Layout::by_id("us").unwrap());

    let target_id = to_id.unwrap_or(if from_layout.id == "us" { "ru" } else { "us" });
    let to_layout = Layout::by_id(target_id).unwrap_or_else(|| Layout::by_id("ru").unwrap());

    let converted = convert(&text, from_layout, to_layout);
    println!("{}", converted);
}

fn handle_detect(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: Missing text to detect.");
        process::exit(1);
    }

    let use_json = args.iter().any(|arg| arg == "--json");
    let text = args
        .iter()
        .filter(|&arg| arg != "--json")
        .cloned()
        .collect::<Vec<String>>()
        .join(" ");

    let all = profiles::all();
    profiles::warn_if_incomplete(&all);
    let refs = profiles::refs(&all);
    let current = profiles::assume_layout(&all, &text, None);
    let decision = pulinto_core::cascade::decide(&text, &refs, current, &[]);

    if use_json {
        println!(
            "{{\"text\":\"{}\",\"should_convert\":{},\"converted\":\"{}\",\"layout\":\"{}\",\"stage\":\"{:?}\"}}",
            text, decision.convert, decision.text, decision.layout_id, decision.stage
        );
    } else {
        println!("Input: {}", text);
        println!("Should convert: {}", decision.convert);
        println!("Converted hypothesis: {}", decision.text);
        println!("Layout: {}", decision.layout_id);
        println!("Stage: {:?}", decision.stage);
    }
}

fn handle_case(args: &[String]) {
    let mut mode = CaseMode::Lower;
    let mut text_parts: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" if i + 1 < args.len() => {
                mode = match args[i + 1].as_str() {
                    "upper" => CaseMode::Upper,
                    "lower" => CaseMode::Lower,
                    "title" => CaseMode::Title,
                    "sentence" => CaseMode::Sentence,
                    m => {
                        eprintln!("Unknown case mode: {}", m);
                        process::exit(1);
                    }
                };
                i += 2;
            }
            arg => {
                text_parts.push(arg);
                i += 1;
            }
        }
    }

    let text = text_parts.join(" ");
    println!("{}", apply(&text, mode));
}

fn handle_translit(args: &[String]) {
    let mut dir: Option<&str> = None;
    let mut text_parts: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" if i + 1 < args.len() => {
                dir = Some(&args[i + 1]);
                i += 2;
            }
            arg => {
                text_parts.push(arg);
                i += 1;
            }
        }
    }

    let text = text_parts.join(" ");
    let res = match dir {
        Some("ru2lat") => ru_to_lat(&text),
        Some("lat2ru") => lat_to_ru(&text),
        _ => translit(&text),
    };
    println!("{}", res);
}

fn handle_exclude(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: Usage: pulinto exclude <add|remove|list> [word]");
        process::exit(1);
    }

    match args[0].as_str() {
        "add" if args.len() > 1 => {
            println!("Added exception: {}", args[1]);
        }
        "remove" if args.len() > 1 => {
            println!("Removed exception: {}", args[1]);
        }
        "list" => {
            println!("Word Exceptions: (empty)");
        }
        _ => {
            eprintln!("Invalid command for exclude");
            process::exit(1);
        }
    }
}
