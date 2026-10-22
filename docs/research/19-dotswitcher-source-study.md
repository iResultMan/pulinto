# Исследование №19: dotSwitcher по исходникам

Дата: 2026-08-07
Репозиторий: `competitors/dotSwitcher` (origin `https://github.com/kurumpa/dotSwitcher.git`)
Коммит клона: `199bf72 Merge License into development`
Лицензия: GNU GPL v3 (`competitors/dotSwitcher/LICENSE.txt:1-2`, заголовок в каждом файле — например `Program.cs:6-9` — "This program is free software... GNU General Public License... version 3, or (at your option) any later version")

Автор: Kirill Mokhovtsev / kurumpa, копирайт 2014-2019 (`Program.cs:3`).

Методология: код прочитан целиком (14 файлов .cs), ниже — построчные ссылки на конкретные утверждения. Где грепом не нашлось подтверждения (словари, исключения по приложениям), это указано явно.

---

## 1. Есть ли автоматический режим?

Нет. Программа полностью ручная, управляется только сконфигурированными хоткеями. Точка принятия решения — `SwitcherCore.OnKeyPress` (`Switcher/Switcher.cs:140-180`):

```csharp
if (evtData.Equals(settings.SwitchLayoutHotkey)) { readyToSwitch = true; return; }
...
if (evtData.Equals(settings.SwitchHotkey)) { ConvertLast(); evtData.Handled = true; return; }
if (evtData.Equals(settings.ConvertSelectionHotkey)) { ConvertSelection(); }
```
(`Switcher/Switcher.cs:144,152,159`)

Вся остальная логика метода (строки 164-179) занимается только сопровождением буфера последних нажатий (какие клавиши считать частью "слова"), а не эвристикой типа "эта раскладка неверна — переключим". Автоматического анализа набранного текста (по словарю, по частотам биграмм и т.п.) в коде нет — конверсия запускается исключительно по факту нажатия хоткея.

## 2. Как перехватывается ввод

Низкоуровневые хуки Windows, не RawInput:

- Клавиатура: `WH_KEYBOARD_LL` через `SetWindowsHookEx` — `WinApi/LowLevelAdapter.cs:37-43` (`SetHook`), константа `WH_KEYBOARD_LL = 13` в `WinApi/LowLevelAdapter.Misc.cs:83`. Вызывается из `Switcher/KeyboardHook.cs:49`: `hookId = LowLevelAdapter.SetHook(LowLevelAdapter.WH_KEYBOARD_LL, keyboardEventHook);`
- Мышь: `WH_MOUSE_LL`, константа в `WinApi/LowLevelAdapter.Misc.cs:84`, используется в `Switcher/MouseHook.cs:45`: `hookId = LowLevelAdapter.SetHook(LowLevelAdapter.WH_MOUSE_LL, callback);` — мышь нужна только чтобы клик сбрасывал буфер набранного (`Switcher/Switcher.cs:93-103`, `BeginNewSelection()`).

Хук клавиатуры разбирает `WM_KEYDOWN/WM_SYSKEYDOWN/WM_KEYUP/WM_SYSKEYUP` и достаёт `KEYBDINPUT` через `Marshal.PtrToStructure` (`Switcher/KeyboardHook.cs:86-97`), состояние модификаторов дочитывает через `GetKeyState` (`WinApi/LowLevelAdapter.cs:53-56`, вызовы в `Switcher/KeyboardHook.cs:99-101`).

## 3. Как выполняется замена текста (центральный вопрос)

