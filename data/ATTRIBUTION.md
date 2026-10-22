# Источники языковых данных / Language data sources

[English version below](#english)

Языковые данные в `data/` (частотные словари `data/dict/ru.*`, `en.*` и таблицы запрещённых
буквосочетаний `data/languages/`) распространяются под лицензией **Creative Commons Attribution 4.0
(CC BY 4.0)**: <https://creativecommons.org/licenses/by/4.0/>. Полный текст лежит в корне репозитория,
файл `LICENSE-CC-BY-4.0`. Код проекта лицензируется отдельно (MIT или Apache-2.0, см. корень
репозитория).

Как ссылаться, если вы используете эти данные: «Языковые данные PuLinTo (Yury ResultMan), CC BY 4.0
(<https://creativecommons.org/licenses/by/4.0/>); основа — Leipzig Corpora Collection, см.
`data/ATTRIBUTION.md`». Использовать, изменять и распространять данные можно, в том числе в
коммерческих продуктах, с указанием авторства.

## Что публикуется

Только частотные счёты слов (`слово<TAB>число`), готовые индексы для быстрого поиска (`*.fst`,
строятся из `*.tsv` командой `pulinto build-fst`) и
таблицы запрещённых буквосочетаний. Предложений, абзацев и исходных текстов в данных нет; сырые
корпуса не публикуются. Все списки построены собственными программами проекта: токенизация, нижний регистр, подсчёт слов и порог для словарей; частотный порог по сочетаниям внутри слов для таблиц запрещённых буквосочетаний.

Состав: `ru` — 207 093 слова (основа из корпуса и около 4 800 форм прилагательных на -ую/-ых);
`en` — 122 128 слов: основа из корпуса плюс слова, добавленные проектом: около 4 700 технических слов
(идентификаторы и имена из программных текстов, частота 3; перечень в `data/raw/code-curated.tsv`
и `code-mined.tsv`) и отдельные слова вручную (например `github`). Слова, которых нет в корпусе,
добавляют вручную в `en.tsv` в алфавитном порядке и пересобирают `en.fst`.

Опубликованы данные для русского и английского.

## Основа: Leipzig Corpora Collection

| Что | Срез | Лицензия |
|---|---|---|
| Русский и английский: основа частотных списков | корпуса Leipzig всех жанров, не только новости | CC BY |

Требуемая атрибуция: D. Goldhahn, T. Eckart, U. Quasthoff, «Building Large Monolingual
Dictionaries at the Leipzig Corpora Collection: From 100 to 200 Languages», Proceedings of the 8th
International Conference on Language Resources and Evaluation (LREC 2012).
<https://wortschatz-leipzig.de>

## Жанровые добавки

Словарь дополнен текстами нескольких жанров, собранными проектом самостоятельно (разговорный,
юридический, технический, IT-жаргон; источники и их лицензии перечислены ниже). Использовались только источники с разрешающими лицензиями:
общественное достояние (US Code; ГК, ТК, НК, УК РФ — ст. 1259 п. 6 ГК РФ), CC BY (Tatoeba —
<https://tatoeba.org>, документация tldr-pages, Kubernetes, GitHub Docs, PHP manual) и MIT/Apache-2.0
(The Rust Programming Language). Для Python docs (PSF License v2) действует уведомление об
авторстве Python Software Foundation.

## Что не использовалось

Лицензии с ограничением коммерческого использования (CC BY-NC и подобные), GPL-данные,
OpenSubtitles, расширенные объединённые наборы, чужие готовые частотные списки и словари. Таблицы
запрещённых буквосочетаний построены проектом самостоятельно из корпусов.

<!-- english -->

---

## English

The language data in `data/` (frequency dictionaries `data/dict/ru.*` and `en.*`, and the tables of
forbidden letter combinations in `data/languages/`) is licensed under **Creative Commons Attribution
4.0 (CC BY 4.0)**: <https://creativecommons.org/licenses/by/4.0/>. The full text is in the repository
root, file `LICENSE-CC-BY-4.0`. The project's code is licensed separately (MIT or Apache-2.0, see the
repository root).

How to credit the data if you use it: "PuLinTo language data (Yury ResultMan), CC BY 4.0
(<https://creativecommons.org/licenses/by/4.0/>); based on the Leipzig Corpora Collection, see
`data/ATTRIBUTION.md`". You may use, change and redistribute the data, including in commercial
products, as long as you give credit.

### What is published

Only word frequency counts (`word<TAB>count`), ready-made indexes for fast lookup (`*.fst`, built
from `*.tsv` with `pulinto build-fst`) and the tables of
forbidden letter combinations. There are no sentences, paragraphs or source texts in the data, and
the raw corpora are not published. All the lists are built by the project's own programs: tokenising, lower case, counting and a threshold for the dictionaries; a frequency threshold on combinations inside words for the tables of forbidden letter combinations.

Contents: `ru` has 207,093 words (the corpus base and about 4,800 adjective forms ending in -ую/-ых);
`en` has 122,128 words: the corpus base plus words added by the project: about 4,700 technical words
(identifiers and names from program texts, frequency 3; listed in `data/raw/code-curated.tsv` and
`code-mined.tsv`) and a few words added by hand (for example `github`). Words that are not in the
corpus are added to `en.tsv` by hand, in alphabetical order, and `en.fst` is rebuilt.

Data is published for Russian and English.

### Base: Leipzig Corpora Collection

| What | Slice | Licence |
|---|---|---|
| Russian and English: the base of the frequency lists | Leipzig corpora of all genres, not only news | CC BY |

Required attribution: D. Goldhahn, T. Eckart, U. Quasthoff, "Building Large Monolingual Dictionaries
at the Leipzig Corpora Collection: From 100 to 200 Languages", Proceedings of the 8th International
Conference on Language Resources and Evaluation (LREC 2012). <https://wortschatz-leipzig.de>

### Genre additions

The dictionary is supplemented with texts of several genres collected by the project itself
(conversational, legal, technical, IT slang; the sources and their licences are listed below). Only sources with permissive licences were used: public
domain (the US Code; the Civil, Labour, Tax and Criminal Codes of the Russian Federation, Article 1259
paragraph 6 of the Russian Civil Code), CC BY (Tatoeba, <https://tatoeba.org>, the documentation of
tldr-pages, Kubernetes, GitHub Docs, the PHP manual) and MIT/Apache-2.0 (The Rust Programming
Language). For the Python docs (PSF License v2) the Python Software Foundation's notice of authorship
applies.

### What was not used

Licences that restrict commercial use (CC BY-NC and the like), GPL data, OpenSubtitles, extended
merged sets, and third-party ready-made frequency lists and dictionaries. The tables of forbidden
letter combinations were built by the project itself from corpora.
