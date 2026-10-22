#!/usr/bin/env python3
"""Аудит словарей перед пересборкой индекса: печатает подозрительные записи,
ничего не меняет. Решение по каждой группе — у заказчика (правило: при
конфликте слов двух языков остаётся более частотное).

Проверки:
  shadow   — редкое слово, которое на другой раскладке читается частым словом
             другого языка (в корпус попал набор не в той раскладке);
  rows     — клавиатурные ряды и повторы одной буквы (йцукен, фыва, asdf, ааа);
  novowel  — четыре буквы и больше без единой гласной (аббревиатуры и мусор);
  alphabet — буквы не того алфавита или не-буквы внутри слова.

Запуск: audit_dict.py [--dict data/dict] [--min-other 500] [--max-self 5]
"""
import argparse
import os
import re

US = "qwertyuiop[]asdfghjkl;'zxcvbnm,./`"
RU = "йцукенгшщзхъфывапролджэячсмитьбю.ё"
U2R = str.maketrans(US, RU)
R2U = str.maketrans(RU, US)
ROWS = {"ru": ["йцукен", "йцук", "фыва", "фывап", "ячсм", "ячсми", "олдж", "йфя", "цув"],
        "en": ["qwerty", "qwer", "asdf", "asdfg", "zxcv", "zxcvb", "hjkl", "wasd", "qaz"]}
VOWELS = {"ru": "аеёиоуыэюя", "en": "aeiouy"}
ALPHA = {"ru": re.compile(r"^[а-яё]+(?:[-'][а-яё]+)*$"), "en": re.compile(r"^[a-z]+(?:[-'][a-z]+)*$")}


def load(path):
    words = {}
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            word, _, count = line.rstrip("\n").partition("\t")
            if word:
                words[word] = int(count or 0)
    return words


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dict", default="data/dict")
    parser.add_argument("--min-other", type=int, default=500, help="частота чужого слова, с которой тень подозрительна")
    parser.add_argument("--max-self", type=int, default=5, help="частота записи, до которой она считается редкой")
    args = parser.parse_args()

    dicts = {lang: load(os.path.join(args.dict, f"{lang}.tsv")) for lang in ("ru", "en")}
    other = {"ru": "en", "en": "ru"}
    shadow_of = {"ru": R2U, "en": U2R}
    findings = {}
    for lang, words in dicts.items():
        for word, count in words.items():
            tags = []
            if not ALPHA[lang].match(word):
                tags.append("alphabet")
            if any(word.startswith(row) or word == row for row in ROWS[lang]) or (len(word) >= 3 and len(set(word)) == 1):
                tags.append("rows")
            letters = word.replace("-", "")
            if len(letters) >= 4 and not any(ch in VOWELS[lang] for ch in letters):
                tags.append("novowel")
            if count <= args.max_self and len(word) >= 3:
                shadow = word.translate(shadow_of[lang])
                if shadow != word and dicts[other[lang]].get(shadow, 0) >= args.min_other:
                    tags.append(f"shadow:{shadow}={dicts[other[lang]][shadow]}")
            if tags:
                findings.setdefault(lang, []).append((word, count, tags))

    for lang, rows in findings.items():
        print(f"== {lang}: {len(rows)} записей")
        for tag in ("alphabet", "rows", "shadow", "novowel"):
            group = [r for r in rows if any(t.startswith(tag) for t in r[2])]
            if not group:
                continue
            print(f"-- {tag}: {len(group)}")
            for word, count, tags in sorted(group, key=lambda r: -r[1])[:40]:
                print(f"   {word}\t{count}\t{' '.join(tags)}")
            if len(group) > 40:
                print(f"   … ещё {len(group) - 40}")


if __name__ == "__main__":
    main()
