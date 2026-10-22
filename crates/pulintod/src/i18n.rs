//! Interface language of the tray menu.
//!
//! The strings in the code are English, the source language: `en` needs no
//! table. Russian and Italian are looked up in `TABLE`; any other language gets
//! English, because a person who reads neither Russian nor Italian is
//! likelier to read it. The settings window follows the same rule
//! (`ui_language` in `gui/pulinto-settings`), so the two never disagree.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
    It,
}

/// The language from `PULINTO_LANG`, `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`,
/// in that order; English when none is set (the C locale counts as none).
pub fn detect(var: impl Fn(&str) -> Option<String>) -> Lang {
    for name in ["PULINTO_LANG", "LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        let Some(value) = var(name) else { continue };
        for part in value.split(':') {
            let code = part
                .split(['.', '@'])
                .next()
                .and_then(|p| p.split('_').next())
                .unwrap_or("")
                .to_lowercase();
            if code.is_empty() || code == "c" || code == "posix" {
                continue;
            }
            return match code.as_str() {
                "ru" => Lang::Ru,
                "it" => Lang::It,
                _ => Lang::En,
            };
        }
    }
    Lang::En
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
            Lang::It => "it",
        }
    }

    /// `ru`, `en` or `it`; anything else (`auto`, empty) is no choice.
    pub fn parse(code: &str) -> Option<Lang> {
        match code.trim() {
            "ru" => Some(Lang::Ru),
            "en" => Some(Lang::En),
            "it" => Some(Lang::It),
            _ => None,
        }
    }
}

/// The language the user picked in the tray menu or the settings window, as
/// 1 + its index; 0 follows the system.
static CHOSEN: AtomicU8 = AtomicU8::new(0);

pub fn choice() -> Option<Lang> {
    match CHOSEN.load(Ordering::Relaxed) {
        1 => Some(Lang::Ru),
        2 => Some(Lang::En),
        3 => Some(Lang::It),
        _ => None,
    }
}

/// Takes effect for the next menu or tooltip the tray builds.
pub fn set_choice(lang: Option<Lang>) {
    let value = match lang {
        None => 0,
        Some(Lang::Ru) => 1,
        Some(Lang::En) => 2,
        Some(Lang::It) => 3,
    };
    CHOSEN.store(value, Ordering::Relaxed);
}

/// Remembers the choice for good: the next start, the tray and the settings
/// window all read it from the config.
pub fn choose(lang: Option<Lang>) {
    set_choice(lang);
    let language = lang.map(|lang| lang.code().to_string()).unwrap_or_default();
    if let Err(error) = crate::config::Config::update(|config| config.language = language) {
        eprintln!("Could not save the settings: {error}");
    }
}

/// The choice, else the language of the system.
pub fn language() -> Lang {
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    choice().unwrap_or_else(|| *SYSTEM.get_or_init(|| detect(|name| std::env::var(name).ok())))
}

