#!/usr/bin/env bash
# Removes what install.sh put in place. User install by default; --system for /usr/local.
set -euo pipefail

# Messages are written in English; with a Russian locale they are shown in Russian.
ru() {
    case "$1" in
        'Unknown option: %s\n') printf '%s' 'Неизвестный параметр: %s\n' ;;
        '--system needs root (sudo); nothing was removed') printf '%s' 'Для --system нужны права root (sudo); ничего не удалено' ;;
        'Removing the udev rule (the administrator password is needed):') printf '%s' 'Удаляю правило udev (нужен пароль администратора):' ;;
        'The udev rule stays: %s still uses it.') printf '%s' 'Правило udev оставлено: им пользуется %s.' ;;
        'the udev rule was not removed; to remove it by hand:') printf '%s' 'правило udev не удалено; удалить вручную:' ;;
        'Removed (%s). Your data in %s and the settings are untouched.') printf '%s' 'Удалено (%s). Пользовательские данные в %s и настройки не тронуты.' ;;
        *) printf '%s' "$1" ;;
    esac
}
case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in ru*) LOCALE_RU=1 ;; *) LOCALE_RU=0 ;; esac
t() { if [ "$LOCALE_RU" = 1 ]; then ru "$1"; else printf '%s' "$1"; fi; }
tf() { local fmt; fmt="$(t "$1")"; shift; printf "$fmt" "$@"; }

MODE=user
for arg in "$@"; do
    case "$arg" in
        --system) MODE=system ;;
        -h|--help)
            if [ "$LOCALE_RU" = 1 ]; then
                cat <<'USAGE_RU'
Использование: ./uninstall.sh [--system]

Удаляет всё, что поставил install.sh. Пользовательские данные
(правила, replacements.tsv, отчёты в ~/.local/share/pulinto и настройки
в ~/.config/pulinto) не трогаются.
USAGE_RU
            else
                cat <<'USAGE'
Usage: ./uninstall.sh [--system]

Removes everything install.sh put in place. Your data (rules, replacements.tsv,
reports in ~/.local/share/pulinto and the settings in ~/.config/pulinto) is not touched.
USAGE
            fi
            exit 0
            ;;
        *) tf 'Unknown option: %s\n' "$arg" >&2; exit 2 ;;
    esac
done

say() { printf '%s\n' "$*"; }

# A gsettings string list without our extension: ['a', 'pulinto@pulinto.ru', 'b'] -> ['a', 'b'].
without_extension() {
    printf '%s' "$1" | sed -e "s/'pulinto@pulinto.ru'//" -e 's/\[, */[/' -e 's/, *\]/]/' -e 's/, *, */, /'
}

if [ "$MODE" = system ]; then
    BIN=/usr/local/bin
    DATA=/usr/local/share/pulinto
    EXT_DIR=/usr/share/gnome-shell/extensions
    APPS_DIR=/usr/local/share/applications
    ICON_DIR=/usr/local/share/icons/hicolor/scalable/apps
    UNIT_DIR=/etc/systemd/user
    EXTRA=(/etc/udev/rules.d/70-pulinto.rules)
    if [ "$(id -u)" -eq 0 ]; then SUDO=(); else SUDO=(sudo); fi
else
    DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
    CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
    BIN="$HOME/.local/bin"
    DATA="$DATA_HOME/pulinto"
    EXT_DIR="$DATA_HOME/gnome-shell/extensions"
    APPS_DIR="$DATA_HOME/applications"
    ICON_DIR="$DATA_HOME/icons/hicolor/scalable/apps"
    UNIT_DIR="$CONFIG_HOME/systemd/user"
    EXTRA=()
    # remove the autostart entry only if it is the one install.sh writes
    if grep -qxE "Exec=\"?$BIN/pulintod\"?" "$CONFIG_HOME/autostart/pulinto.desktop" 2>/dev/null; then
        EXTRA=("$CONFIG_HOME/autostart/pulinto.desktop")
    fi
    SUDO=()
    if command -v systemctl >/dev/null; then
        # failures are fine: the unit may not exist or the user bus may be absent
        systemctl --user disable --now pulintod 2>/dev/null || true
    fi
    # A daemon started another way (autostart entry, by hand) keeps the
    # keyboards grabbed after its binary is gone; stop it too.
    pkill -x -u "$(id -u)" pulintod 2>/dev/null || true
    # Take the extension off both GNOME lists. `gnome-extensions disable` would
    # put it on the disabled list, which the daemon of a later install respects
    # as the user's choice: the focus extension would then stay off for good.
    if command -v gsettings >/dev/null; then
        for key in enabled-extensions disabled-extensions; do
            list="$(gsettings get org.gnome.shell "$key" 2>/dev/null)" || continue
            case "$list" in *"'pulinto@pulinto.ru'"*) ;; *) continue ;; esac
            gsettings set org.gnome.shell "$key" "$(without_extension "$list")" 2>/dev/null || true
        done
    fi
    # The daemon of a later install turns the extension on again.
    rm -f "${XDG_STATE_HOME:-$HOME/.local/state}/pulinto/extensions-enabled"
