# Замеры и методика / Measurements and method

[English version below](#english)

На сайте и в README названы цифры точности. Здесь написано, как они получены, что они значат и чего не доказывают.

## Что считается

Каждое слово корпуса несёт разметку: оставить как есть (`KEEP`) или исправить (`CONVERT`).
Строка `CONVERT` получается механически: настоящее слово переписывается клавишами другой раскладки (`привет` → `ghbdtn`). Программа получает слово вместе с двумя предыдущими, а для коротких слов и со следующим, и отвечает, что с ним делать.

- **Точность** — доля слов, с которыми программа поступила правильно (и оставила, и исправила).
- **Ложное срабатывание** — правильное слово, которое программа испортила. Оно дороже пропуска: пропущенное слово исправляется одним нажатием клавиши исправления, а испорченный пароль или отправленное сообщение — нет.
- **Пропуск** — слово в чужой раскладке, которое осталось как есть.

## Отложенный корпус

Всего 21 300 слов: по 10 000 слов подряд из реальных новостных предложений Leipzig Corpora  Collection на русском и английском (наборы `rus_news_2023_10K` и `eng_news_2023_10K`, CC BY) плюс 1 300 ловушек, взятых из реальных источников: URL из `/usr/share/doc`, идентификаторы из настоящих исходников, версии пакетов из `dpkg -l`, аббревиатуры из тех же предложений. Токены не отбирались по длине: однобуквенные и двухбуквенные слова встречаются в естественной пропорции. Строки, где испорченное слово случайно оказалось словом другого языка, не отсеивались, а помечены (`ambiguous`): это принципиальный потолок задачи, а не ошибка программы.

Разметка механическая, зерно генератора случайных чисел зафиксировано (137), тестовые слова не выбирались руками под код.

## Результат

Точность 99,38 %, ложные срабатывания 0,06 %, пропуски 2,13 %.

## Свежие тексты

Отложенный корпус во время разработки использовался повторно, поэтому его цифры
оптимистичны. Для проверки на новом материале в октябре 2026 были собраны блоки свежих веб-текстов и прогнаны через живой стенд на Wayland. На длинных блоках общим объёмом около 32 000 слов получилось: точность 98,7 %, ложные срабатывания 0,02 %, пропуски 1,1 %. Стенд из 600 слов дал 99,67 % на обеих раскладках.

## Что эти цифры не доказывают

- **Корпус не полностью независим от словарей.** Новостной язык Leipzig хорошо представлен в словарях любой программы. Это завышает цифры, но на живой переписке со сленгом, никами и опечатками результат будет ниже.
- **Отладка и измерение частично пересекались.** Две правки (строгость правила пароля и различение `ё` и `е` в n-граммах) найдены на этом же корпусе. Они исправляют логические ошибки, а не подгоняют пороги, но это не независимая проверка.
- **Корпус и стенд в репозиторий не входят.** Принцип разметки и зерно генератора описаны здесь, но воссоздать корпус байт в байт по репозиторию нельзя: цифры нужно воспринимать как заявление автора, а не как воспроизводимый результат. Проверить программу на своих словах можно командой ниже.
- **Языки.** Измерялись только русский и английский.

## Как проверить самому

Разобрать одно слово можно без демона:

```sh
pulinto judge --context "он сказал" ghbdtn
```

Команда показывает вердикт, прочтение и ступень каскада, которая ответила.

Если вы нашли слово, которое программа исправила неверно, нажмите `Ctrl+Alt+\`
в момент ошибки и приложите нужные строки отчёта к обращению на GitHub: так
методика улучшается на реальных случаях.

<!-- english -->

---

## English

The site and the README quote accuracy figures. This page explains how they were obtained, what they
mean and what they do not prove.

### What is counted

Every word of the corpus carries a label: leave as is (`KEEP`) or fix (`CONVERT`). A `CONVERT` row is
made mechanically: a real word is rewritten with the keys of the other layout (`привет` → `ghbdtn`).
The program receives the word with the two words before it and, for short words, the one after, and
answers what to do with it.

- **Accuracy** is the share of words the program handled correctly (both left alone and fixed).
- **False positive** is a correct word the program ruined. It costs more than a miss: a missed word is
  fixed with one press of the correction key, a ruined password or a sent message is not.
- **Miss** is a word in the wrong layout that was left as it is.

### The held-out corpus

21,300 words in total: 10,000 consecutive words each from real news sentences of the Leipzig Corpora
Collection in Russian and English (the sets `rus_news_2023_10K` and `eng_news_2023_10K`, CC BY), plus
1,300 traps taken from real sources: URLs from `/usr/share/doc`, identifiers from real source code,
package versions from `dpkg -l`, abbreviations from the same sentences. Tokens were not chosen by
length: one- and two-letter words appear in their natural proportion. Rows where the ruined word
happened to be a word of the other language were not removed but marked (`ambiguous`): that is a
ceiling of the task, not a mistake of the program.

The labelling is mechanical, the seed of the random generator is fixed (137), and the test words were
not hand-picked to suit the code.

### Result

Accuracy 99.38%, false positives 0.06%, misses 2.13%.

### Fresh texts

The held-out corpus was reused during development, so its figures are optimistic. To check on new
material, blocks of fresh web texts were collected in October 2026 and
run through the live stand on Wayland. On long blocks of about 32,000 words in total the result was:
accuracy 98.7%, false positives 0.02%, misses 1.1%. The
600-word stand gave 99.67% on both layouts.

### What these figures do not prove

- **The corpus is not fully independent of the dictionaries.** The news language of Leipzig is well
  represented in the dictionaries of any program. This inflates the figures, but on live chat with slang, nicknames and typos the result will be lower.
- **Debugging and measuring partly overlapped.** Two fixes (the strictness of the password rule and
  telling `ё` from `е` in the n-grams) were found on this same corpus. They correct logic errors and do
  not tune thresholds, but this is not an independent check.
- **The corpus and the stand are not in the repository.** The principle of the labelling and the seed are described here, but the corpus cannot be recreated byte for byte from the repository: take the figures as the author's statement, not as a reproducible result. You can check the program on your own words with the command below.
- **Languages.** Only Russian and English were measured.

### How to check it yourself

One word can be looked at without the daemon:

```sh
pulinto judge --context "он сказал" ghbdtn
```

The command shows the verdict, the reading and the stage of the cascade that answered.

If you found a word the program fixed wrongly, press `Ctrl+Alt+\` at the moment of the mistake and
attach the lines that matter to a GitHub issue: that is how the method improves on real cases.
