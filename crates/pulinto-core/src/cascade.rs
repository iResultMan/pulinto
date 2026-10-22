//! The decision cascade.
//!
//! Structure: an ordered chain of decisive checks where the first hit wins,
//! rather than a sum of weighted penalties. Measurement on our 2300-example
//! corpus put that design at 96.7% accuracy, so it is used deliberately.
//!
//! Stage 0 (stop rules) and stage 2 (short words) are added on top of the plain
//! chain. Stage 2 is where the measurement showed 91% of its remaining errors
//! live.

use crate::layout::{convert, Layout};
use crate::ngram_bitmap::NgramBitmap;
use crate::pack::{self, Abbreviation};
use crate::short_words::{self, ShortVerdict};
use crate::stop_rules::{self, StopReason};

/// Which stage produced the verdict. Recorded in the decision log — without
/// it, "it did the wrong thing again" reports are impossible to investigate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    StopRule(StopReason),
    /// One capital letter and the key that is a full stop on the layout the
    /// text before it was typed in: an initial (`Г.`), typed on the wrong one.
    Initial,
    /// The reading is a Latin abbreviation from the closed list
    /// (`abbreviations`): `шз` is `ip`, `ЫЙД` is `SQL`.
    Abbreviation,
    /// A Latin abbreviation, a hyphen and a Russian word, all typed on the
    /// Russian layout (`шз-адрес`): only the abbreviation moves, and that is
    /// the caller's job (`latin_prefix_compound`) — here it is left alone.
    Compound,
    Rule,
    ShortWord,
    Dictionary,
    Bigram,
    Trigram,
    Undecided,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub convert: bool,
    /// Layout the word is judged to belong to.
    pub layout_id: &'static str,
    /// Visible text after applying the decision.
    pub text: String,
    pub stage: Stage,
    /// Both readings are real words and nothing but frequency chose between
    /// them — no agreeing context before the word. The word after it may know
    /// better, and the caller may take this verdict back on its strength.
    pub doubtful: bool,
}

/// Everything the cascade needs from one language. Implemented outside this
/// crate (by `pulinto-data`) so that `pulinto-core` stays dependency-free.
pub trait LanguageProfile {
    fn layout(&self) -> &'static Layout;
    /// Stage 1: regex rules from the `dictionary` file.
    fn matches_rule(&self, word: &str) -> bool;
    /// Stage 3: known word form. Only consulted for words of 2+ characters.
    fn in_dictionary(&self, word: &str) -> bool;
    /// Does any known word start with this? Answered by the daemon's word
    /// boundary, not by the cascade — see [`punctuation_continues_word`].
    /// Default `false`: a profile without a word list has no opinion, and the
    /// caller must treat "no opinion" as "not a letter".
    fn starts_a_word(&self, _prefix: &str) -> bool {
        false
    }

    /// Corpus frequency of a known form, when the profile can supply one.
    /// `None` means "no frequency information available", not "unknown word";
    /// profiles backed by a plain spell checker simply keep the default.
    fn word_frequency(&self, _word: &str) -> Option<u64> {
        None
    }
    /// Stages 4 and 5.
    fn ngrams(&self) -> &NgramBitmap;
}

/// Run the cascade over one finished word.
///
/// `profiles` must contain the profile for the layout the user is currently
/// typing on at index `current`. `context_langs` holds the layout ids of up to
/// two preceding words, most recent first.
pub fn decide<P: LanguageProfile>(
    raw_word: &str,
    profiles: &[&P],
    current: usize,
    context_langs: &[&str],
) -> Decision {
    decide_with(raw_word, profiles, current, context_langs, &Thresholds::DEFAULT)
}

