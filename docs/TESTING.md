# Помощь с тестированием / Help with testing

[English version below](#english)

PuLinTo проверен на Ubuntu: X11 (XFCE) и GNOME 50 на Wayland. На других системах нужны люди, которые поставят программу и скажут, что работает. Особенно ценны:

- **Red Hat Enterprise Linux, AlmaLinux, Rocky Linux, CentOS Stream, Fedora.** Для них собран пакет `.rpm`, но на живых машинах он не проверялся.
- **Debian, Linux Mint, Pop!_OS, elementary OS и другие производные Ubuntu.**
- **openSUSE, Arch Linux, Manjaro и другие.** Здесь поможет архив и установка командой.
- **Другие рабочие столы.** Проверены XFCE (X11) и GNOME 50 (Wayland). Нужны отчёты с другими: GNOME в сеансе X11 (старые версии), Cinnamon, MATE. KDE и Sway пока не поддерживаются, но отчёт о том, что именно не работает, тоже полезен.

## Что проверить (10 минут)

1. Установите любым способом из [README](../README.md): командой, `.deb` или `.rpm`.
2. Исправления работают сразу; входить в сеанс заново не нужно.
3. Проверьте, что в меню приложений есть ярлык «PuLinTo» со значком и что в трее значок клавиатуры с оранжевым пробелом. В «чистом» GNOME (Fedora, RHEL) значков в трее может не быть без расширения AppIndicator: это не ошибка, но напишите, как у вас.
4. Откройте любое поле ввода, переключите раскладку на английскую и наберите `ghbdtn` и пробел.
   Должно получиться «привет», и раскладка должна стать русской.
5. Наберите в русской раскладке `руддщ ` (с пробелом): должно получиться `hello`.
6. Нажмите `Alt` (Apple), `Pause/Break` или правый `Ctrl` (остальные) на слове, которое не исправилось.
7. Откройте окно настроек из меню и добавьте автозамену.
8. Удалите программу (`sudo apt remove pulinto`, `sudo dnf remove pulinto` или `./uninstall.sh`) и убедитесь, что ярлык и значок пропали.

Если клавиатура не отвечает: нажмите одновременно `Backspace`, `Esc` и `Enter`.

## Трудные слова

Печатайте как обычно и замечайте, где программа ошиблась: слово исправила зря, пропустила или исправила не так. Каждый такой случай полезен: слова, смешанные русско-английские фразы, имена, сленг, сокращения. Присылайте их через [форму «Трудное слово»](https://github.com/iResultMan/pulinto/issues/new?template=hard_word.yml) или в Telegram-поддержку https://t.me/pulinto_bot: словари дорабатываются по этим случаям.

## Как прислать результат

Любым удобным способом:

- заполните [форму «Отчёт о тесте»](https://github.com/iResultMan/pulinto/issues/new?template=test_report.yml);
- напишите в Telegram-поддержку [@pulinto_bot](https://t.me/pulinto_bot);
- или на почту i.result.man@gmail.com.

В отчёте укажите: дистрибутив и версию, рабочий стол и сеанс (X11 или Wayland), способ установки, какие пункты списка прошли, а какие нет. Если что-то сломалось, приложите вывод `journalctl --user -u pulintod -n 50` и, при неверном исправлении слова, строки отчёта из `~/.local/share/pulinto/reports.jsonl` (файл появляется после нажатия `Ctrl+Alt+\`).
Он содержит набранные слова: уберите всё, что не хотите показывать.

<!-- english -->

---

## English

PuLinTo has been tested on Ubuntu: X11 (XFCE) and GNOME 50 on Wayland. On other systems we need people
who will install the program and tell us what works. Especially valuable:

- **Red Hat Enterprise Linux, AlmaLinux, Rocky Linux, CentOS Stream, Fedora.** An `.rpm` package is
  built for them, but it has not been tried on live machines.
- **Debian, Linux Mint, Pop!_OS, elementary OS and other Ubuntu derivatives.**
- **openSUSE, Arch Linux, Manjaro and others.** The archive and the one-command install will help here.
- **Other desktops.** XFCE (X11) and GNOME 50 (Wayland) have been tested. Reports from others are
  needed: GNOME in an X11 session (older versions), Cinnamon, MATE. KDE and Sway are not supported
  yet, but a report on what exactly does not work is useful too.

### What to check (10 minutes)

1. Install it in any of the ways described in the [README](../README_EN.md): the command, the `.deb` or the `.rpm`.
2. Corrections work right away; there is no need to log out.
3. Check that the application menu has a "PuLinTo" launcher with an icon and that the tray shows an icon with an orange space bar. In a plain GNOME (Fedora, RHEL) there may be no tray icons without the AppIndicator
   extension: that is not a bug, but tell us what you see.
4. In any input field, switch to the English layout and type `ghbdtn` and a space. You should get
   "привет", and the layout should become Russian.
5. In the Russian layout type `руддщ ` (with a space): you should get `hello`.
6. Press `Alt` (Apple keyboard), `Pause/Break` or the right `Ctrl` (others) on a word that was not fixed.
7. Open the settings window from the menu and add an autocorrection.
8. Uninstall the program (`sudo apt remove pulinto`, `sudo dnf remove pulinto` or `./uninstall.sh`)
   and make sure the launcher and icon are gone.

If the keyboard stops responding, press `Backspace`, `Esc` and `Enter` together.

### Hard words

Type as usual and notice where the program went wrong: it fixed a word it should not have, missed
one, or fixed it wrongly. Every such case is useful: words, mixed Russian-English phrases, names,
slang, abbreviations. Send them through the
[Hard word form](https://github.com/iResultMan/pulinto/issues/new?template=hard_word.yml) or to the
Telegram support [@pulinto_bot](https://t.me/pulinto_bot): the dictionaries are improved from these cases.

### How to send the result

Any way you like:

- fill in the [Test report form](https://github.com/iResultMan/pulinto/issues/new?template=test_report.yml);
- write to the Telegram support [@pulinto_bot](https://t.me/pulinto_bot);
- or email i.result.man@gmail.com.

In the report give: the distribution and version, the desktop and session (X11 or Wayland), the way
you installed it, which items of the list passed and which did not. If something broke, attach the
output of `journalctl --user -u pulintod -n 50` and, for a wrongly fixed word, the lines of the report
from `~/.local/share/pulinto/reports.jsonl` (the file appears after you press `Ctrl+Alt+\`). It
contains the words you typed: remove anything you do not want to show.
