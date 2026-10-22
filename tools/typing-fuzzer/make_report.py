#!/usr/bin/env python3
"""Отчёт «старт против сейчас» из results.tsv цепочки report-chain.sh.

    make_report.py <каталог отчёта> [--send]

Читает <каталог>/results.tsv (по одной строке на прогон: версия new/old),
heldout-{old,new}.txt, x11/hotkeys/chaos/live-wl.out и, если есть,
<каталог>/conclusions.txt (выводы и предложения — пишет человек или Claude
после чтения цифр). Печатает текст для Telegram, разбитый на сообщения
строкой `----8<----`; с --send каждое уходит через `hermes send` в топик
pulinto группы «Офис Медушенко» (telegram:-1004390494245:5).
"""
import csv
import os
import re
import subprocess
import sys

TOPIC = "telegram:-1004390494245:5"
LABELS = [
    ("mac600", "Стенд 600 слов, ru(mac), быстрая рука"),
    ("plain600", "Стенд 600 слов, обычная ru, быстрая рука"),
    ("mac600g", "Стенд 600 слов, ru(mac), мягкий набор"),
    ("plain600g", "Стенд 600 слов, обычная ru, мягкий набор"),
    ("frozen", "Веб-текст, замороженный (8000), быстрая рука"),
    ("frozeng", "Тот же веб-текст, мягкий набор"),
    ("hard", "Веб-текст + трудные токены (8000)"),
    ("it", "IT-жанр, seed 412 (8000)"),
    ("fresh", "СВЕЖИЙ веб-блок из интернета (8000)"),
]
SEP = "\n----8<----\n"


def bar(pct, lo=70.0, width=18):
    filled = max(0, min(width, round((pct - lo) / (100 - lo) * width)))
    return "█" * filled + "░" * (width - filled)


def load(directory):
    rows = {}
    path = os.path.join(directory, "results.tsv")
    if not os.path.exists(path):
        return rows
    with open(path, encoding="utf-8") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            try:
                nums = {k: int(row[k] or 0) for k in ("ok", "missed", "false", "corrupt", "lost", "лишних")}
                nums["punct"] = int(row.get("punct") or 0)
            except ValueError:
                continue
            total = nums["ok"] + nums["missed"] + nums["false"] + nums["corrupt"] + nums["lost"] + nums["punct"]
            rows[(row["метка"], row["версия"])] = {**nums, "total": total, "load": row.get("load", ""), "ev": row.get("событий_потеряно", "")}
    return rows


def pct(n, total):
    return 100.0 * n / total if total else 0.0


def line_pair(label, title, rows):
    new, old = rows.get((label, "new")), rows.get((label, "old"))
    if not new:
        return None
    out = [f"▪ {title}"]
    if old:
        po, pn = pct(old["ok"], old["total"]), pct(new["ok"], new["total"])
        d = pn - po
        out.append(f"  СТАРТ  {po:6.2f}% {bar(po)}")
        out.append(f"  СЕЙЧАС {pn:6.2f}% {bar(pn)}  {'▲' if d > 0.005 else '▼' if d < -0.005 else '='} {d:+.2f} п.п.")
        out.append(f"  пропуски {old['missed']}→{new['missed']} · ложные {old['false']}→{new['false']} · порча {old['corrupt']}→{new['corrupt']} · знак {old['punct']}→{new['punct']} · потери {old['lost']}→{new['lost']}")
    else:
        pn = pct(new["ok"], new["total"])
        out.append(f"  СЕЙЧАС {pn:6.2f}% {bar(pn)}   (старой версии нет)")
        out.append(f"  пропуски {new['missed']} · ложные {new['false']} · порча {new['corrupt']} · знак {new['punct']} · потери {new['lost']}")
    return "\n".join(out)


def totals(rows, version, labels):
    t = {"ok": 0, "missed": 0, "false": 0, "corrupt": 0, "punct": 0, "lost": 0, "total": 0}
    for label in labels:
        r = rows.get((label, version))
        if r:
            for k in t:
                t[k] += r[k]
    return t


def first_line(path):
    return open(path, encoding="utf-8").readline().strip() if os.path.exists(path) else "нет данных"