Два разных механизма для двух хоткеев — оба **не** используют `KEYEVENTF_UNICODE** (грепом по всему исходнику эта константа не найдена вообще — есть только `KEYEVENTF_KEYUP = 0x0002`, `WinApi/LowLevelAdapter.Misc.cs:92`, и `MakeKeyInput` всегда шлёт виртуальный код `Vk`, `Scan` всегда `0`: `WinApi/LowLevelAdapter.Misc.cs:102-119`).

**a) Хоткей "конвертировать последнее слово" — `ConvertLast()` (`Switcher/Switcher.cs:241-255`)**:
```csharp
var selection = currentSelection.ToList();
var backspaces = Enumerable.Repeat<Keys>(Keys.Back, selection.Count);
foreach (var vkCode in backspaces) { LowLevelAdapter.SendKeyPress(vkCode, false); }
Thread.Sleep(settings.SwitchDelay);
LowLevelAdapter.SetNextKeyboardLayout();
foreach (var data in selection) { LowLevelAdapter.SendKeyPress(data.KeyCode, data.Shift); }
```
Это классическая схема **Backspace N раз + переключение раскладки + перенабор тех же виртуальных кодов клавиш**, которые были перехвачены хуком в момент исходного набора (не буфер символов Unicode, а буфер `KeyboardEventArgs` с VK-кодом и признаком Shift — см. вопрос 7). `SendKeyPress` реализован через `SendInput` с виртуальными кодами (`WinApi/LowLevelAdapter.Misc.cs:138-154`), не сканкодами (Scan всегда 0) и не Unicode-инъекцией.

Есть комментарий про костыль под Skype: `Switcher/Switcher.cs:248` `// Fix for skype` перед `Thread.Sleep(settings.SwitchDelay)` — задержка нужна, чтобы приложение успело обработать смену раскладки перед перенабором; в README это же подтверждается как известный баг (см. вопрос 9).

**b) Хоткей "конвертировать выделение" — `ConvertSelection()` (`Switcher/Switcher.cs:205-235`)**:
```csharp
LowLevelAdapter.BackupClipboard();
LowLevelAdapter.SendCopy();
var selection = Clipboard.GetText();
LowLevelAdapter.RestoreClipboard();
...
var keys = new List<Keys>(selection.Length);
for(var i = 0; i < selection.Length; i++) { keys.Add(LowLevelAdapter.ToKey(selection[i])); }
LowLevelAdapter.SetNextKeyboardLayout();
foreach (var key in keys) { if (key != Keys.None) LowLevelAdapter.SendKeyPress(key, (key & Keys.Shift) != Keys.None); }
```
Здесь текст сначала забирается **через буфер обмена** (симулированный `Ctrl+C`, `WinApi/LowLevelAdapter.cs:127-136` `SendCopy`), затем каждый символ переводится в виртуальный код клавиши функцией `ToKey` через WinAPI `VkKeyScanEx(ch, layout)` (`WinApi/LowLevelAdapter.cs:223-233`), после чего раскладка переключается и код клавиш перенабирается. Важно: `BackupClipboard()`/`RestoreClipboard()` (`WinApi/LowLevelAdapter.cs:234-264`) — это **закомментированная заглушка** (весь код тела — комментарии), то есть реального бэкапа/восстановления буфера обмена не происходит несмотря на название функций; это подтверждено и в README как незакрытый TODO (см. вопрос 9, пункт "Restoring clipboard contents").

## 4. Переключает ли программа системную раскладку

Да, переключает — но не через прямой API-вызов, а через **симуляцию хоткея Alt+Shift**. Метод `SetNextKeyboardLayout` (`WinApi/LowLevelAdapter.cs:81-125`):
```csharp
//PostMessage(hWnd, WM_INPUTLANGCHANGEREQUEST, INPUTLANGCHANGE_FORWARD, HKL_NEXT);

var shiftDown = MakeKeyInput(Keys.LShiftKey, true);
var shiftUp = MakeKeyInput(Keys.LShiftKey, false);
var altDown = MakeKeyInput(Keys.LMenu, true);
var altUp = MakeKeyInput(Keys.LMenu, false);

SendInput(2, new INPUT[2] { altDown, shiftDown }, Marshal.SizeOf(typeof(INPUT)));
SendInput(2, new INPUT[2] { altUp, shiftUp }, Marshal.SizeOf(typeof(INPUT)));
```
(`WinApi/LowLevelAdapter.cs:116-124`)

Вариант через `PostMessage(hWnd, WM_INPUTLANGCHANGEREQUEST, ...)` (правильный, "честный" системный API) **закомментирован** (строка 116) — судя по ссылке в README на StackOverflow-вопрос "Can't send WM_INPUTLANGCHANGEREQUEST to some controls" (`README.md:30`), авторы отказались от него из-за того, что не все контролы его принимают, и заменили на грубую эмуляцию системного хоткея Alt+Shift. Это значит, что переключение **полностью зависит от того, что в Windows назначена именно комбинация Alt+Shift для смены раскладки** — иначе `SetNextKeyboardLayout` не сработает.

