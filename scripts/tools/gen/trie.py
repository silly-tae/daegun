SEARCH_BITS = range(3, 10)
MAX_CP = 0x110000

def _dedup(values, size):
    seen, indices, flat = {}, [], []
    for b in range(0, len(values), size):
        key = tuple(values[b:b + size])
        i = seen.get(key)
        if i is None:
            i = len(seen)
            seen[key] = i
            flat.extend(key)
        indices.append(i)
    return indices, flat

def _bytes(top, mid, leaf):
    return sum(len(a) * (1 if max(a, default=0) < 256 else 2) for a in (top, mid, leaf))

def build(columns, defaults):
    cols = []
    for ranges, default in zip(columns, defaults):
        col = [default] * MAX_CP
        for entry in ranges:
            lo, hi, value = entry[0], entry[1], entry[2]
            for cp in range(lo, min(hi + 1, MAX_CP)):
                col[cp] = value
        cols.append(col)

    records, ids = {}, [0] * MAX_CP
    for cp in range(MAX_CP):
        rec = tuple(c[cp] for c in cols)
        i = records.get(rec)
        if i is None:
            i = len(records)
            records[rec] = i
        ids[cp] = i
    assert len(records) <= 0xFFFF, "records overflowed a u16 index: {}".format(len(records))

    best = None
    for leaf_bits in SEARCH_BITS:
        mid_ids, leaf = _dedup(ids, 1 << leaf_bits)
        for mid_bits in SEARCH_BITS:
            if leaf_bits + mid_bits > 16:
                continue
            top, mid = _dedup(mid_ids, 1 << mid_bits)
            if max(len(leaf) >> leaf_bits, len(mid) >> mid_bits) > 0xFFFF:
                continue
            size = _bytes(top, mid, leaf)
            if best is None or size < best[0]:
                best = (size, leaf_bits, mid_bits, top, mid, leaf)
    assert best is not None, "no block shape fit a u16 index"
    _, leaf_bits, mid_bits, out_top, out_mid, out_leaf = best
    out_records = sorted(records, key=records.get)

    leaf_mask, mid_mask = (1 << leaf_bits) - 1, (1 << mid_bits) - 1
    for cp in range(MAX_CP):
        mid = out_top[cp >> (leaf_bits + mid_bits)]
        leaf = out_mid[(mid << mid_bits) | ((cp >> leaf_bits) & mid_mask)]
        got = out_records[out_leaf[(leaf << leaf_bits) | (cp & leaf_mask)]]
        want = tuple(c[cp] for c in cols)
        assert got == want, "trie disagrees at U+{:04X}: {} vs {}".format(cp, got, want)

    return out_records, out_top, out_mid, out_leaf, leaf_bits, mid_bits

def width_for(values):
    return "u8" if max(values, default=0) < 256 else "u16"

def emit_flat(f, name, values, per_line=16):
    ty = width_for(values)
    f.write("pub(crate) static {}: &[{}; {}] = &[\n".format(name, ty, len(values)))
    for i in range(0, len(values), per_line):
        f.write("    " + " ".join("{},".format(v) for v in values[i:i + per_line]) + "\n")
    f.write("];\n\n")

def emit(f, prefix, fields, columns, defaults, doc):
    records, top, mid, leaf, leaf_bits, mid_bits = build(columns, defaults)

    f.write(doc)
    f.write("#[derive(Clone, Copy)]\npub(crate) struct {}Record {{\n".format(prefix.title().replace("_", "")))
    for name, ty in fields:
        f.write("    pub(crate) {}: {},\n".format(name, ty))
    f.write("}\n\n")

    struct = prefix.title().replace("_", "") + "Record"
    f.write("// The distinct combinations these scripts actually use. Indexed by `{}_LEAF`.\n".format(prefix))
    f.write("pub(crate) static {}: &[{}; {}] = &[\n".format(prefix, struct, len(records)))
    for rec in records:
        f.write("    {} {{ {} }},\n".format(
            struct, ", ".join("{}: {}".format(n, v) for (n, _), v in zip(fields, rec))))
    f.write("];\n\n")

    f.write("// Block shape. The descent reads these, so a regeneration landing on a different\n"
            "// shape moves the lookup with it.\n")
    f.write("pub(crate) const {}_LEAF_BITS: u32 = {};\n".format(prefix, leaf_bits))
    f.write("pub(crate) const {}_MID_BITS: u32 = {};\n\n".format(prefix, mid_bits))

    f.write("// Stage 1: `cp >> {}` to a `{}_MID` block.\n".format(leaf_bits + mid_bits, prefix))
    emit_flat(f, prefix + "_TOP", top)
    f.write("// Stage 2: a mid block plus `(cp >> {}) & {}` to a `{}_LEAF` block.\n"
            .format(leaf_bits, (1 << mid_bits) - 1, prefix))
    emit_flat(f, prefix + "_MID", mid)
    f.write("// Stage 3: a leaf block plus `cp & {}` to a `{}` record.\n".format((1 << leaf_bits) - 1, prefix))
    emit_flat(f, prefix + "_LEAF", leaf)

    return len(records), len(mid) >> mid_bits, len(leaf) >> leaf_bits