fi

if [ "$MODE" = system ] && [ "${#SUDO[@]}" -gt 0 ]; then
    sudo -v || { t '--system needs root (sudo); nothing was removed' >&2; echo >&2; exit 1; }
fi

run() { "${SUDO[@]}" "$@"; }

run rm -f "$BIN/pulintod" "$BIN/pulinto" "$BIN/pulinto-settings"
# Only what install.sh put there; $DATA may also hold language packs and word
# lists of the user.
run rm -rf "$DATA/languages/ru" "$DATA/languages/us" "$DATA/dict/ru.fst" "$DATA/dict/en.fst" "$DATA/locale"
run rmdir "$DATA/languages" "$DATA/dict" 2>/dev/null || true
# Nothing else of ours is left in $DATA; drop it when it is empty.
run rmdir "$DATA" 2>/dev/null || true
run rm -rf "$EXT_DIR/pulinto@pulinto.ru"
run rm -f "$APPS_DIR/ru.pulinto.Settings.desktop" "$ICON_DIR/ru.pulinto.Settings.svg" "$UNIT_DIR/pulintod.service" "${EXTRA[@]}"

if [ "$MODE" = system ]; then
    run udevadm control --reload
    run udevadm trigger
else
    if command -v systemctl >/dev/null; then
        systemctl --user daemon-reload 2>/dev/null || true
    fi
    # The user install asks for sudo once to put the udev rule in /etc; take it away again, but only
    # if it is ours (it says PuLinTo) and nobody else still needs it: a package, a system-wide
    # install or another user's install.
    RULE=/etc/udev/rules.d/70-pulinto.rules
    if [ -e "$RULE" ] && grep -q PuLinTo "$RULE" 2>/dev/null; then
        STILL_USED=""
        for other in /usr/bin/pulintod /usr/local/bin/pulintod /usr/lib/udev/rules.d/70-pulinto.rules; do
            [ -e "$other" ] && STILL_USED="$other" && break
        done
        if [ -z "$STILL_USED" ]; then
            for other_home in $(getent passwd | cut -d: -f6); do
                other="$other_home/.local/bin/pulintod"
                [ -e "$other" ] && [ "$other" != "$BIN/pulintod" ] && STILL_USED="$other" && break
            done
        fi
        if [ -n "$STILL_USED" ]; then
            tf 'The udev rule stays: %s still uses it.' "$STILL_USED"; echo
        else
            say "$(t 'Removing the udev rule (the administrator password is needed):')"
            if [ "$(id -u)" -eq 0 ]; then ROOT_RUN=(); else ROOT_RUN=(sudo); fi
            if "${ROOT_RUN[@]}" rm -f "$RULE"; then
                # without a reload the old rights stay until the next boot
                "${ROOT_RUN[@]}" udevadm control --reload 2>/dev/null || true
                "${ROOT_RUN[@]}" udevadm trigger --subsystem-match=input --subsystem-match=misc 2>/dev/null || true
            else
                printf 'uninstall: %s\n' "$(t 'the udev rule was not removed; to remove it by hand:')" >&2
                echo "  sudo rm $RULE && sudo udevadm control --reload && sudo udevadm trigger" >&2
            fi
        fi
    fi
fi

say "$(tf 'Removed (%s). Your data in %s and the settings are untouched.' "$MODE" "$DATA")"
