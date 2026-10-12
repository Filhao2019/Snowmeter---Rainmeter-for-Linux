//! Geometry of shapes as cairo paths, bounds, transforms and boolean combine.

use super::types::*;
use crate::render::geom::arc_to;
use cairo::Matrix;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

pub type Rect4 = (f64, f64, f64, f64);

/// A scratch drawing context for measuring paths.
pub fn scratch() -> cairo::Context {
    let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).expect("scratch surface");
    let cr = cairo::Context::new(&surf).expect("scratch context");
    cr.set_tolerance(0.05);
    cr
}

fn ellipse_path(cr: &cairo::Context, cx: f64, cy: f64, rx: f64, ry: f64) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    let m = cr.matrix();
    cr.translate(cx, cy);
    cr.scale(rx, ry);
    cr.new_sub_path();
    cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);
    cr.close_path();
    cr.set_matrix(m);
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64) {
    let rx = rx.abs().min(w.abs() / 2.0);
    let ry = ry.abs().min(h.abs() / 2.0);
    if rx <= 0.0 || ry <= 0.0 {
        cr.rectangle(x, y, w, h);
        return;
    }
    let m = cr.matrix();
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    // Corners are quarter ellipses.
    let corner = |cx: f64, cy: f64, a0: f64, a1: f64| {
        let m2 = cr.matrix();
        cr.translate(cx, cy);
        cr.scale(rx, ry);
        cr.arc(0.0, 0.0, 1.0, a0, a1);
        cr.set_matrix(m2);
    };
    corner(x + w - rx, y + ry, -pi / 2.0, 0.0);
    corner(x + w - rx, y + h - ry, 0.0, pi / 2.0);
    corner(x + rx, y + h - ry, pi / 2.0, pi);
    corner(x + rx, y + ry, pi, 1.5 * pi);
    cr.close_path();
    cr.set_matrix(m);
}

fn quad_to(cr: &cairo::Context, from: (f64, f64), c: (f64, f64), to: (f64, f64)) {
    let c1 = (from.0 + 2.0 / 3.0 * (c.0 - from.0), from.1 + 2.0 / 3.0 * (c.1 - from.1));
    let c2 = (to.0 + 2.0 / 3.0 * (c.0 - to.0), to.1 + 2.0 / 3.0 * (c.1 - to.1));
    cr.curve_to(c1.0, c1.1, c2.0, c2.1, to.0, to.1);
}

/// Append the geometry to the current path. With `stroked_only`, segments
/// flagged `SetNoStroke` are skipped (the stroke of a path).
pub fn build_path(cr: &cairo::Context, g: &Geom, stroked_only: bool) {
    match g {
        Geom::None => {}
        Geom::Rect { x, y, w, h, rx, ry } => rounded_rect(cr, *x, *y, *w, *h, *rx, *ry),
        Geom::Ellipse { cx, cy, rx, ry } => ellipse_path(cr, *cx, *cy, *rx, *ry),
        Geom::Line { x1, y1, x2, y2 } => {
            cr.move_to(*x1, *y1);
            cr.line_to(*x2, *y2);
        }
        Geom::Arc { x1, y1, x2, y2, rx, ry, angle, clockwise, large, closed } => {
            cr.move_to(*x1, *y1);
            arc_to(cr, (*x1, *y1), (*x2, *y2), *rx, *ry, *angle, *large, *clockwise);
            if *closed {
                cr.close_path();
            }
        }
        Geom::Curve { x1, y1, x2, y2, c1, c2, closed } => {
            cr.move_to(*x1, *y1);
            match c2 {
                Some(c2) => cr.curve_to(c1.0, c1.1, c2.0, c2.1, *x2, *y2),
                None => quad_to(cr, (*x1, *y1), *c1, (*x2, *y2)),
            }
            if *closed {
                cr.close_path();
            }
        }
        Geom::Path { start, segs, closed, .. } => {
            let mut cur = *start;
            let mut pen_down = false;
            let mut begin = |cr: &cairo::Context, at: (f64, f64), pen_down: &mut bool| {
                if !*pen_down {
                    cr.move_to(at.0, at.1);
                    *pen_down = true;
                }
            };
            if !stroked_only {
                cr.move_to(start.0, start.1);
                pen_down = true;
            }
            for s in segs {
                let skip = stroked_only && s.no_stroke;
                let end = match &s.seg {
                    Seg::Line { x, y } => (*x, *y),
                    Seg::Arc { x, y, .. } | Seg::Quad { x, y, .. } | Seg::Cubic { x, y, .. } => (*x, *y),
                };
                if skip {
                    pen_down = false;
                    cur = end;
                    continue;
                }
                begin(cr, cur, &mut pen_down);
                match &s.seg {
                    Seg::Line { x, y } => cr.line_to(*x, *y),
                    Seg::Arc { x, y, rx, ry, angle, clockwise, large } => {
                        arc_to(cr, cur, (*x, *y), *rx, *ry, *angle, *large, *clockwise)
                    }
                    Seg::Quad { x, y, cx, cy } => quad_to(cr, cur, (*cx, *cy), (*x, *y)),
                    Seg::Cubic { x, y, c1, c2 } => cr.curve_to(c1.0, c1.1, c2.0, c2.1, *x, *y),
                }
                cur = end;
            }
            if *closed && !(stroked_only && segs.iter().any(|s| s.no_stroke)) {
                cr.close_path();
            }
        }
        Geom::Polys(polys) => {
            for p in polys {
                if let Some(first) = p.first() {
                    cr.move_to(first.0, first.1);
                    for pt in &p[1..] {
                        cr.line_to(pt.0, pt.1);
                    }
                    cr.close_path();
                }
            }
        }
    }
}

