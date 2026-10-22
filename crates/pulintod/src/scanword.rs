//! Word buffer holding physical keys instead of characters.
//!
//! The cascade needs the word as it appears on screen, but the replacement
//! needs the keys that produced it. Storing characters means reconstructing the
//! keys later, and that reconstruction can disagree with the layout actually in
//! effect. Storing keys means the text is derived (cheap, always consistent)
//! and the replacement is just "send these again".

use pulinto_core::layout::{Layout, Level};
use pulinto_data::profile::{self, Profile};
use pulinto_input::injector::position_of_keycode;
use pulinto_layout::LayoutController;

/// One keystroke as the kernel reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke {
    pub keycode: u16,
    pub shift: bool,
    /// Key that draws punctuation on the layout in effect and a letter on
    /// another one, taken into the word on the strength of the dictionary.
    ///
    /// Remembered because that strength is limited: a short reading is almost
    /// always the start of *something*, so `надо` plus a full stop looked like
    /// the beginning of `надоют` and swallowed the word after it. The word
    /// boundary can settle it — by then the token is finished — and to settle it
    /// there we have to know where the doubtful keys are.
    pub ambiguous: bool,
}

#[derive(Debug, Default)]
pub struct ScanWord {
    strokes: Vec<Stroke>,
    /// Set once a conversion has been applied to this word. A word is corrected
    /// at most once: repeated corrections are how switchers end up ping-ponging
    /// a word between layouts.
    converted: bool,
    /// Whose reading the word on the screen is in now — a profile index.
    /// Auto-replacement and the manual cycle move it; `None` — the word as
    /// typed. This state is what makes the Alt cycle deterministic: the next
    /// step is computed from what is on the screen, not from a guess by the dictionary.
    shown: Option<usize>,
    /// Whose reading the word was typed in before the first manual Alt step.
    /// Without it, after `word → Alt → space` the original reading cannot be
    /// told from the shown one, and the cycle cannot be taught correctly after the space.
    original: Option<usize>,
}

impl ScanWord {
    pub fn push(&mut self, keycode: u16, shift: bool) {
        self.push_key(keycode, shift, false);
    }

    /// Add a key that only the dictionary let into the word.
    pub fn push_ambiguous(&mut self, keycode: u16, shift: bool) {
        self.push_key(keycode, shift, true);
    }

    fn push_key(&mut self, keycode: u16, shift: bool, ambiguous: bool) {
        // Only keys of the alphanumeric block can form a word; the rest
        // (function keys, media keys) are not part of what we would retype.
        if position_of_keycode(keycode).is_some() {
            self.strokes.push(Stroke {
                keycode,
                shift,
                ambiguous,
            });
        }
    }

    /// Positions of the doubtful keys, latest first.
    ///
    /// Latest first because that is the order to try them in: the token is cut
    /// at the last doubtful key before the earlier ones, which keeps the longest
    /// possible first word.
    pub fn ambiguous_positions(&self) -> Vec<usize> {
        let mut positions: Vec<usize> = self
            .strokes
            .iter()
            .enumerate()
            .filter(|(_, stroke)| stroke.ambiguous)
            .map(|(index, _)| index)
            .collect();
        positions.reverse();
        positions
    }

    /// The word as it would be without everything from `position` onwards.
    pub fn text_before(&self, position: usize, layout: &Layout) -> String {
        Self::render(&self.strokes[..position.min(self.strokes.len())], layout)
    }

    /// The reading of saved keys `(code, shift)` under a given layout.
    /// The last-word cycle needs it: `Fix` stores keys as tuples.
    pub fn render_keys(keys: &[(u16, bool)], layout: &Layout) -> String {
        keys.iter()
            .filter_map(|&(keycode, shift)| {
                let position = position_of_keycode(keycode)?;
                let level = if shift {
                    Level::Shifted
                } else {
                    Level::Unshifted
                };
                layout.char_at(position, level)
            })
            .collect()
    }

    fn render(strokes: &[Stroke], layout: &Layout) -> String {
        strokes
            .iter()
            .filter_map(|stroke| {
                let position = position_of_keycode(stroke.keycode)?;
                let level = if stroke.shift {
                    Level::Shifted
                } else {
                    Level::Unshifted
                };
                layout.char_at(position, level)
            })
            .collect()
    }

    pub fn pop(&mut self) {
        self.strokes.pop();
        if self.strokes.is_empty() {
            self.converted = false;
            self.shown = None;
            self.original = None;
        }
    }

