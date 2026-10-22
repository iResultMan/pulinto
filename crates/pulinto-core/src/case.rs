//! Case changing and the typo corrections Punto Switcher performs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    Lower,
    Upper,
    /// Every Word Capitalised.
    Title,
    /// First letter of the text only.
    Sentence,
}

pub fn apply(text: &str, mode: CaseMode) -> String {
    match mode {
        CaseMode::Lower => text.to_lowercase(),
        CaseMode::Upper => text.to_uppercase(),
        CaseMode::Title => map_words(text, capitalise),
        CaseMode::Sentence => capitalise(&text.to_lowercase()),
    }
}

/// What Alt+Break does: lower → Title → UPPER → lower.
pub fn cycle(text: &str) -> String {
    let next = match observe(text) {
        Some(CaseMode::Lower) => CaseMode::Title,
        Some(CaseMode::Title) => CaseMode::Upper,
        Some(CaseMode::Upper) => CaseMode::Lower,
        // Mixed case has no obvious successor; normalising to lower gives the
        // user a predictable starting point for the next press.
        _ => CaseMode::Lower,
    };
    apply(text, next)
}

/// Which of the three cycle states the text is currently in, if any.
pub fn observe(text: &str) -> Option<CaseMode> {
    let has_letters = text.chars().any(|ch| ch.is_alphabetic());
    if !has_letters {
        return None;
    }
    if text == text.to_lowercase() {
        return Some(CaseMode::Lower);
    }
    if text == text.to_uppercase() {
        return Some(CaseMode::Upper);
    }
    if text == apply(text, CaseMode::Title) {
        return Some(CaseMode::Title);
    }
    None
}

/// `ПРивет` → `Привет`. Returns `None` when the word does not show the
/// pattern, so callers can tell "corrected" from "unchanged".
pub fn fix_two_capitals(word: &str) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    if chars.len() < 3 {
        return None;
    }
    let [first, second, third] = [chars[0], chars[1], chars[2]];
    if first.is_uppercase() && second.is_uppercase() && third.is_lowercase() {
        let mut fixed = String::with_capacity(word.len());
        fixed.push(first);
        fixed.extend(second.to_lowercase());
        fixed.extend(chars[2..].iter());
        return Some(fixed);
    }
    None
}

/// `пРИВЕТ` → `Привет`: CapsLock was on while Shift was used for the first
/// letter. Inverting every letter recovers the intent.
pub fn fix_incidental_caps(word: &str) -> Option<String> {
    let mut chars = word.chars();
    let first = chars.next()?;
    if !first.is_lowercase() {
        return None;
    }
    let rest: Vec<char> = chars.collect();
    if rest.is_empty() || !rest.iter().any(|ch| ch.is_alphabetic()) {
        return None;
    }
    if !rest
        .iter()
        .filter(|ch| ch.is_alphabetic())
        .all(|ch| ch.is_uppercase())
    {
        return None;
    }
    let mut fixed = String::with_capacity(word.len());
    fixed.extend(first.to_uppercase());
    for ch in rest {
        fixed.extend(ch.to_lowercase());
    }
    Some(fixed)
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn map_words(text: &str, transform: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '\'' || ch == '-' {
            word.push(ch);
        } else {
            if !word.is_empty() {
                out.push_str(&transform(&word.to_lowercase()));
                word.clear();
            }
            out.push(ch);
        }
    }
    if !word.is_empty() {
        out.push_str(&transform(&word.to_lowercase()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_each_mode() {
        assert_eq!(apply("привет мир", CaseMode::Upper), "ПРИВЕТ МИР");
        assert_eq!(apply("ПРИВЕТ МИР", CaseMode::Lower), "привет мир");
        assert_eq!(apply("привет мир", CaseMode::Title), "Привет Мир");
        assert_eq!(apply("ПРИВЕТ МИР", CaseMode::Sentence), "Привет мир");
    }

    #[test]
    fn cycles_lower_title_upper() {
        let start = "привет мир";
        let title = cycle(start);
        assert_eq!(title, "Привет Мир");
        let upper = cycle(&title);
        assert_eq!(upper, "ПРИВЕТ МИР");
        assert_eq!(cycle(&upper), start);
    }

    #[test]
    fn corrects_two_leading_capitals() {
        assert_eq!(fix_two_capitals("ПРивет").as_deref(), Some("Привет"));
        assert_eq!(fix_two_capitals("HEllo").as_deref(), Some("Hello"));
        assert_eq!(fix_two_capitals("Привет"), None);
        assert_eq!(fix_two_capitals("ПРИВЕТ"), None);
        assert_eq!(fix_two_capitals("ПР"), None);
    }

    #[test]
    fn corrects_incidental_caps_lock() {
        assert_eq!(fix_incidental_caps("пРИВЕТ").as_deref(), Some("Привет"));
        assert_eq!(fix_incidental_caps("hELLO").as_deref(), Some("Hello"));
        assert_eq!(fix_incidental_caps("привет"), None);
        assert_eq!(fix_incidental_caps("Привет"), None);
    }
}
