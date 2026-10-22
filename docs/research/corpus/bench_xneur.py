#!/usr/bin/env python3
"""
Воспроизведение каскада xneur 0.20.0 (check_lang из lib/ai/detection.c) на его
собственных данных (/usr/share/xneur/languages/{ru,en}) для замера качества
автоопределения неправильной раскладки на золотом корпусе.

ИСТОРИЯ ПОПЫТОК (см. отчет 05-bench-xneur.md за подробностями):

Вариант А (вызов настоящего libxneur) был опробован полностью после того, как
в систему поставили libxneur-dev (заголовки /usr/include/xneur/*.h,
pkg-config xneur). Публичный API (xneur_handle_create/xneur_get_layout/
xneur_get_word) экспортирует ровно то, что нужно, и живой X-сервер уже
сконфигурирован с двумя раскладками ru+us (setxkbmap -query). Harness
(bench_xneur.c рядом) успешно создает handle и окно (window_init), но падает
с SIGSEGV внутри XkbGetState() при первом же вызове xneur_get_word -- падение
воспроизводится стабильно, при этом изолированный тест голого XkbGetState()
на том же дисплее падения не дает, т.е. дело не в отсутствии XKB-negotiation,
а в состоянии, которое xneur формирует сам (вероятно, повреждение кучи в
недрах xneur_handle_create/window_init для этой сборки при вызове вне
собственного event-loop демона). Копаться дальше значило бы лезть в приватные
поля struct _window/_buffer (их нет в публичном заголовке, а сторонний клон
исходников на GitHub уже дал НЕСОВПАДАЮЩЕЕ смещение полей при проверке через
gdb) -- то есть чинить это можно только угадыванием ABI, а это ровно тот
случай, когда результат может стать тихо неверным, а не просто упасть. Раз
задача прямо запрещает подгонять замер -- на этом Вариант А остановлен.

Вариант Б (этот файл) -- точное воспроизведение алгоритма на РЕАЛЬНЫХ данных
той же версии xneur:
  - regex-словарь, proto (биграммы), proto3 (триграммы) читаются напрямую из
    /usr/share/xneur/languages/{ru,en}/*;
  - позиционные таблицы раскладок -- те же, что в
    crates/pulinto-core/src/layout.rs (совпадают с проверенными тестами
    conversion в этом файле, той же схемой, что использует xneur/Punto);
  - ступень орфографии -- НАСТОЯЩИЙ libenchant-2.so.2 (тот же файл, что
    подтвержден в NEEDED у /usr/lib/.../libxneur.so.20 через readelf/ldd),
    вызываемый через ctypes напрямую по enchant_dict_check() -- БЕЗ
    промежуточного `enchant-2 -a` (важно: тот CLI работает в ispell-pipe
    режиме и сам ТОКЕНИЗИРУЕТ строку по пунктуации, из-за чего слово вроде
    "ht,`yjr" распадается на "ht" + "yjr" и ломает построчный парсинг вывода
    -- проверено и отброшено; ctypes-вызов enchant_dict_check() атомарен по
    всей строке, ровно как делает сам xneur в get_enchant_hits()).
  - readelf/nm на libxneur.so.20.0.0 подтвердили: линкуется с
    libenchant-2.so.2, символов aspell в бинаре НЕТ. Т.е. эта сборка xneur
    0.20.0-3.3build1 скомпилирована WITH_ENCHANT и БЕЗ WITH_ASPELL -- ступень
    aspell в check_lang в этой сборке физически не выполняется (#ifdef
    WITH_ASPELL). Устанавливать/использовать отдельно aspell-ru было бы
    НЕВЕРНО для этой цели: реальный xneur его не спрашивает. Реальная цепочка
    на этой машине: regex-словарь -> enchant -> (Левенштейн) -> биграммы -> триграммы.

Что опущено (ограничения замера, см. отчет 05-bench-xneur.md):
  - ступень "похожие слова по Левенштейну" (get_similar_words) НЕ реализована.
    Важная деталь: сам публичный API xneur (xneur_get_layout/xneur_get_word,
    см. xneurlib.c) вызывает check_lang(handle, buffer, cur_lang, FALSE) --
    с check_similar_words=FALSE! Т.е. эта ступень отключена и в "настоящем"
    xneur при вызове через штатный публичный интерфейс -- отсутствие
    Левенштейна в нашей реализации не снижает достоверность замера.
  - `cur_lang` (активная раскладка на момент набора) не наблюдаема из TSV;
    восстанавливается эвристикой по алфавиту введенного текста (кириллица ->
    'ru', иначе -> 'us'); для чисто цифровых/символьных input это условно.
"""
import csv
import ctypes
import ctypes.util
import re
import sys
import unicodedata
from pathlib import Path

