#!/usr/bin/env python3
"""Compiles po/<lang>.po into data/locale/<lang>/LC_MESSAGES/pulinto.mo, the
catalogue the settings window reads. No gettext tools needed.

    python3 -I tools/i18n/compile.py [output-dir]      (default: data/locale)
"""

import os
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pofile  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent.parent


def build(entries, header):
    messages = {"": header}
    for msgid, plural, strings in entries:
        if not all(strings):
            continue  # untranslated: the English source text is shown
        key = msgid if plural is None else msgid + "\0" + plural
        messages[key] = "\0".join(strings)
    keys = sorted(messages)
    ids = b""
    strs = b""
    offsets = []
    for key in keys:
        encoded_key, encoded_value = key.encode("utf-8"), messages[key].encode("utf-8")
        offsets.append((len(ids), len(encoded_key), len(strs), len(encoded_value)))
        ids += encoded_key + b"\0"
        strs += encoded_value + b"\0"
    count = len(keys)
    keystart = 7 * 4 + 16 * count
    valuestart = keystart + len(ids)
    table = []
    for o1, l1, o2, l2 in offsets:
        table += [l1, o1 + keystart]
    for o1, l1, o2, l2 in offsets:
        table += [l2, o2 + valuestart]
    output = struct.pack("Iiiiiii", 0x950412DE, 0, count, 7 * 4, 7 * 4 + count * 8, 0, 0)
    output += struct.pack(f"{len(table)}i", *table) + ids + strs
    return output


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "data" / "locale"
    for po in sorted((ROOT / "po").glob("*.po")):
        header, entries = pofile.parse(po)
        target = out / po.stem / "LC_MESSAGES"
        target.mkdir(parents=True, exist_ok=True)
        (target / "pulinto.mo").write_bytes(build(entries, header))
        print(f"{po.name}: {len(entries)} строк -> {target / 'pulinto.mo'}")


if __name__ == "__main__":
    main()
