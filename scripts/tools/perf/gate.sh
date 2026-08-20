#!/bin/sh
set -e
cd "$(dirname "$0")/../../.."

printf 'release build .... '
cargo build --release >/dev/null 2>&1 || { echo "FAILED"; cargo build --release 2>&1 | grep -E "^error" -A6 | head -30; exit 1; }
echo ok

printf 'debug build ...... '
cargo build >/dev/null 2>&1 || { echo "FAILED"; cargo build 2>&1 | grep -E "^error" -A6 | head -30; exit 1; }
echo ok

printf 'tests ............ '
OUT=$(cargo test --tests 2>&1) || { echo "FAILED"; echo "$OUT" | grep -E "^(error|failures:)" -A8 | head -40; exit 1; }
N=$(echo "$OUT" | awk '/^test result: ok\./ { s += $4 } END { print s+0 }')
[ "$N" -eq 0 ] && echo "0 (the suite is empty)" || echo "$N passed"

printf 'clippy ........... '
[ "$(cargo clippy --all-targets 2>&1 | grep -cE '^(warning|error)')" -eq 0 ] && echo clean || { echo "issues"; exit 1; }

printf 'rustdoc links .... '
doc_check() {
  if [ "$(cargo doc -p "$1" --no-deps ${2:+--target "$2"} 2>&1 | grep -cE '^(warning|error)')" -ne 0 ]; then
    echo "issues in $1${2:+ ($2)}"
    cargo doc -p "$1" --no-deps ${2:+--target "$2"} 2>&1 | grep -E '^(warning|error)' | head -5
    exit 1
  fi
}
doc_check daegun
for tgt in x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do doc_check daegun "$tgt"; done
echo clean

printf 'shaders .......... '
if OUT=$(sh scripts/tools/shaders.sh 2>&1); then
  echo "$(echo "$OUT" | grep -c ' ok$') stages compile"
elif [ $? -eq 127 ]; then
  echo "SKIPPED (glslangValidator not installed)"
else
  echo "FAILED"; echo "$OUT" | grep -v ' ok$' | head -20; exit 1
fi

printf 'fuzz ............. '
if OUT=$(sh scripts/tools/fuzz/run.sh 2>&1); then
  echo "$OUT"
else
  echo "FAILED"; echo "$OUT" | head -20; exit 1
fi

printf 'feature threading '
[ "$(cargo build --features threading 2>&1 | grep -cE '^(warning|error)')" -eq 0 ] && echo clean || { echo "issues"; exit 1; }

printf 'threading tests .. '
OUT=$(cargo test --features threading --tests 2>&1) || {
  echo "FAILED"; echo "$OUT" | grep -E "^(error|failures:)" -A8 | head -40; exit 1; }
echo "$(echo "$OUT" | awk '/^test result: ok\./ { s += $4 } END { print s+0 }') passed"

printf 'no-default ....... '
[ "$(cargo build --no-default-features 2>&1 | grep -cE '^(warning|error)')" -eq 0 ] && echo clean || { echo "issues"; exit 1; }

printf 'capi + no-default '
[ "$(cargo clippy --all-targets --no-default-features --features capi 2>&1 | grep -cE '^(warning|error)')" -eq 0 ] \
  && echo clean || { echo "issues"; cargo clippy --all-targets --no-default-features --features capi 2>&1 | grep -E '^(warning|error)' -A6 | head -20; exit 1; }

for t in aarch64-pc-windows-msvc x86_64-pc-windows-msvc; do
  printf 'windows %-9s ' "$(echo "$t" | cut -d- -f1)"
  if ! rustc --print target-libdir --target "$t" >/dev/null 2>&1; then
    echo "skipped (target not installed)"
  elif [ "$(cargo clippy --all-targets --features capi --target "$t" 2>&1 | grep -cE '^(warning|error)')" -eq 0 ]; then
    echo clean
  else
    echo "issues"
    cargo clippy --all-targets --features capi --target "$t" 2>&1 | grep -E '^(warning|error)' -A6 | head -30
    exit 1
  fi
done

printf 'c abi ............ '
if ! command -v cc >/dev/null 2>&1; then
  echo "skipped (no C compiler)"
elif ! cargo rustc --features capi --crate-type staticlib >/dev/null 2>&1; then
  echo "the C ABI does not build"; cargo rustc --features capi --crate-type staticlib 2>&1 | tail -20; exit 1
else
  CW=src/c-wrapper
  if ! cargo test --features capi --lib --quiet >/dev/null 2>&1; then
    echo "the C ABI's own tests failed"; cargo test --features capi --lib 2>&1 | tail -20; exit 1
  fi
  CT=$(mktemp -d)
  case "$(uname -s)" in
    Darwin) LINK="-framework Metal -framework Foundation -framework QuartzCore" ;;
    *)      LINK="-lm -lpthread -ldl" ;;
  esac
  if ! cc -std=c11 -Wall -Wextra -Werror -I "$CW" "$CW/tests/roundtrip.c" \
       target/debug/libdaegun.a $LINK -o "$CT/rt" 2>"$CT/cc.log"; then
    echo "header or test does not compile"; cat "$CT/cc.log"; rm -rf "$CT"; exit 1
  fi
  if ! OUT=$("$CT/rt" assets/test-fonts/inter/InterVariable.ttf 2>&1); then
    echo "round trip failed"; echo "$OUT"; rm -rf "$CT"; exit 1
  fi
  if cc -std=c11 -g -fsanitize=address,undefined -fno-omit-frame-pointer -I "$CW" \
       "$CW/tests/roundtrip.c" target/debug/libdaegun.a $LINK -o "$CT/rt-san" 2>/dev/null; then
    if ! OUT=$("$CT/rt-san" assets/test-fonts/inter/InterVariable.ttf 2>&1); then
      echo "sanitiser found something"; echo "$OUT" | tail -25; rm -rf "$CT"; exit 1
    fi
    echo "round trip ok, clean under asan+ubsan"
  else
    echo "round trip ok (no sanitiser available)"
  fi
  rm -rf "$CT"
fi

if rustup run nightly rustc --version >/dev/null 2>&1; then
  printf 'gpu handle asan .. '
  if RUSTFLAGS="-Zsanitizer=address" cargo +nightly test --features capi --lib \
       --target "$(rustc -vV | awk '/^host:/ {print $2}')" >/dev/null 2>&1; then
    echo "clean"
  else
    echo "a GPU handle outlived its renderer unsoundly"; exit 1
  fi
fi

printf 'c parity ......... '
if ! sh scripts/tools/c-parity.sh; then
  exit 1
fi

if rustup run 1.97.1 rustc --version >/dev/null 2>&1; then
  printf 'MSRV 1.97.1 ........ '
  [ "$(rustup run 1.97.1 cargo check --all-features 2>&1 | grep -cE '^error')" -eq 0 ] && echo clean || { echo "issues"; exit 1; }
else
  printf 'MSRV 1.97.1 ........ skipped (toolchain absent)\n'
fi

echo "GATE GREEN (${N} behavioural assertions – the rest is compile-only)"
