#!/usr/bin/env bash
# PuLinTo installer. User install by default (no root); --system for /usr/local.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# A release archive carries ready binaries in bin/; a checkout keeps them in
# target/release after a build.
if [ -x "$ROOT/bin/pulintod" ] && [ -x "$ROOT/bin/pulinto" ]; then
    BIN_SRC="$ROOT/bin"
    PREBUILT=1
else
    BIN_SRC="$ROOT/target/release"
    PREBUILT=0
fi
# Messages are written in English; with a Russian locale they are shown in Russian
# (the table in `ru` below).
case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in ru*) LOCALE_RU=1 ;; *) LOCALE_RU=0 ;; esac
t() { if [ "$LOCALE_RU" = 1 ]; then ru "$1"; else printf '%s' "$1"; fi; }
tf() { local fmt; fmt="$(t "$1")"; shift; printf "$fmt" "$@"; }

ru() {
    case "$1" in
        'Unknown option: %s\n') printf '%s' 'Неизвестный параметр: %s\n' ;;
        'Warning:') printf '%s' 'Внимание:' ;;
        '--autostart-desktop works only in a user install') printf '%s' '--autostart-desktop работает только в пользовательской установке' ;;
        'an install as root puts the files in the home directory of root; use --system for the whole system') printf '%s' 'установка от root кладёт файлы в домашний каталог root; для всей системы используйте --system' ;;
        'The build is not run as root: first run cargo build --release --bin pulintod --bin pulinto as your user') printf '%s' 'Сборка от root не выполняется: сначала выполните cargo build --release --bin pulintod --bin pulinto от своего пользователя' ;;
        'cargo not found: install Rust (https://rustup.rs)') printf '%s' 'Не найден cargo: установите Rust (https://rustup.rs)' ;;
        'Building pulintod and pulinto (cargo build --release)...') printf '%s' 'Собираю pulintod и pulinto (cargo build --release)...' ;;
        '--system needs root (sudo); nothing was installed') printf '%s' 'Для --system нужны права root (sudo); установка не начата' ;;
        'Installing the files (%s)...') printf '%s' 'Устанавливаю файлы (%s)...' ;;
        'there are no *.fst dictionaries in data/dict: the dictionary stage of the cascade will not work') printf '%s' 'в data/dict нет словарей *.fst: словарная ступень каскада работать не будет' ;;
        'autostart is not enabled (--no-enable): the file %s/pulinto.desktop is not written') printf '%s' 'автозапуск не включён (--no-enable): файл %s/pulinto.desktop не записан' ;;
        '%s/pulinto.desktop already exists: with the systemd unit the daemon would start twice. Delete the file or install with --autostart-desktop') printf '%s' '%s/pulinto.desktop уже есть: вместе с systemd-юнитом демон запустится дважды. Удалите файл или установите с --autostart-desktop' ;;
        'The administrator password is needed once: a udev rule so that the daemon can read the keyboard.') printf '%s' 'Нужен пароль администратора, один раз: правило udev, чтобы демон мог читать клавиатуру.' ;;
        'the udev rule is not installed; the command to install it by hand is at the end') printf '%s' 'правило udev не установлено; команда для ручной установки — в конце' ;;
        'Installing the AppIndicator extension (the tray icon):') printf '%s' 'Ставлю расширение AppIndicator (значок в трее):' ;;
        'the AppIndicator extension is not installed: there will be no tray icon, everything else works') printf '%s' 'расширение AppIndicator не установлено: значка в трее не будет, остальное работает' ;;
        'the user systemd is not available; enable by hand: systemctl --user enable --now pulintod, or install with --autostart-desktop') printf '%s' 'systemd пользователя недоступен; включите вручную: systemctl --user enable --now pulintod или установите с --autostart-desktop' ;;
        'Done:') printf '%s' 'Сделано:' ;;
        'programs:') printf '%s' 'программы:' ;;
        'data:') printf '%s' 'данные:' ;;
        'dictionary files: %s') printf '%s' 'файлов словарей: %s' ;;
        'GNOME extension:') printf '%s' 'расширение GNOME:' ;;
        'launcher:') printf '%s' 'ярлык редактора:' ;;
        'icon:') printf '%s' 'значок:' ;;
        'systemd unit:') printf '%s' 'юнит systemd:' ;;
        'autostart:') printf '%s' 'автозапуск:' ;;
        'no unit is installed') printf '%s' 'юнит не ставится' ;;
        'the daemon is enabled and running:') printf '%s' 'демон включён и запущен:' ;;
        'udev rule:') printf '%s' 'правило udev:' ;;
        'What is left for you to do:') printf '%s' 'Что осталось сделать вам:' ;;
        'access to /dev/uinput and /dev/input (once, needs sudo):') printf '%s' 'доступ к /dev/uinput и /dev/input (один раз, нужен sudo):' ;;
        'Corrections already work. The per-application rules and the tray icon appear after the next login.') printf '%s' 'Исправление слов уже работает. Правила по приложениям и значок в трее появятся после следующего входа в сеанс.' ;;
        'each user:') printf '%s' 'каждому пользователю:' ;;
        'nothing') printf '%s' 'ничего' ;;
        'Hint: %s is not in PATH. The menu launcher and the tray icon do not depend on it; to run pulinto and pulinto-settings from a terminal add the directory to PATH.') printf '%s' 'Подсказка: %s нет в PATH. Значок в меню и в трее от этого не зависят; чтобы запускать pulinto и pulinto-settings из терминала, добавьте каталог в PATH.' ;;
        *) printf '%s' "$1" ;;
    esac
}

