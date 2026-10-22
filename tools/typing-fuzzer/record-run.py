#!/usr/bin/env python3
"""Дописывает результат прогона в журнал стенда.

Вызывается самим run.sh в конце прогона. Смысл в том, чтобы запись не зависела
от того, следит ли кто-нибудь за кампанией: прогоны терялись пачками, когда
наблюдающий отвлекался, и цифры приходилось собирать руками по логам.
"""
import os
import re
import subprocess
import sys

ROOT = "/home/medushenko/dev/pulinto"
JOURNAL = os.path.join(ROOT, "docs/research/bench-history-stand.tsv")
HEADER = (
    "date\tcommit\tdict_kb\tsource\tmode\tpause\tdelim\twords\tmatched\tpct\t"
    "intact\tlost\tcorrupted\ttag"
)


def number(pattern, text, default=0):
    found = re.search(pattern, text)
    return int(found.group(1)) if found else default


def main(tag, mode, source, pause, delim):
    compare = os.path.join(ROOT, f"docs/research/fuzzer-logs/{tag}-compare.txt")
    if os.path.exists(os.path.join(ROOT, f"docs/research/fuzzer-logs/{tag}-INVALID")):
        print(f"прогон {tag} недействителен — в журнал не пишу")
        return 0
    if not os.path.exists(compare):
        print(f"нет файла сверки для {tag} — в журнал не пишу")
        return 1
    text = open(compare, encoding="utf-8").read()

    words = number(r"слов напечатано:\s+(\d+)", text)
    matched = number(r"совпало со эталоном: (\d+)/", text)
    pct = number(r"совпало со эталоном: \d+/\d+ \((\d+)%\)", text)
    intact = number(r"не исправлено \(текст цел\) (\d+)", text)
    lost = number(r"потеряно (\d+)", text)
    corrupted = number(r"ИСПОРЧЕНО (\d+)", text)
    if not words:
        print(f"сверка {tag} нечитаема — в журнал не пишу")
        return 1

    # Словарь опознаётся размером — единственное, что видно изнутри прогона и не
    # врёт: имя варианта нигде не записано, а размер отличается в шесть раз.
    dict_kb = os.path.getsize(os.path.join(ROOT, "data/dict/ru.fst")) // 1024
    try:
        commit = subprocess.check_output(
            ["git", "-C", ROOT, "rev-parse", "--short", "HEAD"], text=True
        ).strip()
    except Exception:
        commit = "?"
    date = subprocess.check_output(["date", "+%Y-%m-%d"], text=True).strip()

    fresh = not os.path.exists(JOURNAL)
    with open(JOURNAL, "a", encoding="utf-8") as handle:
        if fresh:
            handle.write(HEADER + "\n")
        handle.write(
            f"{date}\t{commit}\t{dict_kb}\t{source}\t{mode}\t{pause}\t{delim}\t"
            f"{words}\t{matched}\t{pct}\t{intact}\t{lost}\t{corrupted}\t{tag}\n"
        )
    print(f"записано в журнал: {tag} — {matched}/{words} ({pct}%), испорчено {corrupted}")
    return 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:6]))
