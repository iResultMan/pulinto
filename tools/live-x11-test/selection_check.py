#!/usr/bin/env python3
"""Живые сценарии выделения и смены окна — на ОТДЕЛЬНОМ X-сервере (Xvfb).

    selection_check.py <pulintod> <рабочий каталог>

Что поднимается: Xvfb :99 с раскладками us,ru и xfwm4; два GTK-окна A и B с
текстовыми полями; демон в режиме захвата на своей шине (dbus-run-session,
XDG_CONFIG_HOME и XDG_DATA_HOME в рабочем каталоге) с DISPLAY=:99 и
XDG_SESSION_TYPE=x11 — то есть по X11-пути: фокус по `_NET_ACTIVE_WINDOW`,
щелчки по XI2, PRIMARY по XFIXES, замена через XTest.

Ничего не должно попасть в окна живой сессии:
  * демон захватывает только синтетическую клавиатуру «PuLinTo Test Typist»
    (PULINTO_GRAB_ONLY); она создаётся с vendor id Apple, чтобы одиночный Alt
    был горячей клавишей;
  * выход демона — «PuLinTo Virtual Keyboard» — забирается в эксклюзивный
    захват до первой клавиши и ретранслируется в Xvfb через XTest
    (keycode evdev + 8); если захват не удался, клавиши не нажимаются;
  * замены демон печатает XTest-ом сам — в Xvfb, потому что DISPLAY=:99;
  * щелчки мыши делаются XTest-ом в Xvfb, настоящий указатель не двигается.

Сценарии (ожидание — в конце каждого):
  1. выделил в A, Alt+Tab в B, тап Alt — ничего не напечатано, в логе отказ;
  2. выделил в A, окно B активировано без клавиш, тап Alt — ничего, в логе
     «сменилось окно»;
  3. выделил в A, щёлкнул в B, тап Alt — ничего, в логе «щелчок мыши»;
  4. выделил в A, подождал 20 с, тап Alt — выделение переведено с первого тапа;
  5. выделил с клавиатуры (End, Shift+Left×3), тап Alt — переведено;
  6. выделил, Shift+Pause (аккорд действия selection) — переведено: клавиша,
     замыкающая аккорд, не должна гасить выделение;
  7. выделил, голый Pause на клавиатуре без Apple vendor id (аккорд действия
     word в наборе PC) — переведено.
"""
import os
import signal
import subprocess
import sys
import threading
import time

DAEMON, WORK = sys.argv[1], sys.argv[2]


def free_display():
    for number in range(90, 200):
        if not os.path.exists(f"/tmp/.X11-unix/X{number}") and not os.path.exists(f"/tmp/.X{number}-lock"):
            return f":{number}"
    raise SystemExit("нет свободного номера дисплея")


DISP = os.environ.get("PULINTO_TEST_DISPLAY") or free_display()
NAME = "PuLinTo Test Typist"
OUTPUT = "PuLinTo Virtual Keyboard"
APPLE = 0x05AC
LOG = f"{WORK}/daemon.log"
WAIT_S = 20

os.makedirs(f"{WORK}/config/pulinto", exist_ok=True)
os.makedirs(f"{WORK}/data", exist_ok=True)
open(f"{WORK}/config/pulinto/config.conf", "w").write(
    "mode = grab\nauto_enabled = true\nautostart = false\n"
)

ENV = {k: v for k, v in os.environ.items() if k not in ("WAYLAND_DISPLAY",)}
ENV.update(
    DISPLAY=DISP,
    XDG_SESSION_TYPE="x11",
    XDG_CONFIG_HOME=f"{WORK}/config",
    XDG_DATA_HOME=f"{WORK}/data",
    PULINTO_GRAB_ONLY=NAME,
    GDK_BACKEND="x11",
)