    pub fn clear(&mut self) {
        self.strokes.clear();
        self.converted = false;
        self.shown = None;
        self.original = None;
    }

    pub fn shown_profile(&self) -> Option<usize> {
        self.shown
    }

    pub fn set_shown_from(&mut self, from: usize, to: usize) {
        if self.original.is_none() {
            self.original = Some(from);
        }
        self.shown = Some(to);
    }

    pub fn original_profile(&self) -> Option<usize> {
        self.original
    }

    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.strokes.len()
    }

    pub fn already_converted(&self) -> bool {
        self.converted
    }

    pub fn mark_converted(&mut self) {
        self.converted = true;
    }

    /// The word as it currently appears, under `layout`.
    pub fn text(&self, layout: &Layout) -> String {
        Self::render(&self.strokes, layout)
    }

    /// Keys to replay, in order.
    pub fn replay_keys(&self) -> Vec<(u16, bool)> {
        self.strokes
            .iter()
            .map(|stroke| (stroke.keycode, stroke.shift))
            .collect()
    }
}

/// What a key is to the word being typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRole {
    /// A letter or digit on the layout in effect. No question about it.
    Letter,
    /// Punctuation here, a letter on some other loaded layout — the `[ ] ; ' ,
    /// . \`` keys, which are `х ъ ж э б ю ё`. Carries the character it draws
    /// right now, because deciding which of the two it is takes the dictionary.
    Ambiguous(char),
    /// Punctuation on every loaded layout: it ends the word.
    Separator,
    /// Not on the alphanumeric block at all — a function key, a modifier.
    Other,
}

/// Sort a key into one of those four.
///
/// Splitting `Ambiguous` out of `Letter`/`Separator` is what the two failed
/// attempts at this were missing. Treating those keys as separators cuts
/// Russian words at their own letters; treating them as letters glues two words
/// across the stop between them. They are genuinely undecidable at this level,
/// and the caller has to ask the dictionary — see
/// [`pulinto_core::cascade::punctuation_continues_word`].
pub fn classify<'a>(
    current: &Layout,
    others: impl IntoIterator<Item = &'a Layout>,
    keycode: u16,
    shift: bool,
) -> KeyRole {
    let Some(position) = position_of_keycode(keycode) else {
        return KeyRole::Other;
    };
    let level = if shift {
        Level::Shifted
    } else {
        Level::Unshifted
    };
    let Some(here) = current.char_at(position, level) else {
        return KeyRole::Other;
    };
    if here.is_alphanumeric() {
        return KeyRole::Letter;
    }
    let elsewhere = others.into_iter().any(|layout| {
        layout
            .char_at(position, level)
            .is_some_and(char::is_alphabetic)
    });
    if elsewhere {
        KeyRole::Ambiguous(here)
    } else {
        KeyRole::Separator
    }
}

/// What to do with a word that contains doubtful keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordPlan {
    /// One word, doubtful keys and all: the whole token is a word somebody
    /// knows, so those keys were letters.
    Whole,
    /// The key at this position was punctuation after all, and the token is two
    /// words with a stop between them.
    Split(usize),
}

/// Is `code` (with `shift`) the full stop of an initial or an abbreviation
/// typed on the wrong layout: the key draws `.` on one of the `others` while
/// drawing something else here, and the word so far reads there as one
/// capital letter (`Е.`) or as an abbreviation from the list (`т.`, `стр.`,
/// `млрд.`)? Any other short word (`лет.`) keeps the key as its separator, so
/// the word is judged on its own and the stop is retyped — on the plain
/// Russian layout the key is `/`, and a token `ktn/` would otherwise be a URL
/// to the stop rules.
pub fn ends_an_initial<'a>(
    word: &ScanWord,
    here: &Layout,
    others: impl Iterator<Item = &'a Layout>,
    code: u16,
    shift: bool,
) -> bool {
    if word.strokes.is_empty() || word.strokes.len() > 4 {
        return false;
    }
    let level = if shift {
        Level::Shifted
    } else {
        Level::Unshifted
    };
    let Some(position) = pulinto_input::injector::position_of_keycode(code) else {
        return false;
    };
    if here.char_at(position, level) == Some('.') {
        return false;
    }
    let keys = word.replay_keys();
    others
        .into_iter()
        .filter(|other| other.char_at(position, level) == Some('.'))
        .any(|other| {
            let reading = ScanWord::render_keys(&keys, other);
            if reading.chars().count() != keys.len() || !reading.chars().all(char::is_alphabetic) {
                return false;
            }
            let initial = keys.len() == 1 && reading.chars().next().is_some_and(char::is_uppercase);
            let abbreviation = pulinto_core::pack::language(other.id)
                .is_some_and(|language| language.is_dotted(&reading.to_lowercase()));
            initial || abbreviation
        })
}

