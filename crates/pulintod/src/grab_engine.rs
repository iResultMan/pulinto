//! Grab-mode event loop.
//!
//! The keyboard is held exclusively, so every keystroke passes through here and
//! is re-emitted by us. That is what buys the one capability the observe-only
//! path cannot have: a delimiter can be *held* while the cascade decides, so a
//! correction lands before Enter reaches the application rather than racing it.
//!
//! Everything in this file is written on the assumption that a bug here costs
//! the user their keyboard. Ordinary keys are forwarded first and examined
//! afterwards; only a delimiter is ever withheld.
//!
//! **How long a delimiter waits.** The loop is synchronous: the delimiter is
//! forwarded at the end of `handle_delimiter`, so it is delayed by exactly the
//! time that function takes. The cascade itself is pure computation in the
//! microsecond range; the cost is the injection, bounded by the timing profile
//! at roughly 400 ms for a long word. Keys typed during that window are
//! not lost — the kernel queues them on the device and they are read next
//! iteration, in order.
//!
//! **What is not guarded.** There is no watchdog thread. The one call here that
//! could block without bound is `layout.toggle_next()`, an X11 round trip that
//! would hang if the X server stopped answering. The failsafe for that case is
//! the panic combination, which is checked on every event — and a crash is
//! harmless, since the kernel drops the grab when the descriptor closes.

use std::os::fd::AsRawFd;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use pulinto_core::cascade;
use pulinto_data::profile::{self, Profile};
use pulinto_input::capture;
use pulinto_input::grab::{self as grab, GrabbedKeyboard, PanicDetector};
use pulinto_input::grab::{EventType, InputEvent, Key};
use pulinto_input::injector::Injector;
use pulinto_input::xtest_unicode::{Delim, XTestUnicodeBackend};
use pulinto_layout::LayoutController;

use crate::hotkeys::{Action, Modifier, TapRelease, TapTracker};
use crate::replace::{self, Learning};
use crate::scanword::{self, ScanWord};
use crate::SwitcherState;

pub struct GrabContext {
    pub state: Arc<SwitcherState>,
    pub profiles: Arc<Vec<Profile>>,
    pub injector: Arc<Injector>,
    pub layout: Arc<Box<dyn LayoutController>>,
    /// Reverted words and the last correction — the same instance observe mode
    /// uses, so what the user teaches in one mode holds in the other.
    pub learning: Arc<Learning>,
    /// Delivery of a replacement over one X connection: erasing, ready-made Unicode characters
    /// and the delimiter go in one request queue, and the layout is not touched.
    /// `None` on Wayland and without XTest — then the old scan-code path works.
    pub xinject: Option<Arc<XTestUnicodeBackend>>,
    /// Reading the live PRIMARY for Alt on a selection. Under X11 it is the same
    /// connection as `xinject`; under Wayland a separate, read-only one
    /// (Xwayland mirrors the selection of Wayland windows), and typing goes through uinput.
    pub selection: Option<Arc<XTestUnicodeBackend>>,
    /// Recent decisions, kept so the user can point at one that went wrong.
    pub reports: Arc<crate::report::Registry>,
    /// XI2 click generation. A click can move the caret without producing a
    /// keyboard event, invalidating every word/correction snapshot we hold.
    pub pointer_clicks: Option<crate::xwindow::PointerClicks>,
    /// Focused WM_CLASS for the same safety policy observe mode applies —
    /// and, for a report, what the user had highlighted.
    pub windows: Option<crate::xwindow::XWindows>,
    /// One liveness mark per keyboard loop, read by [`watchdog`].
    pub heartbeats: std::sync::Mutex<Vec<std::sync::Weak<std::sync::atomic::AtomicU64>>>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// How long a keyboard loop may go without a sign of life before its keys are
/// put back up. A loop that is merely slow loses a held key for a moment; a
/// loop that is stuck would otherwise leave it down — and repeating — for good.
const STALL_LIMIT_MS: u64 = 3000;

/// Last line of defence against a key left pressed on the virtual keyboard.
///
/// The loop that forwards a keyboard wakes at least every half second and says
/// so. If one stops saying so while the virtual keyboard holds a key, the key is
/// released from here. Runs for the life of the daemon.
pub fn watchdog(ctx: Arc<GrabContext>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let stalled = ctx
            .heartbeats
            .lock()
            .map(|marks| {
                marks.iter().filter_map(|mark| mark.upgrade()).any(|mark| {
                    now_ms().saturating_sub(mark.load(Ordering::Relaxed)) > STALL_LIMIT_MS
                })
            })
            .unwrap_or(false);
        if stalled && ctx.injector.has_forwarded_down() {
            match ctx.injector.release_forwarded() {
                Ok(count) => eprintln!(
                    "The keyboard loop has not answered for over {} s: keys released — {count}",
                    STALL_LIMIT_MS / 1000
                ),
                Err(error) => eprintln!("Keys not released ({error})"),
            }
        }
    });
}

/// Languages of the last words, and the short words just behind the caret that
/// may yet turn out to belong to the next word's language.
///
/// Typed without touching the layout, the first word of an English phrase
/// after Russian text is often two letters: `ща` for `of`. On its own it cannot
/// be told from a typo, and the context still says Russian, so it is kept. The
/// next word settles it (`еру` → `the`) — and then the short one is rewritten
/// with it. That is the tail. It is only valid while nothing but single spaces
/// separates it from the caret, so every place that drops the language context
/// — a click, an arrow, Enter, an application shortcut — drops it too, through
/// the same `clear`.
#[derive(Default)]
struct Context {
    langs: Vec<&'static str>,
    short_tail: Vec<ShortWordBehind>,
    /// The layout chord was the last thing typed. The words before it are
    /// still the context — the switch may have been habit — but it may also
    /// have been deliberate, and then a single letter typed next (`c`, `b`, a
    /// list item or a variable) must not be converted on the strength of
    /// the language before the switch. The first finished word settles it.
    after_chord: bool,
}

/// A word behind the caret that the next word may still rewrite: a kept short
/// word, or a longer one whose two readings are both words and which only
/// frequency chose between (`doubtful`). Kept as keys and as the profile it
/// is shown in.
struct ShortWordBehind {
    strokes: Vec<(u16, bool)>,
    shown: usize,
    doubtful: bool,
}

impl Context {
    fn clear(&mut self) {
        self.langs.clear();
        self.short_tail.clear();
        self.after_chord = false;
    }

    /// The layout chord was typed: the word is over, the short-word tail is
    /// no longer one space from the caret in the same language, the words
    /// before stay as context.
    fn note_layout_chord(&mut self) {
        self.short_tail.clear();
        self.after_chord = true;
    }
}

impl std::ops::Deref for Context {
    type Target = Vec<&'static str>;
    fn deref(&self) -> &Self::Target {
        &self.langs
    }
}

impl std::ops::DerefMut for Context {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.langs
    }
}

/// The part of `tail` to rewrite along with a word converted into `to`: the
/// words nearest the caret that are no word where they were typed and are one
/// in the target language (`ща` → `of`, `d` → `в`). A real word in its own
/// layout (`в`, `a`) stops the walk — it was typed as meant.
///
/// "No word" for two letters is the short-word table *and* the frequency list:
/// the list holds `vs`, `bp`, `dj` as genuine entries. Such a form is rewritten
/// only when the target is overwhelmingly more common — `bp` (78) against `из`
/// (43 701) yes, `vs` (385) against `мы` (10 893) no: "Russia vs Украина" is
/// real text, and a wrong rewrite costs more than a missed one.
fn tail_to_rewrite<'a>(
    tail: &'a [ShortWordBehind],
    profiles: &[Profile],
    to: usize,
) -> &'a [ShortWordBehind] {
    use pulinto_core::cascade::LanguageProfile;
    const MARGIN: u64 = 50;
    let target = &profiles[to];
    let fits = |behind: &ShortWordBehind| {
        let typed = &profiles[behind.shown];
        let as_typed = ScanWord::render_keys(&behind.strokes, typed.layout());
        let as_meant = ScanWord::render_keys(&behind.strokes, target.layout());
        // A collision word frequency alone decided (`рук` → `her`): the word
        // after it is the context it lacked, and it goes with that word as
        // soon as its reading there is a word at all.
        if behind.doubtful {
            return behind.shown != to
                && (target.in_dictionary(&as_meant.to_lowercase())
                    || pulinto_core::short_words::is_valid_short_word(
                        &as_meant,
                        target.layout_id(),
                    ));
        }
        let outweighed = match typed.word_frequency(&as_typed) {
            None => true,
            Some(typed_count) => target
                .word_frequency(&as_meant)
                .is_some_and(|meant_count| meant_count >= typed_count.saturating_mul(MARGIN)),
        };
        behind.shown != to
            && !pulinto_core::short_words::is_valid_short_word(&as_typed, typed.layout_id())
            && pulinto_core::short_words::is_valid_short_word(&as_meant, target.layout_id())
            && outweighed
    };
    let keep_from = tail
        .iter()
        .rposition(|behind| !fits(behind))
        .map_or(0, |index| index + 1);
    &tail[keep_from..]
}

/// The tail words rendered under `layout`, each followed by a space, in front
/// of `text`; and how many characters that replaces: the word itself plus
/// every tail word and its space.
fn with_tail_text(
    behind: &[ShortWordBehind],
    layout: &'static pulinto_core::layout::Layout,
    word_len: usize,
    text: &str,
) -> (usize, String) {
    let mut erase = word_len;
    let mut out = String::new();
    for short in behind {
        out.push_str(&ScanWord::render_keys(&short.strokes, layout));
        out.push(' ');
        erase += short.strokes.len() + 1;
    }
    out.push_str(text);
    (erase, out)
}

/// Wait up to `millis` for `fd` to have something to read.
fn wait_readable(fd: std::os::fd::RawFd, millis: i32) -> bool {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut poll, 1, millis) };
    // An error or a hang-up is for fetch_events to report; it knows the cause.
    ready != 0
}

/// Modifier keycodes we track so they can be lifted before injection.
const MODIFIERS: [u16; 8] = [42, 54, 29, 97, 56, 100, 125, 126];

const KEY_LEFTALT: u16 = 56;
const KEY_RIGHTALT: u16 = 100;

fn is_modifier_code(code: u16) -> bool {
    MODIFIERS.contains(&code)
}

fn is_alt_code(code: u16) -> bool {
    code == KEY_LEFTALT || code == KEY_RIGHTALT
}

/// Why a keyboard reader stopped. A disconnected device may be discovered
/// again by the hotplug monitor; an emergency release must stay released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    Disconnected,
    EmergencyRelease,
}

/// Run the loop for one grabbed keyboard. Returns when the device disappears
/// or the panic combination is used.
/// Ask RealtimeKit for a scheduling edge for an input thread of ours.
///
/// The kernel keeps 64 events per reader of a device. On a loaded machine (a
/// build, an OCR batch) an ordinary thread can go unscheduled long enough for
/// a word's worth of keys to be dropped before the reader gets to them, and
/// the loop that forwards keys falls behind so the compositor receives them in
/// bursts. rtkit grants an unprivileged process a negative nice value for a
/// thread of its own; without rtkit the thread simply stays ordinary.
fn raise_input_priority(what: &str) {
    const NICE: i32 = -10;
    // SAFETY: gettid takes no arguments and cannot fail.
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as u64;
    let granted = zbus::blocking::Connection::system().and_then(|connection| {
        connection
            .call_method(
                Some("org.freedesktop.RealtimeKit1"),
                "/org/freedesktop/RealtimeKit1",
                Some("org.freedesktop.RealtimeKit1"),
                "MakeThreadHighPriority",
                &(tid, NICE),
            )
            .map(|_| ())
    });
    match granted {
        Ok(()) => println!("Thread priority ({what}): nice {NICE} via rtkit"),
        Err(error) => eprintln!("Thread priority ({what}) not raised: {error}"),
    }
}

