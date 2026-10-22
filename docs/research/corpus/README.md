# Golden corpus — keyboard layout misdetection benchmark

This is the measurement instrument for one question: **how good is our
autodetection of "word typed in the wrong keyboard layout" against reality?**
It exists to produce an honest accuracy number that decides whether the
project continues, so it must not be tuned to make any particular
implementation look good. If a row in `golden.tsv` looks wrong, fix the
labeling to match linguistic/contextual reality — never to match what the
tool currently outputs.

See `docs/ALGORITHM.md` for the detection spec this corpus measures against,
in particular §3 (short 1–2 letter words) and §6 (stop rules: passwords,
URLs, code, abbreviations, mixed-script tokens — things that must never be
auto-converted).

## Files

- `golden.tsv` — the corpus, ~2300 rows.
- `validate.py` — structural validator, run before trusting any metrics
  computed from the corpus.

## Columns (`golden.tsv`)

| # | Column | Meaning |
|---|--------|---------|
| 1 | `id` | Integer row id. |
| 2 | `context` | Up to two preceding words, exactly as they would appear on screen. May be empty (start of input, or not relevant to the category). |
| 3 | `input` | The word as it appears on screen — possibly garbled because it was typed in the wrong layout. This is what the detector under test receives. |
| 4 | `expect` | `KEEP` (leave `input` untouched) or `CONVERT` (the detector should re-map it to `correct`). |
| 5 | `correct` | The correct, human-intended visible text. Equal to `input` when `expect=KEEP`. |
| 6 | `category` | One of `prose_ru`, `prose_en`, `short1`, `short2`, `mixed`, `url`, `password`, `code`, `abbrev`, `name`, `translit`, `number`. |

## Composition

| Category | Rows | Notes |
|---|---|---|
| `prose_ru` | 850 | 600 correctly-typed Russian words (`KEEP`) + 250 real Russian words typed with a US layout, i.e. Latin garbage (`CONVERT`) |
| `prose_en` | 850 | 600 correctly-typed English words (`KEEP`) + 250 real English words typed with a RU layout, i.e. Cyrillic garbage (`CONVERT`) |
| `short1` | 120 | 1-letter words (§3). Mix of correct short words, genuinely broken ones with confirming context, and deliberate traps: wrong-language context, empty context, and a solitary `c` in a code context — all of which must stay `KEEP` per §3's "context is mandatory insurance" rule. |
| `short2` | 120 | Same idea for 2-letter words. |
| `password` | 60 | Always `KEEP`. |
| `url` | 60 | URLs, emails, file paths. Always `KEEP`. |
| `code` | 60 | Identifiers/snippets (`@param`, `std::vector`, `--verbose`, `snake_case`, …). Always `KEEP`. |
| `abbrev` | 40 | All-caps abbreviations (`СССР`, `HTTP`, `NASA`, …). Always `KEEP`. |
| `name` | 40 | Names/nicknames, Cyrillic and Latin. Always `KEEP`. |
| `translit` | 40 | Russian words deliberately spelled in Latin (`privet`, `spasibo`). This is a **user choice**, not a layout accident — always `KEEP`. |
| `mixed` | 30 | A single token already containing both alphabets (`Wi-Fiкой`, `USBпорт`). Always `KEEP`. |
| `number` | 30 | Alphanumeric identifiers (`A4`, `x86_64`, `ГОСТ-2024`). Always `KEEP`. |

Total: **2300 rows** (600+600+250+250+120+120+60+60+60+40+40+40+30+30). Note
this is higher than the "~2000" ballpark mentioned in the task; the per-category
counts given in the task were used as the authoritative target since they are
exact, and they sum to 2300, not 2000.

Layout tables used to generate/verify the `CONVERT` rows are positional
(index *i* in one row corresponds to index *i* in the other), taken from the
task with one correction: the given `us_lower` string is missing the leading
backtick key (present in `ru_lower` as `ё`, in `ru_upper` as `Ё`, and in
`us_upper` as `~`). Without restoring it, every subsequent character is
misaligned by one position and the task's own self-check
(`ghbdtn`→`привет`, `руддщ`→`hello`) fails. Both `validate.py` and the
(unchecked-in) generator restore this leading backtick before building the
US↔RU character maps.

## How to compute metrics

Run your detector over `input` (with `context` available to it, exactly as
the algorithm spec allows) and compare its verdict to `expect`/`correct`.

- **Accuracy** = (rows where the detector's verdict matches `expect`) / (total rows).
  Report it overall and broken down by `category`, since a single blended
  number hides whether failures cluster in short words, prose, or stop-rule
  categories.

- **False positives — the expensive error, measure separately.**
  A false positive is any row where `expect=KEEP` but the detector says
  `CONVERT`. Per `docs/ALGORITHM.md` §1.2, this is the error that destroys a
  password, mangles a URL, or corrupts an already-sent message — it is not
  symmetric with the opposite mistake and must be reported as its own number:

  ```
  false_positive_rate = count(expect=KEEP AND verdict=CONVERT) / count(expect=KEEP)
  ```

  Break this down by category too — a false positive rate on `password`/`url`/
  `code`/`abbrev`/`mixed`/`translit`/`name`/`number` (all `KEEP`-only
  categories) is a direct measurement of stop-rule (§6) failures and should be
  reported as its own headline metric, separate from the general accuracy
  number.

- **False negatives (missed conversions)** = rows where `expect=CONVERT` but
  the detector says `KEEP`:

  ```
  false_negative_rate = count(expect=CONVERT AND verdict=KEEP) / count(expect=CONVERT)
  ```

  Cheaper than a false positive per §1.2 (user fixes it with one hotkey
  press), but still worth tracking, especially split between `prose_ru`,
  `prose_en`, `short1`, and `short2` — the short-word tables (§3) are the
  known weak spot this corpus was specifically built to expose.

- When the detector also outputs the corrected text on a `CONVERT` verdict,
  additionally check it equals `correct` — a right verdict with wrong output
  text is still a bug.

## Running the validator

```bash
cd "docs/research/corpus"
python3 validate.py            # validates ./golden.tsv
python3 validate.py other.tsv  # or an explicit path
```

Exits `0` and prints `OK: all checks passed` when clean; otherwise exits
non-zero and lists every problem found (not just the first). It checks:
structural integrity (6 columns, no empty required fields), `expect` values,
`correct == input` under `KEEP`, that the positional layout table actually
maps `input` to `correct` under `CONVERT` (this is the check most likely to
catch a broken generator — a single off-by-one in the layout table silently
produces a corpus full of wrong ground truth), absence of duplicate
`(context, input)` pairs, and category counts within ±10% of the targets
above.
