use alloc::vec::Vec;

use crate::daecore::daetype::colr_v1::{ColorStop, Paint as Colr};
use super::{
    resolve_stops, Blend, ClipShape, DisplayList, Extend, Gradient, GradientKind, Op, Paint, Rgba,
    Stop, Stops,
};
use crate::daecore::daetype::outline::{FillRule, Path};
use crate::daecore::daemachine::daemath::matrix::{concat, Matrix};

const MAX_DEPTH: usize = 64;

pub fn lower(
    paint: &Colr,
    ctm: Matrix,
    outline: &mut dyn FnMut(u16) -> Option<Path>,
    foreground: Rgba,
    out: &mut DisplayList,
) {
    let mut interned: alloc::vec::Vec<(u16, Option<crate::daecore::daetype::paint::PathId>)> =
        alloc::vec::Vec::new();
    lower_at(paint, ctm, outline, foreground, out, 0, &mut interned);
}

type Interned = alloc::vec::Vec<(u16, Option<crate::daecore::daetype::paint::PathId>)>;

fn lower_at(
    paint: &Colr,
    ctm: Matrix,
    outline: &mut dyn FnMut(u16) -> Option<Path>,
    fg: Rgba,
    out: &mut DisplayList,
    depth: usize,
    interned: &mut Interned,
) {
    if depth > MAX_DEPTH {
        return;
    }
    match paint {
        Colr::Layers(children) => {
            for c in children {
                lower_at(c, ctm, outline, fg, out, depth + 1, interned);
            }
        }

        Colr::Glyph { child, glyph_id } => {
            let entry = match interned.iter().find(|(g, _)| g == glyph_id) {
                Some(&(_, e)) => e,
                None => {
                    let e = match outline(*glyph_id) {
                        Some(path) if !path.is_empty() => Some(out.push_path(path)),
                        _ => None,
                    };
                    interned.push((*glyph_id, e));
                    e
                }
            };
            let Some(path_id) = entry else { return };
            if let Some(p) = leaf(child, fg, ctm) {
                out.push(Op::Fill { path: path_id, paint: p, rule: FillRule::NonZero, transform: ctm });
                return;
            }
            let mark = out.ops().len();
            out.push(Op::PushClip {
                shapes: alloc::vec![ClipShape { path: path_id, rule: FillRule::NonZero, transform: ctm }],
            });
            lower_at(child, ctm, outline, fg, out, depth + 1, interned);
            out.push(Op::PopClip);
            if !out.ops()[mark..].iter().any(|o| matches!(o, Op::Fill { .. })) {
                out.truncate(mark);
            }
        }

        Colr::ColrGlyph { child, .. } => lower_at(child, ctm, outline, fg, out, depth + 1, interned),

        Colr::Transform { child, matrix } => {
            lower_at(child, concat(matrix, &ctm), outline, fg, out, depth + 1, interned)
        }
        Colr::Translate { child, dx, dy } => {
            lower_at(child, concat(&[1.0, 0.0, 0.0, 1.0, *dx, *dy], &ctm), outline, fg, out, depth + 1, interned)
        }
        Colr::Scale { child, sx, sy, center } => {
            let m = about(*center, [*sx, 0.0, 0.0, *sy, 0.0, 0.0]);
            lower_at(child, concat(&m, &ctm), outline, fg, out, depth + 1, interned)
        }
        Colr::ScaleUniform { child, s, center } => {
            let m = about(*center, [*s, 0.0, 0.0, *s, 0.0, 0.0]);
            lower_at(child, concat(&m, &ctm), outline, fg, out, depth + 1, interned)
        }
        Colr::Rotate { child, angle, center } => {
            let (sin, cos) = crate::daecore::daemachine::float::sin_cos(angle * core::f64::consts::PI / 180.0);
            let m = about(*center, [cos, sin, -sin, cos, 0.0, 0.0]);
            lower_at(child, concat(&m, &ctm), outline, fg, out, depth + 1, interned)
        }
        Colr::Skew { child, x_angle, y_angle, center } => {
            let tan = |deg: f64| {
                let (s, c) = crate::daecore::daemachine::float::sin_cos(deg * core::f64::consts::PI / 180.0);
                if c == 0.0 { 0.0 } else { s / c }
            };
            let m = about(*center, [1.0, tan(*y_angle), tan(*x_angle), 1.0, 0.0, 0.0]);
            lower_at(child, concat(&m, &ctm), outline, fg, out, depth + 1, interned)
        }

        Colr::Composite { source, mode, backdrop } => {
            let mark = out.ops().len();
            out.push(Op::PushLayer { opacity: 1.0, blend: Blend::SrcOver });
            lower_at(backdrop, ctm, outline, fg, out, depth + 1, interned);

            let inner = out.ops().len();
            out.push(Op::PushLayer { opacity: 1.0, blend: Blend::from_colr(*mode) });
            lower_at(source, ctm, outline, fg, out, depth + 1, interned);
            out.push(Op::PopLayer);
            if out.ops().len() == inner + 2 && keeps_backdrop(Blend::from_colr(*mode)) {
                out.truncate(inner);
            }

            out.push(Op::PopLayer);
            if out.ops().len() == mark + 2 {
                out.truncate(mark);
            }
        }

        Colr::Solid { .. }
        | Colr::LinearGradient { .. }
        | Colr::RadialGradient { .. }
        | Colr::SweepGradient { .. } => {}
    }
}

