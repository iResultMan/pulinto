//! Stage 0 of the cascade: words that must never
//! be touched, checked before anything language-related. No `regex` crate
//! here — `pulinto-core` carries no external dependencies, so these are
//! plain character scans.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Password,
    Url,
    Code,
    MixedAlphabet,
    EmbeddedDigits,
}

/// Stage 0 of the cascade: words that must never be touched, whatever the circumstances.
pub fn check(word: &str) -> Option<StopReason> {
    check_with(word, false)
}

/// The same, told whether some other layout reads the token as letters alone
/// — then a lone backslash is a letter (ё on the Apple Russian layout), not a
/// path. Every other rule applies as it is.
pub fn check_with(word: &str, letters_elsewhere: bool) -> Option<StopReason> {
    if word.is_empty() {
        return None;
    }

    // Url and Code first: both can look like a mixed-case-plus-punctuation
    // "password" (`git@host:x`), so they must win the race.
    if looks_like_url_unless_letters(word, letters_elsewhere) {
        return Some(StopReason::Url);
    }
    if looks_like_code(word) {
        return Some(StopReason::Code);
    }
    if looks_like_password(word) {
        return Some(StopReason::Password);
    }
    // Abbreviations are deliberately NOT vetoed here.
    //
    // The rule used to be "every letter upper-case → never touch", which kept
    // real abbreviations safe but also froze mistyped ones: `NASA` typed on the
    // Russian layout stayed as `ТФЫФ`. Measurement showed the veto is redundant
    // — the cascade already leaves `СССР`, `ГОСТ`, `ФСБ`, `МИД`, `ВУЗ`, `NASA`,
    // `HTTP`, `FBI`, `USA` alone through the dictionary and n-gram stages. It
    // only ever blocked corrections that should have happened.
    if has_mixed_alphabet(word) {
        return Some(StopReason::MixedAlphabet);
    }
    if has_embedded_digit(word) {
        return Some(StopReason::EmbeddedDigits);
    }
    None
}

/// A backslash alone makes a path — unless, on the Russian Apple layout, it
/// is the letter ё and the whole token reads as letters there (`dc\\` is
/// `всё`). Any other evidence of a URL or a path counts regardless.
fn looks_like_url_unless_letters(word: &str, letters_elsewhere: bool) -> bool {
    word.contains("://")
        || word.contains('@')
        || word.starts_with("www.")
        || word.contains('/')
        || (word.contains('\\') && !letters_elsewhere)
        || looks_like_domain(word)
}

/// What follows the last full stop of a host name or a file name: top-level
/// domains (`yandex.ru`, `vk.com`, `сайт.рф`) and file extensions
/// (`readme.txt`, `index.html`). Such a token is a name, not a word in either
/// language. Short hosts are the danger — `vk.com` read on the Russian layout
/// is `млюсщь`, and the bigram stage would move it. The engine uses the same
/// list to tell `cfqn.ru` (a Russian word in front of a domain: split there)
/// from `yfuhtdf.ncz` (`нагреваются`, the `ю` key inside one word): a finite
/// list, not a length, is what separates the two.
pub const DOT_SUFFIXES: &[&str] = &[
    // domains
    "ru", "com", "рф", "org", "net", "io", "su", "me", "dev", "app", "info", "ua", "by", "kz", "de",
    "uk", "us", "co", "edu", "gov", "tv", "ai", "xyz", "pro", "biz",
    // file extensions
    "md", "py", "js", "ts", "rs", "go", "rb", "sh", "txt", "pdf", "doc", "docx", "xls", "xlsx", "csv",
    "json", "xml", "yml", "yaml", "toml", "ini", "log", "html", "htm", "css", "png", "jpg", "jpeg",
    "gif", "svg", "mp3", "mp4", "wav", "zip", "tar", "gz", "7z", "exe", "dll", "so", "deb", "rpm", "iso",
];

/// Is this a known domain or file extension (without the dot, any case)?
pub fn is_dot_suffix(text: &str) -> bool {
    DOT_SUFFIXES.contains(&text.to_lowercase().as_str())
}

fn looks_like_domain(word: &str) -> bool {
    let lower = word.to_lowercase();
    lower.rsplit_once('.').is_some_and(|(host, suffix)| {
        !host.is_empty()
            && is_dot_suffix(suffix)
            && host.chars().all(|ch| ch.is_alphanumeric() || ch == '.' || ch == '-')
    })
}

fn looks_like_code(word: &str) -> bool {
    let starts_with_marker = word.starts_with('@')
        || word.starts_with('#')
        || word.starts_with('$')
        || word.starts_with("--")
        || word.starts_with('_');
    if starts_with_marker || word.contains("::") || word.contains("()") {
        return true;
    }
    // `_` inside the word (not merely a leading marker, already handled above).
    let chars: Vec<char> = word.chars().collect();
    chars[1..].contains(&'_')
}

