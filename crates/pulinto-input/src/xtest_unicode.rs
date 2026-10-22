//! Delivery of a replacement through XTest over a single X connection.
//!
//! The key property: erasing, typing and the delimiter all go through **one
//! channel**, the request queue of a single client, which the X server must
//! process in order. There is no "kernel against server" race here by
//! construction; the layout is not touched while typing at all: the characters
//! are inserted ready-made by temporarily binding free keycodes to Unicode
//! keysyms.
//!
//! The order inside one act is not arbitrary:
//!
//! 1. `change_keyboard_mapping` (one batch per portion of characters that
//!    fits) + `sync` — the map on the server. If there are more unique
//!    characters than free keycodes, the text goes in several portions, with
//!    no hidden length limit;
//! 2. Backspaces are **real** keys; while handling them the client has time to
//!    process the MappingNotify that arrived before them and re-read the map,
//!    so it is already fresh when the scratch keys arrive. The old code did the
//!    opposite (erase before the remap) and lost the first character, which
//!    it "sacrificed" to an invisible ZWS that stayed in the user's text;
//! 3. scratch key presses, one per character; the keysym already carries the
//!    case;
//! 4. the delimiter: space/Enter as a real key, a punctuation mark as a
//!    **character of the target layout** through the same batch. A full stop
//!    can no longer come out as the letter `ю`: the layout takes no part in
//!    its delivery;
//! 5. the final `sync` is a barrier, after which the temporary assignments are
//!    cleared back to `NoSymbol`. Until the server has confirmed both
//!    operations, we do not return outward (to the uinput hand-over of the
//!    grab).

use crate::{InputBackend, InputError};
use std::thread::sleep;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, Keysym, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt;

/// Pause after a confirmed remap: insurance for clients that re-read the map
/// not in the event handler but later.
const SETTLE_MS: u64 = 10;
/// Key hold time and the step between presses. Below 5 ms GTK sometimes
/// collapses events.
const KEY_PACE_MS: u64 = 5;
/// Arrows do not change the text and do not depend on the dynamic keymap, so
/// they can be delivered faster. That matters for re-selecting a paragraph.
const SELECTION_PACE_MS: u64 = 1;

/// A manual conversion types the text character by character and then moves
/// the selection boundary as many times. A huge document would block the
/// keyboard loop for tens of seconds, so the limit is deliberate and
/// predictable, not dependent on the number of distinct letters or on how
/// many X11 keycodes are free.
pub const MAX_SELECTION_CHARS: usize = 4096;

const X_BACKSPACE: u8 = 22;
const X_SHIFT: u8 = 50;
const X_LEFT: u8 = 113;
/// Modifier keycodes of the shared XTEST keyboard.  A previous daemon could
/// leave one pressed if it tried to "restore" a modifier that actually came
/// from the uinput pass-through device.  Physical release then reaches that
/// other device and can never release the XTEST key.
const X_MODIFIERS: [u8; 8] = [50, 62, 37, 105, 64, 108, 133, 134];

const EVDEV_SHIFTS: [u16; 2] = [42, 54];

fn modifiers_are_safe_for_xtest(held: &[u16]) -> bool {
    // Every scratch key has the same keysym on all four levels, so Shift does
    // not alter the injected Unicode.  Ctrl/Alt/Meta can turn Backspace into
    // an application command; refuse the replacement instead of attempting
    // to lift them on the wrong X input device.
    held.iter().all(|code| EVDEV_SHIFTS.contains(code))
}

/// How to end the word after typing.
pub enum Delim {
    /// A key that is the same in every layout: space, Enter, Tab.
    /// The value is an **evdev** code, converted to an X code inside.
    Key(u16),
    /// A mark that is drawn differently in different layouts: typed as a character.
    Char(char),
    /// Type nothing (manual conversion up to the delimiter).
    None,
}

fn char_to_keysym(ch: char) -> Keysym {
    let cp = ch as u32;
    if cp <= 0xFF {
        cp
    } else {
        0x0100_0000 | cp
    }
}

