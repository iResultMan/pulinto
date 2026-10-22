# Отчёт: чёрный ящик install.sh / uninstall.sh (PuLinTo, коммит 35cc2c4 + рабочее дерево)

Дата: 2026-10-16. Всё выполнялось только в песочнице
`scratchpad/install-test/` (HOME, XDG_CONFIG_HOME, XDG_DATA_HOME внутри неё; PATH =
`stubs:/usr/bin:/bin`). Заглушки `systemctl systemd-run gnome-extensions
update-desktop-database gtk-update-icon-cache loginctl udevadm cargo` пишут аргументы
в `logs/<тест>.stubs` и возвращают 0 (`cargo` — 1, чтобы не собирать; `sudo` — 1).
Настоящие `~/.config`, `~/.local`, systemd и gnome-shell не трогались; pulintod и
pulinto-settings не запускались. Логи каждого прогона: `logs/<имя>.{out,err,stubs}`,
обёртка — `run.sh`. Проверки на живой системе ограничились чтением
`systemctl --user show-environment` и `systemd-analyze --user verify` (оба read-only).

Серьёзность: **блокер** / **важно** / **мелочь**. Блокеров релиза не найдено.

## Что работает (подтверждено)

- `install.sh` без аргументов (t1): раскладка файлов совпадает с README («Установка»,
  стр. 91-95) и с тем, где демон ищет данные (`crates/pulinto-data/src/profile.rs:18-29`:
  `$XDG_DATA_HOME/pulinto/{languages,dict}`, `.fst` по имени языка, `languages/<id>/proto3`).
  Права: бинарники 755, юнит/ярлык/словари 644. Вызовы: `daemon-reload`, `enable`, `restart`.
- `ExecStart` переписан на абсолютный путь `$HOME/.local/bin/pulintod`;
  `systemd-analyze --user verify` на сгенерированном юните — без замечаний (t1/A).
- `--help` (rc 0), неизвестный флаг (rc 2, текст + usage), `--system --autostart-desktop`
  (rc 2, внятный отказ), `--no-enable` (ничего не вызывает), `--autostart-desktop`
  (пишет `~/.config/autostart/pulinto.desktop` с абсолютным Exec, systemctl не вызывается).
- Повторный `install.sh` поверх установленного (t9): md5 всех файлов совпали,
  `replacements.tsv` и `~/.config/pulinto/config.toml` не тронуты; `restart` вызван.
- Запуск из другого каталога `cd / && bash <abs>/install.sh` (t10): rc 0, 21 файл.
- `uninstall.sh` после установки (t11): удалено всё, что ставилось; остались только
  пользовательские `replacements.tsv`, `config.toml` и пустые каталоги. Повторный запуск (t12)
  и запуск на пустом HOME (t13): rc 0, без ошибок. Чужой autostart-файл с другим Exec (t15)
  не удаляется — правильно.
- Без `target/release`: нет `cargo` → «Не найден cargo: установите Rust» rc 1 (t17);
  `cargo` падает → виден только вывод cargo, в HOME ничего не создано (t16) — приемлемо.
- Симлинк `~/.local/share/gnome-shell/extensions/pulinto@pulinto.ru` → чекаут (t22):
  заменяется копией, исходник цел.

## Находки

### 1. uninstall.sh не останавливает демон, запущенный не через systemd — важно
- Файл: `uninstall.sh:50-53` (только `systemctl --user disable --now`), `:58` (rm бинарника).
- Шаги: `install.sh --autostart-desktop` → перелогин (демон запущен из autostart) →
  `uninstall.sh`. Или демон запущен вручную / из трея (`gui/pulinto-settings:101-118`
  сам запускает `pulintod`).
- Факт (статически, t14): вызывается `systemctl --user disable --now pulintod` (юнит не
  включён — ничего не делает), бинарник и autostart-файл удалены, процесс не трогается.
- Ожидание: демон с эксклюзивным захватом клавиатуры остановлен (например, по D-Bus
  `ru.pulinto.Daemon1` или `pkill -x pulintod` от своего uid) либо явное предупреждение
  «демон ещё работает, остановите …». Сейчас после удаления до конца сеанса живёт
  процесс-сирота, удалённый с диска.

### 2. Пути с пробелом или `&` ломают ExecStart/Exec — важно
- Файл: `install.sh:111` (sed `s|^ExecStart=.*|ExecStart=$BIN/pulintod|`), `:121-131`
  (autostart `Exec=$BIN/pulintod`).
- Шаги (t18/t19): `HOME="…/ho me"`, `install.sh --no-enable`; затем
  `systemd-analyze --user verify` юнита.
