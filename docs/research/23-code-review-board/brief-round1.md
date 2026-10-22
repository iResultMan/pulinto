You are a senior systems reviewer. Do a deep, read-only code review of the PuLinTo keyboard-layout switcher daemon in this repository (Rust, Linux evdev/uinput, GNOME Wayland + X11). Do NOT modify any file. Do NOT run the daemon, tests or any program that touches /dev/input or /dev/uinput. Only read code and docs.

Context: the daemon grabs physical keyboards exclusively (EVIOCGRAB), re-emits every key through its own uinput device ("PuLinTo Virtual Keyboard"), buffers the current word as scan codes, and when a delimiter arrives decides whether the word was typed in the wrong layout; if so it erases the word with Backspaces and replays the same scan codes after switching the layout (on GNOME Wayland the switch is the user's own Alt+Shift chord typed through the same uinput device). A bare Alt tap converts the current word, the last word, or the current PRIMARY selection (read via Xwayland X11 PRIMARY with XFIXES owner-change tracking).

Read these files fully, in this order:
- docs/STATE.md (project state notes; the live setup is GNOME 50 on Wayland, grab mode)
- crates/pulintod/src/grab_engine.rs (the grab-mode event loop; main focus)
- crates/pulinto-input/src/injector.rs (uinput forwarding and replay)
- crates/pulintod/src/replace.rs (replacement mechanic, Learning)
- crates/pulinto-input/src/xtest_unicode.rs (PRIMARY selection reading: read_primary, note_user_input, note_injection)
- crates/pulinto-input/src/grab.rs (HeldKeys, key_transition)
- crates/pulintod/src/hotkeys.rs (only TapTracker and Modifier::withholds_press)
- crates/pulintod/src/xwindow.rs (PointerClicks::start_evdev, attach_pointers, XWindows::focused_class)
- crates/pulintod/src/shell_focus.rs and gnome-extension/pulinto@pulinto.ru/extension.js
- crates/pulinto-layout/src/gnome.rs

The owner reports three live symptoms on GNOME Wayland with an Apple keyboard (bare Alt tap = hotkey, Alt+Shift = layout switch):
1. Sometimes a letter is lost, most often the LAST letter of a word, during fast typing when the layout gets switched/corrected.
2. A selected text fragment is often (not always) converted only on the SECOND hotkey press, especially in a newly focused window.
3. Very rarely (maybe already fixed): text that looked like clipboard content got inserted when switching to a new window.

Task: find concrete, code-grounded root causes for each symptom, plus any other correctness bugs you notice on the input path (races, lost/stuck keys, wrong erase counts, stale state across focus changes, kernel/evdev/uinput semantics, Wayland/X11 differences). Think about kernel input-layer semantics for uinput devices, key rollover (next key pressed before previous released), compositor key repeat, ordering of events across two devices, and X11 selection semantics under Xwayland/mutter.

Output format (Russian language, Markdown, at most 1200 words total):
## Гипотезы по симптомам
For each symptom: root cause (file:function, exact mechanism), confidence (high/medium/low), how to verify, suggested fix (1-3 sentences).
## Другие замечания
Up to 8 bullets, each: file:function — issue — severity (critical/major/minor).
Do not restate the architecture. Do not list style nits. Cite exact function names.
