#!/usr/bin/env python3
"""Собирает сайт GitHub Pages в каталог `_site/`.

Лендинг берётся из `site/index.html`, документация — из Markdown-файлов,
перечисленных в PAGES. Адреса репозитория и сайта берутся из окружения CI
(`GITHUB_REPOSITORY`), локально — из переменных PULINTO_REPO и PULINTO_SITE
либо из значений по умолчанию ниже.

    python3 -I tools/build-site.py [--out _site]

Нужен только пакет `markdown` (python3-markdown или pip install markdown).
"""

import argparse
import html
import json
import os
import re
import shutil
import sys
from pathlib import Path

import markdown

ROOT = Path(__file__).resolve().parent.parent

if os.environ.get("GITHUB_REPOSITORY"):
    owner, _, name = os.environ["GITHUB_REPOSITORY"].partition("/")
    DEFAULT_REPO = f"https://github.com/{owner}/{name}"
    DEFAULT_SITE = f"https://{owner.lower()}.github.io/{name}"
else:
    DEFAULT_REPO = "https://github.com/iResultMan/pulinto"
    DEFAULT_SITE = "https://iresultman.github.io/pulinto"
REPO = os.environ.get("PULINTO_REPO", DEFAULT_REPO).rstrip("/")
SITE = os.environ.get("PULINTO_SITE", DEFAULT_SITE).rstrip("/")
BRANCH = os.environ.get("PULINTO_BRANCH", "main")
CONTACT = "i.result.man@gmail.com"

# slug, заголовок в меню, исходный файл, группа
PAGES = [
    ("manual", "Руководство", "docs/MANUAL.md", "Пользователю"),
    ("faq", "Вопросы и ответы", "docs/FAQ.md", "Пользователю"),
    ("measurements", "Замеры и методика", "docs/MEASUREMENTS.md", "Как это устроено"),
    ("data", "Данные и лицензии", "data/ATTRIBUTION.md", "Как это устроено"),
    ("testing", "Помощь с тестированием", "docs/TESTING.md", "Проект"),
    ("languages", "Другие языки и трудные слова", "docs/LANGUAGES.md", "Проект"),
    ("languagepack", "Как собрать языковой пакет", "docs/LANGUAGE_PACK.md", "Проект"),
    ("contributing", "Участие в проекте", "CONTRIBUTING.md", "Проект"),
    ("security", "Безопасность", "SECURITY.md", "Проект"),
]
SLUG_BY_SOURCE = {src: slug for slug, _, src, _ in PAGES}


def slugify(value, separator="-"):
    value = re.sub(r"[^\w\s-]", "", value.lower(), flags=re.UNICODE).strip()
    return re.sub(r"[\s_-]+", separator, value)


def rel(depth):
    return "../" * depth if depth else "./"


def page_url(slug):
    return f"docs/{slug}/"


def fix_links(body, source, depth):
    """Ссылки между документами ведут на страницы сайта, остальные — на GitHub."""
    source_dir = Path(source).parent

    def repl(match):
        href = match.group(1)
        if re.match(r"^(https?:|mailto:|#|data:)", href):
            return match.group(0)
        path, _, frag = href.partition("#")
        target = (source_dir / path).as_posix()
        target = os.path.normpath(target).replace("\\", "/")
        if target in SLUG_BY_SOURCE:
            url = rel(depth) + page_url(SLUG_BY_SOURCE[target])
        else:
            full = ROOT / target
            kind = "tree" if full.is_dir() else "blob"
            url = f"{REPO}/{kind}/{BRANCH}/{target}"
            frag = ""
        if frag:
            url += "#" + frag
        return f'href="{url}"'

    body = re.sub(r'href="([^"]*)"', repl, body)

    def image(match):
        src = match.group(1)
        if re.match(r"^(https?:|data:|/)", src):
            return match.group(0)
        target = os.path.normpath((source_dir / src).as_posix()).replace("\\", "/")
        return f'src="{rel(depth)}{target}"'

    return re.sub(r'src="([^"]*)"', image, body)