if "--inner" not in sys.argv:
    # Снаружи: X-сервер, затем всё остальное на своей шине D-Bus.
    xvfb = subprocess.Popen(
        ["Xvfb", DISP, "-screen", "0", "1280x800x24", "-nolisten", "tcp"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    for _ in range(100):
        if os.path.exists(f"/tmp/.X11-unix/X{DISP[1:]}"):
            break
        time.sleep(0.05)
    else:
        xvfb.kill()
        raise SystemExit("Xvfb не поднялся")
    time.sleep(0.3)
    try:
        subprocess.run(
            ["setxkbmap", "-display", DISP, "-layout", "us,ru", "-option", "grp:alt_shift_toggle"],
            check=True,
        )
        inner = ENV | {"PULINTO_TEST_DISPLAY": DISP}
        status = subprocess.run(
            ["dbus-run-session", "--", sys.executable, "-I", __file__, DAEMON, WORK, "--inner"],
            env=inner,
        ).returncode
    finally:
        xvfb.terminate()
        xvfb.wait()
    raise SystemExit(status)

# ── внутри: шина своя, DISPLAY=:99 ──────────────────────────────────────────
import evdev  # noqa: E402
from evdev import UInput, ecodes as e  # noqa: E402
from Xlib import X, display as xdisplay  # noqa: E402
from Xlib.ext import xtest  # noqa: E402
import gi  # noqa: E402

gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

GLib.set_prgname("pulinto-livetest")

failures = []
checks = 0


def check(name, ok, detail=""):
    global checks
    checks += 1
    print(("  ok   " if ok else "  FAIL ") + name + ("" if ok else f"  — {detail}"), flush=True)
    if not ok:
        failures.append(name)


def device(name):
    for path in evdev.list_devices():
        try:
            dev = evdev.InputDevice(path)
        except OSError:
            continue
        if dev.name == name:
            return dev
        dev.close()
    return None


# Окна -----------------------------------------------------------------------
class Field:
    def __init__(self, title, text):
        self.window = Gtk.Window(title=title)
        self.window.set_default_size(500, 300)
        self.view = Gtk.TextView()
        self.buffer = self.view.get_buffer()
        self.buffer.set_text(text)
        self.window.add(self.view)
        self.window.show_all()

    def xid(self):
        return self.window.get_window().get_xid()

    def text(self):
        return self.buffer.get_text(*self.buffer.get_bounds(), True)

    def select_last_word(self):
        text = self.text()
        start = self.buffer.get_iter_at_offset(len(text) - len(text.split()[-1]))
        self.buffer.select_range(start, self.buffer.get_end_iter())

    def reset(self, text):
        self.buffer.set_text(text)
        self.buffer.place_cursor(self.buffer.get_end_iter())


def on_gtk(fn, *args):
    """Выполнить в главном потоке GTK и дождаться результата."""
    done = threading.Event()
    box = {}

    def run():
        box["r"] = fn(*args)
        done.set()
        return False

    GLib.idle_add(run)
    done.wait(5)
    return box.get("r")


# Клавиатура машинистки и ретранслятор выхода демона --------------------------
def tap(ui, code, hold=0.05):
    ui.write(e.EV_KEY, code, 1); ui.syn(); time.sleep(hold)
    ui.write(e.EV_KEY, code, 0); ui.syn(); time.sleep(0.05)


def relay(out, stop):
    """Всё, что демон пересылает на свою клавиатуру, уходит в Xvfb."""
    d = xdisplay.Display(DISP)
    out.grab()
    while not stop.is_set():
        try:
            for ev in out.read():
                if ev.type == e.EV_KEY and ev.value in (0, 1):
                    xtest.fake_input(d, X.KeyPress if ev.value else X.KeyRelease, ev.code + 8)
                    d.sync()
        except BlockingIOError:
            time.sleep(0.005)


def active_window(d):
    atom = d.intern_atom("_NET_ACTIVE_WINDOW")
    prop = d.screen().root.get_full_property(atom, X.AnyPropertyType)
    return prop.value[0] if prop else None


def click_into(d, window):
    geom = window.get_geometry()
    pos = d.screen().root.translate_coords(window, 0, 0)  # окно → корень
    x = pos.x + geom.width // 2
    y = pos.y + geom.height // 2
    xtest.fake_input(d, X.MotionNotify, x=x, y=y)
    d.sync(); time.sleep(0.05)
    xtest.fake_input(d, X.ButtonPress, 1); d.sync(); time.sleep(0.05)
    xtest.fake_input(d, X.ButtonRelease, 1); d.sync()


def log_since(mark):
    # Отметка — размер файла в байтах, кириллица в UTF-8 длиннее символов.
    return open(LOG, "rb").read()[mark:].decode("utf-8", "replace")


def scenarios(a, b, ui, pc_ui, d):
    def activate(field):
        on_gtk(field.window.present)
        time.sleep(0.6)

    def fresh():
        """A в фокусе с текстом, и щелчок в A: как человек перед новым
        выделением. Без щелчка демон по праву продолжил бы цикл снимка
        прошлого выделения, не читая PRIMARY."""
        on_gtk(a.reset, "привет мир")
        on_gtk(b.reset, "")
        activate(a)
        click_into(d, d.create_resource_object("window", on_gtk(a.xid)))
        time.sleep(0.5)
        return os.path.getsize(LOG)

    def alt():
        tap(ui, e.KEY_LEFTALT)
        time.sleep(1.5)

    # 1. Alt+Tab
    mark = fresh()
    on_gtk(a.select_last_word); time.sleep(0.5)
    ui.write(e.EV_KEY, e.KEY_LEFTALT, 1); ui.syn(); time.sleep(0.08)
    tap(ui, e.KEY_TAB); time.sleep(0.08)
    ui.write(e.EV_KEY, e.KEY_LEFTALT, 0); ui.syn(); time.sleep(0.8)
    switched = active_window(d) == on_gtk(b.xid)
    if not switched:
        activate(b)
    alt()
    tail = log_since(mark)
    check("1 Alt+Tab: ничего не напечатано",
          on_gtk(a.text) == "привет мир" and on_gtk(b.text) == "",
          f"A={on_gtk(a.text)!r} B={on_gtk(b.text)!r}")
    check("1 Alt+Tab: в логе отказ по вводу или окну",
          "Selection is stale" in tail, tail[-300:])
    print(f"     (Alt+Tab переключил окно сам: {switched})")

    # 2. окно сменилось без клавиш
    mark = fresh()
    on_gtk(a.select_last_word); time.sleep(0.5)
    activate(b)
    alt()
    tail = log_since(mark)
    check("2 смена окна: ничего не напечатано",
          on_gtk(a.text) == "привет мир" and on_gtk(b.text) == "",
          f"A={on_gtk(a.text)!r} B={on_gtk(b.text)!r}")
    check("2 смена окна: в логе «сменилось окно»", "сменилось окно" in tail, tail[-300:])

    # 3. щелчок в B
    mark = fresh()
    on_gtk(a.select_last_word); time.sleep(0.5)
    click_into(d, d.create_resource_object("window", on_gtk(b.xid)))
    time.sleep(0.6)
    alt()
    tail = log_since(mark)
    check("3 щелчок: ничего не напечатано",
          on_gtk(a.text) == "привет мир" and on_gtk(b.text) == "",
          f"A={on_gtk(a.text)!r} B={on_gtk(b.text)!r}")
    check("3 щелчок: в логе «щелчок мыши»", "щелчок мыши" in tail, tail[-300:])

    # 4. подождать
    mark = fresh()
    on_gtk(a.select_last_word)
    time.sleep(WAIT_S)
    alt()
    tail = log_since(mark)
    check(f"4 через {WAIT_S} с: переведено с первого тапа",
          on_gtk(a.text) == "привет vbh", f"A={on_gtk(a.text)!r}")
    check("4 в логе «Selection conversion»", "Selection conversion" in tail, tail[-300:])

    # 5. выделение с клавиатуры
    mark = fresh()
    tap(ui, e.KEY_END); time.sleep(0.2)
    ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 1); ui.syn(); time.sleep(0.05)
    for _ in range(3):
        tap(ui, e.KEY_LEFT)
    ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 0); ui.syn(); time.sleep(0.5)
    alt()
    tail = log_since(mark)
    check("5 Shift+стрелки: переведено",
          on_gtk(a.text) == "привет vbh", f"A={on_gtk(a.text)!r}")
    check("5 в логе «Selection conversion»", "Selection conversion" in tail, tail[-300:])

    # 6. Shift+Pause — аккорд, а не печать
    mark = fresh()
    on_gtk(a.select_last_word); time.sleep(0.5)
    ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 1); ui.syn(); time.sleep(0.05)
    tap(ui, e.KEY_PAUSE)
    ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 0); ui.syn(); time.sleep(1.5)
    tail = log_since(mark)
    check("6 Shift+Pause: переведено", on_gtk(a.text) == "привет vbh", f"A={on_gtk(a.text)!r}")
    check("6 в логе «Selection conversion»", "Selection conversion" in tail, tail[-300:])

    # 7. голый Pause на PC-клавиатуре (набор PC: Pause = word)
    mark = fresh()
    on_gtk(a.select_last_word); time.sleep(0.5)
    tap(pc_ui, e.KEY_PAUSE); time.sleep(1.5)
    tail = log_since(mark)
    check("7 Pause (PC): переведено", on_gtk(a.text) == "привет vbh", f"A={on_gtk(a.text)!r}")
    check("7 в логе «Selection conversion»", "Selection conversion" in tail, tail[-300:])