Перед этим код ищет целевое окно/контрол через `GetGUIThreadInfo`, `AttachThreadInput`+`GetFocus`, `GetForegroundWindow` как фолбэк (`WinApi/LowLevelAdapter.cs:81-114`), но сам объект `hWnd` в итоге не используется в реальной реализации (SendInput глобален, не адресован конкретному окну) — используется только в закомментированной ветке с `PostMessage`.

## 5. Словари, n-граммы, статистика языка

Не нашёл. Грепом по всему `.cs`-дереву на `dictionary|n-?gram|word.?list|wordlist` — совпадений в реальном коде нет (единственные совпадения — закомментированные объявления `Dictionary<string, object>` для буфера обмена, не имеющие отношения к языковой статистике: `WinApi/LowLevelAdapter.cs:219,242`).

Конверсия символов чисто табличная/позиционная — через системную функцию `VkKeyScanEx(char ch, IntPtr dwhkl)` (`WinApi/LowLevelAdapter.Misc.cs:78`, использование в `WinApi/LowLevelAdapter.cs:223-233`), которая для данного символа и раскладки возвращает виртуальный код клавиши, физически producing этот символ в указанной раскладке. Никакой лингвистической эвристики нет вообще — ни для определения языка, ни для решения "когда" конвертировать.

## 6. Хоткеи и что они делают

Три хоткея, интерфейс `ISettings` (`Data/ISettings.cs:23-25`):

| Хоткей | Назначение по умолчанию | Действие |
|---|---|---|
| `SwitchHotkey` | `Pause` (`Data/Settings.cs:35`: `new KeyboardEventArgs(Keys.Pause, false)`) | Конвертирует последнее набранное "слово" — `ConvertLast()` (`Switcher/Switcher.cs:154`) |
| `ConvertSelectionHotkey` | `Shift+Pause` (`Data/Settings.cs:39`) | Конвертирует выделенный текст через буфер обмена — `ConvertSelection()` (`Switcher/Switcher.cs:161`) |
| `SwitchLayoutHotkey` | **не имеет дефолта** — в `Settings.Init()` (`Data/Settings.cs:29-51`) присваиваются дефолты только `SwitchHotkey` и `ConvertSelectionHotkey`, про `SwitchLayoutHotkey` в этом методе ничего нет | Просто переключает системную раскладку без перенабора текста — `SwitchLayout()` (`Switcher/Switcher.cs:236-240`), срабатывает по **отпусканию** клавиши (`onKeyRelease`, `Switcher/Switcher.cs:130-138`), а не по нажатию — так реализована защита от случайного тройного нажатия (`readyToSwitch` флаг выставляется в `OnKeyPress`, строка 144-148, сбрасывается любым другим действием) |

Отдельного хоткея "отмена" (Undo) в коде нет. Фактическая отмена достигается повторным нажатием `SwitchHotkey`: `ConvertLast()` не очищает и не изменяет `currentSelection` после выполнения (`Switcher/Switcher.cs:241-255` — в конце метода нет `BeginNewSelection()` или подобного), поэтому второе нажатие того же хоткея бэкспейсит и перенабирает тот же буфер клавиш ещё раз, переключая раскладку в следующее состояние — то есть у пользователя по факту получается переключение по кругу (обычно "туда-обратно" при двух раскладках), а не выделенная функция отмены.

## 7. Как хранится история набранного

Буфер — `List<KeyboardEventArgs>` `currentSelection` (`Switcher/Switcher.cs:35`). Хранятся не Unicode-символы и не скан-коды, а объекты `KeyboardEventArgs`, несущие **виртуальный код клавиши (`KeyCode`) и признак Shift** — то, что физически перехвачено хуком в момент нажатия (`Data/KeyboardEventArgs.cs:29-47`, поля унаследованы от `KeyEventArgs`). Скан-код (`Scan`) в структуре `KEYBDINPUT`, которую формирует хук, не сохраняется в модели `KeyboardEventArgs` вообще — используется только `Vk` (`Switcher/KeyboardHook.cs:96-97`: `var keybdinput = ...; var keyData = (Keys)keybdinput.Vk;`).

