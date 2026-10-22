#!/usr/bin/env python3
"""Свежие тексты из интернета для сценария машинистки — по тематикам, на двух
языках, каждый раз новые.

Стенд до сих пор проверялся на корпусе 3 и жанровых сценариях из него — это
слова, которые мы знаем. Заказчик (18 октября 2026) просит боевые условия:
произвольные статьи из интернета в каждой тематике, русские и английские,
перемешанные, и пересобирать такой блок заново каждый раз. Этот скрипт
качает тексты, `make_script.py --sources` режет их на сценарий.

    fetch_web_sources.py --out docs/research/corpus-web/2026-10-18-1430 \\
        [--words 3000] [--seed 7]

Источники (RSS → страницы статей, абзацы <p>; API Викитеки и Hacker News):
    news        lenta.ru, ria.ru            | BBC News
    it          habr.com                    | The Verge, BBC Technology
    science     nplus1.ru                   | phys.org, BBC Science
    business    rbc.ru                      | BBC Business
    sport       sports.ru                   | BBC Sport
    literature  ru.wikisource (случайные)   | en.wikisource (случайные)
    speech      pikabu.ru, dtf.ru           | комментарии Hacker News
    (наука дополняется случайными статьями Википедии из категорий физика,
    биология, химия, астрономия; phys.org закрыт для роботов)

Выход: `<out>/sources/<topic>_{ru,en}.txt` (абзац на строке) и
`<out>/manifest.json` (адреса, счётчики слов, seed, время). Только стандартная
библиотека. Ничего не печатает в окна — это сбор текста, не стенд.
"""
import argparse
import json
import os
import random
import re
import sys
import time
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from html import unescape
from html.parser import HTMLParser

UA = "Mozilla/5.0 (X11; Linux x86_64) PuLinTo-stand/0.1 (test corpus builder)"
TIMEOUT = 15
PAUSE = 0.5  # между запросами к одному и тому же сайту — не долбить
WIKI_PAUSE = 1.2  # API Викимедиа отвечает 429 на частые запросы без паузы

SOURCES = {
    # topic: {lang: [(kind, url), ...]}
    "news": {
        "ru": [("rss", "https://lenta.ru/rss/news"), ("rss", "https://ria.ru/export/rss2/archive/index.xml")],
        "en": [("rss", "https://feeds.bbci.co.uk/news/rss.xml")],
    },
    "it": {
        "ru": [("rss", "https://habr.com/ru/rss/articles/")],
        "en": [("rss", "https://www.theverge.com/rss/index.xml"), ("rss", "https://feeds.bbci.co.uk/news/technology/rss.xml")],
    },
    "science": {
        "ru": [("rss", "https://nplus1.ru/rss"),
               ("wikipedia", "https://ru.wikipedia.org/w/api.php|Категория:Физика|Категория:Биология|Категория:Химия|Категория:Астрономия")],
        "en": [("rss", "https://feeds.bbci.co.uk/news/science_and_environment/rss.xml"),
               ("wikipedia", "https://en.wikipedia.org/w/api.php|Category:Physics|Category:Biology|Category:Chemistry|Category:Astronomy")],
    },
    "business": {
        "ru": [("rss", "https://rssexport.rbc.ru/rbcnews/news/30/full.rss")],
        "en": [("rss", "https://feeds.bbci.co.uk/news/business/rss.xml")],
    },
    "sport": {
        "ru": [("rss", "https://www.sports.ru/rss/all_news.xml")],
        "en": [("rss", "https://feeds.bbci.co.uk/sport/rss.xml")],
    },
    "literature": {
        "ru": [("wikisource", "https://ru.wikisource.org/w/api.php")],
        "en": [("wikisource", "https://en.wikisource.org/w/api.php")],
    },
    "speech": {
        "ru": [("rss", "https://pikabu.ru/xmlfeeds.php?cmd=hot"), ("rss", "https://dtf.ru/rss")],
        "en": [("hn", "https://hn.algolia.com/api/v1/search?tags=comment&hitsPerPage=100")],
    },
}

CYRILLIC = re.compile(r"[а-яё]", re.IGNORECASE)
LATIN = re.compile(r"[a-z]", re.IGNORECASE)


def get(url, binary=False):
    request = urllib.request.Request(url, headers={"User-Agent": UA, "Accept-Language": "ru,en"})
    for attempt in (1, 2):
        try:
            with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
                data = response.read()
            return data if binary else data.decode("utf-8", "replace")
        except urllib.error.HTTPError as error:
            if error.code == 429 and attempt == 1:
                time.sleep(float(error.headers.get("Retry-After", 10) or 10))
                continue
            raise


