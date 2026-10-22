#!/usr/bin/env python3
"""Хаос-тест залипания клавиш на СИНТЕТИЧЕСКОЙ клавиатуре «PuLinTo Test Typist».

Демон поднимается на своей шине и захватывает только её (PULINTO_GRAB_ONLY).
Каждый цикл — один случайный сценарий; после него в виртуальной клавиатуре
демона не должно остаться нажатых клавиш, а захват при удержании не берётся.

    stuck_keys.py <pulintod> <рабочий каталог> [циклов] [seed]

Настоящие клавиатуры не читаются и не захватываются. Перед запуском остановить
дежурный демон (замок на физическую клавиатуру) — `run-chaos.sh` делает это сам.

Ничего не должно попасть в окна живой сессии. Синтетическая клавиатура — для
композитора обычное устройство, и всё, что на ней нажато до захвата демоном,
печаталось бы в окне в фокусе; выход демона тоже идёт в сессию. Поэтому:
  * выход демона («PuLinTo Virtual Keyboard») забирается в эксклюзивный захват
    до первой клавиши, а если захват не удался, клавиши не нажимаются;
  * клавиши сценариев — F13…F24: они ничего не печатают и не двигают яркость
    (раньше тут были буквы и KEY_BRIGHTNESSDOWN, и текст уходил в чужие окна).
Аварийное сочетание Backspace+Esc+Enter нажимается только при захваченном выходе.
"""
import os, random, signal, subprocess, sys, time
import evdev
from evdev import UInput, ecodes as e

DAEMON, WORK = sys.argv[1], sys.argv[2]
CYCLES = int(sys.argv[3]) if len(sys.argv) > 3 else 40
rng = random.Random(int(sys.argv[4]) if len(sys.argv) > 4 else 1)
# Клавиши, которые не печатают и ничего не делают в GNOME: нажатые до захвата
# демоном, они попадают в сессию напрямую.
HARMLESS = [e.KEY_F13, e.KEY_F14, e.KEY_F15, e.KEY_F16, e.KEY_F17, e.KEY_F18,
            e.KEY_F19, e.KEY_F20, e.KEY_F21, e.KEY_F22, e.KEY_F23, e.KEY_F24]
os.makedirs(f"{WORK}/config/pulinto", exist_ok=True)
os.makedirs(f"{WORK}/data", exist_ok=True)
open(f"{WORK}/config/pulinto/config.conf", "w").write("mode = grab\nauto_enabled = true\nautostart = false\n")
ENV = dict(os.environ, XDG_CONFIG_HOME=f"{WORK}/config", XDG_DATA_HOME=f"{WORK}/data",
           PULINTO_GRAB_ONLY="PuLinTo Test Typist")

def device(name):
    for p in evdev.list_devices():
        try:
            d = evdev.InputDevice(p)
        except OSError:
            continue
        if d.name == name:
            return d
    return None

def virtual_held():
    d = device("PuLinTo Virtual Keyboard")
    return None if d is None else sorted(d.active_keys())

def start_daemon(log):
    wrapper = subprocess.Popen(["dbus-run-session", "--", "bash", "-c", f"exec {DAEMON}"],
                               env=ENV, stdout=log, stderr=subprocess.STDOUT)
    for _ in range(100):
        out = subprocess.run(["pgrep", "-P", str(wrapper.pid), "-x", "pulintod"], capture_output=True, text=True).stdout.split()
        if out:
            return wrapper, int(out[0])
        time.sleep(0.05)
    raise SystemExit("демон не поднялся")

def stop_daemon(wrapper, pid):
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError:
        pass
    wrapper.terminate(); wrapper.wait()

def capture_output():
    """Выход демона — в эксклюзивный захват, до первой клавиши. None — не вышло."""
    for _ in range(100):
        d = device("PuLinTo Virtual Keyboard")
        if d is not None:
            try:
                d.grab()
                return d
            except OSError:
                d.close()
        time.sleep(0.05)
    return None

def grabbed(logpath):
    return "Grab: PuLinTo Test Typist" in open(logpath, encoding="utf-8").read()

def press(ui, keys):
    for k in keys:
        ui.write(e.EV_KEY, k, 1); ui.syn(); time.sleep(0.01)

def release(ui, keys):
    for k in keys:
        ui.write(e.EV_KEY, k, 0); ui.syn(); time.sleep(0.01)

def wait_until(cond, seconds):
    t0 = time.time()
    while time.time() - t0 < seconds:
        if cond():
            return True
        time.sleep(0.05)
    return cond()

