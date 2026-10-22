# Чёрный ящик: D-Bus-интерфейс живого pulintod

Дата: 2026-10-16, 13:33–13:48. Демон: pid 2258576, режим `grab`, Wayland (GNOME), экран заперт.
Pid до и после — 2258576, падений, зависаний и таймаутов не было; 10-КБ строки во всех
строковых аргументах обрабатываются за миллисекунды. zbus 4.4.0 (Cargo.lock).

Все команды ниже — через `./d.sh Метод аргументы`, где
`d.sh` = `gdbus call --session --dest ru.pulinto.Daemon1 --object-path /ru/pulinto/Daemon1 --method ru.pulinto.Daemon1.$1 …`.
Полные протоколы: `batch1.out` (только чтение и невалидные аргументы), `batch2.out`
(изменения с откатом), `typeprobe.out` (dbus-send, проверка типов), `hotkey-detail.out`,
`togglecurrent.out`, `monitor.log` (gdbus monitor), `snapshot-before.txt` / `snapshot-after.txt`.

## Что не проверялось (запрет на перезапуск)

- `SetMode` с корректным режимом, `RestartInput`, `SetAppleLayout` — все три делают re-exec демона
  (dbus.rs:51-66, 69-82, 90-110). Проверены только отказы `SetMode` (пустая строка, мусор, 10 КБ,
  лишние аргументы) — все `false` или ошибка сигнатуры, демон не перезапускался.
- Состояние после перезапуска демона: НЕ ПРОВЕРЕНО. Поэтому находки 4–5 про формат файлов
  доказаны содержимым файлов на диске и кодом загрузчика, но не наблюдением после рестарта.
- `ToggleLayout` на этой платформе всегда `false` («Unsupported platform or desktop environment»,
  gnome.rs:92 `set_group` → Unsupported, lib.rs:87-91) — реального переключения не происходит,
  вызывался свободно.

## Находки

### 1. ВАЖНО — методы без аргументов выполняются при любом теле сообщения

Шаги:
```
./d.sh IsAutoEnabled                       → (true,)
./d.sh ToggleAuto "'extra'"                → Warning: Introspection data indicates 0 parameters…  (false,)
./d.sh IsAutoEnabled                       → (false,)      # автозамена выключилась
dbus-send --session --print-reply --dest=ru.pulinto.Daemon1 /ru/pulinto/Daemon1 \
    ru.pulinto.Daemon1.IsAutoEnabled int32:5 boolean:true   → boolean true   # выполнилось
dbus-send … ru.pulinto.Daemon1.GetAppMode int32:5           → Error … Signature mismatch: got `i`, expected `s`
```
Фактически: для методов с параметрами zbus проверяет сигнатуру тела (`SetMode 'off' 'extra'` →
«Signature mismatch: got `ss`, expected `s`»), а для методов без параметров — нет: `ToggleAuto`,
`ToggleLayout`, `IsAutoEnabled` с мусорным телом выполняются. Ожидалось: единообразная ошибка
«Signature mismatch: got `s`, expected ``». Последствие: клиент, который по ошибке передал
аргумент в `ToggleAuto`/`ToggleCurrentApp`/`ClearForgottenWords`/`ResetLearning`, меняет состояние.
Код: все zero-arg методы под `#[interface(name = "ru.pulinto.Daemon1")]` dbus.rs:28; поведение
zbus 4.4.0 — лечится обновлением zbus или явной проверкой сигнатуры тела у этих методов.
Откат: `./d.sh ToggleAuto` → `(true,)`.

### 2. ВАЖНО — ToggleCurrentApp на экране блокировки включает PuLinTo в gnome-shell

