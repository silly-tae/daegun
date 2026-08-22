#include "daegun.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__APPLE__)
#include <objc/message.h>
#include <objc/runtime.h>

/* Metal.framework's own C entry point, so adopting a device needs no Objective-C of our own. It
   comes back retained, hence the release once daegun has taken its own reference. */
extern void *MTLCreateSystemDefaultDevice(void);
#endif

static int failures = 0;

#define CHECK(cond, ...)                                                                           \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            fprintf(stderr, "  FAIL %s:%d: ", __FILE__, __LINE__);                                 \
            fprintf(stderr, __VA_ARGS__);                                                          \
            fprintf(stderr, "\n");                                                                 \
            failures++;                                                                            \
        }                                                                                          \
    } while (0)

static uint8_t *slurp(const char *path, size_t *len)
{
    FILE *f = fopen(path, "rb");
    if (!f) {
        return NULL;
    }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (n <= 0) {
        fclose(f);
        return NULL;
    }
    uint8_t *buf = malloc((size_t)n);
    if (!buf) {
        fclose(f);
        return NULL;
    }
    size_t got = fread(buf, 1, (size_t)n, f);
    fclose(f);
    if (got != (size_t)n) {
        free(buf);
        return NULL;
    }
    *len = got;
    return buf;
}

static void abi_version_agrees(void)
{
    uint32_t got = daegun_abi_version();
    CHECK(got == DAEGUN_ABI_VERSION,
          "library reports ABI %u.%u.%u, this header is %u.%u.%u", (got >> 16) & 0xffu,
          (got >> 8) & 0xffu, got & 0xffu, (DAEGUN_ABI_VERSION >> 16) & 0xffu,
          (DAEGUN_ABI_VERSION >> 8) & 0xffu, DAEGUN_ABI_VERSION & 0xffu);
}

/* Rule 2: NULL is an answer, never a crash. This is the test that a sanitizer cannot write for us –
 * it has to be attempted deliberately, because no correct program does it. */
static void null_is_refused_not_dereferenced(void)
{
    daegun_font *font = NULL;
    uint16_t out = 0;

    CHECK(daegun_font_open(NULL, 0, &font) == DAEGUN_NULL, "null data was not refused");
    CHECK(daegun_font_glyph_id(NULL, 'A', &out) == DAEGUN_NULL, "null font was not refused");
    CHECK(daegun_font_num_glyphs(NULL, &out) == DAEGUN_NULL, "null font was not refused");
    CHECK(daegun_font_upm(NULL, &out) == DAEGUN_NULL, "null font was not refused");

    daegun_font_free(NULL);
}

static void bad_font_data_is_reported(void)
{
    uint8_t junk[64];
    memset(junk, 0xab, sizeof junk);
    daegun_font *font = NULL;

    CHECK(daegun_font_open(junk, sizeof junk, &font) == DAEGUN_PARSE,
          "64 bytes of 0xab parsed as a font");
    CHECK(font == NULL, "a failed open still wrote a handle");

    daegun_str err = daegun_last_error();
    CHECK(err.len > 0, "a failed parse left no message");
    CHECK(err.data != NULL && strlen(err.data) == err.len,
          "the message length disagrees with strlen, so rule 4's NUL promise is broken");
}

static daegun_font *open_font(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) {
        CHECK(0, "could not read %s", path);
        return NULL;
    }
    daegun_font *font = NULL;
    daegun_status st = daegun_font_open(bytes, len, &font);
    free(bytes);
    if (st != DAEGUN_OK) {
        CHECK(0, "opening %s returned %d: %s", path, st, daegun_last_error().data);
        return NULL;
    }
    return font;
}

static void a_real_font_answers(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) {
        CHECK(0, "could not read %s, so the only test that opens a real font did not run", path);
        return;
    }

    daegun_font *font = NULL;
    daegun_status st = daegun_font_open(bytes, len, &font);
    CHECK(st == DAEGUN_OK, "opening %s returned %d: %s", path, st, daegun_last_error().data);
    if (st != DAEGUN_OK) {
        free(bytes);
        return;
    }

    free(bytes);

    uint16_t upm = 0, glyphs = 0, gid = 0;
    CHECK(daegun_font_upm(font, &upm) == DAEGUN_OK, "upm failed");
    CHECK(upm > 0, "upm is zero");
    CHECK(daegun_font_num_glyphs(font, &glyphs) == DAEGUN_OK, "num_glyphs failed");
    CHECK(glyphs > 1, "a face with %u glyphs is not a face", glyphs);

    CHECK(daegun_font_glyph_id(font, 'A', &gid) == DAEGUN_OK, "no glyph for 'A'");
    CHECK(gid != 0, "'A' mapped to .notdef");
    CHECK(gid < glyphs, "glyph id %u is past the %u the face has", gid, glyphs);

    uint16_t none = 0xffff;
    daegun_status absent = daegun_font_glyph_id(font, 0x10FFFD, &none);
    CHECK(absent == DAEGUN_ABSENT || absent == DAEGUN_OK,
          "an unmapped codepoint returned %d", absent);
    if (absent == DAEGUN_ABSENT) {
        CHECK(none == 0xffff, "an ABSENT answer still wrote the out-parameter");
    }

    size_t count = 0, cbytes = 0;
    CHECK(daegun_font_glyph_cache_stats(font, &count, &cbytes) == DAEGUN_OK, "cache stats failed");
    CHECK(daegun_font_set_glyph_cache_bytes(font, 0) == DAEGUN_OK, "disabling the cache failed");
    CHECK(daegun_font_clear_glyph_cache(font) == DAEGUN_OK, "clearing the cache failed");
    CHECK(daegun_font_glyph_cache_stats(font, &count, NULL) == DAEGUN_OK,
          "cache stats with one NULL out-parameter failed");
    CHECK(count == 0, "the cache holds %zu glyphs after being cleared and bounded to zero", count);

    /* The other five budgets, through the same door. */
    CHECK(daegun_font_set_curve_cache_bytes(font, 64 * 1024) == DAEGUN_OK, "curve budget failed");
    CHECK(daegun_font_clear_curve_cache(font) == DAEGUN_OK, "clearing curves failed");
    CHECK(daegun_font_curve_cache_stats(font, &count, &cbytes) == DAEGUN_OK, "curve stats failed");
    CHECK(count == 0 && cbytes == 0, "curve cache held %zu entries after clearing", count);

    CHECK(daegun_font_set_outline_cache_bytes(font, 64 * 1024) == DAEGUN_OK, "outline budget failed");
    CHECK(daegun_font_outline_cache_stats(font, &count, NULL) == DAEGUN_OK, "outline stats failed");
    CHECK(count == 0, "outline cache held %zu entries after clearing", count);

    CHECK(daegun_font_set_shape_cache_bytes(font, 32 * 1024) == DAEGUN_OK, "shape budget failed");
    CHECK(daegun_font_clear_shape_cache(font) == DAEGUN_OK, "clearing shapes failed");
    CHECK(daegun_font_shape_cache_stats(font, &count, &cbytes) == DAEGUN_OK, "shape stats failed");
    CHECK(count == 0 && cbytes == 0, "shape cache held %zu entries after clearing", count);

    size_t locations = 1, tables = 1;
    CHECK(daegun_font_set_instance_cache_bytes(font, 1024 * 1024) == DAEGUN_OK, "instance budget failed");
    CHECK(daegun_font_instance_cache_stats(font, &locations, &tables) == DAEGUN_OK, "instance stats failed");

    size_t allowance = 0;
    CHECK(daegun_font_set_cmap_index_allowance(font, 4321) == DAEGUN_OK, "index allowance failed");
    CHECK(daegun_font_cmap_index_allowance(font, &allowance) == DAEGUN_OK, "reading allowance failed");
    CHECK(allowance == 4321, "allowance read back as %zu", allowance);

    /* Every one of them has to refuse a null font rather than dereference it. */
    CHECK(daegun_font_set_curve_cache_bytes(NULL, 0) == DAEGUN_NULL, "null curve budget accepted");
    CHECK(daegun_font_set_outline_cache_bytes(NULL, 0) == DAEGUN_NULL, "null outline budget accepted");
    CHECK(daegun_font_set_shape_cache_bytes(NULL, 0) == DAEGUN_NULL, "null shape budget accepted");
    CHECK(daegun_font_set_instance_cache_bytes(NULL, 0) == DAEGUN_NULL, "null instance budget accepted");
    CHECK(daegun_font_set_cmap_index_allowance(NULL, 0) == DAEGUN_NULL, "null allowance accepted");
    CHECK(daegun_font_cmap_index_allowance(NULL, &allowance) == DAEGUN_NULL, "null allowance read accepted");

    daegun_font_free(font);
}

static void zeroed_options_are_the_defaults(void)
{
    daegun_raster_options zeroed;
    memset(&zeroed, 0, sizeof zeroed);

    daegun_raster_options given;
    memset(&given, 0xcd, sizeof given);
    CHECK(daegun_raster_options_default(&given) == DAEGUN_OK, "defaults failed");

    CHECK(memcmp(&zeroed, &given, sizeof zeroed) == 0,
          "memset(0) and daegun_raster_options_default disagree, so the header's promise is broken");
    CHECK(daegun_raster_options_default(NULL) == DAEGUN_NULL, "null options was not refused");
}

static void ttc_count_answers_for_a_plain_font(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) {
        CHECK(0, "could not read %s", path);
        return;
    }
    size_t n = 12345;
    CHECK(daegun_ttc_font_count(bytes, len, &n) == DAEGUN_OK, "ttc count failed");
    CHECK(n == 0, "a plain .ttf reported %zu faces", n);
    free(bytes);
}

static void metrics_answer_and_free_cleanly(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    int32_t asc = 0, desc = 0;
    CHECK(daegun_font_ascender(font, &asc) == DAEGUN_OK, "ascender failed");
    CHECK(daegun_font_descender(font, &desc) == DAEGUN_OK, "descender failed");
    CHECK(asc > desc, "ascender %d is not above descender %d", asc, desc);

    daegun_text *family = NULL;
    if (daegun_font_family_name(font, &family) == DAEGUN_OK) {
        daegun_str v = { NULL, 0 };
        CHECK(daegun_text_str(family, &v) == DAEGUN_OK, "text_str failed");
        CHECK(v.len > 0 && v.data != NULL, "family name came back empty");
        CHECK(strlen(v.data) == v.len, "family name's NUL disagrees with its length");
        daegun_text_free(family);
    }

    daegun_i32_list *bbox = NULL;
    CHECK(daegun_font_bbox(font, &bbox) == DAEGUN_OK, "bbox failed");
    size_t n = 0;
    const int32_t *box = daegun_i32_list_data(bbox, &n);
    CHECK(n == 4, "a bounding box of %zu numbers", n);
    if (n == 4) {
        CHECK(box[0] < box[2] && box[1] < box[3], "the bounding box is inside out");
    }
    daegun_i32_list_free(bbox);

    daegun_u16_list *ids = NULL;
    daegun_str_list *strings = NULL;
    CHECK(daegun_font_names(font, &ids, &strings) == DAEGUN_OK, "names failed");
    size_t id_count = 0, str_count = 0;
    daegun_u16_list_data(ids, &id_count);
    CHECK(daegun_str_list_count(strings, &str_count) == DAEGUN_OK, "str count failed");
    CHECK(id_count == str_count, "%zu name ids against %zu strings", id_count, str_count);
    if (str_count > 0) {
        daegun_str first = { NULL, 0 };
        CHECK(daegun_str_list_at(strings, 0, &first) == DAEGUN_OK, "str_list_at(0) failed");
        daegun_str past = { NULL, 0 };
        CHECK(daegun_str_list_at(strings, str_count, &past) == DAEGUN_RANGE,
              "reading past the end was not DAEGUN_RANGE");
    }
    daegun_u16_list_free(ids);
    daegun_str_list_free(strings);

    bool italic = false;
    daegun_status st = daegun_font_is_italic(font, &italic);
    CHECK(st == DAEGUN_OK || st == DAEGUN_ABSENT, "is_italic returned %d", st);

    bool variable = false;
    CHECK(daegun_font_is_variable(font, &variable) == DAEGUN_OK, "is_variable failed");
    daegun_str_list *tags = NULL;
    daegun_f64_list *ranges = NULL;
    CHECK(daegun_font_axes(font, &tags, &ranges) == DAEGUN_OK, "axes failed");
    size_t tag_count = 0, range_count = 0;
    CHECK(daegun_str_list_count(tags, &tag_count) == DAEGUN_OK, "axis tag count failed");
    daegun_f64_list_data(ranges, &range_count);
    CHECK(range_count == tag_count * 3, "%zu axes but %zu range numbers", tag_count, range_count);
    CHECK(!variable || tag_count > 0, "a variable face declared no axes");
    daegun_str_list_free(tags);
    daegun_f64_list_free(ranges);

    daegun_font_free(font);
}

static void a_subset_is_a_font(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    uint16_t gid = 0;
    if (daegun_font_glyph_id(font, 'A', &gid) != DAEGUN_OK) { daegun_font_free(font); return; }

    uint16_t wanted[2] = { 0, gid };
    daegun_subset *sub = NULL;
    daegun_status st = daegun_font_subset(font, wanted, 2, NULL, 0, &sub);
    CHECK(st == DAEGUN_OK, "subset returned %d: %s", st, daegun_last_error().data);
    if (st == DAEGUN_OK) {
        size_t ttf_len = 0, map_len = 0;
        const uint8_t *ttf = daegun_subset_ttf(sub, &ttf_len);
        daegun_subset_gid_map(sub, &map_len);
        CHECK(ttf_len > 0 && ttf != NULL, "the subset is empty");

        daegun_font *reopened = NULL;
        CHECK(daegun_font_open(ttf, ttf_len, &reopened) == DAEGUN_OK,
              "the subset does not parse as a font: %s", daegun_last_error().data);
        daegun_font_free(reopened);
        daegun_subset_free(sub);
    }
    daegun_font_free(font);
}

