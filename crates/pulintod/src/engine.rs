//! Observe-mode event loop.
//!
//! Watches the key stream without holding it. The delimiter has already reached
//! the application by the time we see it, so a correction here steps back over
//! it — which is the one thing grab mode does not have to do, and the only
//! difference between the two paths that is real.
//!
//! Everything else is now shared with `grab_engine.rs`, and deliberately so.
//! This file used to keep the *old* replacement mechanic: it stored characters,
//! computed the target text and typed it with `send_text_for_layout` straight
//! after `toggle_next()`, with no pause. The layout change travels over the X11
//! socket while the keys go through a kernel device — independent channels
//! — so the keys arrived before the layout did and were swallowed,
//! while the erase went through: `cktlbnm` came back as `cktlbn`. Storing scan
//! codes and replaying them with the three delays of the timing profile is what
//! `grab_engine.rs` has always done, and it is what this does now.
//!
//! The second half of the same defect was the word itself. Characters were
//! derived from a fixed US table, so the buffer was Latin whatever the user was
//! typing, and `assume_layout` then dutifully answered "us" every time. Scan
//! codes are read through the layout that is actually in effect
//! (`scanword::current_profile`), which is the whole point of storing them.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use pulinto_core::cascade;
use pulinto_core::layout::Layout;
use pulinto_data::profile::{self, Profile};
use pulinto_input::injector::Injector;
use pulinto_layout::LayoutController;

use crate::hotkeys::{Action, Modifier, TapRelease, TapTracker};
use crate::replace::{self, Cursor, Fix, Learning};
use crate::scanword::{self, ScanWord};
use crate::SwitcherState;

/// Time for the application to finish with the delimiter it just received,
/// before we start editing the text in front of it.
const POST_DELIMITER_MS: u64 = 20;

/// How still the keyboard must be before a correction may start, and how long
/// we are willing to wait for that stillness.
///
/// Observe mode holds no grab, so anything the user types during a replacement
/// reaches the application while we are busy — and an erase issued into that
/// stream takes out the new text instead of the old. Measured on the rig: at
/// 200 ms between words, 1 word in 60 survived; at 750 ms, 59 in 60.
///
/// Nothing is *lost* while we wait — keystrokes go into `Pending` and are
/// processed afterwards — but a correction still has to land on the text it
/// was computed against, so it waits for a gap. One that never finds a gap is
/// dropped: a missed correction is a nuisance, eaten text is not.
///
/// Giving up has to be *fast*, and that is the whole of the second number.
///
/// It used to be 1500 ms, on the reasoning that a correction is worth waiting
/// for. It is not, and the wait was self-defeating: a second and a half spent
/// discovering that the user is still typing leaves the daemon two words further
/// behind, so the next correction fails for the same reason, and the one after
/// that. Measured on the rig at 600 ms a word: **45 of 63 corrections dropped**,
/// and the same 45 at 300 ms — the rate did not depend on typing speed, which is
/// the signature of a queue that never catches up rather than one that is
/// genuinely overrun.
///
/// Failing in a quarter of a second lets the daemon rejoin the stream at the
/// next word instead of falling out of it for good.
const QUIET_MS: u64 = 140;
const QUIET_GIVE_UP_MS: u64 = 250;

/// Keystrokes waiting their turn.
///
/// The daemon used to *discard* everything typed while it was injecting a
/// correction — a flag said "busy", and the handler returned. That is why the
/// user's English words vanished from the log: they were typed during a
/// replacement, reached the application (nothing holds them back in observe
/// mode) and the daemon never learned they existed, so it could not have
/// corrected them.
///
/// Now a reader thread does nothing but take events off the kernel and put
/// them here, while a second thread processes them at whatever speed it can.
/// When a correction takes 400 ms, 400 ms of typing accumulates and is then
/// worked through in order. The daemon falls behind and catches up; the queue
/// draining to empty is what "caught up" means. Nothing is lost either way.
#[derive(Debug)]
pub struct Pending {
    queue: Mutex<VecDeque<pulinto_input::capture::KeyEvent>>,
    arrived: Condvar,
    /// When the last keystroke came in. The stillness a correction waits for is
    /// measured against this, not against the queue — see `wait_for_quiet`.
    last_arrival: Mutex<std::time::Instant>,
}

impl Default for Pending {
    fn default() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            arrived: Condvar::new(),
            last_arrival: Mutex::new(std::time::Instant::now()),
        }
    }
}

