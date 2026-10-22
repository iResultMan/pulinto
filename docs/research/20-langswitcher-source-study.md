# Исследование №20: LangSwitcher по исходникам

Дата: 2026-08-07
Источник: `/home/medushenko/dev/pulinto/competitors/langSwitcher` (репозиторий склонирован с `https://github.com/reg2005/langSwitcher`)
Коммит склонированного репозитория: `0bfebf6b5ef3dc37e1543a386fb67cc207eb08ce` ("docs: add 'Mentioned in Awesome' badge to README", Sun Mar 1 04:50:55 2026 -0800)
Лицензия: MIT (`LICENSE`, копирайт "LayoutSwitcher Contributors", 2025)

Все ссылки на строки даны относительно корня репозитория LangSwitcher (`competitors/langSwitcher/...`).

---

## 1. Есть ли автоматический режим вообще?

Нет. Программа исключительно **ручная, по хоткею**. Никакого фонового наблюдения за вводом (нет ни keylogging, ни постоянного анализа печатаемого текста) в коде нет — это явно декларируется и в README, и в privacy-policy, и подтверждается кодом.

Единственная точка входа в конвертацию — `AppDelegate.performConversion()`, которая вызывается только из обработчика хоткея или из пункта меню "Convert":

- `LangSwitcher/Sources/App/AppDelegate.swift:81` — `func performConversion() {`
- `LangSwitcher/Sources/App/AppDelegate.swift:55-59` — регистрация хоткея вызывает `self?.performConversion()` внутри `Task { @MainActor in ... }`
- `LangSwitcher/Sources/App/StatusBarController.swift:125` — `onConvertAction?()` (пункт меню "Convert" тоже дергает тот же колбэк)

Внутри `performConversion()` есть "умный" (Smart Conversion) режим, но он тоже срабатывает **только по хоткею**, когда нет выделения текста — это не фоновый мониторинг, а fallback-логика внутри того же обработчика хоткея:

- `LangSwitcher/Sources/App/AppDelegate.swift:117-121`:
```swift
} else {
    NSLog("[LangSwitcher] No selected text, trying smart conversion after short delay...")
    usleep(50_000) // 50ms
    performSmartConversion()
}
```

Явное подтверждение в документации:
- `docs/content/en/guide/3.privacy.md:43` — "The Accessibility permission is **never** used to monitor your typing, log keystrokes, or access any data beyond what is needed for the conversion you explicitly trigger."
- `README.md:7` — "Select it, press a hotkey, and LangSwitcher instantly converts it".

Вывод: точка принятия решения о конвертации — это нажатие хоткея (двойной Shift или кастомный), не постоянный анализ потока клавиш.

---

## 2. Как перехватывается ввод

Ввод не перехватывается в смысле CGEventTap/keylogger. Используются два разных механизма для двух разных целей:

**(а) Обнаружение хоткея** — `NSEvent.addGlobalMonitorForEvents` / `addLocalMonitorForEvents` (не CGEventTap):
- `LangSwitcher/Sources/Services/HotkeyManager.swift:41-48` — мониторинг `.flagsChanged` для двойного Shift:
```swift
flagsGlobalMonitor = NSEvent.addGlobalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
    self?.handleFlagsChanged(event)
}
```
- `LangSwitcher/Sources/Services/HotkeyManager.swift:102` и `:112` — мониторинг `.keyDown` для обычного хоткея (modifier+key).

**(б) Получение текста, который нужно конвертировать** — не через слежение за нажатиями, а через буфер обмена: программа сама эмулирует Cmd+C, читает `NSPasteboard`, затем эмулирует Cmd+V с уже преобразованным текстом. Никакой истории нажатых клавиш не ведётся (см. п.7).

Явно заявленный отказ от CGEventTap — авторы объясняют почему:
- `README.md:254` — "**No CGEvent tap**: Avoids `CGEventTap` which requires special entitlements. Instead uses `NSEvent.addGlobalMonitorForEvents` for hotkey detection and `CGEvent` with `.hidSystemState` for keyboard simulation."

