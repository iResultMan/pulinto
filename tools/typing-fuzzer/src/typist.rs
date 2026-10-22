//! A robot that types like a person and remembers exactly what it meant.
//!
//! The point is the ground truth. A human tester finds real defects — the live
//! test on 3 August found two that a 21 300-word corpus never would — but
//! cannot tell you afterwards which key they actually pressed. This can: for
//! every word it records the layout it chose, the scan codes it sent and what
//! the screen should therefore have shown. Put that next to what the receiving
//! window actually got and next to the daemon's own log, and a divergence
//! names itself.
//!
//! Typing goes through a uinput device, because that is the only path the
//! daemon can see. `xdotool` and anything else built on XTEST injects at a
//! level `/dev/input` never hears, so a rig built on it would measure silence
//! and report success.
//!
//! The device is named so the daemon treats it as an ordinary keyboard — the
//! daemon excludes only its *own* injector, by name.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pulinto_core::layout::{Layout, Level};
use pulinto_input::evdev::POSITION_TO_EVDEV_KEYCODE;
use pulinto_input::grab::{EventType, InputEvent};
use pulinto_input::injector::{Injector, Timing, KEY_LEFTSHIFT};

const DEVICE: &str = "PuLinTo Test Typist";
const KEY_SPACE: u16 = 57;
/// Delimiters a person actually types. Punctuation counts — the customer said
/// so, and on `us` those keys really are punctuation. On `ru` the same keys are
/// letters, so the rig sends them only when typing on `us` and lets the daemon
/// prove it can tell the difference.
/// Punctuation keys, per layout the machine is actually in.
///
/// Which key is a full stop depends on the layout, and the typist has to press
/// the one a human would. On `us` that is `.` (52); on `ru` the same character
/// lives on the `/` key (53), while 52 there is the letter `ю`.
///
/// Getting this wrong invented a defect and cost a cycle chasing it: the typist
/// tapped 52 while the machine was in `ru`, so what reached the screen was
/// `течениею` — a word with a letter stuck on the end, which no human would
/// have produced. The daemon was then blamed for gluing it to the next word.
const PUNCTUATION_US: [u16; 4] = [52, 51, 39, 53]; // . , ; /
const PUNCTUATION_RU: [u16; 1] = [53]; // точка на кириллице — клавиша `/`

struct Options {
    source: String,
    log: String,
    words: usize,
    /// Pause after each word. Long enough for a correction to finish, or short
    /// enough to race it — both are worth measuring, so it is a knob.
    word_pause_ms: u64,
    /// Chance that a word is typed in the wrong layout.
    wrong_layout_percent: u64,
    wait_s: u64,
    seed: u64,
    delimiters: String, // "space", "punct", "mixed"
    length_filter: String, // "all", "short", "normal", "long"
    /// Связный сценарий (`make_script.py`) вместо случайных слов: печать «руки
    /// раскладку не трогают».
    script: String,
    /// Каталог протокола с окном-приёмником `sink_wl.py`.
    ctl: String,
    /// Слов в пачке, после которой окно сдаёт текст и очищается.
    batch: usize,
    /// Выдержка клавиши машинистки, мс.
    hold_ms: u64,
    /// Перекрытие, мс: последняя буква слова отпускается через столько после
    /// нажатия разделителя, как у быстрой руки. 0 — строго нажал-отпустил.
    /// Ядро гасит повторное нажатие ещё не отпущенной клавиши, и без
    /// перекрытия стенд был слеп к потере последней буквы при замене.
    overlap_ms: u64,
    /// Варианты XKB раскладок экрана, `ru=mac,us=mac`: таблицы позиций
    /// следуют за keymap, иначе на сессии с `ru(mac)` машинистка жмёт ё и
    /// точку не на тех клавишах. Пусто — стандартные таблицы.
    variants: String,
    /// Пауза перед сдачей пачки: последняя замена должна успеть дойти.
    settle_ms: u64,
    /// Файл, где виртуальный экран держит текущую раскладку. Пусто — спросить
    /// рабочий стол.
    layout_file: String,
    /// Сценарий: вероятность (%) того, что перед словом машинистка сама
    /// нажмёт Alt+Shift — слово уйдёт в произвольной раскладке.
    shuffle_percent: u64,
    /// Сценарий: вероятность (%) двойного Alt после слова. Alt-цикл уводит
    /// законченное слово в другую раскладку и второй Alt возвращает — на
    /// экране должно остаться то же, что без них.
    alt_percent: u64,
}