Шаги (экран заперт, фокус у gnome-shell):
```
./d.sh GetAppMode "'gnome-shell'"      → ('off',)     # системное правило DEFAULT_OFF
./d.sh ToggleCurrentApp                → ('{"class":"gnome-shell","mode":"auto","enabled":true,"automatic":true}',)
./d.sh ListEnabledApps                 → (['com.anthropic.claude', 'gnome-shell'],)
```
Фактически: один вызов (и тот же путь у горячей клавиши toggle_app, Ctrl+Alt+P —
main.rs:315 `toggle_current_application` → xwindow.rs:673 `cycle` → :542 `next`: Off → Auto)
переводит класс из защитного «выключено по умолчанию» (xwindow.rs:445-470, в т. ч. gnome-shell,
менеджеры паролей, pulinto-settings) прямо в «автоматически» и сохраняет это в
`enabled-apps.txt`. Автозамена начинает работать в поле пароля экрана блокировки.
Ожидалось: для классов из DEFAULT_OFF цикл не должен проходить через Auto одним нажатием
(Off → Manual, либо отказ с сообщением). Лог при этом пишет «PuLinTo для gnome-shell:
Автоматически» — единственная операция из всех, которая вообще логируется (см. 9).
Откат: `./d.sh ResetAppPolicy "'gnome-shell'"` → `(true,)`; список вернулся к исходному.

### 3. ВАЖНО — любое сохранение настроек переписывает Exec в autostart на другой бинарник

Шаги: любой `ToggleAuto`, `SetHotkey`, `ResetHotkeys`, `SetHotkeyChoice` (Config::save,
config.rs:259; файл автозапуска — :267, путь — `daemon_executable()` config.rs:142-151).
```
до:    Exec=/home/medushenko/dev/pulinto/target/release/pulintod   (md5 96c5bcca…)
после: Exec=/usr/local/bin/pulintod                                 (md5 839da4b2…, сборка 14 окт, root)
readlink /proc/$(pgrep -x pulintod)/exe → /home/medushenko/dev/pulinto/target/release/pulintod (deleted)
```
Фактически: во время теста бинарник в target/release был пересобран (13:38), `current_exe()`
стал «… (deleted)», `path.exists()` → false, и `daemon_executable()` молча подставил
`/usr/local/bin/pulintod`. Следующий вход в систему запустит старый установленный бинарник вместо
того, с которым работал пользователь. Ожидалось: автозапуск не трогать при каждом сохранении
настроек (только при смене `autostart`), а при «(deleted)» — брать путь без суффикса.
НЕ ОТКАЧЕНО: файл вне D-Bus, менять напрямую запрещено. Нужно решение заказчика.
Побочно тем же save переписан `config.conf`: устаревшие ключи `report_hotkey`/`toggle_hotkey` и их
комментарии удалены (мигрированы в hotkey.* — значения равны умолчаниям, строк не появилось).
Семантика та же, но файл пользователя переформатирован первым же ToggleAuto.

### 4. ВАЖНО — слово/класс/область с `\n` или `\t` ломают формат файлов на диске

Шаги и факт (never.txt — `слово<TAB>область<TAB>origin`, по строке на правило):
```
./d.sh ExcludeWord '"nl\nword-zzz"'                         → (true,)
cat never.txt    →   nl
                     word-zzz<TAB><TAB>manual               # одна запись в памяти, две строки на диске
./d.sh ExcludeWord '"tab\tword-zzz"'                        → (true,)
cat never.txt    →   tab<TAB>word-zzz<TAB><TAB>manual       # после загрузки: слово "tab", область "word-zzz"
./d.sh ExcludeWordScoped "'nlscope-zzz'" '"app\nline"'       → (true,)
cat never.txt    →   nlscope-zzz<TAB>app
                     line<TAB>manual
./d.sh ListExclusionRules → (['nlscope-zzz\tapp\nline\tmanual', …])   # и строки для GUI неразборны
./d.sh SetAppMode '"zz-line1\nzz-line2"' "'off'"            → (true,)
cat excluded-apps.txt →  zz-line1 / zz-line2 (две строки);  GetAppMode 'zz-line1' сейчас → auto, после рестарта будет off
./d.sh SetAppMode "'# zz-comment'" "'off'"                  → (true,)   # на диске строка-комментарий, после рестарта правило пропадёт
```
Ожидалось: отказ, как делает `validate_rule` для автозамен (autocorrect.rs:261-278 — там
`"a\tb"`→false, проверено). Код: exceptions.rs:362 `normalise` (только to_lowercase),
:308 `save`, :273 `load_list`; xwindow.rs:854 `write_rules`, :836 `load_rules` (отбрасывает `#…`);
scope.rs:19 `AppScope::parse` (режет только по `,`). Все записи удалены, файлы вернулись
к исходному содержимому (md5 совпали).

