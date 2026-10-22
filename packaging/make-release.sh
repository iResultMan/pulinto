#!/usr/bin/env bash
# Builds the release files from ready binaries: an archive with install.sh,
# a .deb, and SHA256SUMS. The .rpm comes from make-rpm.sh.
#
#   packaging/make-release.sh --bin-dir target/release --out dist [--version 0.1.0] [--formats tar,deb]
#
# File names carry no version, so that
# <repository>/releases/latest/download/<name> is a stable link for the site.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR=""
OUT=""
FORMATS="tar,deb"
VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' "$ROOT/Cargo.toml" | head -1)"

while [ $# -gt 0 ]; do
    case "$1" in
        --bin-dir) BIN_DIR="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        --version) VERSION="$2"; shift 2 ;;
        --formats) FORMATS="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done
[ -n "$BIN_DIR" ] && [ -n "$OUT" ] || { echo "--bin-dir and --out are required" >&2; exit 2; }
for b in pulintod pulinto; do
    [ -x "$BIN_DIR/$b" ] || { echo "No $BIN_DIR/$b: run cargo build --release first" >&2; exit 1; }
done

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64) DEB_ARCH=amd64 ;;
    aarch64) DEB_ARCH=arm64 ;;
    *) echo "Architecture $ARCH is not supported" >&2; exit 1 ;;
esac

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# --- shared pieces ---------------------------------------------------------
# Data shipped with every package: ru and en only (it.* is not published).
copy_data() {  # $1 = destination data directory
    install -d "$1/dict" "$1/languages"
    for lang in ru us; do cp -a "$ROOT/data/languages/$lang" "$1/languages/$lang"; done
    for f in ru.fst en.fst; do install -m644 "$ROOT/data/dict/$f" "$1/dict/$f"; done
    # Interface translations: compiled here from po/*.po.
    python3 -I "$ROOT/tools/i18n/compile.py" "$1/locale" >/dev/null
}

# Same modes everywhere, whatever the umask of the machine that builds.
normalise_modes() {  # $1 = tree
    find "$1" -type d -exec chmod 755 {} +
    find "$1" -type f -perm /111 -exec chmod 755 {} +
    find "$1" -type f ! -perm /111 -exec chmod 644 {} +
}

has_format() { case ",$FORMATS," in *",$1,"*) return 0 ;; *) return 1 ;; esac; }

# --- archive ---------------------------------------------------------------
if has_format tar; then
NAME="pulinto-$VERSION-linux-$ARCH"
T="$WORK/$NAME"
install -d "$T/bin" "$T/gui" "$T/data" "$T/gnome-extension"
install -m755 "$BIN_DIR/pulintod" "$BIN_DIR/pulinto" "$T/bin/"
install -m755 "$ROOT/gui/pulinto-settings" "$T/gui/"
install -m755 "$ROOT/install.sh" "$ROOT/uninstall.sh" "$T/"
install -m644 "$ROOT/README.md" "$ROOT/README_EN.md" "$ROOT/NOTICE" "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/LICENSE-CC-BY-4.0" "$T/"
install -m644 "$ROOT/data/70-pulinto.rules" "$ROOT/data/ru.pulinto.Settings.desktop" "$ROOT/data/pulintod.service" "$T/data/"
install -d "$T/data/icons"
install -m644 "$ROOT/data/icons/ru.pulinto.Settings.svg" "$T/data/icons/"
copy_data "$T/data"
cp -a "$ROOT/gnome-extension/pulinto@pulinto.ru" "$T/gnome-extension/"
normalise_modes "$T"
tar -C "$WORK" --owner=0 --group=0 --sort=name -czf "$OUT/pulinto-$ARCH.tar.gz" "$NAME"
fi

# --- .deb ------------------------------------------------------------------
if has_format deb && command -v dpkg-deb >/dev/null; then
    D="$WORK/deb"
    install -d "$D/DEBIAN" "$D/usr/bin" "$D/usr/share/pulinto" "$D/usr/lib/udev/rules.d" \
        "$D/usr/share/applications" "$D/usr/share/icons/hicolor/scalable/apps" \
        "$D/usr/share/gnome-shell/extensions" "$D/usr/lib/systemd/user" "$D/usr/share/doc/pulinto"
    install -m755 "$BIN_DIR/pulintod" "$BIN_DIR/pulinto" "$D/usr/bin/"
    install -m755 "$ROOT/gui/pulinto-settings" "$D/usr/bin/"
    copy_data "$D/usr/share/pulinto"
    install -m644 "$ROOT/data/70-pulinto.rules" "$D/usr/lib/udev/rules.d/"
    install -m644 "$ROOT/data/ru.pulinto.Settings.desktop" "$D/usr/share/applications/"
    install -m644 "$ROOT/data/icons/ru.pulinto.Settings.svg" "$D/usr/share/icons/hicolor/scalable/apps/"
    cp -a "$ROOT/gnome-extension/pulinto@pulinto.ru" "$D/usr/share/gnome-shell/extensions/"
    sed 's#^ExecStart=.*#ExecStart=/usr/bin/pulintod#' "$ROOT/data/pulintod.service" > "$D/usr/lib/systemd/user/pulintod.service"
    chmod 644 "$D/usr/lib/systemd/user/pulintod.service"
    install -m644 "$ROOT/README.md" "$ROOT/README_EN.md" "$ROOT/NOTICE" "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/LICENSE-CC-BY-4.0" "$D/usr/share/doc/pulinto/"
    cat > "$D/usr/share/doc/pulinto/copyright" <<COPYRIGHT
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: PuLinTo
Source: https://github.com/iResultMan/pulinto

Files: *
Copyright: Yury ResultMan and PuLinTo contributors
License: MIT or Apache-2.0
 The code is licensed under either the MIT License (LICENSE-MIT) or the
 Apache License 2.0 (LICENSE-APACHE), at your option.

Files: usr/share/pulinto/*
Copyright: Yury ResultMan and PuLinTo contributors
License: CC-BY-4.0
 The language data (dictionaries, tables of forbidden letter combinations) is
 licensed under Creative Commons Attribution 4.0 (LICENSE-CC-BY-4.0). Sources
 and the required attribution: data/ATTRIBUTION.md in the source tree.
COPYRIGHT
    normalise_modes "$D"
    SIZE_KB="$(du -sk "$D" | cut -f1)"
    cat > "$D/DEBIAN/control" <<CONTROL
Package: pulinto
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: Yury ResultMan <i.result.man@gmail.com>
Installed-Size: $SIZE_KB
Depends: libdbus-1-3, libsystemd0, python3, python3-gi, gir1.2-gtk-4.0, gir1.2-adw-1
Recommends: gnome-shell-extension-appindicator
Section: utils
Priority: optional
Homepage: https://github.com/iResultMan/pulinto
Description: automatic keyboard layout switcher
 A word typed in the wrong layout (ghbdtn instead of привет) is erased and
 retyped correctly. Works on X11 and on GNOME with Wayland; Russian and
 English. Pre-release.
CONTROL
    install -m755 "$ROOT/packaging/deb/postinst" "$ROOT/packaging/deb/prerm" "$ROOT/packaging/deb/postrm" "$D/DEBIAN/"
    dpkg-deb --root-owner-group -Zxz --build "$D" "$OUT/pulinto-$ARCH.deb" >/dev/null
elif has_format deb; then
    echo "dpkg-deb not found: the .deb is not built" >&2
fi

(cd "$OUT" && sha256sum pulinto-* > SHA256SUMS)
echo "done: $OUT"
ls -la "$OUT"
