#!/usr/bin/env python3
"""Draws the PuLinTo icons and renders the tray pixmaps.

The mark is a keyboard in the manner of the system's `input-keyboard-symbolic` (an
outline, rows of keys) drawn from scratch, with one key picked out in the accent
colour: the space bar, because a space is what usually triggers the check of a
word. Change ACCENT below and run the script: it rewrites

    data/icons/ru.pulinto.Settings.svg      the application icon (menu, window)
    data/icons/pulinto-tray-on.svg          the tray icon, working
    data/icons/pulinto-tray-off.svg         the tray icon, off
    site/favicon.svg
    crates/pulintod/assets/tray-*.argb      the raw pixmaps pulintod embeds

    python3 -I tools/gen-icons.py [--preview out.png]

The pixmaps are committed, so a normal build does not need this script.
Rendering needs python3-gi with librsvg (Rsvg) and pycairo.
"""

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SIZES = (16, 22, 24, 32, 48)
OUT = ROOT / "crates/pulintod/assets"

ACCENT = "#f07a3c"      # the space bar when the program works
PLATE = ("#3a3d42", "#26282c")   # application icon plate, top and bottom
TRAY_PLATE = "#2b2d31"
GLYPH = "#f4f5f6"
OFF = {"plate": "#5b6065", "glyph": "#c4c8cc", "space": "#8d9399"}


def glyph(outline, keys, space):
    """The keyboard on a 64x64 field: an outline, three rows of keys, a space bar."""
    parts = [
        f'<rect x="7" y="15" width="50" height="34" rx="8" fill="none" stroke="{outline}" stroke-width="3.6"/>',
        f'<g fill="{keys}">',
    ]
    pitch, size = 8.2, 4.6
    for index in range(5):
        parts.append(f'<rect x="{13.3 + index * pitch:.1f}" y="21.5" width="{size}" height="{size}" rx="1.2"/>')
    for index in range(4):
        parts.append(f'<rect x="{17.4 + index * pitch:.1f}" y="29.7" width="{size}" height="{size}" rx="1.2"/>')
    parts.append(f'<rect x="13.3" y="37.9" width="{size}" height="{size}" rx="1.2"/>')
    parts.append(f'<rect x="46.1" y="37.9" width="{size}" height="{size}" rx="1.2"/>')
    parts.append("</g>")
    # The space bar is a little thicker than the keys: it carries the accent and must
    # survive at 16 pixels.
    parts.append(f'<rect x="21.5" y="37.2" width="21" height="6" rx="3" fill="{space}"/>')
    return "\n  ".join(parts)


def app_icon():
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 64 64">
  <defs><linearGradient id="plate" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{PLATE[0]}"/><stop offset="1" stop-color="{PLATE[1]}"/></linearGradient></defs>
  <rect width="64" height="64" rx="14" fill="url(#plate)"/>
  <rect x="0.5" y="0.5" width="63" height="63" rx="13.5" fill="none" stroke="#ffffff" stroke-opacity="0.12"/>
  {glyph(GLYPH, GLYPH, ACCENT)}
</svg>
'''


def tray_icon(on):
    if on:
        plate, outline, keys, space = TRAY_PLATE, GLYPH, GLYPH, ACCENT
        edge = "#ffffff"
    else:
        plate, outline, keys, space = OFF["plate"], OFF["glyph"], OFF["glyph"], OFF["space"]
        edge = "#ffffff"
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
  <rect width="64" height="64" rx="13" fill="{plate}"/>
  <rect x="1" y="1" width="62" height="62" rx="12" fill="none" stroke="{edge}" stroke-opacity="0.22" stroke-width="2"/>
  {glyph(outline, keys, space)}
</svg>
'''


def render_rgba(svg_text: str, size: int):
    """Straight-alpha RGBA bytes of the SVG at size x size, drawn by librsvg."""
    import cairo
    import gi

    gi.require_version("Rsvg", "2.0")
    from gi.repository import Rsvg

    handle = Rsvg.Handle.new_from_data(svg_text.encode("utf-8"))
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    context = cairo.Context(surface)
    context.scale(size / 64.0, size / 64.0)
    handle.render_document(context, _viewport(Rsvg))
    surface.flush()
    data = bytes(surface.get_data())
    out = bytearray(len(data))
    for i in range(0, len(data), 4):
        blue, green, red, alpha = data[i:i + 4]          # cairo: premultiplied, little endian
        if alpha:
            red, green, blue = (min(255, c * 255 // alpha) for c in (red, green, blue))
        out[i:i + 4] = bytes((red, green, blue, alpha))
    return bytes(out)


def _viewport(Rsvg):
    rect = Rsvg.Rectangle()
    rect.x, rect.y, rect.width, rect.height = 0, 0, 64, 64
    return rect


def render(svg_text: str, size: int) -> bytes:
    """The pixmap format of StatusNotifierItem: ARGB32, network byte order."""
    rgba = render_rgba(svg_text, size)
    argb = bytearray(len(rgba))
    for i in range(0, len(rgba), 4):
        r, g, b, a = rgba[i:i + 4]
        argb[i:i + 4] = bytes((a, r, g, b))
    return bytes(argb)


def main():
    icons = ROOT / "data/icons"
    (icons / "ru.pulinto.Settings.svg").write_text(app_icon(), encoding="utf-8")
    (icons / "pulinto-tray-on.svg").write_text(tray_icon(True), encoding="utf-8")
    (icons / "pulinto-tray-off.svg").write_text(tray_icon(False), encoding="utf-8")
    favicon = ROOT / "site/favicon.svg"
    if favicon.parent.exists():
        favicon.write_text(app_icon(), encoding="utf-8")
    OUT.mkdir(parents=True, exist_ok=True)
    for state in ("on", "off"):
        text = (icons / f"pulinto-tray-{state}.svg").read_text(encoding="utf-8")
        for size in SIZES:
            (OUT / f"tray-{state}-{size}.argb").write_bytes(render(text, size))
    print(f"wrote 3 icons, the favicon and {2 * len(SIZES)} pixmaps")


if __name__ == "__main__":
    main()
