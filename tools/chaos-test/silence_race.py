#!/usr/bin/env python3
"""Гонка «poll — EVIOCGKEY»: первая клавиша после паузы исчезала, потому что
читатель демона после 100 мс тишины спрашивал состояние клавиш через
захваченный дескриптор, а ядро при этом вычищает из его очереди непрочитанные
события клавиш (`__evdev_flush_queue`). Тест поднимает демон в песочнице с
трассой входа (PULINTO_TRACE_INPUT), нажимает на синтетической клавиатуре F13–F24
после паузы, перебирая задержки вокруг 100 мс, и сверяет отправленное с
прочитанным. Дежурный демон на время теста должен быть остановлен: он тоже
захватывает «PuLinTo Test Typist». Выход демона песочницы («PuLinTo Virtual
Keyboard») берётся в захват до первой клавиши, захват своей клавиатуры
подтверждается логом — без этого ни одной клавиши не нажимается.

    silence_race.py <pulintod> <рабочий каталог> [итераций=1500]
"""
import os
import random
import subprocess
import sys
import time

import threading

from evdev import InputDevice, UInput, ecodes as e, list_devices

DAEMON = os.path.abspath(sys.argv[1])
WORK = os.path.abspath(sys.argv[2])
ITERATIONS = int(sys.argv[3]) if len(sys.argv) > 3 else 1500
NAME = "PuLinTo Test Typist"
TRACE = f"{WORK}/daemon-in.log"
os.makedirs(f"{WORK}/config/pulinto", exist_ok=True)
os.makedirs(f"{WORK}/data", exist_ok=True)
open(f"{WORK}/config/pulinto/config.conf", "w").write("")
open(TRACE, "w").close()

env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
env.update(XDG_CONFIG_HOME=f"{WORK}/config", XDG_DATA_HOME=f"{WORK}/data",
           PULINTO_GRAB_ONLY=NAME, XDG_SESSION_TYPE="tty", PULINTO_TRACE_INPUT=TRACE)
# Клавиатура — до демона: он сканирует устройства на старте.
ui = UInput({e.EV_KEY: list(range(1, 249))}, name=NAME, vendor=0x046D, product=0xC31C, version=1)
time.sleep(0.5)
# Клавиатура — до демона (он сканирует устройства на старте), и только
# F13–F24: даже если что-то утечёт в живую сессию, это не буквы.
KEYS = list(range(e.KEY_F13, e.KEY_F24 + 1))
# В возможностях устройства — полный набор (демон признаёт клавиатурой только
# устройство с буквами и пробелом); нажимаются из него только F13–F24.
ui = UInput({e.EV_KEY: list(range(1, 249))}, name=NAME, vendor=0x046D, product=0xC31C, version=1)
time.sleep(0.5)
log = open(f"{WORK}/daemon.log", "w")
wrapper = subprocess.Popen(["dbus-run-session", "--", DAEMON], env=env, stdout=log, stderr=subprocess.STDOUT)


def stop_daemon():
    subprocess.run(["pkill", "-TERM", "-P", str(wrapper.pid), "-x", "pulintod"])
    wrapper.terminate()
    try:
        wrapper.wait(5)
    except subprocess.TimeoutExpired:
        wrapper.kill()


def wait_until(condition, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        if condition():
            return True
        time.sleep(0.05)
    return False


# 1. Выход демона — его виртуальная клавиатура — в захват ДО первой клавиши:
#    иначе всё, что печатает тест, демон пересылает в живую сессию.
vk = None
def find_vk():
    global vk
    for path in list_devices():
        dev = InputDevice(path)
        if dev.name == "PuLinTo Virtual Keyboard":
            vk = dev
            return True
        dev.close()
    return False
if not wait_until(find_vk, 10):
    stop_daemon()
    raise SystemExit("нет виртуальной клавиатуры демона — ни одной клавиши не нажато")
vk.grab()
threading.Thread(target=lambda: [None for _ in vk.read_loop()], daemon=True).start()

# 2. Захват нашей клавиатуры демоном подтверждён логом — только тогда печатаем.
if not wait_until(lambda: ("Grab: " + NAME) in open(f"{WORK}/daemon.log", encoding="utf-8").read(), 10):
    stop_daemon()
    raise SystemExit("демон не захватил тестовую клавиатуру — ни одной клавиши не нажато")


def tap(code):
    ui.write(e.EV_KEY, code, 1); ui.syn()
    time.sleep(0.006)
    ui.write(e.EV_KEY, code, 0); ui.syn()


time.sleep(0.5)  # читателю демона — время встать
for _ in range(5):
    tap(e.KEY_F24); time.sleep(0.05)
time.sleep(0.3)
if os.path.getsize(TRACE) == 0:
    stop_daemon()
    raise SystemExit("демон не читает клавиатуру: трасса пуста")

letters = KEYS[:8]  # F13–F20
sent = 0
rng = random.Random(7)
started = time.time()
for i in range(ITERATIONS):
    # Пауза вокруг таймаута poll (100 мс): перебор от 96 до 106 мс мелким шагом
    # плюс случайный сдвиг, чтобы попасть в окно между таймаутом и ioctl.
    delay = 0.096 + (i % 100) * 0.0001 + rng.random() * 0.0004
    time.sleep(delay)
    tap(letters[i % len(letters)])
    sent += 2
time.sleep(0.5)
ui.close()
try:
    vk.ungrab()
except OSError:
    pass
stop_daemon()

read = 0
by_key = {}
for line in open(TRACE):
    _, code, value = line.split("\t")
    code = int(code); value = int(value)
    if value in (0, 1):
        read += 1
        by_key[(code, value)] = by_key.get((code, value), 0) + 1
read -= 10  # пять прогревочных нажатий
elapsed = time.time() - started
print(f"итераций: {ITERATIONS}, отправлено событий: {sent}, прочитано демоном: {read}, "
      f"потеряно: {sent - read}, {elapsed:.0f} с")
lost_presses = sum(1 for (c, v), n in by_key.items() if v == 1 and n < by_key.get((c, 0), 0))
print("ПРОВАЛ" if sent != read else "ok", f"— нажатий меньше отпусканий у {lost_presses} клавиш")
sys.exit(0 if sent == read else 1)
