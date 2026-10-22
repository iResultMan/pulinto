# Backlog

Что замечено по ходу работы, но не входило в задачу. Источник в скобках.

## Путь ввода (из разбора 23-code-review-board, 16 октября 2026)

- `BTN_TOUCH` считается щелчком в `xwindow::attach_pointers`: у тачпада это
  палец на панели, любое движение курсора сбросит слово и превратит тап Alt в
  комбинацию. Просто исключить нельзя — tap-to-click приходит тем же
  `BTN_TOUCH`. Нужно отличать касание от нажатия (libinput делает это по
  времени и движению). У заказчика тачпада нет.
- `read_primary` ждёт ответа владельца PRIMARY до 300 мс, блокируя цикл
  клавиатуры; клавиши копятся в очереди. Вынести ожидание из цикла или
  сократить предел.
- Лог «Потерянное отпускание клавиш … восстановлено» не отличает отпускание,
  которое пришло в следующей пачке, от настоящей потери события ядром.
- Пересылка клавиш из очереди после замены идёт с зазором 1 мс между нажатием
  и отпусканием (`FORWARD_GAP`); для Xwayland-окон и Electron стоит измерить,
  не теряются ли такие нажатия, и при необходимости держать клавишу 8 мс.
- Задержка XFIXES-уведомления от mutter при выделении в Wayland-окне не
  измерена: наблюдатель `xfixes` рядом с `wl-paste --primary --watch` в GTK4-,
  Qt- и Electron-окне.
- После правки свежести выделения любая комбинация с Ctrl/Alt/Super считается
  вводом: «выделил, Ctrl+C, Alt» больше не переводит выделение. Если это
  мешает, исключить комбинации, которые не двигают курсор и фокус (Ctrl+C,
  Ctrl+Shift+C).
- `Injector::down` отмечает клавишу нажатой, не зная, принял ли ядро событие;
  после правки «отпускать перед повтором» расхождение остаётся только для
  физического нажатия клавиши, уже числящейся нажатой (потерянное отпускание).

## Стенд (из прогонов с перекрытием, 16 октября 2026)

- Машинистка в режиме `--shuffle` читает раскладку модели через 60 мс после
  своего Alt+Shift; если демон в это время занят заменой (до 400 мс), чорд
  ещё не переслан, модель не переключилась, и машинистка набирает слово под
  неверной верой о раскладке. Ждать смены файла `layout` вместо паузы.