def first_paragraph(text):
    for block in re.split(r"\n\s*\n", text):
        block = block.strip()
        if block and not block.startswith(("#", "```", "|", "<", "-", "*", ">", "1.", "[English version below]")):
            plain = re.sub(r"[`*_]|\[([^\]]+)\]\([^)]*\)", r"\1", block)
            return " ".join(plain.split())[:200]
    return "Документация PuLinTo."


def render_markdown(text):
    md = markdown.Markdown(
        extensions=["fenced_code", "tables", "toc", "sane_lists", "attr_list"],
        extension_configs={"toc": {"slugify": slugify, "toc_depth": "2-3"}},
    )
    body = md.convert(text)
    return body, md.toc_tokens


def layout(*, title, description, canonical, depth, body, nav_html="", extra_head="", is_doc=False):
    r = rel(depth)
    docs_link = f'<a href="{r}docs/">Документация</a>'
    return f"""<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)}</title>
<meta name="description" content="{html.escape(description, quote=True)}">
<meta name="theme-color" content="#f5f7f5" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#111512" media="(prefers-color-scheme: dark)">
<link rel="canonical" href="{canonical}">
<meta property="og:type" content="website">
<meta property="og:title" content="{html.escape(title, quote=True)}">
<meta property="og:description" content="{html.escape(description, quote=True)}">
<meta property="og:url" content="{canonical}">
<link rel="icon" href="{r}favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="{r}style.css">
{extra_head}</head>
<body>
<a class="skip" href="#content">К содержимому</a>
<header class="wrap top">
  <a class="mark" href="{r}">PuLinTo</a>
  <nav class="nav" aria-label="Разделы">
    {docs_link}
    <a href="{REPO}">GitHub</a>
  </nav>
</header>
{body}
<footer class="foot">
  <div class="wrap">
    <ul>
      <li><a href="{REPO}">Исходный код</a></li>
      <li><a href="{r}docs/manual/">Руководство</a></li>
      <li><a href="{REPO}/issues">Сообщить об ошибке</a></li>
      <li><a href="mailto:{CONTACT}">{CONTACT}</a></li>
    </ul>
    <p>Проект не связан с Яндексом и не является продуктом Punto Switcher. PuLinTo — независимая свободная программа со своим кодом и своими данными.</p>
    <p class="faint">© Yury ResultMan. Код под лицензией MIT или Apache-2.0, на выбор. Языковые данные — CC BY 4.0.</p>
  </div>
</footer>
{"<script src=\"" + r + "search.js\" defer></script>" if is_doc else ""}
</body>
</html>
"""


def sidebar(current_slug, tokens, depth):
    r = rel(depth)
    groups = {}
    for slug, label, _, group in PAGES:
        groups.setdefault(group, []).append((slug, label))
    out = ['<nav class="side" aria-label="Документация"><details class="side-box" open>'
           '<summary>Содержание</summary>',
           '<form class="search" role="search"><label class="sr" for="q">Поиск по документации</label>'
           '<input id="q" type="search" placeholder="Поиск" autocomplete="off">'
           '<ul id="hits" class="hits" hidden></ul></form>']
    for group, items in groups.items():
        out.append(f'<p class="side-group">{html.escape(group)}</p><ul>')
        for slug, label in items:
            current = ' aria-current="page"' if slug == current_slug else ""
            out.append(f'<li><a href="{r}{page_url(slug)}"{current}>{html.escape(label)}</a>')
            if slug == current_slug and tokens:
                out.append('<ul class="toc">')
                for t in tokens:
                    out.append(f'<li><a href="#{t["id"]}">{html.escape(t["name"])}</a></li>')
                out.append("</ul>")
            out.append("</li>")
        out.append("</ul>")
    out.append("</details></nav>")
    return "".join(out)


def text_of(html_fragment):
    return " ".join(re.sub(r"<[^>]+>", " ", html.unescape(html_fragment)).split())


