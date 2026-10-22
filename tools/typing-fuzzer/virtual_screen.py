#!/usr/bin/env python3
"""Виртуальный экран: модель композитора Wayland на выходе демона.

Забирает виртуальную клавиатуру демона в эксклюзивный захват (EVIOCGRAB) —
до настоящего композитора не доходит ни одно нажатие — и прогоняет каждое
событие через libxkbcommon с той же раскладкой, что у GNOME (`us,ru`,
`grp:alt_shift_toggle`). Это ровно то, что делает клиент Wayland: переводит
keycode в символ по состоянию xkb, которое композитор присылает перед каждой
клавишей. Упорядоченность самого mutter замерена отдельно (450 из 450 слов в
живом окне без пауз); здесь проверяется всё остальное — решения демона,
счёт стирания, переключение аккордом, разделители — на десятках тысяч слов,
не занимая экран.

Нужен, когда живой экран недоступен (сессия заперта): печатать в экран
блокировки нельзя.

Протокол с машинисткой — тот же, что у `sink_wl.py` (каталог ctl):
  ready, focus («1», пока захват держится и демон жив), flush-req/flush-ack,
  batches.jsonl; плюс `layout` — текущая раскладка модели («us»/«ru»).

    virtual_screen.py <ctl> <pid демона> [раскладки] [опции xkb]
"""
import ctypes
import json
import os
import queue
import sys
import threading
import time

import evdev
from evdev import ecodes

CTL = sys.argv[1]
DAEMON_PID = int(sys.argv[2])
LAYOUTS = sys.argv[3] if len(sys.argv) > 3 else "us,ru"
OPTIONS = sys.argv[4] if len(sys.argv) > 4 else "grp:alt_shift_toggle"
# Варианты XKB в порядке раскладок («mac,mac»): модель должна рисовать ту же
# раскладку, что демон читает из настроек, иначе клавиша ё для одного буква,
# для другого «<», и слово режется по вине стенда.
VARIANTS = sys.argv[5] if len(sys.argv) > 5 else ""
DEVICE = "PuLinTo Virtual Keyboard"

xkb = ctypes.CDLL("libxkbcommon.so.0")


class RuleNames(ctypes.Structure):
    _fields_ = [(name, ctypes.c_char_p)
                for name in ("rules", "model", "layout", "variant", "options")]


xkb.xkb_context_new.restype = ctypes.c_void_p
xkb.xkb_keymap_new_from_names.restype = ctypes.c_void_p
xkb.xkb_keymap_new_from_names.argtypes = [ctypes.c_void_p, ctypes.POINTER(RuleNames), ctypes.c_int]
xkb.xkb_state_new.restype = ctypes.c_void_p
xkb.xkb_state_new.argtypes = [ctypes.c_void_p]
xkb.xkb_state_update_key.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_int]
xkb.xkb_state_key_get_utf32.restype = ctypes.c_uint32
xkb.xkb_state_key_get_utf32.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
xkb.xkb_state_key_get_one_sym.restype = ctypes.c_uint32
xkb.xkb_state_key_get_one_sym.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
xkb.xkb_state_serialize_layout.restype = ctypes.c_uint32
xkb.xkb_state_serialize_layout.argtypes = [ctypes.c_void_p, ctypes.c_int]

XKB_KEY_DOWN, XKB_KEY_UP = 1, 0
XKB_STATE_LAYOUT_EFFECTIVE = 1 << 7
XK_BACKSPACE = 0xFF08
XK_LEFT, XK_RIGHT = 0xFF51, 0xFF53

context = xkb.xkb_context_new(0)
names = RuleNames(b"evdev", b"pc105", LAYOUTS.encode(), VARIANTS.encode(), OPTIONS.encode())
keymap = xkb.xkb_keymap_new_from_names(context, ctypes.byref(names), 0)
if not keymap:
    sys.exit("xkbcommon: раскладка не собралась")
