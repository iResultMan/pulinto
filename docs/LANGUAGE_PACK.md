# Как собрать языковой пакет / Building a language pack

[English version below](#english)

Язык в PuLinTo подключается папкой с данными, правки программы не нужны. Эта страница для тех, кто
умеет работать со словарями, корпусами текстов и раскладками: здесь сказано, из чего состоит пакет,
как его собрать, проверить и отдать. Пакет, положенный в папку с языками, подхватывается при
запуске программы. Общая картина и приглашение к работе: [«Другие языки и трудные слова»](LANGUAGES.md).

## Какой должен быть языковой пакет

Пакет — папка, название которой равно идентификатору раскладки XKB (`ua`, `de`, `el`):

```
languages/<id>/
  language.conf     описание языка (обязательно)
  proto, proto3     запрещённые сочетания из 2 и 3 букв, по одному в строке
  <словарь>.fst     частотный словарь (можно положить в соседнюю папку dict/)
  dictionary        правила «слово этого языка»: регулярные выражения, по одному в строке (необязательно)
```

**Куда положить.** В `~/.local/share/pulinto/languages/<id>/` (для одного пользователя) или в
`/usr/local/share/pulinto/languages/<id>/` (для всех). Демон просматривает эти папки при запуске:
после копирования выберите в меню значка в трее «Перезапустить». Пакет с идентификатором `ru` или
`us` заменяет встроенный. Новый язык участвует в исправлениях, только если его раскладка включена в
настройках системы, чтобы не предлагать слова тем, кто на нём не печатает. Установка и обновление
программы чужие пакеты не трогают.

**Что в `language.conf`.** Строки вида `ключ = значение`; строка с `#` в начале — комментарий; ключи
со списками можно повторять, значения складываются; слова в списках разделяются пробелами.

| Ключ | Обязателен | Что это |
|---|---|---|
| `id` | да | идентификатор раскладки XKB, строчные латинские буквы (`ua`); равен названию папки |
| `name` | да | название языка |
| `dictionary` | нет | имя частотного словаря, `<имя>.fst`; по умолчанию равно `id` |
| `alphabet` | да | строчные буквы языка одной строкой, без повторов |
| `fold` | нет | буквы, которые при поиске коротких слов считаются другими: `ё=е` |
| `short1` | да | настоящие слова из одной буквы (русский: `а в и к о с у я…`) |
| `short2` | да | настоящие слова из двух букв |
| `keep` | нет | двухбуквенные формы, которые правило «слово без контекста» трогать не должно (чатовые слова вроде `ща`, сокращения); ещё не в `short2` |
| `abbreviations` | нет | латинские и иные сокращения этого языка, которые люди набирают в чужой раскладке (`ip`, `sql`): исправляются везде, если набранное не слово |
| `abbreviations.after_context` | нет | то же, но только после двух слов этого языка, когда те же клавиши в другом языке дают своё слово (`PC` — это ещё и «ЗС»); только заглавными |
| `dotted` | нет | сокращения, которые пишутся с точкой (`стр`, `ул`): исправляются вместе с точкой |
| `paradigm` | нет | окончания одной парадигмы; помеченные `*` кончаются буквой, которая в другой раскладке сидит на клавише со знаком препинания (`ую*`); пустое окончание пишется `-` |
| `layout.name`, `layout.lower`, `layout.upper` | да | название и клавиши стандартной раскладки |
| `layout.<вариант>.name`, `.lower`, `.upper` | нет | вариант раскладки, чьи клавиши отличаются (у русского `mac` — Apple) |

**Раскладка.** `layout.lower` и `layout.upper` — по 47 символов: что печатают без Shift и с Shift
клавиши основного блока, ряд за рядом, как на американской клавиатуре: `` ` `` `1…0` `-` `=`
(13 клавиш), `q…p` `[` `]` `\` (13), `a…l` `;` `'` (11), `z…m` `,` `.` `/` (10). Проверить: `pulinto
convert --from us --to <id> "qwerty"` должно напечатать то, что даёт эта раскладка на тех же
клавишах.

**Словарь.** Файл `слово<TAB>число`, по одному слову в строке, по порядку байтов слова (`LC_ALL=C sort`): частоты слов
реального текста. Как его собрать из корпуса предложений: разрезать текст на слова из букв
`alphabet` (внутри слова допустимы дефис и апостроф), привести к нижнему регистру, посчитать и
оставить слова, которые встретились не реже трёх раз (порог отсекает опечатки и мусор исходного
текста). Идентификаторов, чисел и слов с чужими буквами в словаре быть не должно. Готовый файл
превращается в быстрый индекс командой `pulinto build-fst <id>.tsv <id>.fst`.

**Запрещённые сочетания.** `proto` и `proto3`: сочетания из двух и трёх букв алфавита, которых внутри
слов не бывает, по одному в строке. Их получают из того же корпуса: берут все возможные сочетания и
вычёркивают те, что встретились внутри слов не реже трёх раз; остальные запрещены. Сочетания считаются
только внутри слов, не через пробел.

**Проверка.** `pulinto pack check languages/<id>` сообщает все ошибки формата сразу, с номерами
строк, и напоминает, чего не хватает рядом с пакетом. Слова можно прогнать так: `pulinto judge --layout <id> слово…`.

**Лицензия и источники.** Данные пакета должны быть доступны на условиях CC BY 4.0 или свободнее
(общественное достояние, CC0, CC BY, MIT, Apache-2.0). Не принимаются данные с запретом
коммерческого использования (CC BY-NC), с условием «на тех же условиях» (CC BY-SA, GPL), чужие
готовые частотные списки и словари. Источники и условия записываются в `data/ATTRIBUTION.md`; без
этого пакет не попадёт в поставку.

**Качество.** Ориентир для встроенного языка: на тексте, набранном
в правильной раскладке, не больше 0,1 % испорченных слов (у русского и английского сейчас 0,06 %),
методика описана в [«Замерах и методике»](MEASUREMENTS.md). Нужны ещё проверка носителями на живой
клавиатуре и список трудных слов.

**Как прислать.** Папку `data/languages/<id>/` и словарь в `data/dict/` запросом на слияние (с
записью в `data/ATTRIBUTION.md`) или архивом: обращением, ботом или почтой. Я проверю, соберу замер и
добавлю язык в поставку, когда его проверят вживую.

**Что пока не вынесено в пакет.** Это три места, где язык ещё зашит в код: транслитерация (одно из
прочтений, которые перебирает клавиша исправления) знает только русский; особый учёт `ё` в таблицах сочетаний есть только у русского;
раскладка Apple (`ru(mac)`, `us(mac)`) сделана только для русской и английской. Команда `pulinto judge`
без `--layout` угадывает раскладку по буквам, поэтому для языков с общим алфавитом указывайте
`--layout <id>`; сам демон опирается на включённую раскладку.

<!-- english -->

---

## English

A language is added to PuLinTo as a folder of data; the program needs no changes. This page is for people
who know how to work with dictionaries, text corpora and keyboard layouts: it says what a pack consists
of, how to build it, check it and hand it over. A pack put in the languages folder is picked up when the
program starts. The general picture and the invitation to take part: ["Other languages and hard
words"](LANGUAGES.md).

### What a language pack must be

A pack is a folder named after the XKB layout id (`ua`, `de`, `el`):

```
languages/<id>/
  language.conf     the description of the language (required)
  proto, proto3     forbidden combinations of 2 and 3 letters, one per line
  <dictionary>.fst  the frequency word list (it may sit in the neighbouring dict/ folder)
  dictionary        "a word of this language" rules: regular expressions, one per line (optional)
```

**Where to put it.** In `~/.local/share/pulinto/languages/<id>/` (for one user) or
`/usr/local/share/pulinto/languages/<id>/` (for all users). The daemon looks through these folders when it
starts: after copying choose "Restart" in the tray icon menu. A pack with the id `ru` or `us` replaces the
built-in one. A new language takes part in the corrections only when its layout is enabled in the system
settings, so that words are not offered to people who do not type it. Installing or updating the program
does not touch packs of your own.

**What is in `language.conf`.** Lines like `key = value`; a line starting with `#` is a comment; list keys
may be repeated and the values add up; words in lists are separated by spaces.

| Key | Required | What it is |
|---|---|---|
| `id` | yes | the XKB layout id, lower case ASCII (`ua`); the same as the folder name |
| `name` | yes | the name of the language |
| `dictionary` | no | the name of the frequency list, `<name>.fst`; defaults to the `id` |
| `alphabet` | yes | the lower case letters of the language in one string, no repeats |
| `fold` | no | letters looked up as another letter in the short-word lists: `ё=е` |
| `short1` | yes | the real one-letter words (Russian: `а в и к о с у я…`) |
| `short2` | yes | the real two-letter words |
| `keep` | no | two-letter forms the "word with no context" rule must leave alone (chat words such as `ща`, abbreviations); not already in `short2` |
| `abbreviations` | no | abbreviations of this language that people type on another layout (`ip`, `sql`): fixed anywhere when what was typed in another layout is not a word |
| `abbreviations.after_context` | no | the same, but only after two words of this language, when the same keys give a word of their own in the other language (`PC` is also the Russian "ЗС"); capitals only |
| `dotted` | no | abbreviations written with a full stop (`стр`, `ул`): fixed together with the stop |
| `paradigm` | no | the endings of one paradigm; those marked `*` end in a letter that the other layout keeps on a punctuation key (`ую*`); an empty ending is written `-` |
| `layout.name`, `layout.lower`, `layout.upper` | yes | the name and the keys of the standard layout |
| `layout.<variant>.name`, `.lower`, `.upper` | no | a variant whose keys differ (for Russian, `mac` is the Apple variant) |

**The layout.** `layout.lower` and `layout.upper` are 47 characters each: what the keys of the main block
print without and with Shift, row by row, as on a US keyboard: `` ` `` `1…0` `-` `=` (13 keys), `q…p` `[` `]`
`\` (13), `a…l` `;` `'` (11), `z…m` `,` `.` `/` (10). To check: `pulinto convert --from us --to <id>
"qwerty"` must print what the layout gives on the same keys.

**The dictionary.** A file `word<TAB>count`, one word per line, in byte order of the word (`LC_ALL=C sort`): the frequencies of words in
real text. How to build it from a corpus of sentences: cut the text into words of the letters of
`alphabet` (a hyphen and an apostrophe are allowed inside a word), make them lower case, count them and
keep the words that occurred at least three times (the threshold cuts the typos and junk of the source
text). A dictionary must hold no identifiers, numbers or words with foreign letters. The finished file
becomes a fast index with `pulinto build-fst <id>.tsv <id>.fst`.

**Forbidden combinations.** `proto` and `proto3`: combinations of two and three letters of the alphabet that
never occur inside words, one per line. They come from the same corpus: take every possible combination and
strike out those that occurred inside words at least three times; the rest are forbidden. Combinations are
counted inside words only, not across a space.

**Checking.** `pulinto pack check languages/<id>` reports every format error at once, with line numbers,
and reminds you what is missing beside the pack. Words can be tried with `pulinto judge --layout <id> word…`.

**Licence and sources.** The data of a pack must be available under CC BY 4.0 or more freely (public
domain, CC0, CC BY, MIT, Apache-2.0). Not accepted: data that forbids commercial use (CC BY-NC),
share-alike data (CC BY-SA, GPL), third-party ready-made frequency lists and dictionaries. Sources and
terms are recorded in `data/ATTRIBUTION.md`; without that entry the pack does not go into a release.

**Quality.** The bar for a built-in language: on text typed in the right
layout, no more than 0.1% of words ruined (Russian and English are at 0.06% now); the method is in
["Measurements and method"](MEASUREMENTS.md). Native speakers' live typing and a list of hard words are
needed as well.

**How to send it.** The folder `data/languages/<id>/` and the dictionary in `data/dict/` as a pull request
(with an entry in `data/ATTRIBUTION.md`), or as an archive through an issue, the bot or email. I will
check it, run the measurement and add the language to a release when it has been tried live.

**What is not in the pack yet.** Three places still have a language built into the code:
transliteration (one of the readings the correction key cycles through) knows only Russian; the special handling of `ё` in the
combination tables is Russian only; the Apple layout (`ru(mac)`, `us(mac)`) exists only for Russian
and English. `pulinto judge` without `--layout` guesses the layout by letters, so for languages that
share an alphabet give `--layout <id>`; the daemon itself relies on the enabled layout.