pub fn run(ctx: Arc<GrabContext>, mut keyboard: GrabbedKeyboard) -> RunExit {
    let name = keyboard.name();
    raise_input_priority(&format!("loop {name}"));
    // The set of hotkeys follows the keyboard, so an Apple one and a PC one
    // attached together each keep their own habits.
    let vendor = keyboard.vendor_id();
    println!("Grab: {name} (vendor {vendor:#06x})");

    // The kernel keeps only 64 events per reader of a device — about ten key
    // taps once MSC scan codes are counted — and drops the rest when they are
    // not read in time. While a replacement is being typed this loop does not
    // read, and a fast typist overran the buffer mid-correction: whole words
    // vanished (`строительных норм,` → `м,` on the stand). So a reader thread
    // empties the kernel buffer into an unbounded queue as events arrive, and
    // owns the emergency release, which must not wait behind a replacement.
    // Each batch carries the click count as of its reading: a click made while
    // earlier keys still wait in the queue must not cut a word those keys
    // belong to, and must cut the one typed after it.
    let (sender, queue) = std::sync::mpsc::channel::<(u64, Instant, Vec<InputEvent>)>();
    let reader_injector = Arc::clone(&ctx.injector);
    let reader_ctx = Arc::clone(&ctx);
    let reader_name = name.clone();
    let reader = std::thread::spawn(move || {
        raise_input_priority(&format!("reader {reader_name}"));
        let mut panic_detector = PanicDetector::default();
        let mut held = grab::HeldKeys::default();
        // `PULINTO_TRACE_INPUT=<file>`: every key event read from this
        // keyboard, to pair with the virtual keyboard's own trace.
        let input_trace = pulinto_input::injector::open_trace("PULINTO_TRACE_INPUT");
        let fd = keyboard.device_mut().as_raw_fd();
        let clicks = || {
            reader_ctx
                .pointer_clicks
                .as_ref()
                .map(crate::xwindow::PointerClicks::generation)
                .unwrap_or(0)
        };
        // Everything still held is released on the way out. The physical
        // release will not reach the system afterwards — the daemon has let
        // go, or the keyboard is gone — and a key left down in our virtual
        // keyboard is repeated by the compositor until someone presses it again.
        let release_held = |held: &mut grab::HeldKeys| {
            let releases: Vec<InputEvent> = held
                .take_all()
                .into_iter()
                .map(grab::release_event)
                .collect();
            if !releases.is_empty() {
                let _ = sender.send((clicks(), Instant::now(), releases));
            }
        };
        loop {
            // Quiet for a moment: compare what we think is held with what the
            // keyboard says. A release the kernel dropped shows up here.
            if !wait_readable(fd, 100) {
                // Asked through the probe handle only: on the grabbed one the
                // ioctl would throw away whatever key events arrived since the
                // poll timed out (see `GrabbedKeyboard::keys_down`).
                let Some(hardware) = keyboard.keys_down() else {
                    continue;
                };
                let lost = held.lost(&hardware);
                if !lost.is_empty() {
                    eprintln!("Lost release of keys {lost:?} recovered");
                    let releases = lost.into_iter().map(grab::release_event).collect();
                    if sender.send((clicks(), Instant::now(), releases)).is_err() {
                        return RunExit::Disconnected;
                    }
                }
                continue;
            }
            let events = match keyboard.device_mut().fetch_events() {
                Ok(events) => events.collect::<Vec<_>>(),
                Err(error) => {
                    eprintln!("Keyboard {reader_name} dropped off: {error}");
                    release_held(&mut held);
                    return RunExit::Disconnected;
                }
            };
            for event in &events {
                pulinto_input::injector::trace_event(&input_trace, 'i', event);
            }
            let panic_at = events.iter().position(|event| {
                held.observe(event);
                grab::key_transition(event)
                    .is_some_and(|(key, pressed)| panic_detector.observe(key, pressed))
            });
            if let Some(index) = panic_at {
                let _ = sender.send((clicks(), Instant::now(), events[..index].to_vec()));
                eprintln!("EMERGENCY CHORD: releasing the keyboard grab");
                let _ = reader_injector.forward(&[events[index]]);
                keyboard.release();
                release_held(&mut held);
                return RunExit::EmergencyRelease;
            }
            if sender.send((clicks(), Instant::now(), events)).is_err() {
                return RunExit::Disconnected;
            }
        }
    });

    let mut word = ScanWord::default();
    let mut context_langs = Context::default();
    let mut shift_down = false;
    let mut held_modifiers: Vec<u16> = Vec::new();
    let mut tap = TapTracker::default();
    // The press of a modifier that is a tap candidate and that applications
    // would act on by itself (Alt, Super): held back until the key comes up
    // untouched or something else is pressed.
    let mut pending_tap: Option<InputEvent> = None;
    // Exact replacements may consume a printable delimiter (`ещ\\` → `ещё`).
    // Consume its release/repeats too: forwarding a release for a press the
    // application never saw creates an invalid key stream.
    let mut swallowed_releases: Vec<u16> = Vec::new();
    let mut seen_pointer_click = ctx
        .pointer_clicks
        .as_ref()
        .map(crate::xwindow::PointerClicks::generation)
        .unwrap_or(0);
    let focus_generation = || {
        ctx.windows
            .as_ref()
            .map(|windows| windows.focus_changes().generation())
            .unwrap_or(0)
    };
    let mut seen_focus = focus_generation();

    let heartbeat = Arc::new(std::sync::atomic::AtomicU64::new(now_ms()));
    if let Ok(mut marks) = ctx.heartbeats.lock() {
        marks.retain(|mark| mark.upgrade().is_some());
        marks.push(Arc::downgrade(&heartbeat));
    }
    loop {
        heartbeat.store(now_ms(), Ordering::Relaxed);
        // `arrived` is when the reader took the batch off the device. A key is
        // stamped with that, not with the moment the loop gets to it: during a
        // replacement the loop is busy for up to 400 ms, and a key typed before
        // a mouse selection would otherwise be stamped after it and retire a
        // live selection. Which keys count is decided below, where a hotkey
        // chord is told from typing — Shift+Pause must not retire the very
        // selection it is asking to convert.
        let (pointer_generation, arrived, events) =
            match queue.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(batch) => batch,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };

        for event in events {
            let Some((key, pressed)) = grab::key_transition(&event) else {
                // Auto-repeat and non-key events: forward untouched. Repeats
                // must reach the application (held Backspace has to repeat),
                // they simply must not enter the word buffer.
                // A repeat of a press we deliberately delayed is the one
                // exception: forwarding it without the press would create an
                // impossible key stream in the application.
                if event.event_type() == EventType::KEY
                    && swallowed_releases.contains(&event.code())
                {
                    continue;
                }
                if pending_tap.as_ref().is_some_and(|held| {
                    event.event_type() == EventType::KEY && event.code() == held.code()
                }) {
                    continue;
                }
                let _ = ctx.injector.forward(&[event]);
                continue;
            };

            // A mouse click can change the focused field and caret while the
            // evdev keyboard stream stays completely silent. Never carry a
            // word, language context or Alt-cycle snapshot across that click.
            // Do this before the first post-click key is interpreted.
            if pointer_generation != seen_pointer_click {
                seen_pointer_click = pointer_generation;
                if !word.is_empty() || ctx.learning.has_last() {
                    println!("Mouse click: the word buffer and the last replacement were reset");
                }
                word.clear();
                context_langs.clear();
                ctx.learning.invalidate_last();
            }
            // Another window took the focus — by Alt+Tab, a launcher, or a
            // click the pointer watch did not see. Nothing we remember about
            // the old window's text is under the caret any more.
            let focus_now = focus_generation();
            if focus_now != seen_focus {
                seen_focus = focus_now;
                if !word.is_empty() || ctx.learning.has_last() {
                    println!("Window change: the word buffer and the last replacement were reset");
                }
                word.clear();
                context_langs.clear();
                ctx.learning.invalidate_last();
            }

            let code = key.code();

            // A press that comes up a second time without its release (the
            // kernel drops events) must not leave a stale mark that would
            // swallow the release of a later, real press of the same key.
            if pressed {
                swallowed_releases.retain(|swallowed| *swallowed != code);
            }
            // While a combination is being recorded, the daemon takes the
            // presses made since — and their releases, so nothing is left
            // pressed anywhere. A release of a key pressed earlier is not
            // taken: the application saw its press.
            if ctx
                .state
                .hotkeys
                .recorder
                .feed(code, pressed, &held_modifiers)
            {
                if pressed {
                    swallowed_releases.push(code);
                } else if let Some(position) = swallowed_releases
                    .iter()
                    .position(|swallowed| *swallowed == code)
                {
                    swallowed_releases.remove(position);
                }
                continue;
            }
            // The release of a key whose press was swallowed (a replacement
            // that consumed it, a hotkey, a recording).
            if !pressed {
                if let Some(position) = swallowed_releases
                    .iter()
                    .position(|swallowed| *swallowed == code)
                {
                    swallowed_releases.remove(position);
                    continue;
                }
            }

            // The set of hotkeys of this keyboard, read per key so an edit in
            // the settings window applies at once.
            let keys = ctx.state.hotkeys.set_for(vendor);

            if is_modifier_code(code) {
                let others_held = held_modifiers.iter().any(|held| *held != code);
                if code == 42 || code == 54 {
                    shift_down = pressed;
                }
                // The layout chord ends the word but not the text: the
                // words before it were typed in whatever language they were
                // in, and the next word is judged against them. That is the
                // very moment the layout is wrong — the user switched out of
                // habit, or did not — so an initial or a short word typed
                // right after it needs that context most.
                let mut layout_chord = false;
                if pressed {
                    if !held_modifiers.contains(&code) {
                        held_modifiers.push(code);
                    }
                    layout_chord = note_user_layout_switch(&ctx, code, &held_modifiers);
                } else {
                    held_modifiers.retain(|held| *held != code);
                }
                // Whatever keys the chord is made of (Alt+Shift, Ctrl+Shift,
                // Caps Lock, Super+Space), it ends the word: letters typed on
                // both sides of a switch are not one word in any layout.
                if layout_chord {
                    ctx.learning.invalidate_last();
                    word.clear();
                    context_langs.note_layout_chord();
                }

                if pressed {
                    // A modifier pressed while a tap candidate is held turns it
                    // into a normal combination. Flush the candidate before
                    // Shift/Ctrl so XKB and the app observe the original
                    // ordering.
                    if tap.is_down() {
                        tap.other_key();
                        forward_pending(&ctx, &mut pending_tap);
                        ctx.learning.invalidate_last();
                        word.clear();
                        if !layout_chord {
                            context_langs.clear();
                        }
                        let _ = ctx.injector.forward(&[event]);
                        continue;
                    }
                    if keys.tap_action(code).is_some() && !others_held {
                        tap.press(code, false, pointer_generation);
                        if Modifier::of_code(code).is_some_and(Modifier::withholds_press) {
                            pending_tap = Some(event);
                        } else {
                            let _ = ctx.injector.forward(&[event]);
                        }
                        continue;
                    }
                    // An Alt that is not a tap — pressed on top of Shift/Ctrl,
                    // or not bound on this keyboard — opens menus or starts a
                    // system/application combination, not part of the word
                    // currently buffered.
                    if is_alt_code(code) {
                        ctx.learning.invalidate_last();
                        word.clear();
                        if !layout_chord {
                            context_langs.clear();
                        }
                    }
                    let _ = ctx.injector.forward(&[event]);
                    continue;
                }

                match tap.release(code, pointer_generation) {
                    TapRelease::Tap => {
                        let action = keys.tap_action(code).filter(|action| {
                            !ctx.state.hotkeys.recorder.is_active()
                                && (!action.respects_app_policy()
                                    || crate::focused_app_allows_manual(
                                        &ctx.state,
                                        ctx.windows.as_ref(),
                                    ))
                        });
                        if let Some(action) = action {
                            // Neither half of a held-back tap reaches an
                            // enabled app, preserving its selection; a tap
                            // that was never held back already has.
                            if pending_tap.take().is_none() {
                                let _ = ctx.injector.forward(&[event]);
                            }
                            fire(
                                &ctx,
                                action,
                                vendor,
                                &mut word,
                                &mut context_langs,
                                &held_modifiers,
                            );
                        } else {
                            // "Disabled here" means transparent: do not steal
                            // even the manual hotkey. Say so: a tap that did
                            // nothing is otherwise invisible in the log.
                            if keys.tap_action(code).is_some() {
                                pulinto_core::diag!("Tap {code} passed to the application: PuLinTo is off here");
                            }
                            forward_pending(&ctx, &mut pending_tap);
                            let _ = ctx.injector.forward(&[event]);
                        }
                    }
                    TapRelease::Combination => {
                        forward_pending(&ctx, &mut pending_tap);
                        let _ = ctx.injector.forward(&[event]);
                    }
                    TapRelease::Other => {
                        let _ = ctx.injector.forward(&[event]);
                    }
                }
                continue;
            }

            if !pressed {
                let _ = ctx.injector.forward(&[event]);
                continue;
            }

            // A hotkey chord. The toggle and the report belong to PuLinTo and
            // work even in a default-blocked terminal or IDE, so the user can
            // opt that application in without opening settings, and file a
            // report right when something went wrong — the daemon may well
            // have been switched off by then. Word and selection stand down in
            // an application where PuLinTo is off, like a tap does. Unlike
            // ordinary combinations the whole chord is swallowed: it is not
            // meant for the application.
            if let Some(action) = keys.chord_action(code, &held_modifiers) {
                if !action.respects_app_policy()
                    || crate::focused_app_allows_manual(&ctx.state, ctx.windows.as_ref())
                {
                    // A held-back Alt is part of this chord; the application
                    // has seen neither half of it and must not see one now.
                    if pending_tap.take().is_some() {
                        if let Some(held) = tap.consume() {
                            swallowed_releases.push(held);
                        }
                    } else {
                        tap.other_key();
                    }
                    swallowed_releases.push(code);
                    fire(
                        &ctx,
                        action,
                        vendor,
                        &mut word,
                        &mut context_langs,
                        &held_modifiers,
                    );
                    continue;
                }
            }

            // Any key while a tap candidate is down makes it a combination,
            // not a hotkey.
            if tap.is_down() {
                tap.other_key();
                forward_pending(&ctx, &mut pending_tap);
            }
            // Alt+Tab/F4/letter, Ctrl+C, Super+key belong wholly to the desktop
            // or focused application. Do not interpret Tab as our delimiter or
            // add the key to the word buffer — a stray «c» from Ctrl+C would
            // otherwise be converted by the next hotkey. Shift alone is typing.
            if held_modifiers.iter().any(|held| !matches!(*held, 42 | 54)) {
                ctx.learning.invalidate_last();
                word.clear();
                context_langs.clear();
                // Ctrl+Tab, Ctrl+W, Alt+Tab: the selection of the tab or
                // window just left must not pass for a fresh one here.
                if let Some(xr) = ctx.selection.as_deref() {
                    xr.note_user_input_at(arrived);
                }
                let _ = ctx.injector.forward(&[event]);
                continue;
            }

            if let Some(xr) = ctx.selection.as_deref() {
                xr.note_user_input_at(arrived);
            }

            // The user typed something new — the cursor moved away from the last
            // replacement, and it cannot be cycled any more: erasing would land in someone
            // else's place. A new replacement (if one happens below) is remembered afresh.
            ctx.learning.invalidate_last();

            // The cursor moved; what we believe sits to its left is no longer
            // true, so the word is abandoned rather than corrected.
            if grab::is_navigation(key) {
                word.clear();
                context_langs.clear();
                let _ = ctx.injector.forward(&[event]);
                continue;
            }

            match key {
                Key::KEY_BACKSPACE => {
                    // On an empty word this eats the space before the caret:
                    // the short words behind it are no longer one space away.
                    if word.is_empty() {
                        context_langs.short_tail.clear();
                    }
                    word.pop();
                    let _ = ctx.injector.forward(&[event]);
                }
                Key::KEY_ESC => {
                    word.clear();
                    context_langs.short_tail.clear();
                    let _ = ctx.injector.forward(&[event]);
                }
                Key::KEY_SPACE | Key::KEY_ENTER | Key::KEY_KPENTER | Key::KEY_TAB => {
                    handle_delimiter(
                        &ctx,
                        &mut word,
                        &mut context_langs,
                        &mut swallowed_releases,
                        &held_modifiers,
                        event,
                        shift_down,
                    );
                }
                _ => {
                    // Letters are letters on both layouts; anything else has to
                    // be asked of every loaded layout, because the comma key is
                    // `,` on one and the letter `б` on another — and a word must
                    // not end on a letter.
                    let letter =
                        capture::key_to_char(key).is_some_and(|ch| ch.is_ascii_alphabetic());
                    let current = scanword::current_profile(
                        ctx.layout.as_ref().as_ref(),
                        &ctx.profiles,
                        &word,
                    );
                    let here = ctx.profiles[current].layout();
                    let others = ctx
                        .profiles
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| *index != current)
                        .map(|(_, profile)| profile.layout());
                    // The full stop of an initial typed on the wrong layout:
                    // one capital Latin letter in the buffer, then the key that
                    // is `.` on another layout but not on this one (`&` on
                    // Shift+7 against the Apple Russian layout, `/` against
                    // the plain one). It joins the word instead of ending it,
                    // so the boundary can judge the pair (`cascade::Stage::
                    // Initial`) and retype both keys. Anything longer (`R&D`)
                    // is a token the stop rules keep.
                    if !letter
                        && scanword::ends_an_initial(&word, here, others.clone(), code, shift_down)
                    {
                        let _ = ctx.injector.forward(&[event]);
                        word.push(code, shift_down);
                        continue;
                    }
                    // A hyphen between letters is part of the word (`по-русски`,
                    // `de-commit`): judged with it at the boundary rather than
                    // cutting the word in two, each half too short to be sure of.
                    if !letter && code == Key::KEY_MINUS.code() && !shift_down && !word.is_empty() {
                        let _ = ctx.injector.forward(&[event]);
                        word.push(code, shift_down);
                        continue;
                    }
                    let word_key = letter
                        || match scanword::classify(here, others, code, shift_down) {
                            scanword::KeyRole::Letter => true,
                            scanword::KeyRole::Ambiguous(_) => {
                                // Punctuation here, a letter on another layout
                                // (`.` is `ю`, `,` is `б`). Deciding at the key
                                // cut unknown words in two (`нейрокомпь.теры`),
                                // and that cut could never be undone. The key
                                // joins the word, marked; the boundary judges
                                // the whole token and, if a prefix of it is a
                                // word of its own, splits it there (`plan_word`,
                                // `split_token`).
                                let _ = ctx.injector.forward(&[event]);
                                word.push_ambiguous(code, shift_down);
                                continue;
                            }
                            _ => false,
                        };
                    if word_key {
                        // Ordinary character: the user sees it immediately.
                        let _ = ctx.injector.forward(&[event]);
                        word.push(code, shift_down);
                    } else if pulinto_input::injector::position_of_keycode(code).is_some() {
                        handle_delimiter(
                            &ctx,
                            &mut word,
                            &mut context_langs,
                            &mut swallowed_releases,
                            &held_modifiers,
                            event,
                            shift_down,
                        );
                    } else {
                        let _ = ctx.injector.forward(&[event]);
                    }
                }
            }
        }
    }
    // The reader has stopped: the device is gone or the emergency combination
    // released it. Its verdict is what the caller needs.
    reader.join().unwrap_or(RunExit::Disconnected)
}

