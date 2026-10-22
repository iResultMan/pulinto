#!/usr/bin/env python3
"""Сверка трёх логов прогона: типист, приёмник, демон.

Эталон простой и потому надёжный: **что бы ни было набрано, на экране должно
оказаться слово в его родном виде.** Набрано в своей раскладке — демон обязан не
трогать; набрано в чужой — обязан починить. В обоих случаях итог один и тот же,
и сверять есть с чем.

Запуск: compare.py <typist.jsonl> <sink.log> <daemon.log>
"""
import collections
import difflib
import json
import re
import sys


def read_typist(path):
    rows = []
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def final_buffer(path):
    """Последнее состояние поля — то, что осталось на экране."""
    last = ""
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 3 and parts[1] == "buffer":
                last = parts[2]
    if last.startswith("'") and last.endswith("'"):
        last = last[1:-1]
    if "\\u" in last:
        last = re.sub(r'\\u([0-9a-fA-F]{4})', lambda m: chr(int(m.group(1), 16)), last)
    return last


def daemon_decisions(path):
    """Слова, о которых демон вообще высказался, и его вердикт."""
    seen = []
    # Два формата: диагностический (наблюдение) и краткий (захват).
    pattern = re.compile(
        r"'([^']*)'\s+(REPLACE → '([^']*)'|keep)"
        r"|Replacement: '([^']*)' → '([^']*)'"
    )
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            match = pattern.search(line)
            if match:
                seen.append(
                    {
                        "text": match.group(1) or match.group(4),
                        "converted": match.group(3) or match.group(5),
                        "line": line.rstrip("\n"),
                    }
                )
    return seen


def main(typist_path, sink_path, daemon_path):
    typed = read_typist(typist_path)
    screen = final_buffer(sink_path)
    decisions = daemon_decisions(daemon_path)

    expected = [row["word"] for row in typed]
    # Разделители в поле разные, поэтому режем по всему, что не буква.
    actual = [token for token in re.split(r"[^^\w]+", screen, flags=re.UNICODE) if token]

    # Выравнивание последовательностей вместо сверки по индексу.
    #
    # Наивное сопоставление `actual[i]` с `typed[i]` разваливается от первого же
    # расхождения: одно лишнее или пропавшее слово сдвигает всё дальнейшее, и
    # сверщик объявляет испорченными сотни слов, которые демон обработал верно.
    # Кампания стенда получила так «199 из 200 испорчено» — цифру, не значившую
    # ничего. difflib сопоставляет по совпадающим кускам и переживает вставки,
    # пропуски и замены.
    # Decisions are matched to words the same way, and for the same reason: the
    # daemon does not speak about every word. It never sees the ones typed while
    # it was injecting, and it says nothing about tokens it does not consider
    # words at all. Taking `decisions[i]` for word `i` — which this did — puts
    # some other word's verdict next to every line after the first silence, and
    # the campaign report of August 3rd was written from that column.
    shown = [row.get("shown") or "" for row in typed]
    spoken = [decision["text"] for decision in decisions]
    said_about = {}
    for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(
        None, shown, spoken, autojunk=False
    ).get_opcodes():
        if tag in ("equal", "replace"):
            for offset in range(min(i2 - i1, j2 - j1)):
                said_about[i1 + offset] = decisions[j1 + offset]

    matcher = difflib.SequenceMatcher(None, expected, actual, autojunk=False)
    pairs = []          # (индекс в typed, слово на экране или None)
    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag == "equal":
            for offset in range(i2 - i1):
                pairs.append((i1 + offset, actual[j1 + offset]))
        elif tag == "replace":
            for offset in range(i2 - i1):
                got = actual[j1 + offset] if j1 + offset < j2 else None
                pairs.append((i1 + offset, got))
        elif tag == "delete":
            for offset in range(i2 - i1):
                pairs.append((i1 + offset, None))
        # insert — лишние слова на экране; учтём отдельно
    extra = sum(j2 - j1 for tag, _, _, j1, j2 in matcher.get_opcodes() if tag == "insert")

    print(f"слов напечатано:      {len(typed)}")
    print(f"решений демона:       {len(decisions)}")
    print(f"слов осталось на экране: {len(actual)}   лишних фрагментов: {extra}")
    print()

    by_verdict = collections.Counter()
    problems = []
    for index, got in pairs:
        row = typed[index]
        want = row["word"]
        ok = got == want
        by_verdict[(row["expect"], "верно" if ok else "НЕВЕРНО")] += 1
        if not ok:
            problems.append(
                {
                    "seq": row["seq"],
                    "want": want,
                    "got": got,
                    "typed_in": row["typed_in"],
                    "native": row["native"],
                    "expect": row["expect"],
                    "shown": row.get("shown"),
                    "daemon": said_about[index]["line"] if index in said_about else None,
                }
            )

    # Испорчено — это когда набранного пользователем на экране НЕТ.
    #
    # Сверять по токенам для этого нельзя: экранный текст режется по всему
    # небуквенному, а `, . ; ' [ ]` — это буквы б ю ж э х ъ, то есть ровно те
    # клавиши, вокруг которых весь проект. «Республиканской», набранное в
    # латинице, выглядит как `Htcge,kbrfycrjq`, сверщик делил его надвое по
    # запятой и объявлял испорченным целое, невредимое слово. Четыре из четырёх
    # «порч» в прогоне по русскому тексту оказались этим.
    #
    # Поэтому вопрос задаётся прямо: осталось ли набранное на экране целиком.
    intact = sum(
        1
        for p in problems
        if p["shown"] and p["shown"] in screen
    )
    lost = sum(1 for p in problems if p["got"] is None and not (p["shown"] and p["shown"] in screen))
    corrupted = len(problems) - intact - lost
    print(f"из расхождений: не исправлено (текст цел) {intact}, потеряно {lost}, ИСПОРЧЕНО {corrupted}")
    print()

    print("итог по классам:")
    for (expect, verdict), count in sorted(by_verdict.items()):
        print(f"   {expect:<8} {verdict:<8} {count}")
    total = len(typed)
    right = sum(count for (_, verdict), count in by_verdict.items() if verdict == "верно")
    print(f"\nсовпало со эталоном: {right}/{total} ({right * 100 // max(total, 1)}%)")

    if problems:
        print(f"\nрасхождения ({len(problems)}), первые 15:")
        for problem in problems[:15]:
            print(
                f"   #{problem['seq']:<4} хотели {problem['want']!r} ({problem['native']}), "
                f"набрано в {problem['typed_in']} → на экране было {problem['shown']!r}"
            )
            print(f"        получили: {problem['got']!r}   ожидание: {problem['expect']}")
            if problem["daemon"]:
                print(f"        демон:    {problem['daemon'][:150]}")

    # Слова, о которых демон не сказал ничего, — самый опасный класс: он их не
    # видел, а значит и починить не мог. Считается по сопоставленным решениям, а
    # не вычитанием их числа: демон может высказаться и о том, чего типист не
    # печатал (собственная замена, разобранная на части), и разность уходила в
    # минус, сообщая «промолчал о -2 словах».
    silent = [index for index in range(len(typed)) if index not in said_about]
    if silent:
        print(f"\nдемон промолчал о {len(silent)} словах из {len(typed)} — он их не видел")
        unmatched = len(decisions) - len(said_about)
        if unmatched:
            print(f"и сказал о {unmatched}, которых типист не печатал")

    print(f"\nхвост экрана: {screen[-160:]!r}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2], sys.argv[3]))
