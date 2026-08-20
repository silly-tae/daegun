import gzip, json, bisect, collections, re, sys

BASE = 0x100000000

def demangle(s):
    m = re.match(r'^_?_ZN(.*)E$', s)
    if not m:
        return s
    body, out, i = m.group(1), [], 0
    while i < len(body):
        j = i
        while j < len(body) and body[j].isdigit():
            j += 1
        if j == i:
            break
        n = int(body[i:j]); seg = body[j:j + n]; i = j + n
        if not re.match(r'^h[0-9a-f]{16}$', seg):
            out.append(seg)
    return "::".join(out) if out else s

prof, syms = sys.argv[1], sys.argv[2]
addrs, names = [], []
for line in open(syms):
    p = line.split(None, 2)
    if len(p) < 3:
        continue
    try:
        addrs.append(int(p[0], 16))
    except ValueError:
        continue
    names.append(p[2].strip())

with gzip.open(prof) as f:
    p = json.load(f)

tot, n = collections.Counter(), 0
for t in p["threads"]:
    sa, ft, fu, st = t["stringArray"], t["frameTable"], t["funcTable"], t["stackTable"]
    for si in (t["samples"].get("stack") or []):
        if si is None:
            continue
        s = sa[fu["name"][ft["func"][st["frame"][si]]]]
        n += 1
        if s.startswith("0x"):
            try:
                i = bisect.bisect_right(addrs, int(s, 16) + BASE) - 1
            except ValueError:
                continue
            tot[names[i] if 0 <= i < len(names) else "<unknown>"] += 1
        else:
            tot[s] += 1

print(f"samples: {n}\n{'self%':>7}  symbol")
for name, c in tot.most_common(400):
    pct = 100 * c / n
    if pct < 0.5:
        break
    d = demangle(name).replace("daegun::daeshaper::", "dsh::").replace("daegun::font::", "font::")
    print(f"{pct:>6.2f}%  {d[:92]}")