/// What a hotkey does, however it was pressed.
fn fire(
    ctx: &GrabContext,
    action: Action,
    vendor: u16,
    word: &mut ScanWord,
    context_langs: &mut Context,
    held_modifiers: &[u16],
) {
    pulinto_core::diag!(
        "Hotkey: {} (set {}, vendor {vendor:#06x})",
        action.id(),
        ctx.state.hotkeys.scheme_for(vendor).id()
    );
    match action {
        Action::Word => manual_toggle(ctx, word, context_langs, held_modifiers),
        Action::Selection => {
            if selection_cycle(ctx, held_modifiers) {
                word.clear();
                context_langs.clear();
            }
        }
        Action::ToggleApp => {
            word.clear();
            context_langs.clear();
            ctx.learning.invalidate_last();
            if let Err(error) = crate::toggle_current_application(&ctx.state, ctx.windows.as_ref())
            {
                eprintln!("Cannot switch PuLinTo for the current application: {error}");
            }
        }
        Action::AppleLayout => {
            word.clear();
            context_langs.clear();
            crate::keyboard::toggle_apple_layout(ctx.layout.as_ref().as_ref(), &ctx.state);
        }
        Action::Report => {
            // The word was dropped by the chord already (a combination with
            // Alt is somebody else's by the general rule), but the registry
            // keeps the last eight decisions: there is something to file.
            let selection = ctx.windows.as_ref().and_then(|windows| windows.selection());
            let focused = ctx
                .windows
                .as_ref()
                .and_then(|windows| windows.focused_class());
            match ctx.reports.report(selection.as_deref(), focused.as_deref()) {
                Ok(path) => println!("Bug report written: {}", path.display()),
                Err(error) => eprintln!("Report not written: {error}"),
            }
        }
    }
}

