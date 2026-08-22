# daegun benchmarks

Every latency benchmark in the tree: 35 tests across six targets, plus the C harness. Nothing here
is a summary of something else.

Re-measured for 1.1.5. Every figure below is the best of three consecutive runs, which is what the
closing note recommends and what the previous edition did not do.

## Machine

| | |
|---|---|
| CPU | Apple M1 Pro |
| Cores | 10 physical / 10 logical |
| Cache line | 128 bytes |
| RAM | 32 GB |
| OS | darwin 27.0.0 |

## Configuration

| | |
|---|---|
| daegun | 1.1.5 |
| rustc | 1.97.1 (8bab26f4f 2026-07-14) |
| Profile | release |
| `opt-level` | 3 |
| `lto` | true |
| `codegen-units` | 1 |
| `panic` | abort |
| `overflow-checks` | false |
| Command | `cargo test --release --test <target> latency -- --ignored --nocapture --test-threads=1` |

## What moved since 1.1.0

The optimization work behind 1.1.5 targeted the shape cache hit path, the rasterizer's overlap
resolution, and the allocations behind both. Rows that trace to a change made on purpose:

| | 1.1.0 | 1.1.5 | |
|---|---|---|---|
| `advance_widths` ×1 | 26.0 ns | 19.6 ns | −25% |
| `rasterize_glyph`, cached | 73.4 ns | 63.5 ns | −13% |
| `colr_v1_paint_graph_variable` | 621.708 µs | 555.584 µs | −11% |
| `cpu_pipeline_by_glyph`, `B` | 4.999 µs | 4.543 µs | −9% |

The first three share a cause: resolving an axis position stopped building a `BTreeMap` and two
intermediate vectors to hold what is almost always one axis. The fourth is the rasterizer's
overlap resolution, whose innermost loop no longer divides.

Other rows moved too – `font_open` by 5%, the whole C column by more – but nothing was changed on
those paths, and the previous edition was a single run where this one is the best of three. Read
those as the measurement improving, not the engine.

Shaping is flat, because these tests measure a fresh shape rather than a repeat and so never touch
the run cache that got 17× cheaper. The maths and GPU sections are unchanged work and move only
with the noise floor.

---

## 1. Rust API

`--test api` · `api_latency` · 200 rounds after 50 warmup

| | min | median |
|---|---|---|
| `from_bytes`, borrowed | 15.375 µs | 15.917 µs |
| `from_vec`, owned | 1.125 µs | 1.250 µs |
| `rasterize_glyph`, uncached | 4.711 µs | 4.801 µs |
| `outline_glyph` | 248.2 ns | 248.6 ns |
| `rasterize_glyph`, cached | 63.5 ns | 64.7 ns |
| `glyph_id` | 63.2 ns | 63.5 ns |
| `advance_widths` ×1 | 19.6 ns | 19.8 ns |
| `line_metrics` | 9.2 ns | 9.3 ns |
| `descender` | 6.7 ns | 6.8 ns |
| `ascender` | 6.4 ns | 6.5 ns |
| `cap_height` | 0.8 ns | 0.9 ns |
| `num_glyphs` | 0.2 ns | 0.3 ns |
| `upm` | 0.2 ns | 0.3 ns |

---

## 2. Shaping

`--test shaper` · 5 tests · one shaped run per sample

| test | font | n | min | median | p95 |
|---|---|---|---|---|---|
| `shape_cjk_sentence_source_han` | Source Han Sans JP, 30 chars | 120,000 | 916 ns | 1.042 µs | 1.084 µs |
| `shape_latin_ligatures_eb_garamond` | EB Garamond, liga fires repeatedly | 20,000 | 2.917 µs | 3.084 µs | 3.250 µs |
| `shape_devanagari_conjuncts_noto` | Noto Sans Devanagari, conjuncts and matra reordering | 8,000 | 8.291 µs | 8.500 µs | 9.084 µs |
| `shape_arabic_joined_run_scheherazade` | Scheherazade New, one fully joined run | 15,000 | 8.458 µs | 8.708 µs | 9.250 µs |
| `shape_latin_sentence_inter` | Inter, 78-character sentence | 20,000 | 9.500 µs | 9.708 µs | 10.292 µs |

