#!/usr/bin/env python3
"""Горячие клавиши на СИНТЕТИЧЕСКИХ клавиатурах «PuLinTo Test Typist».

Две виртуальные клавиатуры: одна с vendor id Apple (0x05ac), другая без него.
Демон поднимается на своей шине и захватывает только их (PULINTO_GRAB_ONLY);
его выход — «PuLinTo Virtual Keyboard» — забирается в эксклюзивный захват, до
настоящего композитора не доходит ни одно нажатие. Дисплей демону не виден
(DISPLAY не задан): настоящий экран не читается.

Проверяется: каждое действие в каждом наборе срабатывает своим сочетанием и не
срабатывает чужим; PC-набор без одиночного Alt пропускает Alt-тап приложению
нетронутым; два набора живут одновременно; правка через D-Bus действует сразу и
переживает перезапуск; запись сочетания, Esc, время, запрет; после всего в
виртуальной клавиатуре ничего не остаётся нажатым.

    hotkeys_check.py <pulintod> <рабочий каталог>

Дежурный демон должен быть остановлен (замок на физическую клавиатуру) —
`run-hotkeys.sh` делает это сам.
"""
import ast
import json
import os
import re
import signal
import subprocess
import sys
import threading
import time

import evdev
from evdev import UInput, ecodes as e

DAEMON, WORK = sys.argv[1], sys.argv[2]
APPLE = 0x05AC
NAME = "PuLinTo Test Typist"
CONFIG = f"{WORK}/config/pulinto/config.conf"
LOG = f"{WORK}/daemon.log"
ADDR_FILE = f"{WORK}/bus-address"
os.makedirs(f"{WORK}/config/pulinto", exist_ok=True)
os.makedirs(f"{WORK}/data", exist_ok=True)

ENV = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
ENV.update(XDG_CONFIG_HOME=f"{WORK}/config", XDG_DATA_HOME=f"{WORK}/data",
           PULINTO_GRAB_ONLY=NAME, XDG_SESSION_TYPE="tty",
           PULINTO_LANG="ru")  # the checks below read the refusal texts in Russian

KEYS = list(range(1, 249))
K = e  # короткое имя для кодов

failures = []
checks = 0


def check(name, ok, detail=""):
    global checks
    checks += 1
    print(("  ok   " if ok else "  FAIL ") + name + ("" if ok else f"  — {detail}"), flush=True)
    if not ok:
        failures.append(name)


def key_name(code):
    name = e.KEY.get(code, code)
    return name[0] if isinstance(name, list) else name


def show(events):
    return " ".join(f"{'+' if v else '-'}{key_name(c).replace('KEY_', '')}" for c, v in events)


# ── демон и его выход ───────────────────────────────────────────────────────

