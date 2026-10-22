#!/usr/bin/env python3
"""Сторож раскладки: пишет каждую смену с отметкой времени.

Опрос раз в 50 мс дешевле подписки и для журнала достаточен: косметическое
переключение после замены видно как смена имени раскладки.
"""
import subprocess
import sys
import time

LOG = sys.argv[1]
BIN = sys.argv[2] if len(sys.argv) > 2 else "./target/release/pulinto"

last = None
with open(LOG, "a", buffering=1, encoding="utf-8") as log:
    while True:
        try:
            out = subprocess.run(
                [BIN, "layout"], capture_output=True, text=True, timeout=2
            ).stdout
        except Exception as e:  # X умер — записать и выйти
            log.write(f"{int(time.time()*1000)}\terror\t{e}\n")
            break
        current = None
        for line in out.splitlines():
            if "раскладка:" in line:
                current = line.strip()
                break
        if current and current != last:
            log.write(f"{int(time.time()*1000)}\t{current}\n")
            last = current
        time.sleep(0.05)