fn union_rect(a: Option<Rect4>, b: Option<Rect4>) -> Option<Rect4> {
    match (a, b) {
        (None, r) | (r, None) => r,
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))),
    }
}

fn nonempty(r: (f64, f64, f64, f64)) -> Option<Rect4> {
    (r.2 > r.0 || r.3 > r.1).then_some(r)
}

/// Untransformed geometry bounds `(left, top, right, bottom)`.
pub fn geometry_bounds(g: &Geom) -> Option<Rect4> {
    let cr = scratch();
    build_path(&cr, g, false);
    let e = cr.path_extents().ok()?;
    nonempty(e)
}

/// Geometry bounds widened by half the stroke width: the reference box for
/// transform anchors and gradients.
pub fn widened_bounds(s: &ShapeDef) -> Rect4 {
    let Some(b) = geometry_bounds(&s.geom) else { return (0.0, 0.0, 0.0, 0.0) };
    let h = s.stroke_width / 2.0;
    (b.0 - h, b.1 - h, b.2 + h, b.3 + h)
}

/// The shape matrix `M` (points are transformed by it, in `TransformOrder`).
pub fn shape_matrix(s: &ShapeDef) -> Matrix {
    let t = &s.transforms;
    let b = widened_bounds(s);
    let center = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    let anchor = |a: Option<(f64, f64)>| match a {
        Some((ax, ay)) => (b.0 + ax, b.1 + ay),
        None => center,
    };
    let about = |m: Matrix, a: (f64, f64)| {
        let to = Matrix::new(1.0, 0.0, 0.0, 1.0, -a.0, -a.1);
        let back = Matrix::new(1.0, 0.0, 0.0, 1.0, a.0, a.1);
        Matrix::multiply(&Matrix::multiply(&to, &m), &back)
    };
    let mut total = Matrix::identity();
    for k in &t.order {
        let step = match k {
            TransformKind::Rotate => t.rotate.map(|(deg, a)| {
                let (sn, cs) = deg.to_radians().sin_cos();
                about(Matrix::new(cs, sn, -sn, cs, 0.0, 0.0), anchor(a))
            }),
            TransformKind::Scale => t.scale.map(|((sx, sy), a)| about(Matrix::new(sx, 0.0, 0.0, sy, 0.0, 0.0), anchor(a))),
            TransformKind::Skew => t.skew.map(|((ax, ay), a)| {
                about(Matrix::new(1.0, ay.to_radians().tan(), ax.to_radians().tan(), 1.0, 0.0, 0.0), anchor(a))
            }),
            TransformKind::Offset => t.offset.map(|(dx, dy)| Matrix::new(1.0, 0.0, 0.0, 1.0, dx, dy)),
        };
        if let Some(m) = step {
            total = Matrix::multiply(&total, &m);
        }
    }
    total
}

pub fn cairo_cap(c: Cap) -> cairo::LineCap {
    match c {
        Cap::Flat => cairo::LineCap::Butt,
        Cap::Round => cairo::LineCap::Round,
        Cap::Square | Cap::Triangle => cairo::LineCap::Square,
    }
}

