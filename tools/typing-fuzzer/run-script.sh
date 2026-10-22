#!/bin/bash
# Прогон стенда по связному сценарию — работает и под Wayland, и под X11.
#
#   run-script.sh <тег> <сценарий> [пауза_мс] [выдержка_мс] [бэкенд окна]
#
# Машинистка печатает сценарий клавишами родной раскладки каждого слова и
# раскладку не трогает (стиль заказчика); демон в захвате; окно-приёмник
# сдаёт текст пачками. В конце — сверка compare_script.py.
#
# Бэкенд окна: wayland (по умолчанию), x11 — окно под Xwayland, virtual —
# модель композитора на libxkbcommon (virtual_screen.py), когда живой экран
# недоступен: вывод демона забирается в захват и до экрана не доходит.
#
# OVERLAP=<мс> в окружении: последняя буква слова отпускается через столько
# после нажатия разделителя (ролловер быстрой руки); без него стенд слеп к
# потере последней буквы при замене.
#
# Режим захвата ставится в ~/.config/pulinto/config.conf только на время
# прогона; прежний файл (если был) возвращается на место. Выученное демоном
# (списки «всегда/никогда менять») живёт в каталоге прогона: двойной Alt
# машинистки — это для демона просьба пользователя, и без изоляции стенд
# засорял настоящие списки (так и вышло 14 октября).
set -u
ROOT=${PULINTO_ROOT:-/home/medushenko/dev/pulinto}
TAG=$1
SCRIPT=$2
PAUSE=${3:-150}
HOLD=${4:-6}
BACKEND=${5:-wayland}
RUN=$ROOT/docs/research/stand-wayland/$TAG
CTL=$RUN/ctl
mkdir -p "$CTL"
rm -f "$CTL"/*
cd "$ROOT"

# Человек за машиной? Робот печатает в то окно, что в фокусе.
IDLE=$(gdbus call --session --dest org.gnome.Mutter.IdleMonitor \
    --object-path /org/gnome/Mutter/IdleMonitor/Core \
    --method org.gnome.Mutter.IdleMonitor.GetIdletime 2>/dev/null | grep -o '[0-9]\+' | tail -1)
# На виртуальном экране вывод до композитора не доходит, а захватывается только
# клавиатура машинистки — человеку прогон не мешает.
if [ "$BACKEND" != virtual ] && [ -n "$IDLE" ] && [ "$IDLE" -lt 60000 ] && [ "${FORCE:-0}" != 1 ]; then
    echo "ОТМЕНА: за машиной кто-то есть (простой ${IDLE} мс)"
    exit 1
fi

# Зомби (defunct) не в счёт: ждущий родителя мёртвый процесс ничего не держит.
# Дежурный демон — тот, у которого нет PULINTO_GRAB_ONLY; демон песочницы
# (стенд, харнессы) всегда запускается с этой переменной.
is_duty() { ! tr '\0' '\n' < "/proc/$1/environ" 2>/dev/null | grep -q '^PULINTO_GRAB_ONLY='; }
live_daemon() {
    for p in $(pgrep -x pulintod); do
        [ "$(ps -o stat= -p "$p" | cut -c1)" = Z ] && continue
        if is_duty "$p"; then return 0; fi
        # Осиротевший демон песочницы (прошлый прогон оборвался): снять.
        echo "снимаю осиротевший демон песочницы $p"; kill "$p"; sleep 1
    done
    return 1
}
if live_daemon; then
    echo "ОТМЕНА: уже работает pulintod"
    exit 1
fi

WORDS=$(wc -l < "$SCRIPT")
sha256sum target/release/pulintod > "$RUN/binary.sha256"
# Время с запасом: слово ~ 9 нажатий по 2×выдержки + пауза + замена.
RUNTIME=$(( WORDS * (PAUSE + 20 * HOLD + 500) / 1000 + WORDS / 200 + 120 ))
echo "прогон $TAG: слов $WORDS, пауза $PAUSE мс, выдержка $HOLD мс, окно $BACKEND, предел ${RUNTIME} с"

CONF=$HOME/.config/pulinto/config.conf
mkdir -p "$(dirname "$CONF")"
if [ -f "$CONF" ]; then cp "$CONF" "$RUN/config.conf.saved"; fi
restore_config() {
    if [ -f "$RUN/config.conf.saved" ]; then mv "$RUN/config.conf.saved" "$CONF"; else rm -f "$CONF"; fi
}
printf 'mode = grab\nauto_enabled = true\nautostart = false\n' > "$CONF"

# Экран не должен уснуть и запереться посреди прогона.
gnome-session-inhibit --inhibit idle:suspend --reason "PuLinTo stand" sleep $(( RUNTIME + 120 )) \
    >/dev/null 2>&1 &
INHIBIT=$!

LAYOUT_ARGS=()
SINK=
if [ "$BACKEND" = x11 ]; then
    GDK_BACKEND=x11 python3 tools/typing-fuzzer/sink_wl.py "$CTL" "PuLinTo sink" >"$RUN/sink.err" 2>&1 &
    SINK=$!
elif [ "$BACKEND" = wayland ]; then
    GDK_BACKEND=wayland python3 tools/typing-fuzzer/sink_wl.py "$CTL" "PuLinTo sink" >"$RUN/sink.err" 2>&1 &
    SINK=$!
else
    LAYOUT_ARGS=(--layout-file "$CTL/layout")
fi
if [ -n "$SINK" ]; then
    for _ in $(seq 1 50); do [ -f "$CTL/ready" ] && break; sleep 0.1; done
fi

# Варианты раскладок (`ru=mac,us=mac`) из настроек GNOME: демон читает их
# оттуда же (на виртуальном экране — из копии dconf), и машинистка с моделью
# экрана должны знать те же. Без этого на сессии с ru(mac) машинистка жмёт ё
# и точку не на тех клавишах, а модель рисует «<» там, где демон ждёт ё.
VARIANTS=${VARIANTS:-}
if [ -z "$VARIANTS" ]; then
    VARIANTS=$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null \
        | grep -o "'xkb', '[a-z]*+[a-z_-]*'" | sed "s/.*'\([a-z]*\)+\([a-z_-]*\)'/\1=\2/" | paste -sd, -)
fi
[ -n "$VARIANTS" ] && echo "варианты раскладок: $VARIANTS"
# То же в форме XKB для модели: по одному на раскладку из «us,ru».
XKB_VARIANTS=$(for id in us ru; do echo "$VARIANTS" | tr , '\n' | awk -F= -v id="$id" '$1==id{print $2}' | head -1; done | paste -sd, -)
PULINTO_TRACE_OUTPUT="$RUN/typist-keys.log" ./tools/typing-fuzzer/target/release/typist \
    --script "$SCRIPT" --ctl "$CTL" --log "$RUN/typist.jsonl" --variants "$VARIANTS" \
    --pause "$PAUSE" --hold "$HOLD" --wait 6 --batch 200 --shuffle "${SHUFFLE:-0}" --alt "${ALT:-0}" \
    --overlap "${OVERLAP:-0}" --seed "${SEED:-1}" "${LAYOUT_ARGS[@]}" \
    >"$RUN/typist.err" 2>&1 &
TYPIST=$!

sleep 1
if [ "$BACKEND" = virtual ]; then
    # Своя шина D-Bus: на виртуальном экране сессия обычно заперта, а запертый
    # экран демон честно считает «PuLinTo выключен». Трей и имя на шине
    # заодно не мешают настоящему демону. `exec` — чтобы $! был pid демона.
    # И свой dconf: копия настроек раскладок на момент старта. Иначе демон
    # слышит настоящий GNOME — человек за машиной переключил раскладку, и
    # демон стенда поверил, что переключилась модель.
    mkdir -p "$RUN/config/dconf" "$RUN/config/pulinto" "$RUN/data"
    cp "$HOME/.config/dconf/user" "$RUN/config/dconf/user"
    # Модель стартует с первой раскладки — с неё же стартует и демон, что бы
    # ни было включено у человека за машиной. Пишется только в копию.
    # SOURCES="[('xkb', 'us'), ('xkb', 'ru')]" подменяет раскладки в копии —
    # чтобы прогнать стенд на обычной ru при ru(mac) у человека (тогда и
    # VARIANTS="us=,ru=" для машинистки с моделью).
    XDG_CONFIG_HOME="$RUN/config" SOURCES="${SOURCES:-}" dbus-run-session -- bash -c \
        '[ -n "$SOURCES" ] && gsettings set org.gnome.desktop.input-sources sources "$SOURCES";
         gsettings set org.gnome.desktop.input-sources mru-sources "$(gsettings get org.gnome.desktop.input-sources sources)"' \
        >/dev/null 2>&1
    cp "$CONF" "$RUN/config/pulinto/config.conf"
    # Трассы событий: что демон прочитал с клавиатуры машинистки и что
    # напечатала его виртуальная клавиатура — по ним ищется потерянная буква.
    XDG_CONFIG_HOME="$RUN/config" XDG_DATA_HOME="$RUN/data" \
        PULINTO_NO_POINTER=1 PULINTO_TRACE_INPUT="$RUN/daemon-in.log" PULINTO_TRACE_OUTPUT="$RUN/daemon-out.log" \
        PULINTO_GRAB_ONLY="PuLinTo Test Typist" dbus-run-session -- \
        bash -c 'exec ./target/release/pulintod' >"$RUN/daemon.log" 2>&1 &
    sleep 0.5
    DAEMON=$(pgrep -n -x pulintod)
else
    XDG_DATA_HOME="$RUN/data" PULINTO_GRAB_ONLY="PuLinTo Test Typist" \
        ./target/release/pulintod >"$RUN/daemon.log" 2>&1 &
    DAEMON=$!
fi

if [ "$BACKEND" = virtual ]; then
    python3 tools/typing-fuzzer/virtual_screen.py "$CTL" "$DAEMON" us,ru grp:alt_shift_toggle "$XKB_VARIANTS" >"$RUN/sink.err" 2>&1 &
    SINK=$!
fi

# Сторож: захват клавиатуры не переживёт зависшего прогона.
( sleep "$RUNTIME"; kill $TYPIST 2>/dev/null; kill $DAEMON 2>/dev/null \
    && echo "СТОРОЖ: снял захват по таймеру" >>"$RUN/daemon.log" ) &
WD=$!

wait $TYPIST
TYPIST_EXIT=$?
sleep 1
kill $DAEMON 2>/dev/null
wait $DAEMON 2>/dev/null
kill $WD 2>/dev/null
: > "$CTL/quit"
sleep 0.5
kill $SINK 2>/dev/null
kill $INHIBIT 2>/dev/null
restore_config

echo "машинистка закончила с кодом $TYPIST_EXIT"
python3 tools/typing-fuzzer/compare_script.py "$RUN/typist.jsonl" "$CTL/batches.jsonl" \
    --errors "$RUN/errors.tsv" | tee "$RUN/compare.txt"

# Прогон записывает себя сам — в журнал кампании, независимо от того,
# следит ли кто-нибудь за ним.
HISTORY=$ROOT/docs/research/stand-wayland/history.tsv
[ -f "$HISTORY" ] || printf 'дата\tтег\tокно\tслов\tshuffle\tпауза\tвыдержка\tok\tmissed\tfalse\tcorrupt\tpunct\tlost\tлишних\tдемон\n' > "$HISTORY"
count() { awk -v k="$1" '$1 == k {print $2; exit}' "$RUN/compare.txt"; }
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$(date '+%F %T')" "$TAG" "$BACKEND" "$WORDS" "${SHUFFLE:-0}/alt${ALT:-0}${OVERLAP:+/ov$OVERLAP}" "$PAUSE" "$HOLD" \
    "$(count ok)" "$(count missed)" "$(count false)" "$(count corrupt)" \
    "$(count punct)" "$(count lost)" \
    "$(grep -o 'лишних слов на экране: [0-9]*' "$RUN/compare.txt" | grep -o '[0-9]*$')" \
    "$(cut -c1-12 "$RUN/binary.sha256")" >> "$HISTORY"