Правила добавления/обрезки буфера, всё в `OnKeyPress` (`Switcher/Switcher.cs:140-180`):
- `Space` без модификаторов **добавляется** в буфер (`vkCode == Keys.Space && notModified) { AddToCurrentSelection(evtData); return; }`, строка 169) — то есть буфер может содержать несколько слов, разделённых пробелами, если не сработали условия сброса.
- `Backspace` без модификаторов удаляет последний элемент буфера (`RemoveLast()`, строка 170, реализация строки 198-202).
- Печатаемый символ (`IsPrintable`, строки 51-60: буквы/цифры, `Oem1..OemBackslash`, numpad, decimal, без Ctrl/Alt/Win) добавляется в буфер; но если предыдущий символ был `Space` и `settings.SmartSelection == false`, буфер сначала полностью очищается (`BeginNewSelection()`, строка 173) — то есть по умолчанию (SmartSelection выкл.) конверсия работает строго на одно последнее слово, а с `SmartSelection` включённым буфер копит несколько слов через пробелы.
- Любая другая клавиша (Enter, Tab, знаки препинания вне диапазона `Oem1..OemBackslash`, стрелки и т.п.) — буфер полностью очищается (`default: BeginNewSelection();`, строка 179).
- Клик мышью (`ProcessMousePress`, строка 93-103) тоже очищает буфер — логично, курсор мог переместиться.
- Клавиши-модификаторы, которые нужно "не мешать" отслеживать (Ctrl, Shift, PrintScreen, NumLock, Scroll — `HaveTrackingKeys`, строки 114-128) не прерывают трекинг сами по себе.

## 8. Исключения по приложениям (IDE/игры)

Не нашёл. Грепом по `exclu|blacklist|whitelist|ignore.*process` по всем `.cs` файлам совпадений нет. Хук глобальный (`WH_KEYBOARD_LL` без фильтрации по процессу/окну — `Switcher/KeyboardHook.cs:46-56`), никакой проверки заголовка окна, класса окна или имени процесса перед обработкой нажатий в коде не встречается. Единственная логика, завязанная на конкретное окно — это поиск целевого `hWnd` внутри `SetNextKeyboardLayout` (`WinApi/LowLevelAdapter.cs:81-114`), но она нужна не для исключения, а для того чтобы определить, какому окну адресовать (в закомментированном варианте) `WM_INPUTLANGCHANGEREQUEST`; в реально работающем коде `SendInput` этот `hWnd` даже не используется.

## 9. Что авторы пишут про автоматику

В `README.md` явного объяснения "почему не сделали автоматику" нет. Позиционирование продукта — намеренная простота и прозрачность в противовес закрытому Punto Switcher:
- `README.md:3`: *"If you're paranoid enough not to use Punto switcher (closed source software)"*
- `README.md:5`: *"Simple keyboard layout switcher. As lightweight as you can easily ensure it doesn't contain any spyware..."*
- Инструкция по использованию прямо описывает ручной процесс: `README.md:9-12`: *"Run / Type a word anywhere / Press Pause/Break until the word is typed in a right layout / Profit!"*

В разделе "Improvements to be implemented" (`README.md:16-21`) про автоматический режим тоже ничего нет — там только рефакторинг, восстановление буфера обмена, скрипт сборки, донат-форма. То есть по документации это не "автоматика, которую не успели сделать", а осознанно ручной инструмент — компромисс в пользу простоты/аудируемости кода, а не забытая фича.

Известный баг про Skype (`README.md:28-30`) прямо подтверждает находку из вопроса 3 и 4: *"If you experience erasing the word and retyping it again in the same layout (as did I, in skype), try to increase the switch delay in settings window (200 ms works for me)"* — то есть переключение раскладки через симуляцию Alt+Shift асинхронно и не гарантированно успевает к моменту перенабора, отсюда `SwitchDelay` и костыль `Thread.Sleep` в `Switcher/Switcher.cs:249`.

GitHub issues самого проекта не входят в склонированный git-репозиторий (это runtime-данные хостинга, не файлы в дереве) — сетевого доступа для их проверки в рамках этой задачи не использовал, поэтому по issues утверждать ничего не буду.

---

## Что из этого применимо к Linux

**Переносимо (принципы, не код):**