- Слово с ё, набранное латиницей (`nh`[vthyjq` для `трёхмерной`): ключ ё в
  us — обратная кавычка, демон режет слово на ней и переводит только хвост.
  Решить, считать ли `` ` `` буквой, когда вокруг буквы.

## Установка и удаление (агент-тестировщик, песочница, 16 октября 2026)

Полный отчёт — docs/research/23-code-review-board/install-test-report.md;
главное:
- `WantedBy=graphical-session.target` на X11/XFCE требует проверки.
- Осталось из мелочей инсталлятора: `uninstall --system` не отключает юнит у
  других пользователей; `WantedBy=graphical-session.target` на X11/XFCE не
  проверен.

## Стенд, Wayland (16 октября 2026)
- `персонажей, → перcjyf;tq?`: замена us → ru, после трёх букв раскладка снова
  us. Единичный случай на живом экране, трассы нет; повторить прогон с
  `--variants`, при повторе снимать `gsettings monitor` и лог демона рядом.
- Хранитель экрана запирает сессию между прогонами (lock-delay 0, idle 300 с):
  `gnome-session-inhibit` живёт только внутри `run-script.sh`. Для серии
  прогонов держать inhibit на всю серию (как `night-chain.sh`?) или просить
  заказчика отключить замок на время.

## Долг из второго прохода ревью (разделы 9.2–9.6 отчёта, 16 октября 2026)

- `FocusChanges::follow_x11` зовёт `note()` на любое PropertyNotify по
  `_NET_ACTIVE_WINDOW`, не сравнивая значение: WM может переписать свойство
  тем же окном и сбросить слово посреди набора. Сравнивать с прошлым xid.
- `reset_hotkeys` одного действия падает на конфликте с заводской клавишей,
  отданной другому действию.
- `convert_selection` с `CURRENT_TIME` и одним атомом: опоздавший ответ прошлого
  запроса может быть принят за новый (сейчас очередь вычищается `try_recv`
  перед запросом, но ответ, пришедший между вычисткой и отправкой, пройдёт).
- Штамповать только печатающие клавиши: сейчас Caps Lock и клавиши яркости
  тоже гасят выделение.
- Подсказки трея захардкожены при редактируемых привязках.
- `RwLock::unwrap` в `HotkeyStore` на каждой клавише.

## D-Bus (агент-тестировщик, 16 октября 2026; отчёт docs/research/23-code-review-board/dbus-test-report.md)

- Ни одного свойства и сигнала на шине: клиенты опрашивают. Для окна настроек
  и трея нужны сигналы об изменении состояния.
- Журнал не фиксирует успешные изменения настроек по D-Bus (ToggleAuto,
  SetAppMode, словари, хоткеи) — только отказы и ToggleCurrentApp.
- `Judge` с неизвестным `layout_hint` молча берёт профиль 0 (profile.rs:184-196).
- `save()` переписывает файл автозапуска при каждом сохранении настроек; при
  смене `current_exe` (переезд бинарника) `Exec` меняется молча — хотя бы
  писать об этом в лог.
- `ToggleLayout` на GNOME всегда false (Unsupported); `KeyboardInfo`
  показывает apple_layout=true при wanted=false и restart_needed=false —
  уточнить семантику полей.

## Окно настроек (агент-тестировщик, 16 октября 2026; отчёт docs/research/23-code-review-board/gui-test-report.md)

Исправлено сразу: предпросмотр «Станет» без обрезки растягивал диалог на
38 000 px; фильтр поиска переживал смену страницы (сброс уходил новой
странице); иконки `emblem-ok-symbolic` и
`preferences-desktop-keyboard-shortcuts-symbolic` есть не во всех темах
(Yaru / Adwaita), заменены на `object-select-symbolic` и
`preferences-desktop-keyboard-symbolic`. Осталось:
- Тексты «Только по Alt» / «раскладку меняете вы клавишей Alt» ложны для
  PC-набора, где одиночный Alt не назначен (gui/pulinto-settings: 989,
  1036–1037, 729, диалог приложений). Брать название клавиши из демона.
- Валидация в диалогах только на непустоту (664): слово из 2 символов, таб и
  перевод строки дают общее «Правило не сохранено…» после нажатия; показывать
  причину отказа демона (теперь он отвечает `Malformed`/`TooShort`).
- Все вызовы D-Bus синхронные в главном потоке: refresh_all 11+ вызовов, для
  «Всегда менять» N+1 (PreviewLayoutConversion на строку, 537), предпросмотр
  на каждый символ (679–684), time.sleep×30 и Popen(pulintod) при старте
  (91–133). Окно само запускает демон, если его нет — на стенде это мешает.
- `Gtk.StyleContext.add_provider_for_display` (2210) deprecated с GTK 4.10.
- AT-SPI: пункты боковой панели и меню без имён, иконки без имён.

## Тесты
- `replace::tests::a_confirmed_switch_is_not_waited_out` меряет настенное время
  (< 40 мс) и падает под нагрузкой (load average 10 сразу после release-сборки);
  в одиночку стабилен. Считать опросы, а не миллисекунды, либо дать запас.

## Потери под нагрузкой (16 октября 2026)

- Демон не видит `SYN_DROPPED` у захваченной клавиатуры: синхронный поток evdev
  (`sync_stream`) глотает это событие и дописывает компенсирующие нажатия.
  Чтобы писать в лог «ядро выбросило события» и считать такие случаи, нужен
  `RawDevice` (evdev::raw_stream) в `GrabbedKeyboard` и своя сверка по
  `EVIOCGKEY`, которая в `HeldKeys::lost` уже есть.
## Слова с ё в чужой раскладке (важно; стенд Wayland с вариантами, 16 октября)

- Односимвольные слова (инициалы `Г.`, `Е.`) не переводятся по правилу коротких
  слов; на `ru(mac)` вместе с ними не туда уходит и точка (Shift+7 → `&`).

## Выход и перезапуск демона (совет, 16 октября 2026, вечер)

- После `RestartInput` (exec того же бинарника) остаётся сирота `gsettings
  monitor` прежнего образа: его stdout-пайп закрыт по CLOEXEC, процесс умрёт
  только при следующей записи (SIGPIPE). У живого демона их было два. Убивать
  детей перед `exec` или держать `Child` и `kill()` в `flush_output`.
- Тест на `flush_output` с дочерним процессом, держащим пайп, не написан:
  `stamp_output` подменяет fd 1/2 всего процесса, в `cargo test` это ломает
  вывод других тестов. Нужен отдельный бинарный тест или интеграционный.

## После пакета инициалов (совет, 16 октября 2026, ночь)

- Строчные сокращения (`г.`, `т.`, `с.`, `н.`) — второй этап правила
  инициалов по белому списку реальных сокращений (ревьюеры, 13.1): на обычной
  `ru` рискуют `w/`, `c/o`, `b/c`.
- Словарь: у половины прилагательных нет формы на -ую и -ых (срез по частоте);
  генератор полных парадигм прилагательных и причастий с низкой псевдочастотой
  (13.4, путь «б»). Семья основы закрывает 22 из 36, составные и имена — нет.
- `Split` на границе отменяет замену целиком; если научить его переводить
  левую часть и заводить правую как новое слово, на клавише можно брать букву
  смелее (13.4, «структурное»).
- Контекст по-прежнему стирается любым Alt-сочетанием, кроме аккорда
  раскладки (Alt+Tab и т. п.) — это по делу (смена окна), но Ctrl+C/Ctrl+V
  тоже стирают (см. выше «любая комбинация с Ctrl/Alt/Super считается вводом»).
- `crates/pulinto-data/examples/bench_corpus.rs`: неиспользуемый импорт `Path`
  (предупреждение clippy, не наше).
- Миноры ревью 14.5: `dup(1)` и конец трубы на чтение в `stamp_output` без
  `O_CLOEXEC` переживают `exec` (лишние дескрипторы); `Child` окна настроек
  в `tray::open_settings` не ждут — зомби на каждое открытие до выхода демона.
- Семья основы не судит дефисные составные (`природно-территориальных`):
  можно судить по последней части (ревью 14.4).
- Ложные конверсии редких верно набранных русских слов (10 000 слов, 16
  октября): `Сэя` (Trigram), `Вальрас`, `дщерь` (Bigram), `гызы` → `usps`.
  Решение заказчика 17 октября: `usps` остаётся — при конфликте слов двух
  языков побеждает более частотное, 100 % недостижимы; граничные случаи
  обсуждать с заказчиком, а не править молча. Остаётся только вопрос порога
  биграмм на `дщ`/`льр` (фамилии), без спешки.
- Буква на знаковой клавише в середине незнакомого слова (`нейрокомпьютеры`,
  `субурбанизация`, `Мержеевского`): семья основы судит только окончания;
  середину держит лишь префикс словаря — та же дыра словаря, что в 13.4 (б).
- Кодовые ловушки на heldout: `ifdef`, `ifndef`, `libc`, `gcvt` без контекста
  переводятся n-граммами в «шавуа», «дшис» (4 ложных из 350 против 1 в августе,
  когда словарная ступень держала «оба прочтения неизвестны»). Возможное
  лечение: при пустом контексте n-граммы не переводят, если целевое прочтение
  не словарное; обсудить с заказчиком (цена — редкие слова без контекста).
- `genre-chain.sh`/`run-script.sh`: проверка «уже работает pulintod» и
  `restore` смотрят только имя процесса; демон песочницы (с `XDG_CONFIG_HOME`
  в каталоге прогона) принимается за дежурный. Различать по окружению и
  снимать осиротевший демон песочницы перед блоком.
- X11 (XTest-путь): хвост коротких/сомнительных слов не переписывается ни при
  переводе, ни при сохранении следующего слова (на Wayland — переписывается
  через replay_under). Сделать через `xr.replace_word` в обеих ветках (ревью 16.3).
- Дефис теперь внутри слова: смешанные по языку составные (`USB-порт`,
  набранное не в той раскладке) судятся целиком и могут не перевестись там,
  где раньше две половины судились по отдельности. Судить половины дефисного
  токена отдельно, если целиком он не слово ни в одном языке.

## Блок IT на `5ff2` (совет, 18 октября 2026, ночь)

- Стенд: сдача пакета (`flush-req`) не ждёт, пока модель экрана дочитает
  очередь событий. В `genre-it-chunk` под нагрузкой (OCR другого проекта,
  load 7–8 на 8 ядрах) пакет 87 сдан, когда модель прочитала одну букву
  «таблицы»; остаток ушёл в пакет 88 — «lost 2 / лишних» без вины демона
  (по `daemon-out.log` всё переслано вовремя); то же на границе 93/94
  (`AI has been`). Перед `flush-req` ждать, пока `last-event` модели
  догонит время последней клавиши машинистки (или `wait_quiet`).
- `T.` → `Ею` (us, контекст ru; `Епанешникова, В. Teorey`): английский
  инициал с настоящей точкой в us читается на ru(mac) как слово «Ею»
  (ступень короткого слова). Правило-кандидат: заглавная + клавиша, которая
  здесь точка, — инициал в текущей раскладке, не трогать. Обсудить с
  заказчиком.
- Стенд под нагрузкой (load > 20) теряет события и у демона (ядро, 63 на
  331 928), и у модели (`SYN_DROPPED`); прогоны при чужой нагрузке не
  показательны — перед серией смотреть `uptime`, а в `compare.py` печатать
  разницу `typist-keys.log` / `daemon-in.log` как отдельную строку.

## Аббревиатуры и n-граммы (вопрос заказчика про `ШЬФС` → `IMAC`, 18 октября 2026)

- `шьф`, `ьфс` уже в `proto3`; `ШЬФС` остаётся потому, что аббревиатуры
  (≥ 2 букв, все заглавные) не доходят до n-грамм (`LXDE` → `ДЧВУ`), а по словарю
  нужна частота ≥ 100 (`acronym_min_frequency`), у `imac` 4.
- Замер правила «аббревиатура с запрещённой своей n-граммой и чистым чужим
  чтением из букв → перевод» по аббревиатурам сценариев (180 русских, 86
  английских, повторяющиеся): выигрыш — 17 английских, набранных в ru
  (`API`, `ACM`, `ICT`, `AGI`…), и 63 русских, набранных в us (`ГОСТ`, `НЛО`,
  `МРТ`, `ОПЕК`, `СМС`…); цена — 9 русских, набранных намеренно, уйдут в
  латиницу (`ДВС`→`LDC`, `КПД`→`RGL`, `КВН`→`RDY`, `РГПУ`→`HUGE`, `НДФЛ`→`YLAK`,
  `МФТИ`→`VANB`, `НПК`, `ФНС`, `ЕФС`) и 9 английских намеренных — в кириллицу
  (`PCR`→`ЗСК`, `SDS`→`ЫВЫ`, `CNH`→`СТР`, `AFP`→`ФАЗ`…). Вариант с условием
  «чужое чтение — словарное слово» режет цену до 3 + 4, но и выигрыш до
  словарных. Решение заказчика; при любом варианте — heldout и серия.

## Стенд слеп к границам (ревью 20, 18 октября 2026)

- `make_script.tokens()` отбрасывает точку внутри токена, цифры, смешанные
  алфавиты, апостроф и все разделители, кроме `.` и `,` на конце: правила
  последних пакетов (`split_token`, `DOT_SUFFIXES`, 19.1, цифры, `'`↔э) стендом
  не проверяются. Нужен `--raw`: токен целиком, машинистка набирает знаки
  клавишей своей раскладки, пропускает посимвольно то, чего нет на клавиатуре;
  компаратору — класс «граница»; `good()` для IT — 50 % или корзина смешанных
  абзацев. Новая точка отсчёта эталона после этого.
- Запрещённые триграммы ru собраны по исконной лексике и задевают
  транслитерированные имена (`удж` в `Вуджек`, `Виндж`): проверить таблицу на
  именах веб-блоков, прежде чем она даст ложное без знака в тени.

## Латинские аббревиатуры в русской раскладке: IP и класс (анализ 19 октября 2026, код не менялся)

- `шз`/`ШЗ` (IP): двухбуквенное слово без словаря → `Undecided` (n-граммы для ≤2 букв
  не работают); заглавными — ещё и охранник аббревиатур. С любым знаком на конце
  (`шзю`→`ip.`, `шзб`, `шзЖ`) уже переводится биграммами. `шз` запрещено русской
  таблицей, в словаре только в `машзавод*`, в 175 тыс. русских слов корпусов — 0.
- Дефисные смешанные (`ШЗ-адрес`, `ЫЫВ-диск`, `USB-порт`) в пустом/английском
  контексте сейчас **портятся целиком** (`ip-flhtc`, `SSD-lbcr`), в русском — остаются.
  Нужен разрез по дефису: левая половина — латинская аббревиатура из списка, правая —
  слово русского словаря → переводить только левую. 33 встречи на 175 тыс. русских слов.
- Общее правило «запрещённая русская n-грамма ⇒ латиница» **неверно** для заглавных:
  4 из 22 русских аббревиатур с дефисом (`ПЗС-`, `ФАП-`, `ЯМР-`) несут такую n-грамму;
  при открытии охранника ложные 9 из 180 (`ДВС`, `КПД`, `КВН`…). Для строчных
  двухбуквенных безопасно (0 из 8 238). Решение — закрытый список латинских
  аббревиатур (≈60–100: IP AI OS PC UK EU TV UI UX DB ID CI CD VM VR AR; API SQL HTTP
  USB SSD HDD CPU GPU DNS VPN TCP SSH URL HTML CSS XML JSON…) с проверкой коллизий
  кириллического чтения с русскими сокращениями (`PC`↔`ЗС`, `AR`↔`ФК`, `PR`↔`ЗК`,
  `HR`↔`РК`, `CI`↔`СШ`: только при английском контексте). Объём: ~0,1 % слов
  английского технического текста (AI 257, UK 70, IP 21, TV 19, PC 13 на 400 тыс.).

## 19 октября: код-токены и акронимы (после поручения заказчика проверить код)

- **Слайс идентификаторов кода в `en.tsv`** — подготовлен, в `data/dict` НЕ применён (чтобы отчёт «старт против
  сейчас» остался на одном словаре): `data/raw/code-curated.tsv` (655 токенов), `data/raw/code-mined.tsv` (13 047,
  документальная частота ≥ 5 по `/usr/lib/python3`, `/usr/include`, bash-completion, doc), инструмент
  `tools/gen-dict/code_tokens.py`. На копии словаря: ложные переводы кода на cascade-стенде 166 → 9 из 3518
  (без контекста), 66 → 4 (с латинским контекстом); heldout 99,38 → 99,40 %, ложных 8 → 4, пропусков те же 123.
  Остаётся по правилам `data/raw/README.md`: применить, `build_dict 2`, стенд 600 (ru(mac) и plain), свежий веб-блок и
  жанровая серия против `baseline-2026-10-17`; принимать без ухудшений. Хвост неизвестных идентификаторов (редкие
  `rktl`, `xdata`) словарём не закрыть: ~2,5 % по весу, ~5 % по видам.
- **Акронимы и английский контекст:** `HBF` (английский контекст «star in several») → `РИА`: ветка `(None, Some)` ступени
  словарей переводит любой неизвестный латинский акроним, чьё кириллическое чтение — русское слово/сокращение частотой
  ≥ 100 (`acronym_min_frequency`), контекст там не спрашивается. Вариант правила: при согласованном контексте языка
  набора (два латинских слова подряд) акроним остаётся, если его нет в закрытом списке `abbreviations.rs`. Цена: русский
  акроним, набранный латиницей посреди английской фразы, не переведётся; замерить на стенде до принятия.

## Публикация: сверка с xneur (20 октября 2026)

- Лицензия в заголовках `data/languages/{ru,us}/dictionary` приведена к CC BY 4.0 (сделано 20 октября 2026).
- `gen_ngrams.py` не воспроизводит `proto3` из репозитория: исходные корпуса (Leipzig, жанровые) в дереве не лежат, порог и состав источников нигде не записаны. Записать команду и источники рядом с генератором.

## Публикация: что осталось и что замечено (20 октября 2026)

- `LICENSE-CC-BY-4.0` скачан с creativecommons.org (20 октября 2026, sha256 9ba9550a…, 18 657 байт).
- Итальянские таблицы вынесены в `backup/data/languages-it`; движок итальянский не использует (`profile::all` знает только us и ru). Вернуть после тестов и подключения языка.
- Движок жёстко рассчитан на пару `us`/`ru` (`profile.rs`: `all`, `is_cyrillic`, `context_langs`); для новых языков нужно обобщение (описано в `docs/LANGUAGES.md`).
- Адреса репозитория и сайта заменены на `github.com/iResultMan/pulinto` и `iresultman.github.io/pulinto` (20 октября 2026); если имя аккаунта или репозитория изменится, заменить заново: README, Cargo.toml, metadata.json расширения, `tools/build-site.py`, шаблоны `.github`, `docs/promo/*`.
- Публичный счёт «21 300 слов реального текста» был неточным: в корпусе 20 000 слов текста и 1 300 ловушек. Поправлено в README, MANUAL, сайте, `docs/MEASUREMENTS.md`, статьях; в `docs/research/` и `CHANGELOG.md` формулировки старые.
- Скрипт построения `heldout.tsv` и сам файл вне репозитория: цифры 99,38 % нельзя воспроизвести по клонированному репозиторию. Положить генератор (`build_heldout.py`, зерно 137) в `tools/`.
- На сайте нет настоящего скриншота окна настроек (заглушку убрал). Снять скриншот и гифку демонстрации.
- `tools/extract_paradigms.py` (`tools/gen-dict/`) ссылается на `data/dict-variants/archive/…`, которых после переноса в `backup/` в дереве нет.
- В сборке бенчмарка предупреждение `unused import: Path` (`cargo test --workspace`).
- Добавить в CI сборку сайта на pull request (сейчас `pages.yml` собирает только при push в main).
- Git-автор коммитов `PuLinTo <dev@pulinto.local>`: перед публикацией решить, под каким именем и почтой выходят коммиты.

## Установка, значок, бот, помощь (20 октября 2026)

- Поддержка в Telegram (`@pulinto_bot`) ведётся отдельным проектом `~/dev/support-bot`: ИИ-помощник отвечает по `README.md`, `docs/FAQ.md` и `docs/MANUAL.md` этого репозитория (`products/pulinto/refresh-knowledge.sh`), сложные случаи уходят в группу PULINTO SUPPORT. Мой черновик бота перенесён в `backup/tools/feedback-bot`. После правок этих трёх файлов базу знаний бота надо пересобрать и перезапустить службу.
- Рабочий процесс `Release` (`.github/workflows/release.yml`) не запускался: он собирает в AlmaLinux 9, чтобы бинарник работал на RHEL 9. Локально проверены только архив, `.deb` (контейнер Ubuntu 24.04) и `.rpm` (контейнер Fedora), установка и удаление в изолированном домашнем каталоге, скрипт установки одной командой на локальном поддельном сервере релиза.
- Кнопки скачивания и команда на сайте работают только после первого выпуска (тег `v0.1.0` при версии 0.1.0 в Cargo.toml) и включённых в настройках репозитория GitHub Pages (источник: GitHub Actions).
- Значок в трее нарисован как встроенные картинки (`tools/gen-icons.py`, `crates/pulintod/assets/`); третье состояние («только по Alt») не различается, значок зависит только от режима выключено/включено.
- Тесты `pulintod` зависят от данных `data/dict` рядом с бинарником тестов: с `CARGO_TARGET_DIR` вне репозитория тест `a_doubtful_collision_word_follows_the_word_after_it` падает (данных не находит). Это особенность тестов, не ошибка кода; в обычном `target/` проходят все 114.
- Интерфейс не переведён: окно настроек (~309 русских строк), меню трея (28), сообщения демона и CLI (185). Если нужна многоязычность: gettext для GUI на Python, таблица переводов для трея, английский как язык-источник.
- Короткие английские слова в русской раскладке (`in on at it`): предложение по ступеням в сообщении от 20 октября; на реальных текстах 191 тыс. русских слов правило «нет прочтения в языке» дало 21 срабатывание (ин, шт, ру), на 444 тыс. английских обратное направление дало 59 (tv, vs). Ждёт решения.

## Залипание клавиши на живом демоне (21 октября 2026)

- В `~/.local/state/pulinto/pulintod.log` за 20 октября 12 записей «Потерянное отпускание клавиш … восстановлено» (клавиши 125, 57, 34, 106, 48, 18, 47, 20, 105, 57) и одна «Цикл клавиатуры не отвечает больше 3 с: отпущено клавиш — 1» (18:46). Заказчик увидел на экране строку из ~15 одинаковых «ь» (автоповтор залипшей клавиши) и цветные квадраты. Записи совпадают по времени с тяжёлыми сборками и тестами на этой же машине (18:46, 19:05–19:41, 21:14). Напоминание из памяти: при высокой нагрузке демон теряет события. Тяжёлые команды запускать через `nice -n 19 ionice -c3`; проверить приоритет потока ввода и записывать в журнал, как долго клавиша считалась нажатой до восстановления.

## Многоязычность, слова из двух букв, знания бота (21 октября 2026)

- Итальянский перевод интерфейса (`po/it.po`, таблица трея `crates/pulintod/src/i18n.rs`) сделан без носителя языка: нужна вычитка. Английский тоже стоит показать носителю.
- Журнал демона и вывод `pulinto` остаются на русском (185 сообщений); перевод нужен только если пользователи будут читать журнал.
- Живой демон (`target/release/pulintod`, pid 143589) собран до правил для двух букв, нового значка и перевода трея: чтобы они заработали, нужно `cargo build --release` и перезапуск демона. Сделано не было, чтобы не трогать работающую клавиатуру.
- Проверочный корпус почти не содержит английских слов в русском тексте (одна строка из 21 300 изменилась): правило для двух букв проверено на 289 тыс. русских и 448 тыс. английских слов без ошибочных срабатываний, но живой набор «английская фраза в русском тексте» нужно собрать и мерить.
- Бот поддержки (`~/dev/support-bot`) на вопрос «Привет! Ты кто?» передаёт разговор человеку, а на «клавиатура зависла» сразу передаёт, не назвав аварийное сочетание `Backspace+Esc+Enter`, которое есть в базе знаний. Предложение для промпта: сначала дать сочетание, потом передавать.
- Страница «Горячие клавиши» показывает названия клавиатур пользователя: для публичных снимков не использовать.

## Двуязычные документы, закрытые материалы (21 октября 2026)

- Справочные документы двуязычные: заголовок «по-русски / English», ссылка «English version below», русская часть, маркер `<!-- english -->`, затем `## English`. Маркер нужен скрипту знаний бота (`refresh-knowledge.sh` отрезает английскую часть). Новый справочный файл делать в том же виде; `README.md` и `README_EN.md` остаются отдельными файлами.
- `ALGORITHM.md` вынесен в `private/` (не публикуется); ссылки на него из кода заменены на «the algorithm spec». Статьи 2–4 в `docs/promo/articles/` всё равно подробно описывают каскад: решить, публиковать ли их.
- `private/icon-prompts.md` и `private/icon-preview.png`: промпты для генерации значка и превью.
- В истории git (с августа) лежат итальянские таблицы, `data/dict-variants/baseline-2026-10-17/` (с `it.tsv`), прежний `docs/ALGORITHM.md` и рабочие записки: если публиковать с историей, они станут публичными. Предлагается публиковать одним чистым первым коммитом.
- `extract_paradigms.py` перенесён в `backup/tools/` вместе с `ru-paradigm-forms.tsv`: в приложении не используются, 4 800 форм уже внутри `data/dict/ru.tsv`.

- GUI has no restart/stop button: the settings window "Overview" page only shows service state; restart/pause/quit live in the tray menu only. If the tray is missing (plain GNOME), the user has to relog or use `systemctl --user`. Consider a "Restart" button on the Overview page (and in its "no connection" state).

- `pc` rule (owner decision 2026-10-21): `зс`/`ЗС` always become `pc`/`PC` through the two-letter table (`EN_TWO_LETTER`), also in Russian context. Other look-alike abbreviations (`ЗК`=`pr`, `ГШ`=`ui`, `ФК`=`ar`) still need English context. Mixed case `Зс` gives `Pc` — decide if it should be `PC`.
- Language switch: the tray menu is rebuilt when opened, so an open menu keeps the old language until reopened; the settings window follows a change made in the tray when it is activated (restarts on the same page). Adding a fourth language needs: `po/<code>.po`, a table column in `i18n.rs`, `Lang` variant, `LANGUAGE_CODES`/`LANGUAGE_NAMES` in the GUI, an item in the tray `language_menu`.

- xneur leftovers in the benchmark harness: `crates/pulinto-cli/src/bin/bench_corpus.rs` and `crates/pulinto-data/examples/bench_corpus.rs` still read xneur's data directory and its predictions file (a comparison tool, not shipped data). Either move them to backup/ or rewrite them against the project's own data before publication. Docs under `docs/research/` and the descriptive comparison in README/FAQ/MEASUREMENTS also name xneur (the comparison is intended; the research notes are for the owner to decide).
- Language packs (`data/languages/<id>/language.conf`) now carry layouts, alphabet, short words, keep lists, abbreviations, dotted abbreviations and paradigms. Still in code, Russian/English-specific: `translit.rs` (ru↔lat table), the `ё` handling in `ngram_bitmap.rs`, TLD list in `stop_rules.rs`, `is_cyrillic` in `stop_rules.rs`. Same-script pairs (English + German) cannot be told apart by letters: the daemon uses the live layout, but `profile::language_of` (CLI/judge) cannot.
- Per-keyboard Apple arrangement (remapping keys of one keyboard on the fly): considered and rejected by the owner on 2026-10-21 ("somewhere we will stumble"); the way back is the hotkey Ctrl+Pause (Ctrl+Alt+M until 2026-10-21). Separate small fix: Apple keyboards over Bluetooth report vendor 0x004c, only USB 0x05ac is recognised by `capture::APPLE_VENDOR_ID`.

- Daemon texts shown in the settings window are Russian whatever the interface language: validation reasons of the hotkey editor (`hotkeys.rs`: «уже назначено действию…», «без Ctrl, Alt или Super…»), Apple-layout errors (`keyboard.rs`), word-rule rejections (`exceptions.rs` RejectReason). Return codes and let the window translate them.

- `pulinto --version` is not supported (prints usage); add if wanted before 1.0.0 (noticed 2026-10-21).
- `tools/export_public.py` strips only top-level `#[cfg(test)] mod x {` blocks and aborts on any other cfg(test) item; keep new tests in that shape.

- Pre-release security review (session "Pulinto код-ревью с Gemini", full text in docs/research/23-code-review-board.md §21), not fixed yet. Before 1.0.0: R1 typed text in the daemon log (`grab_engine.rs` lines "Токен из двух слов", "Составное…"); R2 log and trace files 0600, dir 0700; R3 `uninstall.sh` leaves `/etc/udev/rules.d/70-pulinto.rules` outside `--system`, packages need `udevadm trigger` in postrm/%postun; R4 D-Bus `AddReplacement` + `SetAppMode` can plant a command (user replacements should not fire on Enter/Tab, no key label in the RecordHotkey refusal, log PID of the caller on rule changes). Accepted risk R5: narrow the udev rule to ID_INPUT_KEYBOARD/MOUSE/TOUCHPAD and state the risk in README, SECURITY.md and the installer prompt. 1.1: R6–R12.

- Decided 2026-10-21: the security work (privileged input helper, caller checks on D-Bus, narrowing/disclosing the udev rule — R4/R5 of the board review) waits for 1.1. Consequence for the debug mode: `StartDebug` can be called by any process of the session; the red icon, the notification and the timers are the only guard until the caller check exists.
- Debug mode: no `dbus` option yet (log of D-Bus calls with the caller PID); it needs the caller-check infrastructure above. The observe/watch modes (`engine.rs`) print counts only, no `debug::text` lines. Stands that start the daemon through a pipe (`| systemd-cat`) no longer get the per-word lines in the journal (they stay in memory; the debug mode records them).
- SECURITY.md / README still say nothing about what the udev rule gives to the user's other programs (R5 of the review).

## Статьи и продвижение (из задачи «серия статей», 21 октября 2026)

- `backup/docs/promo/articles/01-…`, `forum-post.md`, `github-about.md` still carry the xneur comparison figures (0,31 % / 11,41 %) that the owner decided not to publish, and the words «пре-релиз» and «пакетов нет», which are wrong for 1.0.0. Not touched in this task (it covered articles 2–4 only); fix before any of them goes out.
- Article 5 not reviewed against MEASUREMENTS.md.
- README.md and SECURITY.md say nothing about what the udev rule gives the user's other programs; the new articles 3 and 6 say it plainly (see R5 above). Align the public text.

## Russian text outside crates/*/src (English-translation pass, 21 October 2026)

- Not in the scope of that pass: `ibus/pulinto_ibus.py` and most of `tools/**` (stand scripts, `tools/typing-fuzzer/src/typist.rs`) still have Russian comments and messages. Check what `export_public.py` ships and translate those parts.
- `hotkeys_check.py` now starts its daemon with `PULINTO_LANG=ru`, because the refusal texts it reads are translated by the daemon (`i18n::tr`); an English variant of the stand would need the English fragments.

## Code review before release (session "Pulinto code review", 21 October 2026)

- `Ctrl+Alt+P` (toggle the focused application) has the same problem as the old `Ctrl+Alt+M`: grab mode takes it in every application, and IntelliJ uses it (Extract Parameter). Not changed: the owner decided only about the Apple-layout key.
- The tree is not `cargo fmt` clean (`cargo fmt --all --check` lists many files). Running it is a separate formatting-only commit; the reviewer also asked to keep formatting apart from real changes. The rustfmt-only edits already sit in commits `56f988b`…`d60b164`; splitting them out would rewrite local history, not done.
- `Config::save` writes the file in place (`fs::write`); a crash mid-write leaves a truncated config, which `load` reads as defaults. Write to a temporary file and rename.
- The tray tooltip connects to X (`XWindows::connect`) on every refresh; refreshes are now rare (state changes, once a minute while recording), so left as is.

- `tools/gen-dict/code_tokens.py` docstring points to `data/raw/README.md`, which does not exist.
- `install.sh` no longer skips `data/dict/it.*`; the public tree has none, but a local checkout with the untracked `it.fst` would now copy it to `~/.local/share/pulinto/dict` (unused: no `it` language pack).
