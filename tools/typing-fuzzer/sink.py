#!/usr/bin/env python3
"""Окно-приёмник: показывает, что на самом деле дошло до приложения.

Ничего не решает и не чинит — только записывает. Нужно, чтобы отличить
«демон принял неверное решение» от «символы не дошли или дошли не те».
"""
import sys
import time

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, GLib  # noqa: E402

LOG = sys.argv[1] if len(sys.argv) > 1 else "/dev/stdout"
SECONDS = int(sys.argv[2]) if len(sys.argv) > 2 else 20


def now():
    return int(time.time() * 1000)


class Sink(Gtk.Window):
    def __init__(self):
        super().__init__(title="PuLinTo sink")
        self.set_default_size(700, 200)
        self.set_keep_above(True)
        self.view = Gtk.TextView()
        self.view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self.add(self.view)
        self.log = open(LOG, "a", encoding="utf-8", buffering=1)
        self.view.get_buffer().connect("changed", self.on_change)
        self.connect("destroy", Gtk.main_quit)
        GLib.timeout_add_seconds(SECONDS, self.finish)

    def on_change(self, buffer):
        start, end = buffer.get_bounds()
        text = buffer.get_text(start, end, False)
        self.log.write(f"{now()}\tbuffer\t{text!r}\n")

    def finish(self):
        self.log.write(f"{now()}\tend\t-\n")
        self.log.close()
        Gtk.main_quit()
        return False


window = Sink()
window.show_all()
window.present()
Gtk.main()
