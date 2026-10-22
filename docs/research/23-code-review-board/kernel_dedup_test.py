# Does the kernel drop a second press of a key that is already down on the
# same uinput device? Device has only KEY_A (no KEY_SPACE), so pulintod does
# not treat it as a keyboard; it is grabbed exclusively before any event is
# written, so nothing reaches the compositor.
import time, select, sys
from evdev import UInput, InputDevice, ecodes as e

ui = UInput({e.EV_KEY: [e.KEY_A]}, name="pulinto-review-dedup-probe")
dev = None
for _ in range(100):
    try:
        dev = InputDevice(ui.device.path)
        dev.grab()
        break
    except Exception as ex:
        time.sleep(0.05)
if dev is None:
    print("cannot open/grab own device", file=sys.stderr); sys.exit(2)
time.sleep(0.1)
seq = [(1, "press#1"), (1, "press#2 (already down)"), (0, "release#1"), (0, "release#2 (already up)")]
for value, label in seq:
    ui.write(e.EV_KEY, e.KEY_A, value); ui.syn(); time.sleep(0.01)
got = []
while select.select([dev.fd], [], [], 0.2)[0]:
    for ev in dev.read():
        if ev.type == e.EV_KEY:
            got.append(ev.value)
print("written:", [v for v, _ in seq])
print("delivered by kernel:", got)
dev.ungrab(); ui.close()
