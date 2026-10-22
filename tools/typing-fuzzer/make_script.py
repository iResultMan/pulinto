#!/usr/bin/env python3
"""Сценарий для машинистки: связный текст, русские и английские куски вперемешку.

Источник — тексты корпуса 3 (`docs/research/corpus3/sources/*_{ru,en}.txt`):
настоящие предложения по одиннадцати тематикам. Куски от 3 до 25 слов идут
подряд из одного файла, язык меняется между кусками — так печатает человек,
который пишет по-русски и вставляет английские термины и фразы.

Строка вывода — одно слово, как оно должно оказаться на экране, с точкой или
запятой на конце, если они там были. Слова с цифрами, смешанными алфавитами и
прочими знаками пропускаются: машинистка печатает только 47-клавишный блок,
и эталон должен быть однозначным.

    make_script.py --words 10000 --seed 1 --out script-10k.txt
"""
import argparse
import glob
import os
import random
import re
import unicodedata

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOURCES = os.path.join(ROOT, "docs/research/corpus3/sources")

RU = re.compile(r"^[а-яё]+(?:-[а-яё]+)*$", re.IGNORECASE)
EN = re.compile(r"^[a-z]+(?:-[a-z]+)*$", re.IGNORECASE)
EDGE = "«»\"'()[]{}—–“”„…:;!?"

# «Трудные» токены — те, что строгий фильтр выше отбрасывает, а машинистка
# набрать может: слова с цифрами, точкой внутри, апострофом, знаками ; : ! ? ( ) "
# на концах. Именно под них писались правила про разрезание токена, домены,
# цифры — и именно их стенд до 18 октября 2026 не видел (ревью, раздел 20).
# Набираемость — знаки 47-клавишного блока своей раскладки; смешанные алфавиты
# (`USB-порт`) и знаки вне раскладки («», —) не набрать — пропускаются.
NOT_ON_KEYBOARD = "«»“”„…—–‑\u00a0"
RU_KEYS = set("абвгдеёжзийклмнопрстуфхцчшщъыьэюяАБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ0123456789.,;:!?()-\"%*/")
EN_KEYS = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.,;:!?()-'\"/+#@&*=_%$<>[]{}\\`~^|")
HARD_POOL = os.path.join(ROOT, "tools/typing-fuzzer/hard-tokens.txt")


def pool_tokens(path):
    """Рукописный пул трудных токенов: `язык<TAB>класс<TAB>токен`; `mixed` и
    ненабираемые пропускаются (с подсчётом)."""
    pool, skipped = {"ru": [], "en": []}, 0
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line or line.startswith("#"):
            continue
        lang, klass, token = line.split("\t")
        keys = {"ru": RU_KEYS, "en": EN_KEYS}.get(lang)
        if keys is None or not all(ch in keys for ch in token):
            skipped += 1
            continue
        pool[lang].append((token, klass))
    return pool, skipped
CYR = re.compile(r"[а-яё]", re.IGNORECASE)
LAT = re.compile(r"[a-z]", re.IGNORECASE)


def hard_class(token, lang):
    """Класс трудного токена своего языка или None (обычный, чужой, не набрать)."""
    cyr, lat = CYR.search(token), LAT.search(token)
    if (cyr and lat) or not (cyr or lat) or (lang == "ru") != bool(cyr):
        return None
    core = token.rstrip(".,")
    if (RU if cyr else EN).match(core) and len(token) - len(core) <= 1:
        return None
    if not all(ch in (RU_KEYS if cyr else EN_KEYS) for ch in token):
        return None
    if re.search(r"\d", token):
        return "цифры"
    if "'" in token:
        return "апостроф"
    if "." in core:
        return "точка внутри"
    if re.search(r'[;:!?()"]', token):
        return "разделитель"
    return "прочее"


def hard_tokens(path, lang):
    """Трудные токены файла с классами, в порядке текста."""
    text = open(path, encoding="utf-8").read()
    for raw in text.split():
        token = raw.strip(NOT_ON_KEYBOARD)
        klass = hard_class(token, lang)
        if klass:
            yield token, klass