/// Same, with explicit stage-3 thresholds, so that they can be swept without
/// rebuilding the logic.
pub fn decide_with<P: LanguageProfile>(
    raw_word: &str,
    profiles: &[&P],
    current: usize,
    context_langs: &[&str],
    thresholds: &Thresholds,
) -> Decision {
    let current_profile = profiles[current];
    let current_layout = current_profile.layout();
    let keep = |stage: Stage| Decision {
        convert: false,
        layout_id: current_layout.id,
        text: raw_word.to_string(),
        stage,
        doubtful: false,
    };

    let switch_to = |target: &P, stage: Stage| Decision {
        convert: true,
        layout_id: target.layout().id,
        text: convert(raw_word, current_layout, target.layout()),
        stage,
        doubtful: false,
    };

    // An initial typed on the wrong layout: `T&` on `us` is `Е.` on the Apple
    // Russian layout (its full stop sits on Shift+7), `U/` is `Г.` on the plain
    // one. Settled ahead of the stop rules, which would read the `/` as a path,
    // and only after a word in the target language: initials are one token in
    // seventy of Russian text, while "capital letter, `&` or `/`, space" does
    // not occur in English text at all (measured on a test corpus). `R&D`
    // and `A/B` never arrive as a two-character token.
    if let Some(target) = initial_elsewhere(raw_word, profiles, current, context_langs) {
        return switch_to(target, Stage::Initial);
    }

    // A Latin abbreviation from the closed list (`шз` → `ip`, `ЫЙД` → `SQL`,
    // `ШЗю` → `IP.`): no n-gram can settle a two-letter word, and an all-caps
    // word is kept out of the n-gram stages on purpose, so the evidence is the
    // list. Ahead of the stop rules, which would see a stray mark in it.
    if let Some(target) = abbreviation_elsewhere(raw_word, profiles, current, context_langs) {
        return switch_to(target, Stage::Abbreviation);
    }
    // `шз-адрес`: converting the whole token would turn the Russian half into
    // `flhtc`. The caller splits at the hyphen; whoever only asks the cascade
    // gets "leave it".
    if latin_prefix_compound(raw_word, profiles, current, context_langs).is_some() {
        return keep(Stage::Compound);
    }

    // Stage 0 runs on the raw word: passwords and identifiers are recognised by
    // their digits and punctuation, which the normalisation below strips away.
    // One exception: on the Russian Apple layout ё sits on the `\` key, so
    // `всё` typed on us is `dc\`, which is not a path. The URL rule is told
    // whether another layout reads the token as letters alone.
    let letters_elsewhere = reads_as_letters_elsewhere(raw_word, profiles, current);
    if let Some(reason) = stop_rules::check_with(raw_word, letters_elsewhere) {
        return keep(Stage::StopRule(reason));
    }

    let word = normalise(raw_word, profiles, current_layout);
    if word.is_empty() {
        return keep(Stage::Undecided);
    }

    // A token with no letters in it, as typed, is punctuation — not a word.
    // Normalisation deliberately keeps `,` `;` `'` because on the Russian
    // layout they are the letters б ж э, without which `,tufnm` (бегать) is
    // unrecoverable. But that reasoning only holds when letters follow: a lone
    // comma is a comma, and converting it to `б` corrupts ordinary punctuation.
    // Typing a bare Russian `б` on the US layout is the sacrificed case, and it
    // is far rarer than a comma.
    if !word.chars().any(char::is_alphabetic) {
        return keep(Stage::Undecided);
    }

    let rule_hit = |word: &str| {
        profiles.iter().position(|profile| {
            profile.matches_rule(&hypothesis_for(word, current_layout, profile.layout()))
        })
    };

    // Stage 2 deliberately runs before stage 1 for short words. The regex rule
    // files carry per-language one-letter whitelists but know nothing about
    // context, so letting them decide first turns every English `f` into a
    // Russian `а`. Measured on the corpus: that ordering alone accounts for
    // most of the error on one-letter words.
    let length = word.chars().count();
    if length <= 2 {
        for (index, profile) in profiles.iter().enumerate() {
            if index == current {
                continue;
            }
            match short_words::judge(word, current_layout.id, profile.layout().id, context_langs) {
                ShortVerdict::Keep => return keep(Stage::ShortWord),
                ShortVerdict::Convert => return switch_to(profile, Stage::ShortWord),
                ShortVerdict::Undecided => {}
            }
            // No context asked: the typed pair is no word of its own language
            // and the other one is (`щт` is `on`, `фе` is `at`).
            if length == 2 && only_reading(word, current_profile, profile) {
                return switch_to(profile, Stage::ShortWord);
            }
        }
        // The regex rules are not consulted for short words at all. Their
        // one-letter entries are the same whitelist stage 2 just applied, minus
        // the context check, so they can only overrule a context-aware verdict
        // with a context-free one. Measured: they fired 18 times and were wrong
        // 17 of them.
        return keep(Stage::Undecided);
    }

    // Stage 1: regex rules.
    if let Some(index) = rule_hit(word) {
        return if index == current {
            keep(Stage::Rule)
        } else {
            switch_to(profiles[index], Stage::Rule)
        };
    }

    // An all-caps word is treated as an acronym: n-gram evidence alone is not
    // allowed to move it, and its alternative must be a common word.
    let is_acronym = {
        let letters = word.chars().filter(|ch| ch.is_alphabetic());
        let mut count = 0usize;
        let all_upper = letters.inspect(|_| count += 1).all(char::is_uppercase);
        count >= 2 && all_upper
    };

    // Stage 3: known word forms.
    let floor = thresholds.floor(length);
    let known = |profile: &P, text: &str| -> Option<u64> {
        match profile.word_frequency(text) {
            Some(frequency) => (frequency >= floor).then_some(frequency),
            // No frequency data: fall back to the boolean answer.
            None => profile.in_dictionary(text).then_some(1),
        }
    };

    // A key that is punctuation here and a letter on another layout may have
    // been taken into the word (`it.` against the Apple Russian layout, whose
    // `.` key is `ю`). The reading as typed is then judged without it: `it`
    // is a word, and `it.` against `шею` is a collision, not a one-sided
    // dictionary hit.
    let bare = word.trim_end_matches(|ch: char| !ch.is_alphabetic());
    let mine = known(current_profile, word)
        .or_else(|| (!bare.is_empty() && bare != word).then(|| known(current_profile, bare)).flatten());
    let mut best_other: Option<(usize, u64)> = None;
    for (index, profile) in profiles.iter().enumerate() {
        if index == current {
            continue;
        }
        let hypothesis = hypothesis_for(word, current_layout, profile.layout());
        if let Some(frequency) = known(profile, &hypothesis) {
            if best_other.is_none_or(|(_, seen)| frequency > seen) {
                best_other = Some((index, frequency));
            }
        }
    }

    match (mine, best_other) {
        // Both readings are real words — the `dyer`/`внук` situation, which at
        // three letters is not rare but routine. The language of the text
        // before the word is the first judge: Russian text, a Russian word
        // typed on the Russian layout — it stays, however common its English
        // shadow (`рук` against `her`); English text before an English word
        // typed on the Russian layout — it moves. Only without that context
        // does frequency decide, and then the alternative must be far more
        // common; a tie stays put, because a false conversion costs more
        // than a miss.
        (Some(mine), Some((index, theirs))) => {
            let target_id = profiles[index].layout().id;
            // One word is not a context: `у GGG все` has English right before
            // a Russian word. Two agreeing words are. Acronyms are exempt —
            // `BBC`, `UFC` sit in Russian text on purpose and their Cyrillic
            // shadows are abbreviations nobody meant — and so is a target too
            // rare to be anything but noise (`Rune` → `кгту`).
            let settled = if is_acronym { None } else { agreed_context(context_langs) };
            // Context moves a word only towards a reading of comparable
            // standing: `her` (20 648) → `рук` (330) yes, `are` (66 143) →
            // `фку` (13) no — a Russian abbreviation in the dictionary is not
            // what an English word in Russian text turns into.
            let by_context = settled == Some(target_id)
                && theirs >= COLLISION_TARGET_MIN
                && theirs.saturating_mul(COLLISION_CONTEXT_RATIO) >= mine;
            let by_frequency = theirs >= mine.saturating_mul(thresholds.margin);
            let mut decision = if settled == Some(current_layout.id) {
                keep(Stage::Dictionary)
            } else if by_context || by_frequency {
                switch_to(profiles[index], Stage::Dictionary)
            } else {
                keep(Stage::Dictionary)
            };
            // Decided by frequency alone (or by its margin), with no agreeing
            // text before it: the next word may still settle it. Acronyms are
            // not offered for that — `BBC` in Russian text is meant.
            decision.doubtful = settled.is_none() && !is_acronym;
            return decision;
        }
        (Some(_), None) => return keep(Stage::Dictionary),
        (None, Some((index, frequency))) => {
            if is_acronym && frequency < thresholds.acronym_min_frequency {
                return keep(Stage::Dictionary);
            }
            // Two words of the typed language before an unknown Latin acronym:
            // it is part of that text, not a Russian abbreviation (`HBF` after
            // "star in several" is not `РИА`). Only this direction: a Latin
            // acronym in Russian text typed on the Russian layout (`ШЫИТ`
            // for `ISBN`) is the ordinary case and still moves. The closed
            // list of Latin abbreviations was consulted before this point, so
            // `IP` or `API` typed on the wrong layout move as well.
            if is_acronym
                && word.chars().any(|ch| ch.is_ascii_alphabetic())
                && agreed_context(context_langs) == Some(current_layout.id)
            {
                return keep(Stage::Dictionary);
            }
            return switch_to(profiles[index], Stage::Dictionary);
        }
        (None, None) => {}
    }

    // Acronyms stop here. Reaching the n-gram stages would mean converting on
    // "nothing forbidden in the other language", which is exactly how `LXDE`
    // turned into `ДЧВУ`.
    if is_acronym {
        return keep(Stage::Undecided);
    }

    // Stages 4 and 5 are separate on purpose. The current language is accepted
    // as soon as its bigrams come back clean and trigrams are not consulted; a
    // combined check would be stricter and would trade false negatives for
    // false positives, which is the wrong direction for us.
    for (stage, has_forbidden) in [
        (Stage::Bigram, NgramBitmap::has_forbidden_bigram as fn(&NgramBitmap, &str) -> bool),
        (Stage::Trigram, NgramBitmap::has_forbidden_trigram as fn(&NgramBitmap, &str) -> bool),
    ] {
        let forbidden_here = has_forbidden(current_profile.ngrams(), word);

        // The forbidden combination is the evidence; a clean one is not. Under
        // this rule a level only decides when it *discriminates* between the
        // readings, and otherwise defers to the next one.
        //
        // The difference from the rule below is a single case: clean in both
        // languages. Today that ends the cascade with "keep" — a verdict drawn
        // from the absence of evidence, since a random string clears the 203
        // forbidden Russian bigrams most of the time. Deferring lets the
        // trigram tables (19 141 entries against 8 990) look at a word the
        // bigram tables were never going to separate.
        // The reading we would move to has to be clean at both levels, not
        // just the one that condemned the typed one: `Санькя` fails a Russian
        // bigram (`кя`), and its Latin shadow `cfymrz` clears the 112
        // forbidden English bigrams — but not the trigrams (`cfy`, `fym`,
        // `ymr`, `mrz`). Converting into a reading the trigrams reject is a
        // false conversion with a guarantee.
        //
        // Only when the text so far is in the language of the layout the word
        // was typed on: moving a word *away* from the language around it
        // needs the stronger evidence. A word typed on the wrong layout after
        // words in the target language (`{fvlfy` after `и Мохамед`) is the
        // ordinary case, and names there often carry a trigram the tables
        // have never seen; one clean level is enough for them, as before.
        let strict = agreed_context(context_langs) == Some(current_layout.id);
        // A reading with punctuation inside is not a word of that language,
        // whatever its n-grams say: `Вуджек` typed on the Russian layout reads
        // `Del;tr` on the Latin one, and the `;` would only break the windows
        // the forbidden trigrams are looked up in. Punctuation at either end
        // is another matter: `Утукпнб` is `Energy,` — the comma key taken for
        // the letter `б` — and `Эгне` is `"But`. Apostrophe and hyphen are
        // letters' company in both languages (`don't`, `по-русски`).
        let reads_as_word = |text: &str| {
            text.trim_matches(|ch: char| !ch.is_alphanumeric())
                .chars()
                .all(|ch| ch.is_alphabetic() || ch == '\'' || ch == '-')
        };
        let clean_target = |profile: &P, text: &str| {
            reads_as_word(text)
                && !has_forbidden(profile.ngrams(), text)
                && (!strict
                    || (!profile.ngrams().has_forbidden_bigram(text)
                        && !profile.ngrams().has_forbidden_trigram(text)))
        };
        if thresholds.ngram_discrimination {
            let mut clean_elsewhere = None;
            let mut clean_count = 0usize;
            for (index, profile) in profiles.iter().enumerate() {
                if index == current {
                    continue;
                }
                let hypothesis = hypothesis_for(word, current_layout, profile.layout());
                if !has_forbidden(profile.ngrams(), &hypothesis) {
                    clean_count += 1;
                    clean_elsewhere.get_or_insert(index);
                }
            }
            match (forbidden_here, clean_count) {
                // Impossible here, possible in exactly one other language — if
                // that language's other level agrees; otherwise this level has
                // not really separated anything and the next one may.
                (true, 1) => {
                    let index = clean_elsewhere.unwrap();
                    let hypothesis = hypothesis_for(word, current_layout, profiles[index].layout());
                    if clean_target(profiles[index], &hypothesis) {
                        return switch_to(profiles[index], stage);
                    }
                    continue;
                }
                // Possible here, impossible everywhere else.
                (false, 0) => return keep(stage),
                // Impossible everywhere, or possible in several places: this
                // level separates nothing. Two candidates are as good as none —
                // picking the first by index is what the rule below does, and it
                // is arbitrary the moment a third language exists.
                _ => continue,
            }
        }

        if !forbidden_here {
            return keep(stage);
        }
        for (index, profile) in profiles.iter().enumerate() {
            if index == current {
                continue;
            }
            let hypothesis = hypothesis_for(word, current_layout, profile.layout());
            if clean_target(profile, &hypothesis) {
                return switch_to(profile, stage);
            }
        }
    }

    keep(Stage::Undecided)
}

