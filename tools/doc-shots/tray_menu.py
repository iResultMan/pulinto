#!/usr/bin/env python3
"""Draws the menu of the tray icon for the manual, with numbered markers.

The desktop draws the real menu and it looks different on every desktop, so the
picture is a diagram: the items are in the order of crates/pulintod/src/tray.rs and
the texts are those of the program's own table (crates/pulintod/src/i18n.rs).

    python3 -I tools/doc-shots/tray_menu.py <out dir>
"""

import re
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent.parent
REGULAR = "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf"
BOLD = "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf"
SYMBOLS = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
RED = (214, 50, 28)


def table():
    source = (ROOT / "crates/pulintod/src/i18n.rs").read_text(encoding="utf-8")
    block = source[source.index("const TABLE"):source.index("];", source.index("const TABLE"))]
    lit = r'"((?:[^"\\]|\\.)*)"'
    rows = re.findall(r"\(\s*" + lit + r",\s*" + lit + r",\s*" + lit + r",?\s*\)", block)
    unescape = lambda text: text.replace('\\"', '"').replace("\\\\", "\\")
    return {unescape(en): {"en": unescape(en), "ru": unescape(ru)} for en, ru, it in rows}


# (marker number, text key, kind) kind: info, item, check, sub, sep
ITEMS = [
    (1, "PuLinTo is working · words fixed: {count}", "info"),
    (2, "Pause", "item"),
    (3, "Fix automatically", "check"),
    (4, "Restart", "item"),
    (None, None, "sep"),
    (5, "In the application \"{class}\"", "sub"),
    (6, "Settings…", "item"),
    (7, "Interface language", "sub"),
    (8, "Hints", "sub"),
    (None, None, "sep"),
    (9, "Quit", "item"),
]


def draw(lang, out):
    words = table()
    regular = ImageFont.truetype(REGULAR, 17)
    bold = ImageFont.truetype(BOLD, 16)
    symbols = ImageFont.truetype(SYMBOLS, 17)
    width, row = 380, 38
    height = sum(14 if kind == "sep" else row for _, _, kind in ITEMS) + 28
    image = Image.new("RGB", (width + 70, height + 28), (255, 255, 255))
    d = ImageDraw.Draw(image)
    d.rounded_rectangle((12, 10, 12 + width, 10 + height), 12, fill=(48, 48, 48), outline=(20, 20, 20), width=2)
    y = 24
    for number, key, kind in ITEMS:
        if kind == "sep":
            d.line((24, y + 7, 12 + width - 12, y + 7), fill=(90, 90, 90), width=1)
            y += 14
            continue
        text = words[key][lang].replace("{count}", "0").replace("{class}", "Firefox")
        colour = (150, 150, 150) if kind == "info" else (240, 240, 240)
        x = 56
        d.text((x, y + row / 2), text, font=regular, fill=colour, anchor="lm")
        if kind == "check":
            d.text((30, y + row / 2), "✓", font=symbols, fill=(240, 240, 240), anchor="lm")
        if kind == "sub":
            d.text((12 + width - 26, y + row / 2), "›", font=bold, fill=(240, 240, 240), anchor="mm")
        if number is not None:
            cx, cy = 12 + width + 36, y + row / 2
            d.line((12 + width + 6, cy, cx - 15, cy), fill=RED, width=3)
            d.ellipse((cx - 14, cy - 14, cx + 14, cy + 14), fill="white")
            d.ellipse((cx - 12, cy - 12, cx + 12, cy + 12), fill=RED)
            d.text((cx, cy), str(number), font=bold, fill="white", anchor="mm")
        y += row
    image.save(out, optimize=True)


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for lang in ("ru", "en"):
        draw(lang, out / f"tray-menu-{lang}.png")
        print(out / f"tray-menu-{lang}.png")


if __name__ == "__main__":
    main()
