# Вопросы и ответы / FAQ

[English version below](#english)

## Что делает PuLinTo?

Исправляет слово, набранное не в той раскладке: вы напечатали `ghbdtn` и забыли переключиться, а на экране окажется «привет». Работает и в обратную сторону: `рщц` станет `how`. Слово исправляется на пробеле или знаке, раньше, чем они дойдут до окна, поэтому сообщение уходит в чат уже правильным. Раскладка при этом тоже переключается, и следующее слово вы набираете в нужной.

## Как установить?

Одним действием: командой в терминале или установочным файлом. Команда:

```
curl -fsSL https://iresultman.github.io/pulinto/install.sh | bash
```

Файлы: `.deb` (Ubuntu, Debian, Mint), `.rpm` (Fedora, RHEL, AlmaLinux, Rocky) и архив `.tar.gz` для любого Linux. Они лежат на странице [последнего выпуска](https://github.com/iResultMan/pulinto/releases/latest), шаги описаны в разделе «Установка» [руководства](MANUAL.md). Установка один раз спросит пароль администратора: нужно правило доступа к клавиатуре. Больше ничего делать не нужно: помощник запускается сам, в меню приложений появляется ярлык «PuLinTo», в трее значок. Пакеты для Red Hat и родственных систем собраны, но на живых машинах пока не проверялись: [помогите проверить](TESTING.md).

## Нужно ли что-то делать после установки?

Нет, исправления работают сразу. Один раз стоит проверить, что в настройках системы включено сочетание переключения раскладки, например Alt+Shift: именно им программа переключает раскладку. Перезагружать компьютер и выходить из сеанса не требуется. Выход и вход нужны только в одном случае: в GNOME на Wayland расширение GNOME Shell, которое сообщает программе, в каком приложении вы печатаете, подхватывается при следующем входе. Без него не работают лишь правила для отдельных программ в Wayland-окнах, а значок в трее GNOME может потребовать расширения AppIndicator (подробнее ниже).

## На чём работает?

Linux с X11 (проверялось на XFCE) и GNOME на Wayland (проверялось на GNOME 50 в Ubuntu 26.04). Раскладки: русская и английская. KDE и Sway пока не поддерживаются: под Wayland нужна связка с GNOME, расширение написано только под GNOME Shell. Готовые сборки есть для 64-разрядных систем (x86\_64).

## Как обновить программу?

Выполните ту же команду установки ещё раз или поставьте новый `.deb` или `.rpm`. Ваши правила и настройки сохраняются.

## Как удалить?

`sudo apt remove pulinto`, `sudo dnf remove pulinto` или `./uninstall.sh` из архива. Правила и настройки в вашем домашнем каталоге остаются.

## Как исправить слово, которое программа пропустила?

Нажмите клавишу исправления: одиночный `Alt` или `Pause/Break` на клавиатуре Apple, `Pause/Break` или одиночный правый `Ctrl` на остальных. Она исправляет слово, откатывает последнюю замену или переводит выделенный текст; каждое следующее нажатие даёт следующее прочтение. Полный список клавиш и как их менять — в разделе «Горячие клавиши» [руководства](MANUAL.md).

## Программа испортила правильное слово. Что делать?

Нажмите клавишу исправления, чтобы откатить замену: слово вернётся, и программа запомнит его. Чтобы мы разобрались, нажмите `Ctrl+Alt+\` сразу после ошибки: демон запишет отчёт о последних восьми решениях в `~/.local/share/pulinto/reports.jsonl`. Отчёт содержит слова, поэтому пишется только по вашей команде и не отправляется никуда сам. Пришлите нужные строки через [форму «Трудное слово»](https://github.com/iResultMan/pulinto/issues/new?template=hard_word.yml), Telegram-поддержку [@pulinto_bot](https://t.me/pulinto_bot) или по почте i.result.man@gmail.com.

## Почему в терминале или редакторе слова не исправляются сами?

Для терминалов, редакторов кода, менеджеров паролей, удалённых рабочих столов и игр заведены безопасные режимы по умолчанию: там программа исправляет только по клавише. Режим для окна в фокусе переключается по кругу сочетанием `Ctrl+Alt+P` (автоматически, только по горячей клавише, выключено); для каждой программы режим задаётся в окне настроек, на странице «Приложения».

## Как отключить для одной программы?

Окно настроек, страница «Приложения»: для программы выберите «Автоматически», «Только по горячей клавише» или «Полностью выключено». Быстрее: `Ctrl+Alt+P` меняет режим для окна в фокусе, а в меню значка в трее режим текущей программы переключается одним щелчком. Подробности: [руководство](MANUAL.md).

## Как приостановить или совсем остановить исправления?

Всё делается из меню значка в трее:

- **Приостановить**: исправления выключены, клавиатура работает как обычно; тот же пункт потом называется «Включить».
- **Исправлять автоматически**: снимите галочку, и слова будут исправляться только по клавише исправления.
- **Перезапустить**: помощник запускается заново, не трогая ваших настроек.
- **Выйти**: помощник полностью останавливается и выгружается из памяти до следующего входа в систему.

Если значка в трее нет, откройте окно настроек (ярлык «PuLinTo» в меню приложений): на странице «Обзор» есть переключатель «Исправлять автоматически» и показано, запущен ли помощник. Полностью остановить или перезапустить помощника без значка можно выходом из сеанса и новым входом (или командой `systemctl --user stop pulintod`, см. ответ «Клавиатура не отвечает или клавиша залипла»).

## У меня клавиатура Apple. Что настроить?

PuLinTo сам находит клавиатуру Apple и на странице «Обзор» окна настроек предлагает привычное расположение знаков, как на Mac: запятая и точка на Shift+6 и Shift+7, буква ё на клавише `\`. Предложение можно принять или отклонить, а позже включить на странице «Клавиатура Apple». С обычной клавиатурой эта страница не нужна. **Если подключены обе, раскладка Apple действует на обе клавиатуры, потому что раскладка в системе одна. Вернуть привычные знаки на обычной клавиатуре можно в любой момент клавишей `Ctrl+Pause/Break`** (она же включает раскладку обратно; на клавиатуре Apple то же делает `Ctrl+Alt+'`); горячие клавиши исправления у каждой клавиатуры свои: `Alt` у Apple, `Pause/Break` у обычной. Подробности: раздел «Клавиатура Apple» [руководства](MANUAL.md).

## Не трогает ли программа пароли и адреса?

Нет: пароли, адреса, команды, код и слова из двух алфавитов в одном слове не меняются. Правила смотрят на форму слова, а не на то, в каком поле оно набрано, поэтому для программ, где хранятся секреты, надёжнее выключить режим целиком.

## Это не кейлоггер?

Программа читает нажатия клавиш, иначе она не заметит слово в чужой раскладке. Но журнала набора нет: демон не сохраняет ни набранный, ни исправленный текст. В его выводе только тип операции, длина слова и ступень решения, а файла журнала нет совсем: последние строки хранятся только в памяти. Режим отладки (страница «Диагностика») по умолчанию выключен, пишет только в память, и пока он работает, на значке горит красная точка. Диагностический отчёт пишется на диск, только если вы сами нажали `Ctrl+Alt+\`. Исходный код открыт, проверить это можно по нему. Что именно попадает на диск, описано в разделе «Приватность» [руководства](MANUAL.md).

## Нет значка в трее

Программа при этом работает. В «чистом» GNOME (Fedora, RHEL и другие) значки в трее показывает расширение «AppIndicator and KStatusNotifierItem Support» (`gnome-shell-extension-appindicator`); в Ubuntu оно уже есть. Установочная команда ставит его сама, пакеты `.deb` и `.rpm` его рекомендуют, а помощник сам включает его в GNOME. Если значка всё равно нет, выйдите из сеанса и войдите снова: GNOME подхватывает новое расширение только при входе. Настройки открываются из меню приложений, ярлык «PuLinTo», значок в трее для этого не нужен.

## Клавиатура не отвечает или клавиша залипла

1. Нажмите одновременно `Backspace`, `Esc` и `Enter`: захват клавиатуры снимется сразу.
2. Выберите в меню значка в трее **Перезапустить**. Если не помогло, выберите **Приостановить**, а затем **Выйти**.
3. Если это повторяется, сообщите об ошибке: что вы делали и какая нагрузка была на компьютере.

Крайний вариант, если значка нет, а клавиатура ведёт себя странно: в терминале `systemctl --user stop pulintod`. Эта команда полностью останавливает помощника и выгружает его из памяти. Подробно: «Если клавиша залипла» в [руководстве](MANUAL.md).

## Программа не исправляет слова

Проверьте по порядку:

1. На значке в трее оранжевый пробел? Серый значок или пункт «Включить» в его меню означает, что исправления выключены: выберите «Включить». Нет значка — откройте окно настроек (ярлык «PuLinTo»): на странице «Обзор» видно, запущен ли помощник и включено ли автоматическое исправление.
2. Выберите в меню значка **Перезапустить**.
3. В настройках системы включено сочетание переключения раскладки (например, Alt+Shift)? Без него программа не может переключить раскладку.
4. В окне, где вы печатаете, программа не выключена? Для терминалов и редакторов кода исправление по умолчанию идёт только по клавише (`Pause/Break`, `Alt` на Apple).

Если ничего не помогло, скопируйте последние строки работы демона в окне настроек («Диагностика» → «Копировать»). Любители командной строки могут посмотреть состояние и журнал: `systemctl --user status pulintod` и `journalctl --user -u pulintod -n 50`. Приложите вывод к обращению в поддержку. Остальное — в разделе «Устранение неполадок» [руководства](MANUAL.md).

## Чем отличается от xneur?

Принцип тот же: слово, набранное не в той раскладке, исправляется. Отличия:

- xneur не запускается под Wayland, PuLinTo работает в GNOME на Wayland;

- PuLinTo учитывает два предыдущих слова, а для коротких слов и следующее, поэтому лучше справляется со словами из одной-двух букв; на длинных словах программы почти не различаются.

## Это продукт Яндекса или Punto Switcher?

Нет. Проект не связан с Яндексом и не является продуктом Punto Switcher. От Punto Switcher взята идея; код, алгоритм распознавания и словари написаны самостоятельно.

## Какой язык ещё можно добавить?

Сейчас русский и английский. Нужен другой язык? Напишите, какой язык вы хотели бы видеть в программе, и согласитесь побыть тестировщиком: словари мы соберём сами, от вас нужно попробовать язык на живой клавиатуре и сказать, как он работает. Если умеете собирать словари, подключайтесь: формат описан в [«Как собрать языковой пакет»](LANGUAGE_PACK.md). Общая картина: [«Другие языки и трудные слова»](LANGUAGES.md).

## На каких языках интерфейс?

Окно настроек и меню трея есть на русском и английском. По умолчанию язык берётся из языка системы, иначе показывается английский. Выбрать язык вручную можно в любой момент: в меню значка в трее пункт «Язык интерфейса» или в меню окна настроек (кнопка с тремя полосками). Выбор запоминается и действует и в трее, и в окне. Для проверок язык задаёт и переменная `PULINTO_LANG=ru` или `en`. Нужен другой язык интерфейса? Напишите в поддержку: перевод — это файл `po/<язык>.po` примерно из 280 строк, его можно прислать готовым ([как](LANGUAGES.md)).

## Лицензия?

Код: MIT или Apache-2.0, на ваш выбор. Языковые данные (словари и таблицы буквосочетаний): CC BY 4.0, можно использовать, в том числе в коммерческих продуктах, с указанием авторства. Источники и атрибуция: [«Данные и лицензии»](../data/ATTRIBUTION.md).

## Как связаться?

Telegram-поддержка [@pulinto\_bot](https://t.me/pulinto_bot) (сначала отвечает ИИ-помощник по документации, при необходимости автор), почта [i.result.man@gmail.com](mailto:i.result.man@gmail.com) или обращения на GitHub. Слово, на котором программа ошиблась, присылайте любым способом: словари дорабатываются по таким случаям.

<!-- english -->

---

## English

### What does PuLinTo do?

It fixes a word typed in the wrong layout: you typed `ghbdtn` and forgot to switch, and "привет" appears on screen. It works the other way too: `рщц` becomes `how`. The word is fixed on the space or punctuation key, before that key reaches the window, so a chat message goes out already correct. The layout is switched as well, so you carry on typing the next word in the right one.

### How do I install it?

In a single step: a command in the terminal or an installer file. The command:

```
curl -fsSL https://iresultman.github.io/pulinto/install.sh | bash
```

Files: `.deb` (Ubuntu, Debian, Mint), `.rpm` (Fedora, RHEL, AlmaLinux, Rocky) and a `.tar.gz` archive for any Linux. They are on the [latest release](https://github.com/iResultMan/pulinto/releases/latest) page; the steps are in the "Installation" section of the [manual](MANUAL.md). The installation asks for the administrator password once: a keyboard access rule is needed. There is nothing else to do: the helper starts by itself, a "PuLinTo" launcher appears in the application menu and an icon in the tray. The packages for Red Hat and related systems are built but have not been tried on live machines yet: [help us test them](TESTING.md).

### Do I need to do anything after installing?

No, corrections work right away. It is worth checking once that the system settings have a layout-switching shortcut enabled, for example Alt+Shift: that is what the program uses to switch the layout. You do not need to reboot or log out. Logging out and back in matters in one case only: on GNOME with Wayland, the GNOME Shell extension that tells the program which application you are typing in is picked up at the next login. Until then, only the per-application rules for Wayland windows do not work, and the GNOME tray icon may need the AppIndicator extension (see below).

### What does it run on?

Linux with X11 (tested on XFCE) and GNOME on Wayland (tested on GNOME 50 in Ubuntu 26.04). Layouts: Russian and English. KDE and Sway are not supported yet: under Wayland a GNOME setup is needed, because the extension is written for GNOME Shell only. Ready-made builds exist for 64-bit systems (x86\_64).

### How do I update?

Run the same install command again or install a new `.deb` or `.rpm`. Your rules and settings are kept.

### How do I uninstall?

`sudo apt remove pulinto`, `sudo dnf remove pulinto` or `./uninstall.sh` from the archive. The rules and settings in your home directory stay.

### How do I fix a word the program missed?

Press the correction key: a lone `Alt` or `Pause/Break` on an Apple keyboard, `Pause/Break` or a lone right `Ctrl` on others. It fixes the word, undoes the last replacement or converts the selected text; each further press gives the next reading. The full list of keys and how to change them is in the "Hotkeys" section of the [manual](MANUAL.md).

### The program broke a correct word. What now?

Press the correction key to undo the replacement: the word comes back and the program remembers it. To help us look into it, press `Ctrl+Alt+\` right after the mistake: the daemon writes a report of the last eight decisions to `~/.local/share/pulinto/reports.jsonl`. The report contains words, so it is written only on your command and is never sent anywhere by itself. Send the relevant lines through the [Hard word form](https://github.com/iResultMan/pulinto/issues/new?template=hard_word.yml), the Telegram support [@pulinto_bot](https://t.me/pulinto_bot) or by email to i.result.man@gmail.com.

### Why are words not fixed automatically in a terminal or editor?

Terminals, code editors, password managers, remote desktops and games have safe defaults: there the program fixes only on the hotkey. The mode for the focused window cycles with the `Ctrl+Alt+P` hotkey (automatic, hotkey only, off); per application the mode is set in the settings window, on the "Applications" page.

### How do I turn it off for one application?

Settings window, "Applications" page: for the program choose "Automatic", "Hotkey only" or "Completely off". Faster: `Ctrl+Alt+P` changes the mode for the focused window, and in the tray icon menu the mode of the current application switches with one click. Details: the [manual](MANUAL.md).

### How do I pause or fully stop the corrections?

Everything is done from the tray icon menu:

- **Pause**: corrections are off and the keyboard works as usual; the same item is then called "Turn on".
- **Fix automatically**: clear the tick and words are fixed only on the correction key.
- **Restart**: the helper starts again, leaving your settings alone.
- **Quit**: the helper stops completely and is unloaded from memory until your next login.

If there is no tray icon, open the settings window (the "PuLinTo" launcher in the application menu): the "Overview" page has a "Fix automatically" switch and shows whether the helper is running. Without the icon, the helper can be fully stopped or restarted by logging out and back in (or with the command `systemctl --user stop pulintod`, see the answer "The keyboard does not respond or a key is stuck").

### I have an Apple keyboard. What should I set?

PuLinTo finds an Apple keyboard by itself and, on the "Overview" page of the settings window, offers the layout you know from a Mac: comma and full stop on Shift+6 and Shift+7, the letter ё on the `\` key. You can accept or decline the offer and turn it on later on the "Apple keyboard" page. With an ordinary keyboard you do not need that page. **If both are connected, the Apple layout applies to both keyboards, because the system has a single layout for all keyboards. You can bring back the usual characters on the ordinary keyboard at any moment with `Ctrl+Pause/Break`** (it turns the Apple layout on again too; on the Apple keyboard `Ctrl+Alt+'` does the same); the correction hotkeys stay separate for each keyboard: `Alt` on the Apple one, `Pause/Break` on the ordinary one. Details: the "Apple keyboard" section of the [manual](MANUAL.md).

### Does it touch passwords and addresses?

No: passwords, addresses, commands, code and words mixing two alphabets are left alone. The rules look at the shape of the word, not at the field it is typed in, so for programs that hold secrets it is safer to turn the mode off completely.

### Is this a keylogger?

The program reads key presses, otherwise it would not notice a word in the wrong layout. But there is no typing log: the daemon stores neither the typed nor the corrected text. Its output holds only the kind of operation, the word length and the cascade stage, and there is no log file at all: the last lines are kept in memory only. The debug mode (the "Diagnostics" page) is off by default, writes to memory only and shows itself with a red dot on the tray icon. A diagnostic report is written to disk only if you press `Ctrl+Alt+\` yourself. The source code is open, so you can check this. What exactly reaches the disk is described in the "Privacy" section of the [manual](MANUAL.md).

### There is no tray icon

The program works regardless. On a plain GNOME (Fedora, RHEL and others) tray icons are shown by the "AppIndicator and KStatusNotifierItem Support" extension (`gnome-shell-extension-appindicator`); Ubuntu already has it. The install command installs it, the `.deb` and `.rpm` packages recommend it, and the helper enables it in GNOME by itself. If there is still no icon, log out and back in: GNOME picks up a new extension only at login. The settings open from the application menu, the "PuLinTo" launcher, and do not need the tray icon.

### The keyboard does not respond or a key is stuck

1. Press `Backspace`, `Esc` and `Enter` together: the keyboard grab is released at once.
2. In the tray icon menu choose **Restart**. If that did not help, choose **Pause** and then **Quit**.
3. If it keeps happening, report it: what you were doing and how loaded the computer was.

The last resort, if there is no icon and the keyboard misbehaves: in a terminal run `systemctl --user stop pulintod`. This command stops the helper completely and unloads it from memory. Details: "If a key is stuck" in the [manual](MANUAL.md).

### The program does not fix words

Check in order:

1. Does the tray icon have an orange space bar? A grey icon, or a "Turn on" item in its menu, means corrections are off: choose "Turn on". No icon: open the settings window (the "PuLinTo" launcher): the "Overview" page shows whether the helper is running and whether auto-correction is on.
2. Choose **Restart** in the icon menu.
3. Is a layout-switching shortcut (for example Alt+Shift) enabled in the system settings? Without it the program cannot switch the layout.
4. Is the program switched off in the window where you type? In terminals and code editors the correction is on the key only by default (`Pause/Break`, `Alt` on Apple).

If nothing helped, copy the last lines of the daemon's work in the settings window ("Diagnostics" → "Copy"). Command-line users can look at the state and the log: `systemctl --user status pulintod` and `journalctl --user -u pulintod -n 50`. Attach the output to your support request. The rest is in the "Troubleshooting" section of the [manual](MANUAL.md).

### How is it different from xneur?

The principle is the same: a word typed in the wrong layout is fixed. The differences:

- xneur does not start under Wayland, PuLinTo works on GNOME with Wayland;

- PuLinTo takes the two previous words into account and, for short words, the next one, so it copes better with words of one or two letters; on long words the programs are almost the same.

### Is this a product of Yandex or Punto Switcher?

No. The project is not affiliated with Yandex and is not a product of Punto Switcher. The idea comes from Punto Switcher; the code, the detection algorithm and the dictionaries are written from scratch.

### Which other languages can be added?

Currently Russian and English. Need another language? Write which language you would like to see in the program and agree to be a tester: we will build the dictionaries ourselves, and what we need from you is to try the language on a real keyboard and tell us how it works. If you can build dictionaries, join in: the format is described in ["Building a language pack"](LANGUAGE_PACK.md). The general picture: ["Other languages and hard words"](LANGUAGES.md).

### Which languages does the interface speak?

The settings window and the tray menu come in Russian and English. By default the language follows the system, otherwise English is shown. You can pick a language by hand at any time: the "Interface language" item in the tray icon menu, or the menu of the settings window (the button with three bars). The choice is remembered and applies to both the tray and the window. For testing, the variable `PULINTO_LANG=ru` or `en` also sets it. Need another interface language? Write to support: a translation is a `po/<language>.po` file of about 280 strings and can be sent in ready-made ([how](LANGUAGES.md)).

### Licence?

Code: MIT or Apache-2.0, your choice. Language data (dictionaries and letter-combination tables): CC BY 4.0, free to use, including in commercial products, with credit. Sources and attribution: ["Data and licences"](../data/ATTRIBUTION.md).

### How do I get in touch?

The Telegram support [@pulinto\_bot](https://t.me/pulinto_bot) (an AI assistant that knows the documentation answers first, the author if needed), email [i.result.man@gmail.com](mailto:i.result.man@gmail.com) or GitHub issues. Send the word the program got wrong any way you like: the dictionaries are improved from such cases.
