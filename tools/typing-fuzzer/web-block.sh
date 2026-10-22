#!/bin/bash
# Свежий тестовый блок из интернета — каждый раз новый.
#
#   tools/typing-fuzzer/web-block.sh [слов=20000] [пауза=100] [выдержка=6]
#
# 1. fetch_web_sources.py качает тексты по тематикам (ru и en) в
#    docs/research/corpus-web/<метка>/sources, manifest.json — адреса и seed;
# 2. make_script.py режет их на сценарий scripts/web-<метка>.txt (куски 3–25
#    слов, язык меняется между кусками);
# 3. run-script.sh прогоняет его на виртуальном стенде (OVERLAP=30 SHUFFLE=25,
#    случайный SEED), дежурный демон снимается на время прогона и поднимается
#    в конце с target/release. Итог — в history.tsv и web-<метка>.out.
# Переменные: WORDS_PER_SOURCE (слов на тематику и язык, 3000), FETCH_SEED,
# SCRIPT_SEED, RUN_SEED — для воспроизведения конкретного блока; HARD_SHARE —
# доля трудных токенов (цифры, точка внутри, апостроф, знаки на концах),
# по умолчанию natural — как в самих текстах; список — <блок>/hard-tokens.tsv.
set -u
cd "$(dirname "$0")/../.." || exit 1
WORDS=${1:-20000}; PAUSE=${2:-100}; HOLD=${3:-6}
STAMP=$(date +%Y-%m-%d-%H%M)
BLOCK=docs/research/corpus-web/$STAMP
SCRIPT=docs/research/stand-wayland/scripts/web-$STAMP.txt
RUN=web-$STAMP
is_duty() { ! tr '\0' '\n' < "/proc/$1/environ" 2>/dev/null | grep -q '^PULINTO_GRAB_ONLY='; }
restore() {
    pgrep -x pulintod >/dev/null || DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus XDG_RUNTIME_DIR=/run/user/1000 \
        setsid -f sh -c 'exec ./target/release/pulintod 2>&1 | systemd-cat -t pulintod' < /dev/null
    sleep 3; echo "duty back: $(pgrep -x pulintod) $(date +%H:%M)"
}
if pgrep -f '^/bin/bash tools/typing-fuzzer/run-script.sh' >/dev/null; then
    echo "run-script.sh уже выполняется — не трогаю"; exit 1
fi
echo "=== блок $STAMP: бинарник $(sha256sum target/release/pulintod | cut -c1-12), load $(cut -d' ' -f1 /proc/loadavg)"
echo "=== тексты $(date +%H:%M)"
python3 -I tools/typing-fuzzer/fetch_web_sources.py --out "$BLOCK" --words "${WORDS_PER_SOURCE:-3000}" ${FETCH_SEED:+--seed "$FETCH_SEED"} || exit 1
SSEED=${SCRIPT_SEED:-$RANDOM}
python3 -I tools/typing-fuzzer/make_script.py --sources "$BLOCK/sources" --words "$WORDS" --seed "$SSEED" \
    --hard-share "${HARD_SHARE:-natural}" --hard-list "$BLOCK/hard-tokens.tsv" --out "$SCRIPT" || exit 1
echo "сценарий $SCRIPT: $(wc -l < "$SCRIPT") слов, seed $SSEED"
RSEED=${RUN_SEED:-$RANDOM}
trap restore EXIT
for p in $(pgrep -x pulintod); do is_duty "$p" && kill -TERM "$p"; done
for i in $(seq 1 40); do pgrep -x pulintod >/dev/null || break; sleep 0.1; done
pgrep -x pulintod >/dev/null && { echo "демон не остановился"; exit 1; }
echo "=== стенд $(date +%H:%M), SEED=$RSEED"
OVERLAP=30 SHUFFLE=25 SEED=$RSEED tools/typing-fuzzer/run-script.sh "$RUN" "$SCRIPT" "$PAUSE" "$HOLD" virtual > "docs/research/stand-wayland/$RUN.out" 2>&1
tail -7 "docs/research/stand-wayland/$RUN.out"
for p in $(pgrep -x pulintod); do is_duty "$p" || kill "$p"; done
sleep 1
echo "=== готово $(date +%H:%M)"
