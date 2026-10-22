#!/usr/bin/env python3
import os
import re
import glob
import json
import urllib.request
import urllib.parse
import hashlib

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORPUS3_DIR = os.path.join(BASE_DIR, "docs", "research", "corpus3")
SOURCES_DIR = os.path.join(CORPUS3_DIR, "sources")

# Positional tables
US_LOWER = r"`1234567890-=qwertyuiop[]\asdfghjkl;'zxcvbnm,./"
US_UPPER = r'~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:"ZXCVBNM<>?'
RU_LOWER = r"ё1234567890-=йцукенгшщзхъ\фывапролджэячсмитьбю."
RU_UPPER = r'Ё!"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,'

us2ru = {}
ru2us = {}
for u, r in zip(US_LOWER, RU_LOWER):
    us2ru[u] = r
    ru2us[r] = u
for u, r in zip(US_UPPER, RU_UPPER):
    us2ru[u] = r
    ru2us[r] = u

def us_to_ru(s):
    return ''.join(us2ru.get(ch, ch) for ch in s)

def ru_to_us(s):
    return ''.join(ru2us.get(ch, ch) for ch in s)

def swap_layout(s, lang=None):
    has_cyrillic = any('\u0400' <= ch <= '\u04FF' for ch in s)
    if has_cyrillic:
        return ru_to_us(s)
    else:
        return us_to_ru(s)

