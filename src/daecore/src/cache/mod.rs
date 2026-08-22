#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use crate::daecore::sync::{mutable, read, write, Mutable, Shared};
use crate::daecore::daetype::decoder::{read_i16_be, read_u16_be, read_u32_be, write_u16_be};
use crate::daecore::daetype::subsetter::SubsetResult;
use crate::daecore::daetype::TableBytes;

pub(crate) mod color;
pub(crate) mod instance;
pub(crate) mod key;
pub(crate) mod metrics;
pub(crate) mod shaping;
pub mod subset;

pub use key::{canonical_axes, normalize_tag, AxisKey};
pub use metrics::LineMetrics;

type PlanCache = BTreeMap<Vec<u8>, Shared<crate::daecore::daeshaper::plan::ShapePlan>>;

type InstanceCache = BTreeMap<AxisKey, Shared<Vec<u8>>>;
type AdvanceCache = BTreeMap<(AxisKey, u16), u32>;
type AdvancesByAxis = BTreeMap<AxisKey, Shared<Vec<u32>>>;
type LocationByAxis = BTreeMap<AxisKey, Shared<Vec<f64>>>;
type AxisIntern = BTreeMap<AxisKey, Shared<AxisKey>>;
#[derive(Default)]
struct ShapeCache {
    runs: BTreeMap<ShapeKey, Shared<crate::daecore::text::shape::ShapedRun>>,
    bytes: usize,
}

// Keyed on `rtl` and on five codepoints of context either side, neither cosmetic. Without `rtl` a
// Common-script run – the case `shaped_run_directional` exists for – collides with its LTR twin.
// Five is the whole context, not a sample: `Buffer::CONTEXT_LENGTH` is five and the buffer
// discards the rest before shaping reads any.
type ShapeKey =
    (AxisKey, String, bool, Option<bool>, String, String, Option<String>, Option<String>, Option<u16>);

#[derive(Clone, Copy, Default, Debug)]
pub struct RunContext<'a> {
    pub rtl: Option<bool>,
    pub before: &'a str,
    pub after: &'a str,
    pub script: Option<&'a str>,
    pub language: Option<&'a str>,
    pub seed_script: Option<crate::daecore::daeshaper::unicode::Script>,
}

impl ShapeCache {
    // Both sides destructured rather than read through `.`, so a new field is a compile error here.
    // Two were once added to `ShapedRun` without touching this and the cache held roughly twice its
    // budget – an undercount has no symptom until memory runs out.
    fn cost(key: &ShapeKey, run: &crate::daecore::text::shape::ShapedRun) -> usize {
        let crate::daecore::text::shape::ShapedRun {
            glyphs, advances, offsets, unsafe_to_break, unsafe_to_concat, safe_to_insert_tatweel,
            clusters, complete: _, has_broken_syllable: _, shaper: _,
        } = run;
        let (_, text, _, _, before, after, script, language, _seed) = key;
        let opt = |s: &Option<String>| s.as_ref().map_or(0, String::len);
        text.len()
            + before.len()
            + after.len()
            + opt(script)
            + opt(language)
            + glyphs.len() * core::mem::size_of::<u16>()
            + advances.len() * core::mem::size_of::<f64>()
            + offsets.len() * core::mem::size_of::<(f64, f64)>()
            + unsafe_to_break.len() * core::mem::size_of::<bool>()
            + unsafe_to_concat.len() * core::mem::size_of::<bool>()
            + safe_to_insert_tatweel.len() * core::mem::size_of::<bool>()
            + clusters.len() * core::mem::size_of::<u32>()
    }
}
type InstancedCache = BTreeMap<AxisKey, Shared<FontCache>>;

// Bytes rather than entries, because an entry is not a unit of anything: one entry is a whole
// instanced font, and Inter costs 1,002,979 B per axis location where a CJK face costs an order of
// magnitude more. The old bound of 64 entries promised to hold 64 of either without saying so.
const INSTANCE_BUDGET_BYTES: usize = 64 * 1024 * 1024;
const ADVANCE_CACHE_CAP: usize = 16_384;
const ADVANCES_BY_AXIS_CAP: usize = 64;
const LOCATION_BY_AXIS_CAP: usize = 64;
const SHAPE_CACHE_BYTES: usize = 8 * 1024 * 1024;

const SHAPE_CACHE_ENTRY_MAX: usize = 64 * 1024;

const INDEX_BUDGET_BYTES: usize = 25_000_000;

const CMAP_INDEX_MAX_ENTRIES: usize = 200_000;

type HintSlot = (u16, u16, crate::daecore::daetype::hinting::HintMode, Option<crate::daecore::daetype::hinting::HintContext>);

type ColrScalarSlot = (Vec<f64>, Shared<Vec<f64>>);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutlineFormat {
    Cff,
    Glyf,
    Neither,
}

