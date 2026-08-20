#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use alloc::vec::Vec;
use super::super::decoder::{read_u16_be, read_u32_be, write_u16_be, write_u32_be};
use super::super::format::feature_variations::FeatureVariations;

fn f2dot14_coords(location: &[f64]) -> Vec<i32> {
    location.iter().map(|&v| (v * 16384.0).round() as i32).collect()
}

pub(crate) fn resolve_feature_variations(gsub: &[u8], location: &[f64]) -> Option<Vec<u8>> {
    if gsub.len() < 14 || read_u16_be(gsub, 2)? < 1 || read_u32_be(gsub, 10)? == 0 {
        return None;
    }

    let feature_list = read_u16_be(gsub, 6)? as usize;
    let feature_count = read_u16_be(gsub, feature_list)? as usize;
    let table = FeatureVariations::parse(gsub)?;
    let coords = f2dot14_coords(location);

    let mut out = gsub.to_vec();
    if let Some(variation) = table.find(&coords) {
        for feature in 0..feature_count {
            let Some(alternate) = table.substitute(variation, u16::try_from(feature).ok()?)
            else {
                continue;
            };
            let rel = u16::try_from(alternate.checked_sub(feature_list)?).ok()?;
            write_u16_be(&mut out, feature_list + 2 + feature * 6 + 4, rel);
        }
    }

    write_u32_be(&mut out, 10, 0);
    Some(out)
}
