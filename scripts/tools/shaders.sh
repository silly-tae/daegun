#!/bin/sh
set -e
cd "$(dirname "$0")/../.."
SHADERS=src/daerizer/src/daegpu/shaders

command -v glslangValidator >/dev/null 2>&1 || {
  echo "glslangValidator not found — install it (brew install glslang) or skip this check"
  exit 127
}

if command -v vkd3d-compiler >/dev/null 2>&1; then HAVE_VKD3D=1; else HAVE_VKD3D=0; fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
fail=0

check() {
  printf '  %-6s %-9s %-11s ' "$1" "$2" "$3"
}

for stage in VERTEX FRAGMENT SUBPIXEL; do
  case $stage in VERTEX) s=vert ;; *) s=frag ;; esac

  for v in "430 core:desktop" "310 es:es"; do
    ver=${v%%:*}
    name=${v##*:}
    {
      echo "#version $ver"
      [ "$name" = es ] && echo "precision highp float;"
      [ "$name" = es ] && [ "$stage" = SUBPIXEL ] && echo "#extension GL_EXT_blend_func_extended : require"
      echo "#define DAEGUN_$stage 1"
      echo "#line 1"
      cat "$SHADERS/daegun.glsl"
    } > "$TMP/g.$s"
    check GLSL "$stage" "$name"
    if glslangValidator -S $s "$TMP/g.$s" >"$TMP/out" 2>&1; then
      echo ok
    else
      echo FAILED
      sed 's/^/      /' "$TMP/out" | head -12
      fail=1
    fi
  done

  {
    echo "#define DAEGUN_$stage 1"
    echo "#line 1"
    cat "$SHADERS/daegun.hlsl"
  } > "$TMP/h.hlsl"
  check HLSL "$stage" ""
  if glslangValidator -D -e main -S $s -V "$TMP/h.hlsl" -o "$TMP/h.spv" >"$TMP/out" 2>&1; then
    echo ok
  else
    echo FAILED
    sed 's/^/      /' "$TMP/out" | head -12
    fail=1
  fi

  if [ "$HAVE_VKD3D" -eq 1 ]; then
    { echo "#define DAEGUN_$stage 1"; cat "$SHADERS/daegun.hlsl"; } > "$TMP/d.hlsl"
    case $stage in VERTEX) k=vs ;; *) k=ps ;; esac
    for model in 5_0 5_1; do
      case $model in 5_0) api=D3D11 ;; *) api=D3D12 ;; esac
      check "$api" "$stage" "${k}_${model}"
      if vkd3d-compiler -x hlsl -b dxbc-tpf -p "${k}_${model}" -e main \
           -o "$TMP/d.dxbc" "$TMP/d.hlsl" >"$TMP/out" 2>&1; then
        echo ok
      else
        echo FAILED
        sed 's/^/      /' "$TMP/out" | head -12
        fail=1
      fi
    done
  fi

  {
    echo "#version 450"
    echo "#define DAEGUN_VULKAN 1"
    echo "#define DAEGUN_$stage 1"
    echo "#line 1"
    cat "$SHADERS/daegun.glsl"
  } > "$TMP/v.$s"
  check Vulkan "$stage" "SPIR-V"
  out="$SHADERS/daegun.$(echo "$stage" | tr '[:upper:]' '[:lower:]').spv"
  if glslangValidator -V -S $s "$TMP/v.$s" -o "$TMP/v.spv" >"$TMP/out" 2>&1; then
    if [ "${1:-}" = "--write" ]; then
      cp "$TMP/v.spv" "$out"
      echo "written"
    elif [ ! -f "$out" ]; then
      echo "MISSING — run with --write"
      fail=1
    elif cmp -s "$TMP/v.spv" "$out"; then
      echo ok
    else
      echo "STALE — the shader changed but $(basename "$out") did not; run with --write"
      fail=1
    fi
  else
    echo FAILED
    sed 's/^/      /' "$TMP/out" | head -12
    fail=1
  fi
done

printf '  %-6s %-9s %-11s ' Metal "all three" ""
echo "via cargo test --test gpu (runtime compiler)"

if [ "$HAVE_VKD3D" -eq 0 ]; then
  printf '  %-6s %-9s %-11s ' D3D "all six" ""
  echo "skipped (vkd3d-compiler not installed — see tasks/direct3d.md)"
fi

exit $fail