impl Pending {
    pub fn push(&self, event: pulinto_input::capture::KeyEvent) {
        if let Ok(mut arrival) = self.last_arrival.lock() {
            *arrival = std::time::Instant::now();
        }
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(event);
            self.arrived.notify_all();
        }
    }

    /// Key presses waiting to be handled — the ones that have already put text
    /// on the screen. Releases do not count: they change nothing.
    fn queued_presses(&self) -> usize {
        self.queue
            .lock()
            .map(|queue| queue.iter().filter(|event| event.pressed).count())
            .unwrap_or(0)
    }

    fn since_last_arrival(&self) -> Duration {
        self.last_arrival
            .lock()
            .map(|arrival| arrival.elapsed())
            .unwrap_or_default()
    }

    /// Next event, waiting for one if the queue is empty.
    fn pop(&self) -> Option<pulinto_input::capture::KeyEvent> {
        let mut queue = self.queue.lock().ok()?;
        loop {
            if let Some(event) = queue.pop_front() {
                return Some(event);
            }
            queue = self.arrived.wait(queue).ok()?;
        }
    }

    /// Block until the keyboard has been still for `quiet_ms`, or give up after
    /// `give_up_ms`. False means the correction must be abandoned.
    ///
    /// This used to wait for the **queue** to drain, which could not work: the
    /// queue is drained by the processing thread, and the processing thread is
    /// the one standing here. One event left in it — and the release of the
    /// delimiter that ended the word is always left in it — and the queue could
    /// never empty, because nobody was left to empty it. Measured on the rig:
    /// **every** correction in observe mode was dropped, 57 out of 57, and the
    /// rate did not move with typing speed. A wait whose condition only the
    /// waiter could satisfy.
    ///
    /// The condition that was meant all along is that the user has stopped
    /// typing, so that is what is asked now: how long since the last keystroke
    /// arrived. Queued *presses* are a separate and stronger objection — they
    /// are text already on the screen, so the erase would land on the wrong
    /// characters — and they abandon the correction outright rather than wait.
    fn wait_for_quiet(&self, quiet_ms: u64, give_up_ms: u64) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_millis(give_up_ms);
        loop {
            if self.queued_presses() > 0 {
                return false;
            }
            if self.since_last_arrival() >= Duration::from_millis(quiet_ms) {
                return true;
            }
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// How fast the user is going, measured rather than assumed.
///
/// Observe mode cannot hold anything back, so a correction races the typing
/// that follows it. Waiting for stillness is not enough: stillness at the
/// moment we look says nothing about the next 300 milliseconds, and that is
/// exactly how long the correction takes. Measured on the rig at 300 ms a word,
/// with the wait fixed and corrections finally going through: 39 words of the
/// 120 came out mangled — worse than not correcting at all.
///
/// So the question asked is not "is it quiet" but "will this fit". The gap
/// between word boundaries is what the user's pace actually is, and a
/// correction longer than the gap is declined. A missed correction costs a
/// hotkey press; eaten text costs trust.
#[derive(Debug)]
pub struct Rhythm {
    last_boundary: Option<std::time::Instant>,
    /// Exponential average of the gap between words, in milliseconds.
    gap_ms: f64,
}

impl Default for Rhythm {
    fn default() -> Self {
        Self {
            last_boundary: None,
            // Until there is evidence, assume a comfortable pace rather than a
            // hurried one: declining every correction until the second word
            // would be its own kind of wrong.
            gap_ms: 700.0,
        }
    }
}

impl Rhythm {
    /// `at` is when the delimiter **arrived**, not when we got round to it.
    fn note_boundary(&mut self, now: std::time::Instant) {
        if let Some(previous) = self.last_boundary {
            let gap = now.saturating_duration_since(previous).as_millis() as f64;
            // Long pauses are thinking, not typing, and must not persuade us
            // that there is room for anything.
            if gap < 5000.0 {
                self.gap_ms = self.gap_ms * 0.7 + gap * 0.3;
            }
        }
        self.last_boundary = Some(now);
    }

    /// Room for a correction of this length, with a fifth of the gap to spare.
    fn fits(&self, estimate_ms: u64) -> bool {
        (estimate_ms as f64) < self.gap_ms * 0.8
    }
}

/// All the context the event handler needs, captured once at startup.
pub struct EventContext {
    pub state: Arc<SwitcherState>,
    pub injector: Arc<Injector>,
    pub layout: Arc<Box<dyn LayoutController>>,
    /// Language profiles feeding the cascade, in the order it expects them.
    pub profiles: Arc<Vec<Profile>>,
    /// The word being typed, as physical keys. Characters would have to be
    /// guessed from a layout, and guessing that is exactly what went wrong.
    pub word: Mutex<ScanWord>,
    /// Modifiers the user is physically holding, so injection can lift them
    /// and put them back.
    pub held_modifiers: Mutex<Vec<u16>>,
    pub shift_down: AtomicBool,
    /// Layouts of the last two completed words. Stage 2 of the cascade cannot
    /// judge a one- or two-letter word without them.
    pub recent_langs: Mutex<Vec<&'static str>>,
    /// Reverted words and the last correction — the same instance grab mode
    /// uses, so what the user teaches in one mode holds in the other.
    pub learning: Arc<Learning>,
    /// Keystrokes that arrived while the daemon was busy. Never dropped.
    pub pending: Arc<Pending>,
    pub tap: Mutex<TapTracker>,
    /// Recent decisions, kept so the user can point at one that went wrong.
    pub reports: Arc<crate::report::Registry>,
    /// Connection used to ask which application has the keyboard. `None` when
    /// there is no X server to ask.
    pub windows: Option<crate::xwindow::XWindows>,
    /// The user's measured typing pace, for deciding whether a correction fits.
    pub rhythm: Mutex<Rhythm>,
    /// Diagnostics mode: decide everything, inject nothing.
    pub dry_run: bool,
}

impl EventContext {
    fn held(&self) -> Vec<u16> {
        self.held_modifiers.lock().unwrap().clone()
    }
}

/// Work through the queue, for as long as there is a queue.
pub fn process_pending(ctx: &Arc<EventContext>) {
    while let Some(event) = ctx.pending.pop() {
        handle_capture_event(ctx, event);
    }
}

/// Process one key transition read straight from `/dev/input`.
pub fn handle_capture_event(ctx: &Arc<EventContext>, event: pulinto_input::capture::KeyEvent) {
    use pulinto_input::capture::{self, Key};

    // Modifier bookkeeping runs before anything else and regardless of mode:
    // injection has to know what the user is holding, and Shift decides what
    // the strokes of this word mean.
    if capture::is_modifier(event.key) {
        let code = event.key.code();
        if matches!(event.key, Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT) {
            ctx.shift_down.store(event.pressed, Ordering::SeqCst);
        }
        let mut held = ctx.held_modifiers.lock().unwrap();
        if event.pressed {
            if !held.contains(&code) {
                held.push(code);
            }
        } else {
            held.retain(|candidate| *candidate != code);
        }
    }

    // Which set this keyboard lives by (Apple or PC) is decided per key, so an
    // edit in the settings window applies at once and two keyboards differ.
    let keys = ctx.state.hotkeys.set_for(event.vendor);

    // Taps run even while auto-switch is off: the hotkeys are manual.
    if Modifier::of_code(event.key.code()).is_some() {
        let code = event.key.code();
        let mut tap = ctx.tap.lock().unwrap();
        if event.pressed {
            let others_held = ctx
                .held_modifiers
                .lock()
                .map(|held| held.iter().any(|other| *other != code))
                .unwrap_or(false);
            if keys.tap_action(code).is_some() {
                tap.press(code, others_held, 0);
            } else {
                tap.other_key();
            }
        } else if tap.release(code, 0) == TapRelease::Tap {
            drop(tap);
            if let Some(action) = keys.tap_action(code) {
                fire(ctx, action);
            }
        }
        return;
    }
    if event.pressed {
        ctx.tap.lock().unwrap().other_key();
    }

    if event.pressed {
        if let Some(action) = keys.chord_action(event.key.code(), &ctx.held()) {
            fire(ctx, action);
            return;
        }
    }

    if !event.pressed || capture::is_modifier(event.key) {
        return;
    }
    // The cursor moved; what we believe sits to its left is no longer true, so
    // the word is abandoned rather than corrected. Grab mode has always done
    // this; observe mode did not, and would happily "correct" a word the user
    // had already navigated away from.
    if pulinto_input::grab::is_navigation(event.key) {
        ctx.learning.invalidate_last();
        ctx.word.lock().unwrap().clear();
        ctx.recent_langs.lock().unwrap().clear();
        return;
    }

    // Enter and Tab end the word but never trigger a correction — by then the
    // line is already submitted.
    if matches!(event.key, Key::KEY_ENTER | Key::KEY_KPENTER | Key::KEY_TAB) {
        ctx.learning.invalidate_last();
        ctx.word.lock().unwrap().clear();
        ctx.recent_langs.lock().unwrap().clear();
        return;
    }

    let shift = ctx.shift_down.load(Ordering::SeqCst);
    match event.key {
        Key::KEY_BACKSPACE => {
            ctx.learning.invalidate_last();
            ctx.word.lock().unwrap().pop();
        }
        Key::KEY_ESC => {
            ctx.learning.invalidate_last();
            ctx.word.lock().unwrap().clear();
        }
        Key::KEY_SPACE => process_word_boundary(ctx, (event.key.code(), false), event.at),
        key => {
            let code = key.code();
            // Letters are letters on both layouts, so the common case costs
            // nothing. Everything else has to be asked about, because the same
            // key is a comma here and the letter `б` there.
            let letter = capture::key_to_char(key).is_some_and(|ch| ch.is_ascii_alphabetic());
            if letter {
                ctx.learning.invalidate_last();
                ctx.word.lock().unwrap().push(code, shift);
                return;
            }
            let current = {
                let word = ctx.word.lock().unwrap();
                scanword::current_profile(ctx.layout.as_ref().as_ref(), &ctx.profiles, &word)
            };
            let here = ctx.profiles[current].layout();
            let others = ctx
                .profiles
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != current)
                .map(|(_, profile)| profile.layout());

            match scanword::classify(here, others, code, shift) {
                scanword::KeyRole::Letter => {
                    ctx.learning.invalidate_last();
                    ctx.word.lock().unwrap().push(code, shift);
                }
                scanword::KeyRole::Ambiguous(ch) => {
                    if ambiguous_is_letter(ctx, current, ch) {
                        ctx.learning.invalidate_last();
                        ctx.word.lock().unwrap().push_ambiguous(code, shift);
                    } else {
                        process_word_boundary(ctx, (code, shift), event.at);
                    }
                }
                scanword::KeyRole::Separator => process_word_boundary(ctx, (code, shift), event.at),
                scanword::KeyRole::Other => ctx.learning.invalidate_last(),
            }
        }
    }
}