/// Apply the stroke style (width, caps, join, dashes) to `cr`.
pub fn apply_stroke_style(cr: &cairo::Context, s: &ShapeDef) {
    let w = s.stroke_width;
    cr.set_line_width(w);
    let cap = if s.dashes.is_empty() { s.start_cap } else { s.dash_cap };
    cr.set_line_cap(cairo_cap(cap));
    cr.set_line_join(match s.join {
        Join::Miter | Join::MiterOrBevel => cairo::LineJoin::Miter,
        Join::Bevel => cairo::LineJoin::Bevel,
        Join::Round => cairo::LineJoin::Round,
    });
    cr.set_miter_limit((s.miter_limit / 2.0).max(1.0));
    if s.dashes.is_empty() {
        cr.set_dash(&[], 0.0);
    } else {
        let mut d: Vec<f64> = s.dashes.iter().map(|v| (v * w).max(0.0)).collect();
        if d.len() % 2 == 1 {
            let again = d.clone();
            d.extend(again);
        }
        if d.iter().all(|v| *v <= 0.0) {
            cr.set_dash(&[], 0.0);
        } else {
            cr.set_dash(&d, s.dash_offset * w);
        }
    }
}

/// Bounds `(left, top, right, bottom)` of a shape for auto sizing: stroke
/// area and fill area, transformed by `M`.
pub fn drawn_bounds(s: &ShapeDef) -> Option<Rect4> {
    let m = shape_matrix(s);
    let cr = scratch();
    cr.transform(m);
    build_path(&cr, &s.geom, false);
    let fill = cr.fill_extents().ok().and_then(nonempty);
    let mut total = fill;
    if s.stroke_width > 0.0 {
        let stroke = if s.custom_stroke {
            let path = cr.copy_path().ok()?;
            cr.set_matrix(Matrix::identity());
            cr.new_path();
            cr.append_path(&path);
            apply_stroke_style(&cr, s);
            cr.stroke_extents().ok()
        } else {
            apply_stroke_style(&cr, s);
            cr.stroke_extents().ok()
        };
        total = union_rect(total, stroke.and_then(nonempty));
    }
    total
}

/// Point-in-shape test (fill or stroke) in shape coordinates.
pub fn contains_point(s: &ShapeDef, x: f64, y: f64) -> bool {
    let m = shape_matrix(s);
    let cr = scratch();
    cr.transform(m);
    build_path(&cr, &s.geom, false);
    if cr.in_fill(x, y).unwrap_or(false) {
        return true;
    }
    if s.stroke_width > 0.0 {
        apply_stroke_style(&cr, s);
        if cr.in_stroke(x, y).unwrap_or(false) {
            return true;
        }
    }
    false
}

pub fn fill_rule(g: &Geom) -> cairo::FillRule {
    match g {
        Geom::Path { winding: true, .. } => cairo::FillRule::Winding,
        _ => cairo::FillRule::EvenOdd,
    }
}

/// Flattened contours of a shape with `M` baked in.
pub fn flatten(s: &ShapeDef) -> Vec<Vec<[f64; 2]>> {
    let cr = scratch();
    cr.transform(shape_matrix(s));
    build_path(&cr, &s.geom, false);
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    let Ok(path) = cr.copy_path_flat() else { return out };
    for seg in path.iter() {
        match seg {
            cairo::PathSegment::MoveTo((x, y)) => {
                if cur.len() >= 3 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                cur.push([x, y]);
            }
            cairo::PathSegment::LineTo((x, y)) => cur.push([x, y]),
            cairo::PathSegment::ClosePath => {
                if cur.len() >= 3 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
            cairo::PathSegment::CurveTo(..) => {}
        }
    }
    if cur.len() >= 3 {
        out.push(cur);
    }
    out
}

fn rule(m: CombineMode) -> OverlayRule {
    match m {
        CombineMode::Union => OverlayRule::Union,
        CombineMode::Xor => OverlayRule::Xor,
        CombineMode::Intersect => OverlayRule::Intersect,
        CombineMode::Exclude => OverlayRule::Difference,
    }
}

/// `a (mode) b` on contour sets, even-odd filled.
pub fn combine(a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>], mode: CombineMode) -> Vec<Vec<[f64; 2]>> {
    let subj: Vec<Vec<[f64; 2]>> = a.to_vec();
    let clip: Vec<Vec<[f64; 2]>> = b.to_vec();
    let shapes = subj.overlay(&clip, rule(mode), FillRule::EvenOdd);
    shapes.into_iter().flatten().collect()
}

pub fn polys_of(contours: Vec<Vec<[f64; 2]>>) -> Geom {
    Geom::Polys(contours.into_iter().map(|c| c.into_iter().map(|p| (p[0], p[1])).collect()).collect())
}