/// The user's own layout hotkey, seen on its way through. With an XKB group
/// option the compositor switches on the press that completes the chord, so the
/// daemon can know the new layout at once instead of ~35 ms later from the
/// desktop — and without the desktop at all on a stand. Sides do not matter:
/// in `grp:alt_shift_toggle` the Alt keys switch on level 2, which any Shift
/// selects, and the Shift keys use XKB's `PC_ALT_LEVEL2`, whose modifier is
/// the virtual `Alt` with no side — left Alt + right Shift switches too.
/// Until the desktop reports the switch the daemon would otherwise hold the
/// old group, and a replacement in that window retypes the word in it.
/// Records a layout switch the user is typing; true when `code` completes the
/// chord that switches layouts here.
fn note_user_layout_switch(ctx: &GrabContext, code: u16, held: &[u16]) -> bool {
    let Some(chord) = ctx.layout.layout_chord().filter(|chord| chord.ordered) else {
        return false;
    };
    if !chord.keys.iter().all(|key| is_modifier_code(*key)) {
        return false;
    }
    let left = |key: u16| match key {
        97 => 29,
        54 => 42,
        100 => 56,
        126 => 125,
        other => other,
    };
    let held: Vec<u16> = held.iter().map(|key| left(*key)).collect();
    let chord_keys: Vec<u16> = chord.keys.iter().map(|key| left(*key)).collect();
    let complete =
        chord_keys.contains(&left(code)) && chord_keys.iter().all(|key| held.contains(key));
    if complete {
        let count = ctx.layout.layout_ids().len().max(2);
        if let Ok(current) = ctx.layout.get_current_group() {
            let next = ((usize::from(current) + 1) % count) as u8;
            ctx.layout.note_group(next);
            pulinto_core::diag!("Layout switched by the user: group {current} → {next}");
        }
    }
    complete
}

fn forward_pending(ctx: &GrabContext, pending: &mut Option<InputEvent>) {
    if let Some(event) = pending.take() {
        let _ = ctx.injector.forward(&[event]);
    }
}