class Session:
    """Один запуск демона на своей шине и захват его виртуальной клавиатуры."""

    def __init__(self, config_text):
        open(CONFIG, "w").write(config_text)
        open(LOG, "w").close()
        self.events = []
        self.lock = threading.Lock()
        self.wrapper = subprocess.Popen(
            ["dbus-run-session", "--", "bash", "-c",
             f"echo $DBUS_SESSION_BUS_ADDRESS > {ADDR_FILE}; exec {DAEMON}"],
            env=ENV, stdout=open(LOG, "a"), stderr=subprocess.STDOUT)
        self.pid = None
        for _ in range(200):
            out = subprocess.run(["pgrep", "-P", str(self.wrapper.pid), "-x", "pulintod"],
                                 capture_output=True, text=True).stdout.split()
            if out:
                self.pid = int(out[0])
                break
            time.sleep(0.05)
        if self.pid is None:
            raise SystemExit("демон не поднялся")
        self.vk = None
        for _ in range(200):
            self.vk = find_device("PuLinTo Virtual Keyboard")
            if self.vk:
                break
            time.sleep(0.05)
        if self.vk is None:
            raise SystemExit("нет виртуальной клавиатуры демона")
        self.vk.grab()
        threading.Thread(target=self._read, daemon=True).start()
        if not wait_until(lambda: self.log().count("Grab: " + NAME) >= 2, 8):
            raise SystemExit("демон не захватил обе синтетические клавиатуры:\n" + self.log())
        self.addr = open(ADDR_FILE).read().strip()
        time.sleep(0.3)

    def _read(self):
        try:
            for ev in self.vk.read_loop():
                if ev.type == e.EV_KEY and ev.value in (0, 1):
                    with self.lock:
                        self.events.append((ev.code, ev.value))
        except OSError:
            pass

    def log(self):
        return open(LOG, encoding="utf-8").read()

    def mark(self):
        with self.lock:
            return len(self.events), len(self.log())

    def since(self, mark):
        """(что вышло из виртуальной клавиатуры, что сработало) с метки."""
        time.sleep(0.35)
        with self.lock:
            out = self.events[mark[0]:]
        fired = re.findall(r"Hotkey: (\w+) \(set (\w+), vendor (0x[0-9a-f]+)\)",
                           self.log()[mark[1]:])
        return out, fired

    def held(self):
        try:
            return sorted(self.vk.active_keys())
        except OSError:
            return []

    def dbus(self, method, *args, timeout=30):
        cmd = ["gdbus", "call", "--address", self.addr, "--dest", "ru.pulinto.Daemon1",
               "--object-path", "/ru/pulinto/Daemon1",
               "--method", f"ru.pulinto.Daemon1.{method}", *args]
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)

    def call(self, method, *args):
        result = self.dbus(method, *args)
        if result.returncode != 0:
            raise SystemExit(f"{method}: {result.stderr}")
        return json.loads(ast.literal_eval(result.stdout.strip())[0])

    def stop(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
        except OSError:
            pass
        self.wrapper.terminate()
        self.wrapper.wait()
        try:
            self.vk.ungrab()
        except OSError:
            pass
        time.sleep(0.3)


def find_device(name):
    for path in evdev.list_devices():
        try:
            device = evdev.InputDevice(path)
        except OSError:
            continue
        if device.name == name:
            return device
    return None


def wait_until(cond, seconds):
    t0 = time.time()
    while time.time() - t0 < seconds:
        if cond():
            return True
        time.sleep(0.05)
    return cond()


# ── нажатия ─────────────────────────────────────────────────────────────────

def down(ui, *keys):
    for key in keys:
        ui.write(e.EV_KEY, key, 1)
        ui.syn()
        time.sleep(0.02)


def up(ui, *keys):
    for key in keys:
        ui.write(e.EV_KEY, key, 0)
        ui.syn()
        time.sleep(0.02)


def hit(ui, *keys):
    """Нажать по порядку, отпустить в обратном."""
    down(ui, *keys)
    up(ui, *reversed(keys))


def clear_words(session, *uis):
    """Esc бросает набранное слово: иначе действие «слово» честно перекладывало бы
    остатки прошлой проверки (Ctrl+C кладёт «c» в слово — так было и раньше)."""
    for ui in uis:
        hit(ui, K.KEY_ESC)
    time.sleep(0.3)


# ── проверки ────────────────────────────────────────────────────────────────

def expect(session, label, do, fired_ids, out=None, scheme=None):
    mark = session.mark()
    do()
    got_out, got_fired = session.since(mark)
    ids = [action for action, *_ in got_fired]
    ok = ids == fired_ids
    if scheme:
        ok = ok and all(s == scheme for _, s, _ in got_fired)
    check(f"{label}: срабатывает {fired_ids or 'ничего'}", ok, f"сработало {got_fired}")
    if out is not None:
        check(f"{label}: приложению ушло [{show(out)}]", got_out == out, f"ушло [{show(got_out)}]")
    return got_out


def defaults_for(session, scheme, ui):
    print(f"\n── набор {scheme}, значения по умолчанию")
    apple = scheme == "apple"
    L, R = ("L", "R")
    PAUSE, LS, LC, RC, LA, RA = K.KEY_PAUSE, K.KEY_LEFTSHIFT, K.KEY_LEFTCTRL, K.KEY_RIGHTCTRL, K.KEY_LEFTALT, K.KEY_RIGHTALT
    P, BS, A, TAB, C, O = K.KEY_P, K.KEY_BACKSLASH, K.KEY_A, K.KEY_TAB, K.KEY_C, K.KEY_O
    expect(session, "Pause", lambda: hit(ui, PAUSE), ["word"], [], scheme)
    expect(session, "Shift+Pause", lambda: hit(ui, LS, PAUSE), ["selection"], [(LS, 1), (LS, 0)], scheme)
    expect(session, "Ctrl+Alt+P", lambda: hit(ui, LC, LA, P), ["toggle_app"], [(LC, 1), (LA, 1), (LA, 0), (LC, 0)], scheme)
    expect(session, "Ctrl+Alt+\\", lambda: hit(ui, LC, LA, BS), ["report"], [(LC, 1), (LA, 1), (LA, 0), (LC, 0)], scheme)
    # Одиночный Alt: набор Apple забирает его, PC отдаёт приложению нетронутым.
    for alt, name in ((LA, "левый Alt"), (RA, "правый Alt")):
        if apple:
            expect(session, f"тап: {name}", lambda: hit(ui, alt), ["word"], [], scheme)
        else:
            expect(session, f"тап: {name}", lambda: hit(ui, alt), [], [(alt, 1), (alt, 0)])
    # Правый Ctrl: тап слова в PC, не вмешивается в Apple. Нажатие не придерживается.
    if apple:
        expect(session, "тап: правый Ctrl", lambda: hit(ui, RC), [], [(RC, 1), (RC, 0)])
    else:
        expect(session, "тап: правый Ctrl", lambda: hit(ui, RC), ["word"], [(RC, 1), (RC, 0)], scheme)
    expect(session, "тап: левый Ctrl", lambda: hit(ui, LC), [], [(LC, 1), (LC, 0)])
    expect(session, "тап: Shift", lambda: hit(ui, LS), [], [(LS, 1), (LS, 0)])
    # Чужое сочетание не срабатывает, а клавиши идут приложению по порядку.
    expect(session, "Alt+Tab", lambda: hit(ui, LA, TAB), [], [(LA, 1), (TAB, 1), (TAB, 0), (LA, 0)])
    expect(session, "Shift, затем Alt (смена раскладки)", lambda: hit(ui, LS, LA), [],
           [(LS, 1), (LA, 1), (LA, 0), (LS, 0)])
    expect(session, "Alt, затем Shift", lambda: hit(ui, LA, LS), [],
           [(LA, 1), (LS, 1), (LS, 0), (LA, 0)])
    expect(session, "Ctrl+Alt+O", lambda: hit(ui, LC, LA, O), [], [(LC, 1), (LA, 1), (O, 1), (O, 0), (LA, 0), (LC, 0)])
    expect(session, "Ctrl+C", lambda: hit(ui, LC, C), [], [(LC, 1), (C, 1), (C, 0), (LC, 0)])
    expect(session, "правый Ctrl+C (Ctrl не скрыт от мыши и клавиш)", lambda: hit(ui, RC, C), [],
           [(RC, 1), (C, 1), (C, 0), (RC, 0)])
    expect(session, "Shift+Pause с лишним Ctrl", lambda: hit(ui, LC, LS, PAUSE), [],
           [(LC, 1), (LS, 1), (PAUSE, 1), (PAUSE, 0), (LS, 0), (LC, 0)])
    expect(session, "буква", lambda: hit(ui, A), [], [(A, 1), (A, 0)])
    # Ctrl+буква — чужое сочетание: буква не попадает в слово, и хоткей не переложит лишнее.
    clear_words(session, ui)
    mark = session.mark()
    hit(ui, LC, C)
    hit(ui, K.KEY_LEFTMETA, K.KEY_L)
    hit(ui, PAUSE)
    out, fired = session.since(mark)
    check("Ctrl+C и Super+L не попадают в слово: Pause ничего не перекладывает",
          [a for a, *_ in fired] == ["word"] and K.KEY_BACKSPACE not in [c for c, _ in out], f"{fired} [{show(out)}]")
    check("ничего не осталось нажатым", session.held() == [], str(session.held()))


def two_keyboards(session, apple_ui, pc_ui):
    print("\n── два набора одновременно")
    LA, RC = K.KEY_LEFTALT, K.KEY_RIGHTCTRL
    clear_words(session, apple_ui, pc_ui)

    def alts():
        down(apple_ui, LA)
        down(pc_ui, LA)
        up(apple_ui, LA)
        up(pc_ui, LA)

    out = expect(session, "Alt на обеих клавиатурах", alts, ["word"], scheme="apple")
    check("Alt пришёл приложению ровно от PC-клавиатуры", out == [(LA, 1), (LA, 0)], show(out))

    def ctrls():
        down(apple_ui, RC)
        down(pc_ui, RC)
        up(apple_ui, RC)
        up(pc_ui, RC)

    expect(session, "правый Ctrl на обеих клавиатурах", ctrls, ["word"], scheme="pc")
    check("ничего не осталось нажатым", session.held() == [], str(session.held()))


def editing(session, apple_ui, pc_ui):
    print("\n── правка через D-Bus")
    LA, RC, PAUSE = K.KEY_LEFTALT, K.KEY_RIGHTCTRL, K.KEY_PAUSE
    clear_words(session, apple_ui, pc_ui)
    state = session.call("GetHotkeys")
    check("GetHotkeys: выбор «авто»", state["choice"] == "auto", state["choice"])
    check("GetHotkeys: слово в PC — Pause и правый Ctrl",
          [b["text"] for b in state["sets"]["pc"]["word"]["bindings"]] == ["pause", "tap:rctrl"])
    check("GetHotkeys: слово в Apple — Alt и Pause",
          [b["text"] for b in state["sets"]["apple"]["word"]["bindings"]] == ["tap:alt", "pause"])
    check("GetHotkeys: четыре действия", [a["id"] for a in state["actions"]] == ["word", "selection", "toggle_app", "report"])
    check("GetHotkeys: по умолчанию всё «по умолчанию»",
          all(s[a]["is_default"] for s in state["sets"].values() for a in s))
    check("GetHotkeys: клавиатуры по наборам",
          sorted(k["scheme"] for k in state["keyboards"] if k["name"] == NAME) == ["apple", "pc"],
          str(state["keyboards"]))

    result = session.call("SetHotkey", "pc", "word", "['tap:alt', 'pause']")
    check("SetHotkey: принят", result["ok"], str(result))
    check("SetHotkey: записан в config.conf", "hotkey.pc.word = tap:alt, pause" in open(CONFIG).read(),
          open(CONFIG).read())
    expect(session, "PC после правки: тап Alt", lambda: hit(pc_ui, LA), ["word"], [], "pc")
    expect(session, "PC после правки: правый Ctrl больше не слово", lambda: hit(pc_ui, RC), [], [(RC, 1), (RC, 0)])
    expect(session, "Apple не затронут: тап Alt", lambda: hit(apple_ui, LA), ["word"], [], "apple")
    expect(session, "ничего не мешает Alt+Tab на PC после правки", lambda: hit(pc_ui, LA, K.KEY_TAB), [],
           [(LA, 1), (K.KEY_TAB, 1), (K.KEY_TAB, 0), (LA, 0)])

    result = session.call("ResetHotkeys", "pc", "")
    check("ResetHotkeys: набор сброшен", result["ok"], str(result))
    check("ResetHotkeys: из config.conf ушли строки", "hotkey." not in open(CONFIG).read(), open(CONFIG).read())
    expect(session, "PC после сброса: Alt снова отдан", lambda: hit(pc_ui, LA), [], [(LA, 1), (LA, 0)])

    result = session.call("SetHotkey", "pc", "word", "[]")
    check("SetHotkey: действие можно выключить", result["ok"], str(result))
    expect(session, "выключенное слово: Pause уходит приложению", lambda: hit(pc_ui, PAUSE), [],
           [(PAUSE, 1), (PAUSE, 0)])
    session.call("ResetHotkeys", "pc", "word")

    print("   защиты")
    for action, bindings, fragment in (
        ("report", "['a']", "обычная клавиша ввода"),
        ("report", "['shift+a']", "обычная клавиша ввода"),
        ("report", "['space']", "обычная клавиша ввода"),
        ("report", "['ctrl+alt+p']", "Включить или выключить"),
        ("selection", "['pause']", "Сменить раскладку слова"),
        ("report", "['ctrl+alt+56']", "Модификатор"),
        ("report", "['ctrl+никогда']", "Не удалось разобрать"),
        ("report", "['ctrl+q', 'ctrl+q']", "уже есть"),
    ):
        result = session.call("SetHotkey", "pc", action, bindings)
        check(f"SetHotkey {action} {bindings}: отказ с причиной",
              (not result["ok"]) and fragment in result["error"], str(result))
    result = session.call("SetHotkey", "nobody", "report", "['ctrl+q']")
    check("SetHotkey: неизвестный набор", not result["ok"], str(result))
    check("отказы ничего не сохранили", "hotkey." not in open(CONFIG).read(), open(CONFIG).read())
    state = session.call("GetHotkeys")
    check("после отказов всё по умолчанию", all(s[a]["is_default"] for s in state["sets"].values() for a in s))

    print("   ручной выбор набора")
    result = session.call("SetHotkeyChoice", "pc")
    check("выбор «всегда PC»", result["ok"], str(result))
    expect(session, "Apple-клавиатура под PC-набором: Alt отдан", lambda: hit(apple_ui, LA), [], [(LA, 1), (LA, 0)])
    expect(session, "Apple-клавиатура под PC-набором: правый Ctrl — слово", lambda: hit(apple_ui, RC), ["word"],
           [(RC, 1), (RC, 0)], "pc")
    session.call("SetHotkeyChoice", "apple")
    expect(session, "PC-клавиатура под набором Apple: Alt — слово", lambda: hit(pc_ui, LA), ["word"], [], "apple")
    session.call("SetHotkeyChoice", "auto")
    check("выбор «авто» записан по умолчанию",
          not any(line.startswith("hotkey_set") for line in open(CONFIG).read().splitlines()))
    expect(session, "снова по устройству: PC-Alt отдан", lambda: hit(pc_ui, LA), [], [(LA, 1), (LA, 0)])
    check("ничего не осталось нажатым", session.held() == [], str(session.held()))


def record(session, scheme, action, do, wait=15):
    """RecordHotkey в фоне, `do()` — нажатия; возвращает (ответ, ушло приложению, сработало)."""
    mark = session.mark()
    proc = subprocess.Popen(
        ["gdbus", "call", "--address", session.addr, "--dest", "ru.pulinto.Daemon1",
         "--object-path", "/ru/pulinto/Daemon1", "--method", "ru.pulinto.Daemon1.RecordHotkey",
         scheme, action], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(0.6)
    do()
    try:
        out, err = proc.communicate(timeout=wait)
    except subprocess.TimeoutExpired:
        proc.kill()
        return {"status": "no-answer"}, [], []
    answer = json.loads(ast.literal_eval(out.strip())[0]) if out.strip() else {"status": "error", "error": err}
    got_out, fired = session.since(mark)
    return answer, got_out, fired


def recording(session, apple_ui, pc_ui):
    print("\n── запись сочетания")
    LS, LC, RA, F9, ESC, A, PAUSE, P = K.KEY_LEFTSHIFT, K.KEY_LEFTCTRL, K.KEY_RIGHTALT, K.KEY_F9, K.KEY_ESC, K.KEY_A, K.KEY_PAUSE, K.KEY_P
    before = open(CONFIG).read()

    answer, out, fired = record(session, "pc", "report", lambda: hit(pc_ui, LC, LS, F9))
    check("запись сочетания: Ctrl+Shift+F9", answer.get("status") == "recorded" and answer.get("binding") == "ctrl+shift+f9"
          and answer.get("label") == "Ctrl+Shift+F9", str(answer))
    check("запись: приложению не ушло ничего", out == [], show(out))
    check("запись: ничего не сработало", fired == [], str(fired))
    check("запись нигде не сохранена", open(CONFIG).read() == before)

    answer, out, fired = record(session, "pc", "report", lambda: hit(pc_ui, RA))
    check("запись одиночного модификатора: правый Alt", answer.get("status") == "recorded"
          and answer.get("binding") == "tap:ralt", str(answer))
    check("запись тапа: приложению не ушло ничего", out == [], show(out))

    answer, out, fired = record(session, "apple", "report", lambda: hit(apple_ui, K.KEY_LEFTMETA))
    check("запись тапа Super с предупреждением о GNOME (изолированный dconf — без него)",
          answer.get("status") == "recorded" and answer.get("binding") == "tap:lmeta", str(answer))

    answer, out, fired = record(session, "pc", "report", lambda: hit(pc_ui, ESC))
    check("Esc отменяет запись", answer.get("status") == "cancelled", str(answer))
    check("Esc: приложению не ушло ничего", out == [], show(out))
    mark = session.mark()
    hit(pc_ui, ESC)
    got, _ = session.since(mark)
    check("после записи Esc снова идёт приложению", got == [(ESC, 1), (ESC, 0)], show(got))

    answer, out, fired = record(session, "pc", "report", lambda: hit(pc_ui, A))
    check("голая буква отвергнута с причиной", answer.get("status") == "rejected"
          and "обычная клавиша ввода" in answer.get("error", ""), str(answer))
    check("отвергнутая буква не напечатана", out == [], show(out))
    mark = session.mark()
    hit(pc_ui, A)
    got, _ = session.since(mark)
    check("после записи буква снова печатается", got == [(A, 1), (A, 0)], show(got))

    answer, out, fired = record(session, "pc", "selection", lambda: hit(pc_ui, PAUSE))
    check("чужая привязка отвергнута с причиной", answer.get("status") == "rejected"
          and "Сменить раскладку слова" in answer.get("error", ""), str(answer))
    check("пока идёт запись, замена не срабатывает", fired == [] and out == [], f"{fired} {show(out)}")

    # Клавиша, нажатая до записи, отпускается приложению: оно видело её нажатие.
    down(pc_ui, LS)
    mark = session.mark()
    proc = subprocess.Popen(
        ["gdbus", "call", "--address", session.addr, "--dest", "ru.pulinto.Daemon1",
         "--object-path", "/ru/pulinto/Daemon1", "--method", "ru.pulinto.Daemon1.RecordHotkey", "pc", "report"],
        stdout=subprocess.PIPE, text=True)
    time.sleep(0.6)
    hit(pc_ui, F9)
    out_text = proc.communicate(timeout=15)[0]
    up(pc_ui, LS)
    answer = json.loads(ast.literal_eval(out_text.strip())[0])
    got, _ = session.since(mark)
    check("Shift, зажатый до записи, считается частью сочетания", answer.get("binding") == "shift+f9", str(answer))
    check("его отпускание дошло до приложения, видевшего нажатие", got == [(LS, 0)], show(got))
    check("ничего не осталось нажатым", session.held() == [], str(session.held()))

    # Нажатия, начатые во время записи, отпускаются после её конца и не выходят наружу.
    answer, out, fired = record(session, "pc", "report", lambda: (down(pc_ui, LC, P), time.sleep(0.3), up(pc_ui, P, LC)))
    check("отпускания клавиш записи не выходят наружу", answer.get("status") == "recorded" and out == [],
          f"{answer} {show(out)}")
    check("ничего не осталось нажатым", session.held() == [], str(session.held()))

    # Окно следит за записью и может её оборвать; демон отвечает на другие вызовы, пока она идёт.
    mark = session.mark()
    proc = subprocess.Popen(
        ["gdbus", "call", "--address", session.addr, "--dest", "ru.pulinto.Daemon1",
         "--object-path", "/ru/pulinto/Daemon1", "--method", "ru.pulinto.Daemon1.RecordHotkey", "pc", "report"],
        stdout=subprocess.PIPE, text=True)
    time.sleep(0.6)
    down(pc_ui, LC, K.KEY_LEFTALT)
    time.sleep(0.2)
    started = time.time()
    progress = ast.literal_eval(session.dbus("HotkeyRecordingProgress").stdout.strip())[0]
    check("пока идёт запись, окно видит зажатые модификаторы", progress == "Ctrl+Alt", repr(progress))
    state = session.call("GetHotkeys")
    check("пока идёт запись, демон отвечает на другие вызовы",
          state["choice"] == "auto" and time.time() - started < 3, f"{time.time() - started:.1f} с")
    session.dbus("CancelHotkeyRecording")
    answer = json.loads(ast.literal_eval(proc.communicate(timeout=15)[0].strip())[0])
    up(pc_ui, K.KEY_LEFTALT, LC)
    got, fired = session.since(mark)
    check("закрытие диалога обрывает запись сразу", answer.get("status") == "cancelled", str(answer))
    check("оборванная запись: приложению не ушло ничего, отпускания проглочены", got == [] and fired == [],
          f"{show(got)} {fired}")
    check("ничего не осталось нажатым", session.held() == [], str(session.held()))

    # Своё сочетание GNOME — предупреждение, не отказ.
    env = dict(ENV, DBUS_SESSION_BUS_ADDRESS=session.addr)
    path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/"
    schema = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:" + path
    for args in (["set", "org.gnome.settings-daemon.plugins.media-keys", "custom-keybindings", f"['{path}']"],
                 ["set", schema, "name", "Test terminal"],
                 ["set", schema, "binding", "<Control><Alt>t"],
                 ["set", schema, "command", "true"]):
        subprocess.run(["gsettings", *args], env=env, capture_output=True)
    answer, out, fired = record(session, "pc", "report", lambda: hit(pc_ui, LC, K.KEY_LEFTALT, K.KEY_T))
    check("сочетание GNOME: записано и с предупреждением",
          answer.get("status") == "recorded" and any("Test terminal" in w for w in answer.get("warnings", [])),
          str(answer))
    result = session.call("SetHotkey", "pc", "report", "['ctrl+alt+t']")
    check("SetHotkey: тоже предупреждает, но сохраняет",
          result["ok"] and any("Test terminal" in w for w in result["warnings"]), str(result))
    session.call("ResetHotkeys", "pc", "report")

    started = time.time()
    answer, out, fired = record(session, "pc", "report", lambda: None, wait=25)
    check("по таймауту запись отменяется (~10 с)", answer.get("status") == "timeout"
          and 9 <= time.time() - started <= 14, f"{answer} за {time.time() - started:.1f} с")
    mark = session.mark()
    hit(pc_ui, A)
    got, _ = session.since(mark)
    check("после таймаута буква печатается", got == [(A, 1), (A, 0)], show(got))


def legacy(apple_ui, pc_ui):
    print("\n══ config.conf прежнего формата")
    session = Session("mode = grab\nauto_enabled = true\nautostart = false\n"
                      "report_hotkey = ctrl+alt+r\ntoggle_hotkey = ctrl+shift+meta+t\n")
    try:
        LC, LA, LS, LM, R, T, BS = K.KEY_LEFTCTRL, K.KEY_LEFTALT, K.KEY_LEFTSHIFT, K.KEY_LEFTMETA, K.KEY_R, K.KEY_T, K.KEY_BACKSLASH
        for scheme, ui in (("apple", apple_ui), ("pc", pc_ui)):
            expect(session, f"{scheme}: старый report_hotkey ctrl+alt+r", lambda: hit(ui, LC, LA, R), ["report"], scheme=scheme)
            expect(session, f"{scheme}: старый toggle_hotkey", lambda: hit(ui, LC, LS, LM, T), ["toggle_app"], scheme=scheme)
            expect(session, f"{scheme}: прежний Ctrl+Alt+\\ больше не отчёт", lambda: hit(ui, LC, LA, BS), [])
            expect(session, f"{scheme}: Pause остаётся", lambda: hit(ui, K.KEY_PAUSE), ["word"], scheme=scheme)
        expect(session, "Apple: Alt остаётся", lambda: hit(apple_ui, LA), ["word"], [], "apple")
        expect(session, "PC: Alt отдан", lambda: hit(pc_ui, LA), [], [(LA, 1), (LA, 0)])
        check("ничего не осталось нажатым", session.held() == [], str(session.held()))
    finally:
        session.stop()


def main():
    apple_ui = UInput({e.EV_KEY: KEYS}, name=NAME, vendor=APPLE, product=0x0250, version=1)
    pc_ui = UInput({e.EV_KEY: KEYS}, name=NAME, vendor=0x046D, product=0xC31C, version=1)
    time.sleep(0.5)
    try:
        legacy(apple_ui, pc_ui)

        print("\n══ новый config.conf")
        session = Session("mode = grab\nauto_enabled = true\nautostart = false\n")
        try:
            defaults_for(session, "apple", apple_ui)
            defaults_for(session, "pc", pc_ui)
            two_keyboards(session, apple_ui, pc_ui)
            editing(session, apple_ui, pc_ui)
            recording(session, apple_ui, pc_ui)
            result = session.call("SetHotkey", "pc", "toggle_app", "['ctrl+alt+k']")
            check("перед перезапуском: правка принята", result["ok"], str(result))
        finally:
            session.stop()

        print("\n══ перезапуск: правка пережила его")
        session = Session(open(CONFIG).read())
        try:
            expect(session, "после перезапуска: новое сочетание", lambda: hit(pc_ui, K.KEY_LEFTCTRL, K.KEY_LEFTALT, K.KEY_K),
                   ["toggle_app"], scheme="pc")
            expect(session, "после перезапуска: прежнее сочетание больше не его", lambda: hit(pc_ui, K.KEY_LEFTCTRL, K.KEY_LEFTALT, K.KEY_P), [])
            expect(session, "после перезапуска: Apple прежний", lambda: hit(apple_ui, K.KEY_LEFTCTRL, K.KEY_LEFTALT, K.KEY_P),
                   ["toggle_app"], scheme="apple")
            check("ничего не осталось нажатым", session.held() == [], str(session.held()))
        finally:
            session.stop()
    finally:
        apple_ui.close()
        pc_ui.close()

    print(f"\nпроверок: {checks}, провалено: {len(failures)}")
    for name in failures:
        print("  ПРОВАЛ:", name)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
