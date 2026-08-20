# daegun benchmarks

Every latency benchmark in the tree: 35 tests across six targets, plus the C harness. Nothing here
is a summary of something else.

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
| rustc | 1.97.1 (8bab26f4f 2026-07-14) |
| Profile | release |
| `opt-level` | 3 |
| `lto` | true |
| `codegen-units` | 1 |
| `panic` | abort |
| `overflow-checks` | false |
| Command | `cargo test --release --test <target> latency -- --ignored --nocapture --test-threads=1` |

---

## 1. Rust API

`--test api` · `api_latency` · 200 rounds after 50 warmup

| | min | median |
|---|---|---|
| `from_bytes`, borrowed | 16.17 µs | 16.29 µs |
| `from_vec`, owned | 1.13 µs | 1.29 µs |
| `rasterize_glyph`, uncached | 4.91 µs | 4.95 µs |
| `outline_glyph` | 250.7 ns | 250.9 ns |
| `rasterize_glyph`, cached | 73.4 ns | 74.8 ns |
| `glyph_id` | 63.6 ns | 63.8 ns |
| `advance_widths` ×1 | 26.0 ns | 26.2 ns |
| `line_metrics` | 9.2 ns | 9.4 ns |
| `ascender` | 7.0 ns | 7.2 ns |
| `descender` | 6.4 ns | 6.5 ns |
| `cap_height` | 0.9 ns | 1.0 ns |
| `num_glyphs` | 0.3 ns | 0.3 ns |
| `upm` | 0.2 ns | 0.3 ns |

---

## 2. Shaping

`--test shaper` · 5 tests · one shaped run per sample

| test | font | n | min | median | p95 |
|---|---|---|---|---|---|
| `shape_cjk_sentence_source_han` | Source Han Sans JP, 30 chars | 120,000 | 916 ns | 1.042 µs | 1.125 µs |
| `shape_latin_ligatures_eb_garamond` | EB Garamond, liga fires repeatedly | 20,000 | 2.958 µs | 3.167 µs | 3.375 µs |
| `shape_devanagari_conjuncts_noto` | Noto Sans Devanagari, conjuncts and matra reordering | 8,000 | 8.333 µs | 8.500 µs | 8.833 µs |
| `shape_arabic_joined_run_scheherazade` | Scheherazade New, one fully joined run | 15,000 | 8.458 µs | 8.750 µs | 9.458 µs |
| `shape_latin_sentence_inter` | Inter, 78-character sentence | 20,000 | 9.541 µs | 9.833 µs | 10.500 µs |

---

## 3. Outlines

`--test type` · `outline_latency` · 9 tests

| test | scope | n | min | median |
|---|---|---|---|---|
| `autohint_inter_h` | Inter `H` at 13 ppem, glyf, collect + grid fit | 20,000 | 458 ns | 583 ns |
| `autohint_stix_h` | STIX `H` at 13 ppem, CFF, collect + grid fit | 20,000 | 1.209 µs | 1.292 µs |
| `outline_glyf_eb_garamond_composite` | EB Garamond gid 2244, 7-component composite | 50,000 | 1.292 µs | 1.417 µs |
| `outline_glyf_scheherazade` | Scheherazade gid 1583, 1,683 points | 20,000 | 9.041 µs | 9.375 µs |
| `outline_cff_stix` | STIX gid 2257, 5,819-byte Type 2 charstring | 2,000 | 11.458 µs | 11.875 µs |
| `outline_glyf_sweep` | EB Garamond, 3,247 glyphs, whole face | 60 | 2.280 ms | 2.411 ms |
| `outline_cff_sweep` | STIX, 5,543 glyphs, 123,321 segments | 30 | 2.367 ms | 2.461 ms |
| `autohint_sweep_inter` | Inter, 2,937 glyphs, 131,498 points | 20 | 6.860 ms | 7.169 ms |
| `autohint_sweep_stix` | STIX, 5,543 glyphs, 211,893 points | 10 | 15.516 ms | 15.940 ms |

---

## 4. Hinting

`--test type` · `hint_latency` · 3 tests

