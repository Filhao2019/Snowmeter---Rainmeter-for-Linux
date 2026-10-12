//! The Shape meter (meters.md section 7, `MeterShape.cpp`).

pub mod geometry;
pub mod parse;
pub mod types;

use super::base::edge_point;
use super::{MeterBase, MeterImpl, MeterInputs};
use crate::parser::OptionReader;
use crate::render::Resources;
use geometry::*;
use parse::{parse_shape, parse_transform, Ctx, Parsed};
use types::*;

#[derive(Default)]
pub struct ShapeMeter {
    shapes: Vec<ShapeDef>,
    log: Vec<String>,
}

/// Parse the `Shape`, `Shape2`, ... options of a section (combines resolved).
pub fn read_shapes(opt: &OptionReader, log: &mut Vec<String>) -> Vec<ShapeDef> {
    let mut parsed: Vec<Parsed> = Vec::new();
    for n in 1.. {
        let key = if n == 1 { "Shape".to_string() } else { format!("Shape{n}") };
        let text = opt.string(&key, "");
        if text.trim().is_empty() {
            break;
        }
        let mut c = Ctx { opt, log, name: key.clone() };
        match parse_shape(&text, &mut c, n - 1) {
            Ok(p) => parsed.push(p),
            Err(e) => {
                log.push(format!("{key}: {e}"));
                break;
            }
        }
    }
    resolve_combines(parsed, opt, log)
}

fn resolve_combines(parsed: Vec<Parsed>, opt: &OptionReader, log: &mut Vec<String>) -> Vec<ShapeDef> {
    let mut shapes: Vec<ShapeDef> = Vec::new();
    let mut specs: Vec<(usize, CombineSpec)> = Vec::new();
    for (i, p) in parsed.into_iter().enumerate() {
        match p {
            Parsed::Shape(s) => shapes.push(s),
            Parsed::Combine(c) => {
                let mut placeholder = ShapeDef::new(Geom::None);
                placeholder.combined = false;
                shapes.push(placeholder);
                specs.push((i, c));
            }
        }
    }
    for (index, spec) in specs {
        let n = shapes.len();
        if spec.parent >= n {
            log.push(format!("Shape{}: invalid shape identifier in Combine", index + 1));
            continue;
        }
        let parent = shapes[spec.parent].clone();
        let mut acc = flatten(&parent);
        let mut ok = true;
        let mut mods = Vec::new();
        for item in &spec.items {
            match item {
                CombineItem::Part(mode, id) => {
                    if *id >= n || *id == index {
                        log.push(format!("Shape{}: invalid shape identifier in Combine", index + 1));
                        ok = false;
                        break;
                    }
                    let other = flatten(&shapes[*id]);
                    acc = combine(&acc, &other, *mode);
                    shapes[*id].combined = true;
                }
                CombineItem::Modifier(m) => mods.push(m.clone()),
            }
        }
        if !ok {
            continue;
        }
        shapes[spec.parent].combined = true;
        let mut result = ShapeDef::new(polys_of(acc));
        result.fill = parent.fill.clone();
        result.stroke = parent.stroke.clone();
        result.stroke_width = parent.stroke_width;
        result.custom_stroke = parent.custom_stroke;
        result.start_cap = parent.start_cap;
        result.end_cap = parent.end_cap;
        result.dash_cap = parent.dash_cap;
        result.join = parent.join;
        result.miter_limit = parent.miter_limit;
        result.dashes = parent.dashes.clone();
        result.dash_offset = parent.dash_offset;
        let mut c = Ctx { opt, log, name: format!("Shape{}", index + 1) };
        for m in &mods {
            parse_transform(&mut result.transforms, m, &mut c);
        }
        shapes[index] = result;
    }
    shapes
}

fn rgba(c: crate::color::Color) -> (f64, f64, f64, f64) {
    (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0, c.a as f64 / 255.0)
}

fn add_stops(p: &cairo::Gradient, stops: &[Stop]) {
    let mut stops = stops.to_vec();
    if stops.len() == 1 {
        let s = stops[0].clone();
        let t = crate::color::Color { r: 0, g: 0, b: 0, a: 0 };
        stops.push(Stop { color: t, pos: if s.pos < 0.5 { 1.0 } else { 0.0 } });
    }
    for s in &stops {
        let (r, g, b, a) = rgba(s.color);
        p.add_color_stop_rgba(s.pos.clamp(0.0, 1.0), r, g, b, a);
    }
}

