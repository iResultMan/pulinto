import sys
import os

tsv_file = "/home/medushenko/dev/PuLinTo/docs/research/corpus3/part3.tsv"

us_lower = r"`1234567890-=qwertyuiop[]\asdfghjkl;'zxcvbnm,./"
us_upper = r'~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:"ZXCVBNM<>?'
ru_lower = r"ё1234567890-=йцукенгшщзхъ\фывапролджэячсмитьбю."
ru_upper = r'Ё!"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,'

us_all = us_lower + us_upper
ru_all = ru_lower + ru_upper

us_to_ru = str.maketrans(us_all, ru_all)
ru_to_us = str.maketrans(ru_all, us_all)

ids = set()
pairs = set()
convert_count = 0
total_count = 0
category_counts = {}
trap_counts = {}

valid_categories = {'trap_abbrev', 'trap_password', 'medicine', 'trap_code', 'finance', 'law', 'trap_url'}

with open(tsv_file, "r", encoding="utf-8") as f:
    header = f.readline().strip().split("\t")
    if header != ["id", "category", "lang", "context", "input", "expect", "correct", "source"]:
        print("[ERROR] Header mismatch:", header)
        sys.exit(1)
        
    for line_num, line in enumerate(f, 2):
        parts = line.strip("\r\n").split("\t")
        if len(parts) != 8:
            print(f"[ERROR] Line {line_num} does not have 8 columns: {len(parts)}")
            sys.exit(1)
            
        row_id, cat, lang, ctx, inp, expect, correct, src = parts
        
        if not row_id or not cat or not lang or not inp or not expect or not correct or not src:
            print(f"[ERROR] Line {line_num} has empty mandatory fields")
            sys.exit(1)
            
        if expect not in ("KEEP", "CONVERT"):
            print(f"[ERROR] Line {line_num} invalid expect: {expect}")
            sys.exit(1)
            
        if cat not in valid_categories:
            print(f"[ERROR] Line {line_num} invalid category: {cat}")
            sys.exit(1)
            
        if row_id in ids:
            print(f"[ERROR] Duplicate id: {row_id}")
            sys.exit(1)
        ids.add(row_id)
        
        pair = (ctx, inp)
        if pair in pairs:
            print(f"[ERROR] Duplicate context+input pair at line {line_num}")
            sys.exit(1)
        pairs.add(pair)
        
        if expect == "KEEP":
            if correct != inp:
                print(f"[ERROR] Line {line_num} KEEP but correct!=input")
                sys.exit(1)
        elif expect == "CONVERT":
            convert_count += 1
            if lang == "ru":
                reconstructed = inp.translate(us_to_ru)
            else:
                reconstructed = inp.translate(ru_to_us)
            if reconstructed != correct:
                print(f"[ERROR] Line {line_num} CONVERT mapping mismatch. Got {reconstructed}, expected {correct}")
                sys.exit(1)
                
        total_count += 1
        if cat.startswith("trap_"):
            trap_counts[cat] = trap_counts.get(cat, 0) + 1
        else:
            category_counts[cat] = category_counts.get(cat, 0) + 1

convert_share = convert_count / float(total_count)
print(f"[OK] Validator part3 passed! Total rows: {total_count}, CONVERT share: {convert_share:.2%}")