state = xkb.xkb_state_new(keymap)
LAYOUT_IDS = LAYOUTS.split(",")


def write_atomic(name, value):
    path = os.path.join(CTL, name)
    with open(path + ".tmp", "w", encoding="utf-8") as handle:
        handle.write(value)
    os.replace(path + ".tmp", path)


def read(name):
    try:
        with open(os.path.join(CTL, name), encoding="utf-8") as handle:
            return handle.read().strip()
    except OSError:
        return None


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def find_device(deadline):
    while time.time() < deadline:
        for path in evdev.list_devices():
            try:
                device = evdev.InputDevice(path)
            except OSError:
                continue
            if device.name == DEVICE:
                return device
            device.close()
        time.sleep(0.1)
    return None


def raise_priority():
    """Тот же nice −10 через rtkit, что просит себе демон: модель читает его
    виртуальную клавиатуру через 64-событийный буфер ядра и под нагрузкой
    отставала — ядро выбрасывало события (DROPPED в трассе), и прогон
    становился недействительным по вине стенда."""
    try:
        import gi
        gi.require_version("Gio", "2.0")
        from gi.repository import Gio, GLib
        tid = ctypes.CDLL(None).syscall(186)  # SYS_gettid на x86-64
        bus = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
        bus.call_sync("org.freedesktop.RealtimeKit1", "/org/freedesktop/RealtimeKit1",
                      "org.freedesktop.RealtimeKit1", "MakeThreadHighPriority",
                      GLib.Variant("(ti)", (tid, -10)), None, Gio.DBusCallFlags.NONE, 2000, None)
        print("приоритет модели: nice -10 через rtkit", flush=True)
    except Exception as error:  # noqa: BLE001
        print(f"приоритет модели не поднят: {error}", flush=True)


