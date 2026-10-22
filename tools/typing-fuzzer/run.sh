#!/bin/bash
# Один прогон стенда: типист → демон → окно-приёмник → три лога.
set -u
ROOT=/home/medushenko/dev/pulinto
LOG_DIR=$ROOT/docs/research/fuzzer-logs
export LANG=en_US.UTF-8
mkdir -p "$LOG_DIR"

TAG=${1:-run01}
MODE=${2:-observe}
PAUSE=${3:-600}
WORDS=${4:-200}
WRONG=${5:-50}
DELIM=${6:-mixed}
LENGTH=${7:-all}
SRC_FILE=${8:-source_mixed.txt}
SEED=${9:-20260803}

SOURCE_PATH="$LOG_DIR/$SRC_FILE"
if [ ! -f "$SOURCE_PATH" ]; then
    SOURCE_PATH="$ROOT/docs/research/fuzzer-logs/$SRC_FILE"
fi

cd "$ROOT"

# Проверка безопасности: слушаем устройство ввода 3 секунды
echo "=== Безопасность: слушаем клавиатуру 3 секунды ==="
HUMAN_ACTIVE=$(python3 -c '
import os, select, sys
devs = ["/dev/input/event4", "/dev/input/event6"]
fds = []
for d in devs:
    if os.path.exists(d):
        try:
            fd = os.open(d, os.O_RDONLY | os.O_NONBLOCK)
            fds.append(fd)
        except Exception:
            pass
if fds:
    r, _, _ = select.select(fds, [], [], 3.0)
    for fd in fds:
        os.close(fd)
    if r:
        print("YES")
        sys.exit(0)
print("NO")
')

if [ "$HUMAN_ACTIVE" = "YES" ]; then
    echo "ОТМЕНА ПРОГОНА: обнаружена активность человека за клавиатурой!"
    exit 1
fi

pkill -x pulintod 2>/dev/null
pkill -f "[s]ink.py" 2>/dev/null
sleep 1

sed -i "s/^mode = .*/mode = $MODE/" ~/.config/pulinto/config.conf
./target/release/pulinto layout --set us >/dev/null 2>&1; sleep 0.5

# Рассчитываем время выполнения с запасом
# 15 секунд на стартовое ожидание + время ввода слов
RUNTIME=$(( WORDS * (PAUSE + 1100) / 1000 + 35 ))

./tools/typing-fuzzer/target/release/typist \
   --source "$SOURCE_PATH" \
   --log "$LOG_DIR/$TAG-typist.jsonl" \
   --words "$WORDS" \
   --pause "$PAUSE" \
   --wrong "$WRONG" \
   --delimiters "$DELIM" \
   --length "$LENGTH" \
   --seed "$SEED" \
   --wait 12 \
   > "$LOG_DIR/$TAG-typist.err" 2>&1 &
TYPIST=$!

sleep 1
nohup ./target/release/pulintod > "$LOG_DIR/$TAG-daemon.log" 2>&1 &
DAEMON_PID=$!

# Сторож. В режиме захвата клавиатура удерживается эксклюзивно,
# если демон зависнет — выбьет через WATCHDOG секунд.
WATCHDOG=$(( RUNTIME + 60 ))
( sleep $WATCHDOG; pkill -x pulintod 2>/dev/null && echo "СТОРОЖ: снял захват по таймеру" >> "$LOG_DIR/$TAG-daemon.log" ) &
WD=$!

sleep 2
: > "$LOG_DIR/$TAG-sink.log"
python3 "$ROOT/tools/typing-fuzzer/sink.py" "$LOG_DIR/$TAG-sink.log" "$RUNTIME" > /dev/null 2>&1 &
SINK=$!

# Держим фокус, пока типист не начал
FOCUSED=нет
for _ in $(seq 1 14); do
    WID=$(xdotool search --name "PuLinTo sink" | head -n 1)
    if [ -n "$WID" ]; then
      xdotool windowfocus $WID
    fi
    sleep 1
    if [ "$(xdotool getwindowfocus getwindowname 2>/dev/null)" = "PuLinTo sink" ]; then
      FOCUSED=да; break
    fi
done

echo "фокус на приёмнике: $FOCUSED"
if [ "$FOCUSED" != "да" ]; then
    echo "ПРОГОН НЕДЕЙСТВИТЕЛЕН: окно не в фокусе"
    kill $TYPIST $SINK $WD 2>/dev/null
    pkill -x pulintod 2>/dev/null
    sed -i 's/^mode = .*/mode = off/' ~/.config/pulinto/config.conf
    ./target/release/pulinto layout --set us >/dev/null 2>&1
    exit 1
fi

# Сторож фокуса. Проверка человека за клавиатурой делается один раз, перед
# стартом, и этого мало: машинистка бьёт по клавишам вслепую, в то окно, которое
# активно СЕЙЧАС. Стоит человеку переключить окно посреди прогона — и робот
# печатает ему в чат. Так и вышло: заказчик получил полстраницы тестового текста
# в поле ввода. Теперь фокус проверяется каждую секунду, и уход с приёмника
# снимает прогон целиком.
rm -f "$LOG_DIR/$TAG-INVALID"
(
  while kill -0 $TYPIST 2>/dev/null; do
    ACTIVE=$(xdotool getwindowfocus getwindowname 2>/dev/null)
    if [ "$ACTIVE" != "PuLinTo sink" ]; then
      echo "СТОРОЖ ФОКУСА: приёмник потерял фокус (активно «$ACTIVE») — снимаю прогон" \
          | tee -a "$LOG_DIR/$TAG-daemon.log"
      : > "$LOG_DIR/$TAG-INVALID"
      kill $TYPIST 2>/dev/null
      pkill -x pulintod 2>/dev/null
      break
    fi
    sleep 1
  done
) &
FOCUS_WD=$!

wait $TYPIST
kill $FOCUS_WD 2>/dev/null
sleep 2
kill $SINK 2>/dev/null
kill $WD 2>/dev/null
pkill -x pulintod 2>/dev/null

sed -i 's/^mode = .*/mode = off/' ~/.config/pulinto/config.conf
./target/release/pulinto layout --set us >/dev/null 2>&1

if [ -f "$LOG_DIR/$TAG-INVALID" ]; then
    echo "ПРОГОН $TAG НЕДЕЙСТВИТЕЛЕН: фокус ушёл с приёмника, цифрам верить нельзя"
    exit 2
fi

echo "--- прогон $TAG ($MODE, $WORDS слов, пауза ${PAUSE}мс, wrong ${WRONG}%, delim $DELIM, len $LENGTH) закончен ---"

python3 "$ROOT/tools/typing-fuzzer/compare.py" \
    "$LOG_DIR/$TAG-typist.jsonl" \
    "$LOG_DIR/$TAG-sink.log" \
    "$LOG_DIR/$TAG-daemon.log" | tee "$LOG_DIR/$TAG-compare.txt"

# Прогон записывает себя сам. Раньше результат попадал в журнал, только если
# кто-то потом его туда переносил, — и прогоны терялись целыми кампаниями,
# когда наблюдающий отвлекался. Теперь дошедший до конца прогон записан всегда.
python3 "$ROOT/tools/typing-fuzzer/record-run.py" "$TAG" "$MODE" "$SRC_FILE" "$PAUSE" "$DELIM"