def json_ld(data):
    text = json.dumps(data, ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")
    return f'<script type="application/ld+json">{text}</script>\n'


def faq_entries(body):
    """Пары «вопрос — ответ» со страницы, где каждый вопрос — заголовок второго уровня."""
    entries = []
    for part in re.split(r"(?=<h2 id=)", body):
        m = re.match(r'<h2 id="(?!english")[^"]+">(.*?)</h2>(.*)', part, re.S)
        if m and text_of(m.group(2)):
            entries.append({
                "@type": "Question",
                "name": text_of(m.group(1)),
                "acceptedAnswer": {"@type": "Answer", "text": text_of(m.group(2))},
            })
    return entries


def build_doc_pages(out, search):
    for index, (slug, label, source, _) in enumerate(PAGES):
        path = ROOT / source
        if not path.exists():
            sys.exit(f"нет файла {source}")
        text = path.read_text(encoding="utf-8").replace("https://github.com/iResultMan/pulinto", REPO)
        body, tokens = render_markdown(text)
        depth = 2
        body = fix_links(body, source, depth)
        title_match = re.search(r"<h1[^>]*>(.*?)</h1>", body, re.S)
        h1 = text_of(title_match.group(1)) if title_match else label
        prev_next = []
        if index > 0:
            p = PAGES[index - 1]
            prev_next.append(f'<a rel="prev" href="../../{page_url(p[0])}">← {html.escape(p[1])}</a>')
        if index + 1 < len(PAGES):
            n = PAGES[index + 1]
            prev_next.append(f'<a rel="next" href="../../{page_url(n[0])}">{html.escape(n[1])} →</a>')
        crumbs = (f'<nav class="crumbs" aria-label="Путь"><a href="../../">Главная</a> / '
                  f'<a href="../">Документация</a> / <span aria-current="page">{html.escape(label)}</span></nav>')
        extra = json_ld({
            "@context": "https://schema.org", "@type": "BreadcrumbList",
            "itemListElement": [
                {"@type": "ListItem", "position": 1, "name": "PuLinTo", "item": f"{SITE}/"},
                {"@type": "ListItem", "position": 2, "name": "Документация", "item": f"{SITE}/docs/"},
                {"@type": "ListItem", "position": 3, "name": label, "item": f"{SITE}/{page_url(slug)}"},
            ]})
        if slug == "faq":
            extra += json_ld({"@context": "https://schema.org", "@type": "FAQPage", "mainEntity": faq_entries(body)})
        page = f"""<main class="wrap docs" id="content">
{sidebar(slug, tokens, depth)}
<article class="doc">
{crumbs}
{body}
<footer class="doc-foot">
<p><a href="{REPO}/edit/{BRANCH}/{source}">Править эту страницу на GitHub</a></p>
<p class="pn">{" ".join(prev_next)}</p>
</footer>
</article>
</main>"""
        html_text = layout(
            title=h1 if "PuLinTo" in h1 else f"{h1} — PuLinTo",
            description=first_paragraph(text),
            canonical=f"{SITE}/{page_url(slug)}",
            depth=depth,
            body=page,
            extra_head=extra + f'<meta property="og:image" content="{SITE}/img/og.png">\n',
            is_doc=True,
        )
        target = out / "docs" / slug
        target.mkdir(parents=True, exist_ok=True)
        (target / "index.html").write_text(html_text, encoding="utf-8")
        # поисковый индекс: по разделам страницы
        parts = re.split(r'(?=<h[23] id=")', body)
        for part in parts:
            m = re.match(r'<h[23] id="([^"]+)">(.*?)</h[23]>', part, re.S)
            anchor = m.group(1) if m else ""
            heading = text_of(m.group(2)) if m else h1
            search.append({
                "url": page_url(slug) + (f"#{anchor}" if anchor else ""),
                "page": label,
                "title": heading,
                "text": text_of(part)[:600],
            })


def build_docs_index(out):
    groups = {}
    for slug, label, source, group in PAGES:
        text = (ROOT / source).read_text(encoding="utf-8")
        groups.setdefault(group, []).append((slug, label, first_paragraph(text)))
    cards = []
    for group, items in groups.items():
        cards.append(f'<section class="idx-group"><h2>{html.escape(group)}</h2><ul class="idx">')
        for slug, label, blurb in items:
            cards.append(f'<li><a href="{page_url(slug)[len("docs/"):]}">{html.escape(label)}</a>'
                         f'<span>{html.escape(blurb)}</span></li>')
        cards.append("</ul></section>")
    body = f"""<main class="wrap" id="content">
<h1 class="page-title">Документация</h1>
<p class="lead">Как поставить и настроить PuLinTo, как он решает, что слово набрано не в той раскладке, и на чём основаны замеры точности.</p>
{"".join(cards)}
<p class="small faint">Что-то непонятно или не работает: <a href="{REPO}/issues">заведите обращение</a> или напишите на <a href="mailto:{CONTACT}">{CONTACT}</a>.</p>
</main>"""
    target = out / "docs"
    target.mkdir(parents=True, exist_ok=True)
    (target / "index.html").write_text(
        layout(title="Документация — PuLinTo", description="Руководство, вопросы и ответы, алгоритм, замеры и лицензии данных PuLinTo.",
               canonical=f"{SITE}/docs/", depth=1, body=body),
        encoding="utf-8")


def build_landing(out):
    text = (ROOT / "site/index.html").read_text(encoding="utf-8")
    text = text.replace("{{REPO}}", REPO).replace("{{SITE}}", SITE).replace("{{CONTACT}}", CONTACT)
    (out / "index.html").write_text(text, encoding="utf-8")

    en_src = ROOT / "site/en.html"
    if en_src.exists():
        en_text = en_src.read_text(encoding="utf-8")
        en_text = en_text.replace("{{REPO}}", REPO).replace("{{SITE}}", SITE).replace("{{CONTACT}}", CONTACT)
        (out / "en.html").write_text(en_text, encoding="utf-8")
        (out / "en").mkdir(parents=True, exist_ok=True)
        (out / "en" / "index.html").write_text(en_text, encoding="utf-8")

    # The one-command installer is served from the site root.
    bootstrap = (ROOT / "packaging/get-pulinto.sh").read_text(encoding="utf-8")
    (out / "install.sh").write_text(bootstrap.replace("@REPO@", REPO).replace("@SITE@", SITE), encoding="utf-8")


def build_404(out):
    body = f"""<main class="wrap" id="content">
<h1 class="page-title">Страницы нет</h1>
<p class="lead">Такого адреса на сайте нет. Начните с <a href="{SITE}/docs/">документации</a> или с <a href="{SITE}/">главной страницы</a>.</p>
</main>"""
    (out / "404.html").write_text(
        layout(title="Страницы нет — PuLinTo", description="Страница не найдена.", canonical=f"{SITE}/404.html", depth=0, body=body)
        .replace('href="./style.css"', f'href="{SITE}/style.css"').replace('href="./favicon.svg"', f'href="{SITE}/favicon.svg"')
        .replace('href="./docs/', f'href="{SITE}/docs/').replace('href="./"', f'href="{SITE}/"'),
        encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="_site", type=Path)
    args = parser.parse_args()
    out = args.out if args.out.is_absolute() else ROOT / args.out
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    for name in ("style.css", "favicon.svg", "search.js", "demo.js"):
        shutil.copy(ROOT / "site" / name, out / name)
    shutil.copytree(ROOT / "site/fonts", out / "fonts")
    shutil.copytree(ROOT / "site/img", out / "img")
    if (ROOT / "docs/img").is_dir():
        shutil.copytree(ROOT / "docs/img", out / "docs/img")
    (out / ".nojekyll").write_text("", encoding="utf-8")

    search = []
    build_landing(out)
    build_docs_index(out)
    build_doc_pages(out, search)
    build_404(out)
    (out / "search-index.json").write_text(json.dumps(search, ensure_ascii=False), encoding="utf-8")

    urls = [f"{SITE}/", f"{SITE}/en/", f"{SITE}/docs/"] + [f"{SITE}/{page_url(s)}" for s, *_ in PAGES]
    (out / "sitemap.xml").write_text(
        '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
        + "".join(f"  <url><loc>{u}</loc></url>\n" for u in urls) + "</urlset>\n", encoding="utf-8")
    (out / "robots.txt").write_text(f"User-agent: *\nAllow: /\nSitemap: {SITE}/sitemap.xml\n", encoding="utf-8")
    print(f"собрано в {out}: {len(PAGES)} страниц документации, {len(search)} записей поиска; сайт {SITE}")


if __name__ == "__main__":
    main()