static void math_constants_are_indexed(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read the math font at %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    int32_t count = daegun_math_constant_count();
    CHECK(count == 56, "the ABI knows %d math constants", count);

    double v = 0.0;
    daegun_status st = daegun_font_math_constant(font, DAEGUN_MATH_AXIS_HEIGHT, &v);
    CHECK(st == DAEGUN_OK, "axis height returned %d", st);
    CHECK(v != 0.0, "a math font reports an axis height of zero");

    CHECK(daegun_font_math_constant(font, count, &v) == DAEGUN_RANGE,
          "an out-of-range constant index was not refused");

    daegun_font_free(font);
}

struct pen_state {
    int moves, lines, quads, curves, closes;
    const daegun_font *font;
    int reentered_ok;
};

static void on_move(void *u, float x, float y)  { (void)x; (void)y; ((struct pen_state *)u)->moves++; }
static void on_line(void *u, float x, float y)  { (void)x; (void)y; ((struct pen_state *)u)->lines++; }
static void on_quad(void *u, float a, float b, float x, float y)
{ (void)a; (void)b; (void)x; (void)y; ((struct pen_state *)u)->quads++; }
static void on_curve(void *u, float a, float b, float c, float d, float x, float y)
{ (void)a; (void)b; (void)c; (void)d; (void)x; (void)y; ((struct pen_state *)u)->curves++; }

static void on_close(void *u)
{
    struct pen_state *st = u;
    st->closes++;
    uint16_t n = 0;
    if (daegun_font_num_glyphs(st->font, &n) == DAEGUN_OK && n > 0) {
        st->reentered_ok = 1;
    }
}

static void the_pen_draws_and_reentry_is_safe(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    uint16_t gid = 0;
    if (daegun_font_glyph_id(font, 'B', &gid) != DAEGUN_OK) { daegun_font_free(font); return; }

    struct pen_state st;
    memset(&st, 0, sizeof st);
    st.font = font;

    daegun_pen pen;
    pen.move_to = on_move;
    pen.line_to = on_line;
    pen.quad_to = on_quad;
    pen.curve_to = on_curve;
    pen.close = on_close;
    pen.user = &st;

    CHECK(daegun_font_outline_glyph(font, gid, &pen) == DAEGUN_OK, "outline failed");
    CHECK(st.moves > 0, "'B' produced no move_to");
    CHECK(st.closes >= st.moves, "%d contours opened but %d closed", st.moves, st.closes);
    CHECK(st.lines + st.quads + st.curves > 0, "'B' produced no segments at all");
    CHECK(st.reentered_ok, "calling back into daegun from a pen callback did not work");

    daegun_pen empty;
    memset(&empty, 0, sizeof empty);
    CHECK(daegun_font_outline_glyph(font, gid, &empty) == DAEGUN_OK,
          "a pen with no callbacks was not accepted");

    daegun_font_free(font);
}

static void rasterizing_produces_ink(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    uint16_t gid = 0;
    if (daegun_font_glyph_id(font, 'B', &gid) != DAEGUN_OK) { daegun_font_free(font); return; }

    daegun_bitmap *bmp = NULL;
    daegun_status st = daegun_font_rasterize_glyph(font, gid, 32.0f, NULL, 0, &bmp);
    CHECK(st == DAEGUN_OK, "rasterize returned %d", st);
    if (st == DAEGUN_OK) {
        daegun_metrics m;
        CHECK(daegun_bitmap_metrics(bmp, &m) == DAEGUN_OK, "metrics failed");
        CHECK(m.width > 0 && m.height > 0, "a %zux%zu bitmap", m.width, m.height);

        size_t n = 0;
        const uint8_t *px = daegun_bitmap_pixels(bmp, &n);
        CHECK(n == m.width * m.height, "grayscale gave %zu bytes for %zux%zu",
              n, m.width, m.height);

        int inked = 0;
        for (size_t i = 0; i < n; i++) { if (px[i] > 0) inked++; }
        CHECK(inked > 0, "'B' rasterized to %zu blank pixels", n);
        daegun_bitmap_free(bmp);
    }

    daegun_raster_options opts;
    daegun_raster_options_default(&opts);
    opts.layout = DAEGUN_LAYOUT_RGB_H;
    daegun_bitmap *sub = NULL;
    if (daegun_font_rasterize_glyph_with(font, gid, 32.0f, NULL, 0, &opts, &sub) == DAEGUN_OK) {
        daegun_metrics m;
        daegun_bitmap_metrics(sub, &m);
        size_t n = 0;
        daegun_bitmap_pixels(sub, &n);
        CHECK(n == m.width * m.height * 3, "a subpixel layout gave %zu bytes for %zux%zu",
              n, m.width, m.height);
        daegun_bitmap_free(sub);
    }

    daegun_font_free(font);
}

static void shaping_produces_positioned_glyphs(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    daegun_run *run = NULL;
    daegun_status st = daegun_font_shape(font, "Waffle", NULL, 0, false, &run);
    CHECK(st == DAEGUN_OK, "shape returned %d", st);
    if (st == DAEGUN_OK) {
        size_t g = 0, a = 0, o = 0, c = 0;
        const uint16_t *glyphs = daegun_run_glyphs(run, &g);
        daegun_run_advances(run, &a);
        daegun_run_offsets(run, &o);
        daegun_run_clusters(run, &c);

        CHECK(g > 0, "shaping 'Waffle' produced no glyphs");
        CHECK(a == g, "%zu glyphs but %zu advances", g, a);
        CHECK(c == g, "%zu glyphs but %zu clusters", g, c);
        CHECK(o == g * 2, "%zu glyphs but %zu offset doubles", g, o);

        int all_notdef = 1;
        for (size_t i = 0; i < g; i++) { if (glyphs[i] != 0) all_notdef = 0; }
        CHECK(!all_notdef, "every glyph of 'Waffle' came back .notdef");

        bool complete = false;
        CHECK(daegun_run_complete(run, &complete) == DAEGUN_OK, "complete failed");
        CHECK(complete, "shaping plain Latin did not complete");

        daegun_str shaper = { NULL, 0 };
        CHECK(daegun_run_shaper(run, &shaper) == DAEGUN_OK, "shaper failed");
        CHECK(shaper.len > 0, "the run does not say which shaper ran");
        daegun_run_free(run);
    }

    daegun_run *lig = NULL;
    if (daegun_font_shape(font, "ffl", NULL, 0, false, &lig) == DAEGUN_OK) {
        size_t n = 0;
        daegun_run_glyphs(lig, &n);
        CHECK(n >= 1 && n <= 3, "'ffl' shaped to %zu glyphs", n);
        daegun_run_free(lig);
    }

    double w = 0.0;
    CHECK(daegun_font_measure_width(font, "Waffle", NULL, 0, 16.0, &w) == DAEGUN_OK,
          "measure_width failed");
    CHECK(w > 0.0, "'Waffle' measured %f wide", w);

    daegun_font_free(font);
}

/* Layout, and the borrowed-run rule: a run inside a layout must not be freed separately. */
static void layout_wraps_and_borrows(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    daegun_layout_options opts;
    CHECK(daegun_layout_options_default(&opts) == DAEGUN_OK, "layout defaults failed");
    /* The default must not wrap. If zeroing were the default this would be zero and wrap at every
     * glyph, which is exactly what the header warns about. */
    CHECK(opts.max_inline_size > 1.0e9, "the default max_inline_size is %f, so it would wrap",
          opts.max_inline_size);
    opts.max_inline_size = 6000.0;

    daegun_layout *layout = NULL;
    daegun_status st = daegun_font_layout(font, "the quick brown fox jumps over the lazy dog",
                                          NULL, 0, &opts, &layout);
    CHECK(st == DAEGUN_OK, "layout returned %d", st);
    if (st == DAEGUN_OK) {
        size_t lines = 0;
        double inline_size = 0.0;
        CHECK(daegun_layout_info(layout, &lines, &inline_size, NULL, NULL, NULL) == DAEGUN_OK,
              "layout info failed");
        CHECK(lines > 1, "a 43-character string at 6000 units wrapped to %zu line(s)", lines);
        /* NOT `inline_size <= max`: a single unbreakable run may exceed the measure, which is
         * documented behavior rather than a defect. The property worth asserting is that a wider
         * measure yields fewer lines. */
        CHECK(inline_size > 0.0, "the layout reports no width at all");

        daegun_layout_options wide = opts;
        wide.max_inline_size = 60000.0;
        daegun_layout *one = NULL;
        if (daegun_font_layout(font, "the quick brown fox jumps over the lazy dog", NULL, 0,
                               &wide, &one) == DAEGUN_OK) {
            size_t wide_lines = 0;
            daegun_layout_info(one, &wide_lines, NULL, NULL, NULL, NULL);
            CHECK(wide_lines < lines, "%zu lines at 6000 units but %zu at 60000",
                  lines, wide_lines);
            daegun_layout_free(one);
        }

        size_t runs = 0;
        CHECK(daegun_layout_line(layout, 0, &runs, NULL, NULL, NULL, NULL, NULL, NULL, NULL)
                  == DAEGUN_OK, "layout line failed");
        CHECK(runs > 0, "the first line holds no runs");

        const daegun_run *borrowed = NULL;
        CHECK(daegun_layout_run(layout, 0, 0, &borrowed, NULL, NULL, NULL, NULL, NULL, NULL)
                  == DAEGUN_OK, "layout run failed");
        size_t g = 0;
        daegun_run_glyphs(borrowed, &g);
        CHECK(g > 0, "the first run of the first line holds no glyphs");
        /* Deliberately NOT daegun_run_free(borrowed) – it belongs to the layout, and freeing it
         * would be the double free the sanitizer exists to catch. */

        CHECK(daegun_layout_line(layout, lines, &runs, NULL, NULL, NULL, NULL, NULL, NULL, NULL)
                  == DAEGUN_RANGE, "a line past the end was not DAEGUN_RANGE");
        daegun_layout_free(layout);
    }
    daegun_font_free(font);
}

static void text_analysis_needs_no_font(void)
{
    daegun_u32_list *graphemes = NULL;
    CHECK(daegun_text_grapheme_boundaries("héllo", &graphemes) == DAEGUN_OK, "graphemes failed");
    size_t n = 0;
    daegun_u32_list_data(graphemes, &n);
    CHECK(n > 0, "no grapheme boundaries in 'héllo'");
    daegun_u32_list_free(graphemes);

    daegun_u32_list *runs = NULL;
    CHECK(daegun_text_script_runs("hello", &runs) == DAEGUN_OK, "script runs failed");
    const uint32_t *r = daegun_u32_list_data(runs, &n);
    CHECK(n % 3 == 0, "script runs gave %zu numbers, not a multiple of three", n);
    if (n >= 3) {
        CHECK(r[0] < r[1], "a script run from %u to %u", r[0], r[1]);
        daegun_text *name = NULL;
        CHECK(daegun_script_name((uint16_t)r[2], &name) == DAEGUN_OK, "script name failed");
        daegun_str v = { NULL, 0 };
        daegun_text_str(name, &v);
        CHECK(v.len > 0, "the script has no name");
        daegun_text_free(name);
    }
    daegun_u32_list_free(runs);

    uint8_t base = 0;
    daegun_blob *levels = NULL;
    CHECK(daegun_text_resolve_bidi("hello", -1, &base, &levels, NULL) == DAEGUN_OK,
          "resolve_bidi failed");
    daegun_blob_data(levels, &n);
    CHECK(n > 0, "no bidi levels for 'hello'");
    daegun_blob_free(levels);
}

static void the_paint_graph_is_walkable(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    uint16_t palettes = 0;
    CHECK(daegun_font_palette_count(font, &palettes) == DAEGUN_OK, "palette count failed");

    uint16_t glyphs = 0;
    daegun_font_num_glyphs(font, &glyphs);
    daegun_paint *paint = NULL;
    uint16_t found = 0;
    for (uint16_t g = 1; g < glyphs && g < 400; g++) {
        if (daegun_font_colr_v1_paint(font, g, NULL, 0, 0, &paint) == DAEGUN_OK) { found = g; break; }
    }
    if (!found) { daegun_font_free(font); return; }

    size_t n = 0, kids = 0;
    const daegun_paint_node *nodes = daegun_paint_nodes(paint, &n);
    const uint32_t *children = daegun_paint_children(paint, &kids);
    CHECK(n > 0, "glyph %u has a paint with no nodes", found);

    int visited = 0;
    for (size_t i = 0; i < n; i++) {
        const daegun_paint_node *node = &nodes[i];
        CHECK(node->kind >= 0 && node->kind <= DAEGUN_PAINT_COMPOSITE,
              "node %zu has kind %d", i, node->kind);
        CHECK((size_t)node->child_start + node->child_count <= kids,
              "node %zu names children %u..%u of %zu", i, node->child_start,
              node->child_start + node->child_count, kids);
        for (uint32_t c = 0; c < node->child_count; c++) {
            uint32_t idx = children[node->child_start + c];
            CHECK(idx < n, "node %zu has child index %u of %zu nodes", i, idx, n);
            CHECK(idx != i, "node %zu is its own child", i);
        }
        if (node->kind == DAEGUN_PAINT_COMPOSITE) {
            CHECK(node->child_count == 2, "a composite has %u children, not 2", node->child_count);
        }
        if (node->kind == DAEGUN_PAINT_SOLID) { visited++; }
    }

    size_t stops = 0;
    const double *offsets = NULL;
    const uint8_t *colors = NULL;
    CHECK(daegun_paint_stops(paint, &stops, &offsets, &colors) == DAEGUN_OK, "stops failed");
    for (size_t i = 0; i < n; i++) {
        CHECK((size_t)nodes[i].stops_start + nodes[i].stops_count <= stops,
              "node %zu names stops past the end", i);
    }
    (void)visited; (void)offsets; (void)colors;

    daegun_paint_free(paint);
    daegun_font_free(font);
}

