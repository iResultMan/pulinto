# Исследование №15: Рекомендации по доработке кода PuLinTo и тестового стенда

По результатам кампании фаззинг-тестирования (Исследование №14) сформирован детальный перечень архитектурных изменений, алгоритмических улучшений и рекомендаций по рефакторингу для демона `pulintod` и инструментарии `typing-fuzzer`.

---

## 1. Рекомендации по доработке демона (`pulintod`)

### 1.1. Атомная буферизация ввода во время эмуляции замены (`crates/pulinto-input/src/injector.rs`)

#### Проблема
При высокой скорости ввода (паузы $\le 200\text{ мс}$) физические события нажатия клавиш пользователем попадают в буфер X11/Wayland **посередине** отправки последовательности `Backspace`, переключения раскладки (`Alt+Shift`) и реплея сканкодов. Это приводит к разрыву слов и появлению "текстового мусора" (Класс 1) или потере вводных символов (Класс 2).

#### Решение
Внедрить паттерн **"Replay Suppression Buffer"** (блокировка и накопление событий на время замены).

```rust
// Архитектурная концепция для crates/pulinto-input/src/injector.rs

pub struct InputInjector {
    is_replaying: AtomicBool,
    event_queue: Arc<Mutex<VecDeque<InputEvent>>>,
}

impl InputInjector {
    /// Выполнить замену слова атомно
    pub fn replace_word_atomic(
        &mut self,
        backspace_count: usize,
        target_layout_group: u8,
        scancodes: &[u16],
    ) -> Result<(), InjectorError> {
        // 1. Установить флаг блокировки прямой трансляции
        self.is_replaying.store(true, Ordering::SeqCst);

        // 2. Отправить Backspaces
        for _ in 0..backspace_count {
            self.send_key(KEY_BACKSPACE, true)?;
            self.send_key(KEY_BACKSPACE, false)?;
            thread::sleep(Duration::from_millis(10));
        }

        // 3. Переключить раскладку с синхронизацией
        self.switch_layout_sync(target_layout_group)?;

        // 4. Воспроизвести сканкоды нативной раскладки
        for &code in scancodes {
            self.send_key(code, true)?;
            self.send_key(code, false)?;
            thread::sleep(Duration::from_millis(8));
        }

        // 5. Сбросить флаг блокировки и проиграть накопленный в фоновом потоке буфер
        self.is_replaying.store(false, Ordering::SeqCst);
        self.flush_buffered_events()?;

        Ok(())
    }
}
```

---

### 1.2. Точный расчёт границ стирания слов с префиксами и пунктуацией (`crates/pulinto-core/src/buffer.rs`)

#### Проблема
При наличии знаков препинания в составе ввода (например, `Sintex.bp]znbz`) демон рассчитывал количество `Backspace` только для последнего буквенного токена (`znbz` — 5 символов), оставляя на экране нестираемый префикс `Sintex.bp]`.

#### Решение
Расширить структуру `WordBuffer` для хранения точного количества введённых графических глифов (включая лидирующую и встроенную пунктуацию), относящихся к текущей сессии конвертации.

```rust
// Изменения в crates/pulinto-core/src/buffer.rs

#[derive(Debug, Clone)]
pub struct WordEntry {
    pub raw_scancodes: Vec<u16>,
    pub char_count: usize,       // Полное количество набранных символов буфера
    pub punct_prefix_len: usize, // Длина знаков препинания в начале
}

impl WordBuffer {
    /// Вычислить точное число Backspace для полного удаления ошибочного фрагмента
    pub fn get_erasure_backspace_count(&self) -> usize {
        // Учитывать как буквы, так и небуквенные знаки, попавшие в единую цепочку ввода
        self.current_word.char_count
    }
}
```

---

### 1.3. Синхронизационный барьер при переключении раскладки (`crates/pulinto-input/src/layout_control.rs`)

#### Проблема
В режиме `grab` вызов смены раскладки (через X11 `XkbLockGroup` или uinput `Alt+Shift`) происходит асинхронно по отношению к ядру Linux. Если отправлять сканкоды сразу после `Alt+Shift`, первые 1–2 сканкода успевают уйти в **старой** раскладке до завершения обработки переключения в ОС.