/// Is the keyboard in an application we have been told to leave alone?
///
/// Costs one X round trip per completed word, which is nothing next to the
/// injection it may prevent. Asked at the word boundary rather than cached,
/// because the answer changes the moment the user switches window — and getting
/// that wrong means correcting text inside a password field.
fn automatic_disabled_for_application(ctx: &Arc<EventContext>) -> bool {
    !crate::focused_app_allows_automatic(&ctx.state, ctx.windows.as_ref())
}

fn manual_disabled_for_application(ctx: &Arc<EventContext>) -> bool {
    !crate::focused_app_allows_manual(&ctx.state, ctx.windows.as_ref())
}

/// Ask the dictionary whether the key that just drew punctuation is really a
/// letter of this word. See `cascade::punctuation_continues_word`.
fn ambiguous_is_letter(ctx: &Arc<EventContext>, current: usize, ch: char) -> bool {
    let so_far = {
        let word = ctx.word.lock().unwrap();
        word.text(ctx.profiles[current].layout())
    };
    let refs = profile::refs(&ctx.profiles);
    cascade::punctuation_continues_word(&so_far, ch, &refs, current)
}

fn safe_observe_delimiter(
    ctx: &Arc<EventContext>,
    key: (u16, bool),
    shown: usize,
) -> Option<replace::WordDelimiter> {
    if key.0 == pulinto_input::injector::KEY_SPACE {
        return Some(replace::WordDelimiter { key, ch: ' ' });
    }
    let position = pulinto_input::injector::position_of_keycode(key.0)?;
    let level = if key.1 {
        pulinto_core::layout::Level::Shifted
    } else {
        pulinto_core::layout::Level::Unshifted
    };
    let ch = ctx.profiles[shown].layout().char_at(position, level)?;
    Some(replace::WordDelimiter { key, ch })
}

