#!/bin/sh
set -eu
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}"
cd "$(dirname "$0")/../../.."

FILTER="$1"
OUT="${2:-${TMPDIR:-/tmp}/attr.$$}"; mkdir -p "$OUT"
MIN_ROWS="${MIN_ROWS:-1200}"
ATTEMPTS="${ATTEMPTS:-8}"

trap "find \"${TMPDIR:-/tmp}\" -maxdepth 1 -name 'instruments*.ktrace' -mmin +60 -exec rm -rf {} + 2>/dev/null || true" EXIT INT TERM

PROFILE="${PROFILE:-pmu}"
BENCH_PKG="${BENCH_PKG:-daecore}"
BENCH_TARGET="${BENCH_TARGET:-shaper}"
BIN=$(cargo test -p "$BENCH_PKG" --test "$BENCH_TARGET" --profile "$PROFILE" --no-run --message-format=json 2>/dev/null \
      | python3 "$(dirname "$0")/pick-test-binary.py" "$BENCH_TARGET")
[ -n "$BIN" ] || { echo "no test binary — did the build fail?" >&2; exit 1; }

if [ "$("$BIN" --ignored --list 2>/dev/null | grep -c ': test$')" -eq 0 ]; then
    echo "$BENCH_PKG/$BENCH_TARGET has no #[ignore] benches — nothing to measure." >&2
    echo "Write them in src/$BENCH_PKG/tests/$BENCH_TARGET/, or set BENCH_PKG/BENCH_TARGET." >&2
    exit 1
fi

xcrun xctrace record --template 'CPU Counters' --show-recording-options 2>/dev/null > "$OUT/base.json"
python3 "$(dirname "$0")/attr-options.py" "$OUT"

ROWS=0
i=0
while [ "$i" -lt "$ATTEMPTS" ]; do
    i=$((i + 1))
    rm -rf "$OUT/t.trace"
    xcrun xctrace record --template 'CPU Counters' --recording-options "$OUT/opts.json" \
        --output "$OUT/t.trace" --launch -- \
        "$BIN" "$FILTER" --ignored --nocapture --test-threads 1 >/dev/null 2>&1 || true
    for t in time-profile kdebug-counters-with-time-sample; do
        xcrun xctrace export --input "$OUT/t.trace" \
            --xpath "/trace-toc/run[@number=\"1\"]/data/table[@schema=\"$t\"]" \
            2>/dev/null > "$OUT/$t.xml" || true
    done
    ROWS=$(grep -c '<row>' "$OUT/kdebug-counters-with-time-sample.xml" 2>/dev/null || echo 0)
    STACKS=$(grep -c '<row>' "$OUT/time-profile.xml" 2>/dev/null || echo 0)
    printf '  attempt %s: %s counter rows, %s stack rows\n' "$i" "$ROWS" "$STACKS" >&2
    if [ "$ROWS" -ge "$MIN_ROWS" ]; then break; fi
done
if [ "$ROWS" -lt "$MIN_ROWS" ]; then
    echo "never reached $MIN_ROWS counter rows in $ATTEMPTS attempts — every one fell back to 1 ms" >&2
    exit 1
fi
echo "$OUT"
