#!/usr/bin/env python3
import os
import json
import urllib.request
import urllib.parse
import re

SOURCES_DIR = os.path.join(os.path.dirname(__file__), "..", "docs", "research", "corpus3", "sources")
os.makedirs(SOURCES_DIR, exist_ok=True)

# Map category -> lang -> list of Wikipedia titles
CATEGORIES = {
    "literature": {
        "ru": ["Русская_литература", "Романтизм", "Фёдор_Достоевский", "Лев_Толстой", "Серебряный_век_русской_поэзии", "Повесть", "Литературоведение"],
        "en": ["English_literature", "Romanticism", "Charles_Dickens", "Jane_Austen", "Modernist_literature", "Novella", "Literary_criticism"]
    },
    "speech": {
        "ru": ["Ораторское_искусство", "Публичное_выступление", "Риторика", "Дискурс", "Оратор", "Убеждение"],
        "en": ["Public_speaking", "Rhetoric", "Debate", "Persuasion", "Discourse_analysis", "Oratory"]
    },
    "names": {
        "ru": ["Русское_имя", "Антропонимика", "Топонимика", "Фамилия", "Ономастика", "Отчество", "Географическое_название"],
        "en": ["Personal_name", "Onomastics", "Toponymy", "Surname", "Etymology", "Patronymic", "Place_name"]
    },
    "it": {
        "ru": ["Компьютерная_сеть", "Операционная_система", "База_данных", "Искусственный_интеллект", "Веб-разработка", "Программная_инженерия", "Компьютерная_безопасность"],
        "en": ["Computer_network", "Operating_system", "Database", "Artificial_intelligence", "Web_development", "Software_engineering", "Computer_security"]
    },
    "gaming": {
        "ru": ["Компьютерная_игра", "Игровой_процесс", "Жанры_компьютерных_игр", "Трехмерная_графика", "Игровой_движок", "Киберспорт"],
        "en": ["Video_game", "Gameplay", "Video_game_genre", "3D_computer_graphics", "Game_engine", "Esports"]
    },
    "stem": {
        "ru": ["Квантовая_механика", "Молекулярная_биология", "Органическая_химия", "Теоретическая_физика", "Астрофизика", "Дифференциальное_уравнение", "Генетика"],
        "en": ["Quantum_mechanics", "Molecular_biology", "Organic_chemistry", "Theoretical_physics", "Astrophysics", "Differential_equation", "Genetics"]
    },
    "law": {
        "ru": ["Римское_право", "Конституционное_право", "Международное_право", "Юриспруденция", "Судебная_система", "Гражданское_право"],
        "en": ["Roman_law", "Constitutional_law", "International_law", "Jurisprudence", "Judiciary", "Civil_law_(legal_system)"]
    },
    "medicine": {
        "ru": ["Анатомия_человека", "Физиология", "Кардиология", "Неврология", "Иммунология", "Фармакология", "Эпидемиология"],
        "en": ["Human_anatomy", "Physiology", "Cardiology", "Neurology", "Immunology", "Pharmacology", "Epidemiology"]
    },
    "finance": {
        "ru": ["Макроэкономика", "Фондовый_рынок", "Банковская_система", "Инфляция", "Корпоративные_финансы", "Инвестиции"],
        "en": ["Macroeconomics", "Stock_market", "Banking", "Inflation", "Corporate_finance", "Investment"]
    },
    "construction": {
        "ru": ["Строительство", "Архитектура", "Железобетон", "Градостроительство", "Фундамент", "Строительные_материалы"],
        "en": ["Construction", "Architecture", "Reinforced_concrete", "Urban_planning", "Foundation_(engineering)", "Building_material"]
    },
    "auto": {
        "ru": ["Автомобиль", "Двигатель_внутреннего_сгорания", "Подвеска_автомобиля", "Трансмиссия", "Электромобиль", "Автомобильная_промышленность"],
        "en": ["Automobile", "Internal_combustion_engine", "Suspension_(vehicle)", "Transmission_(mechanics)", "Electric_vehicle", "Automotive_industry"]
    },
    "geo": {
        "ru": ["География", "Физическая_география", "Тектоника_плит", "Климатология", "Океанология", "Картография"],
        "en": ["Geography", "Physical_geography", "Plate_tectonics", "Climatology", "Oceanography", "Cartography"]
    }
}

headers = {'User-Agent': 'PuLinToCorpusBot/2.0 (research@pulinto.org)'}

metadata_sources = []

def fetch_wiki_article(lang, title):
    url = f"https://{lang}.wikipedia.org/w/api.php"
    params = {
        "action": "query",
        "prop": "extracts",
        "explaintext": "1",
        "titles": title,
        "format": "json"
    }
    full_url = f"{url}?{urllib.parse.urlencode(params)}"
    web_url = f"https://{lang}.wikipedia.org/wiki/{urllib.parse.quote(title)}"
    
    req = urllib.request.Request(full_url, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            code = resp.status
            data = json.loads(resp.read().decode('utf-8'))
            pages = data.get('query', {}).get('pages', {})
            extract = ""
            for page_id, page_data in pages.items():
                if page_id != "-1":
                    extract = page_data.get('extract', '')
            return code, web_url, extract
    except Exception as e:
        print(f"Error fetching {full_url}: {e}")
        return 500, web_url, ""

def clean_extract(text):
    lines = []
    for line in text.splitlines():
        line = line.strip()
        # Filter out empty lines or section header lines like == See also ==
        if not line or line.startswith("=") and line.endswith("="):
            continue
        lines.append(line)
    return "\n".join(lines)

def main():
    source_index = {}
    for cat, langs in CATEGORIES.items():
        for lang, titles in langs.items():
            cell_key = f"{cat}_{lang}"
            print(f"Processing cell: {cell_key}...")
            cell_file = os.path.join(SOURCES_DIR, f"{cell_key}.txt")
            all_lines = []
            
            for title in titles:
                code, web_url, raw_text = fetch_wiki_article(lang, title)
                cleaned = clean_extract(raw_text)
                if cleaned:
                    lines = [l for l in cleaned.splitlines() if len(l) > 20]
                    all_lines.extend(lines)
                    print(f"  Fetched '{title}' ({lang}): {len(lines)} lines, HTTP {code}")
                    metadata_sources.append({
                        "category": cat,
                        "lang": lang,
                        "name": f"Wikipedia: {title}",
                        "url": web_url,
                        "http_code": code,
                        "license": "CC BY-SA 4.0",
                        "local_file": f"docs/research/corpus3/sources/{cell_key}.txt",
                        "title": title
                    })
            
            with open(cell_file, "w", encoding="utf-8") as f:
                f.write("\n".join(all_lines))
            print(f"Saved {cell_file}: {len(all_lines)} lines total.")

    with open(os.path.join(SOURCES_DIR, "metadata.json"), "w", encoding="utf-8") as f:
        json.dump(metadata_sources, f, ensure_ascii=False, indent=2)
    print("Done fetching sources.")

if __name__ == "__main__":
    main()
