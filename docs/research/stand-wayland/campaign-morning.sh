#!/bin/bash
# После c20k-s0: итоговая сборка; дымовой Alt-цикл; A/B «хвоста коротких слов»
# на тех же первых 10 000 словах «руки не трогают»; 50 000 слов на итоговой
# сборке с редкими двойными Alt.
cd /home/medushenko/dev/pulinto
while pgrep -f 'run-script.s[h]' >/dev/null; do sleep 15; done
cargo build --release 2>&1 | tail -2
S=docs/research/stand-wayland/scripts
SHUFFLE=10 ALT=20 SEED=60 tools/typing-fuzzer/run-script.sh alt600      $S/script-600.txt 100 6 virtual
SHUFFLE=0  SEED=20        tools/typing-fuzzer/run-script.sh ab10k-s0-tail $S/script-20000-first10k.txt 60 6 virtual
SHUFFLE=10 ALT=3 SEED=50  tools/typing-fuzzer/run-script.sh c50k-s10    $S/script-50000.txt 60 6 virtual