impl Options {
    fn parse() -> Self {
        let mut options = Options {
            source: String::new(),
            log: "typist.jsonl".into(),
            words: 200,
            word_pause_ms: 600,
            wrong_layout_percent: 50,
            wait_s: 8,
            seed: 20260803,
            delimiters: "mixed".into(),
            length_filter: "all".into(),
            script: String::new(),
            ctl: String::new(),
            batch: 200,
            hold_ms: Timing::default().key_hold_ms,
            overlap_ms: 0,
            variants: String::new(),
            settle_ms: 800,
            layout_file: String::new(),
            shuffle_percent: 0,
            alt_percent: 0,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut value = || args.next().unwrap_or_default();
            match arg.as_str() {
                "--source" => options.source = value(),
                "--log" => options.log = value(),
                "--words" => options.words = value().parse().unwrap_or(options.words),
                "--pause" => {
                    options.word_pause_ms = value().parse().unwrap_or(options.word_pause_ms)
                }
                "--wrong" => {
                    options.wrong_layout_percent =
                        value().parse().unwrap_or(options.wrong_layout_percent)
                }
                "--wait" => options.wait_s = value().parse().unwrap_or(options.wait_s),
                "--seed" => options.seed = value().parse().unwrap_or(options.seed),
                "--delimiters" => options.delimiters = value(),
                "--length" => options.length_filter = value(),
                "--script" => options.script = value(),
                "--ctl" => options.ctl = value(),
                "--batch" => options.batch = value().parse().unwrap_or(options.batch),
                "--hold" => options.hold_ms = value().parse().unwrap_or(options.hold_ms),
                "--overlap" => {
                    options.overlap_ms = value().parse().unwrap_or(options.overlap_ms)
                }
                "--variants" => options.variants = value(),
                "--settle" => options.settle_ms = value().parse().unwrap_or(options.settle_ms),
                "--layout-file" => options.layout_file = value(),
                "--alt" => options.alt_percent = value().parse().unwrap_or(options.alt_percent),
                "--shuffle" => {
                    options.shuffle_percent = value().parse().unwrap_or(options.shuffle_percent)
                }
                other => eprintln!("неизвестный аргумент: {other}"),
            }
        }
        options
    }
}

/// Deterministic and self-contained: a run has to be repeatable exactly, and a
/// dependency is not worth it for this.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next() % bound
        }
    }
}

