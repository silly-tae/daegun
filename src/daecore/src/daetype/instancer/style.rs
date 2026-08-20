#[cfg(all(not(feature = "std"), not(test)))]
use crate::daecore::daemachine::float::FloatExt;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use super::super::decoder::{parse_all_name_strings, parse_fvar_axes, read_fvar_instances, read_u16_be, write_u16_be};
use super::name_table::rewrite_name_table;
use super::stat_filter::filter_stat_to_instance;
use crate::daecore::daetype::TableBytes;

const FS_ITALIC: u16 = 0x0001;
const FS_BOLD: u16 = 0x0020;
const FS_REGULAR: u16 = 0x0040;
const FS_OBLIQUE: u16 = 0x0200;

const MAC_BOLD: u16 = 0x0001;
const MAC_ITALIC: u16 = 0x0002;

const BOLD_WEIGHT: f64 = 700.0;

const WIDTH_CLASSES: [(u16, f64); 9] = [
    (1, 50.0), (2, 62.5), (3, 75.0), (4, 87.5), (5, 100.0),
    (6, 112.5), (7, 125.0), (8, 150.0), (9, 200.0),
];

pub(crate) struct StyleTables {
    pub(crate) head: Option<Vec<u8>>,
    pub(crate) name: Option<Vec<u8>>,
    pub(crate) stat: Option<Vec<u8>>,
}

fn width_class_for(percentage: f64) -> u16 {
    WIDTH_CLASSES
        .iter()
        .min_by(|(_, a), (_, b)| {
            (a - percentage).abs().total_cmp(&(b - percentage).abs())
        })
        .map_or(5, |&(class, _)| class)
}

fn ribbi_name(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    }
}

fn non_ribbi_remainder(subfamily: &str) -> String {
    subfamily
        .split_whitespace()
        .filter(|word| !matches!(*word, "Bold" | "Italic" | "Regular" | "Oblique"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn effective_location(
    table_map: &BTreeMap<String, TableBytes>,
    axis_values: &[(String, f64)],
) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    if let Ok(axes) = parse_fvar_axes(table_map) {
        for axis in axes {
            let requested = axis_values.iter().find(|(tag, _)| *tag == axis.tag).map(|(_, v)| *v);
            let (lo, hi) = (axis.min.min(axis.max), axis.max.max(axis.min));
            out.insert(axis.tag, requested.unwrap_or(axis.default).clamp(lo, hi));
        }
    }
    out
}

fn matching_named_instance(
    table_map: &BTreeMap<String, TableBytes>,
    location: &BTreeMap<String, f64>,
) -> Option<(String, Option<String>)> {
    let instances = read_fvar_instances(table_map).ok()?;
    instances.into_iter().find_map(|instance| {
        let all_match = instance.coords.iter().all(|(tag, value)| {
            location.get(tag).is_some_and(|&coord| (coord - value).abs() < 1.0 / 65536.0)
        });
        match (all_match && !instance.coords.is_empty(), instance.name) {
            (true, Some(name)) => Some((name, instance.postscript_name)),
            _ => None,
        }
    })
}

pub(crate) fn apply_style_metadata(
    table_map: &BTreeMap<String, TableBytes>,
    axis_values: &[(String, f64)],
    os2_data: &mut [u8],
) -> StyleTables {
    let none = StyleTables { head: None, name: None, stat: None };
    if axis_values.is_empty() {
        return none;
    }

    let requested = |tag: &str| axis_values.iter().find(|(t, _)| t == tag).map(|(_, v)| *v);
    let location = effective_location(table_map, axis_values);

    let weight = requested("wght").filter(|_| location.contains_key("wght"));
    let width = requested("wdth").filter(|_| location.contains_key("wdth"));

    if let (Some(weight), true) = (weight, os2_data.len() >= 6) {
        write_u16_be(os2_data, 4, weight.round().clamp(0.0, 65535.0) as u16);
    }
    if let (Some(width), true) = (width, os2_data.len() >= 8) {
        write_u16_be(os2_data, 6, width_class_for(width));
    }

    let source_fs = if os2_data.len() >= 64 { read_u16_be(os2_data, 62).unwrap_or(0) } else { 0 };
    let bold = match location.get("wght") {
        Some(&w) => w >= BOLD_WEIGHT,
        None => source_fs & FS_BOLD != 0,
    };
    let oblique = match location.get("slnt") {
        Some(&s) => s != 0.0,
        None => source_fs & FS_OBLIQUE != 0,
    };
    let italic = match (location.get("ital"), location.get("slnt")) {
        (None, None) => source_fs & FS_ITALIC != 0,
        (ital, _) => ital.is_some_and(|&i| i >= 0.5) || oblique,
    };

    if os2_data.len() >= 64 {
        let version = read_u16_be(os2_data, 0).unwrap_or(0);
        let mut fs = source_fs;
        if bold { fs |= FS_BOLD; } else { fs &= !FS_BOLD; }
        if italic { fs |= FS_ITALIC; } else { fs &= !FS_ITALIC; }
        if version >= 4 {
            if oblique { fs |= FS_OBLIQUE; } else { fs &= !FS_OBLIQUE; }
        }
        if bold || italic { fs &= !FS_REGULAR; } else { fs |= FS_REGULAR; }
        write_u16_be(os2_data, 62, fs);
    }

    let head = table_map.get("head").filter(|h| h.len() >= 46).and_then(|h| {
        let mut out = h.to_owned_vec();
        let mut mac = read_u16_be(&out, 44)?;
        if bold { mac |= MAC_BOLD; } else { mac &= !MAC_BOLD; }
        if italic { mac |= MAC_ITALIC; } else { mac &= !MAC_ITALIC; }
        write_u16_be(&mut out, 44, mac);
        Some(out)
    });

    let name = rebuild_names(table_map, &location, bold, italic);
    let stat = table_map.get("STAT").map(|stat| {
        filter_stat_to_instance(stat, &location).unwrap_or_else(|| stat.to_owned_vec())
    });

    StyleTables { head, name, stat }
}

fn rebuild_names(
    table_map: &BTreeMap<String, TableBytes>,
    location: &BTreeMap<String, f64>,
    bold: bool,
    italic: bool,
) -> Option<Vec<u8>> {
    let name = table_map.get("name")?;
    let (subfamily, postscript) = matching_named_instance(table_map, location)?;

    let names = parse_all_name_strings(table_map);
    let family = names.get(&16).or_else(|| names.get(&1))?.clone();
    let ribbi = ribbi_name(bold, italic);
    let remainder = non_ribbi_remainder(&subfamily);

    let mut updates: Vec<(u16, String)> = Vec::new();
    let mut removals: Vec<u16> = Vec::new();

    if remainder.is_empty() {
        updates.push((1, family.clone()));
        updates.push((2, ribbi.to_string()));
        removals.extend([16, 17]);
    } else {
        updates.push((1, format!("{family} {remainder}")));
        updates.push((2, ribbi.to_string()));
        updates.push((16, family.clone()));
        updates.push((17, subfamily.clone()));
    }
    updates.push((4, format!("{family} {subfamily}")));
    if let Some(ps) = postscript {
        updates.push((6, ps));
    }

    rewrite_name_table(name, &updates, &removals)
}