XNEUR_LANG_DIR = Path("/usr/share/xneur/languages")
NO_LANGUAGE = None

# --- Позиционные таблицы раскладок, идентичные crates/pulinto-core/src/layout.rs ---
LAYOUTS = {
    "us": {
        "lower": "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./",
        "upper": "~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?",
    },
    "ru": {
        "lower": "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю.",
        "upper": "Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,",
    },
}

# xneur держит отдельные каталоги "en" и "us" с идентичными данными (проверено:
# одинаковый размер файлов). Раскладке 'us' соответствуют язык-данные 'en'.
LANG_DATA_DIR = {"us": "en", "ru": "ru"}
ENCHANT_DICT = {"us": "en", "ru": "ru"}


def build_index(layout):
    index = {}
    for pos, ch in enumerate(layout["lower"]):
        index.setdefault(ch, (pos, "lower"))
    for pos, ch in enumerate(layout["upper"]):
        index.setdefault(ch, (pos, "upper"))
    return index


LAYOUT_INDEX = {name: build_index(layout) for name, layout in LAYOUTS.items()}


def char_at(layout_name, position, level):
    table = LAYOUTS[layout_name][level]
    if 0 <= position < len(table):
        return table[position]
    return None


def convert(text, from_layout, to_layout):
    """Ретайп text, будто те же клавиши нажаты под другой раскладкой."""
    idx = LAYOUT_INDEX[from_layout]
    out = []
    for ch in text:
        pos_level = idx.get(ch)
        if pos_level is None:
            out.append(ch)
            continue
        pos, level = pos_level
        mapped = char_at(to_layout, pos, level)
        out.append(mapped if mapped is not None else ch)
    return "".join(out)


def guess_typed_layout(text):
    """cur_lang не наблюдаем напрямую -- восстанавливаем по алфавиту ввода.
    Эвристика: есть кириллица -> 'ru', есть латиница -> 'us', иначе -> 'us'
    (по умолчанию для чисто цифро-символьных строк)."""
    has_cyr = any("CYRILLIC" in unicodedata.name(ch, "") for ch in text if ch.isalpha())
    has_lat = any("LATIN" in unicodedata.name(ch, "") for ch in text if ch.isalpha())
    if has_cyr and not has_lat:
        return "ru"
    if has_lat and not has_cyr:
        return "us"
    if has_cyr and has_lat:
        return "us"  # смешанный алфавит -- условно, редкий случай в корпусе
    return "us"  # нет букв вообще (пароль из цифр/символов и т.п.)


# --- Загрузка словарных regex (ступень 1: get_dictionary_lang) ---
def load_dictionary(lang_dir):
    patterns = []
    path = XNEUR_LANG_DIR / lang_dir / "dictionary"
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if not line:
                continue
            try:
                patterns.append(re.compile(line))
            except re.error as e:
                print(f"WARN: failed to compile regex {line!r} from {path}: {e}", file=sys.stderr)
    return patterns


def load_ngrams(lang_dir, filename):
    path = XNEUR_LANG_DIR / lang_dir / filename
    with open(path, encoding="utf-8") as f:
        return set(line.strip() for line in f if line.strip())


DICTIONARIES = {lo: load_dictionary(ld) for lo, ld in LANG_DATA_DIR.items()}
PROTO2 = {lo: load_ngrams(ld, "proto") for lo, ld in LANG_DATA_DIR.items()}
PROTO3 = {lo: load_ngrams(ld, "proto3") for lo, ld in LANG_DATA_DIR.items()}

LANGS = list(LAYOUTS.keys())  # ["us", "ru"]


# --- Ступень 0: препроцессинг слова, как в check_lang ---
def is_ascii_digit(ch):
    return ch in "0123456789"


def is_ascii_punct(ch):
    # ispunct() в C locale: ASCII-пунктуация, не буквы/цифры/пробелы.
    return ch.isascii() and not ch.isalnum() and not ch.isspace() and ch != ""


def preprocess(raw):
    """del_final_numeric_char + пропуск ведущей пунктуации/цифр.
    Возвращает (word, had_content) где word -- то, что реально идет в каскад."""
    s = raw
    # отрезать хвостовые цифры
    end = len(s)
    while end > 0 and is_ascii_digit(s[end - 1]):
        end -= 1
    s = s[:end]
    # пропустить ведущую пунктуацию и цифры
    offset = 0
    while offset < len(s) and (is_ascii_punct(s[offset]) or is_ascii_digit(s[offset])):
        offset += 1
    return s[offset:]