Требуемое разрешение — **Accessibility** (`AXIsProcessTrusted`), не Input Monitoring как основное (хотя в комментарии в коде упоминается, что CGEvent может работать и от Input Monitoring):
- `LangSwitcher/Sources/Services/AccessibilityService.swift:11-15`:
```swift
static var hasAccessibilityPermission: Bool {
    let trusted = AXIsProcessTrusted()
    ...
}
```
- `LangSwitcher/Sources/App/AppDelegate.swift:86` — "AXIsProcessTrusted=false. Will attempt conversion anyway (CGEvent may work with Input Monitoring)."
- `README.md:280-284` — "LangSwitcher requires **Accessibility** access to: Read selected text (via simulated `⌘C`); Replace text (via simulated `⌘V`)."
- `README.md:256` — "**No sandbox**: The app requires Accessibility access which is incompatible with App Sandbox."

---

## 3. Как выполняется замена текста (центральный вопрос)

Замена — **буфер обмена + симуляция Cmd+C / Cmd+V** через `CGEvent`, а не через `CGEventKeyboardSetUnicodeString` и не через посимвольный backspace+перенабор.

Ключевое место — `LangSwitcher/Sources/Services/AccessibilityService.swift:28-64`, метод `getAndReplaceSelectedText`:
```swift
// Copy selected text (⌘C)
pasteboard.clearContents()
simulateCopy()
usleep(150_000) // 150ms — give target app time to process ⌘C

guard let selectedText = pasteboard.string(forType: .string), ... 
...
// Paste converted text
pasteboard.clearContents()
pasteboard.setString(convertedText, forType: .string)
simulatePaste()
```

Сама симуляция клавиш Cmd+C/Cmd+V — через `CGEvent(keyboardEventSource:...)` с явным виртуальным кодом клавиши, а не через `CGEventKeyboardSetUnicodeString`:
- `LangSwitcher/Sources/Services/AccessibilityService.swift:174-189` — `postKeyEvent`, создаёт `CGEvent` с `CGEventSource(stateID: .hidSystemState)`, ставит `event.flags`, постит через `event.post(tap: .cgAnnotatedSessionEventTap)`.
- `LangSwitcher/Sources/Services/AccessibilityService.swift:197-200`:
```swift
private func simulateCopy() {
    NSLog("[LangSwitcher] Simulating ⌘C...")
    simulateKeystroke(virtualKey: 0x08, modifiers: .maskCommand) // C key
}
```
- `LangSwitcher/Sources/Services/AccessibilityService.swift:202-205` — аналогично для Cmd+V (virtualKey `0x09`).

До и после операции буфер обмена пользователя сохраняется и восстанавливается (`savePasteboard`/`restorePasteboard`, строки 227-245), с задержкой восстановления в 0.3 сек (`AccessibilityService.swift:59-61`), чтобы не перетереть только что вставленный текст раньше времени.

Для режимов Smart Conversion ("Last Word", "Greedy Line") сначала эмулируется выделение (Shift+Option+Left или Cmd+Shift+Left через тот же `CGEvent`-механизм), а дальше — тот же цикл copy → convert → paste:
- `LangSwitcher/Sources/Services/AccessibilityService.swift:207-217` — `simulateSelectToLineStart` (Cmd+Shift+Left) и `simulateSelectWordLeft` (Shift+Option+Left).

Авторы прямо объясняют выбор этого подхода:
- `README.md:253` — "**Clipboard-based replacement**: Uses `⌘C` -> transform -> `⌘V` approach for maximum app compatibility. Works in virtually any text field."

Никакого прямого вызова `CGEventKeyboardSetUnicodeString` или чего-то похожего на посимвольную симуляцию Unicode-ввода в исходниках не нашёл (grep по `CGEventKeyboardSetUnicodeString` дал ноль совпадений во всём `LangSwitcher/Sources/`).

Замена текста в приложении, где нет выделения и Smart Conversion выключен, вообще не срабатывает — `performConversion` в этом случае просто ничего не делает (`AppDelegate.swift:129-132`, кейс `.disabled`).

---

## 4. Переключает ли программа раскладку системы

Да, переключает — но **только после** конвертации текста, и это отдельная, настраиваемая опция, а не предпосылка для замены символов. Переключение раскладки не требуется для самой замены текста (замена — чисто программная перестановка символов через таблицу), оно нужно лишь чтобы дальнейший набор шёл в правильной раскладке.