def summary_of(path):
    if not os.path.exists(path):
        return "нет данных"
    text = open(path, encoding="utf-8").read()
    found = re.findall(r"проверок: (\d+), провал\w*: (\d+)", text)
    return f"{found[-1][0]} проверок, провалов {found[-1][1]}" if found else "нет данных"


def main():
    directory = sys.argv[1]
    rows = load(directory)
    messages = []

    head = ["📊 PuLinTo · отчёт 19 октября 2026",
            "«Старт» = версия 8cda98d от 14 октября (до правок совета) на тех же словарях, что и сейчас,",
            "«сейчас» = 42fb132 (после трёх пакетов и сегодняшних правок).",
            "Одни и те же тексты, один seed, один стенд; успех = слово на экране в родном виде.",
            ""]
    body = []
    for label, title in LABELS:
        block = line_pair(label, title, rows)
        if block:
            body.append(block)
            body.append("")
    messages.append("\n".join(head + body))

    long_labels = ["frozen", "hard", "it", "fresh"]
    new, old = totals(rows, "new", long_labels), totals(rows, "old", long_labels)
    if new["total"]:
        lines = ["🧮 Свод по длинным блокам (веб ×3, IT): доли от всех слов", ""]
        lines.append(f"{'':16s}{'СТАРТ':>9s}{'СЕЙЧАС':>9s}")
        for key, name in (("ok", "успех"), ("missed", "пропущено"), ("false", "ложных"), ("corrupt", "порча"), ("punct", "не тот знак"), ("lost", "потери")):
            lines.append(f"{name:16s}{pct(old[key], old['total']):8.2f}%{pct(new[key], new['total']):8.2f}%" if old["total"] else f"{name:16s}{'-':>9s}{pct(new[key], new['total']):8.2f}%")
        lines += ["", f"слов в сводке: {old['total']} / {new['total']}", ""]
        lines += ["🔬 Эталонные проверки (новая версия)", "",
                  f"• heldout 21 300 слов, старт : {first_line(os.path.join(directory, 'heldout-old.txt')).split(': ', 1)[-1]}",
                  f"• heldout 21 300 слов, сейчас: {first_line(os.path.join(directory, 'heldout-new.txt')).split(': ', 1)[-1]}",
                  f"• X11-харнесс: {summary_of(os.path.join(directory, 'x11.out'))}",
                  f"• хоткеи: {summary_of(os.path.join(directory, 'hotkeys.out'))}",
                  f"• живой Wayland: {summary_of(os.path.join(directory, 'live-wl.out'))}",
                  "• хаос (40 циклов): " + ("; ".join(re.findall(r"scenario_\w+\s+\d+\s+\d+", open(os.path.join(directory, 'chaos.out'), encoding='utf-8').read())[:4]) if os.path.exists(os.path.join(directory, 'chaos.out')) else "нет данных"),
                  ""]
        messages.append("\n".join(lines))

    history = ["📈 Как менялись цифры (из журнала стенда, разные сценарии — не сравнивать прямо)", "",
               "15 окт  2000 слов, обычная ru ........ 96,75 % успеха, порча 3",
               "16 окт  600 слов, ru(mac) ............ 97,67 %, порча 6",
               "16 окт  2000 слов, ru(mac) ........... 97,15 %, порча 24",
               "17 окт  жанровая серия 120 000 слов .. 98,77 %, ложных 0,015 %, порча 358",
               "18 окт  веб-блок 20 000 слов ......... 99,02 %, ложных 0,005 %, порча 8",
               "18 окт  эталон 600 слов, ru(mac) ..... 99,67 %, порча 1",
               "heldout 21 300 слов: 99,36 % → 99,38 %, ложных 9 → 8, пропусков 127 → 123", ""]
    messages.append("\n".join(history))

    concl = os.path.join(directory, "conclusions.txt")
    if os.path.exists(concl):
        messages.append(open(concl, encoding="utf-8").read().strip())

    text = SEP.join(messages)
    print(text)
    if "--send" in sys.argv:
        for i, message in enumerate(messages):
            subprocess.run(["hermes", "send", "--to", TOPIC, "-q"] + (["-s", "PuLinTo · отчёт"] if i == 0 else []), input=message, text=True, check=False)


if __name__ == "__main__":
    main()