fn keeps_backdrop(mode: Blend) -> bool {
    matches!(
        mode,
        Blend::SrcOver | Blend::DestOver | Blend::Dest | Blend::Plus | Blend::Screen
            | Blend::Overlay | Blend::Darken | Blend::Lighten | Blend::ColorDodge | Blend::ColorBurn
            | Blend::HardLight | Blend::SoftLight | Blend::Difference | Blend::Exclusion
            | Blend::Multiply | Blend::HslHue | Blend::HslSaturation | Blend::HslColor
            | Blend::HslLuminosity
    )
}

fn about(center: Option<(f64, f64)>, m: Matrix) -> Matrix {
    let Some((cx, cy)) = center else { return m };
    let to = [1.0, 0.0, 0.0, 1.0, -cx, -cy];
    let back = [1.0, 0.0, 0.0, 1.0, cx, cy];
    concat(&concat(&to, &m), &back)
}

fn leaf(paint: &Colr, fg: Rgba, ctm: Matrix) -> Option<Paint> {
    match paint {
        Colr::Solid { is_foreground, r, g, b, alpha } => {
            let base = if *is_foreground { fg } else { Rgba::opaque(*r, *g, *b) };
            Some(Paint::Solid(Rgba { a: scale(base.a, *alpha), ..base }))
        }

        // COLR's `Extend` is 0 pad, 1 repeat, 2 reflect – a different order from the enum in
        // paint.rs, which follows `spreadMethod`. Casting one to the other swaps repeat and reflect.
        Colr::LinearGradient { extend, stops, x0, y0, x1, y1, x2, y2 } => {
            let (px, py) = project((*x0, *y0), (*x1, *y1), (*x2, *y2))?;
            gradient(
                GradientKind::Linear { x0: *x0 as f32, y0: *y0 as f32, x1: px as f32, y1: py as f32 },
                stops, *extend, fg, ctm,
            )
        }

        Colr::RadialGradient { extend, stops, x0, y0, r0, x1, y1, r1 } => gradient(
            GradientKind::Radial {
                x0: *x0 as f32, y0: *y0 as f32, r0: *r0 as f32,
                x1: *x1 as f32, y1: *y1 as f32, r1: *r1 as f32,
            },
            stops, *extend, fg, ctm,
        ),

        Colr::SweepGradient { extend, stops, cx, cy, start_angle, end_angle } => gradient(
            GradientKind::Sweep {
                cx: *cx as f32, cy: *cy as f32,
                start_angle: *start_angle as f32, end_angle: *end_angle as f32,
            },
            stops, *extend, fg, ctm,
        ),

        _ => None,
    }
}

fn gradient(
    kind: GradientKind,
    stops: &[ColorStop],
    extend: u8,
    fg: Rgba,
    ctm: Matrix,
) -> Option<Paint> {
    let converted: Vec<Stop> = stops
        .iter()
        .map(|s| {
            let base = if s.is_foreground { fg } else { Rgba::opaque(s.r, s.g, s.b) };
            Stop { offset: s.offset as f32, color: Rgba { a: scale(base.a, s.alpha), ..base } }
        })
        .collect();
    match resolve_stops(converted) {
        Stops::Nothing => None,
        Stops::Solid(c) => Some(Paint::Solid(c)),
        Stops::Many(stops) => {
            Some(Paint::Gradient(Gradient { kind, stops, extend: spread(extend), transform: ctm }))
        }
    }
}

fn spread(extend: u8) -> Extend {
    match extend {
        1 => Extend::Repeat,
        2 => Extend::Reflect,
        _ => Extend::Pad,
    }
}

fn scale(a: u8, by: u8) -> u8 {
    ((u16::from(a) * u16::from(by) + 127) / 255) as u8
}

// A COLR linear gradient is three points, not two: `p2` is a rotation point, and the spec's own
// reduction is to project p0->p1 onto the line through p0 perpendicular to p0p2.
fn project(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) -> Option<(f64, f64)> {
    let v = (p1.0 - p0.0, p1.1 - p0.1);
    let r = (p2.0 - p0.0, p2.1 - p0.1);
    if (v.0 == 0.0 && v.1 == 0.0) || (r.0 == 0.0 && r.1 == 0.0) {
        return None;
    }
    let n = (-r.1, r.0);
    let denom = n.0 * n.0 + n.1 * n.1;
    let t = (v.0 * n.0 + v.1 * n.1) / denom;
    let p3 = (p0.0 + t * n.0, p0.1 + t * n.1);
    let len = (p3.0 - p0.0).abs() + (p3.1 - p0.1).abs();
    (len > 1e-12).then_some(p3)
}