---

## 3. Outlines

`--test type` · `outline_latency` · 9 tests

| test | scope | n | min | median |
|---|---|---|---|---|
| `autohint_inter_h` | Inter `H` at 13 ppem, glyf, collect + grid fit | 20,000 | 458 ns | 583 ns |
| `autohint_stix_h` | STIX `H` at 13 ppem, CFF, collect + grid fit | 20,000 | 1.167 µs | 1.291 µs |
| `outline_glyf_eb_garamond_composite` | EB Garamond gid 2244, 7-component composite | 50,000 | 1.291 µs | 1.417 µs |
| `outline_glyf_scheherazade` | Scheherazade gid 1583, 1,683 points | 20,000 | 9.041 µs | 9.167 µs |
| `outline_cff_stix` | STIX gid 2257, 5,819-byte Type 2 charstring | 2,000 | 11.250 µs | 11.459 µs |
| `outline_glyf_sweep` | EB Garamond, 3,247 glyphs, whole face | 60 | 2.247 ms | 2.351 ms |
| `outline_cff_sweep` | STIX, 5,543 glyphs, 123,321 segments | 30 | 2.336 ms | 2.356 ms |
| `autohint_sweep_inter` | Inter, 2,937 glyphs, 131,498 points | 20 | 6.753 ms | 6.929 ms |
| `autohint_sweep_stix` | STIX, 5,543 glyphs, 211,893 points | 10 | 15.472 ms | 15.662 ms |

---

## 4. Hinting

`--test type` · `hint_latency` · 3 tests

| test | scope | n | min | median |
|---|---|---|---|---|
| `hint_glyph_context_cached` | 5 glyphs through `FontCache` | 40,000 | 792 ns | 917 ns |
| `hint_glyph_bytecode` | 5 hinted glyphs at 16 ppem | 40,000 | 833 ns | 917 ns |
| `hint_glyph_context_per_glyph` | 5 glyphs, `HintContext` rebuilt each time | 4,000 | 3.375 µs | 3.708 µs |

---

## 5. Colour

`--test type` · `colr_latency` · 3 tests

| test | scope | n | min | median |
|---|---|---|---|---|
| `colr_v1_paint_graph_static` | 200 base glyphs, whole sweep | 2,000 | 35.583 µs | 36.334 µs |
| `cache_colr_variable` | 200 base glyphs through `FontCache` | 2,000 | 39.958 µs | 40.500 µs |
| `colr_v1_paint_graph_variable` | 200 base glyphs, whole sweep | 2,000 | 555.584 µs | 577.292 µs |

---
## 6. Rasterizing, CPU

`--test cpu` · 3 tests

### `cpu_pipeline_by_glyph` – 16px, grayscale, no gamma

| glyph | pen ops | bitmap | raster | flatten | alloc | draw | resolve |
|---|---|---|---|---|---|---|---|
| `.` | 10 | 3×3 | 0.291 µs | 57.4% | 14.1% | 14.4% | 14.1% |
| `l` | 34 | 4×13 | 0.707 µs | 58.8% | 5.8% | 23.6% | 11.7% |
| `W` | 122 | 15×12 | 2.082 µs | 48.0% | 2.0% | 38.0% | 12.0% |
| `o` | 31 | 8×8 | 2.833 µs | 83.8% | 1.4% | 11.8% | 2.9% |
| `B` | 71 | 9×12 | 4.543 µs | 86.2% | 0.9% | 10.1% | 2.8% |
| `@` | 86 | 12×12 | 7.959 µs | 87.4% | 0.5% | 10.0% | 2.1% |
| `g` | 76 | 8×12 | 41.208 µs | 98.2% | 0.1% | 1.4% | 0.3% |

### `cpu_pipeline_by_size` – EB Garamond `B`, 71 pen ops, grayscale, no gamma