MODE=user
AUTOSTART_DESKTOP=0
ENABLE=1
BUILD=0
NO_SUDO=0

usage() {
    if [ "$LOCALE_RU" = 1 ]; then
        cat <<'USAGE_RU'
Использование: ./install.sh [параметры]

  (без параметров)     установка для текущего пользователя, без root:
                       ~/.local/bin, ~/.local/share/pulinto, systemd-юнит пользователя
  --autostart-desktop  вместо systemd-юнита положить ~/.config/autostart/pulinto.desktop
  --build              пересобрать pulintod и pulinto, даже если бинарники уже есть
                       (после git pull; в готовом архиве не нужен)
  --no-enable          только скопировать файлы, ничего не включать и не запускать
  --no-sudo            не просить пароль администратора (правило udev поставите сами)
  --system             установка для всех через sudo: /usr/local/bin, /usr/local/share/pulinto,
                       /usr/share/gnome-shell/extensions, /etc/udev/rules.d;
                       включать демон каждый пользователь будет сам
  -h, --help           эта справка

Скрипт можно запускать повторно.
USAGE_RU
    else
        cat <<'USAGE'
Usage: ./install.sh [options]

  (no options)         install for the current user, without root:
                       ~/.local/bin, ~/.local/share/pulinto, a user systemd unit
  --autostart-desktop  put ~/.config/autostart/pulinto.desktop instead of the systemd unit
  --build              rebuild pulintod and pulinto even if the binaries exist
                       (after git pull; not needed in a release archive)
  --no-enable          only copy the files, enable and start nothing
  --no-sudo            do not ask for the administrator password (you install the udev rule yourself)
  --system             install for everybody through sudo: /usr/local/bin, /usr/local/share/pulinto,
                       /usr/share/gnome-shell/extensions, /etc/udev/rules.d;
                       each user enables the daemon on their own
  -h, --help           this help

The script can be run again.
USAGE
    fi
}

for arg in "$@"; do
    case "$arg" in
        --system) MODE=system ;;
        --autostart-desktop) AUTOSTART_DESKTOP=1 ;;
        --build) BUILD=1 ;;
        --no-enable) ENABLE=0 ;;
        --no-sudo) NO_SUDO=1 ;;
        -h|--help) usage; exit 0 ;;
        *) tf 'Unknown option: %s\n' "$arg" >&2; usage >&2; exit 2 ;;
    esac
done

say() { printf '%s\n' "$*"; }
warn() { printf '%s %s\n' "$(t 'Warning:')" "$*" >&2; }

if [ "$MODE" = system ]; then
    if [ "$AUTOSTART_DESKTOP" = 1 ]; then
        t '--autostart-desktop works only in a user install' >&2; echo >&2
        exit 2
    fi
    BIN=/usr/local/bin
    DATA=/usr/local/share/pulinto
    EXT_DIR=/usr/share/gnome-shell/extensions
    APPS_DIR=/usr/local/share/applications
    ICON_DIR=/usr/local/share/icons/hicolor/scalable/apps
    UNIT_DIR=/etc/systemd/user
    UDEV_DIR=/etc/udev/rules.d
    if [ "$(id -u)" -eq 0 ]; then SUDO=(); else SUDO=(sudo); fi
