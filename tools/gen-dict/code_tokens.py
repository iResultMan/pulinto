#!/usr/bin/env python3
"""Идентификаторы кода в английский словарь, чтобы `libc`, `ifdef`, `csv` не уходили в кириллицу.

Каскад отдаёт слово, которого нет ни в одном словаре, n-граммам: если в латинском чтении
есть запрещённая пара (`fd`, `bc`, `lx`), а кириллическое чисто, слово переводится
(`ifdef` → `шавуа`). Для кода это ложное срабатывание. Лечение — данные: слово есть в
`en.tsv` с малой частотой, тогда ступень словарей оставляет его (русское слово с тем же
набором клавиш по-прежнему побеждает по частоте или контексту).

  code_tokens.py mine ПАПКА... [--min-docs 5]   токены кода из файлов, по числу файлов (stdout, TSV)
  code_tokens.py apply ФАЙЛ.tsv... [--dict data/dict] [--freq 3] [--dry-run]

`apply` читает TSV (токен, группа или число файлов; строки с # пропускаются) и добавляет в
`<dict>/en.tsv` то, чего там нет и чьё кириллическое чтение не слово `ru.tsv` как минимум вчетверо частотнее. После него
пересобрать индекс (`build_dict 2`) и сверить с baseline по правилам `data/raw/README.md`.
"""
import argparse
import collections
import os
import re
import sys

US = "qwertyuiop[]asdfghjkl;'zxcvbnm,./`"
RU = "йцукенгшщзхъфывапролджэячсмитьбю.ё"
TO_RU = str.maketrans(US, RU)
# Как в каскаде (`Thresholds::margin`): русское слово на тех же клавишах не менее чем в 4 раза
# частотнее кода остаётся выше него, добавлять такой токен незачем.
MARGIN = 4
KEYBOARD_ROWS = {"asdf", "qwer", "zxcv", "qwerty", "hjkl"}
SUFFIXES = (".py", ".h", ".c", ".cc", ".cpp", ".rs", ".sh", ".js", ".ts", ".go", ".java", ".rb", ".pl", ".lua")


def read_words(path):
    words = {}
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            word, _, count = line.rstrip("\n").partition("\t")
            if word:
                words[word] = int(count or 0)
    return words


def mine(args):
    docs = collections.Counter()
    for root in args.dirs:
        for base, _, names in os.walk(root):
            for name in names:
                if not name.endswith(SUFFIXES):
                    continue
                try:
                    with open(os.path.join(base, name), encoding="utf-8", errors="ignore") as handle:
                        text = handle.read()
                except OSError:
                    continue
                docs.update(set(re.findall(r"\b[a-z]{3,14}\b", text)))
    for token, count in sorted(docs.items(), key=lambda item: (-item[1], item[0])):
        if count >= args.min_docs:
            print(f"{token}\t{count}")


def apply(args):
    path = os.path.join(args.dict, "en.tsv")
    english = read_words(path)
    russian = read_words(os.path.join(args.dict, "ru.tsv"))
    added, shadowed, known = [], [], 0
    for source in args.files:
        with open(source, encoding="utf-8") as handle:
            for line in handle:
                token = line.split("\t")[0].strip()
                if not token or token.startswith("#") or not re.fullmatch(r"[a-z]{3,}", token):
                    continue
                if len(set(token)) <= 2 or token in KEYBOARD_ROWS:
                    continue  # `aaaa`, `asdf`: набор для пробы клавиатуры, не идентификатор
                if token in english:
                    known += 1
                elif russian.get(token.translate(TO_RU), 0) >= MARGIN * args.freq:
                    shadowed.append(token)
                else:
                    english[token] = args.freq
                    added.append(token)
    print(f"добавлено {len(added)}, уже были {known}, пропущено (русское слово на тех же клавишах частотнее) {len(shadowed)}", file=sys.stderr)
    if shadowed:
        print("  пропущены: " + " ".join(shadowed[:40]), file=sys.stderr)
    if not args.dry_run:
        with open(path, "w", encoding="utf-8") as handle:
            for word in sorted(english, key=lambda w: w.encode()):
                handle.write(f"{word}\t{english[word]}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    mine_parser = sub.add_parser("mine")
    mine_parser.add_argument("dirs", nargs="+")
    mine_parser.add_argument("--min-docs", type=int, default=5)
    mine_parser.set_defaults(run=mine)
    apply_parser = sub.add_parser("apply")
    apply_parser.add_argument("files", nargs="+")
    apply_parser.add_argument("--dict", default="data/dict")
    apply_parser.add_argument("--freq", type=int, default=3)
    apply_parser.add_argument("--dry-run", action="store_true")
    apply_parser.set_defaults(run=apply)
    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
