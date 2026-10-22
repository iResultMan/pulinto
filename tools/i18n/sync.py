#!/usr/bin/env python3
"""Regenerates po/pulinto.pot (the template translators copy) from the English
strings in gui/pulinto-settings. Run it after adding or changing a `_("...")`
string, then add the new strings to po/ru.po, po/it.po and any other language
(`check.py` lists what is missing).

    python3 -I tools/i18n/sync.py            write the template
    python3 -I tools/i18n/sync.py --check    fail if it is out of date
"""

import ast
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pofile  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent.parent
HEADER = (
    "Project-Id-Version: pulinto\nLanguage: {lang}\nMIME-Version: 1.0\n"
    "Content-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n"
    "Plural-Forms: {plural}\n"
)


def source_messages():
    """Single messages in source order, and plural groups (one, many)."""
    tree = ast.parse((ROOT / "gui" / "pulinto-settings").read_text(encoding="utf-8"))
    singles, plurals = {}, {}
    for node in sorted((n for n in ast.walk(tree) if isinstance(n, ast.Call)), key=lambda n: (n.lineno, n.col_offset)):
        if not isinstance(node.func, ast.Name):
            continue
        if node.func.id == "_" and node.args and isinstance(node.args[0], ast.Constant):
            singles.setdefault(node.args[0].value, None)
        elif node.func.id == "plural" and len(node.args) == 3:
            one, many = (a.value for a in node.args[1:])
            plurals.setdefault((one, many), None)
    return list(singles), plurals


def render(tmp):
    singles, plurals = source_messages()
    pot = [(m, None, [""]) for m in singles] + [(one, many, ["", ""]) for (one, many) in plurals]
    pofile.write(tmp / "pulinto.pot", HEADER.format(lang="", plural="nplurals=2; plural=(n != 1);"), pot)


def main():
    target = ROOT / "po"
    if "--check" in sys.argv:
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            render(Path(tmp))
            stale = [n for n in ("pulinto.pot",) if not (target / n).exists()
                     or (target / n).read_text(encoding="utf-8") != (Path(tmp) / n).read_text(encoding="utf-8")]
        if stale:
            print("устарел po/pulinto.pot: выполните tools/i18n/sync.py")
            return 1
        return 0
    render(target)
    print("po/pulinto.pot обновлён")
    return 0


if __name__ == "__main__":
    sys.exit(main())
