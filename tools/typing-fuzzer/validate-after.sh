#!/bin/bash
# Проверка «после»: текущий target/release (слайс идентификаторов в en.tsv + правило акронимов в английском
# контексте) на тех же нарезках, seed и режимах, что «новая» версия в report-2026-10-19. Результаты —
# docs/research/report-2026-10-19-after/results.tsv; метки прогонов aft-*, старые каталоги не затираются.
set -u
cd /home/medushenko/dev/pulinto || exit 1
DATE=2026-10-19
BASE=docs/research/report-$DATE
OUT=$BASE-after
mkdir -p "$OUT"
RES=$OUT/results.tsv
[ -f "$RES" ] || printf 'группа\tметка\tверсия\tслов\tok\tmissed\tfalse\tcorrupt\tpunct\tlost\tлишних\tload\tсобытий_потеряно\tзаметка\n' > "$RES"
for f in mac600 plain600 frozen hard it fresh; do cp -n "$BASE/$f.txt" "$OUT/$f.txt"; done
echo "бинарник $(sha256sum target/release/pulintod | cut -c1-12), $(date +%H:%M)"
is_duty() { ! tr '\0' '\n' < "/proc/$1/environ" 2>/dev/null | grep -q '^PULINTO_GRAB_ONLY='; }
restore() {
    pgrep -x pulintod >/dev/null || DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus XDG_RUNTIME_DIR=/run/user/1000 \
        setsid -f sh -c 'exec ./target/release/pulintod 2>&1 | systemd-cat -t pulintod' < /dev/null
    sleep 4; echo "дежурный демон вернулся: $(pgrep -x pulintod) $(date +%H:%M)"
}
stop_duty() {
    for p in $(pgrep -x pulintod); do is_duty "$p" && kill -TERM "$p"; done
    for i in $(seq 1 40); do pgrep -x pulintod >/dev/null || break; sleep 0.1; done
}
kill_sandbox() { for p in $(pgrep -x pulintod); do is_duty "$p" || kill "$p"; done; sleep 1; }
trap restore EXIT
stop_duty
run_block() {
    local group=$1 label=$2 script=$3 seed=$4 mode=$5 overlap=${6:-30}
    local tag="aft-$label" cut="$OUT/$label.txt"
    local load; load=$(cut -d' ' -f1 /proc/loadavg)
    echo "=== $tag $(date +%H:%M) load $load"
    local extra=()
    if [ "$mode" = plain ]; then
        extra=(VARIANTS="us=,ru=" SOURCES="[('xkb', 'us'), ('xkb', 'ru')]")
    fi
    env PULINTO_ROOT="$PWD" OVERLAP="$overlap" SHUFFLE=25 SEED="$seed" "${extra[@]}" \
        tools/typing-fuzzer/run-script.sh "$tag" "$PWD/$cut" 100 6 virtual > "docs/research/stand-wayland/$tag.out" 2>&1
    kill_sandbox
    local o="docs/research/stand-wayland/$tag.out"
    local ok missed false corrupt punct lost extra_screen
    for k in ok missed false corrupt punct lost; do printf -v "$k" '%s' "$(awk -v k=$k '$1==k{print $2}' "$o" | tail -1)"; done
    extra_screen=$(grep -o 'лишних слов на экране: [0-9]*' "$o" | tail -1 | grep -o '[0-9]*$')
    local d="docs/research/stand-wayland/$tag" ev=""
    if [ -s "$d/daemon-in.log" ] && [ -s "$d/typist-keys.log" ]; then ev=$(( $(wc -l < "$d/typist-keys.log") - $(wc -l < "$d/daemon-in.log") )); fi
    printf '%s\t%s\tafter\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$group" "$label" "$(wc -l < "$cut")" \
        "${ok:-?}" "${missed:-0}" "${false:-0}" "${corrupt:-0}" "${punct:-0}" "${lost:-0}" "${extra_screen:-0}" "$load" "${ev:-}" "$seed/$mode" >> "$RES"
    mkdir -p "$OUT/errors"; cp "$d/errors.tsv" "$OUT/errors/$label-errors.tsv" 2>/dev/null
    tail -8 "$o" | sed 's/^/    /'
}
S600=docs/research/stand-wayland/scripts/script-600.txt
SC=docs/research/stand-wayland/scripts
run_block стенд600 mac600 "$S600" 301 mac
run_block стенд600 plain600 "$S600" 301 plain
run_block веб-свежий fresh "$SC/web-2026-10-19-0852.txt" 16012 mac
run_block IT-seed412 it "$SC/genre-it.txt" 412 mac
run_block веб-трудные hard "$SC/web-2026-10-18-1443-hard.txt" 19018 mac
run_block веб-заморож frozen "$SC/web-2026-10-18-1443.txt" 19018 mac
echo "=== готово $(date +%H:%M)"