def plain_paragraphs(fragment):
    """Абзацы HTML-фрагмента (описание RSS, комментарий): `<p>` и `<br>` —
    границы, остальные теги снимаются."""
    text = re.sub(r"(?i)<\s*(p|br|div|li)[^>]*>", "\n", unescape(fragment))
    text = re.sub(r"<[^>]+>", " ", text)
    return [" ".join(line.split()) for line in text.split("\n") if line.strip()]


class Paragraphs(HTMLParser):
    """Текст абзацев <p> страницы; скрипты, стили и подписи не считаются."""

    def __init__(self):
        super().__init__()
        self.paragraphs, self.current, self.depth, self.skip = [], [], 0, 0

    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style", "noscript", "figure", "aside", "nav", "footer", "header"):
            self.skip += 1
        if tag == "p":
            self.depth += 1
            self.current = []

    def handle_endtag(self, tag):
        if tag in ("script", "style", "noscript", "figure", "aside", "nav", "footer", "header"):
            self.skip = max(0, self.skip - 1)
        if tag == "p" and self.depth:
            self.depth -= 1
            text = " ".join("".join(self.current).split())
            if text:
                self.paragraphs.append(text)

    def handle_data(self, data):
        if self.depth and not self.skip:
            self.current.append(data)


def paragraphs_of(html):
    parser = Paragraphs()
    parser.feed(html)
    return parser.paragraphs


def good(paragraph, lang):
    """Абзац своего языка: не меньше 8 слов, 70 % букв — из его алфавита."""
    words = paragraph.split()
    if len(words) < 8:
        return False
    letters = CYRILLIC.findall(paragraph) + LATIN.findall(paragraph)
    if not letters:
        return False
    own = CYRILLIC if lang == "ru" else LATIN
    return len(own.findall(paragraph)) / len(letters) >= 0.7


def rss_items(url):
    root = ET.fromstring(get(url, binary=True))
    items = []
    for item in root.iter():
        if item.tag.endswith("item") or item.tag.endswith("entry"):
            link = item.findtext("link") or ""
            if not link:
                for child in item:
                    if child.tag.endswith("link"):
                        link = child.get("href") or child.text or ""
            description = item.findtext("description") or ""
            for child in item:
                if child.tag.endswith("encoded") or child.tag.endswith("content") or child.tag.endswith("full-text"):
                    description = child.text or description
            items.append((link.strip(), description))
    return items


def from_rss(url, lang, need, rng, log):
    out = []
    try:
        items = rss_items(url)
    except Exception as error:  # noqa: BLE001 — один источник не роняет сбор
        log.append({"url": url, "error": repr(error)})
        return out
    rng.shuffle(items)
    for link, description in items:
        if sum(len(p.split()) for p in out) >= need:
            break
        texts = []
        if description and len(description) > 400:
            # Habr и rbc кладут в RSS текст статьи целиком — он лучше страницы.
            texts = plain_paragraphs(description)
        if not texts and link:
            try:
                time.sleep(PAUSE)
                texts = paragraphs_of(get(link))
            except Exception as error:  # noqa: BLE001
                log.append({"url": link, "error": repr(error)})
                continue
        kept = [p for p in texts if good(p, lang)]
        if kept:
            out.extend(kept)
            log.append({"url": link or url, "words": sum(len(p.split()) for p in kept)})
    return out


def from_wikisource(api, lang, need, rng, log):
    """Случайные страницы Викитеки, разобранные через `action=parse` — экстракты
    (`prop=extracts`) для Викитеки по большей части пусты."""
    out = []
    tries = 0
    while sum(len(p.split()) for p in out) < need and tries < 12:
        tries += 1
        try:
            time.sleep(WIKI_PAUSE)
            query = urllib.parse.urlencode({"action": "query", "list": "random", "rnnamespace": 0, "rnlimit": 5, "format": "json"})
            titles = [page["title"] for page in json.loads(get(f"{api}?{query}"))["query"]["random"]]
        except Exception as error:  # noqa: BLE001
            log.append({"url": api, "error": repr(error)})
            continue
        for title in titles:
            if sum(len(p.split()) for p in out) >= need:
                break
            query = urllib.parse.urlencode({"action": "parse", "page": title, "prop": "text", "disabletoc": 1, "format": "json"})
            try:
                time.sleep(WIKI_PAUSE)
                html = json.loads(get(f"{api}?{query}"))["parse"]["text"]["*"]
            except Exception as error:  # noqa: BLE001
                log.append({"url": title, "error": repr(error)})
                continue
            kept = [p for p in paragraphs_of(html) if good(p, lang)][:40]
            if kept:
                out.extend(kept)
                log.append({"url": f"{api.rsplit('/w/', 1)[0]}/wiki/{title}", "words": sum(len(p.split()) for p in kept)})
    return out