fn remember_observed_word(
    ctx: &Arc<EventContext>,
    strokes: Vec<(u16, bool)>,
    original_text: String,
    shown: usize,
    delimiter: (u16, bool),
) {
    ctx.learning.remember(Fix {
        strokes,
        original: original_text,
        shown,
        delimiter: safe_observe_delimiter(ctx, delimiter, shown),
    });
}

/// Called when the user finishes a word (Space or punctuation).
fn process_word_boundary(ctx: &Arc<EventContext>, delimiter: (u16, bool), at: std::time::Instant) {
    ctx.rhythm.lock().unwrap().note_boundary(at);
    let mut word = ctx.word.lock().unwrap();
    if word.is_empty() {
        drop(word);
        // One more delimiter means the cursor is already past the saved word.
        // The cycle must go out, or it would erase only the last of the two
        // spaces and start editing the wrong range.
        ctx.learning.invalidate_last();
        return;
    }

    let current = scanword::current_profile(ctx.layout.as_ref().as_ref(), &ctx.profiles, &word);
    let shown = word.shown_profile().unwrap_or(current);
    let original = word.original_profile().unwrap_or(current);
    let text = word.text(ctx.profiles[shown].layout());
    let original_text = word.text(ctx.profiles[original].layout());
    let already_converted = word.already_converted();
    let plan = scanword::plan_word(
        &word,
        &profile::refs(&ctx.profiles),
        current,
        &ctx.recent_langs.lock().unwrap().clone(),
    );
    let strokes = word.replay_keys();
    // Every reading, captured while the word is still assembled: a report is
    // useless without the alternative the cascade weighed against.
    let readings: Vec<(String, String)> = ctx
        .profiles
        .iter()
        .map(|profile| (profile.layout_id().to_string(), word.text(profile.layout())))
        .collect();
    word.clear();
    drop(word);

    if text.is_empty() {
        ctx.learning.invalidate_last();
        return;
    }

    // A manual Alt up to the delimiter has already set the shown reading. The
    // cascade is not applied to it a second time; all that is left is to keep a
    // safe position for the next Alt step after the space.
    if already_converted || !ctx.state.auto_enabled.load(Ordering::SeqCst) {
        remember_observed_word(ctx, strokes, original_text, shown, delimiter);
        return;
    }

    // What the daemon believes, printed before it acts on it.
    //
    // A misread layout is silent by construction: the word comes out as a
    // perfectly ordinary word of the other language, the cascade rightly
    // leaves it alone, and nothing is logged. The only way to catch that is to
    // print what was read *and under what*, on every word, not just the ones
    // that changed.
    let seen = Seen {
        layout: ctx.layout.current_layout_id(),
        group: ctx.layout.get_current_group().ok(),
        loaded: ctx.layout.layout_ids(),
        profile: ctx.profiles[current].layout_id(),
        text: text.clone(),
    };

    // Whole applications come before individual words: a password manager is
    // not somewhere to be clever about layouts, and on Linux nothing else will
    // stop us reading its keystrokes.
    if automatic_disabled_for_application(ctx) {
        remember_observed_word(ctx, strokes, original_text, shown, delimiter);
        return;
    }

    let focused_class = ctx
        .windows
        .as_ref()
        .and_then(crate::xwindow::XWindows::focused_class);

    let exceptions = ctx.state.exceptions.blocking_lock();
    if exceptions.is_word_excluded_in(&text, focused_class.as_deref()) {
        drop(exceptions);
        remember_observed_word(ctx, strokes, original_text, shown, delimiter);
        return;
    }
    if ctx.learning.is_reverted(&text) {
        drop(exceptions);
        remember_observed_word(ctx, strokes, original_text, shown, delimiter);
        return;
    }

    // A personal exact replacement may include the punctuation key which
    // ended the scanner token. In observe mode that character is already on
    // screen, so it is erased with the source and deliberately not restored.
    let delimited_typo = safe_observe_delimiter(ctx, delimiter, shown).and_then(|boundary| {
        ctx.state
            .autocorrect
            .blocking_lock()
            .fix_delimited_typo_in(&text, boundary.ch, focused_class.as_deref())
            .map(|fixed| (boundary.ch, fixed))
    });
    if let Some((delimiter_ch, fixed)) = delimited_typo {
        drop(exceptions);
        pulinto_core::diag!(
            "{}User auto-replacement: {} → {} characters",
            if ctx.dry_run {
                "DRY RUN "
            } else {
                ""
            },
            text.chars().count() + usize::from(!delimiter_ch.is_whitespace()),
            fixed.chars().count()
        );
        if ctx.dry_run {
            return;
        }
        let layout = ctx.profiles[shown].layout();
        let applied = retype(
            ctx,
            strokes.len(),
            Retype::Text(fixed, layout),
            Cursor::PastConsumedDelimiter,
        );
        if applied {
            ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
        }
        ctx.learning.invalidate_last();
        return;
    }

    // User-created exact replacements are independent of language detection
    // and may be short abbreviations. The separator remains in place.
    let exact_replacement = ctx
        .state
        .autocorrect
        .blocking_lock()
        .fix_user_word_in(&text, focused_class.as_deref());
    if let Some(fixed) = exact_replacement {
        drop(exceptions);
        pulinto_core::diag!(
            "{}User auto-replacement: {} → {} characters",
            if ctx.dry_run {
                "DRY RUN "
            } else {
                ""
            },
            text.chars().count(),
            fixed.chars().count()
        );
        if ctx.dry_run {
            return;
        }
        let layout = ctx.profiles[shown].layout();
        let applied = retype(
            ctx,
            strokes.len(),
            Retype::Text(fixed, layout),
            Cursor::PastDelimiter { key: delimiter },
        );
        if applied {
            ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
        }
        ctx.learning.invalidate_last();
        return;
    }

    // Typo correction (ПРивет → Привет) is independent of layout and stays
    // first. It rewrites characters inside the current layout, so it is the one
    // correction that types text instead of replaying keys.
    if strokes.len() >= 3 {
        let autocorrect = ctx.state.autocorrect.blocking_lock();
        if let Some(fixed) = autocorrect.fix_typo_in(&text, focused_class.as_deref()) {
            drop(autocorrect);
            drop(exceptions);
            pulinto_core::diag!(
                "{}Case auto-correction: {} characters",
                if ctx.dry_run {
                    "DRY RUN "
                } else {
                    ""
                },
                text.chars().count()
            );
            if ctx.dry_run {
                return;
            }
            let layout = ctx.profiles[current].layout();
            let applied = retype(
                ctx,
                strokes.len(),
                Retype::Text(fixed, layout),
                Cursor::PastDelimiter { key: delimiter },
            );
            if applied {
                ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
            }
            // A typo correction is not a positional reading of the same keys;
            // mechanically cycling it between layouts is unsafe.
            ctx.learning.invalidate_last();
            return;
        }
    }

    // No length gate here on purpose. The old engine skipped anything shorter
    // than three characters, which disabled the one stage that beats every
    // other switcher: measured on the held-out corpus, one- and two-letter
    // words are where our whole advantage lives (96.3% and 98.6% against
    // 87.3% and 94.5% in the reference run).
    let refs = profile::refs(&ctx.profiles);
    let context: Vec<&str> = ctx.recent_langs.lock().unwrap().clone();

    // A token whose doubtful keys turn out to be punctuation is two words with a
    // stop between them, and converting it as one is how `надо.представили`
    // became `надоюпредставили`. Splitting it properly is worth doing and is not
    // done yet; until then the token is left exactly as the user typed it.
    // A correction missed costs one hotkey press, text eaten costs trust.
    if let scanword::WordPlan::Split(position) = plan {
        pulinto_core::diag!(
            "Replacement dropped: token of length {} — the mark at position {position} turned out to be the end of a word",
            text.chars().count()
        );
        remember_observed_word(ctx, strokes, original_text, shown, delimiter);
        return;
    }

    let mut decision = cascade::decide(&text, &refs, current, &context);

    // The user's "always convert" list overrides a cascade "keep" verdict —
    // but never a stop rule. Stop rules are the guard against corrupting a
    // password or URL, and a standing word-list preference must not be able
    // to switch that off; only stage 0 saying nothing was wrong lets this
    // through.
    if !decision.convert
        && !matches!(decision.stage, cascade::Stage::StopRule(_))
        && exceptions.is_always_word_in(&text, focused_class.as_deref())
    {
        let target = (current + 1) % ctx.profiles.len();
        decision.layout_id = ctx.profiles[target].layout().id;
        decision.convert = true;
    }
    drop(exceptions);

    seen.report(&decision, ctx.dry_run);
    ctx.reports.note(crate::report::Observed {
        at: crate::report::now_ms(),
        typed: text.clone(),
        strokes: strokes.clone(),
        readings,
        current_layout: seen.layout.clone().unwrap_or_else(|| "unknown".into()),
        loaded_layouts: seen.loaded.clone(),
        context: context.iter().map(|lang| (*lang).to_string()).collect(),
        stage: format!("{:?}", decision.stage),
        converted_to: decision.convert.then(|| decision.text.clone()),
        applied: decision.convert && !ctx.dry_run,
    });

    if decision.convert && !ctx.dry_run {
        let target = ctx
            .profiles
            .iter()
            .position(|p| p.layout().id == decision.layout_id)
            .unwrap_or(0);
        let applied = retype(
            ctx,
            strokes.len(),
            Retype::Replay(strokes.clone(), decision.layout_id),
            Cursor::PastDelimiter { key: delimiter },
        );
        if applied {
            ctx.learning.remember(Fix {
                strokes,
                original: text,
                shown: target,
                delimiter: safe_observe_delimiter(ctx, delimiter, target),
            });
            ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
        } else {
            remember_observed_word(ctx, strokes, original_text, shown, delimiter);
        }
    } else if !ctx.dry_run {
        remember_observed_word(ctx, strokes, original_text, shown, delimiter);
    }

    // Remember what language this word turned out to be, for the next word's
    // stage 2. Only the two most recent are kept, matching what the cascade reads.
    let mut recent = ctx.recent_langs.lock().unwrap();
    recent.insert(0, decision.layout_id);
    recent.truncate(2);
}