else
    if [ "$(id -u)" -eq 0 ]; then
        warn "$(t 'an install as root puts the files in the home directory of root; use --system for the whole system')"
    fi
    DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
    CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
    BIN="$HOME/.local/bin"
    DATA="$DATA_HOME/pulinto"
    EXT_DIR="$DATA_HOME/gnome-shell/extensions"
    APPS_DIR="$DATA_HOME/applications"
    ICON_DIR="$DATA_HOME/icons/hicolor/scalable/apps"
    UNIT_DIR="$CONFIG_HOME/systemd/user"
    AUTOSTART_DIR="$CONFIG_HOME/autostart"
    SUDO=()
fi

# Run a command as root in --system mode and as the current user otherwise.
run() { "${SUDO[@]}" "$@"; }

# Binaries are built as the invoking user, never as root.
if [ "$PREBUILT" = 0 ] && { [ "$BUILD" = 1 ] || [ ! -x "$BIN_SRC/pulintod" ] || [ ! -x "$BIN_SRC/pulinto" ]; }; then
    if [ "$(id -u)" -eq 0 ]; then
        t 'The build is not run as root: first run cargo build --release --bin pulintod --bin pulinto as your user' >&2; echo >&2
        exit 1
    fi
    command -v cargo >/dev/null || { t 'cargo not found: install Rust (https://rustup.rs)' >&2; echo >&2; exit 1; }
    say "$(t 'Building pulintod and pulinto (cargo build --release)...')"
    (cd "$ROOT" && cargo build --release --bin pulintod --bin pulinto)
fi

if [ "$MODE" = system ] && [ "${#SUDO[@]}" -gt 0 ]; then
    sudo -v || { t '--system needs root (sudo); nothing was installed' >&2; echo >&2; exit 1; }
fi

say "$(tf 'Installing the files (%s)...' "$MODE")"
run install -d "$BIN" "$DATA" "$EXT_DIR" "$APPS_DIR" "$ICON_DIR" "$UNIT_DIR"
run install -m755 "$BIN_SRC/pulintod" "$BIN/pulintod"
run install -m755 "$BIN_SRC/pulinto" "$BIN/pulinto"
run install -m755 "$ROOT/gui/pulinto-settings" "$BIN/pulinto-settings"