Механизм — `TISSelectInputSource` из Carbon Input Source API:
- `LangSwitcher/Sources/Services/KeyboardLayoutDetector.swift:81-103`, метод `switchToLayout`:
```swift
static func switchToLayout(_ targetID: String) {
    guard let inputSources = TISCreateInputSourceList(nil, false)?.takeRetainedValue() as? [TISInputSource] else { ... }
    for source in inputSources {
        ...
        if sourceID == targetID {
            let status = TISSelectInputSource(source)
            if status == noErr {
                NSLog("[LangSwitcher] switchToLayout: switched to '\(targetID)'")
            } else {
                NSLog("[LangSwitcher] switchToLayout: TISSelectInputSource failed with status \(status)")
            }
            return
        }
    }
}
```

Вызывается из `AppDelegate.switchLayoutIfNeeded` (`AppDelegate.swift:227-238`), синхронно, сразу после успешной конвертации — никакого ожидания/поллинга применения не реализовано (нет цикла проверки `TISCopyCurrentKeyboardInputSource` после вызова, нет retry). Статус `TISSelectInputSource` только логируется, но не проверяется повторно и не влияет на дальнейший код.

Режим переключения настраивается через `LayoutSwitchMode`:
- `LangSwitcher/Sources/Services/SettingsManager.swift:34-46` — `always` / `ifLastWordConverted` / `ifAnyWordConverted`, по умолчанию `always` (`SettingsManager.swift:188`, "Default: always").

---

## 5. Есть ли словари, n-граммы, статистика языка?

Нет. Конверсия чисто **табличная**, по позиционному соответствию физических клавиш (QWERTY-позиция как общий индекс между раскладками). Явно заявлено:
- `README.md:44` — "**Zero dependencies** — pure Swift, no external libraries, no dictionaries".

