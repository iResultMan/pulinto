# Другие языки и трудные слова / Other languages and hard words

[English version below](#english)

PuLinTo знает русский и английский. Новые языки быстрее всего растут руками тех, кто язык знает,
поэтому проект устроен так, чтобы подключить язык можно было без правки программы: языковой пакет —
это папка с данными. **Если у вас есть опыт со словарями, корпусами текстов или раскладками,
пожалуйста, подключайтесь и давайте дополнять проект вместе.** Что нужно собрать, каким должен быть
пакет и куда его положить, описано в [docs/LANGUAGE_PACK.md](LANGUAGE_PACK.md).

## Как подключиться

**1. Соберите языковой пакет.** Если умеете собирать словари: описание и требования — в
[docs/LANGUAGE_PACK.md](LANGUAGE_PACK.md). Пакет,
положенный в папку с языками, подхватывается при запуске программы, ничего компилировать не
нужно. Подойдёт и неполный пакет: проверка `pulinto pack check` скажет, чего не хватает.

**2. Не умеете собирать словари? Это не нужно, просто напишите.** Если вам нужен другой язык и вы
готовы побыть тестировщиком, скажите, какой язык вы хотели бы видеть в программе, и укажите раскладку
(например, «украинский, `ua`», «немецкий, `de`»). Словари и таблицы мы соберём сами; от вас нужно
главное: попробовать язык на живой клавиатуре и честно сказать, как он работает. Написать можно через
[обращение «Хочу язык»](https://github.com/iResultMan/pulinto/issues/new?template=new_language.yml),
Telegram-поддержку [@pulinto_bot](https://t.me/pulinto_bot) или почту i.result.man@gmail.com. Чем
больше людей просит язык, тем раньше им займёмся.

**3. Проверьте язык на живой клавиатуре.** Нужны носители, которые напечатают обычный текст и пришлют
ошибки. Только живой набор находит то, чего не видят проверочные корпуса: слова с неочевидным
написанием, имена, сленг, смешанные фразы.

**4. Присылайте трудные слова и в русском с английским.** Слово, которое программа исправила
неверно, не исправила или исправила не так: отправьте его с контекстом через
[форму «Трудное слово»](https://github.com/iResultMan/pulinto/issues/new?template=hard_word.yml),
бота или почту. Достаточно строки: что набрали, что получилось, что ожидали.

**5. Если умеете писать код.** Изменения в движке принимаются запросом на слияние, правила описаны в
[CONTRIBUTING.md](../CONTRIBUTING.md). Это необязательно: без кода вы поможете не меньше.

## Языковой пакет

Подключаемый язык — это папка с данными: описание языка, частотный словарь, таблицы запрещённых
сочетаний. Что в ней должно быть, как её собрать, проверить и куда положить, написано в
[«Как собрать языковой пакет»](LANGUAGE_PACK.md).

## Язык интерфейса

Окно настроек и меню трея переведены на русский и английский и по умолчанию следуют языку
системы, иначе показывается английский. Язык можно выбрать вручную в любой
момент: в меню значка в трее («Язык интерфейса») и в меню окна настроек (кнопка с тремя полосками);
выбор запоминается и действует и там, и там. Исходные строки программы английские. Добавить свой
язык интерфейса можно без кода:

1. Скопируйте `po/pulinto.pot` в `po/<код>.po` (например, `po/de.po`) и впишите перевод в строки
   `msgstr`. Слова в `{фигурных скобках}` менять нельзя.
2. Проверьте, что переведена каждая строка и слова в `{фигурных скобках}` остались на месте.
3. Пришлите файл любым способом: запросом на слияние, обращением или письмом.

Меню трея переводится отдельно, таблицей в `crates/pulintod/src/i18n.rs` (около 80 строк); если
вы не пишете на Rust, переведите только `.po`, меню трея и пункт выбора языка добавлю я.

## Как мы делаем язык

- **Источники только совместимые с CC BY 4.0.** Условия те же, что в требованиях к пакету; все источники записаны в `data/ATTRIBUTION.md`.
- **Язык выходит после замера.** Как для русского и английского: точность и доля ложных срабатываний на проверочном корпусе.
- **Не тащим всё на себе.** Я собираю то, что могу собрать сам, но язык лучше знают те, кто на нём пишет. Если вы такой человек, напишите.

<!-- english -->

---

## English

PuLinTo knows Russian and English. New languages grow fastest in the hands of people who know the
language, so the project is built to let a language be added without changing the program: a language
pack is a folder of data. **If you have experience with dictionaries, text corpora or keyboard layouts,
please join in and let us extend the project together.** What to build, what a pack must look like and
where to put it is in [docs/LANGUAGE_PACK.md](LANGUAGE_PACK.md).

### How to take part

**1. Build a language pack.** If you can build dictionaries: the description and the requirements are
in [docs/LANGUAGE_PACK.md](LANGUAGE_PACK.md). A pack put in the languages folder is picked up when the program starts; there is nothing to
compile. An incomplete pack is fine: `pulinto pack check` tells you what is missing.

**2. Cannot build dictionaries? You do not need to, just write.** If you need another language and are
ready to be a tester, tell us which language you would like to see in the program and give the layout (for
example "Ukrainian, `ua`", "German, `de`"). We will build the dictionaries and tables ourselves; what we
need from you is the main thing: to try the language on a real keyboard and tell us honestly how it works.
You can write through the [issue "I want a language"](https://github.com/iResultMan/pulinto/issues/new?template=new_language.yml),
the Telegram support [@pulinto_bot](https://t.me/pulinto_bot) or email i.result.man@gmail.com. The more
people ask for a language, the sooner we start on it.

**3. Check the language on a real keyboard.** We need native speakers who type ordinary text and send the
mistakes. Only live typing finds what the test corpora miss: words with unobvious spelling, names, slang,
mixed phrases.

**4. Send hard words in Russian and English too.** A word the program fixed wrongly, missed or fixed in
a strange way: send it with its context through the
[Hard word form](https://github.com/iResultMan/pulinto/issues/new?template=hard_word.yml), the bot or
email. One line is enough: what you typed, what you got, what you expected.

**5. If you can write code.** Changes to the engine are accepted as pull requests; the rules are in
[CONTRIBUTING.md](../CONTRIBUTING.md). This is optional: without code you help just as much.

### Language pack

A language you add is a folder of data: the description of the language, the frequency dictionary and
the tables of forbidden combinations. What it must contain, how to build it, check it and where to put
it is in ["Building a language pack"](LANGUAGE_PACK.md).

### Interface language

The settings window and the tray menu are translated into Russian and English and follow the system
language by default, otherwise English is shown. You can pick the language by hand at any
time: in the tray icon menu ("Interface language") and in the menu of the settings window (the button
with three bars); the choice is remembered and applies to both. The strings in the program are English.
You can add your own interface language without code:

1. Copy `po/pulinto.pot` to `po/<code>.po` (for example `po/de.po`) and fill in the `msgstr` lines.
   Words in `{braces}` must stay as they are.
2. Check that every string is translated and the words in `{braces}` are in place.
3. Send the file any way you like: a pull request, an issue or an email.

The tray menu is translated separately, by a table in `crates/pulintod/src/i18n.rs` (about 80 strings);
if you do not write Rust, translate only the `.po` and I will add the tray menu and the language item.

### How we build a language

- **Only sources compatible with CC BY 4.0.** The same terms as in the requirements for a pack; all the sources are listed in `data/ATTRIBUTION.md`.
- **A language is released after it is measured.** As for Russian and English: accuracy and the share of false positives on a test corpus.
- **We do not carry everything alone.** I build what I can build myself, but the language is best known by those who write in it. If you are such a person, write to me.