/// Install `paint` as the cairo source (in the current user space).
fn set_paint(cr: &cairo::Context, paint: &Paint, bounds: Rect4) {
    let (l, t, r, b) = bounds;
    let (w, h) = (r - l, b - t);
    match paint {
        Paint::Color(c) => {
            let (r, g, b, a) = rgba(*c);
            cr.set_source_rgba(r, g, b, a);
        }
        Paint::Linear { angle, stops, .. } => {
            let a = angle.rem_euclid(360.0);
            let (sx, sy) = edge_point(l, t, w, h, a);
            let (ex, ey) = edge_point(l, t, w, h, a + 180.0);
            let g = cairo::LinearGradient::new(sx, sy, ex, ey);
            add_stops(&g, stops);
            let _ = cr.set_source(&g);
        }
        Paint::Radial { center, origin, radius, stops, .. } => {
            let (rx, ry) = radius.unwrap_or((w / 2.0, h / 2.0));
            let (rx, ry) = (rx.max(1e-6), ry.max(1e-6));
            let c = (l + w / 2.0 + center.0, t + h / 2.0 + center.1);
            let o = (c.0 + origin.0, c.1 + origin.1);
            let g = cairo::RadialGradient::new((o.0 - c.0) / rx, (o.1 - c.1) / ry, 0.0, 0.0, 0.0, 1.0);
            g.set_matrix(cairo::Matrix::new(1.0 / rx, 0.0, 0.0, 1.0 / ry, -c.0 / rx, -c.1 / ry));
            add_stops(&g, stops);
            let _ = cr.set_source(&g);
        }
    }
}

/// Draw one shape with the origin of `cr` at the shape's coordinate origin.
pub fn draw_shape(cr: &cairo::Context, s: &ShapeDef) {
    if s.combined || s.geom == Geom::None {
        return;
    }
    let _ = cr.save();
    let base = cr.matrix();
    cr.new_path();
    cr.transform(shape_matrix(s));
    let bounds = widened_bounds(s);
    build_path(cr, &s.geom, false);
    cr.set_fill_rule(fill_rule(&s.geom));
    let fill_visible = !matches!(s.fill, Paint::Color(c) if c.a == 0);
    if fill_visible {
        set_paint(cr, &s.fill, bounds);
        let _ = cr.fill_preserve();
    }
    if s.stroke_width > 0.0 {
        set_paint(cr, &s.stroke, bounds);
        cr.new_path();
        build_path(cr, &s.geom, true);
        if s.custom_stroke {
            // Fixed stroke transform: the path stays, the width is not scaled.
            cr.set_matrix(base);
        }
        apply_stroke_style(cr, s);
        let _ = cr.stroke();
    }
    cr.new_path();
    cr.set_matrix(base);
    let _ = cr.restore();
}

impl ShapeMeter {
    pub fn shapes(&self) -> &[ShapeDef] {
        &self.shapes
    }
}

