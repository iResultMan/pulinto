#!/usr/bin/env bash
# Builds the .rpm from the archive made by make-release.sh. Needs rpmbuild.
#
#   packaging/make-rpm.sh --archive dist/pulinto-x86_64.tar.gz --out dist [--version 0.1.0]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCHIVE=""
OUT=""
VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' "$ROOT/Cargo.toml" | head -1)"
while [ $# -gt 0 ]; do
    case "$1" in
        --archive) ARCHIVE="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        --version) VERSION="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done
[ -f "$ARCHIVE" ] && [ -n "$OUT" ] || { echo "--archive (an existing file) and --out are required" >&2; exit 2; }
command -v rpmbuild >/dev/null || { echo "rpmbuild not found (package rpm-build)" >&2; exit 1; }

ARCH="$(uname -m)"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
TOP="$(mktemp -d)"
trap 'rm -rf "$TOP"' EXIT
mkdir -p "$TOP"/{SOURCES,BUILD,RPMS,SRPMS,SPECS}
cp "$ARCHIVE" "$TOP/SOURCES/pulinto-$VERSION-linux-$ARCH.tar.gz"
rpmbuild -bb "$ROOT/packaging/pulinto.spec" \
    --define "_topdir $TOP" \
    --define "pkg_version $VERSION" \
    --define "pkg_source pulinto-$VERSION-linux-$ARCH.tar.gz" >/dev/null
cp "$TOP"/RPMS/*/pulinto-*.rpm "$OUT/pulinto-$ARCH.rpm"
(cd "$OUT" && sha256sum pulinto-* | grep -v SHA256SUMS > SHA256SUMS.new && mv SHA256SUMS.new SHA256SUMS)
echo "done: $OUT/pulinto-$ARCH.rpm"