def scenario_hold_then_start(log, logpath):
    """Клавиша удерживается при старте демона: захвата нет до отпускания."""
    ui = UInput({e.EV_KEY: list(range(1, 249))}, name="PuLinTo Test Typist"); time.sleep(0.4)
    keys = rng.sample(HARMLESS, rng.randint(1, 3)); press(ui, keys)
    wrapper, pid = start_daemon(log)
    out = capture_output()
    time.sleep(1.2)
    ok = out is not None and not grabbed(logpath)
    release(ui, keys)
    ok = ok and wait_until(lambda: grabbed(logpath), 6)
    ok = ok and virtual_held() == []
    stop_daemon(wrapper, pid); ui.close()
    out and out.close()
    return ok, "захват отложен до отпускания"

def scenario_panic(log, logpath):
    """Аварийное сочетание при зажатых клавишах: ничего не остаётся нажатым."""
    ui = UInput({e.EV_KEY: list(range(1, 249))}, name="PuLinTo Test Typist"); time.sleep(0.4)
    wrapper, pid = start_daemon(log)
    out = capture_output()
    ok = out is not None and wait_until(lambda: grabbed(logpath), 6)
    if not ok:  # без захваченного выхода Enter и Esc ушли бы в живое окно
        stop_daemon(wrapper, pid); ui.close()
        out and out.close()
        return False, "выход демона не захвачен, клавиши не нажимались"
    keys = rng.sample(HARMLESS, rng.randint(1, 3)); press(ui, keys)
    press(ui, [e.KEY_BACKSPACE, e.KEY_ESC, e.KEY_ENTER])
    ok = wait_until(lambda: virtual_held() == [], 3)
    release(ui, keys + [e.KEY_BACKSPACE, e.KEY_ESC, e.KEY_ENTER])
    stop_daemon(wrapper, pid); ui.close(); out.close()
    return ok, "после аварийного снятия"

def scenario_unplug(log, logpath):
    """Клавиатура исчезает при зажатых клавишах (Bluetooth уснул)."""
    ui = UInput({e.EV_KEY: list(range(1, 249))}, name="PuLinTo Test Typist"); time.sleep(0.4)
    wrapper, pid = start_daemon(log)
    out = capture_output()
    ok = out is not None and wait_until(lambda: grabbed(logpath), 6)
    if not ok:
        stop_daemon(wrapper, pid); ui.close()
        out and out.close()
        return False, "выход демона не захвачен, клавиши не нажимались"
    keys = rng.sample(HARMLESS, rng.randint(1, 3)); press(ui, keys)
    ui.close()
    ok = wait_until(lambda: virtual_held() == [], 3)
    stop_daemon(wrapper, pid); out.close()
    return ok, "после отключения устройства"

def scenario_kill(log, logpath):
    """Демон убит при зажатых клавишах: виртуальная клавиатура исчезает целиком."""
    ui = UInput({e.EV_KEY: list(range(1, 249))}, name="PuLinTo Test Typist"); time.sleep(0.4)
    wrapper, pid = start_daemon(log)
    out = capture_output()
    ok = out is not None and wait_until(lambda: grabbed(logpath), 6)
    if not ok:
        stop_daemon(wrapper, pid); ui.close()
        out and out.close()
        return False, "выход демона не захвачен, клавиши не нажимались"
    keys = rng.sample(HARMLESS, rng.randint(1, 3)); press(ui, keys)
    stop_daemon(wrapper, pid)
    out.close()
    ok = wait_until(lambda: device("PuLinTo Virtual Keyboard") is None, 3)
    release(ui, keys); ui.close()
    return ok, "после убийства демона"

SCENARIOS = [scenario_hold_then_start, scenario_panic, scenario_unplug, scenario_kill]
results = {s.__name__: [0, 0] for s in SCENARIOS}
failures = []
for n in range(CYCLES):
    scenario = rng.choice(SCENARIOS)
    logpath = f"{WORK}/daemon-{n}.log"
    with open(logpath, "w") as log:
        try:
            ok, what = scenario(log, logpath)
        except Exception as error:  # noqa: BLE001
            ok, what = False, f"исключение {error}"
    results[scenario.__name__][0 if ok else 1] += 1
    if not ok:
        failures.append((n, scenario.__name__, what))
    else:
        os.remove(logpath)
print("сценарий                  ок  сбоев")
for name, (good, bad) in results.items():
    print(f"{name:26}{good:4}{bad:7}")
for n, name, what in failures:
    print(f"СБОЙ цикл {n}: {name} — {what} (журнал {WORK}/daemon-{n}.log)")
sys.exit(1 if failures else 0)