impl MeterImpl for ShapeMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        let mut log = Vec::new();
        self.shapes = read_shapes(opt, &mut log);
        self.log.extend(log);
    }

    fn update(&mut self, _m: &MeterInputs) -> bool {
        false
    }

    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        let (mut mr, mut mb) = (0.0f64, 0.0f64);
        for s in self.shapes.iter().filter(|s| !s.combined) {
            if let Some((_, _, r, b)) = drawn_bounds(s) {
                mr = mr.max(r);
                mb = mb.max(b);
            }
        }
        (mr.ceil(), mb.ceil())
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        let (cx, cy, _, _) = base.content_rect();
        cr.set_antialias(cairo::Antialias::Default);
        cr.translate(cx.trunc(), cy.trunc());
        for s in &self.shapes {
            draw_shape(cr, s);
        }
    }

    fn hit_test(&self, base: &MeterBase, x: f64, y: f64) -> bool {
        let (px, py) = (x - base.rect_x, y - base.rect_y);
        self.shapes.iter().filter(|s| !s.combined).any(|s| contains_point(s, px, py))
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::measures::testutil::skin;

    fn parse(body: &str) -> (Vec<ShapeDef>, Vec<String>) {
        let s = skin(&format!("[S]\nMeter=Shape\n{body}\n"));
        let opt = OptionReader::new(&s.parser, "S", None);
        let mut log = Vec::new();
        let shapes = read_shapes(&opt, &mut log);
        (shapes, log)
    }

    fn col(r: u8, g: u8, b: u8) -> Paint {
        Paint::Color(Color { r, g, b, a: 255 })
    }

    #[test]
    fn rectangle_ellipse_line_defaults_and_fill() {
        let (s, log) = parse("Shape=Rectangle 1,2,30,40,5 | Fill Color 255,0,0\nShape2=Ellipse 10,10,5\nShape3=Line 0,0,10,10");
        assert!(log.is_empty(), "{log:?}");
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].geom, Geom::Rect { x: 1.0, y: 2.0, w: 30.0, h: 40.0, rx: 5.0, ry: 5.0 });
        assert_eq!(s[0].fill, col(255, 0, 0));
        assert_eq!(s[1].geom, Geom::Ellipse { cx: 10.0, cy: 10.0, rx: 5.0, ry: 5.0 });
        assert_eq!(s[1].fill, col(255, 255, 255), "default fill is white");
        assert_eq!(s[1].stroke_width, 1.0);
        assert!(matches!(s[2].geom, Geom::Line { .. }));
    }

    #[test]
    fn arc_curve_path_parse() {
        let (s, log) = parse(
            "Shape=Arc 0,0,10,0,5,5,0,1,1,1\nShape2=Curve 0,0,10,10,5,0\nShape3=Curve 0,0,10,10,5,0,5,10,1\n\
             Shape4=Path P | StrokeWidth 2\nShape5=Path1 P\nP=0,0 | LineTo 10,0 | ArcTo 10,10,5 | CurveTo 0,10,2,2 | CurveTo 0,0,1,1,2,2 | SetNoStroke 1 | ClosePath 1",
        );
        assert!(log.is_empty(), "{log:?}");
        assert!(matches!(s[0].geom, Geom::Arc { clockwise: false, large: true, closed: true, .. }));
        assert!(matches!(s[1].geom, Geom::Curve { c2: None, .. }));
        assert!(matches!(s[2].geom, Geom::Curve { c2: Some(_), closed: true, .. }));
        match (&s[3].geom, &s[4].geom) {
            (Geom::Path { segs, closed: true, winding: false, .. }, Geom::Path { winding: true, .. }) => assert_eq!(segs.len(), 4),
            g => panic!("{g:?}"),
        }
        assert_eq!(s[3].stroke_width, 2.0);
    }

    #[test]
    fn stroke_and_transform_modifiers() {
        let (s, log) = parse(
            "Shape=Rectangle 0,0,10,10 | Stroke Color 0,255,0 | StrokeWidth 3 | StrokeStartCap Round | StrokeLineJoin Bevel, 4 | \
             StrokeDashes 1, 2 | StrokeDashOffset 1 | Rotate 45 | Scale 2, 3 | Skew 10, 20 | Offset 5, 6 | TransformOrder Offset, Rotate",
        );
        assert!(log.is_empty(), "{log:?}");
        let d = &s[0];
        assert_eq!(d.stroke, col(0, 255, 0));
        assert_eq!(d.stroke_width, 3.0);
        assert_eq!(d.start_cap, Cap::Round);
        assert_eq!((d.join, d.miter_limit), (Join::Bevel, 4.0));
        assert_eq!(d.dashes, vec![1.0, 2.0]);
        assert!(d.custom_stroke);
        assert_eq!(d.transforms.rotate, Some((45.0, None)));
        assert_eq!(d.transforms.scale, Some(((2.0, 3.0), None)));
        assert_eq!(d.transforms.skew, Some(((10.0, 20.0), None)));
        assert_eq!(d.transforms.offset, Some((5.0, 6.0)));
        assert_eq!(d.transforms.order[..2], [TransformKind::Offset, TransformKind::Rotate]);
    }

    #[test]
    fn gradients_extend_and_stop_rules() {
        let (s, log) = parse(
            "Shape=Rectangle 0,0,10,10 | Fill LinearGradient1 G | Stroke RadialGradient R | Extend E\nG=90 | 255,0,0 ; 0 | 0,0,255 ; 1\n\
             R=1,2 | 255,0,0 ; 0 | 0,0,255 ; 1\nE=StrokeWidth 7",
        );
        assert!(log.is_empty(), "{log:?}");
        match &s[0].fill {
            Paint::Linear { angle, stops, linear_gamma } => {
                assert_eq!((*angle, stops.len(), *linear_gamma), (90.0, 2, true));
            }
            p => panic!("{p:?}"),
        }
        assert!(matches!(s[0].stroke, Paint::Radial { center: (1.0, 2.0), .. }));
        assert_eq!(s[0].stroke_width, 7.0);
    }

    #[test]
    fn open_shapes_have_transparent_fill_and_bad_shape_stops_the_list() {
        let (s, log) = parse("Shape=Line 0,0,5,5\nShape2=Bogus 1\nShape3=Ellipse 1,1,1");
        assert_eq!(s.len(), 1);
        assert!(!log.is_empty());
        let (s, _) = parse("Shape=Arc 0,0,10,0 | Fill Color 1,2,3\nShape2=Arc 0,0,10,0");
        assert_eq!(s[0].fill, col(1, 2, 3));
        assert_eq!(s[1].fill, Paint::Color(Color { r: 0, g: 0, b: 0, a: 0 }));
    }

    #[test]
    fn combine_marks_parts_and_unions_geometry() {
        let (s, log) = parse(
            "Shape=Rectangle 0,0,10,10 | Fill Color 9,9,9\nShape2=Rectangle 5,0,10,10\nShape3=Combine Shape | Union Shape2",
        );
        assert!(log.is_empty(), "{log:?}");
        assert!(s[0].combined && s[1].combined && !s[2].combined);
        assert_eq!(s[2].fill, col(9, 9, 9));
        let b = drawn_bounds(&{
            let mut c = s[2].clone();
            c.stroke_width = 0.0;
            c
        })
        .unwrap();
        assert!((b.0 - 0.0).abs() < 1e-6 && (b.2 - 15.0).abs() < 1e-6, "{b:?}");
        let (_, log) = parse("Shape=Rectangle 0,0,1,1\nShape2=Combine Shape2");
        assert!(!log.is_empty(), "self reference is an error");
    }

    #[test]
    fn combined_shape_anchors_are_absolute_like_clock_hands() {
        // A clock hand around (100,100): its parts hang above the pivot, and the
        // anchor given to the combine is the pivot itself (Monterey, ModernGadgets).
        let (s, log) = parse(
            "Shape=Rectangle 98,60,4,20 | StrokeWidth 0\nShape2=Rectangle 98,80,4,30 | StrokeWidth 0\n\
             Shape3=Combine Shape | Union Shape2 | Rotate 90,100,100",
        );
        assert!(log.is_empty(), "{log:?}");
        let b = drawn_bounds(&s[2]).unwrap();
        // Rotated 90 degrees clockwise about (100,100): the hand now points right.
        assert!((b.0 - 90.0).abs() < 1e-3 && (b.2 - 140.0).abs() < 1e-3, "{b:?}");
        assert!((b.1 - 98.0).abs() < 1e-3 && (b.3 - 102.0).abs() < 1e-3, "{b:?}");
        // A plain shape keeps the documented top-left-relative anchor.
        let (s, _) = parse("Shape=Rectangle 98,60,4,40 | StrokeWidth 0 | Rotate 90,2,40");
        let b = drawn_bounds(&s[0]).unwrap();
        assert!((b.0 - 100.0).abs() < 1e-3 && (b.2 - 140.0).abs() < 1e-3, "{b:?}");
    }

    #[test]
    fn transformed_bounds_and_hit_test() {
        let (s, _) = parse("Shape=Rectangle 0,0,10,10 | StrokeWidth 0 | Offset 20,0");
        let b = drawn_bounds(&s[0]).unwrap();
        assert_eq!((b.0, b.2), (20.0, 30.0));
        assert!(contains_point(&s[0], 25.0, 5.0));
        assert!(!contains_point(&s[0], 5.0, 5.0));
    }
}