def main():
    raise_priority()
    write_atomic("focus", "0")
    write_atomic("layout", LAYOUT_IDS[0])
    device = find_device(time.time() + 30)
    if device is None:
        sys.exit("виртуальная клавиатура демона не появилась")
    device.grab()
    batches = open(os.path.join(CTL, "batches.jsonl"), "a", encoding="utf-8")
    events_log = open(os.path.join(CTL, "unexpected.log"), "a", encoding="utf-8")
    # Каждое нажатие на выходе демона: по нему восстанавливается, что именно
    # он сделал с экраном — счёт стирания, переключения, разделители.
    trace = open(os.path.join(CTL, "trace.log"), "a", encoding="utf-8", buffering=1 << 16)
    text = []
    caret = 0  # позиция курсора в text: стрелки демона тоже моделируются
    # Номера слов пачки, которые демон стирал: первое стирание серии стоит
    # сразу за словом (разделитель он держит), так что слово под курсором —
    # то, что он заменяет. Отличает «демон испортил верное слово» от «слово
    # было набрано не в той раскладке и осталось как есть».
    touched = set()
    pressed = set()  # клавиши, которые модель считает нажатыми
    erasing = False
    acked = None
    layout = 0
    write_atomic("focus", "1")
    write_atomic("ready", "1")
    # Ядро держит для читателя всего 64 события. Отдельный поток только
    # выгребает их в очередь — без файлов и без xkb: под нагрузкой на диск
    # одна запись файла стояла 300 мс, и ядро выбрасывало нажатия. Демон читает
    # свою клавиатуру так же.
    inbox = queue.Queue()

    def reader():
        try:
            for event in device.read_loop():
                inbox.put(event)
        except OSError:
            pass
        inbox.put(None)

    threading.Thread(target=reader, daemon=True).start()
    last_check = 0.0
    last_event = 0.0
    while True:
        now = time.time()
        if now - last_check > 0.03:
            last_check = now
            if not alive(DAEMON_PID):
                write_atomic("focus", "0")
            request = read("flush-req")
            # Сдавать пачку только в тишине: демон мог ещё не доработать
            # очередь, и тогда слово разрезалось бы между пачками.
            if request is not None and request != acked and now - last_event > 0.5:
                trace.write(f"flush\t{request}\n")
                trace.flush()
                batches.write(json.dumps({"batch": int(request), "text": "".join(text),
                                          "touched": sorted(touched)},
                                         ensure_ascii=False) + "\n")
                batches.flush()
                text.clear()
                caret = 0
                touched.clear()
                acked = request
                write_atomic("flush-ack", request)
            if read("quit") is not None:
                return
        try:
            first = inbox.get(timeout=0.03)
        except queue.Empty:
            continue
        if first is None:
            write_atomic("focus", "0")
            return
        last_event = time.time()
        events = [first]
        while True:
            try:
                more = inbox.get_nowait()
            except queue.Empty:
                break
            if more is None:
                break
            events.append(more)
        for event in events:
            if event.type == ecodes.EV_SYN and event.code == ecodes.SYN_DROPPED:
                # Ядро выбросило события: читатель (здесь — мы, в жизни —
                # libinput композитора) не успел за пачкой.
                trace.write(f"{event.sec * 1000 + event.usec // 1000}\tDROPPED\n")
                events_log.write(f"{time.time():.3f}\tSYN_DROPPED\n")
                events_log.flush()
                # libinput после выброса сверяет нажатые клавиши с устройством
                # (EVIOCGKEY); делаем то же, иначе потерянное отпускание Shift
                # оставляет его нажатым навсегда и весь текст идёт капсом.
                try:
                    down = set(device.active_keys())
                except OSError:
                    down = set()
                for code in pressed - down:
                    xkb.xkb_state_update_key(state, code + 8, XKB_KEY_UP)
                for code in down - pressed:
                    xkb.xkb_state_update_key(state, code + 8, XKB_KEY_DOWN)
                pressed = down
                continue
            if event.type != ecodes.EV_KEY:
                continue
            trace.write(f"{event.sec * 1000 + event.usec // 1000}\t{event.code}\t{event.value}\t{layout}\n")
            # The typist reads this to tell a daemon still replaying the previous
            # word from a quiet screen before it trusts the layout file.
            write_atomic("last-event", str(event.sec * 1000 + event.usec // 1000))
            keycode = event.code + 8
            if event.value in (1, 2):
                sym = xkb.xkb_state_key_get_one_sym(state, keycode)
                char = xkb.xkb_state_key_get_utf32(state, keycode)
                if sym == XK_BACKSPACE and event.value == 1 and not erasing:
                    touched.add(len("".join(text[:caret]).split()) - 1)
                erasing = sym == XK_BACKSPACE
                if sym == XK_BACKSPACE:
                    if caret > 0:
                        del text[caret - 1]
                        caret -= 1
                elif sym == XK_LEFT:
                    caret = max(0, caret - 1)
                elif sym == XK_RIGHT:
                    caret = min(len(text), caret + 1)
                elif char >= 0x20 and char != 0x7F:
                    text.insert(caret, chr(char))
                    caret += 1
                elif char != 0 and event.value == 1:
                    events_log.write(f"{time.time():.3f}\tkey {event.code} → {char:#x}\n")
                    events_log.flush()
            if event.value in (0, 1):
                (pressed.add if event.value == 1 else pressed.discard)(event.code)
                xkb.xkb_state_update_key(state, keycode,
                                         XKB_KEY_DOWN if event.value == 1 else XKB_KEY_UP)
                now_layout = xkb.xkb_state_serialize_layout(state, XKB_STATE_LAYOUT_EFFECTIVE)
                if now_layout != layout:
                    layout = now_layout
                    write_atomic("layout", LAYOUT_IDS[layout % len(LAYOUT_IDS)])


if __name__ == "__main__":
    try:
        main()
    finally:
        write_atomic("focus", "0")