pub struct FontCache {
    pub table_map: BTreeMap<String, TableBytes>,
    upm: u16,
    num_glyphs: Option<u16>,
    os2: Option<crate::daecore::daetype::decoder::Os2Fields>,
    glyf: Option<crate::daecore::daetype::TableBytes>,
    cff: Option<crate::daecore::daetype::TableBytes>,
    pub outline_format: OutlineFormat,
    instance_cache: Mutable<InstanceCache>,
    instance_cache_bytes: crate::daecore::sync::Counter,
    instance_budget: crate::daecore::sync::Counter,
    adv_cache: Mutable<AdvanceCache>,
    advances_by_axis:          Mutable<AdvancesByAxis>,
    vertical_advances_by_axis: Mutable<AdvancesByAxis>,
    advances_by_axis_fu:          Mutable<AdvancesByAxis>,
    vertical_advances_by_axis_fu: Mutable<AdvancesByAxis>,
    location_by_axis: Mutable<LocationByAxis>,
    axis_intern: Mutable<AxisIntern>,
    default_axes: Shared<AxisKey>,
    shape_cache: Mutable<ShapeCache>,
    shape_budget: crate::daecore::sync::Counter,
    autohint_blues: Mutable<Option<crate::daecore::daetype::hinting::auto::BlueZones>>,
    outline_scratch: Mutable<Option<crate::daecore::daetype::instancer::GlyphCoords>>,
    spare_buffer: Mutable<Option<crate::daecore::daeshaper::buffer::Buffer>>,
    plan_cache: Mutable<PlanCache>,
    instanced_cache: Mutable<InstancedCache>,
    instanced_cache_bytes: crate::daecore::sync::Counter,
    loca_offsets_cache: Mutable<Option<Shared<Vec<usize>>>>,
    cff_outlines_cache: Mutable<Option<Option<Shared<crate::daecore::daetype::outline::CffOutlines>>>>,
    pub(crate) gdef_var_store: Mutable<Option<Shared<crate::daecore::daetype::format::ivs::ItemVariationStore>>>,
    colr_v1_var_data: Mutable<Option<Shared<crate::daecore::daetype::colr_v1::ColrV1VarData>>>,
    colr_v1_scalars: Mutable<Option<ColrScalarSlot>>,
    hint_context: Mutable<Option<HintSlot>>,
    autohinter: Mutable<Option<Option<crate::daecore::daetype::hinting::auto::AutoHinter>>>,
    lookup_digests: [Mutable<Vec<Option<crate::daecore::daeshaper::ot::digest::Digest>>>; 2],
    cmap_index: Mutable<Option<Option<Shared<crate::daecore::daetype::format::index::SparseIndex>>>>,
    index_budget: crate::daecore::sync::Counter,
    gdef_class_indexes: Mutable<GdefClassIndexes>,
    subtable_indexes: [Mutable<SubtableIndexSlots>; 2],
}

#[derive(Default)]
pub(crate) struct GdefClassIndexes {
    built: bool,
    glyph_classes: Option<Shared<crate::daecore::daetype::format::index::SparseIndex>>,
    mark_attach: Option<Shared<crate::daecore::daetype::format::index::SparseIndex>>,
}

type SubtableIndexSlots = Vec<Option<Shared<Vec<crate::daecore::daeshaper::ot::SubtableIndex>>>>;

impl FontCache {
    pub fn autohint_blues(&self) -> Option<crate::daecore::daetype::hinting::auto::BlueZones> {
        crate::daecore::sync::read(&self.autohint_blues).clone()
    }

    pub fn set_autohint_blues(&self, zones: crate::daecore::daetype::hinting::auto::BlueZones) {
        *crate::daecore::sync::write(&self.autohint_blues) = Some(zones);
    }

    pub fn try_autohint(
        &self,
        pts: &crate::daecore::daetype::hinting::auto::AutoPoints,
        ppem: u16,
    ) -> Option<Option<crate::daecore::daetype::hinting::HintedOutline>> {
        let mut slot = crate::daecore::sync::write(&self.autohinter);
        let built = slot.as_mut()?;
        Some(built.as_mut().map(|h| h.hint(pts, ppem)))
    }

    pub fn set_autohinter(&self, h: Option<crate::daecore::daetype::hinting::auto::AutoHinter>) {
        *crate::daecore::sync::write(&self.autohinter) = Some(h);
    }

    pub fn draw_glyf_reusing(
        &self,
        loca: &[usize],
        gid: u16,
        pen: &mut dyn crate::daecore::daetype::outline::OutlinePen,
    ) -> Result<(), alloc::string::String> {
        let Some(glyf) = self.glyf.as_ref() else {
            return Err(alloc::string::String::from("glyf: table missing"));
        };
        let mut scratch = crate::daecore::sync::write(&self.outline_scratch).take().unwrap_or_default();
        let drawn = crate::daecore::daetype::outline::outline_glyf_glyph_reusing_bytes(
            glyf, loca, gid, &mut scratch, pen,
        );
        *crate::daecore::sync::write(&self.outline_scratch) = Some(scratch);
        drawn
    }

