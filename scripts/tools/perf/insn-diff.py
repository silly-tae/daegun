USAGE = """    insn-diff.py <binary>                    per-function counts, largest first
    insn-diff.py <baseline> <candidate>      only what changed

Only daegun's own symbols are shown; pass --all to include std and dependencies."""

import re
import subprocess
import sys

def demangle(sym):
    m = re.match(r"^_?_ZN(.*)E$", sym)
    if not m:
        return sym
    body, out, i = m.group(1), [], 0
    while i < len(body):
        j = i
        while j < len(body) and body[j].isdigit():
            j += 1
        if j == i:
            break
        n = int(body[i:j])
        seg = body[j:j + n]
        i = j + n
        if not re.match(r"^h[0-9a-f]{16}$", seg):
            out.append(seg)
    return "::".join(out) if out else sym

def counts(path):
    out = subprocess.run(["nm", "-n", path], capture_output=True, text=True).stdout
    syms = []
    for line in out.splitlines():
        p = line.split(None, 2)
        if len(p) < 3 or p[1] not in ("t", "T"):
            continue
        try:
            syms.append((int(p[0], 16), p[2].strip()))
        except ValueError:
            pass
    res = {}
    for (a, name), (b, _) in zip(syms, syms[1:]):
        if b > a:
            key = demangle(name)
            res[key] = res.get(key, 0) + (b - a) // 4
    return res

def short(name):
    return name.replace("daegun::daeshaper::", "dsh::").replace("daegun::font::", "font::")

def ours(name):
    return "daegun" in name

args = [a for a in sys.argv[1:] if a != "--all"]
keep = (lambda _: True) if "--all" in sys.argv else ours

if len(args) == 1:
    rows = [(c, n) for n, c in counts(args[0]).items() if keep(n)]
    for c, name in sorted(rows, reverse=True)[:40]:
        print(f"{c:>7}  {short(name)[:88]}")
    sys.exit()

if len(args) != 2:
    print(USAGE)
    sys.exit(1)

a, b = counts(args[0]), counts(args[1])
a = {k: v for k, v in a.items() if keep(k)}
b = {k: v for k, v in b.items() if keep(k)}
rows = []
for name in set(a) | set(b):
    x, y = a.get(name), b.get(name)
    if x != y:
        rows.append((abs((y or 0) - (x or 0)), name, x, y))
rows.sort(reverse=True)

print(f"{'delta':>7} {'before':>8} {'after':>8}  symbol")
net = 0
for _, name, x, y in rows[:30]:
    d = (y or 0) - (x or 0)
    net += d
    print(f"{d:>+7} {('-' if x is None else x):>8} {('-' if y is None else y):>8}  {short(name)[:76]}")
print(f"\n{len(rows)} functions changed; net {sum((y or 0) - (x or 0) for _, _, x, y in rows):+} instructions")