def main():
    wm = subprocess.Popen(["xfwm4", "--replace"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.0)
    a = Field("A", "привет мир")
    b = Field("B", "")
    b.window.move(600, 50)
    a.window.move(50, 50)

    ui = UInput({e.EV_KEY: list(range(1, 249))}, name=NAME, vendor=APPLE, product=0x0250, version=1)
    # Вторая клавиатура, не Apple: у неё набор PC, где Pause — это word.
    pc_ui = UInput({e.EV_KEY: list(range(1, 249))}, name=NAME, vendor=0x046D, product=0xC31C, version=1)
    time.sleep(0.5)
    log = open(LOG, "w")
    daemon = subprocess.Popen([DAEMON], env=ENV, stdout=log, stderr=subprocess.STDOUT)

    out = None
    for _ in range(100):
        out = device(OUTPUT)
        if out is not None:
            break
        time.sleep(0.05)
    stop = threading.Event()
    status = 2
    try:
        if out is None:
            raise SystemExit("выход демона не появился")
        try:
            out.grab()
            out.ungrab()
        except OSError as error:
            raise SystemExit(f"выход демона не захвачен, клавиши не нажимаю: {error}")
        threading.Thread(target=relay, args=(out, stop), daemon=True).start()
        for _ in range(100):
            if open(LOG, encoding="utf-8").read().count(f"Grab: {NAME}") >= 2:
                break
            time.sleep(0.1)
        else:
            raise SystemExit("демон не захватил обе клавиатуры машинистки")
        time.sleep(0.5)
        d = xdisplay.Display(DISP)

        def run():
            nonlocal status
            try:
                scenarios(a, b, ui, pc_ui, d)
                status = 1 if failures else 0
            except Exception as error:  # noqa: BLE001
                print(f"сценарии оборваны: {error!r}", flush=True)
            finally:
                GLib.idle_add(Gtk.main_quit)

        threading.Thread(target=run, daemon=True).start()
        Gtk.main()
    finally:
        stop.set()
        daemon.send_signal(signal.SIGTERM)
        try:
            daemon.wait(3)
        except subprocess.TimeoutExpired:
            daemon.kill()
        ui.close()
        pc_ui.close()
        wm.terminate()
        log.close()
    print(f"проверок: {checks}, провалов: {len(failures)}" + (": " + ", ".join(failures) if failures else ""))
    return status


if __name__ == "__main__":
    raise SystemExit(main())