# --- Ступень 2: enchant, прямой вызов libenchant-2.so.2 через ctypes ---
#
# Вызов через `enchant-2 -a` (ispell-pipe CLI) был опробован первым и ОТБРОШЕН:
# этот режим сам токенизирует каждую строку ввода по пунктуации (проверено:
# "ht,`yjr" дает два ответа "ht"+"yjr" в одном блоке), из-за чего построчный
# парсер результатов рассинхронизируется на словах с апострофами, запятыми,
# дефисами и т.п. (а такие остаются после offset-препроцессинга, например
# буква "ё"/"`" в середине слова). Это давало тихо неверные вердикты --
# ровно то, чего нельзя допускать в этом замере. ctypes-вызов
# enchant_dict_check() проверяет ВСЮ переданную строку атомарно, без
# токенизации -- ровно как это делает сам xneur в get_enchant_hits()
# (detection.c): `enchant_dict_check(dict, word[lang], word_len)`.
class EnchantChecker:
    def __init__(self):
        libname = ctypes.util.find_library("enchant-2") or "libenchant-2.so.2"
        self.lib = ctypes.CDLL(libname)
        self.lib.enchant_broker_init.restype = ctypes.c_void_p
        self.lib.enchant_broker_dict_exists.restype = ctypes.c_int
        self.lib.enchant_broker_dict_exists.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self.lib.enchant_broker_request_dict.restype = ctypes.c_void_p
        self.lib.enchant_broker_request_dict.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self.lib.enchant_dict_check.restype = ctypes.c_int
        self.lib.enchant_dict_check.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t]
        self.broker = self.lib.enchant_broker_init()
        if not self.broker:
            raise RuntimeError("enchant_broker_init() failed")
        self.dicts = {}
        for lang, dict_name in ENCHANT_DICT.items():
            name_b = dict_name.encode("utf-8")
            if not self.lib.enchant_broker_dict_exists(self.broker, name_b):
                print(f"WARN: enchant dict '{dict_name}' not found for lang '{lang}'", file=sys.stderr)
                self.dicts[lang] = None
                continue
            d = self.lib.enchant_broker_request_dict(self.broker, name_b)
            self.dicts[lang] = d
            if not d:
                print(f"WARN: enchant_broker_request_dict('{dict_name}') failed", file=sys.stderr)

    def check(self, lang, word):
        """True, если word валиден по enchant_dict_check (возвращает 0)."""
        d = self.dicts.get(lang)
        if not d or len(word) <= 1:
            return False
        wb = word.encode("utf-8")
        result = self.lib.enchant_dict_check(d, wb, len(wb))
        return result == 0


def prime_enchant_cache(all_words_by_lang):
    checker = EnchantChecker()
    cache = {}
    for lang, words in all_words_by_lang.items():
        cache[lang] = {w: checker.check(lang, w) for w in words}
    return cache


# --- Ступени 4/5: биграммы / триграммы, бинарное вето ---
def has_banned_ngram(word, n, banned_set):
    if len(word) < n:
        return False
    for i in range(len(word) - n + 1):
        if word[i:i + n] in banned_set:
            return True
    return False


def get_proto_lang(word_by_lang, cur_lang, n, proto_sets, min_len):
    cur_word = word_by_lang[cur_lang]
    if len(cur_word) < min_len:
        return NO_LANGUAGE
    if not has_banned_ngram(cur_word, n, proto_sets[cur_lang]):
        return cur_lang
    for lang in LANGS:
        if lang == cur_lang:
            continue
        w = word_by_lang[lang]
        if len(w) == 0:
            continue
        if not has_banned_ngram(w, n, proto_sets[lang]):
            return lang
    return NO_LANGUAGE


def check_lang(raw_input, cur_lang, enchant_cache):
    """Воспроизводит check_lang() из detection.c для двух раскладок us/ru."""
    processed = {lang: preprocess(convert(raw_input, cur_lang, lang) if lang != cur_lang else preprocess_source(raw_input))
                 for lang in LANGS}
    # -- см. ниже: правильная реализация в build_word_by_lang --
    raise RuntimeError("unused")


def preprocess_source(raw_input):
    return raw_input


def build_word_by_lang(raw_input, cur_lang):
    """word[lang] для каждой раскладки: raw_input отретайпан под каждую
    раскладку (позиционно), затем применен препроцессинг (тот же offset/strip,
    т.к. цифры и ASCII-пунктуация позиционно одинаковы на обеих раскладках)."""
    out = {}
    for lang in LANGS:
        retyped = raw_input if lang == cur_lang else convert(raw_input, cur_lang, lang)
        out[lang] = preprocess(retyped)
    return out


