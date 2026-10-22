#!/usr/bin/env python3
"""Build a frequency word list from Leipzig Corpora sentence files.

No morphological analysis: a frequency slice of running text already contains
exactly the word forms people actually type, including neologisms and
inflections a 2000s-era stem dictionary never had. Generating unseen forms with
a morphological analyser would cost weeks and add forms nobody writes.

Input:  Leipzig `*-sentences.txt` files (format `id<TAB>sentence`).
Output: `word<TAB>count`, sorted by word, one language per file.

The letters of the language come from its language pack (`--pack`, the id of a
pack under data/languages or the path of a language.conf), so a new language needs
no change here.

Usage:
  gen_dict.py --pack ru --out ru.tsv --min-count 3 <sentences.txt>...
"""

import argparse
import collections
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import langpack  # noqa: E402

# Tokens are runs of letters of the target alphabet, allowing an internal
# hyphen or apostrophe (по-русски, don't). Anything containing a digit or a
# foreign letter is dropped: those are identifiers and code, not vocabulary.
def token_pattern(alphabet: str) -> re.Pattern:
    letters = re.escape(alphabet)
    return re.compile(rf"[{letters}]+(?:[-'][{letters}]+)*")


def count_tokens(paths, alphabet):
    pattern = token_pattern(alphabet)
    counts = collections.Counter()
    sentences = 0
    for path in paths:
        with open(path, encoding="utf-8", errors="replace") as handle:
            for line in handle:
                # Leipzig lines are `id<TAB>sentence`; drop the id.
                _, _, text = line.partition("\t")
                sentences += 1
                counts.update(pattern.findall(text.lower()))
    return counts, sentences


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sentences", nargs="+", type=Path)
    parser.add_argument("--pack", required=True, help="a language id or the path of a language.conf")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument(
        "--min-count",
        type=int,
        default=3,
        help="drop words rarer than this; guards against OCR noise and typos "
        "in the source corpus, which would otherwise become 'known words'",
    )
    args = parser.parse_args()

    pack = langpack.read(args.pack)
    counts, sentences = count_tokens(args.sentences, pack["alphabet"])
    kept = {word: n for word, n in counts.items() if n >= args.min_count}

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as handle:
        for word in sorted(kept):
            handle.write(f"{word}\t{kept[word]}\n")

    total = sum(counts.values())
    print(
        f"{pack['id']}: {sentences} sentences, {total} tokens, "
        f"{len(counts)} distinct, {len(kept)} kept at min-count={args.min_count} "
        f"-> {args.out}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
