#!/usr/bin/env bash
# Prepares the GitHub Pages branch: builds the site and puts it into a separate local repository
# with one commit on `gh-pages`. Nothing is pushed; the command to publish is printed at the end.
#
#   tools/publish-site.sh [--out ~/dev/pulinto-site]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$HOME/dev/pulinto-site"
while [ $# -gt 0 ]; do
    case "$1" in
        --out) OUT="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done
case "$OUT" in "$ROOT"|"$ROOT"/*) echo "The target must be outside the working tree" >&2; exit 2 ;; esac

BUILD="$(mktemp -d)"
trap 'rm -rf "$BUILD"' EXIT
python3 -I "$ROOT/tools/build-site.py" --out "$BUILD/_site"

rm -rf "$OUT"
mkdir -p "$OUT"
cp -a "$BUILD/_site/." "$OUT/"
touch "$OUT/.nojekyll"
git -C "$OUT" init -q -b gh-pages
git -C "$OUT" add -A
git -C "$OUT" -c user.name=PuLinTo -c user.email=noreply@users.noreply.github.com \
    commit -q -m "Publish the site"
echo "Site ready in $OUT ($(git -C "$OUT" ls-files | wc -l) files, branch gh-pages, not pushed)."
echo "To publish: git -C $OUT push --force https://github.com/iResultMan/pulinto.git gh-pages"
