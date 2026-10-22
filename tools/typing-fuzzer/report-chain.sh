#!/bin/bash
# Цепочка «старт против сейчас» (19 октября 2026, по просьбе заказчика).
#
#   OLD_ROOT=<рабочая копия коммита 8cda98d со своей сборкой> \
#     tools/typing-fuzzer/report-chain.sh [слов в блоке, 8000]
#
# Один и тот же сценарий и один и тот же SEED гоняются на двух демонах: старом
# (8cda98d, 14 октября: до правок совета, со своими словарями из data/ копии)
# и сегодняшнем (target/release этой папки). Стенд, машинистка и сценарии —
# сегодняшние для обоих. Результаты — docs/research/report-<дата>/results.tsv;
# в конце make_report.py собирает черновик отчёта. Дежурный демон на время
# прогонов снят и возвращается в конце (trap).
#
# Порядок: быстрая часть на новой версии (стенды 600, X11, хоткеи, хаос, живой
# Wayland), стенды 600 на старой, затем блоки: веб-замороженный, веб с трудными
# токенами, IT (seed 412), свежий веб-блок из интернета — каждый сначала на
# новой, потом на старой.
set -u
cd /home/medushenko/dev/pulinto || exit 1
WORDS=${1:-8000}
OLD_ROOT=${OLD_ROOT:?нужна рабочая копия старой версии}
DATE=2026-10-19
OUT=docs/research/report-$DATE
mkdir -p "$OUT"
RES=$OUT/results.tsv
[ -f "$RES" ] || printf 'группа\tметка\tверсия\tслов\tok\tmissed\tfalse\tcorrupt\tlost\tлишних\tload\tсобытий_потеряно\tзаметка\n' > "$RES"
NEW_BIN=$(sha256sum target/release/pulintod | cut -c1-12)
OLD_BIN=$(sha256sum "$OLD_ROOT/target/release/pulintod" 2>/dev/null | cut -c1-12)
echo "новая $NEW_BIN, старая ${OLD_BIN:-НЕТ}, слов в блоке $WORDS, $(date +%H:%M)"
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

# --- 1. быстрая часть, новая версия ------------------------------------------------
S600=docs/research/stand-wayland/scripts/script-600.txt
WORDS_BAK=$WORDS; WORDS=600
run_block стенд600 mac600 new "$S600" 301 mac
run_block стенд600 plain600 new "$S600" 301 plain
run_block стенд600 mac600 old "$S600" 301 mac
run_block стенд600 plain600 old "$S600" 301 plain
# те же 600 слов мягким набором (без перекрытия): условия тестов до 16 октября
run_block стенд600-мягкий mac600g new "$S600" 301 mac 0
run_block стенд600-мягкий mac600g old "$S600" 301 mac 0
run_block стенд600-мягкий plain600g new "$S600" 301 plain 0
run_block стенд600-мягкий plain600g old "$S600" 301 plain 0
WORDS=$WORDS_BAK
echo "=== X11 $(date +%H:%M)"; mkdir -p docs/research/stand-wayland/x11-report
python3 -I tools/live-x11-test/selection_check.py ./target/release/pulintod docs/research/stand-wayland/x11-report > "$OUT/x11.out" 2>&1; grep -E 'проверок' "$OUT/x11.out" | sed 's/^/    /'
kill_sandbox
echo "=== хоткеи $(date +%H:%M)"; tools/hotkey-test/run-hotkeys.sh > "$OUT/hotkeys.out" 2>&1; grep -E 'проверок' "$OUT/hotkeys.out" | sed 's/^/    /'
echo "=== хаос $(date +%H:%M)"; tools/chaos-test/run-chaos.sh 40 1 > "$OUT/chaos.out" 2>&1; tail -5 "$OUT/chaos.out" | sed 's/^/    /'
kill_sandbox
restore
echo "=== живой Wayland $(date +%H:%M)"
FORCE=1 timeout 900 python3 -I tools/live-wl-test/selection_check_wl.py > "$OUT/live-wl.out" 2>&1; grep -E 'проверок' "$OUT/live-wl.out" | sed 's/^/    /'
stop_duty
echo "=== heldout $(date +%H:%M)"
cargo run --release --example bench_corpus -p pulinto-data -- docs/research/corpus2 heldout.tsv > "$OUT/heldout-new.log" 2>&1
python3 -I tools/typing-fuzzer/score_heldout.py docs/research/corpus2/predictions-pulinto.tsv > "$OUT/heldout-new.txt" 2>&1; head -1 "$OUT/heldout-new.txt" | sed 's/^/    /'
( cd "$OLD_ROOT" && cargo run --release --example bench_corpus -p pulinto-data -- docs/research/corpus2 heldout.tsv > /dev/null 2>&1 ) || echo "heldout старой версии не посчитан"
python3 -I tools/typing-fuzzer/score_heldout.py "$OLD_ROOT/docs/research/corpus2/predictions-pulinto.tsv" > "$OUT/heldout-old.txt" 2>&1; head -1 "$OUT/heldout-old.txt" | sed 's/^/    /'

# --- 2. длинные блоки: новая, потом старая -------------------------------------------
SC=docs/research/stand-wayland/scripts
run_block веб-заморож frozen new "$SC/web-2026-10-18-1443.txt" 19018 mac
run_block веб-заморож frozen old "$SC/web-2026-10-18-1443.txt" 19018 mac
run_block веб-мягкий frozeng new "$SC/web-2026-10-18-1443.txt" 19018 mac 0
run_block веб-мягкий frozeng old "$SC/web-2026-10-18-1443.txt" 19018 mac 0
run_block веб-трудные hard new "$SC/web-2026-10-18-1443-hard.txt" 19018 mac
run_block веб-трудные hard old "$SC/web-2026-10-18-1443-hard.txt" 19018 mac
run_block IT-seed412 it new "$SC/genre-it.txt" 412 mac
run_block IT-seed412 it old "$SC/genre-it.txt" 412 mac
# свежий блок из интернета: новый набор статей, тот же сценарий для обеих версий
STAMP=$(date +%Y-%m-%d-%H%M)
if python3 -I tools/typing-fuzzer/fetch_web_sources.py --out "docs/research/corpus-web/$STAMP" --words 3000 > "$OUT/fetch.log" 2>&1; then
    python3 -I tools/typing-fuzzer/make_script.py --sources "docs/research/corpus-web/$STAMP/sources" --words 20000 \
        --seed $RANDOM --hard-share natural --out "$SC/web-$STAMP.txt" >> "$OUT/fetch.log" 2>&1
    SEEDF=$RANDOM
    run_block веб-свежий fresh new "$SC/web-$STAMP.txt" $SEEDF mac
    run_block веб-свежий fresh old "$SC/web-$STAMP.txt" $SEEDF mac
else
    echo "свежий блок не собран: $(tail -2 "$OUT/fetch.log")"
fi
echo "=== готово $(date +%H:%M)"
python3 -I tools/typing-fuzzer/make_report.py "$OUT" > "$OUT/report-draft.txt" 2>&1
echo "черновик отчёта: $OUT/report-draft.txt"
