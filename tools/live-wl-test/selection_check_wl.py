#!/usr/bin/env python3
"""Живые сценарии выделения и смены окна — на НАСТОЯЩЕМ GNOME/Wayland.

    selection_check_wl.py            # сценарии, дежурный демон должен работать
    selection_check_wl.py --window B # вспомогательный процесс окна B (внутренний)

В отличие от `tools/live-x11-test/selection_check.py`, здесь нет своего
экрана: окна открываются в живой сессии, демон — дежурный (`pulintod` из
автозапуска, лог `~/.local/state/pulinto/pulintod.log`), фокус докладывает
расширение GNOME, PRIMARY идёт через Xwayland. Поэтому запускать только когда
за машиной никого нет: синтетическая клавиатура «PuLinTo Test Typist»
(vendor id Apple, чтобы одиночный Alt был горячей клавишей) захватывается
дежурным демоном при подключении, и всё, что она нажмёт, уйдёт в окно в
фокусе. Перед каждым нажатием проверяется, что в фокусе наше окно (ответ
расширения `FocusedApp`), иначе сценарий пропускается. Щелчки мышью не
делаются: синтетическая мышь двигала бы настоящий указатель.

Окно A и окно A2 — один процесс и один класс `pulinto-livetest`; окно B —
отдельный процесс с классом `pulinto-livetest-b`. Сценарии:
  1. выделил в A, Alt+Tab в B, тап Alt — ничего, в логе отказ;
  2. выделил в A, B переотображено без клавиш (фокус к B), тап Alt — ничего,
     в логе «сменилось окно»;
  3. выделил в A, подождал 20 с, тап Alt — переведено с первого тапа;
  4. выделил с клавиатуры (End, Shift+Left×3), тап Alt — переведено;
  5. выделил в A, A2 того же класса переотображено (фокус к A2), тап Alt —
     ничего и «сменилось окно». Расширение до правки от 16 октября молчало
     при смене окна внутри одного класса — тогда выделение из A печатается
     в A2; это и есть демонстрация пробела.
"""
import os
import subprocess
import sys
import threading
import time

LOG = os.path.expanduser("~/.local/state/pulinto/pulintod.log")
NAME = "PuLinTo Test Typist"
APPLE = 0x05AC
WAIT_S = 20
CLASS_A = "pulinto-livetest"
CLASS_B = "pulinto-livetest-b"

import gi  # noqa: E402

gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402


class Field:
    def __init__(self, title, text):
        self.window = Gtk.Window(title=title)
        self.window.set_default_size(500, 300)
        self.view = Gtk.TextView()
        self.buffer = self.view.get_buffer()
        self.buffer.set_text(text)
        self.window.add(self.view)
        self.window.show_all()

    def text(self):
        return self.buffer.get_text(*self.buffer.get_bounds(), True)

    def select_last_word(self):
        text = self.text()
        start = self.buffer.get_iter_at_offset(len(text) - len(text.split()[-1]))
        self.buffer.select_range(start, self.buffer.get_end_iter())

    def reset(self, text):
        self.buffer.set_text(text)
        self.buffer.place_cursor(self.buffer.get_end_iter())

    def remap(self):
        """Спрятать и показать заново: новое окно получает фокус без клавиш."""
        self.window.hide()
        self.window.show_all()
        self.view.grab_focus()


# ── вспомогательный процесс окна B ───────────────────────────────────────────
if "--window" in sys.argv:
    GLib.set_prgname(CLASS_B)
    b = Field("B", "")
    b.window.move(700, 80)

    def serve():
        for line in sys.stdin:
            cmd = line.strip()
            if cmd == "remap":
                GLib.idle_add(b.remap)
            elif cmd == "text":
                done = threading.Event()
                box = {}

                def get():
                    box["t"] = b.text()
                    done.set()
                    return False

                GLib.idle_add(get)
                done.wait(5)
                print(box.get("t", ""), flush=True)
            elif cmd == "reset":
                GLib.idle_add(b.reset, "")
            elif cmd == "quit":
                GLib.idle_add(Gtk.main_quit)
                return
        GLib.idle_add(Gtk.main_quit)

    threading.Thread(target=serve, daemon=True).start()
    Gtk.main()
    raise SystemExit(0)