/// Tunable part of stage 3.
///
/// Short tokens collide across languages constantly: a 200k-form word list
/// contains `ult`, `vs`, `pf`, `bp` as genuine entries, and every one of them
/// is also the mistyped shadow of a common Russian word. Measured on the
/// held-out corpus, 7.4% of garbled forms were accepted as words at a flat
/// threshold, and every such acceptance stops the cascade with the wrong
/// verdict. Long forms have no such problem, so their floor stays at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// Minimum corpus frequency for a form of 2, 3, 4 characters to count as a
    /// real word. Longer forms are always accepted.
    pub min_frequency: [u64; 3],
    /// How much more common the alternative reading must be before we move a
    /// word that is already valid as typed.
    pub margin: u64,
    /// Frequency an all-caps word's alternative must clear before we touch it.
    /// Calibrated against the corpus: `nasa` sits at 261, the acronyms that
    /// caused false conversions (`atv` 15, `lba` 5) sit far below.
    ///
    /// Acronyms are the hardest case in the whole cascade: `LXDE`, `KVM`, `XDC`
    /// are real words to their users but nonsense to any dictionary, and their
    /// Cyrillic shadows (`ДЧВУ`, `ЛМЬ`, `ЧВС`) contain no forbidden n-grams, so
    /// weak evidence happily "corrects" them into gibberish. Demanding a common
    /// word on the other side lets `ТФЫФ` become `NASA` while leaving unknown
    /// acronyms alone.
    pub acronym_min_frequency: u64,
    /// Stages 4 and 5: decide only when a level tells the readings apart.
    ///
    /// The level must discriminate: forbidden here and clean in exactly one
    /// other language moves the word, clean here and forbidden everywhere else
    /// keeps it, anything else falls through to the next level. Off is the old
    /// rule, where a word with no forbidden n-gram in the current language was
    /// kept there and then — a verdict drawn from the absence of evidence,
    /// which is what left the trigram tables deciding nothing at all.
    ///
    /// Measured before switching on:
    /// held-out corpus 99.19% -> 99.38% with false positives unchanged at the
    /// same ten rows, second corpus 93.40% -> 93.78% at 35 new false positives
    /// against 5 341 recovered misses.
    pub ngram_discrimination: bool,
}

impl Thresholds {
    /// Tuned by sweep on one half of the held-out corpus and validated on the
    /// other: 98.69% -> 98.96% accuracy on the unseen half, misses 4.87% ->
    /// 3.87%, false positives unchanged at 0.03%.
    ///
    /// The zeros are the interesting part. A frequency floor turned out to help
    /// only at two characters; at three and four it cost more real words than
    /// it saved collisions. What actually won was the margin: when both
    /// readings are real words, move only if the alternative is 4x more common.
    pub const DEFAULT: Self = Self {
        min_frequency: [1000, 0, 0],
        margin: 4,
        acronym_min_frequency: 100,
        ngram_discrimination: true,
    };

    fn floor(&self, length: usize) -> u64 {
        match length {
            0..=2 => self.min_frequency[0],
            3 => self.min_frequency[1],
            4 => self.min_frequency[2],
            _ => 1,
        }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A frequency below which a pair of letters counts as no word of its language
/// at all. The Russian dictionary holds `фе` (3) and `ше` (2) as stray entries;
/// the abbreviations that do occur in text sit well above: `ин` 16, `шт` 33,
/// `ру` 135.
const NO_READING_BELOW: u64 = 10;

/// Two letters that cannot be a word where they were typed but are one on the
/// other layout, so that no context is needed to move them. `щт` is a forbidden
/// bigram of Russian and `on` an English word; `фе` is no Russian word worth
/// the name (3 in the dictionary) and `at` is English. Two evidences count:
/// the pair is a forbidden bigram of the typed language, or the typed
/// language's dictionary has next to nothing for it. The other reading must
/// be a word of the short-word table, and a short closed list of Russian
/// colloquialisms and abbreviations (`ща`, `гы`, `шт`) is never moved this way.
///
/// Measured on the project's texts: 191 000 Russian and 444 000 English words
/// gave no wrong move (before the closed list, `ин`, `шт` and `ру` gave 21).
fn only_reading<P: LanguageProfile>(word: &str, here: &P, there: &P) -> bool {
    let lower = word.to_lowercase();
    if short_words::is_valid_short_word(&lower, here.layout().id)
        || short_words::is_kept_short_form(&lower, here.layout().id)
    {
        return false;
    }
    let moved = convert(&lower, here.layout(), there.layout());
    if !short_words::is_valid_short_word(&moved, there.layout().id) {
        return false;
    }
    here.ngrams().has_forbidden_bigram(&lower)
        || here.word_frequency(&lower).is_none_or(|count| count < NO_READING_BELOW)
}

/// Strip trailing digits and leading punctuation or digits before running the
/// cascade — but only where "punctuation" holds in every candidate layout.
fn normalise<'a, P: LanguageProfile>(
    word: &'a str,
    profiles: &[&P],
    current: &Layout,
) -> &'a str {
    let trimmed = word.trim_end_matches(|ch: char| ch.is_ascii_digit());
    trimmed.trim_start_matches(|ch: char| strippable(ch, profiles, current))
}

/// On the Russian layout `,` `;` `'` `[` `]` are the letters б ж э х ъ.
/// Stripping them unconditionally as punctuation before deciding the language
/// would destroy the evidence about to be judged: `,tufnm` (бегать) would lose
/// its first letter and become undetectable. So a character is strippable
/// only when it is punctuation under every candidate layout.
fn strippable<P: LanguageProfile>(ch: char, profiles: &[&P], current: &Layout) -> bool {
    if ch.is_ascii_digit() {
        return true;
    }
    if !ch.is_ascii_punctuation() {
        return false;
    }
    profiles.iter().all(|profile| {
        match current
            .position_of(ch)
            .and_then(|(position, level)| profile.layout().char_at(position, level))
        {
            Some(mapped) => !mapped.is_alphabetic(),
            None => true,
        }
    })
}

