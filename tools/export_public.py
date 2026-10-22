#!/usr/bin/env python3
"""Builds the tree that is published on GitHub from the working tree: the program, the
dictionaries, the user documentation and nothing else. The list of what goes out is an
allowlist, so a new file in the working tree stays private until it is added here.

    python3 -I tools/export_public.py [--out ~/dev/pulinto-public] [--build]

The target is emptied and refilled (never touched by hand); `--build` also compiles and
tests the exported tree, which proves it does not need anything that stayed behind.
The unit tests (`#[cfg(test)]` modules) are stripped from the exported sources.
Nothing is committed or pushed.
"""

import argparse
import fnmatch
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

ALLOW = [
    "Cargo.toml", "Cargo.lock", "README.md", "README_EN.md", "CONTRIBUTING.md", "SECURITY.md",
    "NOTICE", "LICENSE-MIT", "LICENSE-APACHE", "LICENSE-CC-BY-4.0", "install.sh", "uninstall.sh",
    "crates/*/Cargo.toml", "crates/*/src/*", "crates/pulintod/assets/*",
    "gui/pulinto-settings", "gnome-extension/*",
    "data/70-pulinto.rules", "data/ATTRIBUTION.md", "data/pulintod.service",
    "data/ru.pulinto.Settings.desktop", "data/dict/ru.*", "data/dict/en.*", "data/icons/*",
    "data/languages/ru/*", "data/languages/us/*", "data/locale/ru/*", "data/locale/it/*",
    "data/raw/code-*.tsv",
    "po/*.po", "po/pulinto.pot",
    "docs/FAQ.md", "docs/LANGUAGES.md", "docs/LANGUAGE_PACK.md", "docs/MANUAL.md",
    "docs/MEASUREMENTS.md", "docs/TESTING.md", "docs/img/*",
    ".github/ISSUE_TEMPLATE/*", ".github/pull_request_template.md",
]
SKIP_PARTS = {"__pycache__", "target", "tests", "examples", ".git"}
PUBLIC_GITIGNORE = "/target\n**/*.rs.bk\n__pycache__/\n"

# Text that must not appear in what is published.
LEAKS = [r"medushenko", r"/home/", r"docs/research", r"docs/promo", r"tools/", r"backup/",
         r"private/", r"support/", r"\.claude", r"backlog\.md", r"BOT_TOKEN", r"SUPPORT_CHAT"]


def strip_unit_tests(text):
    """Drops the top-level `#[cfg(test)] mod ... { ... }` blocks (they close with a `}` in column 0)."""
    lines = text.split("\n")
    kept = []
    i = 0
    while i < len(lines):
        if lines[i] == "#[cfg(test)]" and i + 1 < len(lines) and re.match(r"mod \w+ \{$", lines[i + 1]):
            i += 2
            while lines[i] != "}":
                i += 1
            i += 1
            while kept and kept[-1] == "" and (i >= len(lines) or lines[i] == ""):
                kept.pop()
            continue
        kept.append(lines[i])
        i += 1
    return "\n".join(kept)


def wanted(path):
    rel = path.relative_to(ROOT).as_posix()
    if any(part in SKIP_PARTS for part in path.relative_to(ROOT).parts):
        return False
    return any(fnmatch.fnmatchcase(rel, pattern) for pattern in ALLOW)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=Path.home() / "dev" / "pulinto-public")
    parser.add_argument("--build", action="store_true", help="compile and test the exported tree")
    args = parser.parse_args()
    out = args.out.expanduser().resolve()
    if out == ROOT or ROOT in out.parents:
        sys.exit("the target must be outside the working tree")

    # Compiled interface catalogues are part of what is published.
    subprocess.run([sys.executable, "-I", str(ROOT / "tools/i18n/compile.py")], check=True, stdout=subprocess.DEVNULL)

    if out.exists():
        for child in out.iterdir():
            if child.name == ".git":
                continue
            shutil.rmtree(child) if child.is_dir() else child.unlink()
    out.mkdir(parents=True, exist_ok=True)

    files = sorted(p for p in ROOT.rglob("*") if p.is_file() and wanted(p))
    for path in files:
        target = out / path.relative_to(ROOT)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        if path.suffix == ".rs":
            stripped = strip_unit_tests(path.read_text(encoding="utf-8"))
            if "cfg(test)" in stripped:
                sys.exit(f"{path}: a cfg(test) item is left that the exporter does not know how to strip")
            target.write_text(stripped, encoding="utf-8")
    (out / ".gitignore").write_text(PUBLIC_GITIGNORE, encoding="utf-8")

    # Leak scan over the text files that went out.
    pattern = re.compile("|".join(LEAKS))
    leaks = []
    for path in sorted(out.rglob("*")):
        if not path.is_file() or ".git" in path.parts or path.suffix in {".png", ".webp", ".fst", ".mo", ".argb", ".svg"}:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if pattern.search(line):
                leaks.append(f"{path.relative_to(out)}:{number}: {line.strip()[:110]}")
    total = sum(p.stat().st_size for p in out.rglob("*") if p.is_file() and ".git" not in p.parts)
    print(f"{len(files) + 1} files, {total / 1e6:.1f} MB -> {out}")
    if leaks:
        print(f"\n{len(leaks)} line(s) mention something private:")
        print("\n".join(leaks[:60]))
    if args.build:
        subprocess.run(["cargo", "build", "--release", "--workspace"], cwd=out, check=True)
        # The published sources carry no tests; this proves they were removed cleanly.
        subprocess.run(["cargo", "test", "--release", "--workspace"], cwd=out, check=True)
    return 1 if leaks else 0


if __name__ == "__main__":
    sys.exit(main())