/// The delimiter is withheld, the cascade runs, and only then is it released —
/// after a correction if one is due. This is the ordering that makes Enter safe.
fn handle_delimiter(
    ctx: &GrabContext,
    word: &mut ScanWord,
    context_langs: &mut Context,
    swallowed_releases: &mut Vec<u16>,
    held_modifiers: &[u16],
    delimiter: InputEvent,
    shift_down: bool,
) {
    let started = Instant::now();
    let auto_on = ctx.state.auto_enabled.load(Ordering::SeqCst);
    // Only the cascade's own verdict below may keep or use the short words
    // behind the caret; every other way out of here leaves them behind.
    let mut tail = std::mem::take(&mut context_langs.short_tail);

    let app_mode = crate::focused_app_decision(&ctx.state, ctx.windows.as_ref())
        .map(|(_, decision)| decision.mode)
        .unwrap_or(crate::xwindow::AppMode::Auto);

    // Manual-only applications still remember the just-finished word so Alt
    // after a space works exactly as it does in automatic mode. Fully-off
    // applications keep no editable snapshot and receive the key unchanged.
    if app_mode != crate::xwindow::AppMode::Auto {
        if app_mode == crate::xwindow::AppMode::Manual && !word.is_empty() {
            let current =
                scanword::current_profile(ctx.layout.as_ref().as_ref(), &ctx.profiles, word);
            let shown = word.shown_profile().unwrap_or(current);
            let original = word.original_profile().unwrap_or(current);
            remember_finished_word(ctx, word, original, shown, &delimiter, shift_down);
        } else {
            ctx.learning.invalidate_last();
        }
        let _ = ctx.injector.forward(&[delimiter, syn()]);
        word.clear();
        context_langs.clear();
        return;
    }

    if word.is_empty() {
        let _ = ctx.injector.forward(&[delimiter, syn()]);
        word.clear();
        return;
    }

    let current = scanword::current_profile(ctx.layout.as_ref().as_ref(), &ctx.profiles, word);
    let shown = word.shown_profile().unwrap_or(current);
    let original = word.original_profile().unwrap_or(current);
    let text = word.text(ctx.profiles[current].layout());
    let focused_class = ctx
        .windows
        .as_ref()
        .and_then(crate::xwindow::XWindows::focused_class);

    // A manual Alt has already changed the word before the delimiter, or automation is off.
    // Either way the word must still be remembered: the next Alt after the
    // space has to cycle exactly it, not some old word corrected earlier.
    if word.already_converted() || !auto_on {
        remember_finished_word(ctx, word, original, shown, &delimiter, shift_down);
        let _ = ctx.injector.forward(&[delimiter, syn()]);
        word.clear();
        return;
    }

    // The never/always lists apply here too. Grab mode used to consult
    // neither — an oversight this task closes alongside making the lists
    // persistent, since a word the user explicitly excluded should not be
    // auto-corrected just because the daemon happens to be in grab mode.
    let exceptions = ctx.state.exceptions.blocking_lock();
    if exceptions.is_word_excluded_in(&text, focused_class.as_deref()) {
        drop(exceptions);
        remember_finished_word(ctx, word, current, current, &delimiter, shift_down);
        let _ = ctx.injector.forward(&[delimiter, syn()]);
        word.clear();
        return;
    }

    // A word the user has already taken back once. Grab mode used to ignore
    // this and re-apply the correction the user had just rejected, which is
    // the "it keeps fighting me" complaint in its purest form.
    if ctx.learning.is_reverted(&text) {
        drop(exceptions);
        remember_finished_word(ctx, word, current, current, &delimiter, shift_down);
        let _ = ctx.injector.forward(&[delimiter, syn()]);
        word.clear();
        return;
    }

    // Exact personal replacements are separate from language recognition. A
    // source may include the punctuation key that ended the scanner token
    // (`ещ\\`), in which case that key is part of the typo and is consumed.
    let delimited_typo = match delim_of(
        &delimiter,
        shift_down,
        Some(ctx.profiles[current].layout()),
        Some(ctx.profiles[current].layout()),
    ) {
        Delim::Char(ch) => ctx
            .state
            .autocorrect
            .blocking_lock()
            .fix_delimited_typo_in(&text, ch, focused_class.as_deref())
            .map(|fixed| (ch, fixed)),
        _ => None,
    };
    if let Some((delimiter_ch, fixed)) = delimited_typo {
        drop(exceptions);
        pulinto_core::diag!(
            "User auto-replacement: {} → {} characters",
            text.chars().count() + usize::from(!delimiter_ch.is_whitespace()),
            fixed.chars().count()
        );
        crate::debug::text_checked(&[&text, &fixed], || format!("auto-replacement: «{text}» → «{fixed}»"));

        if let Err(error) = replace::retype_text(
            &ctx.injector,
            ctx.profiles[current].layout(),
            word.len(),
            &fixed,
            replace::Cursor::AtWord,
            held_modifiers,
        ) {
            eprintln!("Safe auto-replacement is impossible ({error}), the text is untouched");
            let _ = ctx.injector.forward(&[delimiter, syn()]);
            word.clear();
            return;
        }
        swallowed_releases.push(delimiter.code());
        ctx.learning.invalidate_last();
        ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
        remember(context_langs, ctx.profiles[current].layout().id);
        word.clear();
        return;
    }

    // Ordinary exact rules end at (rather than contain) the separator. They
    // are checked before the language cascade and without its three-key floor,
    // so a short abbreviation can expand into a full phrase.
    let exact_replacement = ctx
        .state
        .autocorrect
        .blocking_lock()
        .fix_user_word_in(&text, focused_class.as_deref());
    if let Some(fixed) = exact_replacement {
        drop(exceptions);
        pulinto_core::diag!(
            "User auto-replacement: {} → {} characters",
            text.chars().count(),
            fixed.chars().count()
        );
        crate::debug::text_checked(&[&text, &fixed], || format!("auto-replacement: «{text}» → «{fixed}»"));

        if let Err(error) = replace::retype_text(
            &ctx.injector,
            ctx.profiles[current].layout(),
            word.len(),
            &fixed,
            replace::Cursor::AtWord,
            held_modifiers,
        ) {
            eprintln!("Safe auto-replacement is impossible ({error}), the text is untouched");
            let _ = ctx.injector.forward(&[delimiter, syn()]);
            word.clear();
            return;
        }
        let _ = ctx.injector.forward(&[delimiter, syn()]);
        ctx.learning.invalidate_last();
        ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
        remember(context_langs, ctx.profiles[current].layout().id);
        word.clear();
        return;
    }

    let refs = profile::refs(&ctx.profiles);

    // The doubtful keys get their final say here, where the token is finished.
    // A token that turns out to be two words with a stop between them is left
    // alone: converting it as one produced `надоюпредставили` from `надо.` plus
    // `представили`. Splitting it properly is the next step; until then a missed
    // correction costs a hotkey and eaten text costs trust.
    if let scanword::WordPlan::Split(position) =
        scanword::plan_word(word, &refs, current, context_langs)
    {
        // A split only if the left word really was typed on this layout. If
        // it converts to a language where the key is a letter, the typist
        // was on that language all along — and a Russian typist puts the
        // full stop on another key, so this one was `ю`. The token is then
        // one word for the cascade.
        let strokes = word.replay_keys();
        let left_text = ScanWord::render_keys(&strokes[..position], ctx.profiles[current].layout());
        let left = cascade::decide(&left_text, &refs, current, context_langs);
        // A right part that is a word of this layout's own language was meant
        // as typed (`afqk.txt`, `cfqn.ru`: a file name, a domain) — the stop
        // is a stop, and the left word is judged on its own.
        let right_text =
            ScanWord::render_keys(&strokes[position + 1..], ctx.profiles[current].layout());
        // So is a domain or a file extension after a full stop (`cfqn.ru`,
        // `main.py`, `.mp3`), which the cascade cannot judge on its own. A
        // list, not a length: `yfuhtdf.ncz` is `нагреваются`, one word with
        // the `ю` key inside, and `ncz` is no extension.
        let (mark_code, mark_shift) = strokes[position];
        let stop_here = pulinto_input::injector::position_of_keycode(mark_code).and_then(|p| {
            let level = if mark_shift {
                pulinto_core::layout::Level::Shifted
            } else {
                pulinto_core::layout::Level::Unshifted
            };
            ctx.profiles[current].layout().char_at(p, level)
        }) == Some('.');
        let right_is_a_word_here = !right_text.is_empty() && {
            let right = cascade::decide(&right_text, &refs, current, context_langs);
            !right.convert
                && (right.stage == cascade::Stage::Dictionary
                    || (stop_here && pulinto_core::stop_rules::is_dot_suffix(&right_text)))
        };
        let key_is_letter_there = left.convert
            && !right_is_a_word_here
            && pulinto_core::layout::Layout::by_id(left.layout_id)
                .and_then(|layout| {
                    let (code, shift) = strokes[position];
                    let position = pulinto_input::injector::position_of_keycode(code)?;
                    let level = if shift {
                        pulinto_core::layout::Level::Shifted
                    } else {
                        pulinto_core::layout::Level::Unshifted
                    };
                    layout.char_at(position, level)
                })
                .is_some_and(char::is_alphabetic);
        if !key_is_letter_there {
            drop(exceptions);
            split_token(
                ctx,
                word,
                context_langs,
                held_modifiers,
                current,
                position,
                None,
            );
            // No `Fix` for the pair: Alt would erase the whole token and retype
            // every stroke under one layout — two words, two languages, one
            // key. The user's Alt here does nothing, which is the honest answer.
            let _ = ctx.injector.forward(&[delimiter, syn()]);
            word.clear();
            return;
        }
        pulinto_core::diag!(
            "The mark at position {position} is a letter: the left part is Russian, and the full stop is on another key there"
        );
    }

    // `шз-адрес`: a Latin abbreviation left on the Russian layout in front of a
    // hyphen and a Russian word. Only the abbreviation moves; judged as one
    // token the whole would be turned to `ip-flhtc`.
    if let Some((position, target)) =
        cascade::latin_prefix_compound(&text, &refs, current, context_langs)
    {
        let strokes = word.replay_keys();
        if strokes.len() == text.chars().count() && strokes[position].0 == Key::KEY_MINUS.code() {
            drop(exceptions);
            pulinto_core::diag!(
                "Compound «abbreviation-hyphen-word» ({} characters): left part → {target}",
                text.chars().count()
            );
            crate::debug::text_checked(&[&text], || format!("compound: «{text}», left part → {target}"));
            split_token(
                ctx,
                word,
                context_langs,
                held_modifiers,
                current,
                position,
                Some(target),
            );
            // No `Fix` for the pair, as after any split: Alt would retype the
            // whole token under one layout.
            let _ = ctx.injector.forward(&[delimiter, syn()]);
            word.clear();
            return;
        }
    }

    // Right after the layout chord a lone letter is not judged by the words
    // before the switch: it is kept, and goes into the short-word tail below,
    // where the next word may still carry it along (`b еру` → `и ты`). Any
    // other word settles whether the switch was meant.
    let after_chord = std::mem::take(&mut context_langs.after_chord);
    let mut decision = if after_chord && word.len() == 1 {
        cascade::Decision {
            convert: false,
            layout_id: ctx.profiles[current].layout().id,
            text: text.clone(),
            stage: cascade::Stage::Undecided,
            doubtful: false,
        }
    } else {
        cascade::decide(&text, &refs, current, context_langs)
    };

    // Same override as observe mode (`engine.rs`): "always convert" wins over
    // a cascade "keep", but never over a stop rule. Unlike observe
    // mode we don't need `decision.text`: the replay mechanism retypes the
    // original scan codes under the new keyboard group, so flipping
    // `convert`/`layout_id` is enough to drive the switch below.
    if !decision.convert
        && !matches!(decision.stage, cascade::Stage::StopRule(_))
        && exceptions.is_always_word_in(&text, focused_class.as_deref())
    {
        let target_index = (current + 1) % ctx.profiles.len();
        decision.layout_id = ctx.profiles[target_index].layout().id;
        decision.convert = true;
    }
    drop(exceptions);

    // Every verdict goes into the registry: "should have corrected and did not" is a
    // complaint as good as any. The snapshot is taken before execution, and whether
    // the replacement reached the screen is filled in by the outcome: delivery here is
    // synchronous, and the answer is known, not assumed as in observe mode.
    let mut observed = snapshot(
        ctx.layout.as_ref().as_ref(),
        &ctx.profiles,
        word,
        &text,
        context_langs,
        &decision,
    );

    crate::debug::decision(|| {
        format!(
            "{}, {} characters, stage {:?}, layout {}, {} ms",
            if decision.convert { "replace" } else { "keep" },
            text.chars().count(),
            decision.stage,
            ctx.profiles[current].layout().id,
            started.elapsed().as_millis()
        )
    });
    // Words the stop rules took out (passwords, addresses) never reach the log.
    if !matches!(decision.stage, cascade::Stage::StopRule(_)) {
        crate::debug::text(|| {
            if decision.convert {
                format!("«{text}» → «{}»", decision.text)
            } else {
                format!("«{text}» kept")
            }
        });
    }

    let mut delimiter_sent = false;
    if decision.convert {
        pulinto_core::diag!(
            "Replacement: {} characters, {} → {} (stage: {:?}, decided in {} ms)",
            text.chars().count(),
            ctx.profiles[current].layout().id,
            decision.layout_id,
            decision.stage,
            started.elapsed().as_millis()
        );

        let strokes = word.replay_keys();

        // In X11, deliver the whole correction (erase, converted text and
        // delimiter) through one X connection. The old uinput path changes
        // the XKB group and then replays scan codes through a second channel;
        // under load the tail of the following word can be lost. XTest keeps
        // the operation ordered and inserts the already-decided characters.
        if let Some(xr) = ctx.xinject.as_deref() {
            let target = pulinto_core::layout::Layout::by_id(decision.layout_id);
            let delim = delim_of(
                &delimiter,
                shift_down,
                Some(ctx.profiles[current].layout()),
                target,
            );
            let shown = ctx
                .profiles
                .iter()
                .position(|profile| profile.layout().id == decision.layout_id)
                .unwrap_or(current);
            let shown_layout = target.unwrap_or(ctx.profiles[shown].layout());
            // The short and doubtful words behind go with the word here too,
            // as text: `ща еру` → `of the` in one transaction.
            let behind = tail_to_rewrite(&tail, &ctx.profiles, shown);
            let (erase, replacement) =
                with_tail_text(behind, shown_layout, word.len(), &decision.text);
            if !behind.is_empty() {
                pulinto_core::diag!("Along with the word — short words behind: {}", behind.len());
            }

            match xr.replace_word(erase, &replacement, delim, held_modifiers) {
                Ok(()) => {
                    // The delimiter was emitted inside the XTest transaction;
                    // suppress its physical release or the application would
                    // receive a release without the matching press.
                    swallowed_releases.push(delimiter.code());
                    // A bare group lock is dropped by the first real key, so the
                    // switch to Russian never stuck after an XTest replacement.
                    if let Err(error) =
                        activate_target_layout(ctx, decision.layout_id, held_modifiers)
                    {
                        eprintln!(
                            "The layout could not be switched ({error}), the text is already corrected"
                        );
                    }
                    ctx.learning.remember(replace::Fix {
                        strokes,
                        original: text.clone(),
                        shown,
                        delimiter: safe_word_delimiter(
                            &delimiter,
                            shift_down,
                            ctx.profiles[current].layout(),
                            shown_layout,
                        ),
                    });
                    word.mark_converted();
                    observed.applied = true;
                    ctx.reports.note(observed);
                    ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
                    remember(context_langs, decision.layout_id);
                    word.clear();
                    return;
                }
                Err(error) => {
                    // XTest validates the complete transaction before the
                    // first destructive key. If it cannot run, leave the
                    // user's word intact and pass the withheld delimiter on.
                    eprintln!("XTest delivery failed ({error}), the word is untouched");
                    ctx.reports.note(observed);
                    let _ = ctx.injector.forward(&[delimiter, syn()]);
                    word.clear();
                    return;
                }
            }
        }

        // Replaying the user's physical keys is deterministic in Chromium,
        // GTK and terminals. Dynamic X11 keycode remapping is not: Chromium
        // may identify a scratch key by its old physical meaning (notably
        // NumPad digits), producing strings such as `у3а2ит0` from `удалить`.
        let to = ctx
            .profiles
            .iter()
            .position(|profile| profile.layout().id == decision.layout_id)
            .unwrap_or(current);
        let behind = tail_to_rewrite(&tail, &ctx.profiles, to);
        let mut replayed = Vec::new();
        for short in behind {
            replayed.extend_from_slice(&short.strokes);
            replayed.push((Key::KEY_SPACE.code(), false));
        }
        if !behind.is_empty() {
            pulinto_core::diag!("Along with the word — short words behind: {}", behind.len());
        }
        replayed.extend_from_slice(&strokes);
        if let Err(error) = replace::replay_under(
            &ctx.injector,
            ctx.layout.as_ref().as_ref(),
            &replayed,
            decision.layout_id,
            replace::Cursor::AtWord,
            held_modifiers,
        ) {
            eprintln!("Safe layout switch is impossible ({error}), the text is untouched");
            ctx.reports.note(observed);
            remember_finished_word(ctx, word, current, current, &delimiter, shift_down);
            let _ = ctx.injector.forward(&[delimiter, syn()]);
            word.clear();
            return;
        }

        ctx.learning.remember(replace::Fix {
            strokes,
            original: text.clone(),
            shown: ctx
                .profiles
                .iter()
                .position(|p| p.layout().id == decision.layout_id)
                .unwrap_or(0),
            delimiter: safe_word_delimiter(
                &delimiter,
                shift_down,
                ctx.profiles[current].layout(),
                ctx.profiles
                    .iter()
                    .find(|profile| profile.layout().id == decision.layout_id)
                    .map(|profile| profile.layout())
                    .unwrap_or(ctx.profiles[current].layout()),
            ),
        });

        // The keymap is now on the target layout, where the delimiter's own key
        // may draw something else: `.` is `ю` on the Russian layout. Type the
        // character the user meant with the key the target layout has for it.
        let target_layout = ctx
            .profiles
            .iter()
            .find(|profile| profile.layout().id == decision.layout_id)
            .map(|profile| profile.layout())
            .unwrap_or(ctx.profiles[current].layout());
        if let Some(key) = retyped_delimiter(
            &delimiter,
            shift_down,
            ctx.profiles[current].layout(),
            target_layout,
        ) {
            let typed = ctx
                .injector
                .release_modifiers(held_modifiers)
                .and_then(|()| ctx.injector.tap(key.0, key.1))
                .and_then(|()| ctx.injector.restore_modifiers(held_modifiers));
            if let Err(error) = typed {
                eprintln!("Delimiter not typed ({error})");
            }
            swallowed_releases.push(delimiter.code());
            delimiter_sent = true;
        }

        word.mark_converted();
        observed.applied = true;
        ctx.reports.note(observed);
        ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
        remember(context_langs, decision.layout_id);
        // A converted collision word stays open to the word after it: shown
        // in the target profile now, one space from whatever comes next.
        if decision.doubtful && delimiter.code() == Key::KEY_SPACE.code() {
            context_langs.short_tail.push(ShortWordBehind {
                strokes: word.replay_keys(),
                shown: ctx
                    .profiles
                    .iter()
                    .position(|p| p.layout().id == decision.layout_id)
                    .unwrap_or(current),
                doubtful: true,
            });
        }
    } else {
        ctx.reports.note(observed);
        // A lone letter left unjudged after the chord is no evidence of the
        // language: remembered as `us`, it would rob the next word of its
        // Russian context (`в т. гл.` typed on `us` — `в` kept, `т.` lost).
        // The word after it decides for both, through the tail.
        if !(after_chord && word.len() == 1) {
            remember(context_langs, decision.layout_id);
        }
        // A word kept in its own language settles the doubtful words before
        // it: `her` chosen by frequency, then a Russian word typed on the
        // Russian layout — `her` was `рук` after all. They are erased along
        // with this word and typed again under this layout.
        if !matches!(
            decision.stage,
            cascade::Stage::Undecided | cascade::Stage::StopRule(_)
        ) {
            let behind = tail_to_rewrite(&tail, &ctx.profiles, current);
            if let (Some(xr), true) = (
                ctx.xinject.as_deref(),
                !behind.is_empty() && behind.iter().any(|b| b.doubtful),
            ) {
                // X11: the same rewrite as text, delimiter inside the transaction.
                let here = ctx.profiles[current].layout();
                let (erase, replacement) = with_tail_text(behind, here, word.len(), &text);
                let delim = delim_of(&delimiter, shift_down, Some(here), Some(here));
                match xr.replace_word(erase, &replacement, delim, held_modifiers) {
                    Ok(()) => {
                        swallowed_releases.push(delimiter.code());
                        delimiter_sent = true;
                        pulinto_core::diag!(
                            "Word behind rewritten after the next one: {} (→ {}, XTest)",
                            behind.len(),
                            here.id
                        );
                    }
                    Err(error) => eprintln!("Word behind not rewritten ({error})"),
                }
                tail = Vec::new();
            } else if ctx.xinject.is_none()
                && !behind.is_empty()
                && behind.iter().any(|b| b.doubtful)
            {
                let mut replayed = Vec::new();
                for short in behind {
                    replayed.extend_from_slice(&short.strokes);
                    replayed.push((Key::KEY_SPACE.code(), false));
                }
                replayed.extend_from_slice(&word.replay_keys());
                match replace::replay_under(
                    &ctx.injector,
                    ctx.layout.as_ref().as_ref(),
                    &replayed,
                    ctx.profiles[current].layout().id,
                    replace::Cursor::AtWord,
                    held_modifiers,
                ) {
                    Ok(_) => pulinto_core::diag!(
                        "Word behind rewritten after the next one: {} (→ {})",
                        behind.len(),
                        ctx.profiles[current].layout().id
                    ),
                    Err(error) => eprintln!("Word behind not rewritten ({error})"),
                }
                tail = Vec::new();
            }
        }
        remember_finished_word(ctx, word, current, current, &delimiter, shift_down);
        // A short word left as typed, one space before whatever comes next:
        // the next word may still show it belongs to the other language. A
        // collision word frequency alone decided is kept the same way.
        if (word.len() <= 2 || decision.doubtful) && delimiter.code() == Key::KEY_SPACE.code() {
            context_langs.short_tail = tail;
            context_langs.short_tail.push(ShortWordBehind {
                strokes: word.replay_keys(),
                shown: current,
                doubtful: decision.doubtful,
            });
        }
    }

    // Only now does the delimiter reach the application.
    if !delimiter_sent {
        let _ = ctx.injector.forward(&[delimiter, syn()]);
    }
    // If the daemon took time to process (e.g. conversion happened), the RELEASE event
    // is likely already in the kernel queue. When this function returns, the loop will
    // fetch and forward the RELEASE immediately, resulting in a 0ms hold time.
    // Xorg/GTK may drop 0ms keypresses, so we enforce a minimum hold time here.
    std::thread::sleep(std::time::Duration::from_millis(10));
    word.clear();
}

