#!/bin/bash
# Повтор прогонов СТАРОЙ версии (8cda98d) с теми же словарями, что у новой (19 октября, по просьбе заказчика:
# условия сравнения должны быть одинаковыми). Первая цепочка report-chain.sh дала старой словари
# 16 октября (35cc2c4); ru.* и en.* с тех пор изменились. Здесь в OLD_ROOT/data/dict лежат текущие.
#
#   OLD_ROOT=<рабочая копия 8cda98d> CHAIN_PID=<pid report-chain.sh> tools/typing-fuzzer/rerun-old-dicts.sh
#
# Ждёт конца цепочки, откладывает прежние строки старой версии в results-old16dict.tsv и каталог
# old-16oct-dict/, затем гоняет старую заново на тех же нарезках, seed и режимах (свежий блок уже
# шёл на текущих словарях и не повторяется).
set -u
cd /home/medushenko/dev/pulinto || exit 1
WORDS=8000
OLD_ROOT=${OLD_ROOT:?}
DATE=2026-10-19
OUT=docs/research/report-$DATE
RES=$OUT/results.tsv
while kill -0 "${CHAIN_PID:?}" 2>/dev/null; do sleep 30; done
cmp -s data/dict/ru.fst "$OLD_ROOT/data/dict/ru.fst" && cmp -s data/dict/en.fst "$OLD_ROOT/data/dict/en.fst" || { echo "словари старой и новой различаются"; exit 1; }
OLD_BIN=$(sha256sum "$OLD_ROOT/target/release/pulintod" 2>/dev/null | cut -c1-12)
echo "повтор старой ($OLD_BIN) на текущих словарях, $(date +%H:%M)"
[ -d "$OUT/old-16oct-dict" ] || mv "$OUT/old" "$OUT/old-16oct-dict"
if [ ! -f "$OUT/results-old16dict.tsv" ]; then
    head -1 "$RES" > "$OUT/results-old16dict.tsv"
    awk -F'\t' 'NR>1 && $3=="old" && $2!="fresh"' "$RES" >> "$OUT/results-old16dict.tsv"
    awk -F'\t' 'NR==1 || !($3=="old" && $2!="fresh")' "$RES" > "$RES.tmp" && mv "$RES.tmp" "$RES"
fi
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
# run_block <группа> <метка> <версия new|old> <сценарий> <seed> <mac|plain> [перекрытие_мс, 30]
# Перекрытие 30 мс — ролловер быстрой руки (последняя буква слова отпускается после
# разделителя): старая версия на нём теряет буквы. 0 — «мягкий» набор, как в тестах
# до 16 октября.
run_block() {
    local group=$1 label=$2 ver=$3 script=$4 seed=$5 mode=$6 overlap=${7:-30}
    local root=. tag="rep-$label-$ver"
    [ "$ver" = old ] && root=$OLD_ROOT
    [ "$ver" = old ] && [ -z "$OLD_BIN" ] && { echo "пропуск $tag: старой версии нет"; return; }
    local cut="$OUT/$label.txt"
    [ -f "$cut" ] || head -n "$WORDS" "$script" > "$cut"   # одна и та же нарезка для new и old
    [ "$ver" = old ] && cp "$cut" "$OLD_ROOT/$cut" 2>/dev/null || { mkdir -p "$OLD_ROOT/$OUT"; cp "$cut" "$OLD_ROOT/$cut"; }
    local load; load=$(cut -d' ' -f1 /proc/loadavg)
    echo "=== $tag $(date +%H:%M) load $load"
    local extra=()
    if [ "$mode" = plain ]; then
        extra=(VARIANTS="us=,ru=" SOURCES="[('xkb', 'us'), ('xkb', 'ru')]")
    fi
    env PULINTO_ROOT="$(cd "$root" && pwd)" OVERLAP="$overlap" SHUFFLE=25 SEED="$seed" "${extra[@]}" \
        "$root/tools/typing-fuzzer/run-script.sh" "$tag" "$PWD/$cut" 100 6 virtual \
        > "$root/docs/research/stand-wayland/$tag.out" 2>&1
    kill_sandbox
    local o="$root/docs/research/stand-wayland/$tag.out"
    local ok missed false corrupt lost extra_screen
    ok=$(awk '$1=="ok"{print $2}' "$o" | tail -1); missed=$(awk '$1=="missed"{print $2}' "$o" | tail -1)
    false=$(awk '$1=="false"{print $2}' "$o" | tail -1); corrupt=$(awk '$1=="corrupt"{print $2}' "$o" | tail -1)
    lost=$(awk '$1=="lost"{print $2}' "$o" | tail -1); extra_screen=$(grep -o 'лишних слов на экране: [0-9]*' "$o" | tail -1 | grep -o '[0-9]*$')
    local d="$root/docs/research/stand-wayland/$tag" ev=""
    if [ -s "$d/daemon-in.log" ] && [ -s "$d/typist-keys.log" ]; then ev=$(( $(wc -l < "$d/typist-keys.log") - $(wc -l < "$d/daemon-in.log") )); fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$group" "$label" "$ver" "$(wc -l < "$cut")" \
        "${ok:-?}" "${missed:-0}" "${false:-0}" "${corrupt:-0}" "${lost:-0}" "${extra_screen:-0}" "$load" "${ev:-}" "$seed/$mode" >> "$RES"
    [ "$ver" = old ] && { mkdir -p "$OUT/old"; cp "$d/errors.tsv" "$OUT/old/$label-errors.tsv" 2>/dev/null; } || { mkdir -p "$OUT/new"; cp "$d/errors.tsv" "$OUT/new/$label-errors.tsv" 2>/dev/null; }
    tail -7 "$o" | sed 's/^/    /'
}
S600=docs/research/stand-wayland/scripts/script-600.txt
SC=docs/research/stand-wayland/scripts
WORDS=600
run_block стенд600 mac600 old "$S600" 301 mac
run_block стенд600 plain600 old "$S600" 301 plain
run_block стенд600-мягкий mac600g old "$S600" 301 mac 0
run_block стенд600-мягкий plain600g old "$S600" 301 plain 0
WORDS=8000
run_block веб-заморож frozen old "$SC/web-2026-10-18-1443.txt" 19018 mac
run_block веб-мягкий frozeng old "$SC/web-2026-10-18-1443.txt" 19018 mac 0
run_block веб-трудные hard old "$SC/web-2026-10-18-1443-hard.txt" 19018 mac
run_block IT-seed412 it old "$SC/genre-it.txt" 412 mac
echo "=== готово $(date +%H:%M)"
python3 -I tools/typing-fuzzer/make_report.py "$OUT" > "$OUT/report-draft.txt" 2>&1