def from_hn(url, lang, need, rng, log):
    out = []
    page = rng.randrange(0, 8)  # Algolia отдаёт не больше 1000 результатов
    tries = 0
    while sum(len(p.split()) for p in out) < need and tries < 10:
        tries += 1
        try:
            time.sleep(PAUSE)
            hits = json.loads(get(f"{url}&page={page + tries}"))["hits"]
        except Exception as error:  # noqa: BLE001
            log.append({"url": url, "error": repr(error)})
            continue
        for hit in hits:
            for paragraph in plain_paragraphs(hit.get("comment_text") or ""):
                if good(paragraph, lang):
                    out.append(paragraph)
        log.append({"url": f"{url}&page={page + tries}", "words": sum(len(p.split()) for p in out)})
    return out


def from_wikipedia(spec, lang, need, rng, log):
    """Случайные статьи из перечисленных категорий (`api|Категория:…|…`)."""
    api, *categories = spec.split("|")
    out = []
    titles = []
    for category in categories:
        query = urllib.parse.urlencode({"action": "query", "list": "categorymembers", "cmtitle": category,
                                        "cmnamespace": 0, "cmlimit": 200, "format": "json"})
        try:
            time.sleep(WIKI_PAUSE)
            titles.extend(m["title"] for m in json.loads(get(f"{api}?{query}"))["query"]["categorymembers"])
        except Exception as error:  # noqa: BLE001
            log.append({"url": f"{api} {category}", "error": repr(error)})
    rng.shuffle(titles)
    for title in titles:
        if sum(len(p.split()) for p in out) >= need:
            break
        query = urllib.parse.urlencode({"action": "query", "prop": "extracts", "explaintext": 1, "exchars": 5000,
                                        "titles": title, "format": "json"})
        try:
            time.sleep(WIKI_PAUSE)
            page = next(iter(json.loads(get(f"{api}?{query}"))["query"]["pages"].values()))
        except Exception as error:  # noqa: BLE001
            log.append({"url": title, "error": repr(error)})
            continue
        kept = [" ".join(p.split()) for p in page.get("extract", "").split("\n") if good(" ".join(p.split()), lang)]
        if kept:
            out.extend(kept)
            log.append({"url": f"{api.rsplit('/w/', 1)[0]}/wiki/{title}", "words": sum(len(p.split()) for p in kept)})
    return out


FETCHERS = {"rss": from_rss, "wikisource": from_wikisource, "hn": from_hn, "wikipedia": from_wikipedia}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", required=True, help="каталог блока; внутри sources/ и manifest.json")
    parser.add_argument("--words", type=int, default=3000, help="слов на тематику и язык (по умолчанию 3000)")
    parser.add_argument("--seed", type=int, default=None)
    parser.add_argument("--topics", default="", help="через запятую; по умолчанию все")
    args = parser.parse_args()
    seed = args.seed if args.seed is not None else random.SystemRandom().randrange(1 << 31)
    rng = random.Random(seed)
    wanted = {t.strip() for t in args.topics.split(",") if t.strip()} or set(SOURCES)

    sources_dir = os.path.join(args.out, "sources")
    os.makedirs(sources_dir, exist_ok=True)
    manifest = {"seed": seed, "started": time.strftime("%Y-%m-%d %H:%M:%S"), "words_per_source": args.words, "topics": {}}
    for topic in SOURCES:
        if topic not in wanted:
            continue
        for lang, entries in SOURCES[topic].items():
            paragraphs, log = [], []
            per_source = max(1, args.words // len(entries))
            for kind, url in entries:
                paragraphs.extend(FETCHERS[kind](url, lang, per_source, rng, log))
            rng.shuffle(paragraphs)
            path = os.path.join(sources_dir, f"{topic}_{lang}.txt")
            with open(path, "w", encoding="utf-8") as handle:
                handle.write("\n".join(paragraphs) + ("\n" if paragraphs else ""))
            words = sum(len(p.split()) for p in paragraphs)
            manifest["topics"][f"{topic}_{lang}"] = {"words": words, "paragraphs": len(paragraphs), "pages": log}
            print(f"{topic:10s} {lang}  {words:6d} слов, {len(paragraphs):4d} абзацев, страниц {sum(1 for e in log if 'words' in e)}, ошибок {sum(1 for e in log if 'error' in e)}", flush=True)
            if words < args.words // 2:
                print(f"  МАЛО: {topic}_{lang} — {words} слов из {args.words}", file=sys.stderr, flush=True)
    manifest["finished"] = time.strftime("%Y-%m-%d %H:%M:%S")
    with open(os.path.join(args.out, "manifest.json"), "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, ensure_ascii=False, indent=1)
    print(f"seed {seed}; источники в {sources_dir}", flush=True)


if __name__ == "__main__":
    main()