static void drawing_picks_a_route(const char *path)
{
    size_t len = 0;
    uint8_t *bytes = slurp(path, &len);
    if (!bytes) { CHECK(0, "could not read %s", path); return; }
    daegun_font *font = NULL;
    if (daegun_font_open(bytes, len, &font) != DAEGUN_OK) { free(bytes); CHECK(0, "open failed"); return; }
    free(bytes);

    daegun_batch *batch = NULL;
    CHECK(daegun_batch_new(&batch) == DAEGUN_OK, "batch failed");

    uint16_t gid = 0;
    if (daegun_font_glyph_id(font, 'B', &gid) == DAEGUN_OK) {
        uint64_t before = 0, after = 0;
        daegun_batch_revision(batch, &before);
        daegun_glyph_slot slot;
        memset(&slot, 0, sizeof slot);
        daegun_status st = daegun_font_gpu_glyph(font, batch, gid, NULL, 0, &slot);
        CHECK(st == DAEGUN_OK, "gpu_glyph returned %d: %s", st, daegun_last_error().data);
        if (st == DAEGUN_OK) {
            daegun_batch_revision(batch, &after);
            CHECK(after != before, "uploading a glyph did not move the batch revision");
            CHECK(slot.box_min[0] < slot.box_max[0], "the slot's box is inside out");
            size_t curves = 0;
            daegun_batch_curves(batch, &curves);
            CHECK(curves > 0, "the batch holds no curves after a glyph went in");
        }

        daegun_drawn *drawn = NULL;
        st = daegun_font_draw_glyph(font, batch, NULL, NULL, gid, 24.0f, NULL, 0, NULL, -1, &drawn);
        CHECK(st == DAEGUN_OK, "draw_glyph returned %d", st);
        if (st == DAEGUN_OK) {
            int32_t kind = -1;
            bool ok = false;
            daegun_drawn_kind(drawn, &kind);
            daegun_drawn_is_ok(drawn, &ok);
            CHECK(ok, "a CPU-only draw was not ok, kind %d", kind);
            CHECK(kind == DAEGUN_DRAWN_CPU || kind == DAEGUN_DRAWN_REFERENCE
                      || kind == DAEGUN_DRAWN_SCENE,
                  "a NULL device routed to %d rather than the CPU", kind);

            const daegun_bitmap *bmp = NULL;
            if (daegun_drawn_bitmap(drawn, &bmp) == DAEGUN_OK) {
                daegun_metrics m;
                CHECK(daegun_bitmap_metrics(bmp, &m) == DAEGUN_OK, "borrowed metrics failed");
                CHECK(m.width > 0, "the drawn bitmap is empty");
                /* Deliberately NOT daegun_bitmap_free(bmp) – it belongs to the draw result. */
            }
            daegun_drawn_free(drawn);
        }
    }

    daegun_batch_free(batch);
    daegun_font_free(font);
}

static int32_t key_from_array(size_t index, void *user, uint32_t *out_key)
{
    const uint32_t *keys = (const uint32_t *)user;
    *out_key = keys[index];
    return 1;
}

static int32_t key_that_refuses(size_t index, void *user, uint32_t *out_key)
{
    (void)index; (void)user; (void)out_key;
    return 0;
}

static void the_readers_are_bounds_checked(void)
{
    const uint8_t data[4] = { 0x12, 0x34, 0x56, 0x78 };
    uint16_t u16 = 0;
    int16_t  i16 = 0;
    uint32_t u32 = 0;
    size_t   off = 0;

    CHECK(daegun_read_u16_be(data, sizeof data, 0, &u16) == DAEGUN_OK, "read_u16_be failed");
    CHECK(u16 == 0x1234, "read_u16_be gave %04x, not 1234", u16);
    CHECK(daegun_read_u32_be(data, sizeof data, 0, &u32) == DAEGUN_OK, "read_u32_be failed");
    CHECK(u32 == 0x12345678u, "read_u32_be gave %08x", u32);
    CHECK(daegun_read_u24_be(data, sizeof data, 0, &u32) == DAEGUN_OK, "read_u24_be failed");
    CHECK(u32 == 0x123456u, "read_u24_be gave %06x", u32);
    CHECK(daegun_read_i16_be(data, sizeof data, 2, &i16) == DAEGUN_OK, "read_i16_be failed");
    CHECK(i16 == 0x5678, "read_i16_be gave %04x", (unsigned)i16);
    CHECK(daegun_read_offset24(data, sizeof data, 1, &off) == DAEGUN_OK, "read_offset24 failed");

    CHECK(daegun_read_u16_be(data, sizeof data, 3, &u16) == DAEGUN_RANGE,
          "a two-byte read at offset 3 of a four-byte buffer was allowed");
    CHECK(daegun_read_u32_be(data, sizeof data, 1, &u32) == DAEGUN_RANGE,
          "a four-byte read at offset 1 of a four-byte buffer was allowed");
    CHECK(daegun_read_u16_be(data, sizeof data, (size_t)-1, &u16) == DAEGUN_RANGE,
          "an offset that overflows when two is added was allowed");

    uint8_t buf[8] = { 0 };
    CHECK(daegun_write_u16_be(buf, sizeof buf, 0, 0xbeef) == DAEGUN_OK, "write_u16_be failed");
    CHECK(daegun_read_u16_be(buf, sizeof buf, 0, &u16) == DAEGUN_OK, "read back failed");
    CHECK(u16 == 0xbeef, "wrote beef, read %04x", u16);
    CHECK(daegun_write_i16_be(buf, sizeof buf, 2, -2) == DAEGUN_OK, "write_i16_be failed");
    CHECK(daegun_read_i16_be(buf, sizeof buf, 2, &i16) == DAEGUN_OK, "read back failed");
    CHECK(i16 == -2, "wrote -2, read %d", (int)i16);
    CHECK(daegun_write_u32_be(buf, sizeof buf, 4, 0xdeadbeefu) == DAEGUN_OK, "write_u32_be failed");
    CHECK(daegun_read_u32_be(buf, sizeof buf, 4, &u32) == DAEGUN_OK, "read back failed");
    CHECK(u32 == 0xdeadbeefu, "wrote deadbeef, read %08x", u32);
    CHECK(daegun_write_offset24(buf, sizeof buf, 0, 0x010203) == DAEGUN_OK, "write_offset24 failed");
    CHECK(daegun_write_u16_be(buf, sizeof buf, 7, 1) == DAEGUN_RANGE,
          "a two-byte write at offset 7 of an eight-byte buffer was allowed");
    CHECK(daegun_write_u16_be(NULL, 8, 0, 1) == DAEGUN_NULL, "a null write target was not refused");

    CHECK(daegun_records_fit(0, 4, 2, 8) == 1, "four two-byte records do fit in eight bytes");
    CHECK(daegun_records_fit(1, 4, 2, 8) == 0, "they do not fit starting at one");
    CHECK(daegun_records_fit(0, (size_t)-1, 2, 8) == 0, "an overflowing count was called fitting");

    CHECK(daegun_bytes_window(data, sizeof data, 0, 4) == data, "window did not borrow in place");
    CHECK(daegun_bytes_window(data, sizeof data, 1, 4) == NULL, "window past the end was allowed");
    CHECK(daegun_bytes_window(data, sizeof data, 2, 2) == data + 2, "window offset was wrong");

    uint32_t keys[5] = { 10, 20, 30, 40, 50 };
    size_t index = 999;
    int32_t found = -1;
    CHECK(daegun_search_records(5, 30, key_from_array, keys, &index, &found) == DAEGUN_OK,
          "search_records failed");
    CHECK(found != 0 && index == 2, "30 is at index 2, search said %zu found=%d", index, found);
    CHECK(daegun_search_records(5, 35, key_from_array, keys, &index, &found) == DAEGUN_OK,
          "search_records failed for a missing key");
    CHECK(found == 0 && index == 3, "35 inserts at 3, search said %zu found=%d", index, found);
    CHECK(daegun_search_records(5, 30, key_that_refuses, NULL, &index, &found) == DAEGUN_ABSENT,
          "a key reader that refused was not reported");
    CHECK(daegun_search_records(5, 30, NULL, NULL, &index, &found) == DAEGUN_NULL,
          "a null key reader was not refused");

    CHECK(daegun_ot_round(0.5) == 1, "ot_round(0.5) is 1");
    CHECK(daegun_ot_round(-0.5) == 0, "ot_round(-0.5) is 0, not -1: it is floor(v + 0.5)");
    CHECK(daegun_ot_round(2.49) == 2, "ot_round(2.49) is 2");
}

static void raw_tables_round_trip(const char *path)
{
    daegun_font *font = open_font(path);
    if (!font) {
        return;
    }

    int32_t has = 0;
    CHECK(daegun_font_has_table(font, "head", &has) == DAEGUN_OK, "has_table failed");
    CHECK(has, "a font with no head table is not a font");
    CHECK(daegun_font_has_table(font, "ZZZZ", &has) == DAEGUN_OK, "has_table failed");
    CHECK(!has, "the font claims a ZZZZ table");

    daegun_bytes head = { NULL, 0 };
    CHECK(daegun_font_table(font, "head", &head) == DAEGUN_OK, "no head table");
    CHECK(head.data != NULL && head.len >= 54, "head is %zu bytes, the spec says 54", head.len);

    uint32_t magic = 0;
    CHECK(daegun_read_u32_be(head.data, head.len, 12, &magic) == DAEGUN_OK, "reading magic failed");
    CHECK(magic == 0x5f0f3cf5u, "head magic is %08x, not 5f0f3cf5", magic);

    daegun_bytes absent = { (const uint8_t *)1, 99 };
    CHECK(daegun_font_table(font, "ZZZZ", &absent) == DAEGUN_ABSENT, "a missing table was not ABSENT");
    CHECK(absent.data == NULL && absent.len == 0, "an ABSENT table left the view untouched");

    daegun_str_list *tags = NULL;
    CHECK(daegun_font_table_tags(font, &tags) == DAEGUN_OK, "table_tags failed");
    size_t n = 0;
    CHECK(daegun_str_list_count(tags, &n) == DAEGUN_OK, "counting tags failed");
    CHECK(n >= 5, "a face with %zu tables is not a face", n);
    int saw_head = 0;
    for (size_t i = 0; i < n; i++) {
        daegun_str tag = { NULL, 0 };
        CHECK(daegun_str_list_at(tags, i, &tag) == DAEGUN_OK, "tag %zu failed", i);
        CHECK(tag.len == 4, "tag %zu is %zu characters, not four", i, tag.len);
        if (memcmp(tag.data, "head", 4) == 0) {
            saw_head = 1;
        }
    }
    CHECK(saw_head, "head is a table the font has and did not list");
    daegun_str_list_free(tags);

    daegun_axis axes[1] = { { "wght", 700.0 } };
    daegun_table_map *map = NULL;
    CHECK(daegun_font_instance_tables(font, axes, 1, &map) == DAEGUN_OK, "instance_tables failed");
    size_t count = 0;
    CHECK(daegun_table_map_count(map, &count) == DAEGUN_OK, "map count failed");
    CHECK(count >= 5, "the instanced map has %zu tables", count);

    daegun_bytes mapped = { NULL, 0 };
    CHECK(daegun_table_map_get(map, "head", &mapped) == DAEGUN_OK, "the map has no head");
    CHECK(mapped.len >= 54, "the mapped head is %zu bytes", mapped.len);
    CHECK(daegun_table_map_get(map, "ZZZZ", &mapped) == DAEGUN_ABSENT, "the map claims ZZZZ");

    daegun_str first = { NULL, 0 };
    CHECK(daegun_table_map_tag_at(map, 0, &first) == DAEGUN_OK, "tag_at 0 failed");
    CHECK(first.len == 4, "a tag is four characters");
    daegun_bytes first_bytes = { NULL, 0 };
    CHECK(daegun_table_map_bytes_at(map, 0, &first_bytes) == DAEGUN_OK, "bytes_at 0 failed");
    CHECK(daegun_table_map_tag_at(map, count, &first) == DAEGUN_RANGE,
          "an index past the end was allowed");

    daegun_blob *built = NULL;
    CHECK(daegun_table_map_build(map, &built) == DAEGUN_OK, "building from the map failed");
    size_t built_len = 0;
    const uint8_t *built_data = daegun_blob_data(built, &built_len);
    CHECK(built_len > 1000, "the built font is %zu bytes", built_len);

    daegun_font *rebuilt = NULL;
    CHECK(daegun_font_open(built_data, built_len, &rebuilt) == DAEGUN_OK,
          "the font this ABI built does not open: %s", daegun_last_error().data);
    if (rebuilt) {
        uint16_t a = 0, b = 0;
        daegun_font_num_glyphs(font, &a);
        daegun_font_num_glyphs(rebuilt, &b);
        CHECK(a == b, "the rebuilt font has %u glyphs, the original %u", b, a);
        daegun_font_free(rebuilt);
    }
    daegun_blob_free(built);

    CHECK(daegun_table_map_remove(map, "head") == DAEGUN_OK, "removing head failed");
    CHECK(daegun_table_map_remove(map, "head") == DAEGUN_ABSENT, "removing it twice succeeded");
    size_t after = 0;
    daegun_table_map_count(map, &after);
    CHECK(after == count - 1, "removing one table left %zu of %zu", after, count);
    daegun_table_map_free(map);

    daegun_blob *one = NULL;
    CHECK(daegun_font_instance_table(font, axes, 1, "head", &one) == DAEGUN_OK,
          "instance_table failed");
    size_t one_len = 0;
    daegun_blob_data(one, &one_len);
    CHECK(one_len >= 54, "the instanced head is %zu bytes", one_len);
    daegun_blob_free(one);
    CHECK(daegun_font_instance_table(font, axes, 1, "ZZZZ", &one) == DAEGUN_ABSENT,
          "an absent instanced table was not ABSENT");

    daegun_table_map *empty = daegun_table_map_new();
    daegun_blob *nothing = NULL;
    CHECK(daegun_table_map_build(empty, &nothing) == DAEGUN_RANGE, "an empty map built a font");

    const uint8_t payload[8] = { 1, 2, 3, 4, 5, 6, 7, 8 };
    CHECK(daegun_table_map_set(empty, "TEST", payload, sizeof payload) == DAEGUN_OK, "set failed");
    daegun_bytes back = { NULL, 0 };
    CHECK(daegun_table_map_get(empty, "TEST", &back) == DAEGUN_OK, "get after set failed");
    CHECK(back.len == 8 && memcmp(back.data, payload, 8) == 0, "set and get disagree");
    daegun_table_map_free(empty);

    daegun_font_free(font);
}

