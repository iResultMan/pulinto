"""Reads the few keys of a language pack (`data/languages/<id>/language.conf`) that
the build tools need. The format is described in docs/LANGUAGES.md; the program
itself validates a pack with `pulinto pack check`."""

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def read(lang_or_path):
    """`{"alphabet": ..., "fold": {from: to}, "id": ...}` of a pack, given its
    id (looked up under data/languages) or the path of its language.conf."""
    path = Path(lang_or_path)
    if not path.suffix:
        path = ROOT / "data" / "languages" / str(lang_or_path) / "language.conf"
    if path.is_dir():
        path = path / "language.conf"
    values = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, _, value = line.partition("=")
            values[key.strip()] = (values.get(key.strip(), "") + " " + value.strip()).strip()
    pack = {
        "id": values.get("id", ""),
        "dictionary": values.get("dictionary") or values.get("id", ""),
        "alphabet": values.get("alphabet", ""),
        "fold": dict(pair.split("=") for pair in values.get("fold", "").split()),
    }
    if not pack["id"] or not pack["alphabet"]:
        raise SystemExit(f"{path}: no `id` or `alphabet`; run `pulinto pack check` on it")
    return pack
