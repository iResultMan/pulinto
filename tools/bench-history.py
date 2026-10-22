#!/usr/bin/env python3
"""Журнал замеров: считает файл предсказаний и дописывает строку в историю.

Заведён потому, что цифры в этом проекте появлялись по одной, жили в отчётах и
не сравнивались между собой. Пока замер не записан рядом с предыдущим, «стало
лучше» — это мнение.

Запуск:
    bench-history.py <predictions.tsv> --dict small --corpus corpus2 [--note "..."]
    bench-history.py <predictions.tsv> --dict small --corpus corpus3 --skip <файл-с-id>

История — docs/research/bench-history.tsv, по строке на замер, с отраслями в
отдельном файле bench-history-domains.tsv.
"""
import argparse
import collections
import os
import subprocess
import sys

HISTORY = "docs/research/bench-history.tsv"
DOMAINS = "docs/research/bench-history-domains.tsv"

HEADER = (
    "date\tcommit\tdict\tcorpus\trows\taccuracy\tfalse_positives\tmisses\t"
    "fp_count\tmiss_count\tnote"
)
DOMAIN_HEADER = "date\tcommit\tdict\tcorpus\tcategory\trows\taccuracy\tfalse_positives\tmisses"


def git_commit():
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"], text=True
        ).strip()
    except Exception:
        return "?"


def score(path, skip):
    """Общий счёт и разбивка по категориям."""
    total = collections.Counter()
    by_category = collections.defaultdict(collections.Counter)
    with open(path, encoding="utf-8") as handle:
        header = handle.readline().rstrip("\n").split("\t")
        col = {name: index for index, name in enumerate(header)}
        for line in handle:
            row = line.rstrip("\n").split("\t")
            if len(row) < len(header) or row[col["id"]] in skip:
                continue
            category = row[col["category"]]
            for bucket in (total, by_category[category]):
                bucket["rows"] += 1
                bucket["right"] += row[col["predicted_text"]] == row[col["correct"]]
                if row[col["expect"]] == "KEEP":
                    bucket["keep"] += 1
                    bucket["fp"] += row[col["predicted"]] == "CONVERT"
                else:
                    bucket["convert"] += 1
                    bucket["miss"] += row[col["predicted"]] == "KEEP"
    return total, by_category


def rates(bucket):
    rows = max(bucket["rows"], 1)
    return (
        bucket["right"] / rows * 100,
        bucket["fp"] / max(bucket["keep"], 1) * 100,
        bucket["miss"] / max(bucket["convert"], 1) * 100,
    )


def append(path, header, line):
    fresh = not os.path.exists(path)
    with open(path, "a", encoding="utf-8") as handle:
        if fresh:
            handle.write(header + "\n")
        handle.write(line + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("predictions")
    parser.add_argument("--dict", required=True)
    parser.add_argument("--corpus", required=True)
    parser.add_argument("--date", required=True, help="дата замера, YYYY-MM-DD")
    parser.add_argument("--skip", help="файл со списком id, которые не считать")
    parser.add_argument("--note", default="")
    args = parser.parse_args()

    skip = set()
    if args.skip:
        with open(args.skip, encoding="utf-8") as handle:
            skip = {line.strip() for line in handle if line.strip()}

    total, by_category = score(args.predictions, skip)
    accuracy, false_positives, misses = rates(total)
    commit = git_commit()

    append(
        HISTORY,
        HEADER,
        f"{args.date}\t{commit}\t{args.dict}\t{args.corpus}\t{total['rows']}\t"
        f"{accuracy:.2f}\t{false_positives:.2f}\t{misses:.2f}\t"
        f"{total['fp']}\t{total['miss']}\t{args.note}",
    )
    for category, bucket in sorted(by_category.items()):
        category_accuracy, category_fp, category_miss = rates(bucket)
        append(
            DOMAINS,
            DOMAIN_HEADER,
            f"{args.date}\t{commit}\t{args.dict}\t{args.corpus}\t{category}\t"
            f"{bucket['rows']}\t{category_accuracy:.2f}\t{category_fp:.2f}\t{category_miss:.2f}",
        )

    print(
        f"{args.dict:<18} {args.corpus:<8} строк {total['rows']:>8}  "
        f"точность {accuracy:.2f}%  ложные {false_positives:.2f}%  пропуски {misses:.2f}%"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
