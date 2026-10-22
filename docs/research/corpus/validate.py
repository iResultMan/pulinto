#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Validator for docs/research/corpus/golden.tsv - the golden benchmark corpus
for keyboard-layout misdetection (see docs/ALGORITHM.md).

This is a measurement instrument, not a fixture to be kept "green" by
loosening checks. It exits non-zero on the first class of problem it finds
after scanning the whole file, printing every offending row so problems can
be fixed at the source (usually the generator), never by hand-editing the
TSV to satisfy the checker.

Checks performed:
  1. Header matches the required 6 columns exactly, and every data row has
     exactly 6 fields.
  2. `input`, `expect`, `correct`, `category` are non-empty on every row
     (`context` is allowed to be empty).
  3. `expect` is one of KEEP / CONVERT.
  4. `category` is one of the 13 allowed values.
  5. If `expect == KEEP`, then `correct == input`.
  6. If `expect == CONVERT`, applying the positional US<->RU layout table to
     `input` (in one direction or the other) yields exactly `correct`. This
     is the check most likely to catch a broken generator: a single
     off-by-one in the layout table silently produces a corpus full of
     wrong "ground truth" answers.
  7. No duplicate (context, input) pairs.
  8. Category distribution matches the documented target composition of the
     corpus within +/-10% (relative).

Usage:
    python3 validate.py [path/to/golden.tsv]
Exit code 0 = clean, non-zero = at least one problem found (see stdout).
"""
import csv
import sys
import os
from collections import Counter, defaultdict

# ---------------------------------------------------------------------------
# Positional layout tables - identical to the ones used by the generator.
# NOTE: the us_lower row has a leading backtick ("`") prepended relative to
# the literal string given in the task description. Without it, us_lower is
# one character shorter than ru_lower/ru_upper/us_upper and every subsequent
# position is misaligned by one key, which silently breaks every CONVERT
# row's "correct" field. This was caught by hand-checking the task's own
# self-test (ghbdtn -> привет, руддщ -> hello) before generating anything.
# ---------------------------------------------------------------------------
US_LOWER = "`" + r"""1234567890-=qwertyuiop[]\asdfghjkl;'zxcvbnm,./"""
US_UPPER = r"""~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:"ZXCVBNM<>?"""
RU_LOWER = "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю."
RU_UPPER = 'Ё!"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,'

assert len(US_LOWER) == len(RU_LOWER) == len(US_UPPER) == len(RU_UPPER) == 47

US2RU, RU2US = {}, {}
for a, b in zip(US_LOWER, RU_LOWER):
    US2RU[a] = b
    RU2US[b] = a
for a, b in zip(US_UPPER, RU_UPPER):
    US2RU[a] = b
    RU2US[b] = a


def apply_map(s, mapping):
    return ''.join(mapping.get(ch, ch) for ch in s)


# self-check the tables themselves before trusting them for row-level checks
assert apply_map("ghbdtn", US2RU) == "привет"
assert apply_map("руддщ", RU2US) == "hello"

REQUIRED_HEADER = ["id", "context", "input", "expect", "correct", "category"]
ALLOWED_EXPECT = {"KEEP", "CONVERT"}
ALLOWED_CATEGORIES = {
    "prose_ru", "prose_en", "short1", "short2", "mixed", "url",
    "password", "code", "abbrev", "name", "translit", "number",
}

