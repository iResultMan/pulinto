//! Keyboard layouts as position tables.
//!
//! A layout is two strings of equal length: the characters produced by the 47
//! keys of the alphanumeric block, unshifted and shifted. Converting text
//! between layouts is then a position lookup — exactly how Punto Switcher does
//! it, and why the conversion is exact rather than heuristic.
//!
//! The tables are data: each language pack (`data/languages/<id>/language.conf`)
//! carries the keys of its layout and of the variants that differ, such as Apple's.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use crate::pack;

/// Keys in the alphanumeric block: 13 + 13 + 11 + 10.
pub const KEY_COUNT: usize = 47;

/// Which level of a key a character sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Unshifted,
    Shifted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// XKB-style identifier, matching the directory names of the language
    /// data (`ru`, `us`, `ua`, ...).
    pub id: &'static str,
    /// XKB variant of the layout: empty for the standard one, `mac` for the
    /// Apple arrangement. The identifier stays the language's, so dictionaries
    /// and X groups keep finding it; only the position table differs.
    pub variant: &'static str,
    pub name: &'static str,
    lower: &'static str,
    upper: &'static str,
}

impl Layout {
    pub(crate) fn from_pack(
        id: &'static str,
        variant: &'static str,
        name: &'static str,
        lower: &'static str,
        upper: &'static str,
    ) -> Layout {
        Layout { id, variant, name, lower, upper }
    }
}

/// Which variant is in effect for each layout, as the keyboard is configured
/// right now. Set once at start-up from the live XKB state: it is a fact about
/// the machine, like the keymap itself, not something a caller chooses per call.
static ACTIVE: RwLock<Vec<(&'static str, &'static str)>> = RwLock::new(Vec::new());

fn active_variant(id: &str) -> &'static str {
    ACTIVE
        .read()
        .ok()
        .and_then(|active| {
            active
                .iter()
                .find(|(active_id, _)| *active_id == id)
                .map(|(_, variant)| *variant)
        })
        .unwrap_or("")
}

/// Tell the tables which XKB variant each layout runs with. Pairs naming a
/// variant we have no table for are ignored — the standard table is the honest
/// fallback, and for `us(mac)` it is also the exact one.
pub fn use_variants<'a>(variants: impl IntoIterator<Item = (&'a str, &'a str)>) {
    let chosen: Vec<(&'static str, &'static str)> = variants
        .into_iter()
        .filter_map(|(id, variant)| {
            pack::layouts()
                .iter()
                .find(|layout| layout.id == id && layout.variant == variant && !variant.is_empty())
                .map(|layout| (layout.id, layout.variant))
        })
        .collect();
    if let Ok(mut active) = ACTIVE.write() {
        *active = chosen;
    }
}

/// Character → (key position, level), built once per layout on first use.
type Index = HashMap<char, (usize, Level)>;

fn indexes() -> &'static Vec<Index> {
    static INDEXES: OnceLock<Vec<Index>> = OnceLock::new();
    INDEXES.get_or_init(|| {
        pack::layouts()
            .iter()
            .map(|layout| {
                let mut index = Index::with_capacity(KEY_COUNT * 2);
                for (pos, ch) in layout.lower.chars().enumerate() {
                    index.entry(ch).or_insert((pos, Level::Unshifted));
                }
                for (pos, ch) in layout.upper.chars().enumerate() {
                    index.entry(ch).or_insert((pos, Level::Shifted));
                }
                index
            })
            .collect()
    })
}

impl Layout {
    /// Look up a layout by its identifier, in the variant the keyboard runs.
    pub fn by_id(id: &str) -> Option<&'static Layout> {
        Self::lookup(id, active_variant(id))
    }

    /// A specific variant of a layout, falling back to the standard table when
    /// there is no table for it.
    pub fn lookup(id: &str, variant: &str) -> Option<&'static Layout> {
        pack::layouts()
            .iter()
            .find(|layout| layout.id == id && layout.variant == variant)
            .or_else(|| {
                pack::layouts()
                    .iter()
                    .find(|layout| layout.id == id && layout.variant.is_empty())
            })
    }

    pub fn all() -> &'static [Layout] {
        pack::layouts()
    }

    fn ordinal(&self) -> usize {
        pack::layouts()
            .iter()
            .position(|layout| layout.id == self.id && layout.variant == self.variant)
            .expect("layout must come from a language pack")
    }

    fn index(&self) -> &'static Index {
        &indexes()[self.ordinal()]
    }

    /// Where this character sits on the keyboard, if it does at all.
    pub fn position_of(&self, ch: char) -> Option<(usize, Level)> {
        self.index().get(&ch).copied()
    }

    /// The character produced by a key at the given level.
    pub fn char_at(&self, position: usize, level: Level) -> Option<char> {
        let table = match level {
            Level::Unshifted => self.lower,
            Level::Shifted => self.upper,
        };
        table.chars().nth(position)
    }

    /// Whether the character can be typed on this layout at all.
    pub fn contains(&self, ch: char) -> bool {
        self.index().contains_key(&ch)
    }

    /// Share of alphabetic characters in `text` that belong to this layout.
    /// Used to guess which layout a piece of text was typed on.
    pub fn alphabetic_coverage(&self, text: &str) -> f32 {
        let mut total = 0usize;
        let mut covered = 0usize;
        for ch in text.chars().filter(|ch| ch.is_alphabetic()) {
            total += 1;
            if self.contains(ch) {
                covered += 1;
            }
        }
        if total == 0 {
            0.0
        } else {
            covered as f32 / total as f32
        }
    }
}

