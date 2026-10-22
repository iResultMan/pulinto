"""Reading and writing the tiny subset of .po used by the project: msgid,
msgid_plural, msgstr and msgstr[n]; no contexts, no fuzzy flags."""

import re


def quote(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def unquote(token):
    token = token.strip()
    assert token.startswith('"') and token.endswith('"'), token
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), token[1:-1])


def parse(path):
    """Returns (header_text, [(msgid, msgid_plural or None, [msgstr...])])."""
    entries, current, field = [], None, None
    with open(path, encoding="utf-8") as handle:
        for raw in handle:
            line = raw.strip()
            if not line or line.startswith("#"):
                if current is not None and not line:
                    entries.append(current)
                    current, field = None, None
                continue
            if line.startswith("msgid_plural"):
                current[1] = unquote(line[len("msgid_plural"):])
                field = ("plural",)
            elif line.startswith("msgid"):
                if current is not None:
                    entries.append(current)
                current = [unquote(line[len("msgid"):]), None, []]
                field = ("id",)
            elif line.startswith("msgstr["):
                index = int(line[len("msgstr["):line.index("]")])
                text = unquote(line[line.index("]") + 1:])
                while len(current[2]) <= index:
                    current[2].append("")
                current[2][index] = text
                field = ("str", index)
            elif line.startswith("msgstr"):
                current[2] = [unquote(line[len("msgstr"):])]
                field = ("str", 0)
            elif line.startswith('"'):
                text = unquote(line)
                if field[0] == "id":
                    current[0] += text
                elif field[0] == "plural":
                    current[1] += text
                else:
                    current[2][field[1]] += text
    if current is not None:
        entries.append(current)
    header = ""
    for msgid, _plural, strings in entries:
        if msgid == "":
            header = strings[0]
    return header, [e for e in entries if e[0] != ""]


def write(path, header, entries):
    lines = ['msgid ""', "msgstr " + quote(header), ""]
    for msgid, plural, strings in entries:
        lines.append("msgid " + quote(msgid))
        if plural is not None:
            lines.append("msgid_plural " + quote(plural))
            for i, text in enumerate(strings):
                lines.append(f"msgstr[{i}] " + quote(text))
        else:
            lines.append("msgstr " + quote(strings[0] if strings else ""))
        lines.append("")
    with open(path, "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines))
