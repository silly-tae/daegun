use super::*;
use crate::text::shape::ShapeOptions;

impl Font {
    pub fn shape_bidi(&self, text: &str, axes: &[(&str, f64)], base: Option<bool>) -> Option<Vec<BidiRun>> {
        self.shape_bidi_with(text, axes, base, &ShapeOptions::default())
    }

    pub fn shape_bidi_with(
        &self,
        text: &str,
        axes: &[(&str, f64)],
        base: Option<bool>,
        opts: &ShapeOptions,
    ) -> Option<Vec<BidiRun>> {
        let chars: Vec<char> = text.chars().collect();
        let para = crate::daeshaper::unicode::bidi::resolve(text, base);
        let mut out = Vec::new();
        let runs = crate::daeshaper::unicode::bidi::visual_runs(&para);
        for (indices, level) in runs.iter() {
            let mut logical = indices.to_vec();
            logical.sort_unstable();
            let slice: String = logical.iter().filter_map(|&i| chars.get(i)).collect();
            if slice.is_empty() { continue; }
            let (lo, hi) = (logical[0], logical[logical.len() - 1]);
            let before: String = chars[..lo].iter().collect();
            let after: String = chars[hi + 1..].iter().collect();
            // Characters shape in *logical* order even though the runs are ordered visually –
            // reversing the text too would double-reverse an RTL run – and each run is told what
            // logically surrounds it, or joining would stop at every run edge.
            let run = crate::text::shape::shape_run_directional_with_options(
                &self.cache,
                &owned_axes(axes),
                &slice,
                false,
                !level.is_multiple_of(2),
                &ShapeOptions { before: &before, after: &after, ..*opts },
            )?;
            out.push(BidiRun { run, level, chars: logical });
        }
        Some(out)
    }

    // Shared, not owned: the run is already behind a refcount in the cache, and handing back a copy
    // meant every hit cloned seven vectors for a caller that almost always only reads them.
    pub fn shape(&self, text: &str, axes: &[(&str, f64)], vertical: bool) -> Option<Shared<ShapedRun>> {
        self.cache.shaped_run(axes, text, vertical)
    }

    pub fn shape_with_language(&self, text: &str, axes: &[(&str, f64)], vertical: bool, language: &str) -> Option<ShapedRun> {
        crate::text::shape::shape_run_with_language(&self.cache, &owned_axes(axes), text, vertical, language)
    }

    pub fn shape_with_features(&self, text: &str, axes: &[(&str, f64)], vertical: bool, script: Option<&str>, features: &[(&str, u32)]) -> Option<ShapedRun> {
        let owned: Vec<(String, u32)> =
            features.iter().map(|(t, v)| ((*t).to_string(), *v)).collect();
        crate::text::shape::shape_run_with_user_features(&self.cache, &owned_axes(axes), text, vertical, script, &owned)
    }

    pub fn shape_with_options(
        &self,
        text: &str,
        axes: &[(&str, f64)],
        vertical: bool,
        opts: &ShapeOptions,
    ) -> Option<ShapedRun> {
        crate::text::shape::shape_run_with_options(&self.cache, &owned_axes(axes), text, vertical, opts)
    }

    pub fn shape_justified(&self, text: &str, axes: &[(&str, f64)], vertical: bool, mods: &JstfModLists, shrink: bool) -> Option<ShapedRun> {
        self.cache.shaped_run_justified(&owned_axes(axes), text, vertical, mods, shrink)
    }

    pub fn justify(
        &self,
        text: &str,
        axes: &[(&str, f64)],
        vertical: bool,
        opts: &JustifyOptions,
    ) -> Option<Justified> {
        crate::text::justify::justify_line(&self.cache, &owned_axes(axes), text, vertical, opts)
    }

    pub fn justification_extenders(&self, script_tag: &str) -> Vec<u16> {
        crate::text::justify::extender_glyphs(&self.cache, script_tag)
    }

    pub fn layout(&self, text: &str, axes: &[(&str, f64)], opts: &LayoutOptions) -> Option<TextLayout> {
        crate::text::layout::layout_text(&self.cache, &owned_axes(axes), text, opts)
    }

    pub fn measure_width(&self, text: &str, axes: &[(&str, f64)], font_size: f64) -> f64 {
        crate::text::width::string_width_pt(text, &self.cache, &owned_axes(axes), font_size)
    }
}
