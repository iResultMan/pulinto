#!/usr/bin/env python3
"""Сверка прогона по сценарию: что машинистка хотела против того, что в окне.

    compare_script.py <typist.jsonl> <batches.jsonl> [--errors errors.tsv]

Пачка за пачкой: ожидаемые слова (из журнала машинистки) выравниваются с
текстом, который сдало окно (`difflib` по словам), и каждое ожидаемое слово
получает один исход:

  ok        на экране ровно то, что задумано;
  missed    набрано в чужой раскладке, демон не тронул (на экране абракадабра
            в том виде, как она была напечатана) — терпимо, чинится Alt;
  false     набрано в своей раскладке, а на экране другое — демон испортил
            правильное слово. Недопустимо;
  corrupt   набрано в чужой раскладке, демон тронул, а вышло не то;
  punct     буквы верны, знак на конце не тот (`привет/` вместо `привет.`);
  lost      слова на экране нет вовсе, либо оно склеено с соседним.

Лишние слова на экране (которых не набирали) считаются отдельно — `extra`.
"""
import argparse
import collections
import difflib
import json
import re


def load_typist(path):
    words = collections.defaultdict(list)
    for line in open(path, encoding="utf-8"):
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue  # недописанная строка идущего прогона
        if "expect" in record:
            words[record["batch"]].append(record)
    return words


def load_batches(path):
    batches = {}
    for line in open(path, encoding="utf-8"):
        if line.strip():
            record = json.loads(line)
            batches[record["batch"]] = (record["text"], set(record.get("touched", [])), "touched" in record)
    return batches


US = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./"
US_SHIFT = '~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:"ZXCVBNM<>?'
RU = "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю."
RU_SHIFT = 'Ё!"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,'
TO_RU = dict(zip(US + US_SHIFT, RU + RU_SHIFT))
TO_US = dict(zip(RU + RU_SHIFT, US + US_SHIFT))


def other_layout(record):
    """Те же клавиши, прочитанные в другой раскладке, — что видно на экране,
    если слово набрано не в своей раскладке и осталось нетронутым."""
    table = TO_US if record["native"] == "ru" else TO_RU
    return "".join(table.get(ch, ch) for ch in record["expect"])


def letters(token):
    return re.sub(r"[^\w-]", "", token)


def judge(record, got, touched):
    """`touched` — стирал ли демон это слово; None, если окно этого не знает
    (живой приёмник), тогда верим записи машинистки о раскладке."""
    expect = record["expect"]
    if got == expect:
        return "ok"
    if touched is None:
        wrong = record["layout_was"] != record["native"]
        if wrong and got == record["shown"]:
            return "missed"
        if letters(got) == letters(expect):
            return "punct"
        return "corrupt" if wrong else "false"
    if letters(got) == letters(expect):
        return "punct"
    if got == other_layout(record):
        # Нетронутое — набрано не в той раскладке и не исправлено. Тронутое
        # и вышедшее «в другой раскладке» — демон перевернул верное слово.
        # После двойного Alt слово тронуто всегда: два шага цикла возвращают
        # его к тому, что было до них, — значит, оно и было не исправлено.
        return "false" if touched and not record.get("alt") else "missed"
    return "corrupt"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("typist")
    parser.add_argument("batches")
    parser.add_argument("--errors")
    args = parser.parse_args()

    expected = load_typist(args.typist)
    screens = load_batches(args.batches)
    counts = collections.Counter()
    typed_wrong = 0
    replaced = 0
    extra = 0
    errors = []

    last = max(screens) if screens else -1
    for batch in sorted(expected):
        records = expected[batch]
        if batch not in screens:
            if batch > last:
                continue  # пачка ещё не сдана
            counts["no-batch"] += len(records)
            continue
        screen, touched_set, knows = screens[batch]
        got = screen.split()
        want = [record["expect"] for record in records]
        matcher = difflib.SequenceMatcher(a=want, b=got, autojunk=False)
        for tag, a0, a1, b0, b1 in matcher.get_opcodes():
            if tag == "equal":
                counts["ok"] += a1 - a0
                continue
            if tag == "replace" and a1 - a0 == b1 - b0:
                pairs = zip(range(a0, a1), range(b0, b1))
                for i, j in pairs:
                    outcome = judge(records[i], got[j], (j in touched_set) if knows else None)
                    counts[outcome] += 1
                    if outcome != "ok":
                        errors.append((outcome, records[i], got[j], want, i))
                continue
            # Неравные куски: склейка, потеря или вставка. Что набрано —
            # потеряно; лишнее на экране — сверх того.
            for i in range(a0, a1):
                counts["lost"] += 1
                errors.append(("lost", records[i], " ".join(got[b0:b1]), want, i))
            extra += max(0, (b1 - b0) - (a1 - a0)) if tag != "delete" else 0
        typed_wrong += sum(record["layout_was"] != record["native"] for record in records)
        replaced += len(touched_set)

    total = sum(len(records) for batch, records in expected.items() if batch <= last)
    print(f"слов набрано: {total}, в чужой раскладке по записи машинистки: {typed_wrong}, "
          f"демон стирал слов: {replaced}")
    for outcome in ("ok", "missed", "false", "corrupt", "punct", "lost", "no-batch"):
        if counts[outcome] or outcome in ("ok", "false", "corrupt"):
            share = 100 * counts[outcome] / total if total else 0
            print(f"  {outcome:8} {counts[outcome]:7}  {share:6.2f}%")
    print(f"  лишних слов на экране: {extra}")

    if args.errors:
        with open(args.errors, "w", encoding="utf-8") as handle:
            handle.write("outcome\tseq\tlayout_was\tnative\texpect\tshown\tgot\tcontext\n")
            for outcome, record, got, want, i in errors:
                context = " ".join(want[max(0, i - 3):i])
                handle.write(f"{outcome}\t{record['seq']}\t{record['layout_was']}\t"
                             f"{record['native']}\t{record['expect']}\t{record['shown']}\t"
                             f"{got}\t{context}\n")


if __name__ == "__main__":
    main()