/// Split the sequence so that all of its unique characters fit into each
/// portion. Before, one table was built for the whole text, so the real limit
/// was not the length but about 17 distinct characters.
fn mapping_chunks(chars: &[char], capacity: usize) -> Option<Vec<Vec<char>>> {
    if chars.is_empty() {
        return Some(Vec::new());
    }
    if capacity == 0 {
        return None;
    }

    let mut chunks = Vec::new();
    let mut chunk = Vec::new();
    let mut unique = Vec::new();

    for &ch in chars {
        let is_new = !unique.contains(&ch);
        if is_new && unique.len() == capacity {
            chunks.push(std::mem::take(&mut chunk));
            unique.clear();
        }
        if !unique.contains(&ch) {
            unique.push(ch);
        }
        chunk.push(ch);
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    Some(chunks)
}

/// Our safely recognisable fingerprint on a free keycode after an emergency
/// exit. ASCII/Latin-1 are deliberately not accepted: a real space key looks
/// like a single ASCII keysym and was once wrongly captured by this heuristic
/// recovery. A normal end of an act now always clears the assignments, so the
/// fingerprint is only needed for Unicode orphans after a crash in the middle
/// of an insertion.
fn is_scratch_fingerprint(keysyms: &[Keysym]) -> bool {
    let Some(&first) = keysyms.first() else {
        return false;
    };
    (0x0100_0000..=0x0110_FFFF).contains(&first)
        && keysyms.iter().all(|&symbol| symbol == first || symbol == 0)
}

fn find_free_keycodes(
    conn: &RustConnection,
    min_kc: u8,
    max_kc: u8,
    limit: usize,
) -> Result<Vec<u8>, InputError> {
    let count = max_kc - min_kc + 1;
    let reply = xproto::get_keyboard_mapping(conn, min_kc, count)
        .map_err(|e| InputError::X11Error(e.to_string()))?
        .reply()
        .map_err(|e| InputError::X11Error(e.to_string()))?;
    let width = reply.keysyms_per_keycode as usize;

    let mut free = Vec::new();
    for i in 0..count as usize {
        let base = i * width;
        let slice = &reply.keysyms[base..base + width];
        // Free: empty, or our own orphaned remap. We write one Unicode keysym
        // into all four columns; such a fingerprint does not occur in real
        // layouts, and it tells a past session (including one that crashed
        // in the middle of a replacement) from someone else's key. Without
        // this every daemon restart would eat the pool for good.
        // The server answers with the width of the whole map (usually more
        // than four); our four columns are padded with NoSymbol, so zeros in
        // the tail are ours too.
        // Only our text ranges are allowed: XF86 keys (volume, player) live
        // above, and a similar fingerprint on them must not be touched.
        let ours = is_scratch_fingerprint(slice);
        if slice.iter().all(|&sym| sym == 0) || ours {
            free.push(min_kc + i as u8);
            if free.len() >= limit {
                break;
            }
        }
    }
    Ok(free)
}

pub struct XTestUnicodeBackend {
    conn: RustConnection,
    scratch_kcs: Vec<u8>,
    /// PRIMARY selection: a separate thread watches it on its own connection.
    selection: std::sync::Arc<SelectionWatch>,
    /// When we last typed ourselves. Our own typing makes the application
    /// re-grab PRIMARY ownership; such a "refresh" does not count as
    /// freshness.
    last_injection: std::sync::Mutex<std::time::Instant>,
    /// The user's last ordinary key. A selection is good for Alt only if
    /// PRIMARY changed after it; otherwise it is an old selection, and the
    /// hotkey refers to the word the user is typing now.
    last_user_input: std::sync::Mutex<std::time::Instant>,
}

/// Watching PRIMARY on a connection of its own.
///
/// XFIXES reports a change of the PRIMARY owner, that is, that the user
/// selected something. The event is stamped at the moment it arrives by a
/// thread that has nothing else to do: the freshness of a selection is
/// decided by comparing this stamp with the stamps of the key, the click and
/// the window change, so it has to be taken when the event happened, not when
/// we next got round to it. Before, the stamp was set when events were
/// drained from the queue, before reading PRIMARY, and an old selection
/// looked younger than a click that came after it.
struct SelectionWatch {
    conn: RustConnection,
    /// An invisible sink window for reading PRIMARY.
    window: u32,
    atom_utf8: u32,
    atom_prop: u32,
    /// When PRIMARY last changed owner. `None` — never since startup. In X
    /// PRIMARY lives forever, even when nothing is selected on screen, so
    /// without this stamp Alt would paste a selection long since cleared.
    changed: std::sync::Mutex<Option<std::time::Instant>>,
    /// The owner's answers to our `convert_selection` requests, from the thread.
    notify: std::sync::Mutex<std::sync::mpsc::Receiver<xproto::SelectionNotifyEvent>>,
}

impl SelectionWatch {
    fn start() -> Result<std::sync::Arc<Self>, InputError> {
        use x11rb::protocol::xfixes;
        use x11rb::protocol::Event;
        let x11 = |e: &dyn std::fmt::Display| InputError::X11Error(e.to_string());

        let (conn, screen_num) = x11rb::connect(None).map_err(|e| x11(&e))?;
        let root = conn.setup().roots[screen_num].root;
        let window = conn.generate_id().map_err(|e| x11(&e))?;
        xproto::create_window(
            &conn,
            0, // CopyFromParent
            window,
            root,
            -1,
            -1,
            1,
            1,
            0,
            xproto::WindowClass::INPUT_OUTPUT,
            0,
            &xproto::CreateWindowAux::new(),
        )
        .map_err(|e| x11(&e))?;
        let atom_utf8 = xproto::intern_atom(&conn, false, b"UTF8_STRING")
            .map_err(|e| x11(&e))?
            .reply()
            .map_err(|e| x11(&e))?
            .atom;
        let atom_prop = xproto::intern_atom(&conn, false, b"PULINTO_SELECTION")
            .map_err(|e| x11(&e))?
            .reply()
            .map_err(|e| x11(&e))?
            .atom;

        // Subscription to PRIMARY owner changes: the only way to tell a live
        // selection from one cleared long ago.
        let xfixes_ready = xfixes::query_version(&conn, 5, 0)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some()
            && xfixes::select_selection_input(
                &conn,
                window,
                xproto::AtomEnum::PRIMARY.into(),
                xfixes::SelectionEventMask::SET_SELECTION_OWNER
                    | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
            )
            .is_ok();
        if !xfixes_ready {
            eprintln!("XFIXES unavailable: selection conversion will be disabled");
        }
        conn.flush().map_err(|e| x11(&e))?;

        let (sender, receiver) = std::sync::mpsc::channel();
        let watch = std::sync::Arc::new(Self {
            conn,
            window,
            atom_utf8,
            atom_prop,
            changed: std::sync::Mutex::new(None),
            notify: std::sync::Mutex::new(receiver),
        });
        let watched = std::sync::Arc::clone(&watch);
        std::thread::spawn(move || loop {
            match watched.conn.wait_for_event() {
                Ok(Event::XfixesSelectionNotify(_)) => {
                    if let Ok(mut slot) = watched.changed.lock() {
                        *slot = Some(std::time::Instant::now());
                    }
                }
                Ok(Event::SelectionNotify(notify)) => {
                    if sender.send(notify).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("PRIMARY: selection watching stopped ({error})");
                    return;
                }
            }
        });
        Ok(watch)
    }
}

impl XTestUnicodeBackend {
    pub fn new() -> Result<Self, InputError> {
        Self::connect(true)
    }

    /// Only the PRIMARY watch, for Wayland: there the X server is Xwayland,
    /// which mutter keeps in sync with the Wayland selection, but XTest input
    /// would reach X windows only — so nothing is ever typed through this one,
    /// and it does not even touch the XTest keyboard on start.
    pub fn selection_only() -> Result<Self, InputError> {
        Self::connect(false)
    }

    fn connect(deliver: bool) -> Result<Self, InputError> {
        let (conn, _screen) =
            x11rb::connect(None).map_err(|e| InputError::X11Error(e.to_string()))?;
        let setup = conn.setup();

        let scratch_kcs = if deliver {
            find_free_keycodes(&conn, setup.min_keycode, setup.max_keycode, 32)?
        } else {
            Vec::new()
        };
        if deliver && scratch_kcs.len() < 4 {
            return Err(InputError::X11Error(
                "not enough free X11 keycodes to insert Unicode".into(),
            ));
        }
        let selection = SelectionWatch::start()?;
        let backend = Self {
            conn,
            scratch_kcs,
            selection,
            last_injection: std::sync::Mutex::new(std::time::Instant::now()),
            last_user_input: std::sync::Mutex::new(std::time::Instant::now()),
        };

        // XTEST's virtual keyboard outlives our client connection.  Heal a
        // modifier left pressed by an older/crashed daemon before accepting
        // any input.  Releases are idempotent and do not touch the physical or
        // PuLinTo uinput keyboards.
        if !deliver {
            println!("PRIMARY: watching the selection through Xwayland");
            return Ok(backend);
        }
        backend.release_xtest_modifiers()?;
        println!(
            "XTest PRIMARY watching is ready (no text is typed through XTest; {} spare keycodes)",
            backend.scratch_kcs.len()
        );
        Ok(backend)
    }

    /// Our own typing (uinput, under Wayland) is about to move the selection;
    /// that change must not pass for the user selecting something.
    pub fn note_injection(&self) {
        if let Ok(mut slot) = self.last_injection.lock() {
            *slot = std::time::Instant::now();
        }
    }

    fn require_delivery(&self) -> Result<(), InputError> {
        if self.scratch_kcs.is_empty() {
            return Err(InputError::InjectionError(
                "this connection only watches the selection".into(),
            ));
        }
        Ok(())
    }

    /// Mark real input before it goes to the application: the old selection
    /// will be older than this key, while a new one (made by it through
    /// Shift+arrows) will arrive later and stay usable for Alt.
    pub fn note_user_input(&self) {
        self.note_user_input_at(std::time::Instant::now());
    }

    /// The same, for a key that arrived at `at` — the reader's time, which the
    /// loop may reach much later while a replacement is being typed.
    pub fn note_user_input_at(&self, at: std::time::Instant) {
        if let Ok(mut slot) = self.last_user_input.lock() {
            if at > *slot {
                *slot = at;
            }
        }
    }

    /// Read the PRIMARY selection, but only a **live** one: selected after
    /// our last typing, after the user's last key, after the last mouse click
    /// (`click`; any button, since even a right-click context menu revokes
    /// the right to Alt) and after a window change (`focus`), which the
    /// keyboard says nothing about. The age of the selection alone does not
    /// matter: selected, got distracted for a minute, tapped Alt — it is
    /// still a request to convert the selection. Every refusal is explained
    /// in the log: without that, analysing "converts on the second tap" came
    /// down to guesses.
    /// `None` — there is no live selection, the owner did not answer within
    /// 300 ms, or sent an INCR transfer (giant selections are not supported
    /// in v1).
    /// `max_age`, when window changes cannot be seen: an older selection may
    /// belong to a window the user has since left.
    pub fn read_primary(
        &self,
        click: Option<std::time::Instant>,
        focus: Option<std::time::Instant>,
        max_age: Option<Duration>,
    ) -> Option<String> {
        let changed = (*self.selection.changed.lock().ok()?)?;
        let injected = *self.last_injection.lock().ok()?;
        let user_input = *self.last_user_input.lock().ok()?;
        let age = |since: std::time::Instant| since.saturating_duration_since(changed).as_millis();
        if changed <= injected + Duration::from_millis(300) {
            pulinto_core::diag!(
                "Selection ignored: our own typing re-grabbed PRIMARY ({} ms before it)",
                injected.saturating_duration_since(changed).as_millis()
            );
            return None;
        }
        if changed <= user_input {
            pulinto_core::diag!(
                "Selection is stale: typing happened after it ({} ms) — select again and tap Alt",
                age(user_input)
            );
            return None;
        }
        if let Some(click) = click.filter(|click| changed <= *click) {
            pulinto_core::diag!(
                "Selection is stale: a mouse click came after it ({} ms) — select again and \
                 tap Alt",
                age(click)
            );
            return None;
        }
        if let Some(focus) = focus.filter(|focus| changed <= *focus) {
            pulinto_core::diag!(
                "Selection is stale: the window changed after it ({} ms) — select again and \
                 tap Alt",
                age(focus)
            );
            return None;
        }
        if let Some(limit) = max_age.filter(|limit| changed.elapsed() > *limit) {
            pulinto_core::diag!(
                "Selection is not used: it is {} ms old and window changes are not visible \
                 (more than {} s) — select again and tap Alt",
                changed.elapsed().as_millis(),
                limit.as_secs()
            );
            return None;
        }

        let watch = &self.selection;
        let notify = watch.notify.lock().ok()?;
        // Late answers to earlier requests are not our answer.
        while notify.try_recv().is_ok() {}
        xproto::convert_selection(
            &watch.conn,
            watch.window,
            xproto::AtomEnum::PRIMARY.into(),
            watch.atom_utf8,
            watch.atom_prop,
            x11rb::CURRENT_TIME,
        )
        .ok()?;
        watch.conn.flush().ok()?;

        let Ok(reply) = notify.recv_timeout(Duration::from_millis(300)) else {
            println!("Selection not read: the PRIMARY owner did not answer within 300 ms");
            return None;
        };
        if reply.property == 0 {
            println!("No selection: the PRIMARY owner refused");
            return None;
        }
        let reply = xproto::get_property(
            &watch.conn,
            true, // delete after reading
            watch.window,
            watch.atom_prop,
            xproto::GetPropertyType::ANY,
            0,
            u32::MAX / 4,
        )
        .ok()?
        .reply()
        .ok()?;
        if reply.type_ != watch.atom_utf8 {
            eprintln!("PRIMARY arrived as non-UTF8 (INCR?), skipping the selection");
            return None;
        }
        let text = String::from_utf8(reply.value).ok()?;
        if text.is_empty() {
            println!("No selection: PRIMARY is empty");
        }
        (!text.is_empty()).then_some(text)
    }

    /// A whole replacement act: erase `erase` characters, type `text`,
    /// finish with the delimiter. Returns only after the server has
    /// confirmed processing of the whole queue (the final sync); until then
    /// events must not be handed out through other channels.
    pub fn replace_word(
        &self,
        erase: usize,
        text: &str,
        delim: Delim,
        held_modifiers: &[u16],
    ) -> Result<(), InputError> {
        self.require_delivery()?;
        if !modifiers_are_safe_for_xtest(held_modifiers) {
            return Err(InputError::InjectionError(
                "replacement while Ctrl/Alt/Meta is held is unsafe".into(),
            ));
        }

        // Characters that need a scratch key: the word plus the delimiter mark.
        let mut chars: Vec<char> = text.chars().collect();
        if let Delim::Char(ch) = &delim {
            chars.push(*ch);
        }

        let chunks = mapping_chunks(&chars, self.scratch_kcs.len()).ok_or_else(|| {
            InputError::InjectionError("no free scratch keycodes for Unicode".into())
        })?;

        // An empty replacement can still mean erasing only.
        if chunks.is_empty() {
            for _ in 0..erase {
                self.tap(X_BACKSPACE)?;
            }
        }

        // Each portion gets its own table. Before the first typing the map is
        // already confirmed by the server, so the original text is not yet
        // touched if preparing the first portion fails.
        for (chunk_index, chunk) in chunks.iter().enumerate() {
            let mut mapping: Vec<(char, u8)> = Vec::new();
            for &ch in chunk {
                if mapping.iter().any(|&(known, _)| known == ch) {
                    continue;
                }
                mapping.push((ch, self.scratch_kcs[mapping.len()]));
            }

            for &(ch, kc) in &mapping {
                let sym = char_to_keysym(ch);
                xproto::change_keyboard_mapping(&self.conn, 1, kc, 4, &[sym, sym, sym, sym])
                    .map_err(|e| InputError::InjectionError(e.to_string()))?;
            }
            self.conn
                .sync()
                .map_err(|e| InputError::InjectionError(e.to_string()))?;
            sleep(Duration::from_millis(SETTLE_MS));

            // A real Backspace also forces lazy clients to finish reading the
            // first MappingNotify before the scratch presses arrive.
            if chunk_index == 0 {
                for _ in 0..erase {
                    self.tap(X_BACKSPACE)?;
                }
            }

            for &ch in chunk {
                let kc = mapping
                    .iter()
                    .find(|&&(known, _)| known == ch)
                    .map(|&(_, kc)| kc)
                    .expect("the character was in the current portion");
                self.tap(kc)?;
            }
        }

        // A character delimiter was already the last element of `chars`.
        if let Delim::Key(evdev_code) = delim {
            self.tap((evdev_code + 8) as u8)?;
        }

        self.finish_injection()
    }

    /// Replace the current selection. Re-selecting is a separate step: between
    /// them the daemon synchronises the target layout with a service key,
    /// which would otherwise replace the already selected result itself.
    pub fn replace_selected_text(
        &self,
        text: &str,
        held_modifiers: &[u16],
    ) -> Result<(), InputError> {
        let count = text.chars().count();
        if count > MAX_SELECTION_CHARS {
            return Err(InputError::InjectionError(format!(
                "{count} characters selected, the safe limit is {MAX_SELECTION_CHARS}"
            )));
        }

        // The first printable character replaces the active selection, no
        // Backspace is needed.
        self.replace_word(0, text, Delim::None, held_modifiers)
    }

    /// Select the `count` characters right before the caret.
    pub fn select_previous(&self, count: usize) -> Result<(), InputError> {
        self.require_delivery()?;
        if count > MAX_SELECTION_CHARS {
            return Err(InputError::InjectionError(format!(
                "cannot re-select {count} characters, the safe limit is {MAX_SELECTION_CHARS}"
            )));
        }
        if count > 0 {
            self.fake_key(X_SHIFT, true)?;
            sleep(Duration::from_millis(SELECTION_PACE_MS));

            let mut move_result = Ok(());
            for _ in 0..count {
                if let Err(error) = self.tap_with_pace(X_LEFT, SELECTION_PACE_MS) {
                    move_result = Err(error);
                    break;
                }
            }

            // Do not leave Shift held even if an error hits mid-motion.
            let release_result = self.fake_key(X_SHIFT, false);
            sleep(Duration::from_millis(SELECTION_PACE_MS));
            move_result?;
            release_result?;
        }

        // Our own XFIXES event from the selection must be older than this
        // stamp, so that it is not taken for a new user action.
        self.finish_injection()
    }

    fn finish_injection(&self) -> Result<(), InputError> {
        // First wait until the last key is delivered to the application. A
        // short hold keeps a lazy client from seeing NoSymbol instead of the
        // not yet processed last scratch press.
        self.conn
            .sync()
            .map_err(|e| InputError::InjectionError(e.to_string()))?;
        sleep(Duration::from_millis(SETTLE_MS));

        // No temporary assignment may outlive a finished act. It was a left-over
        // ASCII binding that let the old heuristic take a real Space for a
        // scratch keycode and turn the space into a letter.
        for &keycode in &self.scratch_kcs {
            xproto::change_keyboard_mapping(&self.conn, 1, keycode, 4, &[0, 0, 0, 0])
                .map_err(|e| InputError::InjectionError(e.to_string()))?;
        }
        self.conn
            .sync()
            .map_err(|e| InputError::InjectionError(e.to_string()))?;

        // Typing/re-selecting may have re-grabbed PRIMARY ownership; such a
        // refresh must not pass for a fresh user action.
        if let Ok(mut slot) = self.last_injection.lock() {
            *slot = std::time::Instant::now();
        }
        Ok(())
    }

    fn release_xtest_modifiers(&self) -> Result<(), InputError> {
        for &keycode in &X_MODIFIERS {
            xtest::fake_input(
                &self.conn,
                KEY_RELEASE_EVENT,
                keycode,
                0,
                x11rb::NONE,
                0,
                0,
                0,
            )
            .map_err(|e| InputError::InjectionError(e.to_string()))?;
        }
        self.conn
            .sync()
            .map_err(|e| InputError::InjectionError(e.to_string()))
    }

    fn fake_key(&self, x_keycode: u8, press: bool) -> Result<(), InputError> {
        let kind = if press {
            KEY_PRESS_EVENT
        } else {
            KEY_RELEASE_EVENT
        };
        xtest::fake_input(&self.conn, kind, x_keycode, 0, x11rb::NONE, 0, 0, 0)
            .map_err(|e| InputError::InjectionError(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| InputError::InjectionError(e.to_string()))?;
        Ok(())
    }

    fn tap(&self, x_keycode: u8) -> Result<(), InputError> {
        self.tap_with_pace(x_keycode, KEY_PACE_MS)
    }

    fn tap_with_pace(&self, x_keycode: u8, pace_ms: u64) -> Result<(), InputError> {
        self.fake_key(x_keycode, true)?;
        sleep(Duration::from_millis(pace_ms));
        self.fake_key(x_keycode, false)?;
        sleep(Duration::from_millis(pace_ms));
        Ok(())
    }

    fn tap_shifted(&self, x_keycode: u8, shifted: bool) -> Result<(), InputError> {
        if shifted {
            self.fake_key(X_SHIFT, true)?;
            sleep(Duration::from_millis(KEY_PACE_MS));
        }
        self.tap(x_keycode)?;
        if shifted {
            self.fake_key(X_SHIFT, false)?;
            sleep(Duration::from_millis(KEY_PACE_MS));
        }
        Ok(())
    }

    pub fn type_unicode(&self, text: &str) -> Result<(), InputError> {
        self.require_delivery()?;
        self.replace_word(0, text, Delim::None, &[])
    }
}

impl InputBackend for XTestUnicodeBackend {
    fn name(&self) -> &'static str {
        "X11 Unicode (XTest, single channel)"
    }

    fn send_key_combination(&self, keycode: u8, modifiers: u16) -> Result<(), InputError> {
        self.tap_shifted(keycode, (modifiers & 1) != 0)
    }

    fn send_backspaces(&self, count: usize) -> Result<(), InputError> {
        for _ in 0..count {
            self.tap(X_BACKSPACE)?;
        }
        Ok(())
    }

    fn send_text_for_layout(
        &self,
        text: &str,
        _layout: &pulinto_core::layout::Layout,
    ) -> Result<(), InputError> {
        // The layout takes no part in delivery: characters are inserted ready-made.
        self.type_unicode(text)
    }

    fn send_left(&self) -> Result<(), InputError> {
        self.tap(113)
    }

    fn send_right(&self) -> Result<(), InputError> {
        self.tap(114)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_scratch_fingerprint, mapping_chunks, modifiers_are_safe_for_xtest};

    #[test]
    fn only_orphaned_unicode_scratch_mappings_are_reclaimed() {
        assert!(is_scratch_fingerprint(&[0x0100_0430; 4]));
        assert!(!is_scratch_fingerprint(&['a' as u32; 4]));
    }

    #[test]
    fn real_keyboard_and_xf86_mappings_are_not_claimed() {
        assert!(!is_scratch_fingerprint(&[
            'a' as u32, 'A' as u32, 'a' as u32, 'A' as u32
        ]));
        assert!(!is_scratch_fingerprint(&[0x1008_FF12; 4]));
        assert!(!is_scratch_fingerprint(&[' ' as u32, 0, ' ' as u32, 0]));
        assert!(!is_scratch_fingerprint(&[0; 4]));
    }

    #[test]
    fn long_text_is_split_by_unique_characters_without_losing_order() {
        let source: Vec<char> = "abacdefaghij".chars().collect();
        let chunks = mapping_chunks(&source, 4).expect("есть scratch keycode");

        assert!(chunks.iter().all(|chunk| {
            let mut unique = chunk.clone();
            unique.sort_unstable();
            unique.dedup();
            unique.len() <= 4
        }));
        assert_eq!(chunks.concat(), source);
        assert!(chunks.len() > 1);
    }

    #[test]
    fn repeated_characters_do_not_consume_extra_scratch_keycodes() {
        let source: Vec<char> = "aaaaabbbbbccccc".chars().collect();
        let chunks = mapping_chunks(&source, 3).unwrap();
        assert_eq!(chunks, vec![source]);
    }

    #[test]
    fn shift_is_safe_because_every_scratch_level_is_identical() {
        assert!(modifiers_are_safe_for_xtest(&[]));
        assert!(modifiers_are_safe_for_xtest(&[42]));
        assert!(modifiers_are_safe_for_xtest(&[54]));
    }

    #[test]
    fn command_modifiers_are_never_restored_on_the_xtest_device() {
        for code in [29, 97, 56, 100, 125, 126] {
            assert!(!modifiers_are_safe_for_xtest(&[code]));
        }
        assert!(!modifiers_are_safe_for_xtest(&[42, 29]));
    }
}