# Target composition of the corpus (see docs/research/corpus/README.md).
# Checked within +/-10% relative tolerance, not as an exact match, since the
# generator is allowed some slack when hitting round targets.
EXPECTED_CATEGORY_COUNTS = {
    "prose_ru": 850,   # 600 KEEP + 250 CONVERT
    "prose_en": 850,   # 600 KEEP + 250 CONVERT
    "short1": 120,
    "short2": 120,
    "password": 60,
    "url": 60,
    "code": 60,
    "abbrev": 40,
    "name": 40,
    "translit": 40,
    "mixed": 30,
    "number": 30,
}


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "golden.tsv"
    )

    errors = []

    with open(path, "r", encoding="utf-8", newline="") as f:
        reader = csv.reader(f, delimiter="\t")
        try:
            header = next(reader)
        except StopIteration:
            print(f"FAIL: {path} is empty")
            return 1

        if header != REQUIRED_HEADER:
            errors.append(f"header mismatch: expected {REQUIRED_HEADER}, got {header}")

        rows = []
        seen_pairs = defaultdict(list)
        seen_ids = set()

        for lineno, raw in enumerate(reader, start=2):  # header is line 1
            if len(raw) != 6:
                errors.append(f"line {lineno}: expected 6 columns, got {len(raw)}: {raw!r}")
                continue

            row_id, context, input_, expect, correct, category = raw
            rows.append((lineno, row_id, context, input_, expect, correct, category))

            if not row_id.strip().isdigit():
                errors.append(f"line {lineno}: id {row_id!r} is not a positive integer")
            elif row_id in seen_ids:
                errors.append(f"line {lineno}: duplicate id {row_id!r}")
            else:
                seen_ids.add(row_id)

            for field_name, value in [
                ("input", input_), ("expect", expect),
                ("correct", correct), ("category", category),
            ]:
                if value == "":
                    errors.append(f"line {lineno}: empty required field '{field_name}'")

            if expect not in ALLOWED_EXPECT and expect != "":
                errors.append(f"line {lineno}: expect={expect!r} not in {ALLOWED_EXPECT}")

            if category not in ALLOWED_CATEGORIES and category != "":
                errors.append(f"line {lineno}: category={category!r} not in {sorted(ALLOWED_CATEGORIES)}")

            if expect == "KEEP" and correct != input_:
                errors.append(
                    f"line {lineno}: expect=KEEP but correct != input "
                    f"(input={input_!r}, correct={correct!r})"
                )

            if expect == "CONVERT":
                candidates = {apply_map(input_, US2RU), apply_map(input_, RU2US)}
                if correct not in candidates:
                    errors.append(
                        f"line {lineno}: expect=CONVERT but layout-converting "
                        f"input={input_!r} gives {candidates!r}, not correct={correct!r}"
                    )
                if correct == input_:
                    errors.append(
                        f"line {lineno}: expect=CONVERT but correct == input "
                        f"({input_!r}) -- conversion would be a no-op"
                    )

            seen_pairs[(context, input_)].append(lineno)

    for (context, input_), lines in seen_pairs.items():
        if len(lines) > 1:
            errors.append(
                f"duplicate (context, input) pair {(context, input_)!r} "
                f"on lines {lines}"
            )

    # Category distribution check (+/-10% relative to target).
    actual_counts = Counter(r[6] for r in rows)
    for cat, target in EXPECTED_CATEGORY_COUNTS.items():
        actual = actual_counts.get(cat, 0)
        low, high = target * 0.9, target * 1.1
        if not (low <= actual <= high):
            errors.append(
                f"category {cat!r}: expected ~{target} (+/-10%: "
                f"[{low:.0f}, {high:.0f}]), got {actual}"
            )
    unexpected_cats = set(actual_counts) - set(EXPECTED_CATEGORY_COUNTS)
    if unexpected_cats:
        errors.append(f"unexpected categories present: {unexpected_cats}")

    total = len(rows)
    print(f"rows: {total}")
    for cat in sorted(actual_counts):
        print(f"  {cat}: {actual_counts[cat]}")
    print(f"expect KEEP: {sum(1 for r in rows if r[4] == 'KEEP')}")
    print(f"expect CONVERT: {sum(1 for r in rows if r[4] == 'CONVERT')}")

    if errors:
        print(f"\nFAIL: {len(errors)} problem(s) found:\n")
        for e in errors[:200]:
            print(" -", e)
        if len(errors) > 200:
            print(f" ... and {len(errors) - 200} more")
        return 1

    print("\nOK: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
