#!/usr/bin/env python3
USAGE = """    abi-parity.py                    check every daegun library built under target/
    abi-parity.py <lib> [<lib>...]   check the libraries named instead

What daegun.h promises against what the library actually exports, in both directions: a declaration
with no symbol behind it is a link error waiting for the first caller, and an exported symbol with
no declaration is unreachable from C. Exit status is 1 on any difference.

Names only – a library carries no C types. Signature agreement is what compiling roundtrip.c
against the header proves; this cannot.

c-parity.sh asks a different question – whether the Rust API is reachable from C at all – and
answers it from names in the source. This one answers from the linker's own table.

Build what you want checked first:
    cargo rustc --features capi --crate-type staticlib
    cargo rustc --target aarch64-pc-windows-msvc --features capi --crate-type staticlib"""


import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
HEADER = os.path.join(ROOT, "src", "c-wrapper", "daegun.h")

DECL = re.compile(r"^(?:const\s+)?[A-Za-z_][A-Za-z0-9_ *]*\b(daegun_[a-z_0-9]+)\s*\(")
NAME = re.compile(r"\b(daegun_[a-z_0-9]+)\s*\(")


def declared(lines, platform):
    # The header gates Metal behind __APPLE__ and Direct3D behind _WIN32, so what it declares
    # depends on who is compiling it. Every other #if is a guard that is always on.
    found, live, backends, macro = {}, [], [], ""
    i = 0
    while i < len(lines):
        line = lines[i]
        gate = re.match(r"\s*#\s*(if|ifdef|ifndef|else|elif|endif)\b(.*)", line)
        if gate:
            kind, rest = gate.group(1), gate.group(2)
            if kind in ("if", "ifdef", "ifndef"):
                if "__APPLE__" in rest:
                    live.append(platform == "apple")
                elif "_WIN32" in rest:
                    live.append(platform == "win32")
                else:
                    live.append(True)
            elif kind == "else" and live:
                live[-1] = not live[-1]
            elif kind == "endif" and live:
                live.pop()
            i += 1
            continue

        if line.startswith("#define DAEGUN_DECLARE_BACKEND"):
            body = []
            while i < len(lines):
                body.append(lines[i])
                if not lines[i].rstrip().endswith("\\"):
                    break
                i += 1
            macro = "\n".join(body)
            i += 1
            continue

        invoked = re.match(r"\s*DAEGUN_DECLARE_BACKEND\(([a-z0-9]+)\);", line)
        if invoked:
            if all(live):
                backends.append((invoked.group(1), i + 1))
            i += 1
            continue

        if all(live) and not line.lstrip().startswith(("#", "*", "/")) and DECL.match(line):
            # A declaration may wrap over several lines; join until the semicolon closes it.
            joined, j = line, i
            while ";" not in joined and j + 1 < len(lines):
                j += 1
                joined += " " + lines[j].strip()
            hit = NAME.search(joined)
            if hit:
                found[hit.group(1)] = i + 1
        i += 1

    for backend, line in backends:
        text = macro.replace("##b##", backend).replace("\\\n", " ")
        text = re.sub(r"/\*[\s\S]*?\*/", " ", text)
        for decl in text.split(";"):
            hit = NAME.search(decl)
            if hit:
                found.setdefault(hit.group(1), line)
    return found


def llvm_nm():
    try:
        sysroot = subprocess.run(["rustc", "--print", "sysroot"], capture_output=True, text=True,
                                 check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None
    rustlib = os.path.join(sysroot, "lib", "rustlib")
    if not os.path.isdir(rustlib):
        return None
    for host in sorted(os.listdir(rustlib)):
        found = os.path.join(rustlib, host, "bin", "llvm-nm")
        if os.path.isfile(found):
            return found
    return None


def exported(path):
    # llvm-nm reads Mach-O, ELF and COFF alike, so a Windows .lib cross-built on a Mac is still
    # readable. The system nm is the fallback for everything but COFF.
    tool = llvm_nm()
    if tool:
        argv = [tool, "--defined-only", path]
    elif path.endswith(".lib"):
        return None
    elif sys.platform == "darwin":
        argv = ["nm", "-gU", path]
    else:
        argv = ["nm", "-D", "--defined-only", path]

    out = subprocess.run(argv, capture_output=True, text=True).stdout
    names = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 2 or parts[-2] in ("U", "u", "w"):
            continue
        hit = re.fullmatch(r"_?(daegun_[a-z_0-9]+)", parts[-1])
        if hit:
            names.add(hit.group(1))
    return names


def shown(path):
    rel = os.path.relpath(path, ROOT)
    return path if rel.startswith("..") else rel


def platform_of(path):
    for part in path.split(os.sep):
        if "-windows-" in part:
            return "win32"
        if "-apple-" in part:
            return "apple"
        if "-linux-" in part:
            return "linux"
    if path.endswith(".lib"):
        return "win32"
    return {"darwin": "apple", "win32": "win32"}.get(sys.platform, "linux")


def libraries():
    target = os.path.join(ROOT, "target")
    names = ("libdaegun.a", "libdaegun.dylib", "libdaegun.so", "daegun.lib")
    found = []
    for profile_dir, _, files in os.walk(target):
        if os.path.basename(profile_dir) not in ("debug", "release"):
            continue
        for name in names:
            if name in files:
                found.append(os.path.join(profile_dir, name))
    # Newest first, so a stale debug build never gets checked in place of the release one beside it.
    return sorted(found, key=os.path.getmtime, reverse=True)


def main(argv):
    if "-h" in argv or "--help" in argv:
        print(USAGE)
        return 0

    libs = argv or libraries()
    if not libs:
        print("nothing built (run: cargo rustc --features capi --crate-type staticlib)")
        return 0

    lines = open(HEADER, encoding="utf-8").read().split("\n")
    differences, checked, missing, seen = 0, 0, 0, set()

    for lib in libs:
        platform = platform_of(lib)
        if platform in seen:
            continue
        if not os.path.isfile(lib):
            print(f"{platform:7} {lib}\n        skipped: no such file\n")
            missing += 1
            continue
        symbols = exported(lib)
        if symbols is None:
            print(f"{platform:7} {shown(lib)}\n"
                  f"        skipped: no llvm-nm to read a COFF archive with\n")
            continue
        if not symbols:
            print(f"{platform:7} {shown(lib)}\n"
                  f"        skipped: no daegun symbols, so it was built without --features capi\n")
            continue

        seen.add(platform)
        checked += 1
        header = declared(lines, platform)
        unbuilt = sorted(n for n in header if n not in symbols)
        undeclared = sorted(n for n in symbols if n not in header)

        print(f"{platform:7} {shown(lib)}")
        print(f"        header declares {len(header)}, library exports {len(symbols)}", end="")
        if not unbuilt and not undeclared:
            print(" – matched")
        else:
            print()
            differences += len(unbuilt) + len(undeclared)
        for name in unbuilt:
            print(f"        DECLARED, NEVER EXPORTED  {name}  (daegun.h:{header[name]})")
        for name in undeclared:
            print(f"        EXPORTED, NEVER DECLARED  {name}")
        print()

    if not checked:
        print("nothing checked")
        return 1 if missing else 0
    print(f"{checked} librar{'y' if checked == 1 else 'ies'} checked, "
          f"{differences or 'no'} difference{'' if differences == 1 else 's'}.")
    return 1 if differences or missing else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
