#!/bin/bash
# Ночная цепочка: дождаться быстрой виртуальной серии, затем живые прогоны в
# настоящих окнах (экран свободен — интерфейс больше никто не открывает),
# затем повтор быстрого прогона без процессной бури.
cd /home/medushenko/dev/pulinto
while pgrep -f 'fast-virtual.s[h]' >/dev/null; do sleep 20; done
docs/research/stand-wayland/live-night.sh > docs/research/stand-wayland/live-night.out 2>&1
systemctl --user stop pulintod 2>/dev/null
trap 'systemd-run --user --unit=pulintod --collect --description="PuLinTo daemon" /home/medushenko/dev/pulinto/target/release/pulintod' EXIT
S=docs/research/stand-wayland/scripts
SHUFFLE=50 SEED=201 tools/typing-fuzzer/run-script.sh vfast-s50-p30b $S/script-2000.txt 30 3 virtual
