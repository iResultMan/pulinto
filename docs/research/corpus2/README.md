# Held-out corpus — independent from our own spec

This is the corpus `docs/research/06-bench-pulinto.md` asked for in its
"Оговорки" section: `docs/research/corpus/golden.tsv` was generated from
`docs/ALGORITHM.md`, the same document the detection cascade is implemented
against, so a 99.9% score on it mostly proves "implementation matches spec,"
not "quality on live language." This corpus (`heldout.tsv`) is built from
real, independently-authored text and real system artifacts instead — it
shares no word lists, no short-word whitelist, and no stop-rule dictionary
with the code under test.

**21,300 rows** (well above the 20,000 floor), header included.

## Sources and licenses (verified on the primary page, not by hearsay)

### Sentences — Leipzig Corpora Collection

- Checked directly at `https://wortschatz-leipzig.de/en/usage` (fetched
  2026-08-01). Quote: *"The text corpora offered for download are made
  available under the Creative Commons licence CC BY."* Attribution
  requested by the same portal's download page: cite D. Goldhahn, T. Eckart
  & U. Quasthoff, *"Building Large Monolingual Dictionaries at the Leipzig
  Corpora Collection: From 100 to 200 Languages"*, LREC 2012.
- Files used (downloaded from `downloads.wortschatz-leipzig.de/corpora/`,
  the official Leipzig download host reached via the language pickers at
  `https://wortschatz-leipzig.de/en/download/rus` and `.../eng`):
  - `rus_news_2023_10K.tar.gz` → `rus_news_2023_10K-sentences.txt` (10,000
    Russian news sentences, format `id\tsentence`)
  - `eng_news_2023_10K.tar.gz` → `eng_news_2023_10K-sentences.txt` (10,000
    English news sentences, same format)
- These are **not** the corpus's word or short-word lists — they are full
  news sentences; every token used is extracted from running prose, in the
  order it occurs, not curated or filtered.

### Real-world "trap" tokens (all `KEEP`, all lifted verbatim from real sources — none invented)

- `trap_url` (350 rows): real URLs, `grep`-extracted from files under
  `/usr/share/doc/**` (e.g. `nodejs/changelogs/CHANGELOG_V*.md`,
  `python3-pip/html/search.html`). `source` records the package-relative
  path and line number, e.g. `usr-share-doc:nodejs/changelogs/CHANGELOG_V12.md:3725`.
- `trap_code` (350 rows): real identifiers, `grep`-extracted from (a) the
  `lay-x11` competitor's own source (`git clone
  https://github.com/HelpFreedom/lay-x11`, MIT-licensed, already used as a
  benchmark competitor in `docs/research/04-bench-lay-x11.md`), and (b)
  system C headers (`/usr/include/{stdio,stdlib,string,unistd}.h`).
  `source` is e.g. `lay-x11:src/x11_layout.rs:106` or
  `usr-include:stdlib.h:145`. Some captured identifiers double as plain
  English words (`include`, `len`, `extended`) since the extraction regex
  doesn't distinguish code-only tokens from English words used as
  identifiers/comments — every one of them is still a literal substring of
  a real source file at the recorded line, which is what the task asked
  for.
- `trap_num` (300 rows): real installed-package version strings from
  `dpkg -l` on this machine (e.g. `2025.11-3ubuntu7`, `1:3.14.0-1build2`).
  `source` is `dpkg:<package-name>`.
- `trap_abbrev` (300 rows): real ALL-CAPS tokens (`США`, `НАТО`, `ТК`,
  `BJP`, `TV`, `ODOT`, …) found while tokenizing the same Leipzig sentences
  used for the `ru`/`en` rows — extracted opportunistically during the same
  pass, not a separately curated list. `source` uses the same
  `dataset#sentence_id:token_index` scheme as the main rows.

## Labeling methodology (mechanical, not spec-driven)

1. Both sentence files are tokenized with one fixed regex,
   `[A-Za-zА-Яа-яЁё]+(?:[-'][A-Za-zА-Яа-яЁё]+)*` — letter runs, optionally
   hyphen/apostrophe-joined. **No length filter**: single-letter tokens
   (`в`, `и`, `a`, `I`, …) are kept exactly as they occur.
2. Tokens are walked strictly in sentence order. `context` = the up-to-two
   *actual* preceding tokens from the *same* sentence (own casing
   preserved), exactly as specified.
