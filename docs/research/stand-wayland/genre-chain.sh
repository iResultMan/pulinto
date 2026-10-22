#!/bin/bash
# Жанровая серия: шесть блоков по 20 000 слов (новости, IT, литература,
# разговорный, деловой, научный) на виртуальном экране с вариантами раскладок
# заказчика, перекрытием и перемешиванием раскладок, как в эталонных прогонах.
# Дежурный демон должен быть остановлен заранее; в конце он поднимается тем же
# способом, что и автозапуск GNOME. Итоги — в history.tsv и genre-chain.out.
set -u
cd /home/medushenko/dev/pulinto
S=docs/research/stand-wayland/scripts
TAGS=${TAGS:-news it literature speech business science}
# Дежурный демон — тот, у которого нет PULINTO_GRAB_ONLY; демон песочницы
# (стенд, харнессы) всегда запускается с этой переменной.
is_duty() { ! tr '\0' '\n' < "/proc/$1/environ" 2>/dev/null | grep -q '^PULINTO_GRAB_ONLY='; }
restore() {
    for p in $(pgrep -x pulintod); do is_duty "$p" && return; done
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus XDG_RUNTIME_DIR=/run/user/1000 \
        setsid -f sh -c 'exec ./target/release/pulintod 2>&1 | systemd-cat -t pulintod' < /dev/null
    echo "дежурный демон возвращён: $(sleep 2; pgrep -x pulintod)"
}
trap restore EXIT
for tag in $TAGS; do
    echo "=== genre-$tag $(date +%H:%M)"
    OVERLAP=30 SHUFFLE=25 SEED=4$RANDOM tools/typing-fuzzer/run-script.sh "genre-$tag" "$S/genre-$tag.txt" 100 6 virtual \
        > "docs/research/stand-wayland/genre-$tag.out" 2>&1
    tail -6 "docs/research/stand-wayland/genre-$tag.out"
done
echo "=== итог $(date +%H:%M)"
grep -P '\tgenre-' docs/research/stand-wayland/history.tsv | tail -n "$(echo $TAGS | wc -w)"
