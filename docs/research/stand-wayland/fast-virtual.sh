#!/bin/bash
# Быстрый набор с частыми переключениями на виртуальном экране: ищем, где
# теряются и выскакивают буквы на стыке языков. Экран не нужен.
cd /home/medushenko/dev/pulinto
S=docs/research/stand-wayland/scripts
# Два демона не уживаются (замок на физическую клавиатуру): дежурный на
# время серии останавливается и возвращается в конце.
systemctl --user stop pulintod 2>/dev/null
trap 'systemd-run --user --unit=pulintod --collect --description="PuLinTo daemon" /home/medushenko/dev/pulinto/target/release/pulintod' EXIT
run() { tools/typing-fuzzer/run-script.sh "$@"; }
SHUFFLE=50 SEED=201 run vfast-s50-p30   $S/script-2000.txt 30 3 virtual
SHUFFLE=50 SEED=202 run vfast-s50-p0    $S/script-2000.txt 0  2 virtual
SHUFFLE=25 SEED=203 run vmid-s25-p60    $S/script-2000.txt 60 5 virtual
SHUFFLE=0  SEED=204 run vfast-s0-p0     $S/script-2000.txt 0  2 virtual