### 5. МЕЛОЧЬ — пробелы в словах не обрезаются; ForgetWord и ExcludeWord несогласованы

```
./d.sh ExcludeWord "' spaced-zzz'"       → (true,);  ListExclusions → ([' spaced-zzz'],)
./d.sh RemoveExclusion "'spaced-zzz'"    → (false,)
./d.sh RemoveExclusion "' spaced-zzz'"   → (true,)
./d.sh AddReplacement "' spaced-zzz '" "'v'" → (true,);  ListReplacements → ([' spaced-zzz \tv', …])
./d.sh RemoveReplacement "'spaced-zzz'"  → (false,)
```
`load_list` при загрузке делает `trim()` (exceptions.rs:286), `forget_word` — тоже (:587),
а `add_word_exception`/`remove_word_exception` (:362, :394) и `add_user_replacement_scoped`
(autocorrect.rs:91-109) — нет. Правило с пробелом никогда не сработает на набранном слове,
а после рестарта ключ изменится. Ожидалось: `trim()` в `normalise` и в `validate_rule`.

### 6. МЕЛОЧЬ — SetHotkey принимает код клавиши вне диапазона evdev

```
./d.sh SetHotkey "'pc'" "'report'" "['ctrl+alt+65535']"
   → ('{"ok":true,"error":"","warnings":[]}',)
GetHotkeys → "report":{"bindings":[{"text":"ctrl+alt+65535","label":"Ctrl+Alt+65535"}],"is_default":false}
config.conf → hotkey.pc.report = ctrl+alt+65535
```
hotkeys.rs:404 `text.parse::<u16>()` принимает любое число > 0; KEY_MAX = 0x2ff. Ожидалось:
отказ «неизвестная клавиша». Откат: `ResetHotkeys 'pc' 'report'` → is_default true, строки
hotkey.* в конфиге нет. Попутно: буквы пишутся номерами — `['ctrl+alt+r']` сохраняется как
`ctrl+alt+19` (text и config.conf), хотя комментарий конфига обещает запись вида `ctrl+alt+p`
(Chord::written hotkeys.rs:313 / key_name :384). Круговой обход работает, только нечитаемо.

### 7. МЕЛОЧЬ — отказ без причины: `false` вместо ошибки

`SetAppMode 'zzz-app' 'garbage'`, `SetAppMode '' 'auto'`, `SetAppEnabled '' true`,
`ExcludeWord 'ab'`, `AddReplacement '' 'x'` — все `(false,)`. Причина для слов и автозамен
только в stderr/лог («слово короче 3 символов…», «не могут быть пустыми»), для политики
приложений — вообще нигде: dbus.rs:169-176 `map_err(|_| ())` глотает io::Error «неизвестный
режим приложения»/«пустой WM_CLASS». GUI показывает «Правило не сохранено.» (pulinto-settings:877).
Ожидалось: как у хоткеев — JSON `{"ok":false,"error":…}` или `(b, s)` как у `SetAppleLayout`,
либо D-Bus-ошибка. `report_reject` dbus.rs:638 сам признаёт это в комментарии.

### 8. МЕЛОЧЬ — ни одного сигнала и ни одного свойства

`introspect`: `signals:` и `properties:` пусты; `Properties.GetAll ru.pulinto.Daemon1` → `{}`.
`gdbus monitor --dest ru.pulinto.Daemon1` работал все 15 минут при ~80 изменениях состояния
(режим авто ×4, политика приложений, списки слов, автозамены, хоткеи) — `monitor.log` пуст.
Трей, окно настроек и расширение узнают об изменениях только опросом (`Status` в GUI :1944).
Ожидалось: хотя бы `PropertiesChanged` для mode/auto_enabled/choice или собственный сигнал.

### 9. МЕЛОЧЬ — журнал не фиксирует изменения настроек по D-Bus

За всё время в `pulintod.log` добавилось 12 строк: 4× «Слово не добавлено…», 5× «Автозамена не
добавлена…», 2× «Не удалось переключить раскладку…», 1× «PuLinTo для gnome-shell: Автоматически».
Ни одной строки на успешные `ToggleAuto` (×4, включая случайный из находки 1), `SetAppMode`,
`ResetAppPolicy`, `ExcludeWord`/`IncludeWord`/`ForgetWord`, `AddReplacement`, `SetHotkey`,
`SetHotkeyChoice`, `RecordHotkey`/`Cancel`. По логу нельзя восстановить, кто и когда выключил
автозамену или сменил хоткей. Код: dbus.rs:127 `toggle_auto` и соседние методы.