| size | bitmap | raster | flatten | alloc | draw | resolve |
|---|---|---|---|---|---|---|
| 12px | 7×9 | 4.166 µs | 87.0% | 1.0% | 10.0% | 2.0% |
| 16px | 9×12 | 4.542 µs | 86.2% | 0.9% | 10.1% | 2.8% |
| 24px | 14×17 | 5.666 µs | 81.6% | 0.7% | 11.8% | 5.9% |
| 32px | 18×23 | 6.584 µs | 78.5% | 0.6% | 12.7% | 8.2% |
| 64px | 35×44 | 8.959 µs | 61.4% | 0.9% | 15.3% | 22.3% |
| 128px | 69×86 | 16.667 µs | 38.2% | 1.5% | 14.7% | 45.5% |
| 256px | 137×171 | 45.876 µs | 21.1% | 3.4% | 9.9% | 65.7% |

### `cpu_pipeline_by_layout` – EB Garamond `B`

| layout | size | bitmap | raster | flatten | alloc | draw | resolve |
|---|---|---|---|---|---|---|---|
| grayscale | 16px | 9×12 | 4.583 µs | 85.5% | 0.9% | 10.9% | 2.7% |
| grayscale + gamma | 16px | 9×12 | 4.543 µs | 85.3% | 0.9% | 10.1% | 3.7% |
| subpixel RGB + gamma | 16px | 11×12 | 6.209 µs | 63.1% | 0.7% | 12.1% | 24.2% |
| subpixel RGB | 16px | 11×12 | 6.250 µs | 62.7% | 0.7% | 12.0% | 24.7% |
| grayscale | 64px | 35×44 | 8.916 µs | 60.7% | 1.4% | 15.4% | 22.4% |
| subpixel RGB | 64px | 37×44 | 26.708 µs | 20.1% | 0.9% | 9.4% | 69.6% |

Resolve is what subpixel costs: it is a quarter of a 16px glyph and seven tenths of a 64px one,
because the coverage buffer is three times wider and every sample gets filtered.

---

## 7. Rasterizing, GPU

`--test gpu` · 2 tests · empty draw, median of 200, stages cumulative

### `draw_cost_by_stage`

| backend | target | submit | +wait | +read | readback |
|---|---|---|---|---|---|
| Metal | 64×64 | 10.0 µs | 276.0 µs | 500.7 µs | 224.7 µs |
| Metal | 256×256 | 8.7 µs | 276.4 µs | 506.2 µs | 229.8 µs |
| Metal | 512×512 | 9.9 µs | 277.5 µs | 532.8 µs | 255.3 µs |
| Metal | 1024×1024 | 10.9 µs | 288.8 µs | 559.2 µs | 270.4 µs |
| Metal | 2048×1024 | 9.4 µs | 310.2 µs | 605.9 µs | 295.7 µs |
| Vulkan | 64×64 | 122.1 µs | 333.1 µs | 599.9 µs | 266.8 µs |
| Vulkan | 256×256 | 123.9 µs | 337.3 µs | 586.4 µs | 249.0 µs |
| Vulkan | 512×512 | 125.0 µs | 337.3 µs | 607.5 µs | 270.2 µs |
| Vulkan | 1024×1024 | 124.9 µs | 333.2 µs | 660.4 µs | 327.1 µs |
| Vulkan | 2048×1024 | 115.1 µs | 367.9 µs | 668.7 µs | 300.0 µs |

Fitted:

| backend | stage | fixed | per 1000 px |
|---|---|---|---|
| Metal | submit | 6.9 µs | 0.013 µs |
| Metal | +wait | 275.0 µs | 0.016 µs |
| Metal | +read | 510.1 µs | 0.046 µs |
| Vulkan | submit | 122.6 µs | 0.006 µs |
| Vulkan | +wait | 331.9 µs | 0.014 µs |
| Vulkan | +read | 597.4 µs | 0.039 µs |

Taking the best of three runs cleans up the submit fit that used to slope the wrong way: the
first-call cost of a run lands in whichever size is measured first, and it no longer survives three
passes. Wait and read were always the stable ones.

### `a_read_is_never_cheaper_than_the_wait_inside_it`

| backend | wait | read |
|---|---|---|
| Metal | 310.6 µs | 561.0 µs |
| Vulkan | 358.8 µs | 604.3 µs |

---
## 8. Maths