/// (English source, Russian, Italian). Placeholders in `{braces}` are kept.
const TABLE: &[(&str, &str, &str)] = &[
    (
        "Debug mode stopped by the timer. The log is kept: save a copy or delete it from the tray menu.",
        "Режим отладки остановлен по таймеру. Журнал хранится в памяти: сохраните копию или удалите его в меню значка.",
        "Modalità di debug fermata dal timer. Il registro è conservato: salvane una copia o eliminalo dal menu dell'icona.",
    ),
    (
        "Debug log copy saved: {path}. Look through it before sending: it may hold your texts.",
        "Копия журнала отладки сохранена: {path}. Просмотрите её перед отправкой: в ней могут быть ваши тексты.",
        "Copia del registro di debug salvata: {path}. Controllala prima di inviarla: può contenere i tuoi testi.",
    ),
    (
        "Debug mode is off, the logs are deleted.",
        "Режим отладки выключен, журналы удалены.",
        "Modalità di debug disattivata, i registri sono eliminati.",
    ),
    (
        "● Debug mode is recording · {minutes} min left",
        "● Идёт запись отладки · осталось {minutes} мин",
        "● Registrazione di debug in corso · restano {minutes} min",
    ),
    (
        "Stop debug mode and delete the logs",
        "Выключить отладку и удалить журналы",
        "Disattiva il debug ed elimina i registri",
    ),
    (
        "Stop debug mode and save a copy",
        "Выключить отладку и сохранить копию",
        "Disattiva il debug e salva una copia",
    ),
    (
        "A debug log is kept",
        "Журнал отладки хранится в памяти",
        "Un registro di debug è conservato",
    ),
    (
        "Save a copy of the debug log",
        "Сохранить копию журнала отладки",
        "Salva una copia del registro di debug",
    ),
    (
        "Delete the debug log",
        "Удалить журнал отладки",
        "Elimina il registro di debug",
    ),
    (
        "debug mode is recording",
        "идёт запись отладки",
        "registrazione di debug in corso",
    ),
    (
        "Debug mode is on: PuLinTo is recording. A red dot on the icon; it stops by itself in {minutes} min.",
        "Режим отладки включён: PuLinTo ведёт запись. На значке красная точка; запись остановится сама через {minutes} мин.",
        "Modalità di debug attiva: PuLinTo sta registrando. Sull'icona c'è un punto rosso; si ferma da sola tra {minutes} min.",
    ),
    ("off", "выключен", "disattivato"),
    (
        "fixes automatically",
        "исправляет автоматически",
        "corregge in automatico",
    ),
    ("hotkey only", "только по горячей клавише", "solo con la scorciatoia"),
    (
        "current application: unknown",
        "текущее приложение: неизвестно",
        "applicazione corrente: sconosciuta",
    ),
    (
        "PuLinTo is off",
        "PuLinTo выключен",
        "PuLinTo è disattivato",
    ),
    (
        "PuLinTo: hotkey only",
        "PuLinTo: только по горячей клавише",
        "PuLinTo: solo con la scorciatoia",
    ),
    (
        "PuLinTo is working · words fixed: {count}",
        "PuLinTo работает · исправлено слов: {count}",
        "PuLinTo è in funzione · parole corrette: {count}",
    ),
    ("Pause", "Приостановить", "Sospendi"),
    ("Turn on", "Включить", "Attiva"),
    ("Restart", "Перезапустить", "Riavvia"),
    (
        "Fix automatically",
        "Исправлять автоматически",
        "Correggi automaticamente",
    ),
    (
        "In the application \"{class}\"",
        "В приложении «{class}»",
        "Nell'applicazione «{class}»",
    ),
    ("Hotkey only", "Только по горячей клавише", "Solo con la scorciatoia"),
    ("Leave alone", "Не трогать", "Non toccare"),
    ("Use default", "Как по умолчанию", "Usa il predefinito"),
    (
        "{label}: switch without opening the menu",
        "{label} — переключать, не открывая меню",
        "{label}: cambia senza aprire il menu",
    ),
    (
        "No shortcut is assigned for switching",
        "Сочетание для переключения не назначено",
        "Nessuna scorciatoia assegnata per il cambio",
    ),
    (
        "Interface language",
        "Язык интерфейса",
        "Lingua dell'interfaccia",
    ),
    ("System language", "Язык системы", "Lingua di sistema"),
    (
        "Apple layout is on",
        "Раскладка Apple включена",
        "Layout Apple attivato",
    ),
    (
        "Standard layout restored",
        "Обычная раскладка возвращена",
        "Layout standard ripristinato",
    ),
    ("Settings…", "Настройки…", "Impostazioni…"),
    ("Hints", "Подсказки", "Suggerimenti"),
    (
        "The correction key: fix the word; again: the next reading",
        "Клавиша исправления — сменить раскладку слова; ещё раз — следующее прочтение",
        "Il tasto di correzione: correggi la parola; di nuovo: la lettura successiva",
    ),
    (
        "Shift+Pause/Break: convert the selected text",
        "Shift+Pause/Break — переложить выделенный текст",
        "Maiusc+Pausa/Interr: converti il testo selezionato",
    ),
    (
        "Ctrl+Alt+\\: report the last fix",
        "Ctrl+Alt+\\ — пожаловаться на последнее исправление",
        "Ctrl+Alt+\\: segnala l'ultima correzione",
    ),
    (
        "Backspace+Esc+Enter together: release the keyboard",
        "Backspace+Esc+Enter вместе — отпустить клавиатуру",
        "Backspace+Esc+Invio insieme: rilascia la tastiera",
    ),
    ("Quit", "Выйти", "Esci"),
    ("Automatic", "Автоматически", "Automatico"),
    (
        "Completely off",
        "Полностью выключено",
        "Completamente disattivato",
    ),
    (
        "{modifier} (single press)",
        "{modifier} (одиночное нажатие)",
        "{modifier} (pressione singola)",
    ),
    (
        "Left {modifier} (single press)",
        "Левый {modifier} (одиночное нажатие)",
        "{modifier} sinistro (pressione singola)",
    ),
    (
        "Right {modifier} (single press)",
        "Правый {modifier} (одиночное нажатие)",
        "{modifier} destro (pressione singola)",
    ),
    (
        "A modifier cannot be assigned as the key of a combination. For a single press, choose the modifier's “single press”.",
        "Модификатор нельзя назначить клавишей сочетания. Для одиночного нажатия выберите «одиночное нажатие» модификатора.",
        "Un modificatore non può essere assegnato come tasto di una combinazione. Per la pressione singola scegli «pressione singola» del modificatore.",
    ),
    (
        "“{key}” without Ctrl, Alt or Super is an ordinary typing key: assigning it would stop it from typing. Add a modifier or choose Pause, Scroll Lock, Print Screen, Menu or F1–F24.",
        "«{key}» без Ctrl, Alt или Super — это обычная клавиша ввода: если её назначить, она перестанет печатать. Добавьте модификатор или выберите Pause, Scroll Lock, Print Screen, Menu или F1–F24.",
        "«{key}» senza Ctrl, Alt o Super è un normale tasto di scrittura: assegnarlo gli impedirebbe di scrivere. Aggiungi un modificatore o scegli Pause, Scroll Lock, Print Screen, Menu o F1–F24.",
    ),
    (
        "Change the layout of the word",
        "Сменить раскладку слова",
        "Cambia il layout della parola",
    ),
    (
        "Convert the selected text",
        "Переложить выделенный текст",
        "Converti il testo selezionato",
    ),
    (
        "Cycle the mode for the application",
        "Режим в приложении по кругу",
        "Cambia a turno la modalità per l'applicazione",
    ),
    (
        "Report a wrong correction",
        "Сообщить об ошибке исправления",
        "Segnala una correzione sbagliata",
    ),
    (
        "Turn the Apple layout on or off",
        "Включить или выключить раскладку Apple",
        "Attiva o disattiva il layout Apple",
    ),
    (
        "“{key}” is already in this action.",
        "«{key}» уже есть в этом действии.",
        "«{key}» è già presente in questa azione.",
    ),
    (
        "“{key}” is already assigned to the action “{action}” in this set.",
        "«{key}» уже назначено действию «{action}» в этом наборе.",
        "«{key}» è già assegnato all'azione «{action}» in questo set.",
    ),
    (
        "A shortcut is already being recorded.",
        "Запись сочетания уже идёт.",
        "Una combinazione è già in fase di registrazione.",
    ),
    (
        "The method takes no arguments.",
        "Метод не принимает аргументов.",
        "Il metodo non accetta argomenti.",
    ),
    (
        "Unknown set or action.",
        "Неизвестный набор или действие.",
        "Set o azione sconosciuti.",
    ),
    (
        "Could not parse “{text}”.",
        "Не удалось разобрать «{text}».",
        "Impossibile interpretare «{text}».",
    ),
    (
        "Unknown set.",
        "Неизвестный набор.",
        "Set sconosciuto.",
    ),
    (
        "Unknown action.",
        "Неизвестное действие.",
        "Azione sconosciuta.",
    ),
    (
        "Unknown choice of set.",
        "Неизвестный выбор набора.",
        "Scelta del set sconosciuta.",
    ),
    (
        "Shortcuts can be recorded only in keyboard grab mode: only then can the daemon swallow a key press.",
        "Записывать сочетания можно только в режиме захвата клавиатуры: только в нём демон может проглотить нажатие.",
        "Le combinazioni si possono registrare solo nella modalità di cattura della tastiera: solo allora il demone può trattenere la pressione di un tasto.",
    ),
    (
        "Could not save the settings: {error}",
        "Не удалось сохранить настройки: {error}",
        "Impossibile salvare le impostazioni: {error}",
    ),
    (
        "(unnamed)",
        "(без имени)",
        "(senza nome)",
    ),
    (
        "gsettings did not accept the new list of layouts",
        "gsettings не принял новый список раскладок",
        "gsettings non ha accettato il nuovo elenco dei layout",
    ),
    (
        "The Russian layout uses the “{variant}” variant; the Apple layout is not applied over it, so your choice is kept.",
        "Для русской раскладки выбран вариант «{variant}»: поверх него раскладка Apple не включается, чтобы не отменить ваш выбор.",
        "Il layout russo usa la variante «{variant}»: il layout Apple non viene applicato sopra di essa, così la tua scelta resta.",
    ),
    (
        "There is no Russian layout in the settings to apply the Apple variant to.",
        "В настройках нет русской раскладки, к которой применить вариант Apple.",
        "Nelle impostazioni non c'è un layout russo a cui applicare la variante Apple.",
    ),
    (
        "Choice not saved: {error}",
        "Выбор не сохранён: {error}",
        "Scelta non salvata: {error}",
    ),
    (
        "setxkbmap not found (package x11-xkb-utils): {detail}",
        "setxkbmap не найден (пакет x11-xkb-utils): {detail}",
        "setxkbmap non trovato (pacchetto x11-xkb-utils): {detail}",
    ),
    (
        "setxkbmap reported an error: {detail}",
        "setxkbmap ответил ошибкой: {detail}",
        "setxkbmap ha risposto con un errore: {detail}",
    ),
    (
        "setxkbmap did not say which layout is loaded",
        "setxkbmap не сказал, какая раскладка загружена",
        "setxkbmap non ha detto quale layout è caricato",
    ),
    (
        "{key} is already taken in GNOME: {what}.",
        "{key} уже занято в GNOME: {what}.",
        "{key} è già usato in GNOME: {what}.",
    ),
    (
        "A single press of Super opens the window overview in GNOME; PuLinTo will take it for itself, and the overview will open only with Super plus another key.",
        "Одиночное нажатие Super открывает в GNOME обзор окон; PuLinTo заберёт его себе, и обзор будет открываться только Super вместе с другой клавишей.",
        "La pressione singola di Super apre in GNOME la panoramica delle finestre; PuLinTo la prenderà per sé e la panoramica si aprirà solo con Super insieme a un altro tasto.",
    ),
    (
        "a custom shortcut",
        "своё сочетание",
        "una scorciatoia personale",
    ),
    (
        "The application policy is locked",
        "Политика приложений заблокирована",
        "Le regole delle applicazioni sono bloccate",
    ),
    (
        "Could not save the global state: {error}",
        "Не удалось сохранить глобальное состояние: {error}",
        "Impossibile salvare lo stato globale: {error}",
    ),
    (
        "all applications",
        "все приложения",
        "tutte le applicazioni",
    ),
    (
        "empty WM_CLASS",
        "пустой WM_CLASS",
        "WM_CLASS vuoto",
    ),
    (
        "gnome-shell is the lock screen and the system dialogs; PuLinTo stays off there",
        "gnome-shell — экран блокировки и системные диалоги, PuLinTo там не включается",
        "gnome-shell è la schermata di blocco e le finestre di dialogo di sistema, PuLinTo lì non viene attivato",
    ),
];

