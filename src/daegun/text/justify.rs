use alloc::string::String;
use alloc::vec::Vec;

use crate::daecore::daetype::jstf::{jstf_extender_glyphs, jstf_priorities, JstfModLists};
use crate::cache::FontCache;
use crate::text::shape::ShapedRun;

#[derive(Debug, Clone)]
pub struct Justified {
    pub run: ShapedRun,
    pub level: Option<usize>,
    pub shrink: bool,
    pub width: f64,
    pub best_effort: bool,
}

fn width_of(run: &ShapedRun) -> f64 {
    run.advances.iter().sum()
}

#[derive(Debug, Clone, Copy)]
pub struct JustifyOptions<'a> {
    pub script_tag: &'a str,
    pub lang_sys_tag: Option<&'a str>,
    pub target_width: f64,
    pub tolerance: f64,
}

pub(crate) fn justify_line(
    fc: &FontCache,
    axis_values: &[(String, f64)],
    text: &str,
    vertical: bool,
    opts: &JustifyOptions,
) -> Option<Justified> {
    let (target_width, tolerance) = (opts.target_width, opts.tolerance);
    let natural = fc.shaped_run(axis_values, text, vertical)?;
    let natural_width = width_of(&natural);

    if (natural_width - target_width).abs() <= tolerance {
        return Some(Justified {
            run: (*natural).clone(),
            level: None,
            shrink: false,
            width: natural_width,
            best_effort: false,
        });
    }

    let shrink = natural_width > target_width;

    let levels: Vec<JstfModLists> =
        jstf_priorities(&fc.table_map, opts.script_tag, opts.lang_sys_tag).unwrap_or_default();

    let mut best = Justified {
        run: (*natural).clone(),
        level: None,
        shrink,
        width: natural_width,
        best_effort: true,
    };
    let mut best_error = (natural_width - target_width).abs();

    for (i, mods) in levels.iter().enumerate() {
        let Some(run) = fc.shaped_run_justified(axis_values, text, vertical, mods, shrink) else {
            continue;
        };
        let w = width_of(&run);
        let error = (w - target_width).abs();
        if error < best_error {
            best_error = error;
            best = Justified { run: run.clone(), level: Some(i), shrink, width: w, best_effort: true };
        }
        if error <= tolerance {
            return Some(Justified { run, level: Some(i), shrink, width: w, best_effort: false });
        }
    }

    Some(best)
}

pub(crate) fn extender_glyphs(fc: &FontCache, script_tag: &str) -> Vec<u16> {
    jstf_extender_glyphs(&fc.table_map, script_tag).unwrap_or_default()
}
