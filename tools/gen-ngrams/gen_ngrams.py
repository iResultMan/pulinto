#!/usr/bin/env python3
"""Собирает списки ЗАПРЕЩЁННЫХ буквосочетаний из корпуса.

Данные строятся из корпусов проекта и распространяются под CC BY 4.0
(см. data/ATTRIBUTION.md); внешних пакетов для работы не нужно.

Метод. Сочетание считается допустимым, только если встретилось не реже
`--min-count` раз: одно случайное вхождение (опечатка в исходнике) не делает его
допустимым навсегда. Буквы языка берутся из его языкового пакета (`--pack`).

Учитываются границы слова: сочетания собираются внутри слов, а не через
пробелы, иначе «в тот» дало бы биграмму «вт», которой внутри слова не бывает.
"""

import argparse
import collections
import itertools
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import langpack  # noqa: E402

def tokens(paths, alphabet, fold):
    pattern = re.compile(rf"[{re.escape(alphabet)}]+")
    for path in paths:
        with open(path, encoding="utf-8", errors="replace") as handle:
            for line in handle:
                _, _, text = line.partition("\t")
                text = text.lower()
                for source, target in fold.items():  # `ё` is inconsistent in corpora: counted as `е`
                    text = text.replace(source, target)
                yield from pattern.findall(text)


def observed(paths, alphabet, fold, size):
    """Сколько раз каждое сочетание длины `size` встретилось внутри слов."""
    counts = collections.Counter()
    for word in tokens(paths, alphabet, fold):
        for index in range(len(word) - size + 1):
            counts[word[index : index + size]] += 1
    return counts


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sentences", nargs="+", type=Path)
    parser.add_argument("--pack", required=True, help="id языка или путь к его language.conf")
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument(
        "--min-count",
        type=int,
        default=3,
        help="сочетание считается допустимым от этого числа вхождений; "
        "порог отсекает опечатки и мусор исходных текстов",
    )
    args = parser.parse_args()

    pack = langpack.read(args.pack)
    fold = pack["fold"]
    # Folded letters (`ё`) are not combinations of their own.
    alphabet = "".join(ch for ch in pack["alphabet"] if ch not in fold)
    args.out_dir.mkdir(parents=True, exist_ok=True)

    for size, filename in ((2, "proto"), (3, "proto3")):
        counts = observed(args.sentences, alphabet, fold, size)
        allowed = {gram for gram, count in counts.items() if count >= args.min_count}
        every = {"".join(combo) for combo in itertools.product(alphabet, repeat=size)}
        forbidden = sorted(every - allowed)

        path = args.out_dir / filename
        path.write_text("\n".join(forbidden) + "\n", encoding="utf-8")
        print(
            f"{pack['id']}/{filename}: всего {len(every)}, "
            f"встретилось {len(allowed)}, запрещено {len(forbidden)} "
            f"(порог {args.min_count}) -> {path}",
            file=sys.stderr,
        )


if __name__ == "__main__":
    main()