/// Retype `text` as if the same keys had been pressed under `to`.
///
/// Characters with no position in `from` (digits from the numpad, emoji,
/// anything exotic) pass through untouched — dropping them would corrupt text
/// that is only partly mistyped.
pub fn convert(text: &str, from: &Layout, to: &Layout) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match from
            .position_of(ch)
            .and_then(|(position, level)| to.char_at(position, level))
        {
            Some(mapped) => out.push(mapped),
            None => out.push(ch),
        }
    }
    out
}

/// Guess which layout the text was typed on, by alphabetic coverage.
/// Returns `None` when nothing alphabetic is present to judge by.
pub fn guess_layout(text: &str) -> Option<&'static Layout> {
    let mut best: Option<(&'static Layout, f32)> = None;
    for layout in pack::layouts()
        .iter()
        .filter(|layout| layout.variant == active_variant(layout.id))
    {
        let coverage = layout.alphabetic_coverage(text);
        if coverage > 0.0 && best.is_none_or(|(_, seen)| coverage > seen) {
            best = Some((layout, coverage));
        }
    }
    best.map(|(layout, _)| layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn us() -> &'static Layout {
        Layout::by_id("us").unwrap()
    }

    fn ru() -> &'static Layout {
        Layout::by_id("ru").unwrap()
    }

    #[test]
    fn tables_have_one_character_per_key() {
        for layout in Layout::all() {
            assert_eq!(layout.lower.chars().count(), KEY_COUNT, "{}", layout.id);
            assert_eq!(layout.upper.chars().count(), KEY_COUNT, "{}", layout.id);
        }
    }

    #[test]
    fn converts_the_canonical_examples() {
        assert_eq!(convert("ghbdtn", us(), ru()), "привет");
        assert_eq!(convert("привет", ru(), us()), "ghbdtn");
        assert_eq!(convert("Ghbdtn", us(), ru()), "Привет");
        assert_eq!(convert("руддщ", ru(), us()), "hello");
    }

    #[test]
    fn round_trips_through_both_layouts() {
        for sample in ["hello, world!", "a1b2c3"] {
            let there = convert(sample, us(), ru());
            assert_eq!(convert(&there, ru(), us()), sample);
        }
        for sample in ["привет, мир!", "Ой-ой"] {
            let there = convert(sample, ru(), us());
            assert_eq!(convert(&there, us(), ru()), sample);
        }
    }

    #[test]
    fn leaves_unmappable_characters_alone() {
        assert_eq!(convert("héllo—", us(), ru()), "рéддщ—");
    }

    fn ru_mac() -> &'static Layout {
        Layout::lookup("ru", "mac").unwrap()
    }

    #[test]
    fn the_apple_table_moves_exactly_the_keys_xkb_moves() {
        // Where `ru(mac)` differs from `ru` in the two levels we model, read off
        // the live keymap: eight keys, nothing else.
        let plain = ru();
        let mac = ru_mac();
        let mut moved = Vec::new();
        for position in 0..KEY_COUNT {
            for level in [Level::Unshifted, Level::Shifted] {
                if plain.char_at(position, level) != mac.char_at(position, level) {
                    moved.push((position, level));
                }
            }
        }
        let keys: std::collections::BTreeSet<usize> =
            moved.iter().map(|(position, _)| *position).collect();
        assert_eq!(keys.into_iter().collect::<Vec<_>>(), vec![0, 4, 5, 6, 7, 8, 25, 46]);
    }

    #[test]
    fn apple_punctuation_sits_where_a_mac_user_expects_it() {
        let mac = ru_mac();
        // Shift+6 is a comma, Shift+7 a full stop, and ё lives on the `\` key.
        assert_eq!(mac.char_at(6, Level::Shifted), Some(','));
        assert_eq!(mac.char_at(7, Level::Shifted), Some('.'));
        assert_eq!(mac.char_at(25, Level::Unshifted), Some('ё'));
        assert_eq!(mac.char_at(46, Level::Unshifted), Some('/'));
    }

    #[test]
    fn a_word_typed_on_the_apple_arrangement_converts_with_its_own_punctuation() {
        // `ghbdtn&` is "привет" and Shift+7: a full stop on the Apple layout,
        // a question mark on the standard one.
        assert_eq!(convert("ghbdtn&", us(), ru_mac()), "привет.");
        assert_eq!(convert("ghbdtn&", us(), ru()), "привет?");
        assert_eq!(convert("привет.", ru_mac(), us()), "ghbdtn&");
    }

    #[test]
    fn the_standard_table_is_the_default_and_the_fallback() {
        assert_eq!(Layout::by_id("ru").unwrap().variant, "");
        // No `us(mac)` table is needed: it is the same two levels.
        assert_eq!(Layout::lookup("us", "mac").unwrap().variant, "");
        assert_eq!(Layout::lookup("ru", "no-such-variant").unwrap().variant, "");
    }

    #[test]
    fn guesses_the_layout_from_the_alphabet() {
        assert_eq!(guess_layout("привет").map(|l| l.id), Some("ru"));
        assert_eq!(guess_layout("hello").map(|l| l.id), Some("us"));
        assert_eq!(guess_layout("12345"), None);
    }
}