/// One word as the daemon saw it, for the diagnostics line.
struct Seen {
    layout: Option<String>,
    group: Option<u8>,
    loaded: Vec<String>,
    profile: &'static str,
    text: String,
}

impl Seen {
    fn report(&self, decision: &cascade::Decision, dry_run: bool) {
        let verdict = if decision.convert {
            "replace"
        } else {
            "keep"
        };
        pulinto_core::diag!(
            "{}decision: {verdict}, {} characters [stage {:?} | layout {} (profile {}) | group {} | loaded {}]",
            if dry_run { "DRY RUN " } else { "" },
            self.text.chars().count(),
            decision.stage,
            self.layout.as_deref().unwrap_or("unknown"),
            self.profile,
            self.group.map(|g| g.to_string()).unwrap_or_else(|| "?".into()),
            if self.loaded.is_empty() { "—".to_string() } else { self.loaded.join(",") },
        );
    }
}

/// What to put in place of the erased text.
enum Retype {
    /// Replay the original keys under `layout_id` — the layout-crossing case,
    /// where the letters must come from the keymap and not from us.
    Replay(Vec<(u16, bool)>, &'static str),
    /// Type these characters in the layout already in effect.
    Text(String, &'static Layout),
}

/// How long a replacement will take, near enough to decide by.
///
/// Every key costs a press, a release and the hold between them; on top of that
/// come the pause after the backspaces and the layout margins. Deliberately an
/// over-estimate: being wrong towards "there is no room" costs a correction,
/// being wrong the other way costs the user's text.
fn estimated_ms(ctx: &Arc<EventContext>, erase: usize, what: &Retype) -> u64 {
    let timing = ctx.injector.timing;
    let payload = match what {
        Retype::Replay(strokes, _) => strokes.len(),
        Retype::Text(text, _) => text.chars().count(),
    };
    let keys = (erase + payload + 1) as u64;
    keys * timing.key_hold_ms * 2 + timing.post_backspace_ms + POST_DELIMITER_MS + 90
}

/// Run a replacement on a thread of its own.
///
/// The thread is the only thing observe mode adds. Blocking here would stall
/// the capture reader, and a replacement takes a couple of hundred
/// milliseconds by design. Grab mode calls the same `replace`
/// functions inline, because it is holding the delimiter while they run.
fn retype(ctx: &Arc<EventContext>, erase: usize, what: Retype, cursor: Cursor) -> bool {
    if !matches!(cursor, Cursor::AtWord) {
        // Let the application finish with the delimiter it just received
        // before we start editing the text in front of it.
        std::thread::sleep(Duration::from_millis(POST_DELIMITER_MS));
    }

    // Will it fit? A correction is a few hundred milliseconds of injected keys
    // that cannot be taken back once started, and the user is still typing.
    // The wait counts too: the correction does not start at the delimiter, it
    // starts QUIET_MS later, and that time comes out of the same gap. Leaving
    // it out is why the guard declined only 12 corrections out of 54 and the
    // remaining ones still landed on top of live typing.
    //
    // NOT MEASURED: found by reading the log after the series stopped on its
    // own rule, and shipped because it is an omission rather than a tuning
    // choice. The next series has to confirm it.
    let estimate = estimated_ms(ctx, erase, &what) + QUIET_MS;
    if !ctx.rhythm.lock().unwrap().fits(estimate) {
        pulinto_core::diag!("Replacement dropped: it needs {estimate} ms, and the gap between words is shorter");
        return false;
    }

    // Wait for the keyboard to go quiet before touching anything. Nothing is
    // lost meanwhile — it piles up in `Pending` — but the erase has to land on
    // the text this correction was computed against.
    if !ctx.pending.wait_for_quiet(QUIET_MS, QUIET_GIVE_UP_MS) {
        pulinto_core::diag!("Replacement dropped: the typing does not pause, nowhere to erase");
        return false;
    }

    let held = ctx.held();
    match what {
        Retype::Text(text, target) => {
            if let Err(error) =
                replace::retype_text(&ctx.injector, target, erase, &text, cursor, &held)
            {
                eprintln!("Safe auto-replacement is impossible ({error}), the text is untouched");
                return false;
            }
        }
        Retype::Replay(strokes, target_id) => {
            if let Err(error) = replace::replay_under(
                &ctx.injector,
                ctx.layout.as_ref().as_ref(),
                &strokes,
                target_id,
                cursor,
                &held,
            ) {
                eprintln!("Safe layout switch is impossible ({error}), the text is untouched");
                return false;
            }
        }
    }
    true
}

/// What a hotkey does. Observe mode holds no grab, so it only sees the keys:
/// the combination reaches the application as well, and converting a selection
/// (which needs the grab to stay out of the application's way) is not offered.
fn fire(ctx: &Arc<EventContext>, action: Action) {
    match action {
        Action::Word => manual_toggle(ctx),
        Action::Selection => {
            println!("Observe mode cannot re-lay a selection: it needs keyboard grab");
        }
        Action::ToggleApp => toggle_application(ctx),
        Action::Report => file_report(ctx),
        Action::AppleLayout => {
            crate::keyboard::toggle_apple_layout(ctx.layout.as_ref().as_ref(), &ctx.state)
        }
    }
}

/// This remains available while automatic correction or the focused app is
/// disabled — it is the user's way back in.
fn toggle_application(ctx: &Arc<EventContext>) {
    ctx.learning.invalidate_last();
    ctx.word.lock().unwrap().clear();
    ctx.recent_langs.lock().unwrap().clear();
    if let Err(error) = crate::toggle_current_application(&ctx.state, ctx.windows.as_ref()) {
        eprintln!("Cannot switch PuLinTo for the current application: {error}");
    }
}

/// Filing a report works in every mode and whether or not auto-correction is
/// on: the moment worth capturing is exactly the moment something went
/// wrong, and by then the user may well have switched the daemon off.
fn file_report(ctx: &Arc<EventContext>) {
    // In observe the report key has already reached the application. Even if the
    // chord itself typed nothing there, the cursor position cannot be assumed
    // unchanged after an external hotkey.
    ctx.learning.invalidate_last();
    ctx.word.lock().unwrap().clear();
    // What the user had highlighted names the offending word outright, and
    // what application they were in has explained several defects already.
    let selection = ctx.windows.as_ref().and_then(|windows| windows.selection());
    let window = ctx
        .windows
        .as_ref()
        .and_then(|windows| windows.focused_class());
    match ctx.reports.report(selection.as_deref(), window.as_deref()) {
        Ok(path) => println!("Bug report written: {}", path.display()),
        Err(error) => eprintln!("Report not written: {error}"),
    }
}

/// What a bare Alt tap does: cycle the current word, or the last word while the
/// cursor is still exactly one safe delimiter after it.
fn manual_toggle(ctx: &Arc<EventContext>) {
    if manual_disabled_for_application(ctx) {
        ctx.learning.invalidate_last();
        ctx.word.lock().unwrap().clear();
        return;
    }
    if ctx.dry_run {
        println!("DRY RUN Alt: hotkey noticed, but it does nothing in this mode");
        return;
    }
    let mut word = ctx.word.lock().unwrap();
    if !word.is_empty() {
        let from = word.shown_profile().unwrap_or_else(|| {
            scanword::current_profile(ctx.layout.as_ref().as_ref(), &ctx.profiles, &word)
        });
        let to = (from + 1) % ctx.profiles.len();
        let text = word.text(ctx.profiles[from].layout());
        let strokes = word.replay_keys();

        // Teaching signal for the "always" list: only
        // on the first step. Later steps are a mechanical cycle, not another
        // preference signal.
        if word.shown_profile().is_none() {
            let refs = profile::refs(&ctx.profiles);
            let context: Vec<&str> = ctx.recent_langs.lock().unwrap().clone();
            let would_auto_convert = cascade::decide(&text, &refs, from, &context).convert;
            if !would_auto_convert {
                let mut exceptions = ctx.state.exceptions.blocking_lock();
                exceptions.record_manual_conversion(&text);
            }
        }

        pulinto_core::diag!(
            "Alt cycle: {} characters, {} → {}",
            text.chars().count(),
            ctx.profiles[from].layout().id,
            ctx.profiles[to].layout().id
        );
        // No delimiter has been typed yet, so the cursor is already where the
        // word ends and there is nothing to step over.
        if retype(
            ctx,
            strokes.len(),
            Retype::Replay(strokes, ctx.profiles[to].layout().id),
            Cursor::AtWord,
        ) {
            word.set_shown_from(from, to);
            word.mark_converted();
            ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
        }
        return;
    }
    drop(word);

    let Some(fix) = ctx.learning.take_last() else {
        return;
    };
    let Some(cycle) = replace::plan_word_cycle(&fix, &ctx.profiles) else {
        ctx.learning.remember(fix);
        return;
    };
    let to = cycle.to;
    let current_text = cycle.current_text;
    let target_text = cycle.target_text;
    pulinto_core::diag!(
        "Alt cycle of the last word: {} characters, {} → {}",
        current_text.chars().count(),
        ctx.profiles[fix.shown].layout().id,
        ctx.profiles[to].layout().id
    );

    if retype(
        ctx,
        cycle.erase - 1,
        Retype::Replay(fix.strokes.clone(), ctx.profiles[to].layout().id),
        Cursor::PastDelimiter {
            key: cycle.delimiter.key,
        },
    ) {
        if target_text == fix.original {
            ctx.learning.mark_reverted(&fix.original);
            let mut exceptions = ctx.state.exceptions.blocking_lock();
            exceptions.record_revert(&fix.original);
        } else {
            if current_text == fix.original {
                let mut exceptions = ctx.state.exceptions.blocking_lock();
                exceptions.record_manual_conversion(&fix.original);
            }
            ctx.learning.unmark_reverted(&fix.original);
        }
        ctx.learning.remember(Fix { shown: to, ..fix });
        ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
    } else {
        ctx.learning.remember(fix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulinto_input::capture::{Key, KeyEvent};

    fn press(pressed: bool) -> KeyEvent {
        KeyEvent {
            key: Key::KEY_A,
            pressed,
            at: std::time::Instant::now(),
            vendor: 0,
        }
    }

    /// The defect this replaces: the wait was for the queue to empty, and the
    /// only thread that empties it is the one waiting. A single leftover event —
    /// the release of the delimiter, always — made the condition unreachable.
    #[test]
    fn a_leftover_release_does_not_block_a_correction_forever() {
        let pending = Pending::default();
        pending.push(press(false));
        // Nobody is draining: under the old rule this could never come back true.
        assert!(pending.wait_for_quiet(20, 400));
    }

    /// Text that has already reached the screen is a hard stop, not something
    /// to wait out: erasing would take out the new characters, not the old.
    #[test]
    fn queued_presses_abandon_the_correction_at_once() {
        let pending = Pending::default();
        pending.push(press(true));
        let started = std::time::Instant::now();
        assert!(!pending.wait_for_quiet(20, 1000));
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "отказ должен быть немедленным, а занял {:?}",
            started.elapsed()
        );
    }
}

#[cfg(test)]
mod rhythm_tests {
    use super::*;

    /// A correction that will not fit in the user's pace is declined. This is
    /// the guard that was missing when the rig, typing a word every 300 ms,
    /// came back with 39 mangled words out of 120.
    #[test]
    fn a_correction_longer_than_the_gap_is_declined() {
        let mut rhythm = Rhythm::default();
        // Four words at 300 ms apart teach it the pace.
        let start = std::time::Instant::now();
        for step in 0..4 {
            rhythm.note_boundary(start + Duration::from_millis(300 * step));
        }
        assert!(!rhythm.fits(400), "на 400 мс при паузах в 300 места нет");
        assert!(
            rhythm.fits(120),
            "короткая замена помещается и в быстрый набор"
        );
    }

    /// A pause for thought must not be read as room for anything.
    #[test]
    fn a_long_pause_does_not_stretch_the_measured_pace() {
        let mut rhythm = Rhythm::default();
        let start = std::time::Instant::now();
        rhythm.note_boundary(start);
        rhythm.note_boundary(start + Duration::from_millis(20));
        let after_typing = rhythm.gap_ms;
        // A six-second pause is filtered out entirely.
        rhythm.note_boundary(start + Duration::from_secs(6));
        assert_eq!(rhythm.gap_ms, after_typing);
    }
}
