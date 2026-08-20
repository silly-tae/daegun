#!/usr/bin/env python3

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import trie

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "..", "data", "unicode")
GRAMMAR = os.path.join(HERE, "..", "..", "data", "grammars", "use.grammar")
OUT = os.path.join(HERE, "..", "..", "..", "src", "daecore", "src", "daeshaper", "generated", "use_tables.rs")

def category_numbers():
    names = {}
    section = None
    for line in open(GRAMMAR, encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.endswith(":") and " " not in line:
            section = line[:-1]
            continue
        if section == "categories" and "=" in line:
            k, v = (p.strip() for p in line.split("=", 1))
            names[k] = int(v)
    return names

def load_ranges(name):
    out = {}
    with open(os.path.join(DATA, name), encoding="utf-8") as f:
        for line in f:
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            field, value = (p.strip() for p in line.split(";", 1))
            if ".." in field:
                lo, hi = (int(p, 16) for p in field.split(".."))
            else:
                lo = hi = int(field, 16)
            for cp in range(lo, hi + 1):
                out[cp] = value
    return out

POSITIONAL = {"V", "VM", "CM", "M", "F", "FM", "SM"}

FROM_POSITIONAL = {
    "Top": "Abv",
    "Bottom": "Blw",
    "Overstruck": "Blw",
    "Left": "Pre",
    "Right": "Pst",
    "Visual_Order_Left": "Pre",
    "Top_And_Bottom": "Abv",
    "Top_And_Bottom_And_Left": "Abv",
    "Top_And_Bottom_And_Right": "Abv",
    "Top_And_Left": "Pre",
    "Top_And_Left_And_Right": "Pre",
    "Top_And_Right": "Abv",
    "Bottom_And_Left": "Blw",
    "Bottom_And_Right": "Blw",
    "Left_And_Right": "Pre",
    "NA": "Pst",
}

JOINING = {"C", "D", "L", "R"}

INDIC_SCRIPTS = {
    "Devanagari", "Bengali", "Gurmukhi", "Gujarati", "Oriya", "Tamil", "Telugu", "Kannada",
    "Malayalam", "Sinhala", "Khmer", "Myanmar",
}

ALWAYS = {"Common", "Inherited"}

USE_SCRIPTS = {
    "Adlam", "Ahom", "Balinese", "Batak", "Bhaiksuki", "Brahmi", "Buginese", "Buhid", "Chakma",
    "Cham", "Chorasmian", "Cypro_Minoan", "Dives_Akuru", "Dogra", "Duployan", "Elymaic", "Garay",
    "Grantha", "Gunjala_Gondi", "Gurung_Khema", "Hanifi_Rohingya", "Hanunoo", "Javanese", "Kaithi",
    "Kawi", "Kayah_Li", "Kharoshthi", "Khitan_Small_Script", "Khojki", "Khudawadi", "Kirat_Rai",
    "Lepcha", "Limbu", "Mahajani", "Makasar", "Mandaic", "Manichaean", "Marchen", "Masaram_Gondi",
    "Medefaidrin", "Meetei_Mayek", "Miao", "Modi", "Mongolian", "Multani", "Nag_Mundari",
    "Nandinagari", "Newa", "Nko", "Nyiakeng_Puachue_Hmong", "Ol_Onal", "Old_Sogdian", "Old_Uyghur",
    "Pahawh_Hmong", "Phags_Pa", "Psalter_Pahlavi", "Rejang", "Saurashtra", "Sharada", "Siddham",
    "Sinhala", "Sogdian", "Soyombo", "Sundanese", "Sunuwar", "Syloti_Nagri", "Tagalog", "Tagbanwa",
    "Tai_Le", "Tai_Tham", "Tai_Viet", "Takri", "Tangsa", "New_Tai_Lue", "Tibetan", "Tifinagh", "Tirhuta", "Todhri",
    "Toto", "Tulu_Tigalari", "Vithkuqi", "Wancho", "Yezidi", "Zanabazar_Square",
    "Egyptian_Hieroglyphs",
}

CGJ_CHARS = {0x034F, 0x17B4, 0x17B5, 0x180B, 0x180C, 0x180D, 0x180F, 0x200D}

def is_cgj(cp):
    return cp in CGJ_CHARS or 0xFE00 <= cp <= 0xFE0F or 0xE0100 <= cp <= 0xE01EF

HIEROGLYPH_BLOCKS = ("Egyptian Hieroglyphs", "Egyptian Hieroglyphs Extended-A")

HIEROGLYPH_CONTROLS = [
    (0x13430, 0x13436, "J"),
    (0x13437, 0x13437, "SB"),
    (0x13438, 0x13438, "SE"),
    (0x13439, 0x1343B, "J"),
    (0x1343C, 0x1343F, "G"),
    (0x13440, 0x13440, "HR"),
    (0x13441, 0x13446, "G"),
    (0x13447, 0x13455, "HM"),
]

BASE_OTHER_CHARS = {0x2015, 0x2022, 0x25FB, 0x25FC, 0x25FD, 0x25FE}

BASE_SYLLABIC = {
    "Number", "Consonant", "Consonant_Head_Letter", "Tone_Letter", "Vowel_Independent",
}

BASE_IF_LETTER = {
    "Avagraha": None,
    "Bindu": "VM",
    "Consonant_Final": "F",
    "Consonant_Medial": "M",
    "Consonant_Subjoined": "SUB",
    "Vowel": "V",
    "Vowel_Dependent": "V",
}

DIRECT = {
    "Consonant_With_Stacker": "CS",
    "Consonant_Succeeding_Repha": "F",
    "Consonant_Preceding_Repha": "R",
    "Consonant_Prefixed": "R",
    "Virama": "H",
    "Invisible_Stacker": "IS",
    "Number_Joiner": "HN",
    "Brahmi_Joining_Number": "N",
    "Consonant_Placeholder": "GB",
    "Nukta": "CM",
    "Gemination_Mark": "CM",
    "Consonant_Killer": "CM",
    "Symbol_Modifier": "SM",
    "Syllable_Modifier": "FM",
    "Pure_Killer": "V",
    "Tone_Mark": "VM",
    "Cantillation_Mark": "VM",
    "Register_Shifter": "VM",
    "Visarga": "VM",
    "Reordering_Killer": "RK",
    "Non_Joiner": "ZWNJ",
    "Consonant_Final_Modifier": "FM",
    "Consonant_Initial_Postfixed": "M",
    "Hieroglyph": "G",
    "Hieroglyph_Joiner": "J",
    "Hieroglyph_Mark_Begin": "SB",
    "Hieroglyph_Segment_Begin": "SB",
    "Hieroglyph_Mark_End": "SE",
    "Hieroglyph_Segment_End": "SE",
    "Hieroglyph_Modifier": "HM",
    "Hieroglyph_Mirror": "HR",
}

OVERRIDES = {
    0x061C: "O",   # Arabic letter mark, a bidi control
    0x115F: "O",   # Hangul choseong filler
    0x1160: "O",   # Hangul jungseong filler
    0x3164: "O",   # Hangul filler
    0xFFA0: "O",   # halfwidth Hangul filler
    0x1BCA0: "O",  # Duployan shorthand format controls
    0x1BCA1: "O",
    0x1BCA2: "O",
    0x1BCA3: "O",

    0x0DCA: "HVM",

    0x1A60: "Sk",

    0xAAB0: "VAbv",
    0xAAB2: "VAbv",
    0xAAB3: "VAbv",
    0xAAB4: "VBlw",
    0xAAB7: "VAbv",
    0xAAB8: "VAbv",
    0xAABE: "VAbv",

    0x11302: "VMAbv",  # Grantha anusvara
    0x11303: "VMAbv",  # Grantha visarga
    0x114C1: "VMAbv",  # Tirhuta visarga

    0x1171E: "MPre",

    0x0F18: "VBlw",
    0x0F19: "VBlw",

    0x0F7F: "O",
}

def positioned(family, cp, positional, names):
    if family not in POSITIONAL:
        return names.get(family)
    suffix = FROM_POSITIONAL.get(positional.get(cp, "NA"), "Pst")
    return names.get(family + suffix)

def derive(cp, syllabic, positional, names):
    if cp in OVERRIDES:
        return names.get(OVERRIDES[cp])

    uisc = syllabic.get(cp, "Other")
    gc = GENERAL_CATEGORY.get(cp)

    if cp in HIEROGLYPHS and cp in GENERAL_CATEGORY:
        return HIEROGLYPHS[cp]

    if is_cgj(cp):
        return names["CGJ"]
    script = SCRIPT.get(cp)
    classified = script in USE_SCRIPTS or script in INDIC_SCRIPTS or script in ALWAYS

    if classified and uisc in DIRECT:
        return positioned(DIRECT[uisc], cp, positional, names)

    if cp in IGNORABLE:
        return names["WJ"]

    if not classified:
        return None

    if uisc in BASE_SYLLABIC:
        return names["B"]

    if uisc in BASE_IF_LETTER:
        if gc == "Lo":
            return names["B"]
        alternative = BASE_IF_LETTER[uisc]
        return positioned(alternative, cp, positional, names) if alternative else None

    in_use_script = SCRIPT.get(cp) in USE_SCRIPTS

    if JOINING_TYPE.get(cp) in JOINING and (in_use_script or gc == "Lm"):
        return names["B"]

    if gc in ("Mn", "Mc"):
        uipc = positional.get(cp)
        if uipc in ("Top", "Bottom", "Overstruck"):
            return names["VM" + FROM_POSITIONAL[uipc]]
        if uipc in ("Left", "Right"):
            return names["V" + FROM_POSITIONAL[uipc]]

    if in_use_script and gc in ("Lo", "Lu", "Ll", "Lt") and SCRIPT.get(cp) in CLUSTERING_SCRIPTS:
        return names["B"]

    if uisc in ("Consonant_Dead", "Modifying_Letter", "Other") or gc == "Po":
        return None

    return None

IGNORABLE = set()
HIEROGLYPHS = {}
SCRIPT = {}
GENERAL_CATEGORY = {}
JOINING_TYPE = {}

def load_joining_types():
    for line in open(os.path.join(DATA, "ArabicShaping.txt"), encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        fields = [p.strip() for p in line.split(";")]
        if len(fields) >= 3:
            JOINING_TYPE[int(fields[0], 16)] = fields[2]

def load_scripts():
    for line in open(os.path.join(DATA, "Scripts.txt"), encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line or ";" not in line:
            continue
        field, value = (p.strip() for p in line.split(";", 1))
        if ".." in field:
            lo, hi = (int(p, 16) for p in field.split(".."))
        else:
            lo = hi = int(field, 16)
        for cp in range(lo, hi + 1):
            SCRIPT[cp] = value

def load_general_categories():
    pending = None
    for line in open(os.path.join(DATA, "UnicodeData.txt"), encoding="utf-8"):
        fields = line.split(";")
        if len(fields) < 3:
            continue
        cp, name, gc = int(fields[0], 16), fields[1], fields[2]
        if name.endswith(", First>"):
            pending = (cp, gc)
            continue
        if name.endswith(", Last>") and pending:
            for c in range(pending[0], cp + 1):
                GENERAL_CATEGORY[c] = pending[1]
            pending = None
            continue
        GENERAL_CATEGORY[cp] = gc

def load_ignorable(names):
    with open(os.path.join(DATA, "DerivedCoreProperties.txt"), encoding="utf-8") as f:
        for line in f:
            line = line.split("#", 1)[0].strip()
            if not line or ";" not in line:
                continue
            field, value = (p.strip() for p in line.split(";", 1))
            if value != "Default_Ignorable_Code_Point":
                continue
            if ".." in field:
                lo, hi = (int(p, 16) for p in field.split(".."))
            else:
                lo = hi = int(field, 16)
            IGNORABLE.update(range(lo, hi + 1))

def load_hieroglyphs(names):
    with open(os.path.join(DATA, "Blocks.txt"), encoding="utf-8") as f:
        for line in f:
            line = line.split("#", 1)[0].strip()
            if not line or ";" not in line:
                continue
            field, block = (p.strip() for p in line.split(";", 1))
            if block not in HIEROGLYPH_BLOCKS:
                continue
            lo, hi = (int(p, 16) for p in field.split(".."))
            for cp in range(lo, hi + 1):
                HIEROGLYPHS[cp] = names["G"]

    for lo, hi, name in HIEROGLYPH_CONTROLS:
        for cp in range(lo, hi + 1):
            HIEROGLYPHS[cp] = names[name]

def emit(ours, names):
    by_name = {v: k for k, v in sorted(names.items())}

    ranges = []
    for cp in sorted(ours):
        if ranges and ranges[-1][1] == cp - 1 and ranges[-1][2] == ours[cp]:
            ranges[-1][1] = cp
        else:
            ranges.append([cp, cp, ours[cp]])

    with open(OUT, "w", encoding="utf-8") as f:
        f.write("// Generated by scripts/tools/gen/use-table.py – do not edit.\n")
        f.write("//\n")
        f.write("// The Universal Shaping Engine's category for every codepoint that has one.\n")
        f.write("// Anything absent is Other, which is what the grammar's catch-all production\n")
        f.write("// consumes.\n")
        f.write("//\n")
        f.write("// Derived in part from the Unicode Character Database. Copyright (c) Unicode,\n")
        f.write("// Inc.; the licence notice is reproduced in full at the top of\n")
        f.write("// src/daecore/src/daeshaper/generated/unicode_tables.rs.\n")
        f.write("\n")
        stats = trie.emit(
            f, "USE_CATEGORY", [("category", "u8")], [ranges], [0],
            "// The USE category of a codepoint. Absent is 0 (Other), which is also the trie's\n"
            "// default, so the whole of Unicode outside these scripts shares one leaf block.\n",
        )
        print("  use trie        {} records, {} mid blocks, {} leaf blocks".format(*stats))

    return len(ranges)

CLUSTERING_SCRIPTS = set()

def find_clustering_scripts(syllabic):
    for cp, value in syllabic.items():
        if value != "Other":
            CLUSTERING_SCRIPTS.add(SCRIPT.get(cp))

def main():
    names = category_numbers()

    syllabic = load_ranges("IndicSyllabicCategory.txt")
    positional = load_ranges("IndicPositionalCategory.txt")

    syllabic.update(load_ranges("IndicSyllabicCategory-Additional.txt"))
    positional.update(load_ranges("IndicPositionalCategory-Additional.txt"))
    find_clustering_scripts(syllabic)
    load_ignorable(names)
    load_hieroglyphs(names)
    load_scripts()
    load_general_categories()
    load_joining_types()

    ours = {}
    for cp in range(0x110000):
        c = derive(cp, syllabic, positional, names)
        if c:
            ours[cp] = c

    count = emit(ours, names)
    print("  {} ranges over {} codepoints".format(count, len(ours)))

if __name__ == "__main__":
    main()