- Факт: `ExecStart=/…/ho me/.local/bin/pulintod`; systemd: «Command /…/ho is not
  executable». Autostart `Exec=` тоже без кавычек — launcher разобьёт по пробелу.
- Шаги (t24): `HOME="…/h&m"`. Факт: `ExecStart=/…/hExecStart=/usr/local/bin/pulintodm/.local/bin/pulintod`
  (`&` в sed подставляет найденное). Скрипт при этом rc 0 (с `--no-enable`), в обычном режиме
  упадёт на `restart` с ошибкой systemd, не своей.
- Ожидание: ExecStart в кавычках (`ExecStart="…/pulintod"`), Exec по правилам
  desktop-entry (кавычки + экранирование), подстановка без sed-метасимволов
  (например через `printf`/awk или замену `${var//&/\\&}`).

### 3. `--no-enable --autostart-desktop` всё равно включает автозапуск — важно
- Файл: `install.sh:117-130` (условие только `AUTOSTART_DESKTOP = 1`, `ENABLE` не учитывается);
  справка `:21` обещает «ничего не включать и не запускать».
- Шаги (t6): `install.sh --no-enable --autostart-desktop`.
- Факт: `~/.config/autostart/pulinto.desktop` создан → демон стартует при следующем входе.
- Ожидание: при `--no-enable` autostart-файл не пишется (или отказ в сочетании флагов, как
  для `--system --autostart-desktop`).

### 4. Ярлык и трей зависят от PATH сеанса, а не от пути установки — важно
- Файл: `data/ru.pulinto.Settings.desktop:7` (`Exec=pulinto-settings`), `install.sh:106`
  (ставится как есть, без переписывания, в отличие от юнита), `crates/pulintod/src/tray.rs:19`
  (`/usr/local/bin/pulinto-settings`, затем PATH).
- Шаги: пользователь, у которого `~/.local/bin` не было до установки (Ubuntu добавляет его в
  PATH в `~/.profile` только если каталог существует при входе); `install.sh`; клик по
  ярлыку или пункту трея до перелогина.
- Факт (t1): скрипт лишь предупреждает «нет в PATH … добавьте его в PATH» про текущую
  оболочку; ярлык и юнит не несут абсолютного пути к `pulinto-settings`. На этой машине
  `systemctl --user show-environment` содержит `~/.local/bin`, у свежего пользователя — нет.
- Ожидание: `Exec=` в ярлыке переписывать на `$BIN/pulinto-settings`, как ExecStart; в юните
  задавать `Environment=PULINTO_SETTINGS=$BIN/pulinto-settings` (переменную демон уже
  понимает, `tray.rs:15`).

### 5. `WantedBy=graphical-session.target` на X11/XFCE — важно, требует проверки
- Файл: `data/pulintod.service:4-5,12`; `install.sh:136-138` (`enable` + `restart`).
- Суть: README заявляет поддержку X11/XFCE. Если сеанс (lightdm + xfce4-session) не
  активирует `graphical-session.target` в user-инстансе systemd, `enable` ни к чему не
  привязывает старт после входа; `restart` при установке маскирует это — демон работает до
  первого перелогина, потом «не запускается». В песочнице не проверить; нужно
  `systemctl --user is-active graphical-session.target` на XFCE-машине.
- Ожидание: либо проверка `is-active graphical-session.target` в install.sh с подсказкой
  про `--autostart-desktop`, либо `WantedBy=default.target` с ожиданием дисплея внутри демона.

### 6. Повторная установка не чистит устаревшие словари в `dict/` — мелочь
- Файл: `install.sh:92-98` (install -d + копирование по файлам) против `:86-88`
  (`languages/` заменяется с `rm -rf`) и `uninstall.sh:60` (`dict/` удаляется целиком).
- Шаги (t9): положить `~/.local/share/pulinto/dict/stale.fst`, повторить `install.sh`.
- Факт: `stale.fst` остался (а `languages/stale` и `extensions/…/stale.js` удалены).
- Ожидание: единое поведение — либо `dict/` тоже заменяется (каталог «наш», судя по
  uninstall), либо uninstall не трогает его целиком.

### 7. В `dict/` копируются `*.tsv` (6 МБ), демон их не читает — мелочь
- Файл: `install.sh:94` (`*.fst *.tsv`), `:149` («файлов словарей: 6» — считает и tsv).
  Демон читает только `<lang>.fst` (`profile.rs:131`); tsv — исходники для `build_dict`.
- Факт (t1): 8,8 МБ в `dict/`, из них 6,0 МБ tsv; счётчик показывает 6 вместо 3.
- Также из dev-чекаута ставится `it.*` — в `.gitignore` он помечен «не проверен по лицензиям».