typedef struct { int moves, lines, quads, curves, closes; } pen_tally;
static void tally_move(void *u, float x, float y) { (void)x; (void)y; ((pen_tally *)u)->moves++; }
static void tally_line(void *u, float x, float y) { (void)x; (void)y; ((pen_tally *)u)->lines++; }
static void tally_quad(void *u, float a, float b, float c, float d)
{ (void)a; (void)b; (void)c; (void)d; ((pen_tally *)u)->quads++; }
static void tally_curve(void *u, float a, float b, float c, float d, float e, float f)
{ (void)a; (void)b; (void)c; (void)d; (void)e; (void)f; ((pen_tally *)u)->curves++; }
static void tally_close(void *u) { ((pen_tally *)u)->closes++; }

static void paths_build_stroke_and_replay(void)
{
    daegun_path *path = daegun_path_new();
    CHECK(path != NULL, "a new path is null");

    int32_t empty = 0;
    CHECK(daegun_path_is_empty(path, &empty) == DAEGUN_OK, "is_empty failed");
    CHECK(empty, "a new path is not empty");

    CHECK(daegun_path_move_to(path, 0.0f, 0.0f) == DAEGUN_OK, "move_to failed");
    CHECK(daegun_path_line_to(path, 100.0f, 0.0f) == DAEGUN_OK, "line_to failed");
    CHECK(daegun_path_quad_to(path, 150.0f, 50.0f, 100.0f, 100.0f) == DAEGUN_OK, "quad_to failed");
    CHECK(daegun_path_curve_to(path, 60.0f, 130.0f, 20.0f, 130.0f, 0.0f, 100.0f) == DAEGUN_OK,
          "curve_to failed");
    CHECK(daegun_path_close(path) == DAEGUN_OK, "close failed");

    CHECK(daegun_path_is_empty(path, &empty) == DAEGUN_OK, "is_empty failed");
    CHECK(!empty, "a path with five verbs is empty");

    double min_x = 9, min_y = 9, max_x = -9, max_y = -9;
    CHECK(daegun_path_bounds(path, &min_x, &min_y, &max_x, &max_y) == DAEGUN_OK, "bounds failed");
    CHECK(min_x == 0.0 && min_y == 0.0, "bounds start at (%g, %g)", min_x, min_y);
    CHECK(max_x >= 100.0 && max_y >= 100.0, "bounds end at (%g, %g)", max_x, max_y);

    size_t cost = 0;
    CHECK(daegun_path_cost(path, &cost) == DAEGUN_OK, "cost failed");
    CHECK(cost > 0, "a path with four segments costs nothing");

    size_t verb_count = 0, point_count = 0;
    CHECK(daegun_path_verbs(path, NULL, 0, &verb_count) == DAEGUN_OK, "verb count failed");
    CHECK(verb_count == 5, "five verbs went in, %zu came out", verb_count);
    CHECK(daegun_path_points(path, NULL, NULL, 0, &point_count) == DAEGUN_OK, "point count failed");
    CHECK(point_count == 7, "1 + 1 + 2 + 3 points went in, %zu came out", point_count);

    uint8_t verbs[8] = { 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff };
    CHECK(daegun_path_verbs(path, verbs, 8, &verb_count) == DAEGUN_OK, "verbs failed");
    CHECK(verbs[0] == DAEGUN_VERB_MOVE, "the first verb is not a move");
    CHECK(verbs[1] == DAEGUN_VERB_LINE, "the second verb is not a line");
    CHECK(verbs[2] == DAEGUN_VERB_QUAD, "the third verb is not a quad");
    CHECK(verbs[3] == DAEGUN_VERB_CUBIC, "the fourth verb is not a cubic");
    CHECK(verbs[4] == DAEGUN_VERB_CLOSE, "the fifth verb is not a close");
    CHECK(verbs[5] == 0xff, "verbs wrote past the count");

    uint8_t two[2] = { 0xff, 0xff };
    size_t whole = 0;
    CHECK(daegun_path_verbs(path, two, 2, &whole) == DAEGUN_OK, "a short verb buffer failed");
    CHECK(whole == 5, "a short buffer reported %zu rather than the whole count", whole);
    CHECK(two[0] == DAEGUN_VERB_MOVE && two[1] == DAEGUN_VERB_LINE, "a short buffer filled wrong");

    float xs[8] = { 0 }, ys[8] = { 0 };
    CHECK(daegun_path_points(path, xs, ys, 8, &point_count) == DAEGUN_OK, "points failed");
    CHECK(xs[0] == 0.0f && ys[0] == 0.0f, "the first point is (%g, %g)", xs[0], ys[0]);
    CHECK(xs[1] == 100.0f && ys[1] == 0.0f, "the second point is (%g, %g)", xs[1], ys[1]);

    pen_tally seen = { 0, 0, 0, 0, 0 };
    daegun_pen pen = { tally_move, tally_line, tally_quad, tally_curve, tally_close, &seen };
    CHECK(daegun_path_replay(path, NULL, &pen) == DAEGUN_OK, "replay failed");
    CHECK(seen.moves == 1 && seen.lines == 1 && seen.quads == 1 && seen.curves == 1
              && seen.closes == 1,
          "replay gave %d/%d/%d/%d/%d rather than one of each",
          seen.moves, seen.lines, seen.quads, seen.curves, seen.closes);

    const double shift[6] = { 1.0, 0.0, 0.0, 1.0, 10.0, 20.0 };
    daegun_path *moved = daegun_path_new();
    daegun_pen into_moved;
    CHECK(daegun_path_as_pen(moved, &into_moved) == DAEGUN_OK, "as_pen failed");
    CHECK(daegun_path_replay(path, shift, &into_moved) == DAEGUN_OK, "transformed replay failed");
    double mx = 9, my = 9, xx = 0, yy = 0;
    CHECK(daegun_path_bounds(moved, &mx, &my, &xx, &yy) == DAEGUN_OK, "bounds of the copy failed");
    CHECK(mx == 10.0 && my == 20.0, "a (10, 20) shift put the corner at (%g, %g)", mx, my);
    daegun_path_free(moved);

    daegun_stroke_style style = { 10.0f, DAEGUN_CAP_ROUND, DAEGUN_JOIN_ROUND, 4.0f };
    pen_tally stroked = { 0, 0, 0, 0, 0 };
    daegun_pen spen = { tally_move, tally_line, tally_quad, tally_curve, tally_close, &stroked };
    CHECK(daegun_path_stroke(path, &style, 0.25f, &spen) == DAEGUN_OK, "stroke failed");
    CHECK(stroked.moves > 0 && stroked.lines > 0, "stroking drew nothing");

    pen_tally simple = { 0, 0, 0, 0, 0 };
    daegun_pen sipen = { tally_move, tally_line, tally_quad, tally_curve, tally_close, &simple };
    CHECK(daegun_path_stroke_simplified(path, &style, 0.25f, &sipen) == DAEGUN_OK,
          "stroke_simplified failed");
    CHECK(simple.moves > 0, "simplified stroking drew nothing");

    CHECK(daegun_path_stroke(NULL, &style, 0.25f, &spen) == DAEGUN_NULL, "a null path was stroked");
    CHECK(daegun_path_stroke(path, NULL, 0.25f, &spen) == DAEGUN_NULL, "a null style was accepted");

    daegun_path_free(path);
    daegun_path_free(NULL);
}

static void character_properties_need_no_font(void)
{
    int32_t gc = -1;
    CHECK(daegun_char_general_category('A', &gc) == DAEGUN_OK, "general_category failed");
    CHECK(gc == DAEGUN_GC_UPPERCASE_LETTER, "'A' is category %d, not uppercase letter", gc);
    CHECK(daegun_char_general_category('a', &gc) == DAEGUN_OK, "general_category failed");
    CHECK(gc == DAEGUN_GC_LOWERCASE_LETTER, "'a' is category %d, not lowercase letter", gc);
    CHECK(daegun_char_general_category(' ', &gc) == DAEGUN_OK, "general_category failed");
    CHECK(gc == DAEGUN_GC_SPACE_SEPARATOR, "space is category %d", gc);
    CHECK(daegun_char_general_category('5', &gc) == DAEGUN_OK, "general_category failed");
    CHECK(gc == DAEGUN_GC_DECIMAL_NUMBER, "'5' is category %d", gc);

    CHECK(daegun_char_general_category(0xD800, &gc) == DAEGUN_RANGE, "a surrogate was categorised");
    CHECK(daegun_char_general_category(0x110000, &gc) == DAEGUN_RANGE, "past U+10FFFF was allowed");

    uint32_t form = 0;
    CHECK(daegun_char_vertical_form(0x2014, &form) == DAEGUN_OK, "an em dash has a vertical form");
    CHECK(form == 0xFE31, "the em dash's vertical form is U+%04X, not FE31", form);
    CHECK(daegun_char_vertical_form(0x3001, &form) == DAEGUN_OK, "an ideographic comma has one");
    CHECK(form == 0xFE11, "the ideographic comma's form is U+%04X", form);
    CHECK(daegun_char_vertical_form('A', &form) == DAEGUN_ABSENT, "'A' has a vertical form");

    int32_t upright = -1;
    CHECK(daegun_char_is_upright(0x4E00, 0, &upright) == DAEGUN_OK, "is_upright failed");
    CHECK(upright, "a CJK ideograph does not stand upright in vertical text");
    CHECK(daegun_char_is_upright('A', 0, &upright) == DAEGUN_OK, "is_upright failed");
    CHECK(!upright, "a latin capital stands upright in vertical text");
}

static void the_format_walkers_read_a_real_table(const char *path)
{
    daegun_font *font = open_font(path);
    if (!font) {
        return;
    }
    uint16_t num_glyphs = 0;
    daegun_font_num_glyphs(font, &num_glyphs);

    daegun_bytes gsub = { NULL, 0 };
    if (daegun_font_table(font, "GSUB", &gsub) == DAEGUN_OK) {
        uint16_t idx = 0;
        daegun_status st = daegun_coverage_index(gsub.data, gsub.len, 1, &idx);
        CHECK(st == DAEGUN_OK || st == DAEGUN_ABSENT, "coverage_index returned %d", st);
    }

    daegun_bytes hvar = { NULL, 0 };
    if (daegun_font_table(font, "HVAR", &hvar) == DAEGUN_OK) {
        uint32_t ivs_off = 0;
        CHECK(daegun_read_u32_be(hvar.data, hvar.len, 4, &ivs_off) == DAEGUN_OK,
              "HVAR has no itemVariationStoreOffset");
        daegun_ivs *ivs = NULL;
        daegun_status st = daegun_ivs_parse(hvar.data, hvar.len, ivs_off, &ivs);
        CHECK(st == DAEGUN_OK, "parsing HVAR's store returned %d: %s", st, daegun_last_error().data);
        if (st == DAEGUN_OK) {
            size_t axes = 0, regions = 0, ivds = 0;
            CHECK(daegun_ivs_axis_count(ivs, &axes) == DAEGUN_OK, "axis_count failed");
            CHECK(daegun_ivs_region_count(ivs, &regions) == DAEGUN_OK, "region_count failed");
            CHECK(daegun_ivs_ivd_count(ivs, &ivds) == DAEGUN_OK, "ivd_count failed");
            CHECK(axes > 0, "a variable font's store covers no axes");
            CHECK(regions > 0, "a variable font's store has no regions");
            CHECK(ivds > 0, "a variable font's store has no subtables");

            daegun_region_axis ra = { 9, 9, 9 };
            CHECK(daegun_ivs_region_axis(ivs, 0, 0, &ra) == DAEGUN_OK, "region_axis failed");
            CHECK(ra.start <= ra.peak && ra.peak <= ra.end,
                  "a region runs %g / %g / %g, out of order", ra.start, ra.peak, ra.end);
            CHECK(daegun_ivs_region_axis(ivs, regions, 0, &ra) == DAEGUN_RANGE,
                  "a region past the end was allowed");

            size_t rows = 0;
            CHECK(daegun_ivs_ivd_rows(ivs, 0, &rows) == DAEGUN_OK, "ivd_rows failed");
            size_t region_count = 0;
            const size_t *indices = daegun_ivs_ivd_region_indices(ivs, 0, &region_count);
            CHECK(indices != NULL || region_count == 0, "region indices came back null");
            if (rows > 0) {
                size_t row_len = 0;
                const int32_t *row = daegun_ivs_ivd_row(ivs, 0, 0, &row_len);
                CHECK(row != NULL, "the first delta row is null");
                CHECK(row_len == region_count,
                      "a row has %zu deltas for %zu regions", row_len, region_count);
            }
            CHECK(daegun_ivs_ivd_row(ivs, 0, rows, &rows) == NULL, "a row past the end was given");

            double location[4] = { 0.0, 0.0, 0.0, 0.0 };
            daegun_f64_list *scalars = NULL;
            CHECK(daegun_ivs_region_scalars(ivs, location, axes, &scalars) == DAEGUN_OK,
                  "region_scalars failed");
            size_t scalar_count = 0;
            const double *sd = daegun_f64_list_data(scalars, &scalar_count);
            CHECK(scalar_count == regions, "%zu scalars for %zu regions", scalar_count, regions);
            double delta = 99.0;
            CHECK(daegun_ivs_delta(ivs, 0, 0, sd, scalar_count, &delta) == DAEGUN_OK,
                  "ivs_delta failed");
            CHECK(delta == 0.0, "at the default location every delta is zero, got %g", delta);
            daegun_f64_list_free(scalars);
            daegun_ivs_free(ivs);
        }
    }

    daegun_bytes morx = { NULL, 0 };
    if (daegun_font_table(font, "morx", &morx) == DAEGUN_OK) {
        daegun_aat_lookup *lookup = NULL;
        daegun_status st = daegun_aat_lookup_open(morx.data, morx.len, num_glyphs, &lookup);
        CHECK(st == DAEGUN_OK || st == DAEGUN_PARSE, "aat_lookup_open returned %d", st);
        if (st == DAEGUN_OK) {
            daegun_aat_lookup_free(lookup);
        }
    }

    const uint8_t junk[8] = { 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff };
    daegun_ivs *bad = NULL;
    CHECK(daegun_ivs_parse(junk, sizeof junk, 0, &bad) == DAEGUN_PARSE, "junk parsed as a store");
    daegun_feature_variations *fv = NULL;
    CHECK(daegun_feature_variations_open(junk, sizeof junk, &fv) == DAEGUN_ABSENT,
          "junk carried feature variations");
    daegun_ankr *ankr = NULL;
    CHECK(daegun_ankr_open(junk, sizeof junk, num_glyphs, &ankr) == DAEGUN_PARSE,
          "junk parsed as an ankr table");

    daegun_font_free(font);
}

