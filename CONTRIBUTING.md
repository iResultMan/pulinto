# Как участвовать в проекте / Contributing to PuLinTo

[English version below](#english)

Спасибо за интерес. Приветствуются сообщения об ошибках, исправления и трудные слова.

## Сборка

```sh
sudo apt-get install libdbus-1-dev   # dbus (через ksni)
cargo build --release --workspace
```

Нужен Rust 1.82 или новее. Русский и английский словари в `data/dict` хранятся в git (CC BY 4.0,
источники в `data/ATTRIBUTION.md`); как собрать словарь и языковой пакет для другого языка, описано
в `docs/LANGUAGE_PACK.md`.

Чтобы проверить изменение без установки, запускайте бинарники из `target/release`: демон ищет
`data/` относительно своего исполняемого файла. `./install.sh --no-enable` копирует всё в `~/.local`,
ничего не запуская.

## Запросы на слияние (pull requests)

- Одно изменение — одна тема; объясните, зачем оно нужно.
- Перед запросом выполните `cargo build --release --workspace`; автор прогоняет проверки на своём наборе тестов при слиянии.
- Код, идентификаторы и комментарии — на английском. Строки для пользователя в коде пишутся
  по-английски и переводятся в `po/`; документация может быть на русском.
- Изменение каскада опишите словами, которые меняют результат (было и стало): замер на корпусе
  делает автор.

## Новые языки

Язык подключается папкой с данными, без правки программы: описание и требования в
`docs/LANGUAGE_PACK.md`, общая картина в `docs/LANGUAGES.md`. Очень полезны и трудные слова (слово, на
котором каскад ошибся, с контекстом) и проверка на живой клавиатуре.

## Переводы

Строки программы в коде английские; переводы лежат в `po/*.po`, меню трея —
в `crates/pulintod/src/i18n.rs`. Добавили или изменили строку: допишите её перевод в каждый `po/*.po`
(ключ `msgid` — английский текст из кода) или оставьте это автору. Новый язык интерфейса: скопируйте
`po/pulinto.pot`, переведите и пришлите файл. См. `docs/LANGUAGES.md`.

## Вопросы и безопасность

Ошибки и идеи: обращения на GitHub. По остальному и об уязвимостях (см. `SECURITY.md`) пишите на
i.result.man@gmail.com.

## Лицензия

Присылая изменения, вы соглашаетесь, что они лицензируются как `MIT OR Apache-2.0`, так же, как и
проект.

<!-- english -->

---

## English

Thanks for your interest. Bug reports, fixes and hard words are welcome.

### Building

```sh
sudo apt-get install libdbus-1-dev   # dbus (via ksni)
cargo build --release --workspace
```

Rust 1.82 or newer is required. The Russian and English dictionaries in `data/dict` are
stored in git (CC BY 4.0, sources in `data/ATTRIBUTION.md`); how to build a dictionary and a language pack
for another language is described in `docs/LANGUAGE_PACK.md`.

To try a change without installing, run the binaries from `target/release`:
the daemon finds `data/` relative to its own executable. `./install.sh
--no-enable` copies everything into `~/.local` without starting anything.

### Pull requests

- Keep a change focused on one thing and say why it is needed.
- Run `cargo build --release --workspace` before opening the pull request; the maintainer runs the
  test suite when merging.
- Code, identifiers and comments are in English. User-facing strings in code are in English and
  translated in `po/`; documentation may be in Russian.
- Describe a change to the cascade with the words whose result it changes (before and after): the
  measurement on the corpus is done by the maintainer.

### New languages

A language is added with a folder of data, without changing the program: the description and the
requirements are in `docs/LANGUAGE_PACK.md`, the general picture in `docs/LANGUAGES.md`. Hard words (a word
the cascade got wrong, with its context) and testing on a real keyboard are very useful too.

### Translations

The strings in the code are English; the translations live in `po/*.po`, the tray
menu in `crates/pulintod/src/i18n.rs`. After adding or changing a string, add its translation to every
`po/*.po` (the `msgid` is the English text from the code) or leave it to the maintainer. New interface
language: copy `po/pulinto.pot`, translate and send the file. See `docs/LANGUAGES.md`.

### Questions and security

Bugs and ideas: GitHub issues. For anything else, or for a vulnerability (see
`SECURITY.md`), write to i.result.man@gmail.com.

### Licence

By contributing you agree that your work is licensed under `MIT OR
Apache-2.0`, the same as the project.