Таблицы — статические словари `[Character: Character]`, построенные позиционным zip двух строк символов:
- `LangSwitcher/Sources/Models/KeyboardLayout.swift:40-48`, русская раскладка:
```swift
static let russian: [Character: Character] = {
    let qwerty = Array("`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?")
    let russian = Array("ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю.Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,")
    var map: [Character: Character] = [:]
    for i in 0..<min(qwerty.count, russian.count) {
        map[qwerty[i]] = russian[i]
    }
    return map
}()
```
- `LangSwitcher/Sources/Services/LayoutMapper.swift:15-22` — конвертация идёт через reverse-lookup по этим таблицам (символ → физическая позиция → символ в целевой раскладке), никакого языкового контекста не используется.

Определение исходной раскладки (`detectSourceLayout`) — тоже не статистическое, а простой подсчёт пересечения множества символов текста с множеством символов раскладки (максимум по score):
- `LangSwitcher/Sources/Services/LayoutMapper.swift:65-89`:
```swift
for layoutID in candidateLayouts {
    guard let map = LayoutCharacterMap.characterMap(for: layoutID) else { continue }
    let layoutChars = Set(map.values)
    let score = text.filter { layoutChars.contains($0) }.count
    ...
}
```

Эвристика "похоже ли слово на неправильную раскладку" (`looksLikeWrongLayout`, `TextConverter.swift:76-129`) тоже не лингвистическая: она просто проверяет, меняется ли скрипт (латиница↔кириллица) при перестановке через ту же таблицу, а не оценивает "читаемость"/вероятность слова. Авторы прямо описывают отказ от per-word gibberish scoring:
- `LangSwitcher/Sources/Services/TextConverter.swift:149-152` — "Key insight: per-word gibberish scoring doesn't work because many wrong-layout words (like "lheu" = "друг") look like valid English (50% vowels, no consonant clusters). Instead, when ALL words on a line switch script, that's strong enough signal to convert everything."

Порог для "конвертировать всю строку" — эвристический процент, не статистика:
- `LangSwitcher/Sources/Services/TextConverter.swift:183-188` — если ≥70% слов "выглядят как неправильная раскладка" и слов ≥3, конвертируется вся строка целиком.

---

## 6. Какие хоткеи и что каждый делает

Основной пользовательский хоткей — один, конфигурируемый, вызывающий `performConversion()`:

1. **Двойной Shift (⇧⇧)** — режим по умолчанию (`SettingsManager.swift:172` — "Default ON"). Реализован через отслеживание `flagsChanged` и измерение интервала между двумя "чистыми" нажатиями Shift (без других модификаторов) с порогом 400 мс:
   - `LangSwitcher/Sources/Services/HotkeyManager.swift:26` — `private let doubleTapInterval: TimeInterval = 0.4`
   - `LangSwitcher/Sources/Services/HotkeyManager.swift:61-88` — логика `handleFlagsChanged`.

2. **Кастомный хоткей (modifier+key)** — запасной вариант, по умолчанию Option+S (`SettingsManager.swift:176` — `0x01` = S key, `179` — `.option`). Пользователь может записать свою комбинацию через `HotkeyRecorderView`. Регистрируется через `HotkeyManager.register` (`HotkeyManager.swift:93-123`).

Оба варианта запускают ровно одно действие — `performConversion()` (см. п.1) — никакой разницы в поведении между способами вызова хоткея нет.

Внутри самого `performConversion()` используются ещё и **служебные** комбинации клавиш, которые программа сама себе эмулирует (не хоткеи пользователя, а инструменты для получения/замены текста через Accessibility):
- Cmd+C / Cmd+V — копирование/вставка (`AccessibilityService.swift:197-205`).
- Cmd+Shift+Left — выделение "до начала строки" для режима Greedy Line (`AccessibilityService.swift:208-211`).
- Shift+Option+Left — выделение "слово влево" для режима Last Word (`AccessibilityService.swift:213-217`).
- Стрелка вправо (без модификаторов) — снятие выделения, если конвертация не потребовалась (`AccessibilityService.swift:219-223`).

Ручной вызов конвертации доступен также из пункта меню статус-бара "Convert" (`StatusBarController.swift:58-59, 125`), который вызывает тот же `onConvertAction` → `performConversion()`.

Документация подтверждает: `docs/content/en/getting-started/3.hotkey.md:6-8` — "By default, LangSwitcher uses **Double Shift** (⇧⇧) ... This is the same hotkey used by popular layout switchers."

---

## 7. Как хранится история набранного

**Долгоживущей истории нажатий/буфера символов не ведётся вообще** — программа не следит за вводом непрерывно, поэтому ей нечего копить между нажатиями хоткея. Единственная "история" появляется постфактум, уже после конвертации, и это не буфер клавиш, а **журнал результатов** (символьный текст, не keycode) — опционально включаемый пользователем и хранимый в SQLite:

- `LangSwitcher/Sources/Services/ConversionLogStore.swift:64-75` — схема таблицы:
```sql
CREATE TABLE IF NOT EXISTS conversion_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp REAL NOT NULL,
    input_text TEXT NOT NULL,
    output_text TEXT NOT NULL,
    source_layout TEXT NOT NULL,
    target_layout TEXT NOT NULL,
    conversion_mode TEXT NOT NULL,
    is_correct INTEGER
);
```
Хранятся именно `TEXT` (уже собранные строки-символы), а не keycode-последовательности.

Логирование **выключено по умолчанию**:
- `LangSwitcher/Sources/Services/SettingsManager.swift:191` — `self.loggingEnabled = defaults.object(forKey: Keys.loggingEnabled) as? Bool ?? false  // Default: OFF`.

Обрезка старых записей — по количеству, не по времени, лимит по умолчанию 100 (0 = безлимит):
- `LangSwitcher/Sources/Services/SettingsManager.swift:192` — "Default: 100"
- `LangSwitcher/Sources/Services/ConversionLogStore.swift:233-261`, `trimToMaxEntries`:
```swift
let sql = """
DELETE FROM conversion_log WHERE id NOT IN (
    SELECT id FROM conversion_log ORDER BY timestamp DESC LIMIT ?
);
"""
```
вызывается из `AppDelegate.logConversion` после каждой удачной конвертации (`AppDelegate.swift:217-221`).

"Оперативного" же буфера набранных клавиш (что печаталось до нажатия хоткея) в коде нет — источник текста для конвертации всегда получается заново через симуляцию Cmd+C/выделения непосредственно в момент нажатия хоткея (см. п.2-3), а не из накопленной истории keydown-событий.

---

## 8. Есть ли исключения по приложениям

В коде — **нет**. Grep по `frontmostApplication|bundleIdentifier|excludeApp|blocklist|blacklist|ignoredApp` внутри `LangSwitcher/Sources/` не дал ни одного релевантного совпадения (единственные вызовы `NSWorkspace` — открытие System Settings по ссылке, `AppDelegate.swift:285`, `PermissionsView.swift:61`, к логике исключений отношения не имеют). Нет ни списка бандл-ID, ни проверки текущего приложения перед конвертацией — `performConversion()` выполняется одинаково независимо от того, какое приложение активно.