/// At a key that draws punctuation on the layout in effect and a letter on
/// another one: is it a letter of this word, or the thing that ends it?
///
/// Neither answer can be given from the key alone, and both mistakes are real.
/// Say "punctuation" and a Russian word typed in Latin is cut at its own
/// letter: `изъятия` arrives as `bp]znbz`, and the daemon corrects `znbz` while
/// `bp]` sits on the screen. Say "letter" and two words are glued across the
/// stop between them: `течение.` plus `craft` became the single token
/// `течениеюскфае`, which the cascade then cheerfully judged.
///
/// The evidence that separates them is the dictionary, asked about a **prefix**
/// rather than a word. `изъ` starts `изъятие`; `течениею` starts nothing. The
/// word is not finished yet, so a membership test cannot help — but a
/// transducer walks prefixes for free.
///
/// `word_so_far` is the text of the word as typed, under `profiles[current]`.
pub fn punctuation_continues_word<P: LanguageProfile>(
    word_so_far: &str,
    punctuation: char,
    profiles: &[&P],
    current: usize,
) -> bool {
    let current_layout = profiles[current].layout();
    let mut candidate = String::with_capacity(word_so_far.len() + punctuation.len_utf8());
    candidate.push_str(word_so_far);
    candidate.push(punctuation);

    profiles.iter().enumerate().any(|(index, profile)| {
        if index == current {
            return false;
        }
        let reading = hypothesis_for(&candidate, current_layout, profile.layout());
        // A reading whose last character is not a letter tells us nothing: the
        // key is punctuation on that layout too.
        if !reading.chars().next_back().is_some_and(char::is_alphabetic) {
            return false;
        }
        let reading = reading.to_lowercase();
        if profile.starts_a_word(&reading) {
            return true;
        }
        // No word starts like that — but the dictionary is a frequency cut,
        // not a paradigm: half the adjectives in it lack their feminine
        // accusative (`сдерживающую`) or genitive plural (`гильбертовых`),
        // and those endings sit on the full stop and the bracket keys. When
        // the word before the key is unknown as well, a stem long enough to
        // mean something that the dictionary knows under another ending of
        // the same paradigm is taken as evidence that the key is a letter.
        // `дом.` and `течение.` are known words before the key and are never
        // asked; `из]` is settled by the prefix above.
        let before = hypothesis_for(word_so_far, current_layout, profile.layout()).to_lowercase();
        !profile.in_dictionary(&before) && has_stem_family(*profile, &reading)
    })
}

const STEM_MIN_CHARS: usize = 5;

/// Does `reading` end in one of the endings of the language pack (`paradigm`), on a stem of at least
/// [`STEM_MIN_CHARS`] letters that the dictionary knows with another ending
/// of the same paradigm?
fn has_stem_family<P: LanguageProfile>(profile: &P, reading: &str) -> bool {
    let Some(language) = pack::language(profile.layout().id) else {
        return false;
    };
    language.families().iter().any(|(ending, siblings)| {
        let Some(stem) = reading.strip_suffix(ending) else {
            return false;
        };
        stem.chars().count() >= STEM_MIN_CHARS
            && siblings
                .iter()
                .filter(|sibling| *sibling != ending)
                .any(|sibling| profile.in_dictionary(&format!("{stem}{sibling}")))
    })
}

/// Least corpus frequency of a reading a collision may be moved to on the
/// strength of context alone.
const COLLISION_TARGET_MIN: u64 = 5;
/// And how much rarer than the typed word that reading may be: within this
/// factor context decides, beyond it the typed word stands (`are` 66 143
/// against `фку` 13 is 5 000×; `her` against `рук` is 63×; `far` against
/// `афк` 175× stays).
const COLLISION_CONTEXT_RATIO: u64 = 100;

/// The language the text before the word is in, when the last two words
/// agree on it; one word alone proves nothing.
fn agreed_context(context_langs: &[&str]) -> Option<&'static str> {
    match context_langs {
        [first, second, ..] if first == second => Layout::by_id(first).map(|layout| layout.id),
        _ => None,
    }
}

/// The profile under which `raw_word` — a capital letter and one more key —
/// reads as an initial with its full stop, when the current layout draws
/// something other than a full stop on that key and the word before was typed
/// in that profile's language.
fn initial_elsewhere<'a, P: LanguageProfile>(
    raw_word: &str,
    profiles: &[&'a P],
    current: usize,
    context_langs: &[&str],
) -> Option<&'a P> {
    // The letters are checked in the target reading below, not here: `ж`
    // sits on `;` and `Дж.` arrives as `L;&`.
    let mark = raw_word.chars().next_back()?;
    let count = raw_word.chars().count() - 1;
    if !(1..=4).contains(&count) || mark.is_alphabetic() || mark == '.' {
        return None;
    }
    let previous = *context_langs.first()?;
    let from = profiles[current].layout();
    profiles
        .iter()
        .enumerate()
        .filter(|(index, profile)| *index != current && profile.layout().id == previous)
        .find(|(_, profile)| {
            let reading = convert(raw_word, from, profile.layout());
            let Some(head) = reading.strip_suffix('.') else { return false };
            if !head.chars().all(char::is_alphabetic) {
                return false;
            }
            let mut head_chars = head.chars();
            let initial = head_chars.next().is_some_and(char::is_uppercase) && head_chars.next().is_none();
            // Lower-case: only the abbreviations Russian really writes with a
            // full stop (`г.`, `т.`, `др.`, `стр.`); any lower-case letter
            // would also catch `w/`, `c/o`, `b/c` on the plain layout.
            let abbreviation = pack::language(profile.layout().id)
                .is_some_and(|language| language.is_dotted(&head.to_lowercase()));
            initial || abbreviation
        })
        .map(|(_, profile)| *profile)
}

/// The first `(char index, target layout id)` of a hyphen that turns the token
/// into "Latin abbreviation, hyphen, Russian word" typed all on the Russian
/// layout: `шз-адрес` is `IP-адрес` with the first half left on the wrong
/// layout. Only the half before the hyphen moves; the rest was typed right.
///
/// Both halves are checked: the left reads as a listed abbreviation and is no
/// word as typed, every part of the right is a word of the typed layout's
/// language. Genuine Russian abbreviations before a hyphen (`ЖК-банк`) fail
/// the first test, ordinary compounds (`по-русски`) the second.
pub fn latin_prefix_compound<P: LanguageProfile>(
    raw_word: &str,
    profiles: &[&P],
    current: usize,
    context_langs: &[&str],
) -> Option<(usize, &'static str)> {
    let hyphen = raw_word.chars().position(|ch| ch == '-')?;
    if hyphen == 0 {
        return None;
    }
    let left: String = raw_word.chars().take(hyphen).collect();
    let right: String = raw_word.chars().skip(hyphen + 1).collect();
    if right.is_empty() || !left.chars().all(char::is_alphabetic) {
        return None;
    }
    let here = profiles[current];
    // The Russian side: every part is a word, no digits or marks.
    if !right
        .split('-')
        .all(|part| part.chars().count() >= 2 && part.chars().all(char::is_alphabetic) && here.in_dictionary(&part.to_lowercase()))
    {
        return None;
    }
    let target = abbreviation_target(&left, profiles, current, context_langs)?;
    Some((hyphen, target.layout().id))
}

