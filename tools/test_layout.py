#!/usr/bin/env python3
US_LOWER = r"`1234567890-=qwertyuiop[]\asdfghjkl;'zxcvbnm,./"
US_UPPER = r'~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:"ZXCVBNM<>?'
RU_LOWER = r"ё1234567890-=йцукенгшщзхъ\фывапролджэячсмитьбю."
RU_UPPER = r'Ё!"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,'

us2ru = {u: r for u, r in zip(US_LOWER, RU_LOWER)}
us2ru.update({u: r for u, r in zip(US_UPPER, RU_UPPER)})
ru2us = {r: u for u, r in zip(US_LOWER, RU_LOWER)}
ru2us.update({r: u for u, r in zip(RU_UPPER, RU_UPPER)})

def us_to_ru(s):
    return ''.join(us2ru.get(ch, ch) for ch in s)

def ru_to_us(s):
    return ''.join(ru2us.get(ch, ch) for ch in s)

def swap_layout(s):
    has_cyrillic = any('\u0400' <= ch <= '\u04FF' for ch in s)
    if has_cyrillic:
        return ru_to_us(s)
    else:
        return us_to_ru(s)

print("swap_layout('heccrfz'):", swap_layout('heccrfz'))
print("swap_layout('русская'):", swap_layout('русская'))
print("swap_layout('реезыЖ..кгюцшлшзувшфющкп.цшлш.дшеукфегку'):", swap_layout('реезыЖ..кгюцшлшзувшфющкп.цшлш.дшеукфегку'))
