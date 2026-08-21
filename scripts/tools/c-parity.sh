#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."

FILES="src/daegun/api/*.rs src/daegun/text/*.rs"
GPU="src/daerizer/src/daegpu/ffi.rs src/daerizer/src/daegpu/vk.rs\
 src/daerizer/src/daegpu/d3d11.rs src/daerizer/src/daegpu/d3d12.rs"
REEXPORTS="src/daegun/lib.rs src/daegun/api/*.rs src/daegun/text/*.rs"

modules=" binding bytes class eval format paint shape state gpu metal vulkan d3d11 d3d12 "

rust=$(awk '
  FILENAME != last { last = FILENAME; delete private; depth = 0; skip = 0 }
  /^pub\(crate\) (struct|enum) [A-Za-z_]/ { split($0, a, " "); gsub(/[<({].*/, "", a[3]); private[a[3]] = 1 }
  /^(struct|enum) [A-Za-z_]/             { split($0, a, " "); gsub(/[<({].*/, "", a[2]); private[a[2]] = 1 }
  /^impl( <[^>]*>)? [A-Za-z_]/ {
    t = $0; sub(/^impl( <[^>]*>)? /, "", t); sub(/ .*/, "", t); gsub(/[<({].*/, "", t)
    skip = (t in private) ? 1 : 0
  }
  /^}/ { skip = 0 }
  skip == 0 && /^[[:space:]]*pub (const )?fn [a-z_0-9]+/ {
    line = $0
    sub(/.*fn /, "", line); sub(/[^a-z_0-9].*/, "", line)
    print line
  }
' $FILES | sort -u)

reexported=$(awk '
  /^[[:space:]]*pub use / { inuse = 1; buf = ""; depth = 0 }
  inuse {
    stripped = $0
    sub(/\/\/.*/, "", stripped)
    buf = buf " " stripped
    n = length(stripped)
    for (i = 1; i <= n; i++) {
      ch = substr(stripped, i, 1)
      if (ch == "{") depth++
      else if (ch == "}") depth--
      else if (ch == ";" && depth == 0) { inuse = 0; emit(buf); buf = ""; break }
    }
  }
  function emit(line,   parts, n, i, t) {
    if (line ~ /\{/) { sub(/^[^{]*\{/, "", line); sub(/\}[^}]*$/, "", line) }
    else { sub(/^[[:space:]]*pub use[[:space:]]*/, "", line); sub(/;.*$/, "", line) }
    n = split(line, parts, ",")
    for (i = 1; i <= n; i++) {
      t = parts[i]
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", t)
      if (t ~ /[[:space:]]as[[:space:]]/) sub(/^.*[[:space:]]as[[:space:]]+/, "", t)
      sub(/^.*::/, "", t)
      if (t ~ /^[a-z_][a-z_0-9]*$/) print t
    }
  }
' $REEXPORTS | sort -u)

for name in $reexported; do
  echo "$modules" | grep -q " ${name} " || rust="$rust
$name"
done

gpu_names=$(awk '
  /^impl( <[^>]*>)? / { skip = 0 }
  skip == 0 && /^[[:space:]]*pub (const )?fn [a-z_0-9]+/ {
    line = $0
    sub(/.*fn /, "", line); sub(/[^a-z_0-9].*/, "", line)
    print line
  }
' $GPU | sort -u)
rust="$rust
$gpu_names"

types=$(awk -f "$(dirname "$0")/c-parity-types.awk" $REEXPORTS | sort -u)

aliases=$(sed -n 's/^pub use .*::\([A-Za-z_][A-Za-z_0-9]*\) as \([A-Za-z_][A-Za-z_0-9]*\);.*/\2 \1/p' \
            $REEXPORTS 2>/dev/null)

for ty in $types; do
  # A type re-exported under another name has no `impl <alias>` to find, so resolve it first or its
  # methods are skipped in silence – which is how DisplayList's went missing under `as ColorScene`.
  real=$(echo "$aliases" | awk -v a="$ty" '$1 == a { print $2; exit }')
  [ -n "$real" ] && ty="$real"
  files=$(grep -rl "^impl.*[[:space:]]${ty}[[:space:]<{]" src/daecore/src src/daerizer/src src/daegun 2>/dev/null | head -20)
  [ -z "$files" ] && continue
  methods=$(awk -v T="$ty" '
    $0 ~ ("^impl(<[^>]*>)?[[:space:]]+" T "([<[:space:]{]|$)") { inimpl = 1; next }
    inimpl && /^}/ { inimpl = 0 }
    inimpl && /^[[:space:]]*pub (const )?fn [a-z_0-9]+/ {
      line = $0; sub(/.*fn /, "", line); sub(/[^a-z_0-9].*/, "", line); print line
    }
  ' $files)
  rust="$rust
$methods"
done
rust=$(echo "$rust" | grep -c . >/dev/null && echo "$rust" | sort -u)

lib=$(ls -t target/debug/libdaegun.dylib target/debug/libdaegun.so target/debug/libdaegun.a \
        2>/dev/null | head -1)
case "$lib" in
  *.dylib) syms=$(nm -gU "$lib" 2>/dev/null || true) ;;
  *.so)    syms=$(nm -D --defined-only "$lib" 2>/dev/null || true) ;;
  *.a)     syms=$(nm -g "$lib" 2>/dev/null || true) ;;
  *)       echo "not built (run: cargo rustc --features capi --crate-type staticlib)"; exit 0 ;;
esac
c=$(echo "$syms" | grep -oE '_?daegun_[a-z_0-9]+' | sed -E 's/^_?daegun_//' | sort -u)

renamed=" from_bytes from_ttc with_layout with_gamma with_transform with_hinting with_stroke\
 with_embolden with_oblique math_constants named_instances cpu_only with_policy build_font\
 parse_item_variation_store parse_delta_set_index_map precompute_region_scalars\
 compute_ivs_delta_f64 draw_hinted line_height parts at_any_size prefer strictly\
 channels grayscale horizontal is_grayscale oversample taps unfiltered weight_rows\
 geometry target shader from_vec push ops "

# `fade` and `opaque` build an Rgba, which C passes as four plain bytes – there is nothing
# for an entry point to do that the caller cannot write inline.
unreachable=" append_prebuilt build_glyph remember slot_for fade opaque "

# The only names allowed to pass on a header declaration alone. Anything else declared but not
# built is a deleted implementation, not a platform gate, and must fail.
platform_only=" feature_level "

CW=src/c-wrapper
total=$(echo "$rust" | grep -c . || true)
missing=""
gated=""
loose=""
covered=0
for name in $rust; do
  if echo "$renamed" | grep -q " ${name} " || echo "$unreachable" | grep -q " ${name} "; then
    covered=$((covered + 1))
  elif echo "$c" | grep -qE "(^|_)${name}$"; then
    covered=$((covered + 1))
  elif echo "$c" | grep -qE "(^|_)${name}_"; then
    covered=$((covered + 1))
    loose="$loose $name"
  elif echo "$platform_only" | grep -q " ${name} " \
       && grep -qE "daegun_[a-z0-9_]*(^|_)?${name} *\(" "$CW/daegun.h"; then
    covered=$((covered + 1))
    gated="$gated $name"
  else
    missing="$missing $name"
  fi
done

pct=$((covered * 100 / (total > 0 ? total : 1)))
printf '%d of %d reachable from C (%d%%)\n' "$covered" "$total" "$pct"

if [ -n "$loose" ]; then
  printf '  (loosely matched, check by hand:%s)\n' "$loose"
fi

if [ -n "$gated" ]; then
  printf '  (%s: declared, built on another platform)\n' "$(echo "$gated" | sed 's/^ //')"
fi

if [ "${1:-}" = "--list" ] && [ -n "$missing" ]; then
  echo "$missing" | tr ' ' '\n' | grep -v '^$' | sed 's/^/    /'
fi

# The pass above pools all four backends into one name set, so a call present on one counts as
# covered for all of them – which is how Metal's adoption and borrow went missing while the total
# still read 100%. Adoption and surfaces are per backend, so they get their own pass over whichever
# backends were compiled in here.
surface=""
for b in metal vulkan d3d11 d3d12; do
  echo "$c" | grep -qx "${b}_renderer_new" || continue
  for call in renderer_from_device target_with_format target_set_clear; do
    echo "$c" | grep -qx "${b}_${call}" || surface="$surface daegun_${b}_${call}"
  done
  echo "$c" | grep -qE "^${b}_target_from_(texture|image|drawable)$" \
    || surface="$surface daegun_${b}_target_from_*"
done

if [ -n "$missing" ] || [ -n "$loose" ] || [ -n "$surface" ]; then
  [ -n "$missing" ] && printf 'not reachable from C:%s\n' "$missing"
  [ -n "$loose" ] && printf 'matched only loosely, so not confirmed:%s\n' "$loose"
  [ -n "$surface" ] && printf 'backend surface API not reachable from C:%s\n' "$surface"
  exit 1
fi