/// The profile a typed abbreviation (`ШЗ`, `шз`, `ЫЙД`; letters only, one
/// case) belongs to, if its reading is on the closed list, the typed form is no
/// word, and — for the entries with a Russian look-alike — English text came
/// before.
fn abbreviation_target<'a, P: LanguageProfile>(
    typed: &str,
    profiles: &[&'a P],
    current: usize,
    context_langs: &[&str],
) -> Option<&'a P> {
    let count = typed.chars().count();
    if !(2..=6).contains(&count) || !typed.chars().all(char::is_alphabetic) {
        return None;
    }
    let lower = typed.to_lowercase();
    // One case only; the reading keeps it.
    if typed != lower && typed != typed.to_uppercase() {
        return None;
    }
    let here = profiles[current];
    profiles
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != current)
        .find(|(_, profile)| {
            let word_here = crate::short_words::is_valid_short_word(&lower, here.layout().id);
            let Some(language) = pack::language(profile.layout().id) else {
                return false;
            };
            match language.abbreviation(&convert(typed, here.layout(), profile.layout())) {
                // A real word of the typed language is a word, not an
                // abbreviation: `ем` is `tv` on the other layout, and a
                // Russian speaker means `ем`.
                Abbreviation::Anywhere => !word_here && !here.in_dictionary(&lower),
                // The look-alikes (`фк`, `гл` are Russian abbreviations the
                // dictionary knows) move only between two words of the
                // abbreviation's language: the user is plainly writing it, and
                // the dictionary has nothing to say about that.
                Abbreviation::AfterContext => {
                    agreed_context(context_langs) == Some(profile.layout().id) && !word_here
                }
                Abbreviation::No => false,
            }
        })
        .map(|(_, profile)| *profile)
}

/// A listed Latin abbreviation typed on the wrong layout, with at most one
/// mark after it that is a letter here (`ШЗю` is `IP.`).
fn abbreviation_elsewhere<'a, P: LanguageProfile>(
    raw_word: &str,
    profiles: &[&'a P],
    current: usize,
    context_langs: &[&str],
) -> Option<&'a P> {
    let count = raw_word.chars().count();
    if count < 2 {
        return None;
    }
    // Try the whole token first, then the token without its last key when
    // that key reads as a mark in the other layout.
    if let Some(target) = abbreviation_target(raw_word, profiles, current, context_langs) {
        return Some(target);
    }
    let core: String = raw_word.chars().take(count - 1).collect();
    let target = abbreviation_target(&core, profiles, current, context_langs)?;
    let reading = convert(raw_word, profiles[current].layout(), target.layout());
    let last = reading.chars().last()?;
    (".,;:!?".contains(last)).then_some(target)
}

/// Does another layout read this token as nothing but letters?
fn reads_as_letters_elsewhere<P: LanguageProfile>(
    raw_word: &str,
    profiles: &[&P],
    current: usize,
) -> bool {
    let current_layout = profiles[current].layout();
    profiles.iter().enumerate().any(|(index, profile)| {
        if index == current {
            return false;
        }
        let reading = hypothesis_for(raw_word, current_layout, profile.layout());
        !reading.is_empty() && reading.chars().all(char::is_alphabetic)
    })
}

