#!/usr/bin/env python3
"""Checks that every string the settings window asks the catalogue for is
translated in every po/*.po, that placeholders match the English source, and
that the compiled catalogue loads. Exit code 1 on any problem.

    python3 -I tools/i18n/check.py
"""

import ast
import gettext
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compile as compiler  # noqa: E402
import pofile  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent.parent
CYRILLIC = re.compile(r"[А-Яа-яЁё]")


def source_messages():
    tree = ast.parse((ROOT / "gui" / "pulinto-settings").read_text(encoding="utf-8"))
    singles, plurals = set(), set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name):
            if node.func.id == "_" and node.args and isinstance(node.args[0], ast.Constant):
                singles.add(node.args[0].value)
            if node.func.id == "plural" and len(node.args) == 3:
                forms = [a.value for a in node.args[1:] if isinstance(a, ast.Constant)]
                plurals.add((forms[0], forms[1]))
    return singles, plurals


def placeholders(text):
    return sorted(re.findall(r"\{\w+\}", text))


def main():
    singles, plurals = source_messages()
    problems = [
        f"исходная строка не английская: {text[:60]!r}"
        for text in sorted(singles)
        if CYRILLIC.search(text.replace("ё", ""))
    ]
    for po in sorted((ROOT / "po").glob("*.po")):
        header, entries = pofile.parse(po)
        have = {(e[0], e[1]): e for e in entries}
        for text in sorted(singles):
            entry = have.get((text, None))
            if entry is None or not entry[2][0]:
                problems.append(f"{po.name}: нет перевода: {text[:60]!r}")
            elif placeholders(entry[2][0]) != placeholders(text):
                problems.append(f"{po.name}: не совпадают {{поля}}: {text[:60]!r}")
            elif po.stem != "ru" and CYRILLIC.search(entry[2][0].replace("ё", "")):
                problems.append(f"{po.name}: в переводе осталась кириллица: {entry[2][0][:60]!r}")
        for one, many in sorted(plurals):
            entry = have.get((one, many))
            forms = 3 if po.stem == "ru" else 2
            if entry is None or len(entry[2]) != forms or not all(entry[2]):
                problems.append(f"{po.name}: нет перевода множественного числа: {one!r}")
        for msgid, plural, _ in entries:
            if plural is None and msgid not in singles:
                problems.append(f"{po.name}: лишняя строка, в коде её нет: {msgid[:60]!r}")
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / po.stem / "LC_MESSAGES"
            target.mkdir(parents=True)
            (target / "pulinto.mo").write_bytes(compiler.build(entries, header))
            catalogue = gettext.translation("pulinto", tmp, [po.stem])
            sample = sorted(singles)[0]
            if catalogue.gettext(sample) == sample:
                problems.append(f"{po.name}: скомпилированный каталог не переводит {sample[:40]!r}")
    import subprocess
    if subprocess.run([sys.executable, "-I", str(ROOT / "tools" / "i18n" / "sync.py"), "--check"]).returncode:
        problems.append("po/pulinto.pot устарел")
    for problem in problems:
        print(problem)
    print(f"строк в коде: {len(singles)}, множественных: {len(plurals)}, проблем: {len(problems)}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
