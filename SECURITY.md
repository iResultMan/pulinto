# Безопасность / Security

[English version below](#english)

PuLinTo читает нажатия клавиш и печатает от имени пользователя, поэтому ошибки в нём
могут быть серьёзнее, чем в обычной программе. Если вы нашли уязвимость, не заводите
публичное обращение: напишите автору, Yury ResultMan, на **i.result.man@gmail.com** с темой «PuLinTo: безопасность».

В письме укажите:

- коммит (`git rev-parse --short HEAD`) или версию программы (пункт «Подсказки» в меню значка), дистрибутив и среду (X11 или Wayland);
- что происходит и как это воспроизвести;
- что, по вашему мнению, может сделать злоумышленник.

Это проект одного автора, поэтому сроки ответа и исправления не гарантируются, но
подтверждённые проблемы с утечкой набранного текста, повышением привилегий и порчей чужого
ввода будут разобраны в первую очередь.

## Что в области действия

- Утечка набранного или исправленного текста в журнал, файл или по сети.
- Права файлов с правилами пользователя (должны быть `0600`).
- Доступ к D-Bus-интерфейсу демона и расширению GNOME Shell.
- Правило udev (`data/70-pulinto.rules`) и доступ к `/dev/input` и `/dev/uinput`.
- Возможность заставить демон напечатать произвольный текст.

## Поддерживаемые версии

Поддерживается последний выпуск (сейчас 1.0.0): исправления выходят в основной ветке и в
следующем выпуске.

<!-- english -->

---

## English

PuLinTo reads keystrokes and types on the user's behalf, so a bug in it can be more serious than in
an ordinary program. If you have found a vulnerability, do not open a public issue: write to the
author, Yury ResultMan, at **i.result.man@gmail.com** with the subject "PuLinTo: security".

In the email, give:

- the commit (`git rev-parse --short HEAD`) or the version of the program (the "Hints" item of the tray icon menu), the distribution and
  the environment (X11 or Wayland);
- what happens and how to reproduce it;
- what you think an attacker could do.

This is a one-person project, so response and fix times are not guaranteed, but confirmed problems
that leak typed text, escalate privileges or corrupt input in other applications will be handled first.

### In scope

- A leak of typed or corrected text into a log, a file or over the network.
- The permissions of the user's rule files (they must be `0600`).
- Access to the daemon's D-Bus interface and to the GNOME Shell extension.
- The udev rule (`data/70-pulinto.rules`) and access to `/dev/input` and `/dev/uinput`.
- Any way to make the daemon type arbitrary text.

### Supported versions

The latest release is supported (currently 1.0.0): fixes land on the main branch and in the next release.