1. **Модель "буфер последних нажатых клавиш с VK-кодом+модификатором, а не готовый текст"** (вопрос 7) — сама идея перехватывать нажатия на уровне до применения раскладки и хранить логическую клавишу+модификатор, а не итоговый символ, переносится на X11/evdev один в один: на Linux то же самое даёт evdev-код клавиши (keycode) плюс состояние модификаторов, снятое до/независимо от XKB-трансляции.
2. **Backspace + пересборка через "перевод виртуальных кодов через новую раскладку"** (вопрос 3a) — общий паттерн (стереть N символов, переключить раскладку, эмулировать те же физические клавиши заново) применим и на X11 через `XTestFakeKeyEvent`, и на Wayland через virtual-keyboard/input-method протоколы или `uinput`. Ключевая мысль, которую стоит перенять: не нужно генерировать Unicode напрямую — надо переиграть **те же физические клавиши** после смены раскладки, тогда результат идентичен тому, что ввёл бы пользователь, будь раскладка верной с самого начала. Это устраняет целый класс проблем с приложениями, которые игнорируют синтетический Unicode-ввод.
3. **Позиционная (табличная) конверсия символов через API раскладки** (`VkKeyScanEx`, вопрос 5) — прямой аналог на Linux: `XkbKeycodeToKeysym`/`xkb_state_key_get_one_sym` или обратный маппинг через `libxkbcommon`, что PuLinTo уже, вероятно, делает в `pulinto-layout`. Сам принцип "чистая таблица, никаких словарей" — соответствует и упрощает наш собственный подход, если мы тоже не хотим тащить словари.
4. **Правила сегментации "слова"** (пробел/backspace/прочие клавиши сбрасывают буфер, вопрос 7) — чисто логические правила, не завязаны на ОС, переносятся без изменений.
5. **Ручной, хоткей-driven UX** и обоснование через простоту/аудируемость (вопрос 9) — концептуально применимо как альтернативная стратегия, если решим не гнаться за полной автоматикой.

**Держится на Windows-специфике, аналогов в X11 нет "из коробки":**

1. **`SetWindowsHookEx(WH_KEYBOARD_LL/WH_MOUSE_LL)`** (вопрос 2) — глобальный низкоуровневый хук ввода уровня ОС. В X11 прямого эквивалента с той же семантикой (перехват до всех приложений, включая привилегированные) нет; ближайшие аналоги — XRecord extension (устаревший, требует прав), либо перехват на уровне evdev/`libinput` (нужны права на `/dev/input/*` или демон с CAP_root), что архитектурно совсем другая модель прав доступа, чем в Windows.
2. **Эмуляция раскладки через симуляцию хоткея Alt+Shift вместо системного API** (вопрос 4) — это специфичный костыль вокруг того, что `WM_INPUTLANGCHANGEREQUEST` не всем контролам Windows доходит. Прямого смысла переносить этот конкретный обходной путь на Linux нет: на X11/XKB переключение раскладки — глобальное состояние сервера (`XkbLockGroup`/`setxkbmap`/DBus у GNOME/KDE), не завязанное на конкретное окно так же болезненно, поэтому можно (и нужно) использовать штатный API, а не эмулировать хоткей.
3. **`GetGUIThreadInfo`/`AttachThreadInput`/`GetFocus` для поиска "настоящего" сфокусированного контрола внутри чужого процесса** (вопрос 4, `WinApi/LowLevelAdapter.cs:58-114`) — техника, завязанная на модели потоков сообщений Win32. В X11 аналогичной проблемы (какое окно на самом деле активно внутри процесса) в такой форме не стоит; там скорее актуальна проблема "какое приложение сейчас активно" через `_NET_ACTIVE_WINDOW`/фокус-трекинг оконного менеджера, что уже другая архитектура.
4. **`VkKeyScanEx` как системный сервис "символ → виртуальный код в данной раскладке"** — есть прямой функциональный аналог в libxkbcommon, но именно этот Win32-вызов, конечно, не переносится буквально — только сама идея.
5. **Общий момент об асинхронности переключения раскладки и гонке (баг про Skype, вопрос 9)** — сама проблема (переключение раскладки не гарантированно синхронно с моментом отправки следующих событий) скорее всего актуальна и в X11/Wayland по тем же причинам (межпроцессная асинхронность), так что защитная задержка/подтверждение переключения — универсальный урок, а не Windows-специфика.
