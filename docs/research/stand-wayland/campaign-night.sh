#!/bin/bash
# Ночная кампания 14 октября: 10, 20 и 50 тысяч слов на виртуальном экране.
cd /home/medushenko/dev/pulinto
S=docs/research/stand-wayland/scripts
SHUFFLE=25 SEED=10 tools/typing-fuzzer/run-script.sh c10k-s25 $S/script-10000.txt 60 6 virtual
SHUFFLE=0  SEED=20 tools/typing-fuzzer/run-script.sh c20k-s0  $S/script-20000.txt 60 6 virtual
SHUFFLE=10 SEED=50 tools/typing-fuzzer/run-script.sh c50k-s10 $S/script-50000.txt 60 6 virtual