| test | scope | n | min | median |
|---|---|---|---|---|
| `hint_glyph_bytecode` | 5 hinted glyphs at 16 ppem | 40,000 | 792 ns | 917 ns |
| `hint_glyph_context_cached` | 5 glyphs through `FontCache` | 40,000 | 833 ns | 917 ns |
| `hint_glyph_context_per_glyph` | 5 glyphs, `HintContext` rebuilt each time | 4,000 | 3.375 µs | 3.625 µs |

---

## 5. Colour

`--test type` · `colr_latency` · 3 tests

| test | scope | n | min | median |
|---|---|---|---|---|
| `colr_v1_paint_graph_static` | 200 base glyphs, whole sweep | 2,000 | 35.333 µs | 35.792 µs |
| `cache_colr_variable` | 200 base glyphs through `FontCache` | 2,000 | 39.792 µs | 40.625 µs |
| `colr_v1_paint_graph_variable` | 200 base glyphs, whole sweep | 2,000 | 621.708 µs | 649.125 µs |

---

## 6. Rasterizing, CPU

`--test cpu` · 3 tests

### `cpu_pipeline_by_glyph` – 16px, grayscale, no gamma

| glyph | pen ops | bitmap | raster | flatten | alloc | draw | resolve |
|---|---|---|---|---|---|---|---|
| `.` | 10 | 3×3 | 0.333 µs | 50.2% | 12.6% | 24.9% | 12.3% |
| `l` | 34 | 4×13 | 0.750 µs | 55.6% | 5.6% | 27.7% | 11.1% |
| `W` | 122 | 15×12 | 2.166 µs | 50.0% | 1.9% | 36.6% | 11.5% |
| `o` | 31 | 8×8 | 3.083 µs | 83.8% | 1.4% | 12.2% | 2.7% |
| `B` | 71 | 9×12 | 4.999 µs | 85.9% | 0.8% | 10.0% | 3.3% |
| `@` | 86 | 12×12 | 8.375 µs | 87.6% | 0.5% | 9.5% | 2.5% |
| `g` | 76 | 8×12 | 44.166 µs | 98.3% | 0.1% | 1.3% | 0.3% |

### `cpu_pipeline_by_size` – EB Garamond `B`, 71 pen ops, grayscale, no gamma

| size | bitmap | raster | flatten | alloc | draw | resolve |
|---|---|---|---|---|---|---|
| 12px | 7×9 | 4.415 µs | 87.7% | 0.9% | 9.4% | 1.9% |
| 16px | 9×12 | 4.791 µs | 86.1% | 0.9% | 10.4% | 2.6% |
| 24px | 14×17 | 5.709 µs | 81.7% | 0.7% | 11.7% | 5.8% |
| 32px | 18×23 | 6.710 µs | 78.9% | 0.6% | 12.4% | 8.1% |
| 64px | 35×44 | 8.959 µs | 61.4% | 0.9% | 15.3% | 22.3% |
| 128px | 69×86 | 16.793 µs | 38.7% | 1.5% | 14.6% | 45.2% |
| 256px | 137×171 | 45.416 µs | 21.1% | 3.1% | 9.7% | 66.1% |

### `cpu_pipeline_by_layout` – EB Garamond `B`

| layout | size | bitmap | raster | flatten | alloc | draw | resolve |
|---|---|---|---|---|---|---|---|
| grayscale | 16px | 9×12 | 4.874 µs | 85.5% | 0.8% | 10.3% | 3.4% |
| grayscale + gamma | 16px | 9×12 | 4.916 µs | 85.6% | 0.8% | 10.2% | 3.4% |
| subpixel RGB | 16px | 11×12 | 6.375 µs | 64.7% | 0.7% | 11.8% | 22.9% |
| subpixel RGB + gamma | 16px | 11×12 | 6.417 µs | 64.3% | 0.7% | 11.7% | 23.4% |
| grayscale | 64px | 35×44 | 8.917 µs | 61.2% | 0.9% | 15.4% | 22.4% |
| subpixel RGB | 64px | 37×44 | 26.792 µs | 20.5% | 0.9% | 9.3% | 69.2% |

---

## 7. Rasterizing, GPU

`--test gpu` · 2 tests · empty draw, median of 200, stages cumulative

### `draw_cost_by_stage`

