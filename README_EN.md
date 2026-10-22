<div align="center">

# PuLinTo

### Typed in the wrong layout? The word fixes itself.

`ghbdtn` → **привет** &nbsp;·&nbsp; `руддщ` → **hello** &nbsp;·&nbsp; `ghbdtn vbh` → **привет мир**

The cursor stays where it is, the layout is already the right one, and you keep typing.

[Install](#install-with-one-command) · [Hotkeys](#hotkeys) · [Tell the author](#feedback-and-wishes) · [Website](https://iresultman.github.io/pulinto/) · **English** · [Русский](README.md)

</div>

---

## It feels like magic, and you get used to it in a day

For the first half hour you notice how it works. By the evening you have forgotten that the layout ever needed switching. Then you sit down at someone else's computer, type `ghbdtn`, and it stays `ghbdtn`. That is when you see how much it was doing.

- **You stop thinking about the layout.** Type the way you type. Forgot to switch? The word is fixed.
- **Fixed before Enter.** The word is corrected at the space or punctuation mark, before it reaches the window. A chat message goes out already correct: during a live test the author typed `ujnjdj` into a chat field and what was sent was "готово".
- **More careful than you are.** Passwords, addresses, commands, code and mixed-alphabet words are left alone. When it is not sure it keeps the word as it is: a missed word is one key press away from fixed, a ruined message cannot be taken back.
- **Missed a word? One key.** (`Alt` on an Apple keyboard, `Pause/Break` on others.) It fixes the word, undoes the last replacement or converts the selected text; pressing it again gives the next reading.
- **It does not spy.** There is no typing log and nothing you type is stored. Only your own rules go to disk; a bug report is written only when you press the key for it.
- **It adapts to you.** Your own autocorrections (`ещ\` → `ещё`), words it must never touch, a mode for each application.
- **An Apple keyboard that feels like a Mac.** When an Apple keyboard is connected, PuLinTo offers the layout you are used to: comma and full stop on Shift+6 and Shift+7, the letter ё on the `\` key. It is one switch on the "Apple keyboard" page.

On 21,300 words (20,000 words of real news text and 1,300 traps: URLs, identifiers, version strings) PuLinTo reaches **99.38% accuracy with 0.06% of words ruined**. [How this was measured and what it does not prove.](docs/MEASUREMENTS.md)

Works on **X11** and on **GNOME with Wayland**. Typing languages: Russian and English. The interface (settings window, tray menu) speaks Russian and English and follows your system language by default; you can switch it at any time from the tray icon menu or the settings window.

## Install with one command

```sh
curl -fsSL https://iresultman.github.io/pulinto/install.sh | bash
```

The command downloads a ready build, verifies its checksum and installs it for your user. It asks for the administrator password once: that installs the rule that lets the program read the keyboard. You can read the script at the same link before running it.

**Or with an installer file:** download it and open it with a double click.

| System | File | From a terminal |
|---|---|---|
| Ubuntu, Debian, Mint | [pulinto-x86_64.deb](https://github.com/iResultMan/pulinto/releases/latest/download/pulinto-x86_64.deb) | `sudo apt install ./pulinto-x86_64.deb` |
| Fedora, RHEL, AlmaLinux, Rocky | [pulinto-x86_64.rpm](https://github.com/iResultMan/pulinto/releases/latest/download/pulinto-x86_64.rpm) | `sudo dnf install ./pulinto-x86_64.rpm` |
| Any Linux | [pulinto-x86_64.tar.gz](https://github.com/iResultMan/pulinto/releases/latest/download/pulinto-x86_64.tar.gz) | unpack and run `./install.sh` |

**After installing** there is nothing to do: corrections work right away. GNOME must have a layout-switching shortcut enabled, for example Alt+Shift: that is what the program uses to switch the layout. A "PuLinTo" launcher (the settings) appears in the application menu and an icon in the tray: an orange space bar means working, grey means off. The icon menu lets you pause the corrections, restart the helper or quit it.

**Uninstall:** `./uninstall.sh` from the archive, `sudo apt remove pulinto` or `sudo dnf remove pulinto`. Your rules stay.

**If the keyboard stops responding:** press `Backspace`, `Esc` and `Enter` together. The grab is released.

## Hotkeys

Each keyboard gets its own set: an Apple keyboard uses the Apple set, any other keyboard uses the PC set. Any shortcut can be reassigned in the settings window, on the "Hotkeys" page.

| What it does | Apple | PC |
|---|---|---|
| **Fix the word.** The word being typed, then the last replacement, then the selection; repeating cycles the readings | a lone `Alt`, `Pause/Break` | `Pause/Break` or a lone right `Ctrl` |
| Convert only the selected text | `Shift+Pause/Break` | `Shift+Pause/Break` |
| Mode for the focused window, in a loop: automatic, hotkey only, off | `Ctrl+Alt+P` | `Ctrl+Alt+P` |
| Write a bug report (only when you press it) | `Ctrl+Alt+\` | `Ctrl+Alt+\` |
| **Turn the Apple layout on or off.** With an Apple and an ordinary keyboard connected the Apple layout applies to both: this key brings back the one you know | `Ctrl+Alt+'` | `Ctrl+Pause/Break` |
| **Release the keyboard** (emergency) | `Backspace+Esc+Enter` together | same |

In the PC set a lone `Alt` is left to applications: it opens the menu in GTK, Qt, LibreOffice and Firefox. You can turn it back on from the Hotkeys page if you want it.

**The tray icon** shows the state and lets you pause the program, choose the mode for the focused window and open the settings. **The settings window** (application menu, "PuLinTo"): autocorrections, "never change", "always change", per-application mode, the Apple layout, hotkeys. Rules can be exported to and imported from a file.

More: [manual](docs/MANUAL.md), [FAQ](docs/FAQ.md).

## Feedback and wishes

I want to hear where the program got it wrong and what you are missing. Any of these works:

- **Telegram support [@pulinto_bot](https://t.me/pulinto_bot):** write what went wrong or what you want. An AI assistant that knows the documentation answers first; if it cannot, or you send `/human`, the author joins. Do not send passwords or private text;
- **Email:** [i.result.man@gmail.com](mailto:i.result.man@gmail.com);
- **GitHub:** [issues](https://github.com/iResultMan/pulinto/issues) for bugs and suggestions.

If a word was fixed wrongly, press `Ctrl+Alt+\` right after the mistake: the program writes the last eight decisions to `~/.local/share/pulinto/reports.jsonl`. The file contains the words you typed, so it appears only on your command and is never sent anywhere by itself. Attach the lines that matter to your message.

Please report security problems by email, not as a public issue: [SECURITY.md](SECURITY.md).

## Help wanted

- **Testers on other systems.** Especially Red Hat Enterprise Linux, Fedora, AlmaLinux, Rocky, openSUSE, Arch and other Linux builds: the packages are built, but only tested on Ubuntu. What to check and how to send the result: [docs/TESTING.md](docs/TESTING.md).
- **Hard words and other languages.** Send me a word the program got wrong and I will improve the dictionaries. Need another language? Write which language you would like to see in the program and agree to be a tester: we will build the dictionaries ourselves, and what we need from you is to try the language on a real keyboard and tell us how it works. Can you build dictionaries? Join in, the format is described in [docs/LANGUAGE_PACK.md](docs/LANGUAGE_PACK.md). Details: [docs/LANGUAGES.md](docs/LANGUAGES.md).

## State of the project, honestly

Version 1.0.0. Tested on Ubuntu: X11 (XFCE) and GNOME 50 on Wayland. It has not yet run a full working day in grab mode on every setup; so far it has been checked on a test stand and in live tests. The main open debt: short English words typed in the Russian layout. About twenty of them (`on`, `at`, `it`, `to`…) are fixed on their own, but `in`, `by` and `he` look like Russian abbreviations (`шт.` means "pieces") and are settled only by the words around them. KDE, Sway and other Wayland desktops besides GNOME are not supported yet.

## For developers and licences

Sources and building: [CONTRIBUTING.md](CONTRIBUTING.md). How it works: [the user manual](docs/MANUAL.md), [how to add a language](docs/LANGUAGES.md), [measurements](docs/MEASUREMENTS.md).

Author: Yury ResultMan. Code: MIT or Apache-2.0, your choice (`LICENSE-MIT`, `LICENSE-APACHE`; attribution notice: `NOTICE`). Language data in `data/`: CC BY 4.0 (`LICENSE-CC-BY-4.0`); sources and attribution in `data/ATTRIBUTION.md`.

The project is not affiliated with Yandex and is not a product of Punto Switcher. The idea comes from Punto Switcher; the code, the detection algorithm and the dictionaries are written from scratch.