Вместо программных исключений — задокументированное ограничение по факту (некоторые приложения просто не отдают текст через Accessibility API, и это не обходится специальным кодом, а описано как limitation):
- `docs/content/en/guide/2.troubleshooting.md:48-55` — "Hotkey doesn't work in a specific app / Some apps (especially those running with elevated privileges or in sandboxed environments) may not respond to Accessibility-based text reading. Known cases: Terminal/iTerm when focused on the command line; Some Electron apps with custom text fields. Try manually selecting the text first, then pressing the hotkey."

Т.е. в терминале/некоторых Electron-приложениях фича просто не работает надёжно (потому что копирование через симулированный Cmd+C не даёт текста), а не потому что приложение явно исключено в коде.

---

## 9. Что авторы пишут про автоматику — пробовали и отказались, или не пробовали

В `docs/` нет отдельного раздела вида "мы пробовали полностью автоматический режим и отказались". Судя по документации и коду, авторы **сознательно спроектировали приложение как хоткей-триггерное** с разными уровнями "автоматизации выделения" (Smart Conversion), но не с автоматическим запуском конвертации без действия пользователя:

- `docs/content/en/getting-started/2.smart-conversion.md:6` — "Smart Conversion lets you convert text **without manually selecting it** first" — то есть "smart" здесь означает "не нужно вручную выделять текст", а не "не нужно нажимать хоткей". Хоткей остаётся обязательным триггером в любом из трёх режимов (`Greedy Line`, `Last Word`, `Disabled`).
- `README.md:251-258` (раздел "Key Design Decisions") — единственное явное архитектурное обоснование, но оно про отказ от **CGEventTap** (из-за требуемых entitlements) и про выбор clipboard-based замены "для максимальной совместимости", а не про отказ от автоматики как таковой.
- Приватность объявлена именно как следствие хоткей-модели: `docs/content/en/guide/3.privacy.md:43` — Accessibility permission "is **never** used to monitor your typing" — это подаётся как фича/гарантия приватности, а не как временное ограничение.
- Есть намёк на то, что часть данных собирается **впрок для будущего ML** (это не автоматический режим набора текста, а сбор датасета решений пользователя о correct/incorrect): `README.md:39` — "Conversion Log — optionally log conversions to a local SQLite database (disabled by default for privacy). Review and label entries (correct/incorrect) for future ML training"; `ConversionLog.swift:5` — "Represents a single text conversion event, stored in SQLite for **data labeling purposes**." Это можно трактовать как задел на будущее (возможно, для более умной эвристики определения раскладки), но явных слов "мы делали автоматический режим и отказались" в докам нет.

Ни файлов вида `CHANGELOG`, ни issue-трекера в локальном клоне нет (issues — на GitHub, не в репозитории), поэтому по текстам issues утверждать ничего нельзя — в клонированных исходниках их просто не нашёл.

---

## Что из этого применимо к Linux

**Переносимо на X11/evdev (принцип, не код):**

1. **Модель "хоткей → действие", а не постоянный анализ потока** (п.1). Это архитектурно нейтральная идея: решение принимается в одной точке по явному триггеру, а не size-of-buffer-эвристиками на каждый keydown. На X11 роль `NSEvent.addGlobalMonitorForEvents`/`flagsChanged` играет XGrabKey/XRecord или слушание evdev-устройства для двойного Shift — сама идея измерения интервала между двумя "чистыми" нажатиями модификатора (`doubleTapInterval = 0.4s`, `HotkeyManager.swift:26,61-88`) переносится один в один, это чистая арифметика по временным меткам событий, не зависящая от macOS API.

2. **Позиционная (QWERTY-slot) таблица раскладок** (п.5, `LayoutMapper.swift`, `KeyboardLayout.swift`) — идея "символ → физическая позиция клавиши → символ в другой раскладке" не привязана к платформе. На X11/evdev это даже проще: можно работать с keycode/scancode напрямую, а не реверс-инжинирить позицию через строку символов, как это вынуждена делать macOS-версия из-за отсутствия прямого доступа к raw keycode на уровне Carbon TIS для сравнения layout-ов.