/// Type a word and its delimiter. With `overlap_ms` the last letter is still
/// held when the delimiter goes down, the way a fast hand rolls over keys.
fn type_word(injector: &Injector, strokes: &[(u16, bool)], delimiter: (u16, bool), overlap_ms: u64) {
    let Some(((last, last_shift), head)) = strokes.split_last().filter(|_| overlap_ms > 0) else {
        let _ = injector.replay(strokes);
        let _ = injector.tap(delimiter.0, delimiter.1);
        return;
    };
    let _ = injector.replay(head);
    let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
    let key = |code: u16, value: i32| InputEvent::new(EventType::KEY, code, value);
    let hold = Duration::from_millis(injector.timing.key_hold_ms);
    if *last_shift {
        let _ = injector.forward(&[key(KEY_LEFTSHIFT, 1), syn]);
    }
    let _ = injector.forward(&[key(*last, 1), syn]);
    std::thread::sleep(hold);
    let _ = injector.forward(&[key(delimiter.0, 1), syn]);
    std::thread::sleep(Duration::from_millis(overlap_ms));
    let _ = injector.forward(&[key(*last, 0), syn]);
    if *last_shift {
        let _ = injector.forward(&[key(KEY_LEFTSHIFT, 0), syn]);
    }
    std::thread::sleep(hold);
    let _ = injector.forward(&[key(delimiter.0, 0), syn]);
    std::thread::sleep(hold);
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Scan codes that produce `word` on `layout`, or `None` if any character is
/// not on the 47-key block — those words are skipped rather than mangled.
fn strokes_for(word: &str, layout: &Layout) -> Option<Vec<(u16, bool)>> {
    word.chars()
        .map(|ch| {
            let (position, level) = layout.position_of(ch)?;
            let keycode = POSITION_TO_EVDEV_KEYCODE.get(position)?;
            Some((*keycode, level == Level::Shifted))
        })
        .collect()
}

fn native_layout(word: &str) -> &'static str {
    if word
        .chars()
        .any(|ch| ('\u{0400}'..='\u{04FF}').contains(&ch))
    {
        "ru"
    } else {
        "us"
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn main() {
    let options = Options::parse();
    pulinto_core::layout::use_variants(options.variants.split(',').filter_map(|pair| {
        let (id, variant) = pair.split_once('=')?;
        Some((id.trim(), variant.trim()))
    }));
    if !options.script.is_empty() {
        run_script(&options);
        return;
    }
    if options.source.is_empty() {
        eprintln!("нужен --source <файл с текстом>");
        std::process::exit(1);
    }

    let text = match std::fs::read_to_string(&options.source) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("не прочитать {}: {error}", options.source);
            std::process::exit(1);
        }
    };

    // Words worth typing: letters only, filtered by length_filter
    let us = Layout::by_id("us").expect("us");
    let ru = Layout::by_id("ru").expect("ru");
    let words: Vec<String> = text
        .split(|ch: char| !ch.is_alphabetic())
        .filter(|word| {
            let len = word.chars().count();
            match options.length_filter.as_str() {
                "short" => len >= 1 && len <= 3,
                "normal" => len >= 4 && len <= 9,
                "long" => len >= 10,
                _ => len >= 2,
            }
        })
        .filter(|word| {
            let layout = if native_layout(word) == "ru" { ru } else { us };
            strokes_for(word, layout).is_some()
        })
        .map(str::to_string)
        .collect();

    if words.is_empty() {
        eprintln!("в источнике нет пригодных слов для фильтра length={}", options.length_filter);
        std::process::exit(1);
    }
    eprintln!("слов в источнике после фильтрации: {}", words.len());

    let injector = match Injector::named(Timing::default(), DEVICE) {
        Ok(injector) => injector,
        Err(error) => {
            eprintln!("не открыть /dev/uinput: {error}");
            std::process::exit(1);
        }
    };
    let layout_control = pulinto_layout::create_default_layout_controller();
    eprintln!("контроллер раскладки: {}", layout_control.name());

    let log = File::create(&options.log).expect("файл лога");
    let mut log = BufWriter::new(log);

    // The daemon enumerates keyboards once, at startup: the device has to exist
    // before it does. Creating it above and waiting here is what makes the
    // daemon see us at all.
    eprintln!("жду {} с — пусть поднимется демон и наведётся фокус…", options.wait_s);
    std::thread::sleep(Duration::from_secs(options.wait_s));

    let mut rng = Rng(options.seed | 1);
    let mut switches = 0usize;
    let mut wrong = 0usize;

    for index in 0..options.words {
        let word = &words[rng.below(words.len() as u64) as usize];
        let native = native_layout(word);
        let other = if native == "ru" { "us" } else { "ru" };

        let type_in = if rng.below(100) < options.wrong_layout_percent {
            wrong += 1;
            other
        } else {
            native
        };

        let native_layout_table = Layout::by_id(native).expect("layout");
        let Some(strokes) = strokes_for(word, native_layout_table) else {
            continue;
        };

        // What the screen shows before the daemon touches anything: the same
        // keys read through the layout we are actually typing on.
        let shown_layout = Layout::by_id(type_in).expect("layout");
        let shown: String = strokes
            .iter()
            .filter_map(|(code, shift)| {
                let position = POSITION_TO_EVDEV_KEYCODE.iter().position(|c| c == code)?;
                shown_layout.char_at(
                    position,
                    if *shift { Level::Shifted } else { Level::Unshifted },
                )
            })
            .collect();

        let before = layout_control.current_layout_id();
        if before.as_deref() != Some(type_in) {
            let mut attempts = 0;
            while layout_control.current_layout_id().as_deref() != Some(type_in) && attempts < 4 {
                // Send Alt+Shift key combination via uinput (KEY_LEFTALT=56 with shift=true)
                let _ = injector.tap(56, true);
                std::thread::sleep(Duration::from_millis(100));
                attempts += 1;
            }
            if layout_control.current_layout_id().as_deref() == Some(type_in) {
                switches += 1;
            } else {
                eprintln!("не удалось переключить раскладку на {type_in} клавишами Alt+Shift!");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let actually_in = layout_control
            .current_layout_id()
            .unwrap_or_else(|| "?".into());

        // Chosen by the layout the machine is *in*, not the one the word was
        // meant for: the punctuation key a human reaches for is the one that
        // draws punctuation right now.
        let punctuation: &[u16] = if actually_in == "ru" {
            &PUNCTUATION_RU
        } else {
            &PUNCTUATION_US
        };
        let pick = |rng: &mut Rng| punctuation[rng.below(punctuation.len() as u64) as usize];
        let delimiter = match options.delimiters.as_str() {
            "space" => KEY_SPACE,
            "punct" => pick(&mut rng),
            _ => {
                if rng.below(100) < 40 {
                    pick(&mut rng)
                } else {
                    KEY_SPACE
                }
            }
        };

        let started = now_ms();
        type_word(&injector, &strokes, (delimiter, false), options.overlap_ms);
        let finished = now_ms();

        writeln!(
            log,
            "{{\"t\":{started},\"t_end\":{finished},\"seq\":{index},\"word\":\"{}\",\
             \"native\":\"{native}\",\"typed_in\":\"{type_in}\",\"layout_was\":\"{}\",\
             \"expect\":\"{}\",\"shown\":\"{}\",\"delimiter\":{delimiter},\"codes\":{:?}}}",
            escape(word),
            escape(&actually_in),
            if type_in == native { "KEEP" } else { "CONVERT" },
            escape(&shown),
            strokes.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        )
        .expect("запись лога");

        std::thread::sleep(Duration::from_millis(options.word_pause_ms));
    }

    log.flush().ok();
    eprintln!(
        "напечатано слов: {}, переключений раскладки: {switches}, из них в чужой раскладке: {wrong}",
        options.words
    );
    // The device must outlive the last events.
    std::thread::sleep(Duration::from_millis(400));
}

/// Ждёт, пока раскладка станет отличной от `before`: аккорд машинистки стоит
/// в очереди демона за его собственной заменой и доходит до экрана с опозданием.
fn wait_layout_change(current_layout: &dyn Fn() -> Option<String>, before: Option<&str>) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(1500);
    while std::time::Instant::now() < deadline {
        if current_layout().as_deref() != before {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Ждёт, пока модель экрана `quiet_ms` ничего не получала (`last-event` в
/// `ctl`). Без модели (живой Wayland) файла нет — не ждём.
fn wait_quiet(ctl: &str, quiet_ms: u128) {
    if ctl.is_empty() {
        return;
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match read_ctl(ctl, "last-event").and_then(|text| text.parse::<u128>().ok()) {
            Some(last) if now_ms().saturating_sub(last) < quiet_ms => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => return,
        }
    }
}

fn read_ctl(ctl: &str, name: &str) -> Option<String> {
    std::fs::read_to_string(std::path::Path::new(ctl).join(name))
        .ok()
        .map(|text| text.trim().to_string())
}

/// Печатать только в окно-приёмник. Под Wayland фокус снаружи не узнать —
/// окно само пишет, в фокусе ли оно. Ждём до десяти минут, потом сдаёмся:
/// печатать вслепую в чужое окно хуже, чем оборвать прогон.
fn wait_for_focus(ctl: &str) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    let mut warned = false;
    while std::time::Instant::now() < deadline {
        if read_ctl(ctl, "focus").as_deref() == Some("1") {
            if warned {
                eprintln!("фокус вернулся на приёмник");
            }
            return true;
        }
        if !warned {
            eprintln!("приёмник не в фокусе — жду");
            warned = true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn flush(ctl: &str, batch: usize) -> bool {
    let path = std::path::Path::new(ctl).join("flush-req");
    let tmp = std::path::Path::new(ctl).join("flush-req.tmp");
    if std::fs::write(&tmp, batch.to_string()).is_err() || std::fs::rename(&tmp, &path).is_err() {
        return false;
    }
    // Окно сдаёт пачку только в тишине, а демон может отставать на десятки слов.
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        if read_ctl(ctl, "flush-ack").as_deref() == Some(batch.to_string().as_str()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Связный текст, раскладку машинистка не трогает никогда — как заказчик.
/// Каждое слово печатается клавишами своей родной раскладки; если машина в
/// другой, на экране сначала появится абракадабра, и исправить её — дело
/// демона. Эталон поэтому простой: на экране должен оказаться исходный текст.
fn run_script(options: &Options) {
    let tokens: Vec<String> = std::fs::read_to_string(&options.script)
        .unwrap_or_else(|error| {
            eprintln!("не прочитать {}: {error}", options.script);
            std::process::exit(1);
        })
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if options.ctl.is_empty() {
        eprintln!("для сценария нужен --ctl <каталог протокола с приёмником>");
        std::process::exit(1);
    }

    let timing = Timing {
        key_hold_ms: options.hold_ms,
        ..Timing::default()
    };
    let injector = Injector::named(timing, DEVICE).unwrap_or_else(|error| {
        eprintln!("не открыть /dev/uinput: {error}");
        std::process::exit(1);
    });
    let layout_control = pulinto_layout::create_default_layout_controller();
    eprintln!("контроллер раскладки: {}, слов: {}", layout_control.name(), tokens.len());
    let current_layout = || -> Option<String> {
        if options.layout_file.is_empty() {
            layout_control.current_layout_id()
        } else {
            std::fs::read_to_string(&options.layout_file)
                .ok()
                .map(|text| text.trim().to_string())
        }
    };
    let mut log = BufWriter::new(File::create(&options.log).expect("файл лога"));

    eprintln!("жду {} с — пусть поднимется демон…", options.wait_s);
    std::thread::sleep(Duration::from_secs(options.wait_s));
    if !wait_for_focus(&options.ctl) {
        eprintln!("приёмник так и не получил фокус — прогон не начат");
        std::process::exit(2);
    }

    // Старт всегда с латиницы: прогон должен быть воспроизводим.
    let mut attempts = 0;
    while current_layout().as_deref() != Some("us") && attempts < 4 {
        let _ = injector.tap(56, true);
        std::thread::sleep(Duration::from_millis(300));
        attempts += 1;
    }

    let mut rng = Rng(options.seed | 1);
    let mut batch = 0usize;
    let mut in_batch = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if rng.below(100) < options.shuffle_percent {
            // Как человек: Alt+Shift наугад, затем слово. Демон видит
            // переключение в потоке клавиш — но доносит его до экрана только
            // после того, как доиграет замену предыдущего слова (до 400 мс),
            // так что раскладке верим лишь после того, как она сменилась.
            let before = current_layout();
            let _ = injector.tap(56, true);
            if !wait_layout_change(&current_layout, before.as_deref()) {
                eprintln!("слово {index}: раскладка не сменилась после Alt+Shift за 1,5 с");
            }
        }
        // Замена предыдущего слова могла ещё идти, когда мы читали раскладку:
        // ждём, пока экран хоть 120 мс ничего не получал.
        wait_quiet(&options.ctl, 120);
        let native = native_layout(token);
        let table = Layout::by_id(native).expect("layout");
        let Some(strokes) = strokes_for(token, table) else {
            continue;
        };
        if !wait_for_focus(&options.ctl) {
            eprintln!("фокус потерян надолго — прогон оборван на слове {index}");
            break;
        }
        let layout_was = current_layout().unwrap_or_else(|| "?".into());
        let shown: String = Layout::by_id(&layout_was)
            .map(|layout| {
                strokes
                    .iter()
                    .filter_map(|(code, shift)| {
                        let position = POSITION_TO_EVDEV_KEYCODE.iter().position(|c| c == code)?;
                        layout.char_at(
                            position,
                            if *shift { Level::Shifted } else { Level::Unshifted },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let started = now_ms();
        type_word(&injector, &strokes, (KEY_SPACE, false), options.overlap_ms);
        let alt = rng.below(100) < options.alt_percent;
        if alt {
            // Человеческие паузы: Alt после пробела, и ещё раз.
            std::thread::sleep(Duration::from_millis(150));
            let _ = injector.tap(56, false);
            std::thread::sleep(Duration::from_millis(150));
            let _ = injector.tap(56, false);
        }
        writeln!(
            log,
            "{{\"t\":{started},\"seq\":{index},\"batch\":{batch},\"expect\":\"{}\",\
             \"native\":\"{native}\",\"layout_was\":\"{}\",\"shown\":\"{}\",\"alt\":{alt}}}",
            escape(token),
            escape(&layout_was),
            escape(&shown),
        )
        .expect("запись лога");
        std::thread::sleep(Duration::from_millis(options.word_pause_ms));

        in_batch += 1;
        if in_batch == options.batch || index + 1 == tokens.len() {
            std::thread::sleep(Duration::from_millis(options.settle_ms));
            if !flush(&options.ctl, batch) {
                eprintln!("приёмник не сдал пачку {batch} — прогон оборван");
                break;
            }
            writeln!(log, "{{\"flushed\":{batch}}}").ok();
            log.flush().ok();
            batch += 1;
            in_batch = 0;
            if batch % 10 == 0 {
                eprintln!("пачек сдано: {batch}, слов: {}", index + 1);
            }
        }
    }
    log.flush().ok();
    eprintln!("сценарий окончен");
    std::thread::sleep(Duration::from_millis(400));
}