| backend | target | submit | +wait | +read | readback |
|---|---|---|---|---|---|
| Metal | 64×64 | 79.2 µs | 293.8 µs | 524.9 µs | 231.1 µs |
| Metal | 256×256 | 82.2 µs | 313.2 µs | 549.0 µs | 235.8 µs |
| Metal | 512×512 | 9.4 µs | 286.8 µs | 540.2 µs | 253.5 µs |
| Metal | 1024×1024 | 9.3 µs | 295.2 µs | 590.2 µs | 295.0 µs |
| Metal | 2048×1024 | 9.0 µs | 301.7 µs | 627.8 µs | 326.1 µs |
| Vulkan | 64×64 | 103.3 µs | 357.9 µs | 716.2 µs | 358.3 µs |
| Vulkan | 256×256 | 139.4 µs | 362.8 µs | 699.0 µs | 336.2 µs |
| Vulkan | 512×512 | 113.9 µs | 371.0 µs | 687.7 µs | 316.7 µs |
| Vulkan | 1024×1024 | 129.0 µs | 359.9 µs | 720.5 µs | 360.6 µs |
| Vulkan | 2048×1024 | 151.4 µs | 384.8 µs | 807.3 µs | 422.5 µs |

Fitted:

| backend | stage | fixed | per 1000 px |
|---|---|---|---|
| Metal | submit | 58.8 µs | −0.030 µs |
| Metal | +wait | 297.6 µs | 0.001 µs |
| Metal | +read | 534.3 µs | 0.046 µs |
| Vulkan | submit | 116.6 µs | 0.016 µs |
| Vulkan | +wait | 360.7 µs | 0.009 µs |
| Vulkan | +read | 692.5 µs | 0.048 µs |

Submit carries the first-call cost of the run, which is why its fit slopes the wrong way. Wait and
read are the stable ones.

### `a_read_is_never_cheaper_than_the_wait_inside_it`

| backend | wait | read |
|---|---|---|
| Metal | 288.2 µs | 547.6 µs |
| Vulkan | 371.4 µs | 648.2 µs |

---

## 8. Maths

`--test machine` · 9 tests · daegun is `no_std` and carries its own float maths

### `float_ext_against_std` – ratio > 1 means daegun is slower

| | daegun | std | ratio |
|---|---|---|---|
| `f64 abs` | 0.244 ns | 0.244 ns | 1.00× |
| `f32 abs` | 0.244 ns | 0.244 ns | 1.00× |
| `f32 round_ties_even` | 0.386 ns | 0.244 ns | 1.58× |
| `f32 floor` | 0.458 ns | 0.254 ns | 1.80× |
| `f64 round_ties_even` | 0.560 ns | 0.244 ns | 2.29× |
| `f64 trunc` | 0.641 ns | 0.244 ns | 2.62× |
| `f32 round` | 0.651 ns | 0.244 ns | 2.67× |
| `f64 ceil` | 0.682 ns | 0.244 ns | 2.79× |
| `f64 floor` | 0.956 ns | 0.244 ns | 3.92× |
| `f64 round` | 1.455 ns | 0.244 ns | 5.96× |

### `sqrt_against_std` – Newton iteration against one hardware instruction

| | daegun | std | ratio |
|---|---|---|---|
| `f32 sqrt` | 2.411 ns | 0.295 ns | 8.17× |
| `f64 sqrt` | 2.787 ns | 0.325 ns | 8.56× |

### `trig_against_std`

| | daegun | std | ratio |
|---|---|---|---|
| `f64 sin_cos` | 3.438 ns | 7.426 ns | 0.46× |
| `f64 atan2` | 8.168 ns | 8.575 ns | 0.95× |

### `rounding_old_against_new` – ratio < 1 means the new one is faster

| | new | old | ratio |
|---|---|---|---|
| `round_ties_even` | 0.570 ns | 2.014 ns | 0.28× |
| `ceil` | 0.682 ns | 1.597 ns | 0.43× |
| `floor` | 1.017 ns | 1.068 ns | 0.95× |
| `trunc` | 0.641 ns | 0.641 ns | 1.00× |
| `round` | 1.445 ns | 1.445 ns | 1.00× |

### `atan2_cost_breakdown`