`--test machine` · 9 tests · daegun is `no_std` and carries its own float maths

### `float_ext_against_std` – ratio > 1 means daegun is slower

| | daegun | std | ratio |
|---|---|---|---|
| `f64 abs` | 0.234 ns | 0.234 ns | 1.00× |
| `f32 abs` | 0.244 ns | 0.244 ns | 1.00× |
| `f32 round_ties_even` | 0.376 ns | 0.244 ns | 1.54× |
| `f32 floor` | 0.438 ns | 0.244 ns | 1.79× |
| `f64 round_ties_even` | 0.549 ns | 0.234 ns | 2.33× |
| `f32 round` | 0.631 ns | 0.244 ns | 2.58× |
| `f64 trunc` | 0.621 ns | 0.234 ns | 2.65× |
| `f64 ceil` | 0.661 ns | 0.234 ns | 2.82× |
| `f64 floor` | 0.926 ns | 0.234 ns | 3.95× |
| `f64 round` | 1.404 ns | 0.234 ns | 6.00× |

### `sqrt_against_std` – Newton iteration against one hardware instruction

| | daegun | std | ratio |
|---|---|---|---|
| `f32 sqrt` | 2.340 ns | 0.285 ns | 8.21× |
| `f64 sqrt` | 2.706 ns | 0.315 ns | 8.58× |

### `trig_against_std`

| | daegun | std | ratio |
|---|---|---|---|
| `f64 sin_cos` | 3.245 ns | 7.070 ns | 0.44× |
| `f64 atan2` | 7.558 ns | 7.578 ns | 0.94× |

### `rounding_old_against_new` – ratio < 1 means the new one is faster

| | new | old | ratio |
|---|---|---|---|
| `round_ties_even` | 0.549 ns | 1.943 ns | 0.27× |
| `ceil` | 0.661 ns | 1.546 ns | 0.43× |
| `floor` | 0.926 ns | 0.977 ns | 0.93× |
| `trunc` | 0.621 ns | 0.621 ns | 1.00× |
| `round` | 1.404 ns | 1.404 ns | 1.00× |

### `atan2_cost_breakdown`

| | first | second | ratio |
|---|---|---|---|
| atan2 vs one divide | 7.222 ns | 0.336 ns | 20.89× |
| atan2 vs horner20 | 7.212 ns | 3.286 ns | 2.19× |
| horner20 vs one divide | 3.296 ns | 0.336 ns | 9.82× |
| estrin20 vs horner20 | 2.156 ns | 3.296 ns | 0.65× |

### `atan_polynomial_shape` – 11 coefficients, identical inputs

| | first | second | ratio |
|---|---|---|---|
| estrin4 vs current | 1.312 ns | 1.271 ns | 1.03× |

### `atan_reduction_shape` – ratio < 1 means the first named is faster

| | first | second | ratio |
|---|---|---|---|
| hybrid, one arm | 2.838 ns | 4.028 ns | 0.70× |
| branchless, scattered | 5.117 ns | 5.687 ns | 0.90× |
| branchless, raster sweep | 5.117 ns | 5.524 ns | 0.92× |
| hybrid, mid range | 5.737 ns | 5.890 ns | 0.94× |
| hybrid, raster sweep | 5.259 ns | 5.524 ns | 0.95× |
| hybrid, scattered | 5.829 ns | 5.687 ns | 1.03× |
| branchless, one arm | 5.117 ns | 4.028 ns | 1.27× |

### `sqrt_iteration_shape`

| | first | second | ratio |
|---|---|---|---|
| rsqrt 4 vs current | 2.787 ns | 2.706 ns | 1.03× |
| rsqrt 5 vs current | 3.530 ns | 2.706 ns | 1.30× |
| current vs std sqrt | 2.716 ns | 0.305 ns | 8.68× |

Accuracy against std, worst ulp over 60,000 values in [1e-8, 1e8]:

| | worst ulp |
|---|---|
| newton on root, 4 passes | 1 |
| newton on reciprocal, 5 passes | 2 |
| newton on reciprocal, 4 passes | 3 |

### `daemath_baseline` – blending, per pixel, anchored on SrcOver