def tokens(path, pattern):
    text = open(path, encoding="utf-8").read()
    # Знаки ударения (о́) — комбинирующие символы, на клавиатуре их нет.
    text = unicodedata.normalize("NFD", text)
    text = "".join(ch for ch in text if ch != "́")
    text = unicodedata.normalize("NFC", text)
    for raw in text.split():
        word = raw.strip(EDGE)
        tail = ""
        if word and word[-1] in ".,":
            tail = word[-1]
            word = word[:-1].strip(EDGE)
        if word and pattern.match(word):
            yield word + tail


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--words", type=int, required=True)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--ru-share", type=float, default=0.6)
    parser.add_argument("--out", required=True)
    parser.add_argument("--sources", default=SOURCES,
                        help="каталог с *_{ru,en}.txt (по умолчанию корпус 3)")
    parser.add_argument("--topics", default="",
                        help="через запятую: брать только эти тематики (имя файла до _ru/_en), "
                             "например it или law,finance — так собирается жанровый сценарий")
    parser.add_argument("--hard-share", default="0",
                        help="доля трудных токенов (цифры, точка внутри, апостроф, знаки на концах): "
                             "число 0..1 или natural — как в исходных текстах; 0 — прежний чистый сценарий")
    parser.add_argument("--hard-pool", default=HARD_POOL,
                        help="рукописный пул трудных токенов (по умолчанию tools/typing-fuzzer/hard-tokens.txt); "
                             "harvested — брать трудные токены из самих текстов")
    parser.add_argument("--hard-list", default="",
                        help="записать сюда список трудных токенов источников с классами и счётчиками")
    args = parser.parse_args()

    topics = {t.strip() for t in args.topics.split(",") if t.strip()}
    rng = random.Random(args.seed)
    pools = {}
    for lang, pattern in (("ru", RU), ("en", EN)):
        paths = sorted(glob.glob(os.path.join(args.sources, f"*_{lang}.txt")))
        if topics:
            paths = [p for p in paths if os.path.basename(p)[: -len(f"_{lang}.txt")] in topics]
        if not paths:
            raise SystemExit(f"нет источников {lang} для тематик {sorted(topics)} в {args.sources}")
        pools[lang] = [list(tokens(path, pattern)) for path in paths]
    cursors = {lang: [rng.randrange(len(pool)) for pool in pools[lang]] for lang in pools}

    # Трудные токены источников — для доли `natural` и списка `--hard-list`;
    # подмешивается рукописный пул (решение заказчика: такие слова писать
    # самим, по классам, а не собирать из текста), либо собранные — `harvested`.
    harvested = {lang: [] for lang in pools}
    for lang in pools:
        paths = sorted(glob.glob(os.path.join(args.sources, f"*_{lang}.txt")))
        if topics:
            paths = [p for p in paths if os.path.basename(p)[: -len(f"_{lang}.txt")] in topics]
        for path in paths:
            harvested[lang].extend(hard_tokens(path, lang))
    if args.hard_pool == "harvested":
        hard, skipped = harvested, 0
    else:
        hard, skipped = pool_tokens(args.hard_pool)
        if skipped:
            print(f"пул {args.hard_pool}: пропущено ненабираемых токенов {skipped}")
    share = {}
    for lang in pools:
        total = sum(len(pool) for pool in pools[lang]) + len(harvested[lang])
        natural = len(harvested[lang]) / total if total else 0.0
        share[lang] = natural if args.hard_share == "natural" else float(args.hard_share)
    if args.hard_list:
        counts = {}
        for lang in harvested:
            for token, klass in harvested[lang]:
                counts[(lang, klass, token)] = counts.get((lang, klass, token), 0) + 1
        with open(args.hard_list, "w", encoding="utf-8") as handle:
            handle.write("# язык\tкласс\tсколько\tтокен\n")
            for (lang, klass, token), n in sorted(counts.items(), key=lambda item: (item[0][0], item[0][1], -item[1], item[0][2])):
                handle.write(f"{lang}\t{klass}\t{n}\t{token}\n")
    for lang in pools:
        print(f"{lang}: трудных токенов в источниках {len(harvested[lang])}, в пуле {len(hard[lang])}, доля в сценарии {share[lang]:.3f}")

    out = []
    while len(out) < args.words:
        lang = "ru" if rng.random() < args.ru_share else "en"
        index = rng.randrange(len(pools[lang]))
        pool = pools[lang][index]
        start = cursors[lang][index]
        size = rng.randint(3, 25)
        chunk = [pool[(start + offset) % len(pool)] for offset in range(size)]
        cursors[lang][index] = (start + size) % len(pool)
        # Трудные токены ложатся внутрь куска своего языка — контекст у них
        # тот же, что у человека, который их набирает.
        if hard[lang] and share[lang] > 0:
            for _ in range(size):
                if rng.random() < share[lang]:
                    token, _klass = rng.choice(hard[lang])
                    chunk.insert(rng.randrange(len(chunk) + 1), token)
        out.extend(chunk)

    with open(args.out, "w", encoding="utf-8") as handle:
        handle.write("\n".join(out[: args.words]) + "\n")


if __name__ == "__main__":
    main()