| | first | second | ratio |
|---|---|---|---|
| atan2 vs one divide | 7.568 ns | 0.356 ns | 21.26× |
| atan2 vs horner20 | 7.782 ns | 3.499 ns | 2.22× |
| horner20 vs one divide | 3.499 ns | 0.356 ns | 9.83× |
| estrin20 vs horner20 | 2.289 ns | 3.499 ns | 0.65× |

### `atan_polynomial_shape` – 11 coefficients, identical inputs

| | first | second | ratio |
|---|---|---|---|
| estrin4 vs current | 1.394 ns | 1.343 ns | 1.04× |

### `atan_reduction_shape` – ratio < 1 means the first named is faster

| | first | second | ratio |
|---|---|---|---|
| hybrid, one arm | 2.930 ns | 4.161 ns | 0.70× |
| branchless, scattered | 5.442 ns | 6.032 ns | 0.90× |
| branchless, raster sweep | 5.117 ns | 5.524 ns | 0.93× |
| hybrid, raster sweep | 5.422 ns | 5.697 ns | 0.95× |
| hybrid, mid range | 6.073 ns | 6.226 ns | 0.98× |
| hybrid, scattered | 6.012 ns | 5.859 ns | 1.03× |
| branchless, one arm | 5.300 ns | 4.191 ns | 1.26× |

### `sqrt_iteration_shape`

| | first | second | ratio |
|---|---|---|---|
| rsqrt 4 vs current | 2.879 ns | 2.787 ns | 1.03× |
| rsqrt 5 vs current | 3.530 ns | 2.706 ns | 1.30× |
| current vs std sqrt | 2.716 ns | 0.305 ns | 8.90× |

Accuracy against std, worst ulp over 60,000 values in [1e-8, 1e8]:

| | worst ulp |
|---|---|
| newton on root, 4 passes | 1 |
| newton on reciprocal, 5 passes | 2 |
| newton on reciprocal, 4 passes | 3 |

### `daemath_baseline` – blending, per pixel, anchored on SrcOver

| | ns/px | anchor | ratio |
|---|---|---|---|
| blend Multiply | 2.004 | 2.309 | 0.87× |
| blend HardLight | 5.229 | 2.309 | 2.26× |
| blend HslSaturation | 8.392 | 2.340 | 3.59× |
| composite SrcOver | 6.022 | 1.068 | 5.64× |

### `daemath_baseline` – gradients, per pixel, anchored on linear

| | ns/px | anchor | ratio |
|---|---|---|---|
| gradient linear | 17.883 | anchor | 1.00× |
| gradient radial | 23.387 | 17.883 | 1.31× |
| gradient sweep | 32.695 | 17.070 | 1.92× |

---

## 9. The C ABI

`src/c-wrapper/tests/latency.c` · 200 rounds after 50 warmup · both sides on a release build

| | Rust min | C min | Rust median | C median |
|---|---|---|---|---|
| `from_bytes`, borrowed | 16.17 µs | 15.00 µs | 16.29 µs | 17.00 µs |
| `from_vec`, owned | 1.13 µs | 1.00 µs | 1.29 µs | 2.00 µs |
| `rasterize_glyph`, uncached | 4.91 µs | 4.87 µs | 4.95 µs | 4.90 µs |
| `outline_glyph` | 250.7 ns | 272.0 ns | 250.9 ns | 276.0 ns |
| `rasterize_glyph`, cached | 73.4 ns | 110.0 ns | 74.8 ns | 112.0 ns |
| `glyph_id` | 63.6 ns | 98.0 ns | 63.8 ns | 108.0 ns |
| `advance_widths` ×1 | 26.0 ns | 74.0 ns | 26.2 ns | 74.0 ns |
| `upm` | 0.2 ns | 0.0 ns | 0.3 ns | 2.0 ns |

The C column comes off `clock_gettime` over batches of 500, which quantises it to 2 ns steps. Read
the cheap rows as an upper bound rather than a figure.

---

## About these numbers

They are latency tests living beside the ordinary ones, marked `#[ignore]` because they measure
rather than assert. The gate runs what can fail; these report, and are run by hand.

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

For counters rather than wall clock, `scripts/tools/perf/pmu.sh` drives Instruments, and
`insn-diff.py` and `pmu-attribute.py` beside it attribute the difference between two builds down to
the function. `tasks/baselines/` holds captured before-and-after studies from past optimization work.