3. Per token, a seeded RNG (`random.Random(137)`, `P_CONVERT = 0.29`)
   decides `KEEP` vs. `CONVERT`. Seed **137** — rerun
   `build_heldout.py` with the same seed to reproduce this file exactly
   (see "Reproducing" below; the generator script itself lives only in the
   working scratchpad, not in this directory, matching how
   `docs/research/corpus/golden.tsv`'s own generator was never checked in
   either).
4. `CONVERT` rows apply the **positional** US⟷RU layout tables (identical
   to the ones in `docs/research/corpus/validate.py`, same self-check:
   `ghbdtn↔привет`, `руддщ↔hello`) to the real corpus token — `ru`-category
   tokens are mapped RU→US (simulating "typed on a US-set keyboard"),
   `en`-category tokens US→RU. `correct` is always the real, untouched
   corpus token; `input` is the garbled form.
5. **Round-trip guard.** A handful of real sentences contain genuine
   mixed-script/homoglyph tokens — e.g. `rus_news_2023_10K` sentence 33
   literally reads `"Oб этом заявил..."` with a Latin `O` typo'd in place
   of Cyrillic `О`. A one-directional positional map over such a token
   doesn't invert cleanly (only the Cyrillic half converts), which would
   silently produce a `CONVERT` row with a `correct` unreachable from
   `input` by either direction of the table. Before committing to
   `CONVERT`, the generator checks that mapping back the garbled form
   reproduces the original token exactly; if not, the row falls back to
   `KEEP` (the token, mixed-script warts and all, is still real and still
   included).
6. **`ambiguous` flag** (CONVERT rows only): the garbled `input` is looked
   up in the *other* language's system dictionary via
   `aspell list -l {en,ru}` (aspell/hunspell dictionaries are already
   installed as `hunspell-{en-us,ru}` / `aspell-{en,ru}`). If the garbled
   form is itself a valid word there, `ambiguous=1` — this is the `dyer`
   → `внук`, `ye` → `ну` class of case from `06-bench-pulinto.md`: a real
   ceiling on the detection task, not a defect, so these rows are **kept**,
   never filtered out. Only pure-alphabetic garbled forms are looked up:
   four Cyrillic letters (`х`, `ъ`, `ж`, `э`) sit on physical keys that
   produce US punctuation (`[`, `]`, `;`, `'`) rather than a letter, so
   their garble contains embedded punctuation; `aspell list` tokenizes on
   punctuation internally, and checking such a string for *literal*
   presence in its misspelled-word output desyncs (it reports the
   sub-token, e.g. `nfr` out of `nfr;t`, not the whole punctuated string) —
   which was caught and fixed during construction (an earlier draft mis-flagged
   ~750 such rows as ambiguous; see the length-4/5 columns below, which
   correctly show ~0% once fixed, since long random strings rarely collide
   with real dictionary words). Punctuation-containing garbles cannot be
   real dictionary words anyway, so they default to `ambiguous=0`.
   **Caveat worth knowing before reading the ambiguous share at face
   value:** aspell accepts bare single letters (`b`, `d`, `t`, …) as valid
   "words" in both its `en` and `ru` dictionaries, which is why the 1-char
   bucket is ~100% ambiguous below — that's a dictionary quirk, not 400
   genuine `dyer`-class collisions. The 2-3 character buckets are the
   meaningful ones for judging the real ceiling.
7. Per the task's hard constraint, **no words were sourced from
   hunspell/aspell/enchant dictionaries** — those are used only as a
   lookup for the `ambiguous` flag on already-real corpus tokens, never to
   generate or select corpus content.
8. Global de-duplication on `(context, input)` across the *entire* corpus
   (main pool + all trap categories together), so no row contradicts
   another row's label for the same on-screen situation.

## Composition

| category | rows | expect | notes |
|---|---|---|---|
| `ru` | 10,000 | KEEP 7,212 / CONVERT 2,788 | tokens from `rus_news_2023_10K`, sentence order |
| `en` | 10,000 | KEEP 7,058 / CONVERT 2,942 | tokens from `eng_news_2023_10K`, sentence order |
| `trap_url` | 350 | KEEP only | real URLs from `/usr/share/doc` |
| `trap_code` | 350 | KEEP only | real identifiers, lay-x11 + `/usr/include` |
| `trap_num` | 300 | KEEP only | real `dpkg` package versions |
| `trap_abbrev` | 300 | KEEP only | real abbreviations found in the same news sentences |
| **total** | **21,300** | KEEP 15,570 (73.1%) / CONVERT 5,730 (26.9%) | |

