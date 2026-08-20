#!/bin/sh
set -eu
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}"
BIN="$1"; FILTER="$2"; ITERS="$3"; RUNS="${4:-3}"
OUT="${TMPDIR:-/tmp}/pmu.$$"; mkdir -p "$OUT"
MIN_SAMPLES="${MIN_SAMPLES:-2000}"

cleanup() {
    rm -rf "$OUT"
    find "${TMPDIR:-/tmp}" -maxdepth 1 -name 'instruments*.ktrace' -mmin +60 -exec rm -rf {} + 2>/dev/null || true
}
trap cleanup EXIT INT TERM

cat > "$OUT/opts.json" <<'JSON'
{ "CPU Counters" : {
  "allEventsAndFormulas" : [], "configurationType" : { "guided" : {} },
  "countingLevel" : "EL0", "pmiEventAliasOrMnemonic" : "", "pmiThreshold" : 1000000,
  "processBucketSize" : 10, "sampleByTime" : true,
  "selectedCountingMode" : { "analysisMode" : "bottleneck", "countingMode" : "bottlenecks" },
  "selectedCountingModeDisplayName" : "CPU Bottlenecks", "useDebuggingInformation" : false,
  "useHighFrequencyForGuidedMode" : true, "useHighFrequencyForManualMode" : true } }
JSON

ATTEMPTS=$((RUNS * 3))
i=0
while [ "$i" -lt "$ATTEMPTS" ]; do
    i=$((i + 1))
    xcrun xctrace record --template 'CPU Counters' --recording-options "$OUT/opts.json" \
        --output "$OUT/r$i.trace" --launch -- \
        "$BIN" "$FILTER" --ignored --nocapture --test-threads 1 >/dev/null 2>&1 || true
    xcrun xctrace export --input "$OUT/r$i.trace" \
        --xpath '/trace-toc/run[@number="1"]/data/table[@schema="kdebug-counters-with-time-sample"]' \
        2>/dev/null > "$OUT/r$i.xml" || true
done
MIN_SAMPLES="$MIN_SAMPLES" python3 "$(dirname "$0")/pmu.py" "$OUT" "$ITERS" "$ATTEMPTS"