# Language packs: replace the ones we ship (to drop stale files) and leave any
# pack the user put here: a folder with a language.conf is picked up as it is.
[ -L "$DATA/languages" ] && run rm -f "$DATA/languages"
run install -d "$DATA/languages"
for pack in "$ROOT"/data/languages/*/; do
    [ -f "$pack/language.conf" ] || continue
    run rm -rf "$DATA/languages/$(basename "$pack")"
    run cp -a "${pack%/}" "$DATA/languages/$(basename "$pack")"
done

# Interface translations for the settings window: compiled catalogues come
# with the program.
run rm -rf "$DATA/locale"
if [ -d "$ROOT/data/locale" ]; then
    run cp -a "$ROOT/data/locale" "$DATA/locale"
fi

# The daemon reads only the .fst files; the .tsv sources stay in the checkout.
# A symlink to the checkout is replaced by a real directory; word lists the user
# added for a pack of their own stay.
[ -L "$DATA/dict" ] && run rm -f "$DATA/dict"
run install -d "$DATA/dict"
dict_count=0
for f in "$ROOT"/data/dict/*.fst; do
    [ -e "$f" ] || continue
    run install -m644 "$f" "$DATA/dict/$(basename "$f")"
    dict_count=$((dict_count + 1))
done
if ! compgen -G "$ROOT/data/dict/*.fst" >/dev/null; then
    warn "$(t 'there are no *.fst dictionaries in data/dict: the dictionary stage of the cascade will not work')"
fi

EXT_UUID=pulinto@pulinto.ru
run rm -rf "$EXT_DIR/$EXT_UUID"
run cp -a "$ROOT/gnome-extension/$EXT_UUID" "$EXT_DIR/$EXT_UUID"

# Exec paths are absolute and quoted: ~/.local/bin is not on PATH until the
# next login, and a home directory may contain spaces.
desktop_tmp="$(mktemp)"
awk -v exe="\"$BIN/pulinto-settings\"" '/^Exec=/{print "Exec=" exe; next} {print}' \
    "$ROOT/data/ru.pulinto.Settings.desktop" > "$desktop_tmp"
run install -m644 "$desktop_tmp" "$APPS_DIR/ru.pulinto.Settings.desktop"
rm -f "$desktop_tmp"
# The launcher icon is named after the application id, so the shell finds it
# for the menu entry and for the window alike.
run install -m644 "$ROOT/data/icons/ru.pulinto.Settings.svg" "$ICON_DIR/ru.pulinto.Settings.svg"
if [ "$MODE" = system ] && command -v gtk-update-icon-cache >/dev/null; then
    run gtk-update-icon-cache -q -t -f /usr/local/share/icons/hicolor || true
fi

# The unit template carries a placeholder path; point it at what we installed.
# With --autostart-desktop the desktop entry is the way to start the daemon,
# and a unit beside it would start a second copy once somebody enables it.
if [ "$AUTOSTART_DESKTOP" = 0 ]; then
    unit_tmp="$(mktemp)"
    trap 'rm -f "$unit_tmp"' EXIT
    awk -v exe="\"$BIN/pulintod\"" '/^ExecStart=/{print "ExecStart=" exe; next} {print}' \
        "$ROOT/data/pulintod.service" > "$unit_tmp"
    run install -m644 "$unit_tmp" "$UNIT_DIR/pulintod.service"
else
    run rm -f "$UNIT_DIR/pulintod.service"
fi

if [ "$MODE" = system ]; then
    run install -m644 "$ROOT/data/70-pulinto.rules" "$UDEV_DIR/70-pulinto.rules"
    run udevadm control --reload
    run udevadm trigger
fi

if [ "$AUTOSTART_DESKTOP" = 1 ] && [ "$ENABLE" = 0 ]; then
    say "$(tf 'autostart is not enabled (--no-enable): the file %s/pulinto.desktop is not written' "$AUTOSTART_DIR")"
elif [ "$AUTOSTART_DESKTOP" = 1 ]; then
    mkdir -p "$AUTOSTART_DIR"
    cat > "$AUTOSTART_DIR/pulinto.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=PuLinTo
Comment=Smart keyboard layout switcher
Exec="$BIN/pulintod"
Icon=input-keyboard
Terminal=false
Categories=Utility;
NoDisplay=true
X-GNOME-Autostart-enabled=true
DESKTOP
elif [ "$MODE" = user ] && [ -e "$AUTOSTART_DIR/pulinto.desktop" ]; then
    warn "$(tf '%s/pulinto.desktop already exists: with the systemd unit the daemon would start twice. Delete the file or install with --autostart-desktop' "$AUTOSTART_DIR")"
fi

# Access to the keyboard needs a udev rule, and a rule is a system file: ask
# for the administrator password once, here, instead of leaving it as a chore.
udev_done=0
if [ "$MODE" = user ] && [ "$ENABLE" = 1 ] && [ "$NO_SUDO" = 0 ] \
    && [ ! -e /etc/udev/rules.d/70-pulinto.rules ] && [ ! -e /usr/lib/udev/rules.d/70-pulinto.rules ]; then
    say "$(t 'The administrator password is needed once: a udev rule so that the daemon can read the keyboard.')"
    if sudo install -m644 "$ROOT/data/70-pulinto.rules" /etc/udev/rules.d/70-pulinto.rules \
        && sudo udevadm control --reload && sudo udevadm trigger; then
        udev_done=1
    else
        warn "$(t 'the udev rule is not installed; the command to install it by hand is at the end')"
    fi
fi

# The tray icon in a plain GNOME comes from the AppIndicator extension, which
# Ubuntu ships and Fedora, RHEL and others do not. Without it the program works
# but shows no icon, so it is installed with the same administrator password.
appindicator_present() {
    for uuid in appindicatorsupport@rgcjonas.gmail.com ubuntu-appindicators@ubuntu.com; do
        [ -d "/usr/share/gnome-shell/extensions/$uuid" ] && return 0
    done
    return 1
}
if [ "$MODE" = user ] && [ "$ENABLE" = 1 ] && [ "$NO_SUDO" = 0 ] \
    && case "${XDG_CURRENT_DESKTOP:-}" in *GNOME*) true ;; *) false ;; esac \
    && ! appindicator_present; then
    if command -v dnf >/dev/null; then
        say "$(t 'Installing the AppIndicator extension (the tray icon):') sudo dnf install gnome-shell-extension-appindicator"
        sudo dnf install -y gnome-shell-extension-appindicator || warn "$(t 'the AppIndicator extension is not installed: there will be no tray icon, everything else works')"
    elif command -v apt-get >/dev/null; then
        say "$(t 'Installing the AppIndicator extension (the tray icon):') sudo apt-get install gnome-shell-extension-appindicator"
        sudo apt-get install -y gnome-shell-extension-appindicator || warn "$(t 'the AppIndicator extension is not installed: there will be no tray icon, everything else works')"
    fi
fi

# The extensions are turned on by the daemon at its first start for each user
# (`extensions.rs`): one place for it, also for the .deb and .rpm packages.

enabled=0
if [ "$MODE" = user ] && [ "$AUTOSTART_DESKTOP" = 0 ] && [ "$ENABLE" = 1 ]; then
    if command -v systemctl >/dev/null && systemctl --user daemon-reload 2>/dev/null; then
        # restart so that a re-install picks up the new binary
        systemctl --user enable pulintod
        systemctl --user restart pulintod
        enabled=1
    else
        warn "$(t 'the user systemd is not available; enable by hand: systemctl --user enable --now pulintod, or install with --autostart-desktop')"
    fi
fi

say
say "$(t 'Done:')"
say "  $(t 'programs:')        $BIN/{pulintod,pulinto,pulinto-settings}"
say "  $(t 'data:')            $DATA/{languages,dict} ($(tf 'dictionary files: %s' "$dict_count"))"
say "  $(t 'GNOME extension:') $EXT_DIR/$EXT_UUID"
say "  $(t 'launcher:')        $APPS_DIR/ru.pulinto.Settings.desktop ($(t 'icon:') $ICON_DIR/ru.pulinto.Settings.svg)"
[ "$AUTOSTART_DESKTOP" = 0 ] && say "  $(t 'systemd unit:')    $UNIT_DIR/pulintod.service"
[ "$AUTOSTART_DESKTOP" = 1 ] && [ "$ENABLE" = 1 ] && say "  $(t 'autostart:')       $AUTOSTART_DIR/pulinto.desktop ($(t 'no unit is installed'))"
[ "$enabled" = 1 ] && say "  $(t 'the daemon is enabled and running:') systemctl --user status pulintod"
[ "$MODE" = system ] && say "  $(t 'udev rule:')       $UDEV_DIR/70-pulinto.rules"

say
say "$(t 'What is left for you to do:')"
todo=0
if [ "$MODE" = user ] && [ ! -e /etc/udev/rules.d/70-pulinto.rules ] \
    && [ ! -e /usr/lib/udev/rules.d/70-pulinto.rules ]; then
    todo=1
    say "  - $(t 'access to /dev/uinput and /dev/input (once, needs sudo):')"
    say "      sudo install -m644 \"$ROOT/data/70-pulinto.rules\" /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger"
fi
# Corrections work from the first key. Only the extension (per-application rules
# under Wayland) and the tray icon wait for GNOME to notice a new extension.
gnome_hint=""
case "${XDG_CURRENT_DESKTOP:-}" in
    *GNOME*)
        gnome_hint="$(t 'Corrections already work. The per-application rules and the tray icon appear after the next login.')"
        ;;
esac
if [ "$MODE" = system ]; then
    todo=1
    say "  - $(t 'each user:') systemctl --user enable --now pulintod"
fi
[ "$todo" = 0 ] && say "  - $(t 'nothing')"
[ -n "$gnome_hint" ] && { say; say "$gnome_hint"; }
case ":$PATH:" in
    *":$BIN:"*) ;;
    *)
        say
        say "$(tf 'Hint: %s is not in PATH. The menu launcher and the tray icon do not depend on it; to run pulinto and pulinto-settings from a terminal add the directory to PATH.' "$BIN")"
        ;;
esac
exit 0
