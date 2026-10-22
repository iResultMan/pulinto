#!/usr/bin/env bash
# PuLinTo: one-command installation.
#
#   curl -fsSL @SITE@/install.sh | bash
#
# The script downloads the ready archive of the latest release, checks its
# checksum and runs the install.sh from the archive. Parameters are passed on
# as they are:
#
#   curl -fsSL @SITE@/install.sh | bash -s -- --no-sudo
#
# The text of the script is at the same address: you can read it before running it.
set -euo pipefail

# Messages are written in English; with a Russian locale they are shown in Russian.
ru() {
    case "$1" in
        'PuLinTo has no ready builds for %s yet: building from source is described at %s') printf '%s' 'Готовых сборок для %s пока нет: установка из исходников описана на %s' ;;
        'PuLinTo works only on Linux') printf '%s' 'PuLinTo работает только в Linux' ;;
        'curl or wget is needed') printf '%s' 'Нужен curl или wget' ;;
        'Downloading PuLinTo (%s)...') printf '%s' 'Скачиваю PuLinTo (%s)...' ;;
        'Could not download %s') printf '%s' 'Не удалось скачать %s' ;;
        'Could not download the checksums') printf '%s' 'Не удалось скачать контрольные суммы' ;;
        'The checksum does not match: the installation is stopped') printf '%s' 'Контрольная сумма не совпала: установка остановлена' ;;
        'The archive has no install.sh') printf '%s' 'В архиве нет install.sh' ;;
        *) printf '%s' "$1" ;;
    esac
}
case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in ru*) LOCALE_RU=1 ;; *) LOCALE_RU=0 ;; esac
t() { if [ "$LOCALE_RU" = 1 ]; then ru "$1"; else printf '%s' "$1"; fi; }
tf() { local fmt; fmt="$(t "$1")"; shift; printf "$fmt" "$@"; }

REPO="@REPO@"

case "$(uname -m)" in
    x86_64) ARCH=x86_64 ;;
    *) tf 'PuLinTo has no ready builds for %s yet: building from source is described at %s\n' "$(uname -m)" "$REPO" >&2; exit 1 ;;
esac
[ "$(uname -s)" = Linux ] || { t 'PuLinTo works only on Linux' >&2; echo >&2; exit 1; }

if command -v curl >/dev/null; then
    fetch() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null; then
    fetch() { wget -q -O "$2" "$1"; }
else
    t 'curl or wget is needed' >&2; echo >&2; exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
BASE="$REPO/releases/latest/download"
FILE="pulinto-$ARCH.tar.gz"

tf 'Downloading PuLinTo (%s)...\n' "$FILE"
fetch "$BASE/$FILE" "$TMP/$FILE" || { tf 'Could not download %s\n' "$BASE/$FILE" >&2; exit 1; }
fetch "$BASE/SHA256SUMS" "$TMP/SHA256SUMS" || { t 'Could not download the checksums' >&2; echo >&2; exit 1; }
(cd "$TMP" && grep " $FILE\$" SHA256SUMS | sha256sum -c --status -) \
    || { t 'The checksum does not match: the installation is stopped' >&2; echo >&2; exit 1; }

tar -xzf "$TMP/$FILE" -C "$TMP"
DIR="$(find "$TMP" -maxdepth 1 -type d -name 'pulinto-*-linux-*' | head -1)"
[ -n "$DIR" ] && [ -x "$DIR/install.sh" ] || { t 'The archive has no install.sh' >&2; echo >&2; exit 1; }
cd "$DIR"
./install.sh "$@"