    pub fn hint_glyph_cached(
        &self,
        glyf: &[u8],
        loca: &[usize],
        gid: u16,
        ppem: u16,
        upm: u16,
        mode: crate::daecore::daetype::hinting::HintMode,
    ) -> Option<crate::daecore::daetype::hinting::HintedOutline> {
        let mut slot = crate::daecore::sync::write(&self.hint_context);
        if !matches!(slot.as_ref(), Some((p, u, m, _)) if *p == ppem && *u == upm && *m == mode) {
            let built = crate::daecore::daetype::hinting::HintContext::new(&self.table_map, ppem, upm, mode);
            *slot = Some((ppem, upm, mode, built));
        }
        slot.as_mut()?.3.as_mut()?.hint_glyph(glyf, loca, gid, ppem, upm)
    }

    // Each is a ceiling the cache grows into, never a reservation, and each is dead weight for
    // some caller: a CPU-only app never fills the curve cache, nor a fixed-weight one the axes.
    pub fn set_shape_cache_bytes(&self, bytes: usize) {
        self.shape_budget.set(bytes);
        let mut cache = write(&self.shape_cache);
        if cache.bytes > bytes {
            cache.runs.clear();
            cache.bytes = 0;
        }
    }

    pub fn shape_cache_stats(&self) -> (usize, usize) {
        let cache = read(&self.shape_cache);
        (cache.runs.len(), cache.bytes)
    }

    pub fn clear_shape_cache(&self) {
        let mut cache = write(&self.shape_cache);
        cache.runs.clear();
        cache.bytes = 0;
    }

    pub fn set_instance_cache_bytes(&self, bytes: usize) {
        self.instance_budget.set(bytes);
    }

    pub fn instance_cache_stats(&self) -> (usize, usize) {
        (self.instance_cache_bytes.get(), self.instanced_cache_bytes.get())
    }

    // Unlike the others this is what is left to spend rather than a ceiling: index building draws
    // it down and never returns it, so setting it grants a fresh allowance.
    pub fn set_cmap_index_allowance(&self, bytes: usize) {
        self.index_budget.set(bytes);
    }

    pub fn cmap_index_allowance(&self) -> usize {
        self.index_budget.get()
    }

    pub fn new(table_map: BTreeMap<String, TableBytes>) -> Self {
        let outline_format = if table_map.contains_key("CFF ") {
            OutlineFormat::Cff
        } else if table_map.contains_key("glyf") {
            OutlineFormat::Glyf
        } else {
            OutlineFormat::Neither
        };
        let upm = table_map
            .get("head")
            .filter(|h| h.len() >= 20)
            .and_then(|h| crate::daecore::daetype::decoder::read_u16_be(h, 18))
            .filter(|&v| v > 0)
            .unwrap_or(2048);
        let num_glyphs = table_map
            .get("maxp")
            .and_then(|m| crate::daecore::daetype::decoder::read_u16_be(m, 4));
        let os2 = crate::daecore::daetype::decoder::parse_os2(&table_map);
        let glyf = table_map.get("glyf").cloned();
        let cff = table_map.get("CFF ").cloned();
        Self {
            table_map,
            upm,
            num_glyphs,
            os2,
            glyf,
            cff,
            outline_format,
            instance_cache: mutable(BTreeMap::new()),
            instance_cache_bytes: crate::daecore::sync::Counter::new(0),
            instance_budget: crate::daecore::sync::Counter::new(INSTANCE_BUDGET_BYTES),
            adv_cache: mutable(BTreeMap::new()),
            advances_by_axis: mutable(BTreeMap::new()),
            vertical_advances_by_axis: mutable(BTreeMap::new()),
            advances_by_axis_fu: mutable(BTreeMap::new()),
            vertical_advances_by_axis_fu: mutable(BTreeMap::new()),
            location_by_axis: mutable(BTreeMap::new()),
            axis_intern: mutable(BTreeMap::new()),
            default_axes: Shared::new(AxisKey::default()),
            shape_cache: mutable(ShapeCache::default()),
            shape_budget: crate::daecore::sync::Counter::new(SHAPE_CACHE_BYTES),
            autohint_blues: mutable(None),
            outline_scratch: mutable(None),
            spare_buffer: mutable(None),
            plan_cache: mutable(PlanCache::default()),
            instanced_cache: mutable(BTreeMap::new()),
            instanced_cache_bytes: crate::daecore::sync::Counter::new(0),
            loca_offsets_cache: mutable(None),
            cff_outlines_cache: mutable(None),
            gdef_var_store: mutable(None),
            colr_v1_var_data: mutable(None),
            colr_v1_scalars: mutable(None),
            hint_context: mutable(None),
            autohinter: mutable(None),
            lookup_digests: [mutable(Vec::new()), mutable(Vec::new())],
            cmap_index: mutable(None),
            index_budget: crate::daecore::sync::Counter::new(INDEX_BUDGET_BYTES),
            gdef_class_indexes: mutable(GdefClassIndexes::default()),
            subtable_indexes: [mutable(Vec::new()), mutable(Vec::new())],
        }
    }
}