`ambiguous=1` on 724 / 5,730 CONVERT rows (12.6%). By garbled-input length:
1 char 412/413 (99.8%, dictionary quirk — see caveat above), 2 chars
93/653 (14.2%), 3 chars 219/789 (27.8%), 4 chars 0/667, 5+ chars 0/3,209.

## Token-length histogram (characters, all categories combined)

| length | rows | share |
|---|---|---|
| 1 | 1,396 | 6.6% |
| 2 | 2,330 | 10.9% |
| 3 | 2,767 | 13.0% |
| 4 | 2,311 | 10.8% |
| 5+ | 12,496 | 58.7% |

Broken out for the two main pools, where the natural-distribution
requirement actually matters (traps are excluded — URLs/identifiers/version
strings/abbreviations are inherently longer and would just dilute this):

| length | `ru` (n=10,000) | `en` (n=10,000) |
|---|---|---|
| 1 | 1,013 (10.1%) | 383 (3.8%) |
| 2 | 743 (7.4%) | 1,515 (15.2%) |
| 3 | 782 (7.8%) | 1,796 (18.0%) |
| 4 | 690 (6.9%) | 1,533 (15.3%) |
| 5+ | 6,772 (67.7%) | 4,773 (47.7%) |

Russian 1–2 letter tokens: **17.5%** of the `ru` pool — this matches the
task's 15–20% expectation for real Russian prose *without any curation*;
nothing was filtered or padded to hit that number, it fell out of walking
real news sentences in order. English 1–2 letter tokens land at 19.0% for
the same reason (mostly `a`, `I`, `to`, `of`, `in`, `is`, `on`, …).

## Columns (`heldout.tsv`)

| # | Column | Meaning |
|---|---|---|
| 1 | `id` | Row id, 1..21300. |
| 2 | `context` | Up to two real preceding tokens from the same sentence, own casing. Empty at sentence start or for trap rows. |
| 3 | `input` | What the detector under test receives — garbled if `expect=CONVERT`. |
| 4 | `expect` | `KEEP` or `CONVERT`. |
| 5 | `correct` | The real, correct token. Equal to `input` under `KEEP`. |
| 6 | `category` | `ru`, `en`, `trap_url`, `trap_code`, `trap_num`, `trap_abbrev`. |
| 7 | `ambiguous` | `1` if a CONVERT row's garbled `input` is also a valid word in the other language's dictionary (see above); always `0` for `KEEP`. |
| 8 | `source` | Exact provenance — `dataset#sentence_id:token_index` for `ru`/`en`/`trap_abbrev`, `usr-share-doc:<path>:<line>` / `lay-x11:<path>:<line>` / `usr-include:<header>:<line>` for `trap_url`/`trap_code`, `dpkg:<package>` for `trap_num`. Every row can be independently re-verified against its source. |

## Running the validator

```bash
cd "docs/research/corpus2"
python3 validate.py            # validates ./heldout.tsv
python3 validate.py other.tsv  # or an explicit path
```

Checks: 8-column structure, `expect ∈ {KEEP, CONVERT}`, `correct == input`
under `KEEP`, positional-table round-trip under `CONVERT` (with the
`correct != input` no-op guard), no full-row duplicates, no duplicate
`(context, input)` pairs, `ambiguous ∈ {0, 1}`. Prints category counts,
KEEP/CONVERT split, `ambiguous` share, and the length histogram (overall and
per category) — not gated against fixed targets, since this corpus's
composition is emergent from real text rather than a generation spec.
Current status: **`OK: all checks passed`**.

## Reproducing

Re-download the two source files (same URLs as above), tokenize with the
regex in step 1 above, and re-run with `random.Random(137)` and
`P_CONVERT = 0.29` in the exact order described in "Labeling methodology."
The trap sources are: `/usr/share/doc/**` and `dpkg -l` on the build
machine (system-dependent — package versions in particular will differ on
another machine/date), and `git clone --depth 1
https://github.com/HelpFreedom/lay-x11` for the code identifiers.

## What this corpus does *not* claim to fix

It removes the "corpus == spec" circularity and the "debugged on the same
data it's measured on" concern from `06-bench-pulinto.md`. It does **not**
remove the structural ceiling noted there (real-word collisions like
`dyer`/`внук`) — it measures that ceiling directly via `ambiguous`, and
keeps every such row rather than discarding it.