static void loca_and_glyf_are_walkable(const char *path)
{
    daegun_font *font = open_font(path);
    if (!font) {
        return;
    }
    daegun_bytes head = { NULL, 0 }, loca = { NULL, 0 }, glyf = { NULL, 0 };
    if (daegun_font_table(font, "loca", &loca) != DAEGUN_OK
        || daegun_font_table(font, "glyf", &glyf) != DAEGUN_OK
        || daegun_font_table(font, "head", &head) != DAEGUN_OK) {
        daegun_font_free(font);
        return;
    }

    int16_t loca_format = 0;
    CHECK(daegun_read_i16_be(head.data, head.len, 50, &loca_format) == DAEGUN_OK,
          "head has no indexToLocFormat");

    uint16_t num_glyphs = 0;
    daegun_font_num_glyphs(font, &num_glyphs);

    daegun_usize_list *offsets = NULL;
    CHECK(daegun_parse_loca(loca.data, loca.len, loca_format, num_glyphs, &offsets) == DAEGUN_OK,
          "parse_loca failed");
    size_t n = 0;
    const size_t *offs = daegun_usize_list_data(offsets, &n);
    CHECK(n == (size_t)num_glyphs + 1, "loca gave %zu offsets for %u glyphs", n, num_glyphs);
    CHECK(offs[0] <= offs[n - 1], "loca runs backwards");

    uint16_t gid = 0;
    daegun_font_glyph_id(font, 'A', &gid);
    pen_tally raw = { 0, 0, 0, 0, 0 };
    daegun_pen pen = { tally_move, tally_line, tally_quad, tally_curve, tally_close, &raw };
    daegun_status st = daegun_outline_glyf_bytes(glyf.data, glyf.len, offs, n, gid, &pen);
    CHECK(st == DAEGUN_OK, "outlining glyph %u from raw bytes returned %d: %s",
          gid, st, daegun_last_error().data);
    CHECK(raw.moves > 0, "outlining 'A' from raw glyf bytes drew nothing");

    pen_tally via_font = { 0, 0, 0, 0, 0 };
    daegun_pen fpen = { tally_move, tally_line, tally_quad, tally_curve, tally_close, &via_font };
    CHECK(daegun_font_outline_glyph(font, gid, &fpen) == DAEGUN_OK, "drawing via the font failed");
    CHECK(raw.moves == via_font.moves && raw.lines == via_font.lines
              && raw.quads == via_font.quads && raw.closes == via_font.closes,
          "the raw tier drew %d/%d/%d/%d and the font tier %d/%d/%d/%d for the same glyph",
          raw.moves, raw.lines, raw.quads, raw.closes,
          via_font.moves, via_font.lines, via_font.quads, via_font.closes);

    daegun_usize_list_free(offsets);
    daegun_font_free(font);
}

static void the_shader_source_is_available(void)
{
    const int32_t langs[3] = { DAEGUN_SHADER_GLSL, DAEGUN_SHADER_HLSL, DAEGUN_SHADER_MSL };
    const int32_t stages[3] = { DAEGUN_STAGE_VERTEX, DAEGUN_STAGE_FRAGMENT,
                                DAEGUN_STAGE_SUBPIXEL_FRAGMENT };
    for (int l = 0; l < 3; l++) {
        for (int st = 0; st < 3; st++) {
            daegun_text *src = NULL;
            CHECK(daegun_shader_source(langs[l], stages[st], &src) == DAEGUN_OK,
                  "shader source %d/%d failed", langs[l], stages[st]);
            daegun_str v = { NULL, 0 };
            CHECK(daegun_text_str(src, &v) == DAEGUN_OK, "shader str failed");
            CHECK(v.len > 200, "shader %d/%d is %zu bytes, too short to be a shader",
                  langs[l], stages[st], v.len);
            CHECK(strlen(v.data) == v.len, "the shader's NUL disagrees with its length");
            daegun_text_free(src);
        }
    }
    daegun_text *none = NULL;
    CHECK(daegun_shader_source(99, DAEGUN_STAGE_VERTEX, &none) == DAEGUN_RANGE,
          "an unknown language was accepted");
    CHECK(daegun_shader_source(DAEGUN_SHADER_GLSL, 99, &none) == DAEGUN_RANGE,
          "an unknown stage was accepted");
}