/// Settle the doubtful keys now that the token is finished.
///
/// Asking at the key itself cannot work, and the rig showed why: at that moment
/// only a prefix exists, and a short prefix is almost always the start of
/// something. `надо` plus a full stop reads as `надою`, which begins `надоют`,
/// so the stop was taken for a letter and the next word was glued on —
/// `надоюпредставили`. `ст` plus a stop begins `стюардесса`. The dictionary was
/// answering honestly; the question was wrong.
///
/// By the boundary the question can be put properly: is the **whole** token a
/// word? `изъятия` is, so its `]` was the letter `ъ`. `надоюпредставили` is not,
/// so its `.` was a full stop. No guessing about how a word might continue.
pub fn plan_word<P: pulinto_core::cascade::LanguageProfile>(
    word: &ScanWord,
    profiles: &[&P],
    current: usize,
    context_langs: &[&str],
) -> WordPlan {
    let positions = word.ambiguous_positions();
    if positions.is_empty() {
        return WordPlan::Whole;
    }
    let layout = profiles[current].layout();

    // "Somebody recognised this as a word" — and that is not the dictionary
    // stage alone. Words of one or two letters never reach it: they are settled
    // by `ShortWord`, which is the stage carrying our whole advantage over
    // other switchers. Demanding `Dictionary` made `не` unrecognisable, so `не.создал`
    // found no place to cut and went through as `неюсоздал` — the very defect
    // this planner exists to stop. Measured: it fired once in a 120-word run.
    let recognised = |text: &str| {
        use pulinto_core::cascade::Stage;
        !text.is_empty()
            && matches!(
                pulinto_core::cascade::decide(text, profiles, current, context_langs).stage,
                Stage::Dictionary | Stage::ShortWord | Stage::Rule
            )
    };

    if recognised(&word.text(layout)) {
        return WordPlan::Whole;
    }
    for position in positions {
        if recognised(&word.text_before(position, layout)) {
            return WordPlan::Split(position);
        }
    }
    WordPlan::Whole
}