# ── сценарии ─────────────────────────────────────────────────────────────────
import evdev  # noqa: E402
from evdev import UInput, ecodes as e  # noqa: E402

GLib.set_prgname(CLASS_A)
failures = []
checks = 0


def check(name, ok, detail=""):
    global checks
    checks += 1
    print(("  ok   " if ok else "  FAIL ") + name + ("" if ok else f"  — {detail}"), flush=True)
    if not ok:
        failures.append(name)


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


def focused_app():
    out = subprocess.run(
        ["gdbus", "call", "--session", "--dest", "ru.pulinto.Shell1", "--object-path",
         "/ru/pulinto/Shell1", "--method", "ru.pulinto.Shell1.FocusedApp"],
        capture_output=True, text=True,
    ).stdout.strip()
    return out.strip("(),'")


def tap(ui, code, hold=0.05):
    ui.write(e.EV_KEY, code, 1); ui.syn(); time.sleep(hold)
    ui.write(e.EV_KEY, code, 0); ui.syn(); time.sleep(0.05)


def log_since(mark):
    return open(LOG, "rb").read()[mark:].decode("utf-8", "replace")


class WindowB:
    def __init__(self):
        self.proc = subprocess.Popen(
            [sys.executable, "-I", __file__, "--window", "B"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
        )

    def cmd(self, line):
        self.proc.stdin.write(line + "\n"); self.proc.stdin.flush()

    def text(self):
        self.cmd("text")
        return self.proc.stdout.readline().rstrip("\n")

    def close(self):
        try:
            self.cmd("quit")
            self.proc.wait(3)
        except Exception:  # noqa: BLE001
            self.proc.kill()


def scenarios(a, a2, b, ui):
    def fresh():
        """A в фокусе с текстом; B и A2 пусты и не в фокусе."""
        on_gtk(a2.window.hide)
        b.cmd("reset")
        on_gtk(a.reset, "привет мир")
        on_gtk(a.remap)
        time.sleep(0.8)
        return os.path.getsize(LOG)

    def ours():
        app = focused_app()
        return app in (CLASS_A, CLASS_B), app

    def alt():
        ok, app = ours()
        if not ok:
            print(f"     в фокусе чужое окно ({app!r}), Alt не нажимаю", flush=True)
            return False
        tap(ui, e.KEY_LEFTALT)
        time.sleep(1.5)
        return True

    # 1. Alt+Tab
    mark = fresh()
    on_gtk(a.select_last_word); time.sleep(0.5)
    if ours()[0]:
        ui.write(e.EV_KEY, e.KEY_LEFTALT, 1); ui.syn(); time.sleep(0.08)
        tap(ui, e.KEY_TAB); time.sleep(0.08)
        ui.write(e.EV_KEY, e.KEY_LEFTALT, 0); ui.syn(); time.sleep(0.8)
    print(f"     (после Alt+Tab в фокусе: {focused_app()!r})", flush=True)
    pressed = alt()
    tail = log_since(mark)
    check("1 Alt+Tab: ничего не напечатано",
          on_gtk(a.text) == "привет мир" and b.text() == "",
          f"A={on_gtk(a.text)!r} B={b.text()!r}")
    check("1 Alt+Tab: в логе отказ", not pressed or "Selection is stale" in tail, tail[-300:])

    # 2. фокус к B без клавиш
    mark = fresh()
    on_gtk(a.select_last_word); time.sleep(0.5)
    b.cmd("remap"); time.sleep(0.8)
    print(f"     (после переотображения B в фокусе: {focused_app()!r})", flush=True)
    pressed = alt()
    tail = log_since(mark)
    check("2 смена окна: ничего не напечатано",
          on_gtk(a.text) == "привет мир" and b.text() == "",
          f"A={on_gtk(a.text)!r} B={b.text()!r}")
    check("2 смена окна: в логе «сменилось окно»", not pressed or "the window changed" in tail, tail[-300:])

    def ensure_ru():
        """Перед конверсией ru → us GNOME должен быть в ru: тогда демону
        нужен аккорд, и путь переключения проверяется каждый раз."""
        mru = subprocess.run(["gsettings", "get", "org.gnome.desktop.input-sources", "mru-sources"],
                             capture_output=True, text=True).stdout
        if mru.find("'ru") > mru.find("'us"):
            ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 1); ui.syn(); time.sleep(0.05)
            tap(ui, e.KEY_LEFTALT)
            ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 0); ui.syn(); time.sleep(0.4)
            print("     (GNOME переведён в ru аккордом)", flush=True)

    # 3. подождать
    mark = fresh()
    ensure_ru()
    on_gtk(a.select_last_word)
    time.sleep(WAIT_S)
    pressed = alt()
    tail = log_since(mark)
    check(f"3 через {WAIT_S} с: переведено с первого тапа",
          pressed and on_gtk(a.text) == "привет vbh", f"A={on_gtk(a.text)!r}")
    check("3 в логе «Selection conversion»", "Selection conversion" in tail, tail[-300:])

    # 4. выделение с клавиатуры
    mark = fresh()
    ensure_ru()
    if ours()[0] and focused_app() == CLASS_A:
        tap(ui, e.KEY_END); time.sleep(0.2)
        ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 1); ui.syn(); time.sleep(0.05)
        for _ in range(3):
            tap(ui, e.KEY_LEFT)
        ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 0); ui.syn(); time.sleep(0.5)
    pressed = alt()
    tail = log_since(mark)
    check("4 Shift+стрелки: переведено",
          pressed and on_gtk(a.text) == "привет vbh", f"A={on_gtk(a.text)!r}")
    check("4 в логе «Selection conversion»", "Selection conversion" in tail, tail[-300:])

    # 5. окно того же класса
    mark = fresh()
    on_gtk(a.select_last_word); time.sleep(0.5)
    on_gtk(a2.reset, "")
    on_gtk(a2.remap); time.sleep(0.8)
    pressed = alt()
    tail = log_since(mark)
    check("5 окно того же класса: ничего не напечатано",
          on_gtk(a.text) == "привет мир" and on_gtk(a2.text) == "",
          f"A={on_gtk(a.text)!r} A2={on_gtk(a2.text)!r}")
    check("5 окно того же класса: в логе «сменилось окно»",
          not pressed or "the window changed" in tail, tail[-300:])

    def chord():
        ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 1); ui.syn(); time.sleep(0.05)
        tap(ui, e.KEY_LEFTALT)
        ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 0); ui.syn(); time.sleep(0.4)

    def type_keys(codes):
        for code in codes:
            tap(ui, code); time.sleep(0.06)

    def after_chord(label, letter, next_word, expected):
        """Русское слово набрано, Alt+Shift, одиночная латинская буква, затем
        слово: буква не переводится по старому контексту сразу, а только
        вслед за следующим русским словом (хвост коротких слов)."""
        mark = fresh()
        ensure_ru()
        if not (ours()[0] and focused_app() == CLASS_A):
            check(label, False, f"в фокусе {focused_app()!r}")
            return
        tap(ui, e.KEY_END); time.sleep(0.2)
        type_keys([e.KEY_SPACE, e.KEY_V, e.KEY_B, e.KEY_H, e.KEY_SPACE])  # « мир» в ru
        time.sleep(0.8)
        chord()                                                         # → us
        type_keys([letter, e.KEY_SPACE])
        time.sleep(0.8)
        type_keys(next_word + [e.KEY_SPACE])
        time.sleep(1.5)
        check(label, on_gtk(a.text) == expected, f"A={on_gtk(a.text)!r}")

    # 6. намеренное переключение: `c` остаётся латинской, `the` — английское
    after_chord("6 после аккорда: `c the` не тронуто", e.KEY_C,
                [e.KEY_T, e.KEY_H, e.KEY_E], "привет мир мир c the ")
    # 7. переключение по привычке: `ns` → «ты», и хвост `b` → «и» вместе с ним
    after_chord("7 после аккорда: `b ns` → «и ты» через хвост", e.KEY_B,
                [e.KEY_N, e.KEY_S], "привет мир мир и ты ")

    def ensure_us():
        mru = subprocess.run(["gsettings", "get", "org.gnome.desktop.input-sources", "mru-sources"],
                             capture_output=True, text=True).stdout
        if mru.find("'ru") < mru.find("'us"):
            chord()
            print("     (GNOME переведён в us аккордом)", flush=True)

    def doubtful(label, start_ru, first, second, expected):
        """Спорное слово без контекста (`рук` против `her`, решено частотой)
        переписывается вслед за следующим словом, когда то решается в другой
        язык: и когда следующее переводится, и когда оно набрано верно."""
        mark = fresh()
        if not (ours()[0] and focused_app() == CLASS_A):
            check(label, False, f"в фокусе {focused_app()!r}")
            return
        on_gtk(a.reset, "")
        time.sleep(0.5)
        if start_ru:
            ensure_ru()
        else:
            ensure_us()
        # Переотображение окна сбросило контекст у демона: слово без соседей.
        type_keys(first + [e.KEY_SPACE])
        time.sleep(0.8)
        type_keys(second + [e.KEY_SPACE])
        time.sleep(1.5)
        check(label, on_gtk(a.text) == expected, f"A={on_gtk(a.text)!r}")

    HER = [e.KEY_H, e.KEY_E, e.KEY_R]              # `her` в us, «рук» в ru — одни клавиши
    PRIVET = [e.KEY_G, e.KEY_H, e.KEY_B, e.KEY_D, e.KEY_T, e.KEY_N]  # «привет» в us-раскладке
    # 8. в us: `her` оставлено по частоте; `ghbdtn` → «привет» переводится и тянет «рук»
    doubtful("8 спорное слово: `her ghbdtn` → «рук привет»", False, HER, PRIVET, "рук привет ")
    # 9. в ru: «рук» → `her` по частоте (раскладка ушла в us); «привет» набрано
    #    теми же клавишами, переводится обратно и возвращает «рук»
    doubtful("9 спорное слово: «рук» → `her`, затем «привет» → «рук привет»", True, HER, PRIVET, "рук привет ")

    def mixed_token(label, keys, expected):
        """Русский контекст набран в ru, аккорд в us, затем токен «русское
        слово, точка, латинский хвост» (`cfqn.ru`, `afqk.txt`): левая часть
        переводится, точка остаётся точкой, хвост — как набран."""
        mark = fresh()
        if not (ours()[0] and focused_app() == CLASS_A):
            check(label, False, f"в фокусе {focused_app()!r}")
            return
        on_gtk(a.reset, "")
        time.sleep(0.5)
        ensure_ru()
        type_keys(PRIVET + [e.KEY_SPACE])                               # «привет» в ru
        time.sleep(0.6)
        type_keys([e.KEY_V, e.KEY_B, e.KEY_H, e.KEY_SPACE])            # «мир» в ru
        time.sleep(0.8)
        chord()                                                         # → us
        time.sleep(0.8)
        type_keys(keys + [e.KEY_SPACE])
        time.sleep(1.5)
        check(label, on_gtk(a.text) == expected, f"A={on_gtk(a.text)!r}")

    # 10. домен: `cfqn.ru` в us после русского текста → «сайт.ru» (хвост из
    #     двух букв каскад судить не может — точка всё равно точка)
    mixed_token("10 домен: `cfqn.ru` → «сайт.ru»",
                [e.KEY_C, e.KEY_F, e.KEY_Q, e.KEY_N, e.KEY_DOT, e.KEY_R, e.KEY_U], "привет мир сайт.ru ")
    # 11. имя файла: `afqk.txt` → «файл.txt» (хвост — английское словарное слово)
    mixed_token("11 имя файла: `afqk.txt` → «файл.txt»",
                [e.KEY_A, e.KEY_F, e.KEY_Q, e.KEY_K, e.KEY_DOT, e.KEY_T, e.KEY_X, e.KEY_T], "привет мир файл.txt ")
    def ru_token(label, keys, expected):
        """Русский контекст, раскладка ru, затем токен на клавишах ru-раскладки:
        латинская аббревиатура, оставленная на русской раскладке (`шз` — `ip`)."""
        mark = fresh()
        if not (ours()[0] and focused_app() == CLASS_A):
            check(label, False, f"в фокусе {focused_app()!r}")
            return
        on_gtk(a.reset, "")
        time.sleep(0.5)
        ensure_ru()
        type_keys(PRIVET + [e.KEY_SPACE])
        time.sleep(0.6)
        type_keys([e.KEY_V, e.KEY_B, e.KEY_H, e.KEY_SPACE])
        time.sleep(0.8)
        type_keys(keys + [e.KEY_SPACE])
        time.sleep(1.8)
        check(label, on_gtk(a.text) == expected, f"A={on_gtk(a.text)!r}")

    IP_KEYS = [e.KEY_I, e.KEY_P]  # «шз» в ru, `ip` в us
    # 13-15. закрытый список латинских аббревиатур и разрез по дефису
    ru_token("13 аббревиатура: «шз» → `ip`", IP_KEYS, "привет мир ip ")
    ru_token("14 аббревиатура со знаком: «шзю» → `ip.`", IP_KEYS + [e.KEY_DOT], "привет мир ip. ")
    ru_token("15 составное: «шз-адрес» → `ip-адрес`",
             IP_KEYS + [e.KEY_MINUS, e.KEY_F, e.KEY_L, e.KEY_H, e.KEY_T, e.KEY_C], "привет мир ip-адрес ")
    # 16. отрицательный: русское слово из двух букв, чьё латинское чтение в списке
    #     (`ем` — `tv`: «я ем» остаётся)
    ru_token("16 слово «ем» остаётся русским", [e.KEY_T, e.KEY_V], "привет мир ем ")

    # 12. отрицательный: `yfuhtdf.ncz` — одно слово «нагреваются», ю на клавише
    #     точки; `ncz` не расширение, разреза быть не должно (ревью 19.1)
    mixed_token("12 слово с ю внутри: `yfuhtdf.ncz` → «нагреваются»",
                [e.KEY_Y, e.KEY_F, e.KEY_U, e.KEY_H, e.KEY_T, e.KEY_D, e.KEY_F, e.KEY_DOT, e.KEY_N, e.KEY_C, e.KEY_Z],
                "привет мир нагреваются ")