def run_check_lang(raw_input, cur_lang, enchant_cache):
    word = build_word_by_lang(raw_input, cur_lang)

    # Ступень 1: regex-словарь
    for lang in LANGS:
        w = word[lang]
        if not w:
            continue
        for pat in DICTIONARIES[lang]:
            if pat.match(w):
                return lang, "dictionary"

    # Ступень 2: enchant (текущий язык первым, потом остальные)
    def enchant_ok(lang):
        w = word[lang]
        if len(w) <= 1:
            return False
        return enchant_cache.get(lang, {}).get(w, False)

    if enchant_ok(cur_lang):
        return cur_lang, "enchant"
    for lang in LANGS:
        if lang == cur_lang:
            continue
        if enchant_ok(lang):
            return lang, "enchant"

    # Ступень 3 (Левенштейн) -- ОПУЩЕНА, см. ограничения в отчете.

    # Ступень 4: биграммы (min_len=2)
    result = get_proto_lang(word, cur_lang, 2, PROTO2, min_len=2)
    if result is not None:
        return result, "proto2"

    # Ступень 5: триграммы (min_len=3)
    result = get_proto_lang(word, cur_lang, 3, PROTO3, min_len=3)
    if result is not None:
        return result, "proto3"

    return None, "no_language"


def decide(raw_input, cur_lang, enchant_cache):
    lang, stage = run_check_lang(raw_input, cur_lang, enchant_cache)
    if lang is None or lang == cur_lang:
        return "KEEP", raw_input, stage, cur_lang
    else:
        converted = convert(raw_input, cur_lang, lang)
        return "CONVERT", converted, stage, lang


def main():
    if len(sys.argv) < 2:
        print("usage: bench_xneur.py <golden.tsv> [predictions_out.tsv]", file=sys.stderr)
        sys.exit(2)
    corpus_path = Path(sys.argv[1])
    out_path = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("predictions-xneur.tsv")

    rows = []
    with open(corpus_path, encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter="\t")
        for row in reader:
            rows.append(row)

    # Собираем все нужные enchant-слова заранее, одним проходом на язык.
    needed = {"us": set(), "ru": set()}
    precomputed_cur = {}
    for row in rows:
        raw = row["input"]
        cur_lang = guess_typed_layout(raw)
        precomputed_cur[row["id"]] = cur_lang
        wbl = build_word_by_lang(raw, cur_lang)
        for lang in LANGS:
            if len(wbl[lang]) > 1:
                needed[lang].add(wbl[lang])

    enchant_cache = prime_enchant_cache(needed)

    out_rows = []
    for row in rows:
        rid = row["id"]
        raw = row["input"]
        cur_lang = precomputed_cur[rid]
        decision, output_text, stage, final_lang = decide(raw, cur_lang, enchant_cache)
        out_rows.append({
            "id": rid,
            "context": row.get("context", ""),
            "input": raw,
            "expect": row.get("expect", ""),
            "correct": row.get("correct", ""),
            "category": row.get("category", ""),
            "guessed_cur_lang": cur_lang,
            "predicted_decision": decision,
            "predicted_output": output_text,
            "decision_stage": stage,
        })

    fieldnames = ["id", "context", "input", "expect", "correct", "category",
                  "guessed_cur_lang", "predicted_decision", "predicted_output", "decision_stage"]
    with open(out_path, "w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames, delimiter="\t")
        writer.writeheader()
        writer.writerows(out_rows)

    # --- метрики ---
    total = len(out_rows)
    correct = sum(1 for r in out_rows if r["predicted_decision"] == r["expect"])
    false_positive = sum(1 for r in out_rows if r["expect"] == "KEEP" and r["predicted_decision"] == "CONVERT")
    total_keep = sum(1 for r in out_rows if r["expect"] == "KEEP")
    miss = sum(1 for r in out_rows if r["expect"] == "CONVERT" and r["predicted_decision"] == "KEEP")
    total_convert = sum(1 for r in out_rows if r["expect"] == "CONVERT")

    print(f"TOTAL={total} ACCURACY={correct}/{total} = {correct/total:.3f}" if total else "TOTAL=0")
    print(f"FALSE_POSITIVES (expect KEEP -> got CONVERT) = {false_positive}/{total_keep}"
          + (f" = {false_positive/total_keep:.3f}" if total_keep else ""))
    print(f"MISSES (expect CONVERT -> got KEEP) = {miss}/{total_convert}"
          + (f" = {miss/total_convert:.3f}" if total_convert else ""))

    print("\nBy category:")
    cats = sorted(set(r["category"] for r in out_rows))
    for cat in cats:
        crows = [r for r in out_rows if r["category"] == cat]
        c_correct = sum(1 for r in crows if r["predicted_decision"] == r["expect"])
        print(f"  {cat:15s} n={len(crows):4d}  acc={c_correct}/{len(crows)} = {c_correct/len(crows):.3f}")


if __name__ == "__main__":
    main()