/// Which profile the user is typing on — the one thing every decision hangs
/// from, since it is what turns scan codes into the word the cascade judges.
///
/// Resolved by layout **name**, never by group index. A group is a slot in the
/// keymap the server holds at this instant, and IBus rebuilds that keymap on
/// every engine switch with the layout it just activated in group 0 — so group
/// 0 is `ru` one moment and `us` the next. Indexing our profile list by the
/// group number is what made the daemon read Russian as Latin and "correct" it.
///
/// When the controller cannot name the layout (Wayland, no `_XKB_RULES_NAMES`)
/// we fall back to guessing from the text, which is weak but at least does not
/// assert something false.
pub fn current_profile(
    layout: &dyn LayoutController,
    profiles: &[Profile],
    word: &ScanWord,
) -> usize {
    if let Some(id) = layout.current_layout_id() {
        if let Some(index) = profiles
            .iter()
            .position(|profile| profile.layout_id() == id)
        {
            return index;
        }
    }
    let text = word.text(profiles[0].layout());
    profile::assume_layout(profiles, &text, None)
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

    /// The keys a user presses to type "ghbdtn" — g h b d t n.
    fn privet_strokes() -> ScanWord {
        let mut word = ScanWord::default();
        for keycode in [34u16, 35, 48, 32, 20, 49] {
            word.push(keycode, false);
        }
        word
    }

    /// Shift+7 is `&` on `us` and `.` on the Apple Russian layout; `/` is `.`
    /// on the plain one. After a lone capital either key joins the word.
    #[test]
    fn the_full_stop_key_joins_a_lone_capital() {
        let mac = Layout::lookup("ru", "mac").unwrap();
        let mut word = ScanWord::default();
        word.push(20, true); // Shift+T
        assert!(ends_an_initial(&word, us(), [mac].into_iter(), 8, true));
        assert!(ends_an_initial(&word, us(), [ru()].into_iter(), 53, false));
        // On the Russian layout the key is a full stop already.
        assert!(!ends_an_initial(&word, mac, [us()].into_iter(), 8, true));
        // Lower-case letters join only as a listed abbreviation: `т.` yes,
        // `лет.` no (the word is judged on its own, the stop stays a
        // separator), `млрд.` yes even at four letters.
        let mut lower = ScanWord::default();
        lower.push(49, false); // n → т
        assert!(ends_an_initial(&lower, us(), [mac].into_iter(), 8, true));
        assert!(ends_an_initial(&lower, us(), [ru()].into_iter(), 53, false));
        let mut unlisted = ScanWord::default();
        unlisted.push(20, false); // t → е
        assert!(!ends_an_initial(
            &unlisted,
            us(),
            [mac].into_iter(),
            8,
            true
        ));
        let mut years = ScanWord::default();
        for code in [37u16, 20, 49] {
            years.push(code, false); // k t n → лет
        }
        assert!(!ends_an_initial(
            &years,
            us(),
            [ru()].into_iter(),
            53,
            false
        ));
        let mut billions = ScanWord::default();
        for code in [47u16, 37, 35, 38] {
            billions.push(code, false); // v k h l → млрд
        }
        assert!(ends_an_initial(
            &billions,
            us(),
            [ru()].into_iter(),
            53,
            false
        ));
        // Several capitals are no initial.
        for code in [32, 33, 34] {
            word.push(code, true);
        }
        assert!(!ends_an_initial(&word, us(), [mac].into_iter(), 8, true));
        // A key that is punctuation on both layouts (comma on the Apple one is
        // Shift+6, `^` on us) is not a full stop.
        let mut capital = ScanWord::default();
        capital.push(20, true);
        assert!(!ends_an_initial(&capital, us(), [mac].into_iter(), 7, true));
    }

    #[test]
    fn same_keys_read_differently_per_layout() {
        let word = privet_strokes();
        assert_eq!(word.text(us()), "ghbdtn");
        // This is the entire point: replaying these keys after a layout switch
        // produces the correction without computing a single character.
        assert_eq!(word.text(ru()), "привет");
    }

    #[test]
    fn shift_is_carried_through() {
        let mut word = ScanWord::default();
        word.push(34, true); // G
        word.push(35, false);
        assert_eq!(word.text(us()), "Gh");
        assert_eq!(word.text(ru()), "Пр");
    }

    #[test]
    fn non_block_keys_are_not_part_of_the_word() {
        let mut word = ScanWord::default();
        word.push(34, false);
        word.push(57, false); // space
        word.push(59, false); // F1
        assert_eq!(word.len(), 1);
    }

    #[test]
    fn conversion_happens_once_per_word() {
        let mut word = privet_strokes();
        assert!(!word.already_converted());
        word.mark_converted();
        assert!(word.already_converted());
        word.clear();
        assert!(!word.already_converted());
    }

    /// The keys both previous attempts got wrong. On `us` they draw
    /// punctuation, on `ru` they are `х ъ ж э б ю ё`. They are genuinely
    /// undecidable here, and the classifier has to say so instead of picking a
    /// side: calling them separators cut `изъятия` into `bp]` plus `ятия`,
    /// calling them letters glued `течение.` to the word after it.
    #[test]
    fn the_keys_that_are_letters_elsewhere_come_back_ambiguous() {
        for keycode in [51u16, 52, 39, 40, 26, 27, 41] {
            assert!(
                matches!(
                    classify(us(), [ru()], keycode, false),
                    KeyRole::Ambiguous(_)
                ),
                "клавиша {keycode} на us знак, на ru буква — это неоднозначность"
            );
            // On the Cyrillic layout the same key *is* a letter, no question.
            assert_eq!(classify(ru(), [us()], keycode, false), KeyRole::Letter);
            // With no Cyrillic layout loaded there is nothing to be ambiguous
            // about, and the key ends the word.
            assert_eq!(
                classify(us(), std::iter::empty(), keycode, false),
                KeyRole::Separator
            );
        }
    }

    /// `Казахстана` typed on the US layout: the `х` lands on `[`. Not one of
    /// its keys may be classified as a separator, or the word ends inside
    /// itself — which is how the screen ended up with `Каза` plus `[cnfyf`.
    #[test]
    fn no_key_of_a_russian_word_typed_in_latin_is_a_separator() {
        // R f p f [ c n f y f
        for keycode in [19u16, 33, 25, 33, 26, 46, 49, 33, 21, 33] {
            assert_ne!(
                classify(us(), [ru()], keycode, false),
                KeyRole::Separator,
                "клавиша {keycode} входит в слово «Казахстана»"
            );
        }
    }

    #[test]
    fn letters_and_digits_are_letters_on_either_layout() {
        for keycode in [30u16, 34, 49, 2, 11] {
            assert_eq!(classify(us(), [ru()], keycode, false), KeyRole::Letter);
            assert_eq!(classify(ru(), [us()], keycode, false), KeyRole::Letter);
        }
    }

    /// Shift changes the answer: the `1` key is a digit bare and `!` shifted,
    /// and `!` is a letter on no layout at all.
    #[test]
    fn a_shifted_digit_ends_the_word() {
        assert_eq!(classify(us(), [ru()], 2, false), KeyRole::Letter);
        assert_eq!(classify(us(), [ru()], 2, true), KeyRole::Separator);
    }

    #[test]
    fn keys_outside_the_block_are_not_word_keys() {
        assert_eq!(classify(us(), [ru()], 57, false), KeyRole::Other); // space
        assert_eq!(classify(us(), [ru()], 59, false), KeyRole::Other); // F1
    }

    /// A controller with a keymap we choose, standing in for the X server.
    struct FakeLayout {
        group: u8,
        layouts: Vec<String>,
    }

    impl FakeLayout {
        fn new(group: u8, layouts: &[&str]) -> Self {
            Self {
                group,
                layouts: layouts.iter().map(|id| (*id).to_string()).collect(),
            }
        }
    }

    impl LayoutController for FakeLayout {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn get_current_group(&self) -> Result<u8, pulinto_layout::LayoutError> {
            Ok(self.group)
        }
        fn set_group(&self, _group: u8) -> Result<(), pulinto_layout::LayoutError> {
            Ok(())
        }
        fn layout_ids(&self) -> Vec<String> {
            self.layouts.clone()
        }
    }

    /// The regression. Both cases are group 0 — the number the daemon used to
    /// trust — and they mean opposite layouts.
    #[test]
    fn the_profile_follows_the_layout_name_not_the_group_number() {
        let profiles = pulinto_data::profile::all();
        let ru = profiles.iter().position(|p| p.layout_id() == "ru").unwrap();
        let us = profiles.iter().position(|p| p.layout_id() == "us").unwrap();
        let word = privet_strokes();

        // IBus with the Russian engine active: it rebuilt the keymap as
        // `ru,us`, so the user typing group 0 is typing Russian.
        let russian = FakeLayout::new(0, &["ru", "us"]);
        assert_eq!(current_profile(&russian, &profiles, &word), ru);

        // A moment later, English engine: same group number, other layout.
        let english = FakeLayout::new(0, &["us", "ru"]);
        assert_eq!(current_profile(&english, &profiles, &word), us);

        // A conventional two-group keymap still works, because the name is
        // read out of the position the group points at.
        let conventional = FakeLayout::new(1, &["us", "ru"]);
        assert_eq!(current_profile(&conventional, &profiles, &word), ru);
    }

    #[test]
    fn an_unknown_layout_falls_back_to_guessing_rather_than_asserting() {
        let profiles = pulinto_data::profile::all();
        let word = privet_strokes();

        // Controller that cannot name anything (Wayland today).
        let silent = FakeLayout::new(0, &[]);
        assert_eq!(current_profile(&silent, &profiles, &word), 0);

        // A layout we have no profile for must not be mapped onto one we do.
        let german = FakeLayout::new(0, &["de", "us"]);
        assert_eq!(current_profile(&german, &profiles, &word), 0);
    }

    #[test]
    fn backspace_shortens_the_word() {
        let mut word = privet_strokes();
        word.pop();
        assert_eq!(word.text(us()), "ghbdt");
        assert_eq!(word.replay_keys().len(), 5);
    }

    #[test]
    fn alt_cycle_keeps_the_original_profile_until_the_word_ends() {
        let mut word = privet_strokes();
        word.set_shown_from(0, 1);
        word.set_shown_from(1, 0);
        assert_eq!(word.original_profile(), Some(0));
        assert_eq!(word.shown_profile(), Some(0));

        while !word.is_empty() {
            word.pop();
        }
        assert_eq!(word.original_profile(), None);
        assert_eq!(word.shown_profile(), None);
        assert!(!word.already_converted());
    }
}