| | ns/px | anchor | ratio |
|---|---|---|---|
| blend Multiply | 1.943 | 2.238 | 0.87× |
| blend HardLight | 4.995 | 2.248 | 2.21× |
| blend HslSaturation | 7.925 | 2.238 | 3.54× |
| composite SrcOver | 5.483 | 1.007 | 5.36× |

### `daemath_baseline` – gradients, per pixel, anchored on linear

| | ns/px | anchor | ratio |
|---|---|---|---|
| gradient linear | 14.974 | anchor | 1.00× |
| gradient radial | 21.820 | 14.974 | 1.44× |
| gradient sweep | 32.104 | 15.208 | 2.03× |

---

## 9. The C ABI

`src/c-wrapper/tests/latency.c` · 200 rounds after 50 warmup · both sides on a release build

| | Rust min | C min | Rust median | C median |
|---|---|---|---|---|
| `from_bytes`, borrowed | 15.375 µs | 15.000 µs | 15.917 µs | 16.000 µs |
| `from_vec`, owned | 1.125 µs | 1.000 µs | 1.250 µs | 1.000 µs |
| `rasterize_glyph`, uncached | 4.711 µs | 4.684 µs | 4.801 µs | 4.768 µs |
| `outline_glyph` | 248.2 ns | 270.0 ns | 248.6 ns | 272.0 ns |
| `rasterize_glyph`, cached | 63.5 ns | 104.0 ns | 64.7 ns | 106.0 ns |
| `glyph_id` | 63.2 ns | 66.0 ns | 63.5 ns | 68.0 ns |
| `advance_widths` ×1 | 19.6 ns | 48.0 ns | 19.8 ns | 50.0 ns |
| `upm` | 0.2 ns | 0.0 ns | 0.3 ns | 0.0 ns |

The C column comes off `clock_gettime` over batches of 500, which quantises it to 2 ns steps. Read
the cheap rows as an upper bound rather than a figure – `upm` at 0.0 ns means "below what this clock
can see", not free.

The overhead over Rust fell on both cheap rows – `glyph_id` from 35 ns to 3 ns, `advance_widths`
from 48 ns to 28 ns. Only part of that is real: `advance_widths` got faster on the Rust side too,
but `glyph_id` did not change at all this release, so most of its apparent gain is the previous
edition having been a single run of a clock that quantises to 2 ns steps. Treat the C column as
bounded above, and compare releases on the Rust column.

---

## About these numbers

They are latency tests living beside the ordinary ones, marked `#[ignore]` because they measure
rather than assert. The gate runs what can fail; these report, and are run by hand.

Every figure is the best of three consecutive runs. A single run of this suite moves by several
percent between passes, and more than that on the GPU stages, so a lone number is not a baseline.

`--test-threads=1` is not optional. The reports are multi-line and interleave into nonsense without
it, which makes them look broken rather than unread.

`min` rather than mean is the number to watch for a regression, being the least polluted by whatever
else the machine was doing. Section 9 is not a cargo target and has to be built against a release
staticlib, or it measures a debug library and reports several times the real cost:

```sh
cargo rustc --release --features capi --crate-type staticlib
cc -std=c11 -O2 -I src/c-wrapper src/c-wrapper/tests/latency.c target/release/libdaegun.a \
   <platform frameworks> -o clat && ./clat assets/test-fonts/inter/InterVariable.ttf
```

Each sweep in sections 3 to 5 runs a different face, so divide by the glyph count before reading
anything across their rows. `DAEGUN_SWEEP_FONT` repoints `outline_glyf_sweep`; every other sweep is
fixed to the face named beside it.

Nothing here measures a whole frame. For that – frame times as a distribution, cache hit rates, and
where a frame's milliseconds actually go – see `tasks/stat-log-2.md`, which covers the engine under
a real-time budget rather than call by call.

For counters rather than wall clock, `scripts/tools/perf/pmu.sh` drives Instruments, and
`insn-diff.py` and `pmu-attribute.py` beside it attribute the difference between two builds down to
the function. `tasks/baselines/` holds captured before-and-after studies from past optimization work.
