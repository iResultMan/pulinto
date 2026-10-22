#!/bin/bash
# Живая ночная серия в настоящих окнах GNOME. Дежурный демон на время
# серии останавливается (стенд поднимает свой) и возвращается в конце.
cd /home/medushenko/dev/pulinto
S=docs/research/stand-wayland/scripts
systemctl --user stop pulintod 2>/dev/null
trap 'systemd-run --user --unit=pulintod --collect --description="PuLinTo daemon" /home/medushenko/dev/pulinto/target/release/pulintod' EXIT
run() { tools/typing-fuzzer/run-script.sh "$@"; }
# средний темп, случайная раскладка, двойной Alt — в Wayland-окне и X-окне
SHUFFLE=25 ALT=5  SEED=101 run live-wl-2k   $S/script-2000.txt 100 6 wayland
SHUFFLE=25 ALT=5  SEED=102 run live-x11-2k  $S/script-2000.txt 100 6 x11
# быстрый темп, частые переключения — где теряются буквы
SHUFFLE=50 ALT=0  SEED=103 run live-wl-fast $S/script-2000.txt 30 3 wayland
SHUFFLE=50 ALT=0  SEED=104 run live-x11-fast $S/script-2000.txt 30 3 x11
