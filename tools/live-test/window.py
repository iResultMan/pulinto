#!/usr/bin/env python3
"""Окно живого теста: эталонный текст сверху, поле набора снизу.

Ничего не решает и не чинит — только записывает, что дошло до приложения,
с отметками времени. Журнал потом сопоставляется с логом демона.

Запуск: window.py <файл-эталона> <файл-журнала>
"""
import sys
import time

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, GLib, Pango  # noqa: E402

REFERENCE = sys.argv[1]
LOG = sys.argv[2]


def now():
    return int(time.time() * 1000)


class LiveTest(Gtk.Window):
    def __init__(self):
        super().__init__(title="PuLinTo — живой тест")
        self.set_default_size(900, 520)
        self.set_keep_above(True)

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_margin_top(12)
        box.set_margin_bottom(12)
        box.set_margin_start(12)
        box.set_margin_end(12)
        self.add(box)

        header = Gtk.Label(label="Наберите этот текст ниже. Печатайте как обычно; "
                                 "на части слов нарочно «забудьте» переключить раскладку.")
        header.set_line_wrap(True)
        header.set_xalign(0)
        box.pack_start(header, False, False, 0)

        ref_text = open(REFERENCE, encoding="utf-8").read().strip()
        ref = Gtk.Label(label=ref_text)
        ref.set_line_wrap(True)
        ref.set_xalign(0)
        ref.set_selectable(False)
        ref.modify_font(Pango.FontDescription("Monospace 13"))
        frame = Gtk.Frame(label="Эталон")
        frame.add(ref)
        box.pack_start(frame, False, False, 0)

        self.view = Gtk.TextView()
        self.view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self.view.modify_font(Pango.FontDescription("Monospace 14"))
        scroll = Gtk.ScrolledWindow()
        scroll.add(self.view)
        typing_frame = Gtk.Frame(label="Ваш набор")
        typing_frame.add(scroll)
        box.pack_start(typing_frame, True, True, 0)

        self.log = open(LOG, "a", encoding="utf-8", buffering=1)
        self.log.write(f"{now()}\tstart\t{ref_text!r}\n")
        self.view.get_buffer().connect("changed", self.on_change)
        self.connect("destroy", self.finish)

    def on_change(self, buffer):
        start, end = buffer.get_bounds()
        text = buffer.get_text(start, end, False)
        self.log.write(f"{now()}\tbuffer\t{text!r}\n")

    def finish(self, *_):
        self.log.write(f"{now()}\tend\t-\n")
        self.log.close()
        Gtk.main_quit()


window = LiveTest()
window.show_all()
window.present()
Gtk.main()