/// The text in the interface language; the English source itself when the
/// language is English or the string is not in the table.
pub fn tr(text: &'static str) -> &'static str {
    match language() {
        Lang::En => text,
        Lang::Ru => TABLE
            .iter()
            .find(|row| row.0 == text)
            .map_or(text, |row| row.1),
        Lang::It => TABLE
            .iter()
            .find(|row| row.0 == text)
            .map_or(text, |row| row.2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(vars: &[(&str, &str)]) -> Lang {
        detect(|name| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string())
        })
    }

    #[test]
    fn the_language_follows_the_locale_and_foreigners_get_english() {
        assert_eq!(lang(&[("LANG", "ru_RU.UTF-8")]), Lang::Ru);
        assert_eq!(lang(&[("LANG", "it_IT.UTF-8")]), Lang::It);
        assert_eq!(lang(&[("LANG", "en_US.UTF-8")]), Lang::En);
        assert_eq!(lang(&[("LANG", "de_DE.UTF-8")]), Lang::En);
        assert_eq!(lang(&[("LANG", "fr_FR@euro")]), Lang::En);
        // The override and the list come first; C and an empty locale count as none.
        assert_eq!(
            lang(&[("PULINTO_LANG", "it"), ("LANG", "ru_RU.UTF-8")]),
            Lang::It
        );
        assert_eq!(
            lang(&[("LANGUAGE", "it:en"), ("LANG", "ru_RU.UTF-8")]),
            Lang::It
        );
        assert_eq!(lang(&[("LC_ALL", "C"), ("LANG", "it_IT.UTF-8")]), Lang::It);
        assert_eq!(lang(&[]), Lang::En);
        assert_eq!(lang(&[("LANG", "C")]), Lang::En);
    }

    #[test]
    fn only_the_three_languages_are_a_choice() {
        assert_eq!(Lang::parse("ru"), Some(Lang::Ru));
        assert_eq!(Lang::parse(" it "), Some(Lang::It));
        assert_eq!(Lang::parse("auto"), None);
        assert_eq!(Lang::parse("de"), None);
        for lang in [Lang::Ru, Lang::En, Lang::It] {
            assert_eq!(Lang::parse(lang.code()), Some(lang));
        }
    }

    #[test]
    fn every_row_is_translated_and_keeps_its_placeholders() {
        let braces = |text: &str| -> Vec<String> {
            let mut found: Vec<String> = text
                .split('{')
                .skip(1)
                .filter_map(|s| s.split('}').next())
                .map(str::to_string)
                .collect();
            found.sort_unstable();
            found
        };
        let cyrillic = |text: &str| text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
        for (en, ru, it) in TABLE {
            assert!(!ru.is_empty() && !it.is_empty(), "{en}");
            assert_eq!(braces(en), braces(ru), "{en}");
            assert_eq!(braces(en), braces(it), "{en}");
            assert!(!cyrillic(en), "the source must be English: {en}");
            assert!(!cyrillic(it), "{it}");
        }
    }

    /// Every `tr("...")` in the tray source names a row of the table, so a
    /// misspelt string is a failing test and not an English line in a Russian menu.
    #[test]
    fn every_string_the_tray_asks_for_is_in_the_table() {
        for source in [
            include_str!("tray.rs"),
            include_str!("xwindow.rs"),
            include_str!("keyboard.rs"),
            include_str!("hotkeys.rs"),
            include_str!("dbus.rs"),
            include_str!("gnome_keys.rs"),
            include_str!("main.rs"),
        ] {
            let mut rest = source;
            while let Some(at) = rest.find("tr(\"") {
                // `tr(` of another identifier (`str(`, `entry(`) is not ours.
                let preceded = rest[..at]
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                rest = &rest[at + 4..];
                if preceded {
                    continue;
                }
                let mut text = String::new();
                let mut chars = rest.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => text.push(chars.next().unwrap_or('\\')),
                        '"' => break,
                        other => text.push(other),
                    }
                }
                assert!(
                    TABLE.iter().any(|row| row.0 == text),
                    "not in the table: {text:?}"
                );
            }
        }
    }
}