def idle_ms():
    out = subprocess.run(
        ["gdbus", "call", "--session", "--dest", "org.gnome.Mutter.IdleMonitor", "--object-path",
         "/org/gnome/Mutter/IdleMonitor/Core", "--method", "org.gnome.Mutter.IdleMonitor.GetIdletime"],
        capture_output=True, text=True,
    ).stdout
    digits = "".join(ch for ch in out if ch.isdigit())
    return int(digits) if digits else 0


def main():
    if not subprocess.run(["pgrep", "-x", "pulintod"], capture_output=True).stdout:
        raise SystemExit("дежурный pulintod не работает — сценариям нужен он")
    # Человек за машиной? Клавиши уйдут в то окно, что в фокусе.
    if idle_ms() < 60000 and os.environ.get("FORCE") != "1":
        raise SystemExit(f"ОТМЕНА: за машиной кто-то есть (простой {idle_ms()} мс)")
    a = Field("A", "привет мир")
    a.window.move(80, 80)
    a2 = Field("A2", "")
    a2.window.move(80, 450)
    a2.window.hide()
    b = WindowB()
    time.sleep(1.0)

    mark = os.path.getsize(LOG)
    ui = UInput({e.EV_KEY: list(range(1, 249))}, name=NAME, vendor=APPLE, product=0x0250, version=1)
    status = 2
    try:
        for _ in range(100):
            if f"Grab: {NAME}" in log_since(mark):
                break
            time.sleep(0.1)
        else:
            raise SystemExit("дежурный демон не захватил клавиатуру машинистки")
        time.sleep(0.5)

        def run():
            nonlocal status
            try:
                scenarios(a, a2, b, ui)
                status = 1 if failures else 0
            except Exception as error:  # noqa: BLE001
                print(f"сценарии оборваны: {error!r}", flush=True)
            finally:
                GLib.idle_add(Gtk.main_quit)

        threading.Thread(target=run, daemon=True).start()
        Gtk.main()
    finally:
        ui.close()
        b.close()
    print(f"проверок: {checks}, провалов: {len(failures)}" + (": " + ", ".join(failures) if failures else ""))
    return status


if __name__ == "__main__":
    raise SystemExit(main())
