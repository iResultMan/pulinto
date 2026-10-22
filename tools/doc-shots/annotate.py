#!/usr/bin/env python3
"""Turns the raw screenshots of the settings window (tools/doc-shots/shoot.sh) into the
pictures of the manual: crops the window, hides what must not be published and puts
numbered markers with arrows on it. The numbers are explained in the text of the manual.

    python3 -I tools/doc-shots/annotate.py <raw dir> <out dir>

The coordinates are those of the 960x640 screenshot the stand makes.
"""

import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

BOLD = "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf"
RED = (214, 50, 28)
PAGE_BG = (250, 250, 250)
WINDOW = (4, 4, 936, 636)

# page -> list of (number, marker centre, point the arrow shows)
SPEC = {
    "overview": [
        (1, (125, 440), (125, 350)),
        (2, (330, 172), (468, 172)),
        (3, (268, 305), None),
        (4, (268, 430), None),
        (5, (798, 74), (798, 42)),
        (6, (268, 555), None),
    ],
    "replacements": [
        (1, (420, 27), (368, 27)),
        (2, (590, 460), (560, 352)),
        (3, (834, 450), (834, 388)),
        (4, (874, 450), (874, 388)),
        (5, (690, 27), (741, 27)),
    ],
    "never": [
        (1, (420, 27), (330, 27)),
        (2, (590, 360), (560, 250)),
        (3, (874, 330), (874, 262)),
    ],
    "always": [
        (1, (420, 27), (330, 27)),
        (2, (590, 360), (560, 200)),
        (3, (874, 330), (874, 200)),
    ],
    "apps": [
        (1, (262, 196), None),
        (2, (640, 242), (700, 205)),
        (3, (905, 240), (880, 205)),
        (4, (262, 360), None),
    ],
    "keyboard": [
        (1, (268, 155), None),
        (2, (905, 262), (864, 296)),
        (3, (905, 440), (866, 398)),
    ],
    "hotkeys": [
        (1, (268, 139), None),
        (2, (400, 232), (452, 232)),
        (3, (268, 363), None),
        (4, (690, 326), (765, 352)),
        (5, (860, 326), (860, 347)),
        (6, (700, 473), (480, 473)),
    ],
    "add-dialog": [
        (1, (212, 225), (257, 225)),
        (2, (212, 280), (257, 280)),
        (3, (212, 420), (257, 420)),
        (4, (740, 168), (697, 168)),
        (5, (190, 168), (243, 168)),
    ],
    "diagnostics": [
        (1, (268, 168), None),
        (2, (905, 196), (870, 196)),
        (3, (905, 444), (870, 444)),
        (4, (905, 498), (870, 498)),
    ],
    "help": [],
}

# Rectangles painted over with the page colour: the hotkeys page names the keyboards
# that are plugged into the machine of whoever takes the screenshot.
HIDE = {"hotkeys": [(287, 171, 892, 206)]}


def marker(draw, font, number, centre, target):
    cx, cy = centre
    tx, ty = target if target else centre
    dx, dy = tx - cx, ty - cy
    length = math.hypot(dx, dy) or 1
    ux, uy = dx / length, dy / length
    radius = 13
    start = (cx + ux * radius, cy + uy * radius)
    end = (tx - ux * 3, ty - uy * 3)
    if target and length > radius + 4:
        draw.line([start, end], fill="white", width=8)
        draw.line([start, end], fill=RED, width=4)
        head = 17
        left = (end[0] - ux * head + uy * head * 0.6, end[1] - uy * head - ux * head * 0.6)
        right = (end[0] - ux * head - uy * head * 0.6, end[1] - uy * head + ux * head * 0.6)
        draw.polygon([end, left, right], fill=RED, outline="white")
    draw.ellipse((cx - radius - 2, cy - radius - 2, cx + radius + 2, cy + radius + 2), fill="white")
    draw.ellipse((cx - radius, cy - radius, cx + radius, cy + radius), fill=RED)
    draw.text((cx, cy), str(number), font=font, fill="white", anchor="mm")


def process(raw, out, page):
    image = Image.open(raw).convert("RGB")
    draw = ImageDraw.Draw(image)
    for box in HIDE.get(page, []):
        draw.rectangle(box, fill=PAGE_BG)
    font = ImageFont.truetype(BOLD, 16)
    for number, centre, target in SPEC[page]:
        marker(draw, font, number, centre, target)
    image.crop(WINDOW).save(out, optimize=True)


def main():
    raw_dir, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)
    for lang, short in (("ru_RU", "ru"), ("en_US", "en")):
        for page in SPEC:
            raw = raw_dir / f"{lang}-{page}.png"
            if raw.exists():
                process(raw, out_dir / f"{page}-{short}.png", page)
                print(out_dir / f"{page}-{short}.png")


if __name__ == "__main__":
    main()
