#!/bin/bash
# Наблюдатель жанровой серии: после каждого завершённого блока шлёт заказчику
# в Telegram (hermes send) строку history.tsv с разбором порчи; в конце —
# сводную таблицу. Останавливается, когда отчитался за все блоки или цепочка
# genre-chain.sh исчезла.
cd /home/medushenko/dev/pulinto
H=docs/research/stand-wayland/history.tsv
TAGS="news it literature speech business science"
declare -A done
# SKIP="news it" — блоки, о которых уже отчитались (перезапуск наблюдателя).
for t in ${SKIP:-}; do done[$t]=1; done
names=( [news]="новости" [it]="IT" [literature]="литература" [speech]="разговорный" [business]="деловой" [science]="научный" )
report() {
    local tag=$1 row=$2 d=docs/research/stand-wayland/genre-$tag
    local ok missed false corrupt
    IFS=$'\t' read -r _ _ _ _ _ _ _ ok missed false corrupt _ <<<"$row"
    local total=$((ok+missed+false+corrupt))
    local dropped; dropped=$(grep -c DROPPED "$d/ctl/trace.log" 2>/dev/null)
    local pct; pct=$(awk -v o=$ok -v t=$total 'BEGIN{printf "%.2f", o*100/t}')
    local fpct; fpct=$(awk -v f=$false -v t=$total 'BEGIN{printf "%.3f", f*100/t}')
    {
        echo "Блок «${names[$tag]}» (genre-$tag): $total слов"
        echo "ok $ok ($pct%) · missed $missed · false $false ($fpct%) · corrupt $corrupt · DROPPED ${dropped:-?}"
        if [ "$false" -gt 0 ]; then
            echo "— ложные:"; grep -P '^false' "$d/errors.tsv" | awk -F'\t' '{printf "  %s → %s (%s)\n", $5, $7, $8}' | head -8
        fi
        if [ "$corrupt" -gt 0 ]; then
            echo "— порча (до 10):"; grep -P '^corrupt' "$d/errors.tsv" | awk -F'\t' '{printf "  %s → %s\n", $5, $7}' | head -10
        fi
    } | hermes send --to telegram -s "PuLinTo · блок «${names[$tag]}»" -q
}
while :; do
    while IFS= read -r row; do
        tag=$(echo "$row" | awk -F'\t' '{print $2}' | sed 's/^genre-//')
        [ -n "${done[$tag]:-}" ] && continue
        report "$tag" "$row" && done[$tag]=1
    done < <(grep -P '\tgenre-(news|it|literature|speech|business|science)\t' "$H")
    [ "${#done[@]}" -ge 6 ] && break
    pgrep -f 'genre-chain.s[h]' >/dev/null || break
    sleep 60
done
{
    echo "Серия завершена, $(date +%H:%M). Сводка (ok / missed / false / corrupt):"
    for tag in $TAGS; do
        grep -P "\tgenre-$tag\t" "$H" | tail -1 | awk -F'\t' -v n="${names[$tag]}" '{printf "  %-12s %5d / %3d / %2d / %3d\n", n, $8, $9, $10, $11}'
    done
    echo "Дежурный демон: $(pgrep -x pulintod >/dev/null && echo поднят || echo НЕ ПОДНЯТ)"
} | hermes send --to telegram -s "PuLinTo · итог серии" -q
