#!/bin/bash
# Живая проверка в настоящих окнах GNOME: Wayland-окно и X-окно (Xwayland).
# Запускать, когда экран разблокирован и за машиной никого не будет ~15 минут:
# робот печатает в окно-приёмник, демон захватывает только клавиатуру робота.
cd /home/medushenko/dev/pulinto
S=docs/research/stand-wayland/scripts
FORCE=1 SHUFFLE=25 SEED=101 tools/typing-fuzzer/run-script.sh live-wl-2k  $S/script-2000.txt 100 6 wayland
FORCE=1 SHUFFLE=25 SEED=102 tools/typing-fuzzer/run-script.sh live-x11-2k $S/script-2000.txt 100 6 x11