/// How to end the word on X delivery. Space, Enter and Tab are the same in any
/// layout, so they are struck with the same key. The mark key is decided by the
/// **target** layout: the user typed the word in it mentally, and `/` gives a full stop there.
///
/// The exception is a key that is a letter in the target layout (`.` is `ю`,
/// `,` is `б`). It gets here only if the dictionary has already decided that
/// it is a mark and not a letter of the word; so what stays is what the key drew
/// where it was pressed. Otherwise the word gets an extra letter: `привет.` came out as `приветю`.
fn delim_of(
    delimiter: &InputEvent,
    shift_down: bool,
    source: Option<&'static pulinto_core::layout::Layout>,
    target: Option<&'static pulinto_core::layout::Layout>,
) -> Delim {
    let code = delimiter.code();
    match pulinto_input::injector::position_of_keycode(code) {
        Some(position) => {
            let level = if shift_down {
                pulinto_core::layout::Level::Shifted
            } else {
                pulinto_core::layout::Level::Unshifted
            };
            let drawn = |layout: Option<&'static pulinto_core::layout::Layout>| {
                layout.and_then(|layout| layout.char_at(position, level))
            };
            drawn(target)
                .filter(|ch| !ch.is_alphabetic())
                .or_else(|| drawn(source).filter(|ch| !ch.is_alphabetic()))
                .map(Delim::Char)
                .unwrap_or(Delim::Key(code))
        }
        None => Delim::Key(code),
    }
}

/// Key and Shift that type `ch` under `layout`, if the layout has it.
fn key_for(ch: char, layout: &pulinto_core::layout::Layout) -> Option<(u16, bool)> {
    if ch == ' ' {
        return Some((Key::KEY_SPACE.code(), false));
    }
    let (position, level) = layout.position_of(ch)?;
    let code = pulinto_input::evdev::POSITION_TO_EVDEV_KEYCODE.get(position)?;
    Some((*code, level == pulinto_core::layout::Level::Shifted))
}

/// After a scan-code replay the keymap is on `target`. If the delimiter's own
/// key would draw a different character there than the one decided for it,
/// the key that draws that character instead; `None` when the physical key is
/// already right (space, Enter, `/` → `.`) or the target has no such key.
fn retyped_delimiter(
    delimiter: &InputEvent,
    shift_down: bool,
    source: &'static pulinto_core::layout::Layout,
    target: &'static pulinto_core::layout::Layout,
) -> Option<(u16, bool)> {
    let Delim::Char(ch) = delim_of(delimiter, shift_down, Some(source), Some(target)) else {
        return None;
    };
    let key = key_for(ch, target)?;
    (key != (delimiter.code(), shift_down)).then_some(key)
}

/// A space and a printable mark can be safely erased together with the word and put back
/// without moving the cursor. Enter/Tab are deliberately not saved: a Backspace after
/// them may jump to another line or affect another UI element.
fn safe_word_delimiter(
    delimiter: &InputEvent,
    shift_down: bool,
    typed_layout: &'static pulinto_core::layout::Layout,
    shown_layout: &'static pulinto_core::layout::Layout,
) -> Option<replace::WordDelimiter> {
    let code = delimiter.code();
    if code == Key::KEY_SPACE.code() {
        return Some(replace::WordDelimiter {
            key: (code, false),
            ch: ' ',
        });
    }
    if matches!(code, 15 | 28 | 96) {
        return None;
    }
    match delim_of(
        delimiter,
        shift_down,
        Some(typed_layout),
        Some(shown_layout),
    ) {
        Delim::Char(ch) => Some(replace::WordDelimiter {
            key: (code, shift_down),
            ch,
        }),
        _ => None,
    }
}

/// A snapshot of the word exactly at the position where Alt still has the right to change it.
/// It is made for every finished word, even if the cascade decided KEEP:
/// this is the forced conversion after a space.
/// A token the boundary found to be two words with a punctuation key between
/// them (`надо.представили`, `hello,` plus more): each side is judged on its
/// own, and whatever needs converting is erased and typed again — the left
/// word under its language, the key as the punctuation it is, the right word
/// under its own. Before this the whole token was left alone, so `yflj.`
/// stayed `yflj.` whenever more letters followed the stop.
///
/// `forced_left` names the layout the left part is known to belong to (the
/// Latin abbreviation of `шз-адрес`, found by `cascade::latin_prefix_compound`):
/// the cascade is not asked about either half then, the left moves, the right
/// stays as typed.
fn split_token(
    ctx: &GrabContext,
    word: &ScanWord,
    context_langs: &mut Context,
    held_modifiers: &[u16],
    current: usize,
    position: usize,
    forced_left: Option<&'static str>,
) {
    let strokes = word.replay_keys();
    let (left, rest) = strokes.split_at(position);
    let Some((&key, right)) = rest.split_first() else {
        return;
    };
    let refs = profile::refs(&ctx.profiles);
    let here = ctx.profiles[current].layout();
    let left_text = ScanWord::render_keys(left, here);
    let right_text = ScanWord::render_keys(right, here);
    let (left_to, right_to) = if let Some(to) = forced_left {
        remember(context_langs, to);
        remember(context_langs, here.id);
        (Some(to), None)
    } else {
        let left_decision = cascade::decide(&left_text, &refs, current, context_langs);
        remember(context_langs, left_decision.layout_id);
        let right_decision = (!right.is_empty())
            .then(|| cascade::decide(&right_text, &refs, current, context_langs));
        if let Some(decision) = &right_decision {
            remember(context_langs, decision.layout_id);
        }
        (
            left_decision.convert.then_some(left_decision.layout_id),
            right_decision
                .as_ref()
                .and_then(|d| d.convert.then_some(d.layout_id)),
        )
    };
    if left_to.is_none() && right_to.is_none() {
        pulinto_core::diag!(
            "Two-word token ({}+{} characters): both stay in place",
            left_text.chars().count(),
            right_text.chars().count()
        );
        return;
    }
    // The key as the punctuation it draws here; under the left word's
    // language the same mark may sit on another key.
    let key_event = InputEvent::new(EventType::KEY, key.0, 1);
    let mark = match delim_of(&key_event, key.1, Some(here), Some(here)) {
        Delim::Char(ch) => ch,
        _ => return,
    };
    let layout_of = |id: &str| {
        ctx.profiles
            .iter()
            .find(|profile| profile.layout().id == id)
            .map(|profile| profile.layout())
            .unwrap_or(here)
    };
    pulinto_core::diag!(
        "Two-word token ({}+{} characters): left → {}, right → {}",
        left_text.chars().count(),
        right_text.chars().count(),
        left_to.unwrap_or("in place"),
        right_to.unwrap_or("in place")
    );
    crate::debug::text_checked(&[&left_text, &right_text], || {
        format!(
            "token: «{left_text}»{mark}«{right_text}» → left {}, right {}",
            left_to.unwrap_or("in place"),
            right_to.unwrap_or("in place")
        )
    });
    // `replay_under` erases as many characters as it replays, from the caret
    // back; the caret is after the right part, so a left rewrite must take
    // the whole token down and put all three pieces back.
    let result = (|| -> std::io::Result<()> {
        let layout = ctx.layout.as_ref().as_ref();
        if let Some(to) = left_to {
            let mark_key = key_for(mark, layout_of(to)).unwrap_or(key);
            if right.is_empty() || right_to == Some(to) {
                let mut all = left.to_vec();
                all.push(mark_key);
                all.extend_from_slice(right);
                replace::replay_under(
                    &ctx.injector,
                    layout,
                    &all,
                    to,
                    replace::Cursor::AtWord,
                    held_modifiers,
                )?;
            } else {
                // The right word stays as typed (or goes elsewhere): erase it by
                // hand, rewrite the left word and the mark, type it back under
                // its own layout.
                ctx.injector.release_modifiers(held_modifiers)?;
                ctx.injector.backspaces(right.len())?;
                ctx.injector.restore_modifiers(held_modifiers)?;
                let mut head = left.to_vec();
                head.push(mark_key);
                replace::replay_under(
                    &ctx.injector,
                    layout,
                    &head,
                    to,
                    replace::Cursor::AtWord,
                    held_modifiers,
                )?;
                let right_layout = right_to.unwrap_or(here.id);
                activate_target_layout(ctx, right_layout, held_modifiers)
                    .map_err(std::io::Error::other)?;
                ctx.injector.release_modifiers(held_modifiers)?;
                ctx.injector.replay(right)?;
                ctx.injector.restore_modifiers(held_modifiers)?;
            }
        } else if let Some(to) = right_to {
            replace::replay_under(
                &ctx.injector,
                layout,
                right,
                to,
                replace::Cursor::AtWord,
                held_modifiers,
            )?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
            ctx.learning.invalidate_last();
        }
        Err(error) => eprintln!("Two-word token not rewritten ({error})"),
    }
}

fn remember_finished_word(
    ctx: &GrabContext,
    word: &ScanWord,
    original: usize,
    shown: usize,
    delimiter: &InputEvent,
    shift_down: bool,
) {
    ctx.learning.remember(replace::Fix {
        strokes: word.replay_keys(),
        original: word.text(ctx.profiles[original].layout()),
        shown,
        delimiter: safe_word_delimiter(
            delimiter,
            shift_down,
            ctx.profiles[original].layout(),
            ctx.profiles[shown].layout(),
        ),
    });
}

fn syn() -> InputEvent {
    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0)
}

