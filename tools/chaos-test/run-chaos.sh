#!/bin/bash
# Хаос-тест залипания: дежурный демон уходит на время прогона и возвращается.
#   run-chaos.sh [циклов] [seed]
cd "$(dirname "$0")/../.." || exit 1
ROOT=$PWD
# Дежурный демон — тот, у которого нет PULINTO_GRAB_ONLY; демон песочницы
# (стенд, харнессы) всегда запускается с этой переменной.
is_duty() { ! tr '\0' '\n' < "/proc/$1/environ" 2>/dev/null | grep -q '^PULINTO_GRAB_ONLY='; }
duty_pid() { for p in $(pgrep -x pulintod); do [ "$(ps -o stat= -p "$p" | cut -c1)" != Z ] && is_duty "$p" && echo "$p"; done; }
restore() {
  [ -n "$HAD_DUTY" ] && DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus XDG_RUNTIME_DIR=/run/user/$(id -u) \
      setsid -f "$ROOT/target/release/pulintod" >> ~/.local/state/pulinto/pulintod.log 2>&1 < /dev/null
}
HAD_DUTY=$(duty_pid)
trap restore EXIT
[ -n "$HAD_DUTY" ] && { kill $HAD_DUTY; sleep 1.5; }
WORK=${TMPDIR:-/tmp}/pulinto-chaos-$$
mkdir -p "$WORK"
python3 tools/chaos-test/stuck_keys.py "$ROOT/target/release/pulintod" "$WORK" "${1:-40}" "${2:-1}"
STATUS=$?
echo "рабочий каталог: $WORK"
exit $STATUS
