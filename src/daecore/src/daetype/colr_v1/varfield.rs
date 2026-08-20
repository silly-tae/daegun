use super::super::format::ivs::{compute_ivs_delta_f64, ItemVariationStore};
use super::Colrv1Ctx;

const NO_VARIATION_INDEX: u32 = 0xFFFF_FFFF;

#[derive(Clone, Copy)]
pub(super) enum VarField {
    Raw,
    RawU16,
    F2Dot14,
    Fixed1616,
}

impl VarField {
    fn divisor(self) -> f64 {
        match self {
            VarField::Raw | VarField::RawU16 => 1.0,
            VarField::F2Dot14 => 16384.0,
            VarField::Fixed1616 => 65536.0,
        }
    }

    fn clamp(self, v: i32) -> i32 {
        match self {
            VarField::Raw | VarField::F2Dot14 => v.clamp(i16::MIN as i32, i16::MAX as i32),
            VarField::RawU16 => v.clamp(0, u16::MAX as i32),
            VarField::Fixed1616 => v,
        }
    }
}

pub(super) fn resolve_var_field(
    raw_base: i32, field: VarField, var_index_base: u32, field_position: u32, ctx: &Colrv1Ctx,
) -> f64 {
    let raw = raw_base as f64 + resolve_var_delta(var_index_base, field_position, ctx);
    f64::from(field.clamp(super::super::format::round::ot_round(raw))) / field.divisor()
}

pub(super) fn resolve_var_angle(
    raw_base: i16, bias: f64, var_index_base: u32, field_position: u32, ctx: &Colrv1Ctx,
) -> f64 {
    let f2dot14 = resolve_var_field(raw_base as i32, VarField::F2Dot14, var_index_base, field_position, ctx);
    (f2dot14 + bias) * 180.0
}

pub(super) fn decode_angle(raw: i16, bias: f64) -> f64 {
    (raw as f64 / 16384.0 + bias) * 180.0
}

fn resolve_var_delta(var_index_base: u32, field_position: u32, ctx: &Colrv1Ctx) -> f64 {
    resolve_delta_raw(ctx.var_store, ctx.var_index_map, ctx.region_scalars, var_index_base, field_position)
}

pub(super) fn resolve_delta_raw(
    var_store: Option<&ItemVariationStore>,
    var_index_map: Option<&[(u32, u32)]>,
    region_scalars: &[f64],
    var_index_base: u32,
    field_position: u32,
) -> f64 {
    if var_index_base == NO_VARIATION_INDEX { return 0.0; }
    let Some(store) = var_store else { return 0.0 };
    let idx = var_index_base as usize + field_position as usize;
    let (outer, inner) = match var_index_map {
        Some(map) => super::super::format::ivs::delta_set_index_map_lookup(map, idx),
        None => (0, idx),
    };
    compute_ivs_delta_f64(store, outer, inner, region_scalars)
}