/// What a bare Alt tap does: convert the word being typed, or — when there is
/// nothing to convert — take back the last correction.
///
/// Grab mode had no manual hotkey at all: automatic conversion and the panic
/// combination were the entire interface, so a wrong correction could only be
/// fixed by hand. It now behaves exactly like observe mode, sharing both the
/// mechanic (`replace`) and the memory of what the user has rejected.
/// How long a selection or a remembered replacement is trusted when window
/// changes cannot be seen.
const BLIND_TRUST: std::time::Duration = std::time::Duration::from_secs(5);

fn watching_windows(ctx: &GrabContext) -> bool {
    ctx.windows
        .as_ref()
        .is_some_and(|windows| windows.focus_changes().watching())
}

fn manual_toggle(
    ctx: &GrabContext,
    word: &mut ScanWord,
    context_langs: &mut Context,
    held_modifiers: &[u16],
) {
    // The user has taken the words behind the caret into their own hands.
    context_langs.short_tail.clear();
    if !crate::focused_app_allows_manual(&ctx.state, ctx.windows.as_ref()) {
        word.clear();
        context_langs.clear();
        ctx.learning.invalidate_last();
        return;
    }
    // Without a watch on window changes (Wayland while the PuLinTo extension is
    // off) a window change made with the mouse wheel, a gesture or a device we
    // do not read goes unseen: what is remembered from the old window is then
    // trusted only for a few seconds, or Alt types it into the new one.
    if !watching_windows(ctx)
        && ctx
            .learning
            .remembered_at()
            .is_some_and(|at| at.elapsed() > BLIND_TRUST)
    {
        pulinto_core::diag!(
            "Window changes are not visible: the last replacement is older than {} s and is not cycled",
            BLIND_TRUST.as_secs()
        );
        ctx.learning.invalidate_last();
    }
    // ── 1. An active selection or its already started cycle ─────────────
    // A fresh PRIMARY matters more than the internal word buffer: PuLinTo does not see the mouse,
    // so after a selection that buffer almost inevitably describes another
    // range. A repeated Alt first continues the SelectionFix and does not ask
    // PRIMARY, which our own typing has already cleared.
    if selection_cycle(ctx, held_modifiers) {
        word.clear();
        context_langs.clear();
        return;
    }

    // ── 2. A word being typed: a cycle of its readings ─────────────────
    if !word.is_empty() {
        let from = word.shown_profile().unwrap_or_else(|| {
            scanword::current_profile(ctx.layout.as_ref().as_ref(), &ctx.profiles, word)
        });
        let to = (from + 1) % ctx.profiles.len();
        let text = word.text(ctx.profiles[from].layout());
        let strokes = word.replay_keys();

        // A learning signal for the "always" list — only on the first step of
        // the cycle: further steps go back and forth and express no will.
        if word.shown_profile().is_none() {
            let refs = profile::refs(&ctx.profiles);
            let would_auto = cascade::decide(&text, &refs, from, context_langs).convert;
            if !would_auto {
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
        crate::debug::text_checked(&[&text], || format!("Alt cycle: «{text}»"));

        if let Err(error) = replace::replay_under(
            &ctx.injector,
            ctx.layout.as_ref().as_ref(),
            &strokes,
            ctx.profiles[to].layout().id,
            replace::Cursor::AtWord,
            held_modifiers,
        ) {
            eprintln!("Safe layout switch is impossible ({error}), the word is untouched");
            return;
        }
        word.set_shown_from(from, to);
        word.mark_converted();
        ctx.state.conversion_count.fetch_add(1, Ordering::SeqCst);
        return;
    }

    // ── 3. The word is already ended by a delimiter: a cycle of the last replacement ──
    // Only while the user has typed nothing new: any key press
    // revokes the right to a cycle (see Learning::invalidate_last), or the erasing
    // would land in someone else's place — that is how `merge.` once became `mergepfntv`.
    if let Some(fix) = ctx.learning.take_last() {
        let Some(cycle) = replace::plan_word_cycle(&fix, &ctx.profiles) else {
            // Enter or Tab: erasing across the line edge eats someone else's text.
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
        crate::debug::text_checked(&[&current_text, &target_text], || {
            format!("Alt cycle of the last word: «{current_text}» → «{target_text}»")
        });

        let replaced = replace::replay_under(
            &ctx.injector,
            ctx.layout.as_ref().as_ref(),
            &fix.strokes,
            ctx.profiles[to].layout().id,
            replace::Cursor::PastDelimiter {
                // The character on screen, typed with the key the target
                // layout has for it — the physical key may be a letter there.
                key: key_for(cycle.delimiter.ch, ctx.profiles[to].layout())
                    .unwrap_or(cycle.delimiter.key),
            },
            held_modifiers,
        )
        .map(|_| ())
        .map_err(|error| error.to_string());

        match replaced {
            Ok(()) => {
                // Back to the original reading — a rejection of our
                // replacement, the word goes to the exceptions. Moved away from it — changed their mind.
                if target_text == fix.original {
                    ctx.learning.mark_reverted(&fix.original);
                    let mut exceptions = ctx.state.exceptions.blocking_lock();
                    exceptions.record_revert(&fix.original);
                } else {
                    // The first move away from the original KEEP is an explicit request to
                    // always convert this word.
                    if current_text == fix.original {
                        let mut exceptions = ctx.state.exceptions.blocking_lock();
                        exceptions.record_manual_conversion(&fix.original);
                    }
                    ctx.learning.unmark_reverted(&fix.original);
                }
                ctx.learning.remember(replace::Fix { shown: to, ..fix });
            }
            Err(error) => {
                eprintln!("XTest delivery failed during the cycle ({error})");
                ctx.learning.remember(fix);
            }
        }
    }
}

/// A single Alt on selected text: replace the selection with its reading in
/// the next layout. Typing over an active selection replaces it by itself —
/// there is nothing to erase. A repeated Alt cycles what was already inserted.
fn selection_cycle(ctx: &GrabContext, held_modifiers: &[u16]) -> bool {
    let Some(xr) = ctx.selection.as_deref() else {
        return false;
    };

    // A fresh selection replacement? Cycle it without reading PRIMARY: the result of our
    // previous typing is still selected and will be replaced by the very first character.
    if let Some(sel) = ctx.learning.take_selection() {
        let from = sel.shown;
        let to = (from + 1) % ctx.profiles.len();
        let target_text = pulinto_core::layout::convert(
            &sel.text,
            ctx.profiles[from].layout(),
            ctx.profiles[to].layout(),
        );
        pulinto_core::diag!(
            "Alt cycle of the selection: {} characters → layout {}",
            sel.text.chars().count(),
            ctx.profiles[to].layout().id
        );
        crate::debug::text_checked(&[&sel.text, &target_text], || {
            format!("Alt cycle of the selection: «{}» → «{target_text}»", sel.text)
        });
        let replacement = replace::SelectionFix {
            text: target_text.clone(),
            shown: to,
        };
        match replace_selection_and_activate(
            ctx,
            &target_text,
            ctx.profiles[to].layout().id,
            held_modifiers,
        ) {
            Ok(()) => {
                ctx.learning.remember_selection(replacement);
            }
            Err(error) => {
                eprintln!("XTest delivery failed on the selection ({error})");
                ctx.learning.remember_selection(sel);
            }
        }
        return true;
    }

    // The last click and the last focus change: a selection older than either
    // belongs to a place the caret has since left.
    let click = ctx
        .pointer_clicks
        .as_ref()
        .and_then(crate::xwindow::PointerClicks::last_click);
    let focus = ctx
        .windows
        .as_ref()
        .and_then(|windows| windows.focus_changes().last());
    let max_age = (!watching_windows(ctx)).then_some(BLIND_TRUST);
    let Some(text) = xr.read_primary(click, focus, max_age) else {
        return false;
    };

    // The direction of the first step is purely positional, no dictionary: if there is
    // Cyrillic, read it as Russian and convert to Latin, otherwise the other way round.
    let from = ctx
        .profiles
        .iter()
        .position(|p| {
            text.chars()
                .any(|ch| ch.is_alphabetic() && p.layout().contains(ch))
        })
        .unwrap_or(0);
    let to = (from + 1) % ctx.profiles.len();
    let target_text = pulinto_core::layout::convert(
        &text,
        ctx.profiles[from].layout(),
        ctx.profiles[to].layout(),
    );
    pulinto_core::diag!(
        "Selection conversion: {} characters, {} → {}",
        text.chars().count(),
        ctx.profiles[from].layout().id,
        ctx.profiles[to].layout().id
    );
    crate::debug::text_checked(&[&text, &target_text], || {
        format!("Selection conversion: «{text}» → «{target_text}»")
    });
    match replace_selection_and_activate(
        ctx,
        &target_text,
        ctx.profiles[to].layout().id,
        held_modifiers,
    ) {
        Ok(()) => {
            ctx.learning.remember_selection(replace::SelectionFix {
                text: target_text,
                shown: to,
            });
        }
        Err(error) => eprintln!("XTest delivery failed on the selection ({error})"),
    }
    true
}

/// Replace and reselect through the same uinput keyboard used for ordinary
/// pass-through. XTest remains read-only (PRIMARY observation): dynamically
/// remapping X keycodes is interpreted inconsistently by Chromium/GTK and can
/// turn some letters into keypad digits.
fn replace_selection_and_activate(
    ctx: &GrabContext,
    target_text: &str,
    layout_id: &str,
    held_modifiers: &[u16],
) -> Result<(), String> {
    let target = pulinto_core::layout::Layout::by_id(layout_id)
        .ok_or_else(|| format!("unknown layout {layout_id}"))?;
    // Validate every character before changing either text or layout.
    for ch in target_text.chars() {
        if ch != ' ' && target.position_of(ch).is_none() {
            return Err(format!("character missing from layout {layout_id}"));
        }
    }
    ctx.injector
        .release_modifiers(held_modifiers)
        .map_err(|error| error.to_string())?;
    if let Some(selection) = ctx.selection.as_deref() {
        selection.note_injection();
    }
    let operation = (|| {
        activate_target_layout(ctx, layout_id, held_modifiers)?;
        ctx.injector
            .type_text(target_text, target)
            .map_err(|error| error.to_string())?;
        ctx.injector
            .select_previous(target_text.chars().count())
            .map_err(|error| error.to_string())
    })();
    let restore = ctx
        .injector
        .restore_modifiers(held_modifiers)
        .map_err(|error| error.to_string());
    operation.and(restore)
}

/// A snapshot of a decision for the error registry (`Ctrl+Alt+\`): the decision itself and all
/// the state against which it was made. A decision that looks wrong on
/// the screen may have been right in the cascade and wrong in the mechanics — the report must tell them
/// apart, so `applied` is false at birth: an intent is not a delivery,
/// and the executor confirms it when the outcome is known.
fn snapshot(
    layout: &dyn LayoutController,
    profiles: &[Profile],
    word: &ScanWord,
    text: &str,
    context_langs: &[&'static str],
    decision: &cascade::Decision,
) -> crate::report::Observed {
    crate::report::Observed {
        at: crate::report::now_ms(),
        typed: text.to_string(),
        strokes: word.replay_keys(),
        readings: profiles
            .iter()
            .map(|profile| (profile.layout_id().to_string(), word.text(profile.layout())))
            .collect(),
        current_layout: layout
            .current_layout_id()
            .unwrap_or_else(|| "unknown".into()),
        loaded_layouts: layout.layout_ids(),
        context: context_langs
            .iter()
            .map(|lang| (*lang).to_string())
            .collect(),
        stage: format!("{:?}", decision.stage),
        converted_to: decision.convert.then(|| decision.text.clone()),
        applied: false,
    }
}

fn remember(context_langs: &mut Vec<&'static str>, layout_id: &'static str) {
    context_langs.insert(0, layout_id);
    context_langs.truncate(2);
}

/// Finish an XTEST replacement in a group that survives the next real key.
///
/// XTEST always reports group 0 on this Xorg server. Every XTEST replacement
/// therefore leaves the keyboard in group 0, regardless of the language it
/// printed. A direct XKB lock looks successful but is discarded on the first
/// uinput event. Starting explicitly at group 0 and advancing with the real
/// XKB `Alt+Shift` action on PuLinTo's forwarding device produces durable
/// state — exactly the path used by the user's own layout shortcut.
fn activate_target_layout(
    ctx: &GrabContext,
    layout_id: &str,
    held_modifiers: &[u16],
) -> Result<(), String> {
    // GNOME on Wayland: no group can be set from outside; type the hotkey.
    if let (Some(chord), Some(target)) = (ctx.layout.layout_chord(), ctx.layout.group_of(layout_id))
    {
        let layout = ctx.layout.as_ref().as_ref();
        replace::switch_by_chord(&ctx.injector, layout, &chord, target)
            .map_err(|error| error.to_string())?;
        // A selection is replaced on the user's explicit request, so a few
        // tens of milliseconds are affordable: wait for the desktop to report
        // the new group before typing into it. Once the text has gone in the
        // wrong layout, the selection is gone with it. `None` means reports
        // of chords are still on their way — ours or the user's — and only
        // the group reported after all of them is a confirmation.
        if layout.confirmed_group() != Some(target) {
            let deadline = Instant::now() + std::time::Duration::from_millis(300);
            while layout.confirmed_group() != Some(target) {
                if Instant::now() >= deadline {
                    println!(
                        "GNOME did not confirm the switch to group {target} within 300 ms — typing on faith"
                    );
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        return Ok(());
    }
    let Some(target_group) = ctx.layout.group_of(layout_id) else {
        ctx.layout
            .toggle_next()
            .map_err(|error| error.to_string())?;
        ctx.injector.wait_for_layout();
        return Ok(());
    };

    ctx.layout.set_group(0).map_err(|error| error.to_string())?;

    let shift_down = held_modifiers.iter().any(|code| *code == 42 || *code == 54);
    for _ in 0..target_group {
        ctx.injector
            .advance_layout_group(shift_down)
            .map_err(|error| error.to_string())?;
    }
    ctx.injector.wait_for_layout();

    pulinto_core::diag!("layout activated: {layout_id} → group {target_group}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub controller: answers what it is told and switches nothing.
    struct StubLayout {
        ids: Vec<String>,
    }

    impl LayoutController for StubLayout {
        fn name(&self) -> &'static str {
            "стенд"
        }
        fn get_current_group(&self) -> Result<u8, pulinto_layout::LayoutError> {
            Ok(0)
        }
        fn set_group(&self, _group: u8) -> Result<(), pulinto_layout::LayoutError> {
            Ok(())
        }
        fn layout_ids(&self) -> Vec<String> {
            self.ids.clone()
        }
    }

    /// The keys a user presses to type "ghbdtn" — g h b d t n.
    fn privet_strokes() -> ScanWord {
        let mut word = ScanWord::default();
        for keycode in [34u16, 35, 48, 32, 20, 49] {
            word.push(keycode, false);
        }
        word
    }

    /// The snapshot carries both readings of the word and the picture of the layouts — without them
    /// the report cannot tell a cascade error from a mechanics error. This was the first hole
    /// of grab mode: the report hotkey worked only in observe mode, while the working mode is
    /// grab, so there was nowhere to record a complaint exactly where it is needed.
    /// `ghbdtn.`: the dictionary has already decided the `.` key is a full stop,
    /// so the converted word must end in one — not in `ю`, which is what the
    /// same key draws on the Russian layout.
    #[test]
    fn a_full_stop_after_a_converted_word_stays_a_full_stop() {
        let ru = pulinto_core::layout::Layout::by_id("ru");
        let dot = InputEvent::new(EventType::KEY, 52, 1);
        let us = pulinto_core::layout::Layout::by_id("us");
        let shown = |delimiter: Delim| match delimiter {
            Delim::Char(ch) => format!("Char({ch:?})"),
            Delim::Key(code) => format!("Key({code})"),
            Delim::None => "None".to_string(),
        };
        assert_eq!(shown(delim_of(&dot, false, us, ru)), "Char('.')");
        // The comma key is `б` on the Russian layout: same rule.
        let comma = InputEvent::new(EventType::KEY, 51, 1);
        assert_eq!(shown(delim_of(&comma, false, us, ru)), "Char(',')");
        // `/` is punctuation on both layouts, and the target one decides: a full
        // stop where the Russian layout has it.
        let slash = InputEvent::new(EventType::KEY, 53, 1);
        assert_eq!(shown(delim_of(&slash, false, us, ru)), "Char('.')");
    }

    /// Scan-code path (Wayland): after `ghbdtn` is replayed under Russian the
    /// full-stop key would draw `ю`, so the full stop is typed with the key
    /// Russian has for it. A `/` typed on US already draws `.` there.
    #[test]
    fn after_a_replay_the_delimiter_keeps_its_character() {
        let us = pulinto_core::layout::Layout::by_id("us").unwrap();
        let ru = pulinto_core::layout::Layout::by_id("ru").unwrap();
        let dot = InputEvent::new(EventType::KEY, 52, 1);
        assert_eq!(retyped_delimiter(&dot, false, us, ru), Some((53, false)));
        let comma = InputEvent::new(EventType::KEY, 51, 1);
        assert_eq!(retyped_delimiter(&comma, false, us, ru), Some((53, true)));
        let slash = InputEvent::new(EventType::KEY, 53, 1);
        assert_eq!(retyped_delimiter(&slash, false, us, ru), None);
        let space = InputEvent::new(EventType::KEY, 57, 1);
        assert_eq!(retyped_delimiter(&space, false, us, ru), None);
    }

    fn behind(keys: &[u16], shown: usize) -> ShortWordBehind {
        ShortWordBehind {
            strokes: keys.iter().map(|code| (*code, false)).collect(),
            shown,
            doubtful: false,
        }
    }

    /// `рук` shown as `her` (us) by frequency alone: a Russian word next
    /// takes it back; a converted doubtful word goes the same way.
    #[test]
    fn a_doubtful_collision_word_follows_the_word_after_it() {
        let profiles = profile::all();
        let (us, ru) = (0, 1);
        let mut rook = behind(&[35, 18, 19], us); // h e r — the very keys of рук on ru
        rook.doubtful = true;
        assert_eq!(
            tail_to_rewrite(std::slice::from_ref(&rook), &profiles, ru).len(),
            1
        );
        // Towards a language where the reading is no word: left alone.
        let mut junk = behind(&[16, 17, 18], ru); // qwe → йцу
        junk.doubtful = true;
        assert!(tail_to_rewrite(std::slice::from_ref(&junk), &profiles, us).is_empty());
    }

    /// `ща еру` typed without switching from Russian: once `еру` becomes
    /// `the`, `ща` is `of`. Profiles: 0 = us, 1 = ru.
    #[test]
    fn a_short_non_word_behind_a_converted_word_goes_with_it() {
        let profiles = pulinto_data::profile::all();
        let us = profiles.iter().position(|p| p.layout_id() == "us").unwrap();
        let ru = profiles.iter().position(|p| p.layout_id() == "ru").unwrap();
        // O, F keys: `ща` on Russian, `of` on US.
        let tail = [behind(&[24, 33], ru)];
        assert_eq!(tail_to_rewrite(&tail, &profiles, us).len(), 1);
        // D: `d` on US, `в` on Russian.
        let tail = [behind(&[32], us)];
        assert_eq!(tail_to_rewrite(&tail, &profiles, ru).len(), 1);
    }

    /// A word in its own layout stays: `в` typed on Russian is not `d`, and
    /// `vs` is real English even before a Russian word.
    #[test]
    fn a_real_short_word_is_not_rewritten() {
        let profiles = pulinto_data::profile::all();
        let us = profiles.iter().position(|p| p.layout_id() == "us").unwrap();
        let ru = profiles.iter().position(|p| p.layout_id() == "ru").unwrap();
        let tail = [behind(&[32], ru)];
        assert!(tail_to_rewrite(&tail, &profiles, us).is_empty());
        if profiles[us].has_word_list() {
            // V, S: `vs` / `мы`.
            let tail = [behind(&[47, 31], us)];
            assert!(tail_to_rewrite(&tail, &profiles, ru).is_empty());
        }
        // Only the words nearest the caret go: a real word stops the walk.
        // `d в` → D(us) then D(ru): the nearer one is real, nothing moves.
        let tail = [behind(&[32], us), behind(&[32], ru)];
        assert!(tail_to_rewrite(&tail, &profiles, ru).is_empty());
    }

    #[test]
    fn a_snapshot_carries_every_reading_and_the_layout_state() {
        let profiles = pulinto_data::profile::all();
        let layout = StubLayout {
            ids: vec!["us".into(), "ru".into()],
        };
        let decision = cascade::Decision {
            convert: true,
            layout_id: "ru",
            text: "привет".into(),
            stage: cascade::Stage::Dictionary,
            doubtful: false,
        };

        let observed = snapshot(
            &layout,
            &profiles,
            &privet_strokes(),
            "ghbdtn",
            &["ru"],
            &decision,
        );

        assert_eq!(observed.typed, "ghbdtn");
        assert!(observed.readings.contains(&("us".into(), "ghbdtn".into())));
        assert!(observed.readings.contains(&("ru".into(), "привет".into())));
        assert_eq!(observed.current_layout, "us");
        assert_eq!(
            observed.loaded_layouts,
            vec!["us".to_string(), "ru".to_string()]
        );
        assert_eq!(observed.context, vec!["ru".to_string()]);
        assert_eq!(observed.converted_to.as_deref(), Some("привет"));
        assert!(
            !observed.applied,
            "доставку подтверждает исполнитель, не снимок"
        );
    }

    /// The layout may be unknown (Wayland): the snapshot says so honestly,
    /// and neither crashes nor makes it up.
    #[test]
    fn a_snapshot_without_a_layout_name_says_so() {
        let profiles = pulinto_data::profile::all();
        let layout = StubLayout { ids: Vec::new() };
        let decision = cascade::Decision {
            convert: false,
            layout_id: "us",
            text: "ghbdtn".into(),
            stage: cascade::Stage::Undecided,
            doubtful: false,
        };

        let observed = snapshot(
            &layout,
            &profiles,
            &privet_strokes(),
            "ghbdtn",
            &[],
            &decision,
        );

        assert_eq!(observed.current_layout, "unknown");
        assert!(observed.loaded_layouts.is_empty());
        assert_eq!(observed.converted_to, None);
        assert!(!observed.applied);
    }
}