static void routing_decides_without_drawing(void)
{
    daegun_policy pol;
    CHECK(daegun_policy_default(&pol) == DAEGUN_OK, "policy default failed");
    daegun_request req = { 24.0f, 0, 0, 0, 0, 0 };
    int32_t routed = -1;

    CHECK(daegun_route(DAEGUN_GPU_OK, &req, NULL, &pol, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed == DAEGUN_ROUTED_CPU, "no device routed to %d rather than the CPU", routed);

    daegun_device_profile *gpu = NULL;
    CHECK(daegun_device_profile_new(DAEGUN_DEVICE_DISCRETE, "a made-up card", &gpu) == DAEGUN_OK,
          "building a profile failed");
    int32_t kind = -1;
    CHECK(daegun_device_profile_kind(gpu, &kind) == DAEGUN_OK, "profile kind failed");
    CHECK(kind == DAEGUN_DEVICE_DISCRETE, "a discrete profile reads back as %d", kind);
    daegun_text *pname = NULL;
    CHECK(daegun_device_profile_name(gpu, &pname) == DAEGUN_OK, "profile name failed");
    daegun_str nv = { NULL, 0 };
    daegun_text_str(pname, &nv);
    CHECK(nv.len == strlen("a made-up card") && memcmp(nv.data, "a made-up card", nv.len) == 0,
          "the profile renamed itself to %s", nv.data);
    daegun_text_free(pname);

    CHECK(daegun_route(DAEGUN_GPU_OK, &req, gpu, &pol, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed == DAEGUN_ROUTED_GPU, "a discrete card at 24ppem routed to %d", routed);

    CHECK(daegun_route(DAEGUN_GPU_NO_OUTLINE, &req, gpu, &pol, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed == DAEGUN_ROUTED_NOTHING, "a glyph with no outline routed to %d", routed);
    CHECK(daegun_route(DAEGUN_GPU_TOO_COMPLEX, &req, gpu, &pol, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed == DAEGUN_ROUTED_CPU, "a too-complex glyph routed to %d rather than the CPU", routed);
    CHECK(daegun_route(DAEGUN_GPU_NON_FINITE, &req, gpu, &pol, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed == DAEGUN_ROUTED_REFUSED_NON_FINITE, "a non-finite glyph routed to %d", routed);
    CHECK(daegun_route(DAEGUN_GPU_BATCH_FULL, &req, gpu, &pol, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed == DAEGUN_ROUTED_FLUSH_AND_RETRY, "a full batch routed to %d", routed);
    CHECK(daegun_route(DAEGUN_GPU_NOT_FLAT_COLOR, &req, gpu, &pol, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed == DAEGUN_ROUTED_SCENE, "a color glyph routed to %d", routed);

    daegun_policy cpu_only = pol;
    cpu_only.prefer = DAEGUN_PREFER_CPU;
    CHECK(daegun_route(DAEGUN_GPU_OK, &req, gpu, &cpu_only, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed == DAEGUN_ROUTED_CPU, "PREFER_CPU with a card routed to %d", routed);

    CHECK(daegun_route(99, &req, gpu, &pol, &routed) == DAEGUN_RANGE, "an unknown attempt was taken");
    CHECK(daegun_route(DAEGUN_GPU_OK, NULL, gpu, &pol, &routed) == DAEGUN_NULL,
          "a null request was accepted");
    daegun_device_profile_free(gpu);
    daegun_device_profile_free(NULL);
}

static void subpixel_params_come_from_a_layout(void)
{
    daegun_subpixel_params sp;
    memset(&sp, 0xff, sizeof sp);
    CHECK(daegun_subpixel_params_from_layout(DAEGUN_LAYOUT_GRAYSCALE, &sp) == DAEGUN_OK,
          "grayscale params failed");
    CHECK(sp.channels == 1, "grayscale has %u channels", sp.channels);
    CHECK(sp.taps[0] <= DAEGUN_MAX_SUBPIXEL_TAPS && sp.taps[1] <= DAEGUN_MAX_SUBPIXEL_TAPS,
          "taps %u/%u exceed the shader's table", sp.taps[0], sp.taps[1]);
    CHECK(sp.supersample <= DAEGUN_MAX_SUPERSAMPLE, "supersample %u is past the cap", sp.supersample);

    daegun_subpixel_params rgb;
    CHECK(daegun_subpixel_params_from_layout(DAEGUN_LAYOUT_RGB_H, &rgb) == DAEGUN_OK,
          "rgb params failed");
    CHECK(rgb.channels == 3, "horizontal RGB has %u channels", rgb.channels);
    CHECK(memcmp(&sp, &rgb, sizeof sp) != 0, "grayscale and RGB produced identical filters");
    CHECK(daegun_subpixel_params_from_layout(DAEGUN_LAYOUT_GRAYSCALE, NULL) == DAEGUN_NULL,
          "a null out-parameter was accepted");
}

#define GPU_BACKEND_TEST(b)                                                                        \
static int b##_draws_a_glyph(const char *path)                                                     \
{                                                                                                  \
    daegun_##b##_renderer *r = NULL;                                                               \
    daegun_status st = daegun_##b##_renderer_new(&r);                                              \
    if (st == DAEGUN_UNSUPPORTED) {                                                                \
        return 0; /* No such device here. An answer, not a failure. */                             \
    }                                                                                              \
    CHECK(st == DAEGUN_OK, #b " renderer_new returned %d: %s", st, daegun_last_error().data);      \
    if (st != DAEGUN_OK) {                                                                         \
        return 0;                                                                                  \
    }                                                                                              \
                                                                                                   \
    daegun_text *name = NULL;                                                                      \
    CHECK(daegun_##b##_renderer_device_name(r, &name) == DAEGUN_OK, #b " device_name failed");     \
    daegun_str nv = { NULL, 0 };                                                                   \
    daegun_text_str(name, &nv);                                                                    \
    CHECK(nv.len > 0, #b " reports an empty device name");                                         \
    daegun_text_free(name);                                                                        \
                                                                                                   \
    daegun_device_profile *prof = NULL;                                                            \
    CHECK(daegun_##b##_renderer_profile(r, &prof) == DAEGUN_OK, #b " profile failed");             \
    int32_t kind = -1;                                                                             \
    daegun_device_profile_kind(prof, &kind);                                                       \
    CHECK(kind >= DAEGUN_DEVICE_UNKNOWN && kind <= DAEGUN_DEVICE_SOFTWARE,                         \
          #b " reports device kind %d", kind);                                                     \
    daegun_device_profile_free(prof);                                                              \
                                                                                                   \
    int32_t sub = -1;                                                                              \
    CHECK(daegun_##b##_renderer_supports_subpixel(r, &sub) == DAEGUN_OK, #b " subpixel failed");   \
    CHECK(sub == 0 || sub == 1, #b " answered %d to a yes-or-no question", sub);                   \
                                                                                                   \
    float proj[16] = { 0 };                                                                        \
    CHECK(daegun_##b##_ortho(64, 64, proj) == DAEGUN_OK, #b " ortho failed");                      \
    CHECK(proj[0] != 0.0f && proj[15] != 0.0f, #b " ortho is degenerate");                         \
                                                                                                   \
    daegun_font *font = open_font(path);                                                           \
    if (!font) { daegun_##b##_renderer_free(r); return 0; }                                        \
    daegun_batch *batch = NULL;                                                                    \
    CHECK(daegun_batch_new(&batch) == DAEGUN_OK, "batch_new failed");                              \
    uint16_t gid = 0;                                                                              \
    daegun_font_glyph_id(font, 'B', &gid);                                                         \
    daegun_glyph_slot slot;                                                                        \
    memset(&slot, 0, sizeof slot);                                                                 \
    st = daegun_font_gpu_glyph(font, batch, gid, NULL, 0, &slot);                                  \
    CHECK(st == DAEGUN_OK, "gpu_glyph returned %d", st);                                           \
                                                                                                   \
    daegun_##b##_target *target = NULL;                                                            \
    st = daegun_##b##_target_new(r, 64, 64, &target);                                              \
    CHECK(st == DAEGUN_OK, #b " target_new returned %d: %s", st, daegun_last_error().data);        \
    daegun_##b##_geometry *geom = NULL;                                                            \
    st = daegun_##b##_geometry_new(r, batch, &geom);                                               \
    CHECK(st == DAEGUN_OK, #b " geometry_new returned %d: %s", st, daegun_last_error().data);      \
                                                                                                   \
    if (target && geom) {                                                                          \
        uint32_t w = 0, h = 0;                                                                     \
        CHECK(daegun_##b##_target_width(target, &w) == DAEGUN_OK, #b " target width failed");      \
        CHECK(daegun_##b##_target_height(target, &h) == DAEGUN_OK, #b " target height failed");    \
        CHECK(w == 64 && h == 64, #b " made a %ux%u target from a 64x64 request", w, h);           \
                                                                                                   \
        uint64_t brev = 0, grev = 0;                                                               \
        daegun_batch_revision(batch, &brev);                                                       \
        CHECK(daegun_##b##_geometry_revision(geom, &grev) == DAEGUN_OK, #b " revision failed");    \
        CHECK(grev == brev, #b " uploaded revision %llu of a batch at %llu",                       \
              (unsigned long long)grev, (unsigned long long)brev);                                 \
                                                                                                   \
        daegun_subpixel_params sp;                                                                 \
        daegun_subpixel_params_from_layout(DAEGUN_LAYOUT_GRAYSCALE, &sp);                          \
                                                                                                   \
        const float off[2] = { 8.0f, 8.0f };                                                       \
        const float em[2] = { 48.0f, 48.0f };                                                      \
        const float white[4] = { 1.0f, 1.0f, 1.0f, 1.0f };                                         \
        daegun_glyph_instance inst;                                                                \
        memset(&inst, 0, sizeof inst);                                                             \
        CHECK(daegun_glyph_slot_instance(&slot, off, 48.0f, em, white, &inst) == DAEGUN_OK,        \
              "slot_instance failed");                                                             \
        CHECK(inst.inv_scale == 1.0f / 48.0f, "inv_scale is %g", (double)inst.inv_scale);          \
                                                                                                   \
        st = daegun_##b##_draw(r, target, geom, &inst, 1, &sp, DAEGUN_MODE_GRAYSCALE);             \
        CHECK(st == DAEGUN_OK, #b " draw returned %d: %s", st, daegun_last_error().data);          \
        CHECK(daegun_##b##_wait(r, target) == DAEGUN_OK, #b " wait failed");                       \
                                                                                                   \
        size_t n = 0;                                                                              \
        const uint8_t *px = daegun_##b##_read_pixels(r, target, &n);                               \
        CHECK(px != NULL, #b " read_pixels gave nothing: %s", daegun_last_error().data);           \
        if (px) {                                                                                  \
            CHECK(n == 64u * 64u * 4u, #b " read %zu bytes for a 64x64 BGRA target", n);           \
            size_t ink = 0;                                                                        \
            for (size_t i = 3; i < n; i += 4) { if (px[i] != 0) ink++; }                           \
            CHECK(ink > 0, #b " drew a glyph and every pixel came back empty");                    \
            size_t cn = 0;                                                                         \
            const uint8_t *cached = daegun_##b##_target_pixels(target, &cn);                       \
            CHECK(cached != NULL && cn == n, #b " the cached view is %zu of %zu bytes", cn, n);    \
            if (cached && cn == n) {                                                               \
                CHECK(memcmp(cached, px, n) == 0, #b " the cached pixels differ from the read");   \
            }                                                                                      \
            uint8_t one[4] = { 0, 0, 0, 0 };                                                       \
            CHECK(daegun_##b##_target_pixel(target, 0, 0, one) == DAEGUN_OK, #b " pixel failed");  \
            CHECK(daegun_##b##_target_pixel(target, 64, 0, one) == DAEGUN_RANGE,                   \
                  #b " read a pixel outside the target");                                          \
        }                                                                                          \
                                                                                                   \
        st = daegun_##b##_draw_with(r, target, geom, &inst, 1, &sp, DAEGUN_MODE_GRAYSCALE, proj);  \
        CHECK(st == DAEGUN_OK, #b " draw_with returned %d: %s", st, daegun_last_error().data);     \
                                                                                                   \
        daegun_##b##_target *bad = NULL;                                                           \
        CHECK(daegun_##b##_target_new(r, 0, 64, &bad) != DAEGUN_OK, #b " made a zero-wide target");\
    }                                                                                              \
                                                                                                   \
    daegun_##b##_renderer_free(r);                                                                 \
    if (target) {                                                                                  \
        uint32_t w = 0;                                                                            \
        CHECK(daegun_##b##_target_width(target, &w) == DAEGUN_OK,                                  \
              #b " target stopped answering after its renderer was freed");                        \
        CHECK(w == 64, #b " target width is %u after the renderer went", w);                       \
        size_t cn = 0;                                                                             \
        const uint8_t *cached = daegun_##b##_target_pixels(target, &cn);                           \
        CHECK(cached != NULL && cn > 0, #b " the pixels went with the renderer");                  \
    }                                                                                              \
    if (geom) {                                                                                    \
        uint64_t rev = 0;                                                                          \
        CHECK(daegun_##b##_geometry_revision(geom, &rev) == DAEGUN_OK,                             \
              #b " geometry stopped answering after its renderer was freed");                      \
    }                                                                                              \
    daegun_##b##_geometry_free(geom);                                                              \
    daegun_##b##_target_free(target);                                                              \
    daegun_batch_free(batch);                                                                      \
    daegun_font_free(font);                                                                        \
    return 1;                                                                                      \
}

#if defined(__APPLE__)
GPU_BACKEND_TEST(metal)
#endif
GPU_BACKEND_TEST(vulkan)
#if defined(_WIN32)
GPU_BACKEND_TEST(d3d11)
GPU_BACKEND_TEST(d3d12)
#endif

#if defined(__APPLE__)
static void metal_adopts_a_device_and_refuses_bad_surfaces(const char *path)
{
    void *device = MTLCreateSystemDefaultDevice();
    if (!device) {
        return;
    }

    daegun_metal_renderer *adopted = NULL;
    daegun_status st = daegun_metal_renderer_from_device(device, &adopted);
    CHECK(st == DAEGUN_OK, "metal renderer_from_device returned %d: %s", st,
          daegun_last_error().data);

    if (st == DAEGUN_OK) {
        daegun_metal_renderer *own = NULL;
        if (daegun_metal_renderer_new(&own) == DAEGUN_OK) {
            daegun_text *a = NULL;
            daegun_text *b = NULL;
            daegun_metal_renderer_device_name(adopted, &a);
            daegun_metal_renderer_device_name(own, &b);
            daegun_str av = { NULL, 0 };
            daegun_str bv = { NULL, 0 };
            daegun_text_str(a, &av);
            daegun_text_str(b, &bv);
            CHECK(av.len > 0 && av.len == bv.len && memcmp(av.data, bv.data, av.len) == 0,
                  "adopted \"%s\" but the default device is \"%s\"",
                  av.data ? av.data : "?", bv.data ? bv.data : "?");
            daegun_text_free(a);
            daegun_text_free(b);
            daegun_metal_renderer_free(own);
        }

        daegun_font *font = open_font(path);
        daegun_batch *batch = NULL;
        if (font && daegun_batch_new(&batch) == DAEGUN_OK) {
            uint16_t gid = 0;
            daegun_glyph_slot slot;
            memset(&slot, 0, sizeof slot);
            daegun_font_glyph_id(font, 'B', &gid);
            CHECK(daegun_font_gpu_glyph(font, batch, gid, NULL, 0, &slot) == DAEGUN_OK,
                  "gpu_glyph failed for the adopted device");

            daegun_metal_geometry *geom = NULL;
            CHECK(daegun_metal_geometry_new(adopted, batch, &geom) == DAEGUN_OK,
                  "geometry_new on an adopted device failed: %s", daegun_last_error().data);

            /* Both byte orders, since a caller's surface picks the format rather than daegun. */
            const int32_t formats[2] = { DAEGUN_SURFACE_RGBA8, DAEGUN_SURFACE_BGRA8 };
            for (int f = 0; f < 2 && geom; f++) {
                daegun_metal_target *target = NULL;
                st = daegun_metal_target_with_format(adopted, 64, 64, formats[f], &target);
                CHECK(st == DAEGUN_OK, "target_with_format(%d) returned %d: %s", formats[f], st,
                      daegun_last_error().data);
                if (st != DAEGUN_OK) {
                    continue;
                }

                daegun_subpixel_params sp;
                daegun_subpixel_params_from_layout(DAEGUN_LAYOUT_GRAYSCALE, &sp);
                const float off[2] = { 8.0f, 8.0f };
                const float em[2] = { 48.0f, 48.0f };
                const float white[4] = { 1.0f, 1.0f, 1.0f, 1.0f };
                daegun_glyph_instance inst;
                memset(&inst, 0, sizeof inst);
                daegun_glyph_slot_instance(&slot, off, 48.0f, em, white, &inst);

                st = daegun_metal_draw(adopted, target, geom, &inst, 1, &sp,
                                       DAEGUN_MODE_GRAYSCALE);
                CHECK(st == DAEGUN_OK, "the adopted device refused to draw: %d", st);
                CHECK(daegun_metal_wait(adopted, target) == DAEGUN_OK, "wait failed");

                size_t n = 0;
                const uint8_t *px = daegun_metal_read_pixels(adopted, target, &n);
                CHECK(px != NULL, "read_pixels on an adopted device gave nothing");
                size_t ink = 0;
                for (size_t i = 3; px && i < n; i += 4) {
                    if (px[i] != 0) {
                        ink++;
                    }
                }
                CHECK(ink > 0, "format %d drew nothing on the adopted device", formats[f]);
                daegun_metal_target_free(target);
            }
            daegun_metal_geometry_free(geom);
        }
        daegun_batch_free(batch);
        daegun_font_free(font);
    }

    daegun_metal_target *bad = NULL;
    CHECK(daegun_metal_target_from_texture(adopted, NULL, 64, 64, &bad) != DAEGUN_OK,
          "a NULL texture was accepted as a surface");
    CHECK(bad == NULL, "a refused borrow still wrote an out-parameter");
    CHECK(daegun_metal_target_from_drawable(adopted, NULL, 64, 64, &bad) != DAEGUN_OK,
          "a NULL drawable was accepted as a surface");
    CHECK(daegun_metal_target_from_texture(NULL, NULL, 64, 64, &bad) == DAEGUN_NULL,
          "a NULL renderer was not reported as such");

    daegun_metal_renderer *nope = NULL;
    CHECK(daegun_metal_renderer_from_device(NULL, &nope) != DAEGUN_OK,
          "a NULL device was adopted");
    CHECK(daegun_metal_renderer_from_device(device, NULL) == DAEGUN_NULL,
          "a NULL out-parameter was accepted");

    daegun_metal_renderer_free(adopted);
    ((void (*)(void *, SEL))objc_msgSend)(device, sel_registerName("release"));
    printf("  metal: adopted the caller's device, both byte orders drew\n");
}
#endif

static void the_gpu_backends_draw(const char *path)
{
    int ran = 0;
#if defined(__APPLE__)
    ran += metal_draws_a_glyph(path);
    metal_adopts_a_device_and_refuses_bad_surfaces(path);
#endif
    ran += vulkan_draws_a_glyph(path);
#if defined(_WIN32)
    ran += d3d11_draws_a_glyph(path);
    ran += d3d12_draws_a_glyph(path);
#endif
#if defined(_WIN32)
    for (int which = 0; which < 2; which++) {
        daegun_text *level = NULL;
        int32_t soft = -1;
        daegun_status st;
        if (which == 0) {
            daegun_d3d11_renderer *r = NULL;
            if (daegun_d3d11_renderer_new(&r) != DAEGUN_OK) { continue; }
            st = daegun_d3d11_feature_level(r, &level);
            CHECK(st == DAEGUN_OK, "d3d11 feature_level returned %d", st);
            CHECK(daegun_d3d11_is_software(r, &soft) == DAEGUN_OK, "d3d11 is_software failed");
            daegun_d3d11_renderer_free(r);
        } else {
            daegun_d3d12_renderer *r = NULL;
            if (daegun_d3d12_renderer_new(&r) != DAEGUN_OK) { continue; }
            st = daegun_d3d12_feature_level(r, &level);
            CHECK(st == DAEGUN_OK, "d3d12 feature_level returned %d", st);
            CHECK(daegun_d3d12_is_software(r, &soft) == DAEGUN_OK, "d3d12 is_software failed");
            daegun_d3d12_renderer_free(r);
        }
        daegun_str lv = { NULL, 0 };
        daegun_text_str(level, &lv);
        CHECK(lv.len >= 4, "d3d%d feature level is \"%s\"", which ? 12 : 11, lv.data ? lv.data : "");
        CHECK(soft == 0 || soft == 1, "d3d%d answered %d to a yes-or-no question",
              which ? 12 : 11, soft);
        printf("  d3d%d: feature level %s, software %d\n", which ? 12 : 11,
               lv.data ? lv.data : "?", soft);
        daegun_text_free(level);
    }
#endif
    printf("  gpu backends exercised: %d\n", ran);
}

static void the_atlas_packer_packs(void)
{
    daegun_shelf_packer *p = daegun_shelf_packer_new(64, 64);
    CHECK(p != NULL, "a new packer is null");

    daegun_rect a = { 9, 9, 9, 9 }, b = { 9, 9, 9, 9 };
    CHECK(daegun_shelf_packer_insert(p, 16, 16, &a) == DAEGUN_OK, "the first insert failed");
    CHECK(a.w == 16 && a.h == 16, "a 16x16 request came back %zux%zu", a.w, a.h);
    CHECK(daegun_shelf_packer_insert(p, 16, 16, &b) == DAEGUN_OK, "the second insert failed");
    /* Two rectangles in one atlas must not overlap – the one thing a packer is for. */
    int disjoint = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
    CHECK(disjoint, "the packer overlapped (%zu,%zu %zux%zu) with (%zu,%zu %zux%zu)",
          a.x, a.y, a.w, a.h, b.x, b.y, b.w, b.h);
    CHECK(a.x + a.w <= 64 && a.y + a.h <= 64, "a rectangle ran outside the atlas");

    daegun_rect huge = { 0, 0, 0, 0 };
    CHECK(daegun_shelf_packer_insert(p, 65, 65, &huge) == DAEGUN_ABSENT,
          "a rectangle bigger than the atlas was placed");

    CHECK(daegun_shelf_packer_reset(p) == DAEGUN_OK, "reset failed");
    daegun_rect again = { 9, 9, 9, 9 };
    CHECK(daegun_shelf_packer_insert(p, 64, 64, &again) == DAEGUN_OK,
          "a full-size rectangle did not fit after a reset");
    CHECK(daegun_shelf_packer_insert(p, 1, 1, &again) == DAEGUN_ABSENT,
          "a 64x64 atlas holding a 64x64 rectangle still had room");

    CHECK(daegun_shelf_packer_insert(NULL, 1, 1, &again) == DAEGUN_NULL, "a null packer was used");
    daegun_shelf_packer_free(p);
    daegun_shelf_packer_free(NULL);
}

static void the_rules_a_caller_would_get_wrong(const char *path)
{
    daegun_font *font = open_font(path);
    if (!font) {
        return;
    }
    daegun_line_metrics lm;
    CHECK(daegun_font_line_metrics(font, false, &lm) == DAEGUN_OK, "line_metrics failed");
    double h = 0.0;
    CHECK(daegun_line_metrics_height(&lm, &h) == DAEGUN_OK, "line height failed");
    CHECK(h > 0.0, "line height is %g", h);
    CHECK(h == lm.ascent - lm.descent + lm.line_gap, "line height is not the documented formula");
    /* And it is NOT the sum, which is the mistake this call exists to prevent. */
    CHECK(h != lm.ascent + lm.descent + lm.line_gap || lm.descent == 0.0,
          "with a negative descent the sum and the difference must differ");

    int32_t may = -1;
    CHECK(daegun_hint_mode_may_autohint(DAEGUN_HINT_AUTO, &may) == DAEGUN_OK, "may_autohint failed");
    CHECK(may, "DAEGUN_HINT_AUTO may not autohint");
    CHECK(daegun_hint_mode_may_autohint(DAEGUN_HINT_AUTO_FORCE, &may) == DAEGUN_OK, "failed");
    CHECK(may, "DAEGUN_HINT_AUTO_FORCE may not autohint");
    CHECK(daegun_hint_mode_may_autohint(DAEGUN_HINT_NONE, &may) == DAEGUN_OK, "failed");
    CHECK(!may, "DAEGUN_HINT_NONE may autohint");
    CHECK(daegun_hint_mode_may_autohint(DAEGUN_HINT_CLASSIC, &may) == DAEGUN_OK, "failed");
    CHECK(!may, "DAEGUN_HINT_CLASSIC may autohint");

    int32_t g = -1, m = -1;
    CHECK(daegun_cluster_level_is_graphemes(DAEGUN_CLUSTER_MONOTONE_GRAPHEMES, &g) == DAEGUN_OK, "failed");
    CHECK(daegun_cluster_level_is_monotone(DAEGUN_CLUSTER_MONOTONE_GRAPHEMES, &m) == DAEGUN_OK, "failed");
    CHECK(g && m, "MONOTONE_GRAPHEMES is graphemes=%d monotone=%d", g, m);
    CHECK(daegun_cluster_level_is_graphemes(DAEGUN_CLUSTER_GRAPHEMES, &g) == DAEGUN_OK, "failed");
    CHECK(daegun_cluster_level_is_monotone(DAEGUN_CLUSTER_GRAPHEMES, &m) == DAEGUN_OK, "failed");
    CHECK(g && !m, "GRAPHEMES is graphemes=%d monotone=%d", g, m);
    CHECK(daegun_cluster_level_is_graphemes(DAEGUN_CLUSTER_CHARACTERS, &g) == DAEGUN_OK, "failed");
    CHECK(!g, "CHARACTERS groups by grapheme");

    daegun_font_free(font);
}

static void scripts_answer_about_themselves(void)
{
    uint16_t latin = 0xffff, arabic = 0xffff;
    daegun_u32_list *runs = NULL;
    CHECK(daegun_text_script_runs("Ab", &runs) == DAEGUN_OK, "script_runs failed for latin");
    size_t rn = 0;
    const uint32_t *rd = daegun_u32_list_data(runs, &rn);
    CHECK(rn >= 3, "a latin run came back as %zu numbers", rn);
    if (rn >= 3) {
        latin = (uint16_t)rd[2];
    }
    daegun_u32_list_free(runs);

    runs = NULL;
    CHECK(daegun_text_script_runs("\xD8\xA7\xD9\x84", &runs) == DAEGUN_OK,
          "script_runs failed for arabic");
    rd = daegun_u32_list_data(runs, &rn);
    CHECK(rn >= 3, "an arabic run came back as %zu numbers", rn);
    if (rn >= 3) {
        arabic = (uint16_t)rd[2];
    }
    daegun_u32_list_free(runs);
    CHECK(latin != arabic, "latin and arabic came back as the same script id %u", latin);

    daegun_str_list *tags = NULL;
    CHECK(daegun_script_opentype_tags(latin, &tags) == DAEGUN_OK, "opentype_tags failed");
    size_t n = 0;
    CHECK(daegun_str_list_count(tags, &n) == DAEGUN_OK, "counting tags failed");
    CHECK(n >= 1, "latin maps to %zu OpenType tags", n);
    if (n >= 1) {
        daegun_str t = { NULL, 0 };
        CHECK(daegun_str_list_at(tags, 0, &t) == DAEGUN_OK, "tag 0 failed");
        CHECK(t.len == 4, "an OpenType script tag is four characters, got %zu", t.len);
    }
    daegun_str_list_free(tags);

    int32_t ctx = -1;
    CHECK(daegun_script_is_context_dependent(latin, &ctx) == DAEGUN_OK, "failed");
    CHECK(!ctx, "latin takes its identity from its neighbors");
    CHECK(daegun_script_is_context_dependent(arabic, &ctx) == DAEGUN_OK, "failed");
    CHECK(!ctx, "arabic takes its identity from its neighbors");

    runs = NULL;
    CHECK(daegun_text_script_runs(",", &runs) == DAEGUN_OK, "script_runs failed for a comma");
    rd = daegun_u32_list_data(runs, &rn);
    if (rn >= 3) {
        CHECK(daegun_script_is_context_dependent((uint16_t)rd[2], &ctx) == DAEGUN_OK, "failed");
        CHECK(ctx, "a lone comma is not context dependent, so nothing is");
    }
    daegun_u32_list_free(runs);
}

static void subsetting_maps_glyph_ids(const char *path)
{
    daegun_font *font = open_font(path);
    if (!font) {
        return;
    }
    uint16_t a = 0, b = 0;
    daegun_font_glyph_id(font, 'A', &a);
    daegun_font_glyph_id(font, 'B', &b);
    const uint16_t keep[3] = { 0, a, b };

    daegun_subset *sub = NULL;
    daegun_status st = daegun_font_subset(font, keep, 3, NULL, 0, &sub);
    CHECK(st == DAEGUN_OK, "subset returned %d: %s", st, daegun_last_error().data);
    if (st == DAEGUN_OK) {
        uint16_t na = 0xffff, nb = 0xffff, notdef = 0xffff;
        CHECK(daegun_subset_new_gid(sub, 0, &notdef) == DAEGUN_OK, ".notdef was dropped");
        CHECK(notdef == 0, ".notdef moved to %u", notdef);
        CHECK(daegun_subset_new_gid(sub, a, &na) == DAEGUN_OK, "'A' was dropped from its own subset");
        CHECK(daegun_subset_new_gid(sub, b, &nb) == DAEGUN_OK, "'B' was dropped from its own subset");
        CHECK(na != nb, "'A' and 'B' both mapped to %u", na);

        uint16_t dropped_from = 0;
        for (uint16_t g = 1; g < 200; g++) {
            if (g != a && g != b) { dropped_from = g; break; }
        }
        uint16_t nd = 0xffff;
        st = daegun_subset_new_gid(sub, dropped_from, &nd);
        CHECK(st == DAEGUN_ABSENT || (st == DAEGUN_OK && nd != 0),
              "glyph %u was dropped but mapped to %u with status %d", dropped_from, nd, st);

        size_t len = 0;
        const uint8_t *ttf = daegun_subset_ttf(sub, &len);
        CHECK(ttf != NULL && len > 0, "the subset has no bytes");
        daegun_font *reopened = NULL;
        CHECK(daegun_font_open(ttf, len, &reopened) == DAEGUN_OK, "the subset does not open");
        daegun_font_free(reopened);
        daegun_subset_free(sub);
    }
    daegun_font_free(font);
}

static void stat_values_are_readable(const char *path)
{
    daegun_font *font = open_font(path);
    if (!font) {
        return;
    }
    daegun_stat *stat = NULL;
    if (daegun_font_stat_info(font, &stat) != DAEGUN_OK) {
        daegun_font_free(font);
        return;
    }
    size_t n = 0;
    CHECK(daegun_stat_value_count(stat, &n) == DAEGUN_OK, "value_count failed");
    CHECK(n > 0, "a face with a STAT table names no axis values");

    size_t combo_n = 0;
    const daegun_axis_value *combos = daegun_stat_combo_values(stat, &combo_n);
    CHECK(combos != NULL || combo_n == 0, "the combo list is null with a non-zero count");

    size_t named = 0;
    for (size_t i = 0; i < n; i++) {
        daegun_stat_value v;
        memset(&v, 0xff, sizeof v);
        CHECK(daegun_stat_value_at(stat, i, &v) == DAEGUN_OK, "value %zu failed", i);
        CHECK(v.kind >= DAEGUN_STAT_SINGLE && v.kind <= DAEGUN_STAT_COMBO,
              "value %zu has kind %d", i, v.kind);
        CHECK(v.elidable == 0 || v.elidable == 1, "elidable is %u", v.elidable);
        if (v.kind == DAEGUN_STAT_RANGE) {
            CHECK(v.min <= v.value && v.value <= v.max,
                  "range %zu is %g in [%g, %g]", i, v.value, v.min, v.max);
        }
        if (v.kind == DAEGUN_STAT_COMBO) {
            CHECK((size_t)v.combo_start + v.combo_count <= combo_n,
                  "combo %zu indexes %u..%u of %zu pairs",
                  i, v.combo_start, v.combo_start + v.combo_count, combo_n);
        } else {
            CHECK(v.combo_count == 0, "a non-combo value carries %u pairs", v.combo_count);
        }
        daegun_str name = { NULL, 0 };
        daegun_status ns = daegun_stat_value_name(stat, i, &name);
        CHECK(ns == DAEGUN_OK || ns == DAEGUN_ABSENT, "name %zu returned %d", i, ns);
        CHECK((ns == DAEGUN_OK) == (v.has_name != 0),
              "value %zu says has_name=%u and the name call returned %d", i, v.has_name, ns);
        if (ns == DAEGUN_OK) {
            CHECK(name.data != NULL && strlen(name.data) == name.len, "name %zu is malformed", i);
            named++;
        }
    }
    CHECK(named > 0, "a STAT table with %zu values names none of them", n);
    CHECK(daegun_stat_value_at(stat, n, NULL) == DAEGUN_NULL, "a null out-parameter was accepted");
    daegun_stat_free(stat);
    daegun_font_free(font);
}

static void the_subpixel_filter_answers_about_itself(void)
{
    daegun_subpixel_params sp;
    CHECK(daegun_subpixel_params_from_layout(DAEGUN_LAYOUT_RGB_H, &sp) == DAEGUN_OK, "failed");

    float dil[2] = { -1.0f, -1.0f };
    CHECK(daegun_subpixel_params_dilation(&sp, dil) == DAEGUN_OK, "dilation failed");
    CHECK(dil[0] >= 0.0f && dil[1] >= 0.0f, "dilation is (%g, %g)", (double)dil[0], (double)dil[1]);

    size_t pad[2] = { 999, 999 };
    CHECK(daegun_subpixel_params_pad(&sp, pad) == DAEGUN_OK, "pad failed");
    CHECK(pad[0] < 64 && pad[1] < 64, "pad is (%zu, %zu)", pad[0], pad[1]);

    daegun_subpixel_params ss;
    CHECK(daegun_subpixel_params_with_supersampling(&sp, 2, &ss) == DAEGUN_OK, "supersample failed");
    CHECK(ss.supersample == 2, "supersample is %u after asking for 2", ss.supersample);
    CHECK(ss.channels == sp.channels, "supersampling changed the channel count");

    uint64_t k1 = 0, k2 = 0, k3 = 0;
    CHECK(daegun_subpixel_layout_key(DAEGUN_LAYOUT_RGB_H, &k1) == DAEGUN_OK, "key failed");
    CHECK(daegun_subpixel_layout_key(DAEGUN_LAYOUT_RGB_H, &k2) == DAEGUN_OK, "key failed");
    CHECK(daegun_subpixel_layout_key(DAEGUN_LAYOUT_GRAYSCALE, &k3) == DAEGUN_OK, "key failed");
    CHECK(k1 == k2, "the same layout gave two keys");
    CHECK(k1 != k3, "RGB and grayscale share a cache key");

    float w[3 * 3] = { 0.25f, 0.5f, 0.25f, 0.25f, 0.5f, 0.25f, 0.25f, 0.5f, 0.25f };
    daegun_subpixel_params custom;
    CHECK(daegun_subpixel_params_from_weights(1, 1, 3, 1, -1, 0, w, &custom) == DAEGUN_OK,
          "a custom filter was refused");
    CHECK(custom.taps[0] == 3, "the custom filter has %u taps", custom.taps[0]);
    CHECK(daegun_subpixel_params_from_weights(0, 1, 3, 1, -1, 0, w, &custom) == DAEGUN_RANGE,
          "a zero oversample was accepted");
    CHECK(daegun_subpixel_params_from_weights(1, 1, 99, 1, -1, 0, w, &custom) == DAEGUN_RANGE,
          "a tap count past the shader's table was accepted");
}

static void device_profiles_come_from_every_api(void)
{
    daegun_device_profile *d3d = NULL, *mtl = NULL, *warp = NULL;
    CHECK(daegun_device_profile_from_d3d(0, 1, "a d3d adapter", &d3d) == DAEGUN_OK, "from_d3d failed");
    CHECK(daegun_device_profile_from_metal(1, "a metal device", &mtl) == DAEGUN_OK, "from_metal failed");
    CHECK(daegun_device_profile_from_d3d(1, -1, "WARP", &warp) == DAEGUN_OK, "from_d3d failed");

    int32_t soft = -1;
    CHECK(daegun_device_profile_is_software(warp, &soft) == DAEGUN_OK, "is_software failed");
    CHECK(soft, "an adapter declared software does not read as software");
    CHECK(daegun_device_profile_is_software(d3d, &soft) == DAEGUN_OK, "is_software failed");
    CHECK(!soft, "a hardware adapter reads as software");

    int32_t kind = -1;
    CHECK(daegun_device_profile_kind(mtl, &kind) == DAEGUN_OK, "kind failed");
    CHECK(kind == DAEGUN_DEVICE_INTEGRATED, "a UMA metal device is kind %d, not integrated", kind);

    daegun_policy pol;
    daegun_policy_default(&pol);
    pol.avoid_software_gpu = true;
    daegun_request req = { 24.0f, 0, 0, 0, 0, 0 };
    int32_t routed = -1;
    CHECK(daegun_route(DAEGUN_GPU_OK, &req, warp, &pol, &routed) == DAEGUN_OK, "route failed");
    CHECK(routed != DAEGUN_ROUTED_GPU, "avoid_software_gpu still routed to a WARP adapter");

    daegun_device_profile_free(d3d);
    daegun_device_profile_free(mtl);
    daegun_device_profile_free(warp);
}

static void arbitrary_geometry_goes_into_a_batch(void)
{
    daegun_batch *batch = NULL;
    CHECK(daegun_batch_new(&batch) == DAEGUN_OK, "batch_new failed");

    const float square[24] = {
        0.0f, 0.0f,  0.5f, 0.0f,  1.0f, 0.0f,
        1.0f, 0.0f,  1.0f, 0.5f,  1.0f, 1.0f,
        1.0f, 1.0f,  0.5f, 1.0f,  0.0f, 1.0f,
        0.0f, 1.0f,  0.0f, 0.5f,  0.0f, 0.0f,
    };
    float given[24];
    memcpy(given, square, sizeof square);

    daegun_glyph_slot slot;
    memset(&slot, 0, sizeof slot);
    CHECK(daegun_batch_append(batch, given, 4, &slot) == DAEGUN_OK, "append of four quads failed");
    CHECK(memcmp(given, square, sizeof square) == 0, "append wrote back through the caller's array");

    size_t nc = 0, nb = 0, nh = 0;
    const daegun_curve_point *curves = daegun_batch_curves(batch, &nc);
    const daegun_band *bands = daegun_batch_bands(batch, &nb);
    const daegun_hull_vertex *hulls = daegun_batch_hulls(batch, &nh);
    CHECK(curves != NULL && nc == 12, "four quads uploaded %zu curve points, wanted 12", nc);
    CHECK(bands != NULL && nb > 0, "it uploaded %zu bands", nb);
    CHECK(hulls != NULL && nh > 0, "it uploaded %zu hull vertices", nh);

    CHECK(slot.box_min[0] == 0.0f && slot.box_min[1] == 0.0f, "box_min is (%g,%g), wanted (0,0)",
          (double)slot.box_min[0], (double)slot.box_min[1]);
    CHECK(slot.box_max[0] == 1.0f && slot.box_max[1] == 1.0f, "box_max is (%g,%g), wanted (1,1)",
          (double)slot.box_max[0], (double)slot.box_max[1]);
    CHECK(slot.h_bands > 0 && slot.v_bands > 0, "the slot claims %ux%u bands",
          slot.h_bands, slot.v_bands);
    CHECK((size_t)slot.band_base + slot.h_bands + slot.v_bands <= nb,
          "the slot's bands run to %u of %zu", slot.band_base + slot.h_bands + slot.v_bands, nb);

    daegun_glyph_slot second;
    memset(&second, 0, sizeof second);
    CHECK(daegun_batch_append(batch, given, 4, &second) == DAEGUN_OK, "a second append failed");
    CHECK(second.band_base >= slot.band_base + slot.h_bands + slot.v_bands,
          "the second slot's bands start at %u, inside the first's", second.band_base);
    daegun_batch_curves(batch, &nc);
    CHECK(nc == 24, "two appends uploaded %zu curve points, wanted 24", nc);

    CHECK(daegun_batch_append(NULL, given, 4, &slot) == DAEGUN_NULL, "a null batch was accepted");
    CHECK(daegun_batch_append(batch, NULL, 4, &slot) == DAEGUN_NULL, "a null array was accepted");
    CHECK(daegun_batch_append(batch, given, 4, NULL) == DAEGUN_NULL, "a null out was accepted");
    CHECK(daegun_batch_append(batch, given, 0, &slot) != DAEGUN_OK, "an empty append reported success");

    daegun_batch_free(batch);
}

static void the_gpu_buffers_are_readable(const char *path)
{
    daegun_font *font = open_font(path);
    if (!font) {
        return;
    }
    daegun_batch *batch = NULL;
    CHECK(daegun_batch_new(&batch) == DAEGUN_OK, "batch_new failed");
    uint16_t gid = 0;
    daegun_font_glyph_id(font, 'g', &gid);
    daegun_glyph_slot slot;
    memset(&slot, 0, sizeof slot);
    CHECK(daegun_font_gpu_glyph(font, batch, gid, NULL, 0, &slot) == DAEGUN_OK, "gpu_glyph failed");

    size_t nc = 0, nb = 0, nh = 0, nbc = 0;
    const daegun_curve_point *curves = daegun_batch_curves(batch, &nc);
    const daegun_band *bands = daegun_batch_bands(batch, &nb);
    const daegun_hull_vertex *hulls = daegun_batch_hulls(batch, &nh);
    const uint32_t *band_curves = daegun_batch_band_curves(batch, &nbc);

    CHECK(curves != NULL && nc > 0, "a glyph with an outline uploaded %zu curve points", nc);
    CHECK(bands != NULL && nb > 0, "it uploaded %zu bands", nb);
    CHECK(hulls != NULL && nh > 0, "it uploaded %zu hull vertices", nh);
    CHECK(band_curves != NULL && nbc > 0, "it uploaded %zu band-curve indices", nbc);

    CHECK(slot.h_bands > 0 && slot.v_bands > 0, "the slot claims %ux%u bands",
          slot.h_bands, slot.v_bands);
    CHECK((size_t)slot.band_base + slot.h_bands + slot.v_bands <= nb,
          "the slot's bands run to %u of %zu", slot.band_base + slot.h_bands + slot.v_bands, nb);
    CHECK((size_t)slot.hull_base + 5 <= nh, "the slot's hull runs past the %zu vertices", nh);
    CHECK(slot.box_min[0] < slot.box_max[0] && slot.box_min[1] < slot.box_max[1],
          "the slot's box is (%g,%g)..(%g,%g)", (double)slot.box_min[0], (double)slot.box_min[1],
          (double)slot.box_max[0], (double)slot.box_max[1]);

    size_t inside = 0;
    for (size_t i = 0; i < nc; i++) {
        CHECK(curves[i].x == curves[i].x && curves[i].y == curves[i].y,
              "curve point %zu is NaN", i);
        if (curves[i].x >= slot.box_min[0] && curves[i].x <= slot.box_max[0]) {
            inside++;
        }
    }
    CHECK(inside > 0, "not one of %zu curve points falls inside the slot's own box", nc);

    for (size_t i = 0; i < nb; i++) {
        CHECK((size_t)bands[i].first_curve + bands[i].curve_count <= nbc,
              "band %zu indexes %u..%u of %zu", i, bands[i].first_curve,
              bands[i].first_curve + bands[i].curve_count, nbc);
    }
    for (size_t i = 0; i < nh; i++) {
        CHECK(hulls[i].pos[0] == hulls[i].pos[0], "hull vertex %zu is NaN", i);
    }

    daegun_color_slots *slots = NULL;
    if (daegun_font_gpu_color_glyph(font, batch, gid, NULL, 0, 0, &slots) == DAEGUN_OK) {
        size_t ns = 0;
        const daegun_color_slot *cs = daegun_color_slots_data(slots, &ns);
        CHECK(cs != NULL || ns == 0, "the color slot list is null with %zu entries", ns);
        for (size_t i = 0; i < ns; i++) {
            CHECK(cs[i].tint[3] >= 0.0f && cs[i].tint[3] <= 1.0f,
                  "color slot %zu has alpha %g", i, (double)cs[i].tint[3]);
            CHECK(cs[i].slot.h_bands > 0, "color slot %zu claims no bands", i);
        }
        daegun_color_slots_free(slots);
    }

    daegun_batch_free(batch);
    daegun_font_free(font);
}

static void an_owned_buffer_opens_without_a_copy(const char *path)
{
    size_t len = 0;
    uint8_t *file = slurp(path, &len);
    if (!file) { CHECK(0, "could not read %s", path); return; }

    uint8_t *buf = daegun_font_buffer_new(len);
    CHECK(buf != NULL, "buffer_new returned null for %zu bytes", len);
    memcpy(buf, file, len);

    daegun_font *font = NULL;
    daegun_status st = daegun_font_open_owned(buf, len, &font);
    CHECK(st == DAEGUN_OK, "open_owned returned %d: %s", st, daegun_last_error().data);
    if (st == DAEGUN_OK) {
        daegun_font *copied = NULL;
        CHECK(daegun_font_open(file, len, &copied) == DAEGUN_OK, "the copying open failed");
        uint16_t a = 0, b = 0, ga = 0, gb = 0;
        daegun_font_upm(font, &a);          daegun_font_upm(copied, &b);
        daegun_font_num_glyphs(font, &ga);  daegun_font_num_glyphs(copied, &gb);
        CHECK(a == b && ga == gb, "owned open gave upm %u/%u glyphs %u/%u", a, b, ga, gb);

        daegun_bytes h1 = { NULL, 0 }, h2 = { NULL, 0 };
        daegun_font_table(font, "head", &h1);
        daegun_font_table(copied, "head", &h2);
        CHECK(h1.len == h2.len && h1.len > 0 && memcmp(h1.data, h2.data, h1.len) == 0,
              "the owned font's head differs from the copied one's");
        daegun_font_free(copied);
        daegun_font_free(font);   /* takes the buffer with it */
    }

    uint8_t *unused = daegun_font_buffer_new(4096);
    CHECK(unused != NULL, "buffer_new(4096) returned null");
    unused[0] = 1; unused[4095] = 2;   /* writable to its full length */
    daegun_font_buffer_free(unused, 4096);
    daegun_font_buffer_free(NULL, 0);

    CHECK(daegun_font_buffer_new(0) == NULL, "a zero-length buffer was allocated");
    daegun_font *none = NULL;
    CHECK(daegun_font_open_owned(NULL, 10, &none) == DAEGUN_NULL, "a null buffer was accepted");

    uint8_t *junk = daegun_font_buffer_new(64);
    memset(junk, 0xff, 64);
    CHECK(daegun_font_open_owned(junk, 64, &none) == DAEGUN_PARSE, "junk parsed as a font");

    free(file);
}

int main(int argc, char **argv)
{
    const char *font = argc > 1 ? argv[1] : "assets/test-fonts/inter/InterVariable.ttf";

    printf("daegun C round trip\n");
    abi_version_agrees();
    null_is_refused_not_dereferenced();
    bad_font_data_is_reported();
    zeroed_options_are_the_defaults();
    ttc_count_answers_for_a_plain_font(font);
    a_real_font_answers(font);
    an_owned_buffer_opens_without_a_copy(font);
    metrics_answer_and_free_cleanly(font);
    a_subset_is_a_font(font);
    the_pen_draws_and_reentry_is_safe(font);
    rasterizing_produces_ink(font);
    shaping_produces_positioned_glyphs(font);
    layout_wraps_and_borrows(font);
    text_analysis_needs_no_font();
    drawing_picks_a_route(font);
    the_paint_graph_is_walkable("assets/test-fonts/colr-v1-test-glyphs/test_glyphs.ttf");
    math_constants_are_indexed("assets/test-fonts/stix-two-math/STIX2Math.otf");
    the_readers_are_bounds_checked();
    raw_tables_round_trip(font);
    paths_build_stroke_and_replay();
    character_properties_need_no_font();
    the_format_walkers_read_a_real_table(font);
    loca_and_glyf_are_walkable(font);
    the_shader_source_is_available();
    routing_decides_without_drawing();
    subpixel_params_come_from_a_layout();
    the_gpu_backends_draw(font);
    the_atlas_packer_packs();
    the_rules_a_caller_would_get_wrong(font);
    scripts_answer_about_themselves();
    subsetting_maps_glyph_ids(font);
    stat_values_are_readable(font);
    the_subpixel_filter_answers_about_itself();
    device_profiles_come_from_every_api();
    the_gpu_buffers_are_readable(font);
    arbitrary_geometry_goes_into_a_batch();

    if (failures == 0) {
        printf("  ok\n");
        return 0;
    }
    printf("  %d failed\n", failures);
    return 1;
}
