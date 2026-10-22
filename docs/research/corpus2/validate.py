#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Validator for docs/research/corpus2/heldout.tsv - the held-out, independent
benchmark corpus for keyboard-layout misdetection.

Unlike docs/research/corpus/golden.tsv (generated from docs/ALGORITHM.md,
the same document the cascade is implemented against), this corpus is built
from real Leipzig Corpora Collection news sentences (CC BY) plus real
system/source-derived "trap" tokens. See README.md in this directory for
provenance, licensing, and the labeling methodology.

This is a measurement instrument, not a fixture to be kept "green" by
loosening checks. It scans the whole file and reports every problem found
before exiting non-zero.

Checks performed:
  1. Header matches the required 8 columns exactly, and every data row has
     exactly 8 fields.
  2. `input`, `expect`, `correct`, `category`, `source` are non-empty on
     every row (`context` may be empty - start of sentence or a trap row;
     `ambiguous` must be exactly "0" or "1").
  3. `expect` is one of KEEP / CONVERT.
  4. `category` is one of the 6 allowed values.
  5. If `expect == KEEP`, then `correct == input`.
  6. If `expect == CONVERT`, applying the positional US<->RU layout table to
     `input` (in one direction or the other) yields exactly `correct`, and
     `correct != input` (a real conversion, not a no-op).
  7. No duplicate `id` values.
  8. No full-row duplicates, and no duplicate `(context, input)` pairs
     (the latter would mean the same on-screen situation is labeled two
     different ways elsewhere in the corpus).

Also prints (not gated on a fixed target, since this corpus's composition
is emergent from real text rather than a fixed generation spec):
  - row count and category distribution
  - KEEP/CONVERT split
  - `ambiguous` share among CONVERT rows
  - input-length histogram (1, 2, 3, 4, 5+ characters)

Usage:
    python3 validate.py [path/to/heldout.tsv]
Exit code 0 = clean, non-zero = at least one problem found (see stdout).
"""
import csv
import sys
import os
from collections import Counter, defaultdict

# ---------------------------------------------------------------------------
# Positional layout tables - identical to docs/research/corpus/validate.py.
# Self-check: ghbdtn -> привет, руддщ -> hello.
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


assert apply_map("ghbdtn", US2RU) == "привет"
assert apply_map("руддщ", RU2US) == "hello"

REQUIRED_HEADER = ["id", "context", "input", "expect", "correct", "category", "ambiguous", "source"]
ALLOWED_EXPECT = {"KEEP", "CONVERT"}
ALLOWED_CATEGORIES = {"ru", "en", "trap_url", "trap_code", "trap_num", "trap_abbrev"}
ALLOWED_AMBIGUOUS = {"0", "1"}


def bucket_len(n):
    return str(n) if n < 5 else "5+"


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "heldout.tsv"
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
        seen_full_rows = defaultdict(list)
        seen_ids = set()

        for lineno, raw in enumerate(reader, start=2):  # header is line 1
            if len(raw) != 8:
                errors.append(f"line {lineno}: expected 8 columns, got {len(raw)}: {raw!r}")
                continue

            row_id, context, input_, expect, correct, category, ambiguous, source = raw
            rows.append((lineno, row_id, context, input_, expect, correct, category, ambiguous, source))

            if not row_id.strip().isdigit():
                errors.append(f"line {lineno}: id {row_id!r} is not a positive integer")
            elif row_id in seen_ids:
                errors.append(f"line {lineno}: duplicate id {row_id!r}")
            else:
                seen_ids.add(row_id)

            for field_name, value in [
                ("input", input_), ("expect", expect), ("correct", correct),
                ("category", category), ("source", source),
            ]:
                if value == "":
                    errors.append(f"line {lineno}: empty required field '{field_name}'")

            if expect not in ALLOWED_EXPECT and expect != "":
                errors.append(f"line {lineno}: expect={expect!r} not in {ALLOWED_EXPECT}")

            if category not in ALLOWED_CATEGORIES and category != "":
                errors.append(f"line {lineno}: category={category!r} not in {sorted(ALLOWED_CATEGORIES)}")

            if ambiguous not in ALLOWED_AMBIGUOUS:
                errors.append(f"line {lineno}: ambiguous={ambiguous!r} not in {sorted(ALLOWED_AMBIGUOUS)}")

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
            seen_full_rows[(context, input_, expect, correct, category, ambiguous)].append(lineno)

    for (context, input_), lines in seen_pairs.items():
        if len(lines) > 1:
            errors.append(
                f"duplicate (context, input) pair {(context, input_)!r} on lines {lines}"
            )

    for key, lines in seen_full_rows.items():
        if len(lines) > 1:
            errors.append(f"full-row duplicate {key!r} on lines {lines}")

    # -----------------------------------------------------------------
    # Report (not gated - composition is emergent from real text).
    # -----------------------------------------------------------------
    total = len(rows)
    cat_counts = Counter(r[6] for r in rows)
    expect_counts = Counter(r[4] for r in rows)
    convert_rows = [r for r in rows if r[4] == "CONVERT"]
    ambiguous_convert = sum(1 for r in convert_rows if r[7] == "1")
    length_hist = Counter(bucket_len(len(r[3])) for r in rows)
    length_hist_by_cat = defaultdict(Counter)
    for r in rows:
        length_hist_by_cat[r[6]][bucket_len(len(r[3]))] += 1

    print(f"rows: {total}")
    print("category distribution:")
    for cat in sorted(cat_counts):
        print(f"  {cat}: {cat_counts[cat]}")
    print(f"expect KEEP: {expect_counts.get('KEEP', 0)}")
    print(f"expect CONVERT: {expect_counts.get('CONVERT', 0)}")
    if convert_rows:
        pct = 100.0 * ambiguous_convert / len(convert_rows)
        print(f"ambiguous CONVERT rows: {ambiguous_convert} / {len(convert_rows)} ({pct:.1f}%)")
    print("input-length histogram (chars, bucketed 1/2/3/4/5+):")
    for k in ["1", "2", "3", "4", "5+"]:
        print(f"  {k}: {length_hist.get(k, 0)}")
    print("input-length histogram by category:")
    for cat in sorted(length_hist_by_cat):
        row = length_hist_by_cat[cat]
        print(f"  {cat}: " + ", ".join(f"{k}={row.get(k, 0)}" for k in ["1", "2", "3", "4", "5+"]))

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