fn looks_like_password(word: &str) -> bool {
    if word.chars().count() <= 6 {
        return false;
    }
    let has_upper = word.chars().any(|ch| ch.is_uppercase());
    let has_lower = word.chars().any(|ch| ch.is_lowercase());
    // A digit is required, not merely "a digit or any special character".
    // Punctuation cannot stand in for it: on the Russian layout `,` `.` `-` are
    // ordinary characters of ordinary words, so accepting them turned
    // `Xtkz,bycrjq` (Челябинской) and `Cfy-Lbtuj` (Сан-Диего) into "passwords"
    // and blocked their correction. A password needs mixed case *and* digits.
    let has_digit = word.chars().any(|ch| ch.is_ascii_digit());
    has_upper && has_lower && has_digit
}

fn has_mixed_alphabet(word: &str) -> bool {
    let mut has_cyrillic = false;
    let mut has_latin = false;
    for ch in word.chars() {
        if ch.is_alphabetic() {
            if is_cyrillic(ch) {
                has_cyrillic = true;
            } else if ch.is_ascii_alphabetic() {
                has_latin = true;
            }
        }
    }
    has_cyrillic && has_latin
}

fn is_cyrillic(ch: char) -> bool {
    matches!(ch, '\u{0400}'..='\u{04FF}')
}

/// A digit that sits inside the word rather than trailing it — trailing
/// digits are stripped by the cascade's preprocessing before stage 0 is
/// ever reached, so anything left here is an embedded identifier like
/// `x86_64` or `a1b`.
fn has_embedded_digit(word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    let last_digit = chars.iter().rposition(|ch| ch.is_ascii_digit());
    match last_digit {
        Some(pos) => pos + 1 != chars.len(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_needs_length_mixed_case_and_digit_or_special() {
        assert_eq!(check("Qw3rty!x"), Some(StopReason::Password));
        assert_eq!(check("Pass1"), None); // length <= 6
        assert_eq!(check("Password"), None); // no digit/special
    }

    #[test]
    fn url_wins_before_password_or_code() {
        assert_eq!(check("http://x.ru"), Some(StopReason::Url));
        assert_eq!(check("www.example.com"), Some(StopReason::Url));
        assert_eq!(check("user@example.com"), Some(StopReason::Url));
        assert_eq!(check("git@host:x"), Some(StopReason::Url));
    }

    #[test]
    fn a_bare_domain_is_a_url() {
        assert_eq!(check("yandex.ru"), Some(StopReason::Url));
        assert_eq!(check("vk.com"), Some(StopReason::Url));
        assert_eq!(check("mail.yandex.ru"), Some(StopReason::Url));
        assert_eq!(check("сайт.рф"), Some(StopReason::Url));
        assert_eq!(check("cfqn.ru"), Some(StopReason::Url), "whole token; the engine splits it first");
        assert_eq!(check("readme.txt"), Some(StopReason::Url), "a file name is a name too");
        assert_eq!(check("index.HTML"), Some(StopReason::Url));
        // Not names: no host, an unknown suffix (`нагреваются` typed on `us`).
        assert_eq!(check(".ru"), None);
        assert_eq!(check("yfuhtdf.ncz"), None);
        assert_eq!(check("меню"), None);
        assert!(is_dot_suffix("PY") && !is_dot_suffix("ncz"));
    }

    #[test]
    fn code_markers() {
        assert_eq!(check("std::vector"), Some(StopReason::Code));
        // Leading `@` is ambiguous with the Url rule's "contains @" (e-mail
        // marker); Url wins for that character specifically, so this case
        // uses a marker (`#`) that Url does not also claim.
        assert_eq!(check("#define"), Some(StopReason::Code));
        assert_eq!(check("--flag"), Some(StopReason::Code));
        assert_eq!(check("_private"), Some(StopReason::Code));
        assert_eq!(check("foo_bar"), Some(StopReason::Code));
        assert_eq!(check("main()"), Some(StopReason::Code));
    }

    #[test]
    fn abbreviations_are_left_to_the_cascade() {
        // Deliberately no veto: an all-caps word gets judged like any other.
        // Real abbreviations survive on dictionary and n-gram evidence, while
        // mistyped ones — `ТФЫФ` being `NASA` on the Russian layout — become
        // correctable, which the blanket rule prevented.
        assert_eq!(check("СССР"), None);
        assert_eq!(check("NASA"), None);
        assert_eq!(check("ТФЫФ"), None);
        assert_eq!(check("Hello"), None);
    }

    #[test]
    fn mixed_alphabet_is_flagged() {
        assert_eq!(check("приvet"), Some(StopReason::MixedAlphabet));
    }

    #[test]
    fn embedded_digits_flagged_when_not_trailing() {
        // `x86_64` hits the Code rule first (underscore inside the word) —
        // both readings are acceptable ("Code" or "EmbeddedDigits"), but Code
        // wins under this ordering since it runs earlier.
        assert_eq!(check("x86_64"), Some(StopReason::Code));
        assert_eq!(check("a1b"), Some(StopReason::EmbeddedDigits));
        assert_eq!(check("word123"), None); // trailing digits, stripped upstream in real cascade
    }

    #[test]
    fn plain_words_pass_through() {
        assert_eq!(check("привет"), None);
        assert_eq!(check("hello"), None);
    }
}
