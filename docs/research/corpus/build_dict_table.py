#!/usr/bin/env python3
"""Precompute enchant dictionary membership for every word form the PuLinTo
cascade can ask about, so the Rust benchmark consults exactly the same
speller as the xneur benchmark did.

Why a table instead of calling enchant from Rust: the fairness requirement is
that both benchmarks hit the same libenchant-2.so.2 with the same dictionaries.
Reusing bench_xneur.py's already-validated ctypes binding guarantees that;
a second, independently written FFI binding would not.

Output: dict_table.tsv with columns `lang`, `word`, `known` (0/1).
"""

import csv
import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

# Layout tables, identical to crates/pulinto-core/src/layout.rs.
US_LOWER = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./"
US_UPPER = '~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:"ZXCVBNM<>?'
RU_LOWER = "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю."
RU_UPPER = 'Ё!"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,'

for name, table in [("US_LOWER", US_LOWER), ("US_UPPER", US_UPPER),
                    ("RU_LOWER", RU_LOWER), ("RU_UPPER", RU_UPPER)]:
    assert len(table) == 47, f"{name} has {len(table)} keys, expected 47"

TO_RU = str.maketrans(US_LOWER + US_UPPER, RU_LOWER + RU_UPPER)
TO_US = str.maketrans(RU_LOWER + RU_UPPER, US_LOWER + US_UPPER)


def convert(word, target):
    return word.translate(TO_RU if target == "ru" else TO_US)


def normalise(word):
    """Mirror of cascade::normalise in Rust: strip trailing digits, then
    leading ASCII punctuation and digits."""
    trimmed = word.rstrip("0123456789")
    return trimmed.lstrip("0123456789" + "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~")


def load_enchant_checker():
    spec = importlib.util.spec_from_file_location("bench_xneur", HERE / "bench_xneur.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules["bench_xneur"] = module
    spec.loader.exec_module(module)
    return module.EnchantChecker()


def main():
    checker = load_enchant_checker()
    corpus_path = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "golden.tsv"
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else corpus_path.parent / "dict_table.tsv"
    corpus = list(csv.DictReader(open(corpus_path, encoding="utf-8"), delimiter="\t"))

    # Every form the cascade may query: the word as typed and its conversion,
    # each attributed to the language it would be checked against.
    # The cascade's normalisation is layout-aware: `,` `;` `'` `[` `]` are the
    # Russian letters б ж э х ъ and must survive stripping. Rather than mirror
    # that logic here and risk the two drifting apart again, over-generate:
    # every plausible normalisation of every row, in both languages. A lookup
    # table costs nothing for entries nobody asks about.
    wanted = set()
    for row in corpus:
        raw = row["input"]
        for word in {raw, raw.rstrip("0123456789"), normalise(raw)}:
            if not word:
                continue
            for lang in ("ru", "us"):
                wanted.add((lang, word))
                wanted.add((lang, convert(word, lang)))

    rows = []
    for lang, word in sorted(wanted):
        # bench_xneur.EnchantChecker.check(lang, word); its lang keys are
        # exactly "ru"/"us", and it returns False for 1-character words,
        # mirroring xneur's own lower bound. Keeping that behaviour is what
        # makes the two benchmarks comparable.
        known = checker.check(lang, word)
        rows.append((lang, word, "1" if known else "0"))

    with open(out, "w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", quoting=csv.QUOTE_NONE, escapechar="\\")
        writer.writerow(["lang", "word", "known"])
        writer.writerows(rows)
    known_count = sum(1 for r in rows if r[2] == "1")
    print(f"{len(rows)} forms, {known_count} known by enchant -> {out}")


def lang_of(word):
    return "ru" if any("Ѐ" <= ch <= "ӿ" for ch in word) else "us"


if __name__ == "__main__":
    main()