3. **Эвристика смены скрипта (латиница/кириллица) вместо словаря/n-грамм** (п.5, `TextConverter.swift:107-123`) и порог 70% "плохих" слов для конвертации всей строки (`TextConverter.swift:185-188`) — чисто алгоритмические решения, переносимые как есть. Полезно как референс "минимально достаточной" эвристики без словарей.

4. **Двухпроходный greedy-алгоритм границы (весь текст vs скан справа налево)** (п.5, `TextConverter.swift:137-231`) — общий алгоритмический паттерн, никак не завязан на API macOS.

5. **Сохранение/восстановление буфера обмена вокруг chp операции** (`AccessibilityService.swift:227-245`) и **пауза перед восстановлением** (0.3с) — общий паттерн для clipboard-based замены, применим и в X11 (через `xclip`/`xsel`/прямую работу с X selection), если PuLinTo решит использовать буфер обмена как канал замены.

**Не переносимо / держится на возможностях macOS, которых в X11 нет "из коробки":**

1. **Замена текста через симулированный Cmd+C/Cmd+V + `NSPasteboard`** (п.3) — сама механика Cmd+C/V как надёжного способа "выдернуть выделение из любого приложения" опирается на то, что почти все macOS-приложения одинаково поддерживают системный `NSPasteboard` и стандартные шорткаты копирования. На X11 аналог (симуляция Ctrl+C/Ctrl+V через `XTestFakeKeyEvent` + чтение `CLIPBOARD`/`PRIMARY` selection) в принципе реализуем, но X11-приложения гораздо более рассогласованы в обработке selection (particularly PRIMARY vs CLIPBOARD, и behaviour в терминалах/Java-приложениях/некоторых Qt-приложениях), так что степень "работает почти везде" из README (`README.md:253`) не гарантирована на Linux — это ключевое отличие, которое стоит зафиксировать отдельно, если PuLinTo рассматривает такой путь.

2. **`AXUIElement`/Accessibility API для чтения текста без copy** — в коде фактически не используется для самого чтения текста (используется только `AXIsProcessTrusted`/`AXIsProcessTrustedWithOptions` для проверки/запроса разрешения, `AccessibilityService.swift:11-22`); текст реально читается только через pasteboard. AT-SPI2 в Linux — не полный аналог по покрытию приложений (особенно терминалы и GTK/Qt в разных версиях), так что даже частичное использование Accessibility в macOS-версии не даёт готового рецепта на Linux.

3. **`TISSelectInputSource`/`TISCopyCurrentKeyboardInputSource`** (п.4, `KeyboardLayoutDetector.swift`) — специфичный Carbon API. Аналоги на X11 — `XkbLockGroup`/`setxkbmap`/general IBus/Fcitx API в зависимости от того, использует ли пользователь XKB-группы напрямую или через IM framework (это уже отражено в собственном коде PuLinTo — `crates/pulinto-layout/src/ibus.rs`, `crates/pulinto-layout/src/x11.rs`). Важное наблюдение из чужого кода: LangSwitcher **не ждёт и не проверяет** применение переключения раскладки (нет поллинга после `TISSelectInputSource`, статус только логируется) — то есть даже у референсной реализации это слабое место, и слепо копировать паттерн "выстрелил и забыл" не стоит, если для PuLinTo критична гонка между сменой раскладки и следующим нажатием клавиши.

4. **App Sandbox / notarization специфика** (`README.md:256,362-394`) — вопрос дистрибуции, не архитектуры, для Linux неприменим.

5. **CGEvent с `.hidSystemState`** (п.2-3, `AccessibilityService.swift:174-189`) — конкретный macOS-механизм генерации синтетических событий "в обход" текущего состояния модификаторов (что нужно, чтобы Shift, всё ещё физически зажатый после double-tap-триггера, не просочился в эмулируемые Cmd+C/V). Аналогичная проблема существует и в X11 (`XTestFakeKeyEvent` тоже подвержен наложению реального состояния модификаторов), но решается другими средствами (например, явные keyup синтетических модификаторов или использование XKB core protocol событий вместо XTest) — сам факт проблемы (не породы синтетического события, интерферирующей с реальным состоянием клавиатуры) стоит взять на заметку, а не конкретный API.