#### Решение
Добавить явный таймаут-барьер и проверку подтверждения текущей активной группы раскладки в ОС:

```rust
// Изменения в crates/pulinto-input/src/layout_control.rs

pub fn switch_layout_with_barrier(&self, target_group: u8) -> Result<(), LayoutError> {
    self.trigger_layout_switch_event()?;
    
    let start = Instant::now();
    let timeout = Duration::from_millis(150);
    
    // Ожидание фактической смены группы в ОС
    while self.get_current_layout_group()? != target_group {
        if start.elapsed() > timeout {
            log::warn!("Layout switch barrier timeout elapsed");
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    
    // Дополнительная задержка стабилизации X11/Wayland буфера
    thread::sleep(Duration::from_millis(15));
    Ok(())
}
```

---

## 2. Рекомендации по доработке тестового стенда (`typing-fuzzer`)

### 2.1. Устранение каскадного сдвига в `compare.py` (Sequence Alignment)

#### Проблема
В текущей версии `compare.py` сверка производится наивным сопоставлением элементов по индексу массива `actual[i] == typed[i]`. При пропуске хотя бы одного слова или разделении токена пунктуацией происходит **каскадный сдвиг индексов** (Класс 5), в результате чего 95%+ верных конвертаций ошибочно помечаются как `ИСПОРЧЕНО`.

#### Решение
Заменить плоскую проверку по индексу на алгоритм **глобального выравнивания последовательностей Нидлмана-Вунша (Needleman-Wunsch Alignment)**.

```python
# Изменения в tools/typing-fuzzer/compare.py

def align_sequences(typed_words, actual_words):
    """
    Глобальное выравнивание последовательности ожидаемых слов (typed_words)
    и реально полученных на экране слов (actual_words) с помощью динамического программирования.
    """
    n, m = len(typed_words), len(actual_words)
    dp = [[0] * (m + 1) for _ in range(n + 1)]

    # Штрафы за вставку / пропуск слова
    GAP_PENALTY = 5

    for i in range(n + 1):
        dp[i][0] = i * GAP_PENALTY
    for j in range(m + 1):
        dp[0][j] = j * GAP_PENALTY

    for i in range(1, n + 1):
        for j in range(1, m + 1):
            w_expected = typed_words[i - 1]["word"]
            w_actual = actual_words[j - 1]

            match_cost = 0 if w_expected == w_actual else levenshtein_distance(w_expected, w_actual)

            dp[i][j] = min(
                dp[i - 1][j - 1] + match_cost,  # Сопоставление / Замена
                dp[i - 1][j] + GAP_PENALTY,     # Пропуск слова в actual (Loss)
                dp[i][j - 1] + GAP_PENALTY      # Вставка лишнего токена (Punctuation split)
            )

    return dp[n][m]
```

---

### 2.2. Задержка стабилизации перед проверкой в `typist.rs`

#### Проблема
В `tools/typing-fuzzer/src/typist.rs` после завершения набора всех слов скрипт завершает работу мгновенно, из-за чего последние 1–2 слова не успевают сохраниться в `sink.log` до остановки процесса.

#### Решение
Добавить явную паузу выравнивания (drain flush pause) 500 мс в конце цикла `typist`:

```rust
// Изменения в tools/typing-fuzzer/src/typist.rs

// В конце работы главного цикла typist:
println!("Typist finished sending all words. Waiting for sink and daemon to settle...");
thread::sleep(Duration::from_millis(500));
```

---

## 3. Итоговый план применения рекомендаций

| Этап | Задача | Модуль | Приоритет | Влияние |
| :---: | :--- | :--- | :---: | :--- |
| **1** | Внедрение `Needleman-Wunsch` в `compare.py` | `tools/typing-fuzzer` | **High** | Ликвидация ложных 98% "испорченных" слов в отчетах сверки |
| **2** | Атомная буферизация на время замены | `crates/pulinto-input` | **High** | Полное устранение текстового мусора (Класс 1) и гонок |
| **3** | Синхронизационный барьер смены раскладки | `crates/pulinto-input` | **Medium** | Ликвидация потери первых букв в режиме `grab` |
| **4** | Учёт пунктуации в границах стирания | `crates/pulinto-core` | **Medium** | Исправление префиксов при конвертации слов с точками/скобками |
