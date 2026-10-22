#!/usr/bin/env python3
"""Покадровый просмотр выхода демона на виртуальном экране.

    trace_view.py <ctl/trace.log> <от, мс> <до, мс>

Прогоняет журнал нажатий через ту же модель xkb, что `virtual_screen.py`, и
печатает каждое нажатие в окне времени с хвостом экрана после него. Счёт
стирания, переключения раскладки и разделители видны глазами.
"""
import sys

sys.argv, args = sys.argv[:2] + ["0"], sys.argv[1:]
import importlib.util  # noqa: E402

spec = importlib.util.spec_from_file_location(
    "vs", __file__.replace("trace_view.py", "virtual_screen.py"))
vs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vs)

NAMES = {14: "BS", 42: "LSHIFT", 54: "RSHIFT", 56: "LALT", 57: "SPACE", 29: "LCTRL"}

trace, start, end = args[0], int(args[1]), int(args[2])
text, caret = [], 0
for line in open(trace, encoding="utf-8"):
    parts = line.rstrip("\n").split("\t")
    if len(parts) > 1 and parts[1] == "DROPPED":
        print(f"{parts[0]} *** SYN_DROPPED: ядро выбросило события ***")
        continue
    if parts[0] == "flush":
        text, caret = [], 0
        continue
    t, code, value = int(parts[0]), int(parts[1]), int(parts[2])
    keycode = code + 8
    if value in (1, 2):
        sym = vs.xkb.xkb_state_key_get_one_sym(vs.state, keycode)
        char = vs.xkb.xkb_state_key_get_utf32(vs.state, keycode)
        if sym == vs.XK_BACKSPACE:
            if caret > 0:
                del text[caret - 1]
                caret -= 1
        elif char >= 0x20 and char != 0x7F:
            text.insert(caret, chr(char))
            caret += 1
    if value in (0, 1):
        vs.xkb.xkb_state_update_key(vs.state, keycode, 1 if value == 1 else 0)
    if start <= t <= end and value == 1:
        group = vs.xkb.xkb_state_serialize_layout(vs.state, vs.XKB_STATE_LAYOUT_EFFECTIVE)
        name = NAMES.get(code, str(code))
        print(f"{t} {name:>6} g{group} …{''.join(text[max(0, caret - 50):caret])}")