### 8. Симлинк `~/.local/share/pulinto/dict` → `data/dict` чекаута: установка падает на середине — мелочь
- Файл: `install.sh:95` (`install -m644` через симлинк).
- Шаги (t21): `ln -s <repo>/data/dict ~/.local/share/pulinto/dict`; `install.sh --no-enable`.
- Факт: `install: … are the same file`, rc 1; бинарники и `languages/` уже скопированы,
  расширение, ярлык и юнит — нет; своего сообщения о частичной установке нет. Если симлинк
  ведёт в другое место — файлы там будут молча перезаписаны.
- Ожидание: как для `languages/` — `rm -rf` + копирование (симлинк снимается, цель цела).

### 9. `--system` без root: отказ только словами sudo — мелочь
- Файл: `install.sh:81` (`sudo -v` под `set -e`), `uninstall.sh:58` (первый `sudo rm`).
- Факт (t5, t23): при отказе sudo rc 1 и ничего от скрипта; в uninstall падение на первом
  `rm`. С настоящим sudo это запрос пароля — приемлемо, но при отмене пользователь видит
  только «sudo: …».
- Ожидание: одна строка «нужны права root (sudo) для --system» перед выходом.

### 10. `uninstall.sh --system` оставляет юниты включёнными у пользователей — мелочь
- Файл: `uninstall.sh:27-35, 58-61`: удаляет `/etc/systemd/user/pulintod.service`, но не
  делает `systemctl --user disable --now` даже для вызывающего пользователя; симлинк в
  `~/.config/systemd/user/graphical-session.target.wants/` остаётся висеть, демон работает.
  Пустой `/usr/local/share/pulinto` не удаляется.

### 11. uninstall не отключает расширение GNOME — мелочь
- Файл: `uninstall.sh:61`. uuid остаётся в `org.gnome.shell enabled-extensions`.
  Ожидание: `gnome-extensions disable pulinto@pulinto.ru 2>/dev/null || true`.

### 12. `--autostart-desktop` всё равно кладёт юнит — мелочь
- Файл: `install.sh:108-112` (юнит ставится всегда), справка `:17` («вместо systemd-юнита»).
- Факт (t4): `~/.config/systemd/user/pulintod.service` присутствует, не включён. Если
  пользователь позже выполнит `systemctl --user enable --now pulintod` (README, стр. 99-107
  описывает только systemd-путь) — два демона; предупреждение `:132-134` срабатывает только
  при следующей переустановке.

### 13. Подсказка про udev с путём без кавычек — мелочь
- Файл: `install.sh:160`: `sudo install -m644 $ROOT/data/70-pulinto.rules …` — при пробеле в
  пути чекаута команду нельзя скопировать. (В песочнице не видна: на машине есть
  `/etc/udev/rules.d/70-pulinto.rules`, условие `:156-157` читает настоящий путь.)

### 14. Расхождения документации — мелочь
- `README.md:326-328` (англ.): «dictionaries must be built first» ↔ `README.md:63-64`
  (рус.): «собирать его не нужно». `git ls-files` подтверждает: `data/dict/{ru,en}.{fst,tsv}`
  в репозитории.
- `CONTRIBUTING.md:13-14`: «The word lists in data/dict are not stored in git» — устарело;
  `docs/STATE.md:164` — то же.
- README не упоминает `uninstall.sh` и флаги `--no-enable`, `--autostart-desktop`, `--build`
  (только CONTRIBUTING упоминает `--no-enable`).
- README советует `sudo ./install.sh --system`: под sudo `XDG_CURRENT_DESKTOP`/
  `XDG_SESSION_TYPE` обычно сброшены (`install.sh:163`) — подсказка про
  `gnome-extensions enable` не печатается.

### Статический разбор (без находок)
- `set -euo pipefail` в обоих скриптах; `bash -n` чист; `shellcheck` на машине нет.
- `mktemp` + `trap EXIT` для юнита — корректно; симлинки `languages/` и расширения
  снимаются `rm -rf`, цель не трогается; `install` над запущенным бинарником безопасен
  (unlink + create). Устаревших имён файлов не найдено: все пути в скриптах существуют
  (`gui/pulinto-settings`, `data/70-pulinto.rules`, `gnome-extension/pulinto@pulinto.ru`).
- Юнит без `WorkingDirectory`: демон стартует в `$HOME` и первым пробует `data/dict`
  относительно cwd (`profile.rs:19,25`) — безвредно.

## Файлы песочницы
- `run.sh` — обёртка запуска; `stubs/` — заглушки; `logs/t*.{out,err,stubs}` — все прогоны
  (t1…t24); `logs/snap-{before,after}.txt` — md5 до/после повторной установки;
  `home*`, `ho me*`, `h&m`, `checkout-nobuild/`, `check out/` — тестовые HOME и чекауты.