fn hypothesis_for(word: &str, from: &Layout, to: &Layout) -> String {
    if from.id == to.id {
        word.to_string()
    } else {
        convert(word, from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::alphabet;

    struct Profile {
        layout: &'static Layout,
        rules: Vec<String>,
        words: Vec<String>,
        frequencies: std::collections::HashMap<String, u64>,
        ngrams: NgramBitmap,
    }

    impl Profile {
        fn new(id: &str, alphabet: &str, words: &[&str], forbidden: &[&str]) -> Self {
            let (bigrams, trigrams): (Vec<String>, Vec<String>) = forbidden
                .iter()
                .map(|s| s.to_string())
                .partition(|s| s.chars().count() == 2);
            Profile {
                layout: Layout::by_id(id).unwrap(),
                rules: Vec::new(),
                words: words.iter().map(|w| w.to_string()).collect(),
                frequencies: Default::default(),
                ngrams: NgramBitmap::build(alphabet, bigrams, trigrams),
            }
        }

        fn with_rule(mut self, rule: &str) -> Self {
            self.rules.push(rule.to_string());
            self
        }

        fn on_variant(mut self, variant: &str) -> Self {
            self.layout = Layout::lookup(self.layout.id, variant).unwrap();
            self
        }
    }

    /// On the Apple Russian layout ё is the `\` key: `всё` typed on us is
    /// `dc\`, and the URL stop rule must not take the backslash for a path.
    #[test]
    fn a_backslash_that_is_yo_on_the_other_layout_is_not_a_url() {
        let ru = Profile::new("ru", alphabet("ru"), &["всё", "счёт"], &[]).on_variant("mac");
        let us = us();
        let profiles = [&us, &ru];
        let decision = decide("dc\\", &profiles, 0, &[]);
        assert!(decision.convert, "{decision:?}");
        assert_eq!(decision.text, "всё");
        // A path with a forward slash still stops, and so does code.
        let decision = decide("src/lib.rs", &profiles, 0, &[]);
        assert!(matches!(decision.stage, Stage::StopRule(_)), "{decision:?}");
        let decision = decide("std::vector", &profiles, 0, &[]);
        assert!(matches!(decision.stage, Stage::StopRule(_)), "{decision:?}");
    }

    /// With the standard Russian layout the backslash is punctuation on both,
    /// and the stop rule applies as before.
    #[test]
    fn a_backslash_that_is_punctuation_everywhere_still_stops() {
        let ru = Profile::new("ru", alphabet("ru"), &["всё"], &[]);
        let us = us();
        let profiles = [&us, &ru];
        let decision = decide("dc\\", &profiles, 0, &[]);
        assert!(!decision.convert);
        assert!(matches!(decision.stage, Stage::StopRule(_)), "{decision:?}");
    }

    impl LanguageProfile for Profile {
        fn layout(&self) -> &'static Layout {
            self.layout
        }
        fn matches_rule(&self, word: &str) -> bool {
            self.rules.iter().any(|rule| rule == word)
        }
        fn in_dictionary(&self, word: &str) -> bool {
            self.words.iter().any(|known| known == word)
        }
        fn word_frequency(&self, word: &str) -> Option<u64> {
            self.frequencies.get(word).copied()
        }
        fn starts_a_word(&self, prefix: &str) -> bool {
            self.words.iter().any(|known| known.starts_with(prefix))
        }
        fn ngrams(&self) -> &NgramBitmap {
            &self.ngrams
        }
    }

    fn us() -> Profile {
        Profile::new("us", alphabet("us"), &["hello", "cell"], &["gh", "bdt"])
    }

    fn ru() -> Profile {
        Profile::new("ru", alphabet("ru"), &["привет", "табуретка", "бегать"], &["ыъ", "жыа"])
    }

    fn ru_mac() -> Profile {
        let mut profile = ru();
        profile.layout = Layout::lookup("ru", "mac").unwrap();
        profile
    }

    /// `Е.` typed on `us` against the Apple Russian layout arrives as `T&`;
    /// against the plain one `Г.` arrives as `U/`. Both are initials when the
    /// text before was Russian, and only then.
    #[test]
    fn an_initial_typed_on_the_wrong_layout_is_converted_with_its_full_stop() {
        let us = us();
        let mac = ru_mac();
        let profiles = [&us, &mac];
        let decision = decide("T&", &profiles, 0, &["ru"]);
        assert_eq!((decision.convert, decision.stage), (true, Stage::Initial), "{decision:?}");
        assert_eq!(decision.text, "Е.");

        let ru = ru();
        let profiles = [&us, &ru];
        let decision = decide("U/", &profiles, 0, &["ru"]);
        assert_eq!((decision.convert, decision.stage), (true, Stage::Initial), "{decision:?}");
        assert_eq!(decision.text, "Г.");

        // English text before it: a pair like this is left alone.
        assert!(!decide("U/", &profiles, 0, &["us"]).convert);
        assert!(!decide("U/", &profiles, 0, &[]).convert);
        // A listed lower-case abbreviation moves too (`г.`); an unlisted
        // letter, a full stop already here, or more than a pair do not.
        assert_eq!(decide("u/", &profiles, 0, &["ru"]).text, "г.");
        assert!(!decide("z/", &profiles, 0, &["ru"]).convert, "`я` is no abbreviation");
        assert!(!decide("U.", &profiles, 0, &["ru"]).convert);
        assert!(!decide("R&D", &[&us, &mac], 0, &["ru"]).convert);
        assert!(!decide("A/B", &profiles, 0, &["ru"]).convert);
        // Typed on the Russian layout it is already an initial.
        assert!(!decide("Е.", &[&us, &mac], 1, &["ru"]).convert);
        // Lower-case abbreviations from the list, and nothing else lower-case.
        let decision = decide("n&", &[&us, &mac], 0, &["ru"]);
        assert_eq!((decision.convert, decision.text.clone()), (true, "т.".to_string()), "{decision:?}");
        assert_eq!(decide("cnh&", &[&us, &mac], 0, &["ru"]).text, "стр.");
        assert!(decide("lh/", &profiles, 0, &["ru"]).convert, "др. on the plain layout");
        assert_eq!(decide("htl/", &profiles, 0, &["ru"]).text, "ред.");
        assert_eq!(decide("vkhl/", &profiles, 0, &["ru"]).text, "млрд.", "four letters from the list");
        assert_eq!(decide("L;&", &[&us, &mac], 0, &["ru"]).text, "Дж.", "a transliterated J.");
        assert!(!decide("ktn/", &profiles, 0, &["ru"]).convert, "`лет` is a word, not an abbreviation");
        assert!(!decide("w/", &profiles, 0, &["ru"]).convert);
        assert!(!decide("b/", &profiles, 0, &["ru"]).convert, "`b` is no abbreviation: `b/c`");
    }

    /// `in on at it` typed on the Russian layout: `шт щт фе ше`. No Russian
    /// word is any of them (`щт` is not even a possible Russian bigram), so no
    /// context is asked. Russian pairs that do live in text stay.
    #[test]
    fn two_letters_that_are_no_word_here_and_a_word_there_move_without_context() {
        let us = us();
        let mut ru = Profile::new("ru", alphabet("ru"), &["привет"], &["щт"]);
        // The dictionary holds `шт` (pieces) and `ин` as real abbreviations,
        // `фе` as a stray entry.
        ru.frequencies.insert("шт".into(), 33);
        ru.frequencies.insert("фе".into(), 3);
        ru.frequencies.insert("ин".into(), 16);
        ru.frequencies.insert("ща".into(), 2);
        let profiles = [&us, &ru];

        for context in [&[][..], &["ru", "ru"][..], &["us", "us"][..]] {
            let decision = decide("щт", &profiles, 1, context);
            assert_eq!(
                (decision.convert, decision.text.as_str(), decision.stage),
                (true, "on", Stage::ShortWord),
                "{context:?}"
            );
        }
        // Rare in the dictionary, and `at` / `it` are English words.
        assert_eq!(decide("фе", &profiles, 1, &["ru", "ru"]).text, "at");
        assert_eq!(decide("ше", &profiles, 1, &["ru", "ru"]).text, "it");
        assert_eq!(decide("ЩТ", &profiles, 1, &["ru"]).text, "ON");

        // A Russian word of the table, a Russian abbreviation that is in text,
        // a chat word: all stay with Russian text before them.
        for kept in ["на", "шт", "ин", "ща"] {
            assert!(!decide(kept, &profiles, 1, &["ru", "ru"]).convert, "{kept}");
        }
        // The other reading must be a table word: `щщ` is `oo`.
        assert!(!decide("щщ", &profiles, 1, &["ru", "ru"]).convert);
        // A Russian pair the dictionary knows well is a word, whatever it reads as.
        ru.frequencies.insert("ше".into(), 500);
        assert!(!decide("ше", &[&us, &ru], 1, &["ru", "ru"]).convert);
    }

    /// The same on the other side: `он`, `от`, `по` typed on the English layout
    /// are `jy`, `jn`, `gj`, forbidden English bigrams. English abbreviations
    /// with a life of their own (`tv` is `ем`, `vs` is `мы`) are not moved.
    #[test]
    fn a_forbidden_english_pair_that_is_a_russian_word_moves_without_context() {
        let mut us = Profile::new("us", alphabet("us"), &["hello"], &["jy", "jn", "gj"]);
        us.frequencies.insert("tv".into(), 1399);
        us.frequencies.insert("vs".into(), 385);
        let ru = Profile::new("ru", alphabet("ru"), &["привет"], &[]);
        let profiles = [&us, &ru];

        for (typed, moved) in [("jy", "он"), ("jn", "от"), ("gj", "по")] {
            for context in [&[][..], &["us", "us"][..]] {
                let decision = decide(typed, &profiles, 0, context);
                assert_eq!((decision.convert, decision.text.as_str()), (true, moved), "{typed} {context:?}");
            }
        }
        assert!(!decide("tv", &profiles, 0, &["us", "us"]).convert);
        assert!(!decide("vs", &profiles, 0, &["us", "us"]).convert);
        // An English word of the table stays.
        assert!(!decide("is", &profiles, 0, &["us", "us"]).convert);
    }

    #[test]
    fn converts_a_mistyped_russian_word_by_dictionary() {
        let (us, ru) = (us(), ru());
        let decision = decide("ghbdtn", &[&us, &ru], 0, &[]);
        assert!(decision.convert);
        assert_eq!(decision.text, "привет");
        assert_eq!(decision.stage, Stage::Dictionary);
    }

    /// The single case where the discrimination rule parts company with the
    /// shipped one: neither reading is forbidden at bigram level. Today that
    /// ends the cascade with `keep`, and the trigram tables — an order of
    /// magnitude denser than the bigram ones — are never consulted at all.
    #[test]
    fn a_level_that_separates_nothing_defers_to_the_next_one() {
        let us = Profile::new("us", alphabet("us"), &[], &["bdt"]);
        let ru = Profile::new("ru", alphabet("ru"), &[], &[]);

        // Neither dictionary knows the word, so the n-gram stages decide it.
        let old = decide_with(
            "ghbdtn",
            &[&us, &ru],
            0,
            &[],
            &Thresholds {
                ngram_discrimination: false,
                ..Thresholds::DEFAULT
            },
        );
        assert!(!old.convert);
        assert_eq!(old.stage, Stage::Bigram);

        let shipped = decide("ghbdtn", &[&us, &ru], 0, &[]);
        assert!(shipped.convert);
        assert_eq!(shipped.text, "привет");
        assert_eq!(shipped.stage, Stage::Trigram);
    }

    /// `рук` against `her`, `дев` against `ltd`: both readings are words, and
    /// the text before decides — Russian before a Russian word keeps it however
    /// common the English shadow; English before it moves it; nothing before
    /// it leaves the call to frequency.
    #[test]
    fn the_language_of_the_text_before_settles_a_collision() {
        let mut us = us();
        us.words.push("her".into());
        us.frequencies.insert("her".into(), 20_000);
        let mut ru = ru();
        ru.words.push("рук".into());
        ru.frequencies.insert("рук".into(), 300);
        let profiles = [&us, &ru];
        assert!(!decide("рук", &profiles, 1, &["ru", "ru"]).convert);
        assert!(decide("рук", &profiles, 1, &["us", "us"]).convert);
        let alone = decide("рук", &profiles, 1, &[]);
        assert!(alone.convert, "frequency alone: 20 000 against 300");
        assert!(alone.doubtful, "frequency alone is a verdict the next word may take back");
        assert!(!decide("рук", &profiles, 1, &["ru", "ru"]).doubtful);
        assert!(!decide("her", &profiles, 0, &["ru", "ru"]).doubtful);
        assert!(decide("her", &profiles, 0, &["ru", "ru"]).convert);
        assert!(!decide("her", &profiles, 0, &["us", "us"]).convert);
        // A reading hundreds of times rarer than the typed word is no target
        // even in agreeing context: `are` is not `фку`.
        us.words.push("are".into());
        us.frequencies.insert("are".into(), 66_143);
        ru.words.push("фку".into());
        ru.frequencies.insert("фку".into(), 13);
        let profiles = [&us, &ru];
        assert!(!decide("are", &profiles, 0, &["ru", "ru"]).convert);
        // One English word inside Russian text is not a context: frequency rules.
        assert!(decide("рук", &profiles, 1, &["us", "ru"]).convert, "margin: 20 000 against 300");
        assert!(!decide("her", &profiles, 0, &["ru", "us"]).convert);
    }

    /// `HBF` after English text is an English acronym nobody has listed, not
    /// `РИА`; the same shadow with no text before it, or Russian text, still
    /// moves, and a listed abbreviation moves anywhere.
    #[test]
    fn an_unknown_acronym_inside_english_text_is_left_alone() {
        let us = us();
        let mut ru = ru();
        // The test profile does not lower-case as the real one does.
        ru.words.push("РИА".into());
        ru.frequencies.insert("РИА".into(), 500);
        let profiles = [&us, &ru];
        assert!(!decide("HBF", &profiles, 0, &["us", "us"]).convert);
        assert!(decide("HBF", &profiles, 0, &[]).convert, "no text before it: the dictionary stays the judge");
        assert!(decide("HBF", &profiles, 0, &["ru", "ru"]).convert);
        assert!(decide("HBF", &profiles, 0, &["us", "ru"]).convert, "one English word is not a context");
        // The other direction is the ordinary case: `ISBN` typed on the Russian
        // layout inside Russian text.
        let mut us = us;
        us.words.push("ISBN".into());
        us.frequencies.insert("ISBN".into(), 500);
        let profiles = [&us, &ru];
        assert!(decide("ШЫИТ", &profiles, 1, &["ru", "ru"]).convert);
    }

    /// `it.` with the full stop taken in as a possible `ю`: the typed reading
    /// is judged as `it`, so against `шею` this is a collision and English
    /// context keeps it.
    #[test]
    fn a_trailing_key_that_is_a_letter_elsewhere_does_not_hide_the_typed_word() {
        let mut us = us();
        us.words.push("it".into());
        us.frequencies.insert("it".into(), 80_000);
        let mut ru = ru_mac();
        ru.words.push("шею".into());
        ru.frequencies.insert("шею".into(), 75);
        let profiles = [&us, &ru];
        let decision = decide("it.", &profiles, 0, &["us", "us"]);
        assert!(!decision.convert, "{decision:?}");
        assert_eq!(decision.stage, Stage::Dictionary);
        // Russian context alone does not move it either: `it` is a thousand
        // times the word `шею` is (COLLISION_CONTEXT_RATIO).
        assert!(!decide("it.", &profiles, 0, &["ru", "ru"]).convert);
    }

    /// The reading a forbidden bigram would send the word to must clear the
    /// trigrams as well: `Санькя` fails `кя`, and `cfymrz` clears the bigram
    /// table only to fail `fym` — so nothing moves.
    #[test]
    fn a_target_the_trigrams_reject_is_not_moved_to() {
        let us = Profile::new("us", alphabet("us"), &[], &["fym"]);
        let ru = Profile::new("ru", alphabet("ru"), &[], &["кя"]);
        let decision = decide("Санькя", &[&us, &ru], 1, &["ru", "ru"]);
        assert!(!decision.convert, "{decision:?}");
        // Typed on the wrong layout after words in the target language, one
        // clean level still moves it: names carry trigrams the tables lack.
        let ru = Profile::new("ru", alphabet("ru"), &[], &["кя"]);
        let us = Profile::new("us", alphabet("us"), &[], &["gh", "вет"]);
        assert!(decide("ghbdtn", &[&us, &ru], 0, &["ru", "ru"]).convert);
    }

    /// The closed list of Latin abbreviations: `шз` is `ip`, with every guard.
    #[test]
    fn a_listed_latin_abbreviation_typed_on_the_russian_layout_is_converted() {
        let us = Profile::new("us", alphabet("us"), &[], &[]);
        let ru = Profile::new("ru", alphabet("ru"), &["адрес", "порт", "ем"], &[]);
        let profiles = [&us, &ru];
        for (typed, reading) in [("шз", "ip"), ("ШЗ", "IP"), ("ыйд", "sql"), ("ФЗШ", "API"), ("афй", "faq"), ("АФЙ", "FAQ"), ("шзю", "ip."), ("ШЗб", "IP,")] {
            let decision = decide(typed, &profiles, 1, &[]);
            assert_eq!((decision.convert, decision.text.as_str(), decision.stage), (true, reading, Stage::Abbreviation), "{typed}: {decision:?}");
        }
        // One case only, and a word of the typed language stays a word.
        assert!(!decide("Шз", &profiles, 1, &[]).convert);
        assert!(!decide("ем", &profiles, 1, &["us", "us"]).convert, "`ем` is `tv` and a Russian word");
        // Not on the list: nothing to say.
        assert!(!decide("щщ", &profiles, 1, &[]).convert);
        // Russian look-alikes move only between two English words.
        assert!(!decide("ФК", &profiles, 1, &[]).convert);
        assert!(!decide("ФК", &profiles, 1, &["ru", "ru"]).convert);
        assert!(!decide("ФК", &profiles, 1, &["us"]).convert, "one English word is not a context");
        assert!(!decide("фк", &profiles, 1, &["us", "us"]).convert, "look-alikes: capitals only");
        let decision = decide("ФК", &profiles, 1, &["us", "us"]);
        assert_eq!((decision.convert, decision.text.as_str()), (true, "AR"), "{decision:?}");
        // Typed the right way round nothing happens.
        assert!(!decide("ip", &profiles, 0, &[]).convert);
    }

    /// `pc`, `pr` and `ui` are written the same way everywhere: `зс`, `зк` and `гш`
    /// are always the keys of these, even though `ЗС` is also a Russian abbreviation.
    #[test]
    fn pc_pr_and_ui_typed_on_the_russian_layout_always_move() {
        let us = Profile::new("us", alphabet("us"), &[], &[]);
        let ru = Profile::new("ru", alphabet("ru"), &["зс", "зк", "гш"], &[]);
        let profiles = [&us, &ru];
        for (typed, reading) in [("зс", "pc"), ("ЗС", "PC"), ("зк", "pr"), ("ЗК", "PR"), ("гш", "ui"), ("ГШ", "UI")] {
            for context in [&[][..], &["ru", "ru"][..], &["us", "us"][..]] {
                let decision = decide(typed, &profiles, 1, context);
                assert_eq!((decision.convert, decision.text.as_str()), (true, reading), "{typed} {context:?}: {decision:?}");
            }
        }
        // Typed the right way round it stays.
        assert!(!decide("pc", &profiles, 0, &[]).convert);
        assert!(!decide("PC", &profiles, 0, &["ru", "ru"]).convert);
    }

    /// `шз-адрес`: the abbreviation moves, the Russian word does not.
    #[test]
    fn a_latin_abbreviation_before_a_hyphen_and_a_russian_word_is_a_compound() {
        let us = Profile::new("us", alphabet("us"), &[], &[]);
        let ru = Profile::new("ru", alphabet("ru"), &["адрес", "порт", "диск", "русски"], &[]);
        let profiles = [&us, &ru];
        assert_eq!(latin_prefix_compound("шз-адрес", &profiles, 1, &[]), Some((2, "us")));
        assert_eq!(latin_prefix_compound("ШЗ-адрес", &profiles, 1, &["ru", "ru"]), Some((2, "us")));
        assert_eq!(latin_prefix_compound("ыыв-диск", &profiles, 1, &[]), Some((3, "us")));
        assert_eq!(decide("шз-адрес", &profiles, 1, &[]).stage, Stage::Compound);
        assert!(!decide("шз-адрес", &profiles, 1, &[]).convert, "the whole token is never converted");
        // The right half must be words; the left a listed abbreviation, no word.
        assert_eq!(latin_prefix_compound("шз-адреска", &profiles, 1, &[]), None);
        assert_eq!(latin_prefix_compound("шз-1", &profiles, 1, &[]), None);
        assert_eq!(latin_prefix_compound("по-русски", &profiles, 1, &[]), None);
        assert_eq!(latin_prefix_compound("жк-банк", &profiles, 1, &[]), None, "`ЖК` is not on the list");
        assert_eq!(latin_prefix_compound("ем-адрес", &profiles, 1, &["us", "us"]), None, "`ем` is a word");
        assert_eq!(latin_prefix_compound("-адрес", &profiles, 1, &[]), None);
        // A look-alike needs English text before it.
        assert_eq!(latin_prefix_compound("ЗС-адрес", &profiles, 1, &[]), None);
        assert_eq!(latin_prefix_compound("ЗС-адрес", &profiles, 1, &["us", "us"]), Some((2, "us")));
    }

    /// A reading with punctuation inside is no word: `Вуджек` fails the Russian
    /// trigram `удж`, and its Latin shadow `Del;tr` carries a `;` that breaks
    /// every window the English tables would have condemned.
    #[test]
    fn a_target_with_punctuation_inside_is_not_a_word() {
        let us = Profile::new("us", alphabet("us"), &[], &[]);
        let ru = Profile::new("ru", alphabet("ru"), &[], &["удж"]);
        let decision = decide("Вуджек", &[&us, &ru], 1, &[]);
        assert!(!decision.convert, "{decision:?}");
        assert!(!decide("Вуджек", &[&us, &ru], 1, &["ru", "ru"]).convert);
        // Punctuation at the end is the delimiter taken for a letter, not a
        // mark inside the word: `Утукпнб` is `Energy,` and moves.
        let ru = Profile::new("ru", alphabet("ru"), &[], &["пн"]);
        let decision = decide("Утукпнб", &[&us, &ru], 1, &["us", "us"]);
        assert_eq!((decision.convert, decision.text.as_str()), (true, "Energy,"), "{decision:?}");
    }

    /// Impossible in both languages is not evidence for either: leave it alone.
    #[test]
    fn a_word_forbidden_everywhere_is_left_alone() {
        let us = Profile::new("us", alphabet("us"), &[], &["gh"]);
        let ru = Profile::new("ru", alphabet("ru"), &[], &["пр"]);

        let decision = decide_with(
            "ghbdtn",
            &[&us, &ru],
            0,
            &[],
            &Thresholds {
                ngram_discrimination: true,
                ..Thresholds::DEFAULT
            },
        );
        assert!(!decision.convert);
        assert_eq!(decision.stage, Stage::Undecided);
    }

    /// Both mistakes this replaces, in one test. `[` after `Rfpf` is the `х` of
    /// `Казахстана` and the word must go on; `.` after `ntxtybt` is a full stop
    /// and the word must end, or `течение.` and the word after it become the
    /// single token `течениеюскфае`.
    #[test]
    fn the_dictionary_says_whether_punctuation_is_a_letter() {
        let us = Profile::new("us", alphabet("us"), &[], &[]);
        let ru = Profile::new(
            "ru",
            alphabet("ru"),
            &["казахстана", "изъятие", "течение"],
            &[],
        );
        let profiles = [&us, &ru];

        // Rfpf + [ reads as `Казах`, which starts a word we know.
        assert!(punctuation_continues_word("Rfpf", '[', &profiles, 0));
        // bp + ] reads as `изъ` — not a word, but the start of one.
        assert!(punctuation_continues_word("bp", ']', &profiles, 0));
        // ntxtybt + . reads as `течениею`, which starts nothing.
        assert!(!punctuation_continues_word("ntxtybt", '.', &profiles, 0));
    }

    /// `сдерживающую` typed on `us` ends on the full stop key, and the
    /// dictionary has only `сдерживающего`: the stem is known under another
    /// ending, so the key is the letter `ю`. A known word before the key
    /// (`течение`) or an unknown stem is still a full stop.
    #[test]
    fn an_unknown_form_of_a_known_stem_keeps_its_last_letter() {
        let us = Profile::new("us", alphabet("us"), &[], &[]);
        let ru = Profile::new(
            "ru",
            alphabet("ru"),
            &["сдерживающего", "течение", "гильбертового", "функция"],
            &[],
        );
        let profiles = [&us, &ru];
        // clth;bdf.oe + . reads as `сдерживающую`.
        assert!(punctuation_continues_word("clth;bdf.oe", '.', &profiles, 0));
        // ubkm,thnjds + [ reads as `гильбертовых`.
        assert!(punctuation_continues_word("ubkm,thnjds", '[', &profiles, 0));
        // aeyrwb + . reads as `функцию`.
        assert!(punctuation_continues_word("aeyrwb", '.', &profiles, 0));
        // `течение` is a word of its own: its full stop stays a full stop.
        assert!(!punctuation_continues_word("ntxtybt", '.', &profiles, 0));
        // An unknown stem (`ghjcnj` → `просто` is not in this dictionary).
        assert!(!punctuation_continues_word("ghjcnj", '.', &profiles, 0));
        // A stem too short to mean anything: `rhf` + . → `крфю`.
        assert!(!punctuation_continues_word("rhf", '.', &profiles, 0));
    }

    /// A key that is punctuation on every layout is never the dictionary's
    /// business, however promising the prefix looks.
    #[test]
    fn a_key_that_is_punctuation_everywhere_is_not_asked_about() {
        let us = Profile::new("us", alphabet("us"), &[], &[]);
        let ru = Profile::new("ru", alphabet("ru"), &["казахстана"], &[]);
        // `-` is a hyphen on both layouts: the reading ends in a non-letter and
        // the prefix is never consulted.
        assert!(!punctuation_continues_word("Rfpf", '-', &[&us, &ru], 0));
    }

    #[test]
    fn keeps_a_correctly_typed_word() {
        let (us, ru) = (us(), ru());
        let decision = decide("hello", &[&us, &ru], 0, &[]);
        assert!(!decision.convert);
        assert_eq!(decision.stage, Stage::Dictionary);
    }

    #[test]
    fn stop_rules_win_over_everything() {
        let (us, ru) = (us(), ru());
        for (word, reason) in [
            ("Qw3rty!x", StopReason::Password),
            ("http://example.ru", StopReason::Url),
            ("std::vector", StopReason::Code),
        ] {
            let decision = decide(word, &[&us, &ru], 0, &[]);
            assert!(!decision.convert, "{word}");
            assert_eq!(decision.stage, Stage::StopRule(reason), "{word}");
        }
    }

    #[test]
    fn single_letter_converts_only_with_agreeing_context() {
        let (us, ru) = (us(), ru());
        let with_context = decide("b", &[&us, &ru], 0, &["ru"]);
        assert!(with_context.convert);
        assert_eq!(with_context.text, "и");
        assert_eq!(with_context.stage, Stage::ShortWord);

        let without_context = decide("b", &[&us, &ru], 0, &[]);
        assert!(!without_context.convert);
    }

    #[test]
    fn lone_punctuation_is_never_a_word() {
        let (us, ru) = (us(), ru());
        // Found in live testing: `,` is `б` on the Russian layout, and with a
        // Russian context the short-word stage happily converted it.
        for mark in [",", ".", ";", "'", "/"] {
            let decision = decide(mark, &[&us, &ru], 0, &["ru", "ru"]);
            assert!(!decision.convert, "{mark} must stay punctuation");
        }
        // A comma that actually starts a word still converts.
        assert!(decide(",tufnm", &[&us, &ru], 0, &["ru"]).convert);
    }

    #[test]
    fn single_letter_in_foreign_context_is_left_alone() {
        let (us, ru) = (us(), ru());
        let decision = decide("c", &[&us, &ru], 0, &["us"]);
        assert!(!decision.convert);
    }

    #[test]
    fn falls_through_to_ngrams_for_unknown_words() {
        let (us, ru) = (us(), ru());
        // Not in any dictionary; "ghb" is clean for us bigrams but "gh" is not.
        let decision = decide("ghbdtnrf", &[&us, &ru], 0, &[]);
        assert!(decision.convert);
        assert_eq!(decision.layout_id, "ru");
    }

    #[test]
    fn regex_rules_outrank_dictionaries() {
        let us = us().with_rule("hello");
        let ru = ru();
        let decision = decide("hello", &[&us, &ru], 0, &[]);
        assert_eq!(decision.stage, Stage::Rule);
        assert!(!decision.convert);
    }
}
