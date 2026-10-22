#!/usr/bin/env python3
"""Reproduces "the old selection is typed into another window" on live GNOME/Wayland
while the daemon cannot see window changes (PuLinTo focus extension off).

Window A holds «привет мир», its last word is selected (PRIMARY changes), then window B
takes the focus without any key or click (hide/show), and after WAIT seconds a single
bare Alt is tapped on a synthetic Apple keyboard. Correct: nothing appears in B.

The only key this script ever presses is a bare Alt, and only while its own window B is
the active one (asked from GTK, not from the extension). Usage:
    blind_repro.py WAIT [WAIT ...]
"""
import json
import subprocess
import sys
import threading
import time

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402
import evdev  # noqa: E402,F401
from evdev import UInput, ecodes as e  # noqa: E402

NAME = "PuLinTo Test Typist"
APPLE = 0x05AC


def ring():
    out = subprocess.run(
        ["busctl", "--user", "--json=short", "call", "ru.pulinto.Daemon1", "/ru/pulinto/Daemon1",
         "ru.pulinto.Daemon1", "GetDiagnostics"], capture_output=True, text=True).stdout
    return json.loads(out)["data"][0].splitlines() if out else []


def idle_ms():
    out = subprocess.run(
        ["gdbus", "call", "--session", "--dest", "org.gnome.Mutter.IdleMonitor", "--object-path",
         "/org/gnome/Mutter/IdleMonitor/Core", "--method", "org.gnome.Mutter.IdleMonitor.GetIdletime"],
        capture_output=True, text=True).stdout
    digits = "".join(ch for ch in out if ch.isdigit())
    return int(digits) if digits else 0


class Field:
    def __init__(self, title, text, x):
        self.window = Gtk.Window(title=title)
        self.window.set_default_size(420, 200)
        self.window.move(x, 80)
        self.view = Gtk.TextView()
        self.buffer = self.view.get_buffer()
        self.buffer.set_text(text)
        self.window.add(self.view)
        self.window.show_all()

    def text(self):
        return self.buffer.get_text(*self.buffer.get_bounds(), True)

    def reset(self, text):
        self.buffer.set_text(text)
        self.buffer.place_cursor(self.buffer.get_end_iter())

    def select_last_word(self):
        text = self.text()
        start = self.buffer.get_iter_at_offset(len(text) - len(text.split()[-1]))
        self.buffer.select_range(start, self.buffer.get_end_iter())

    def remap(self):
        self.window.hide()
        self.window.show_all()
        self.view.grab_focus()


def on_gtk(fn, *args):
    done = threading.Event()
    box = {}

    def run():
        box["r"] = fn(*args)
        done.set()
        return False

    GLib.idle_add(run)
    done.wait(5)
    return box.get("r")


def main():
    waits = [float(arg) for arg in sys.argv[1:]] or [6.0]
    if idle_ms() < 60000:
        raise SystemExit(f"CANCELLED: somebody is at the machine (idle {idle_ms()} ms)")
    a = Field("A", "привет мир", 80)
    b = Field("B", "", 560)
    seen = len(ring())
    ui = UInput({e.EV_KEY: list(range(1, 249))}, name=NAME, vendor=APPLE, product=0x0250, version=1)
    results = []
    try:
        for _ in range(100):
            if any(f"Grab: {NAME}" in line for line in ring()[-40:]):
                break
            time.sleep(0.1)
        else:
            raise SystemExit("the daemon did not grab the typist")
        time.sleep(0.5)

        def run():
            try:
                for wait in waits:
                    on_gtk(b.reset, "")
                    on_gtk(a.reset, "привет мир")
                    on_gtk(a.remap)
                    time.sleep(0.8)
                    before = len(ring())
                    on_gtk(a.select_last_word)
                    time.sleep(0.5)
                    on_gtk(b.remap)
                    time.sleep(0.8 + wait)
                    if not (on_gtk(b.window.is_active) and not on_gtk(a.window.is_active)):
                        results.append((wait, "SKIPPED: window B is not the active one"))
                        continue
                    ui.write(e.EV_KEY, e.KEY_LEFTALT, 1); ui.syn(); time.sleep(0.05)
                    ui.write(e.EV_KEY, e.KEY_LEFTALT, 0); ui.syn()
                    time.sleep(2.0)
                    lines = [line for line in ring()[before:] if "election" in line or "Alt" in line]
                    results.append((wait, f"A={on_gtk(a.text)!r} B={on_gtk(b.text)!r} log={lines}"))
            finally:
                GLib.idle_add(Gtk.main_quit)

        threading.Thread(target=run, daemon=True).start()
        Gtk.main()
    finally:
        ui.close()
    for wait, outcome in results:
        print(f"wait {wait:>4} s: {outcome}")


if __name__ == "__main__":
    main()
