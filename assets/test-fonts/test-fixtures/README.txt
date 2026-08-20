Test-only fixtures, mechanically derived from the already-licensed fonts
vendored elsewhere in scripts/tests/fonts/ — same font data, different container
format, no new content and no separate license needed.

EBGaramond-InterVariable.ttc      fontTools TTCollection merge of
                                   ../inter/InterVariable.ttf and
                                   ../eb-garamond/EBGaramond.ttf

Regenerate via:

python3 -c "
from fontTools.ttLib import TTFont, TTCollection
f1 = TTFont('scripts/tests/fonts/eb-garamond/EBGaramond.ttf')
f2 = TTFont('scripts/tests/fonts/inter/InterVariable.ttf')
tc = TTCollection()
tc.fonts = [f1, f2]
tc.save('scripts/tests/fonts/test-fixtures/EBGaramond-InterVariable.ttc')
"

Used by: daetype's decoder/ttc_tests.rs.

---

test_glyphs_variable_no_cycle_test_glyphs.ttf   ../colr-v1-test-glyphs/
                                                  test_glyphs_variable.ttf,
                                                  minus 2 of 200 base glyphs.

Same Apache-2.0 license as ../colr-v1-test-glyphs/LICENSE.txt — content
removed, nothing added. The original's COLR table has two deliberately
cyclic PaintColrGlyph entries named "paintcolrglyph_cycle_first"/
"paintcolrglyph_cycle_second" (gids 178/179) — almost certainly an
intentional adversarial case from whatever COLR v1 conformance suite this
was sourced from, to test that renderers correctly reject cycles. Real
sanitizers (confirmed via Google's own `ots-sanitize`, same code Chromium
runs) refuse the entire COLR table over it, taking down every other glyph
in the file along with it — including gid 116 ("translate_100_0"), which
is genuinely valid and has zero relation to the two cycle-test glyphs
(confirmed: nothing else in the paint graph references them). Regenerated
by dropping just those two BaseGlyphPaintRecord entries and re-saving,
verified clean via `ots-sanitize` afterward.

Regenerate via:

python3 -c "
from fontTools.ttLib import TTFont
f = TTFont('scripts/tests/fonts/colr-v1-test-glyphs/test_glyphs_variable.ttf')
bgl = f['COLR'].table.BaseGlyphList
bgl.BaseGlyphPaintRecord = [r for r in bgl.BaseGlyphPaintRecord
    if r.BaseGlyph not in ('paintcolrglyph_cycle_first', 'paintcolrglyph_cycle_second')]
f.save('scripts/tests/fonts/test-fixtures/test_glyphs_variable_no_cycle_test_glyphs.ttf')
"

Used by: scripts/gen-images/src/main.rs's gen_colr_v1() (05-colr-v1 demo).

---

carets.ttf and hinted.ttf are neither vendored nor derived -- they are
synthetic, built from nothing by daegun's own scripts, and carry no
third-party content at all. Both are MIT with the rest of the project.

carets.ttf        scripts/tools/oracles/diff/build_caret_fixture.py
                  Family name "DaegunCarets". A GDEF LigCaretList with
                  known caret positions, so caret geometry can be asserted
                  against numbers the fixture itself defines rather than
                  against whatever a real font happens to contain.
                  Used by: src/daegun/tests/api/autohint.rs.

hinted.ttf        scripts/tools/oracles/diff/build_hinted_fixture.py
                  Family name "DaegunHinted". Carries real TrueType
                  bytecode (fpgm/prep/glyf instructions) so the interpreter
                  is graded on instructions written for it, at sizes chosen
                  to make the v35/v40 difference visible.

                  Glyphs A-D aim one opcode family each: A rounds a stem
                  horizontally (so v35 and v40 differ on it), B branches on
                  ppem through IF/ELSE/EIF, C jumps backward via LOOPCALL,
                  D carries no instructions at all as the control.

                  E and F are a pair, and only mean anything together. E
                  redefines fn 0 inside its own glyph program as MDAP[0] --
                  touch without rounding, the opposite of fpgm's fn 0. F
                  carries A's program byte for byte over A's box. So F must
                  hint identically to A whether or not E ran first, and a
                  glyph-level FDEF that outlives its glyph makes F quietly
                  stop rounding. That was a real bug, and nothing caught it
                  because every FDEF here used to live in fpgm, which is
                  where they belong and where none of them leak.
                  Used by: src/daecore/tests/type/fdef_scope.rs,
                  src/daecore/tests/type/hint_latency.rs,
                  src/daecore/tests/fingerprint/main.rs.

Regenerate either by running its script; both take an output path.

---

scripts/tests/fonts/Irianisadfstd/*.ttc are NOT derived fixtures -- they're real,
independently-sourced TTC files (Arkandis Digital Foundry, Irianis ADF,
GPL v2+ with font exception -- free/redistributable, same freedom class as
the other bundled fonts) used as-is, referenced directly from
scripts/tests/fonts/Irianisadfstd/ rather than copied here. Each is a genuine
2-font collection where both members are STATIC (no fvar at all) and share
identical glyf data under two different family names -- the real-world case
EBGaramond-InterVariable.ttc above can't cover, since its own EBGaramond
member is a variable build, not the static standalone EBGaramond.ttf. Used
by: daetype's decoder/ttc_tests.rs (extraction-level) and the font-cache tests
(register_font_ttc end-to-end, real nearest-weight resolution between
IrianisadfstdRegular.ttc weight 400 and IrianisadfstdBold.ttc weight 700).