### 10. МЕЛОЧЬ — Judge молча игнорирует неизвестный layout_hint

```
./d.sh Judge "'ghbdtn'" "''" "'zzz'"  → {"convert":true,"text":"привет","layout":"ru","stage":"Dictionary"}
./d.sh Judge "'ghbdtn'" "''" "'ru'"   → {"convert":false,"text":"ghbdtn","layout":"ru","stage":"Trigram"}
```
profile.rs:184-196 `assume_layout`: неизвестный id → `unwrap_or(0)` (профиль us). Ожидалось:
ошибка или явный откат к угадыванию по алфавиту. Остальное у Judge в порядке: пустое слово и
пробелы → Undecided, `p@ssw0rd` и URL → StopRule(Url), 10 КБ слова и контекста — мгновенно.

### 11. Наблюдения (не ошибки, но стоит знать)

- `PreviewLayoutConversion 'ghbdtn мир'` → `ghbdtn vbh`: раскладка угадывается по всей строке,
  конвертируются только символы «текущей» раскладки. Для однословного предпросмотра в GUI ок.
- `KeyboardInfo` → `"apple_layout":true,"wanted":false,"restart_needed":false`: на Wayland
  значение `apple_layout` в config.conf не совпадает с фактическим вариантом ru(mac), и это не
  считается расхождением. Для окна настроек поля противоречивы.
- `CancelHotkeyRecording` всегда `true`, даже когда записи нет; `ForgetWord 'never-seen-zzz'`
  → `true` и сохраняет отпечаток (по дизайну, но счётчик растёт от опечаток).
- `RecordHotkey` в режиме grab: запущен в фоне, через 0,4 с `HotkeyRecordingProgress` → `''`,
  параллельный `RecordHotkey` → `{"status":"unavailable","error":"Запись сочетания уже идёт."}`,
  `CancelHotkeyRecording` → `true`, первый вызов ответил `{"status":"cancelled"}`. Корректно.
- Проверки хоткеев внятные и работают: модификатор как клавиша, голая буква, дубль в действии,
  занято другим действием, неизвестный набор/действие/выбор — все с человекочитаемым `error`.
- Политика приложений: регистр и пробелы класса нормализуются (`' PuLinTo-DBus-Test-ZZZ '`),
  алиасы `alt`/`automatic`/`disabled` принимаются, точное правило бьёт `prefix-*`,
  `ResetAppPolicy` повторно → `false`. Исключения: `Include` после `Exclude` переносит слово между
  списками, `UPPER-ZZZ` хранится строчными, повторное `Exclude` идемпотентно.
- Побочные файлы, которые нельзя убрать через D-Bus: `~/.local/share/pulinto/forgotten.sha256`
  и `replacements.tsv` созданы демоном (сейчас — только строка-заголовок).

## Снимки состояния

`snapshot.sh` снимает все методы чтения (GetMode, IsAutoEnabled, все List*, ForgottenWordCount,
GetHotkeys, HotkeyRecordingProgress, Status без счётчика conversions, KeyboardInfo,
GetAppMode com.anthropic.claude) плюс md5 файлов и текст config.conf.

Diff по D-Bus (`snapshot-before.txt` vs `snapshot-after.txt`, часть до `--- files`): **пуст**.

Diff по файлам (только для полноты; содержимое списков слов и приложений побайтно совпало):
```
2c2   config.conf      b002768581be01f88730475a01aa0299 → 0d8bc72fc941400cad181d27221ddc33   (находка 3: переформатирован, устаревшие ключи сняты)
5a6   + forgotten.sha256  803c7949…   (новый, только заголовок)
      + replacements.tsv  ce007bfe…   (новый, только заголовок)
9c10  autostart/pulinto.desktop  96c5bcca… → 839da4b2…   (находка 3: Exec сменился на /usr/local/bin/pulintod — НЕ восстановлено)
```
Файлы always.txt, never.txt, enabled-apps.txt, excluded-apps.txt, manual-apps.txt, learning.key —
md5 без изменений.
