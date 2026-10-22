#!/usr/bin/env python3
"""Окно-приёмник для стенда под Wayland (и под Xwayland с GDK_BACKEND=x11).

Ничего не решает и не чинит — только записывает. В отличие от `sink.py` оно
не пишет поле целиком после каждого изменения (на 50 000 слов это квадрат
от объёма), а работает пачками: машинистка просит сдать пачку, окно
записывает текст поля одной строкой и очищает его.

Протокол — файлы в каталоге прогона:
  ready            окно поднялось;
  focus            «1», пока окно в фокусе, иначе «0» — машинистка печатает
                   только при «1», xdotool под Wayland фокус не видит;
  flush-req        номер пачки, которую пора сдать;
  flush-ack        номер последней сданной пачки;
  batches.jsonl    {"batch": N, "text": "...", "touched": [...]} на каждую
                   пачку; touched — номера слов, которые демон стирал (первое
                   стирание серии стоит сразу за словом).
"""
import json
import os
import sys
import time

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk  # noqa: E402

RUN_DIR = sys.argv[1]
TITLE = sys.argv[2] if len(sys.argv) > 2 else "PuLinTo sink"


def write_atomic(name, value):
    path = os.path.join(RUN_DIR, name)
    with open(path + ".tmp", "w", encoding="utf-8") as handle:
        handle.write(value)
    os.replace(path + ".tmp", path)


def read(name):
    try:
        with open(os.path.join(RUN_DIR, name), encoding="utf-8") as handle:
            return handle.read().strip()
    except OSError:
        return None


class Sink(Gtk.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app, title=TITLE)
        self.set_default_size(900, 300)
        self.view = Gtk.TextView()
        self.view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        scroller = Gtk.ScrolledWindow()
        scroller.set_child(self.view)
        self.set_child(scroller)
        self.batches = open(os.path.join(RUN_DIR, "batches.jsonl"), "a", encoding="utf-8")
        self.acked = None
        self.last_change = 0.0
        self.touched = set()
        self.erasing = False
        buffer = self.view.get_buffer()
        buffer.connect("delete-range", self.on_delete)
        buffer.connect("insert-text", self.on_insert)
        buffer.connect("changed", self.on_changed)
        self.connect("notify::is-active", self.on_active)
        GLib.timeout_add(30, self.poll)
        write_atomic("focus", "0")
        write_atomic("ready", "1")

    def on_active(self, *_):
        active = self.is_active()
        write_atomic("focus", "1" if active else "0")
        if active:
            self.view.grab_focus()

    def on_changed(self, *_):
        self.last_change = time.time()

    def on_delete(self, buffer, start, end):
        if not self.erasing:
            before = buffer.get_text(buffer.get_start_iter(), end, False)
            self.touched.add(len(before.split()) - 1)
        self.erasing = True

    def on_insert(self, *_):
        self.erasing = False

    def poll(self):
        request = read("flush-req")
        # Сдавать пачку только в тишине: демон мог ещё не доработать очередь.
        if request is not None and request != self.acked and time.time() - self.last_change > 0.5:
            buffer = self.view.get_buffer()
            start, end = buffer.get_bounds()
            text = buffer.get_text(start, end, False)
            self.batches.write(json.dumps({"batch": int(request), "text": text,
                                           "touched": sorted(self.touched)},
                                          ensure_ascii=False) + "\n")
            self.batches.flush()
            buffer.set_text("")
            self.touched.clear()
            self.erasing = False
            self.acked = request
            write_atomic("flush-ack", request)
        if read("quit") is not None:
            self.get_application().quit()
            return False
        return True


def on_activate(app):
    window = Sink(app)
    window.present()
    window.view.grab_focus()


app = Gtk.Application(application_id=None)
app.connect("activate", on_activate)
app.run([])