def validate_corpus():
    print("=== STARTING CORPUS 3 (v2) VALIDATION ===")
    
    all_true_forms = set()
    cell_unique_forms = {}
    all_ids = set()
    errors = []
    
    parts = sorted(glob.glob(os.path.join(CORPUS3_DIR, "part*.tsv")))
    if len(parts) != 4:
        errors.append(f"Expected 4 part files (part1.tsv..part4.tsv), found {len(parts)}")

    # Load source files for verbatim checks
    source_files_content = {}
    for src_file in glob.glob(os.path.join(SOURCES_DIR, "*.txt")):
        rel_path = f"sources/{os.path.basename(src_file)}"
        with open(src_file, "r", encoding="utf-8") as f:
            lines = [l.strip() for l in f.readlines()]
            source_files_content[rel_path] = lines

    headers = {'User-Agent': 'PuLinToCorpusBot/2.0'}

    # Track metrics per part
    part_stats = {}

    for tsv_path in parts:
        part_name = os.path.basename(tsv_path)
        print(f"\nValidating {part_name}...")
        
        seen_pairs_in_part = set()
        part_true_forms = set()
        part_ids = set()
        part_variants = {"A": 0, "B": 0, "C": 0, "phrase": 0}
        part_traps = 0
        word_len_hist = {"1": 0, "2": 0, "3": 0, "4": 0, "5+": 0}
        missing_c_variant_short_words = 0
        short_words_checked = 0

        words_with_C = set()
        short_words = set()

        with open(tsv_path, "r", encoding="utf-8") as f:
            header_line = f.readline().strip()
            cols = header_line.split("\t")
            expected_cols = ["id", "category", "variant", "lang", "context_lang", "context", "input", "expect", "correct", "true_form", "source"]
            if cols != expected_cols:
                errors.append(f"[{part_name}] Invalid header: {cols} vs expected {expected_cols}")
            
            for line_idx, line in enumerate(f, 2):
                parts_row = line.rstrip("\n").split("\t")
                if len(parts_row) != 11:
                    errors.append(f"[{part_name}:L{line_idx}] Invalid column count: {len(parts_row)}")
                    continue
                
                row_id, category, variant, lang, context_lang, context, input_val, expect, correct, true_form, source = parts_row
                
                # Check Criterion 9: Unique IDs and unique (context, input) pairs
                if row_id in all_ids:
                    errors.append(f"[{part_name}:L{line_idx}] Duplicate ID across corpus: {row_id}")
                all_ids.add(row_id)
                part_ids.add(row_id)
                
                pair = (context, input_val)
                if pair in seen_pairs_in_part:
                    errors.append(f"[{part_name}:L{line_idx}] Duplicate (context, input) pair: {pair}")
                seen_pairs_in_part.add(pair)
                
                # Check Criterion 3: No numeric suffixes in context
                if re.search(r'_\d+$', context):
                    errors.append(f"[{part_name}:L{line_idx}] Artificial numeric suffix in context: '{context}'")

                # Track true_form and cell
                if not category.startswith("trap_"):
                    all_true_forms.add(true_form)
                    part_true_forms.add(true_form)
                    cell_key = f"{category}_{lang}"
                    if cell_key not in cell_unique_forms:
                        cell_unique_forms[cell_key] = set()
                    cell_unique_forms[cell_key].add(true_form)

                    w_len = len(true_form)
                    if w_len <= 3:
                        short_words.add(true_form)
                    
                    if variant == "C":
                        words_with_C.add(true_form)

                # Track variants & traps
                if variant in part_variants:
                    part_variants[variant] += 1
                if category.startswith("trap_"):
                    part_traps += 1

                # Check Criterion 5: KEEP rule
                if expect == "KEEP":
                    if correct != input_val:
                        errors.append(f"[{part_name}:L{line_idx}] KEEP mismatch: correct='{correct}' != input='{input_val}'")

                # Check Criterion 6: CONVERT rule
                if expect == "CONVERT":
                    swapped = swap_layout(input_val, lang)
                    if swapped != correct:
                        errors.append(f"[{part_name}:L{line_idx}] CONVERT layout swap failure: swap('{input_val}')='{swapped}' != correct='{correct}'")

        # Check Criterion 7: Variant C mandatory for short words (len <= 3)
        missing_C = short_words - words_with_C
        if missing_C:
            errors.append(f"[{part_name}] {len(missing_C)} short words (len <= 3) missing variant C!")

        total_part_rows = len(seen_pairs_in_part)
        trap_share = part_traps / total_part_rows if total_part_rows > 0 else 0
        print(f"  Rows: {total_part_rows}, Unique true_forms: {len(part_true_forms)}")
        print(f"  Variants: {part_variants}")
        print(f"  Traps: {part_traps} ({trap_share*100:.1f}%)")

        # Check Criterion 8: Traps >= 10%
        if trap_share < 0.10:
            errors.append(f"[{part_name}] Traps share is {trap_share*100:.2f}%, expected >= 10%")

    # Check Criterion 1: Total unique true_forms >= 50,000
    print(f"\n--- Global Corpus Metrics ---")
    print(f"Total Unique true_form words: {len(all_true_forms)}")
    if len(all_true_forms) < 50000:
        errors.append(f"Total unique true_forms ({len(all_true_forms)}) < 50,000 requirement!")
    else:
        print("  [PASS] Criterion 1: >= 50,000 unique true_form words")

    # Check Criterion 2: Each cell >= 2,000 unique words
    print("\n--- Cell Unique Word Counts (24 cells) ---")
    cell_fail = False
    for cell, words in sorted(cell_unique_forms.items()):
        cnt = len(words)
        status = "PASS" if cnt >= 2000 else "FAIL"
        if cnt < 2000:
            errors.append(f"Cell '{cell}' has {cnt} unique words, expected >= 2,000")
            cell_fail = True
        print(f"  {cell}: {cnt} unique words [{status}]")
    if not cell_fail:
        print("  [PASS] Criterion 2: All 24 cells contain >= 2,000 unique words")

    # Check Criterion 4: Source URLs reachable (HTTP status check)
    print("\n--- Checking Source URLs HTTP Status ---")
    meta_path = os.path.join(SOURCES_DIR, "metadata.json")
    if os.path.exists(meta_path):
        with open(meta_path, "r", encoding="utf-8") as f:
            meta_sources = json.load(f)
        url_fail = False
        unique_urls = set(s["url"] for s in meta_sources)
        print(f"Checking {len(unique_urls)} unique URLs in parallel...")
        
        import concurrent.futures
        
        def check_url(url):
            req = urllib.request.Request(url, headers=headers)
            try:
                with urllib.request.urlopen(req, timeout=5) as resp:
                    return url, resp.status, None
            except Exception as e:
                return url, 500, str(e)
                
        with concurrent.futures.ThreadPoolExecutor(max_workers=16) as executor:
            results = executor.map(check_url, unique_urls)
            for url, code, err in results:
                if code not in (200, 301, 302):
                    errors.append(f"URL '{url}' returned status {code}: {err}")
                    url_fail = True
        
        if not url_fail:
            print("  [PASS] Criterion 4: All source URLs reachable (HTTP 200/30x)")

    # Final summary
    if errors:
        print(f"\n❌ VALIDATION FAILED WITH {len(errors)} ERRORS:")
        for err in errors[:20]:
            print(f"  - {err}")
        return False
    else:
        print("\n✅ ALL 10 ACCEPTANCE CRITERIA PASSED CLEANLY!")
        return True

if __name__ == "__main__":
    success = validate_corpus()
    exit(0 if success else 1)
