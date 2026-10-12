//! Parsing of `Shape`, `Shape2`, ... options (`MeterShape.cpp`).

use super::types::*;
use crate::color::{parse_color_with, Color};
use crate::parser::OptionReader;

/// Split on `delim` outside parentheses; parts are trimmed.
pub fn split_top(s: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
                cur.push(c);
            }
            c if c == delim && depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    let last = cur.trim().to_string();
    if !last.is_empty() || !out.is_empty() {
        out.push(last);
    }
    out
}

/// Consume a leading keyword (ASCII case-insensitive); returns the rest.
pub fn consume<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let head = s.get(..kw.len())?;
    head.eq_ignore_ascii_case(kw).then(|| s[kw.len()..].trim_start())
}

pub struct Ctx<'a, 'b> {
    pub opt: &'a OptionReader<'b>,
    pub log: &'a mut Vec<String>,
    pub name: String,
}

impl Ctx<'_, '_> {
    pub fn number(&self, s: &str, default: f64) -> f64 {
        let s = s.trim();
        if s.is_empty() {
            return default;
        }
        if s.starts_with('(') {
            return self.opt.parser.parse_formula(s, self.opt.sv).unwrap_or(default);
        }
        crate::numparse::parse_f64_prefix(s)
    }

    pub fn int(&self, s: &str) -> i64 {
        self.number(s, 0.0) as i64
    }

    pub fn color(&self, s: &str) -> Color {
        parse_color_with(s, &|n| self.opt.sv.and_then(|sv| sv.measure_value(n))).unwrap_or(Color { r: 255, g: 255, b: 255, a: 255 })
    }

    fn warn(&mut self, msg: impl std::fmt::Display) {
        let n = self.name.clone();
        self.log.push(format!("{n}: {msg}"));
    }
}

/// Values of a definition (`a, b, c`), consumed left to right.
pub struct Vals {
    items: Vec<String>,
    i: usize,
}

impl Vals {
    pub fn new(s: &str) -> Vals {
        let mut items = split_top(s, ',');
        while items.last().map(|x| x.is_empty()).unwrap_or(false) {
            items.pop();
        }
        Vals { items, i: 0 }
    }

    fn next(&mut self) -> Option<String> {
        let v = self.items.get(self.i).cloned();
        if v.is_some() {
            self.i += 1;
        }
        v
    }

    fn read(&mut self, c: &Ctx, out: &mut f64) -> bool {
        match self.next() {
            Some(v) => {
                *out = c.number(&v, 0.0);
                true
            }
            None => false,
        }
    }

    fn read_default(&mut self, c: &Ctx, out: &mut f64, default: f64) -> bool {
        match self.next() {
            Some(v) => {
                *out = c.number(&v, default);
                true
            }
            None => false,
        }
    }

    fn pair(&mut self, c: &Ctx) -> Option<(f64, f64)> {
        let (mut x, mut y) = (0.0, 0.0);
        if self.read(c, &mut x) && self.read(c, &mut y) {
            Some((x, y))
        } else {
            None
        }
    }

    /// `*` keeps the default.
    fn read_or_default(&mut self, c: &Ctx, out: &mut f64) -> bool {
        match self.next() {
            Some(v) => {
                if !v.starts_with('*') {
                    *out = c.number(&v, 0.0);
                }
                true
            }
            None => false,
        }
    }

    /// Flags are inverted: `0` means true.
    fn flag_or_default(&mut self, c: &Ctx, out: &mut bool) -> bool {
        match self.next() {
            Some(v) => {
                if !v.starts_with('*') {
                    *out = c.int(&v) == 0;
                }
                true
            }
            None => false,
        }
    }
}

pub enum Parsed {
    Shape(ShapeDef),
    Combine(CombineSpec),
}

fn read_shape_id(rest: &str) -> usize {
    let t = rest.trim();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().map(|n| (n - 1).max(0) as usize).unwrap_or(0)
}

/// Parse one `ShapeN` option text. `Err` carries the error; the shape list
/// stops there.
pub fn parse_shape(text: &str, c: &mut Ctx, index: usize) -> Result<Parsed, String> {
    let parts: Vec<String> = split_top(text, '|').into_iter().filter(|p| !p.is_empty()).collect();
    let Some(def) = parts.first() else { return Err("empty shape".into()) };
    let mods = &parts[1..];
    if let Some(rest) = consume(def, "Combine") {
        return parse_combine(rest, mods, index).map(Parsed::Combine);
    }
    let mut shape = parse_definition(def, c)?;
    parse_modifiers(&mut shape, mods, c, false);
    Ok(Parsed::Shape(shape))
}

fn parse_combine(rest: &str, mods: &[String], index: usize) -> Result<CombineSpec, String> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err("definition contains no shape identifiers".into());
    }
    let Some(id) = consume(rest, "Shape") else {
        return Err(format!("definition contains invalid shape identifier: {rest}"));
    };
    let parent = read_shape_id(id);
    if parent == index {
        return Err(format!("cannot combine with: Shape{}", id.trim()));
    }
    let mut items = Vec::new();
    for m in mods {
        let mode = [("Union", CombineMode::Union), ("Xor", CombineMode::Xor), ("Intersect", CombineMode::Intersect), ("Exclude", CombineMode::Exclude)]
            .into_iter()
            .find_map(|(kw, mode)| consume(m, kw).map(|r| (mode, r)));
        match mode {
            Some((mode, r)) => {
                let r = consume(r, "Shape").unwrap_or(r);
                let id = read_shape_id(r);
                if id == index {
                    return Err(format!("cannot combine with: Shape{}", r.trim()));
                }
                items.push(CombineItem::Part(mode, id));
            }
            None => items.push(CombineItem::Modifier(m.clone())),
        }
    }
    Ok(CombineSpec { parent, items })
}

fn parse_definition(def: &str, c: &mut Ctx) -> Result<ShapeDef, String> {
    if let Some(r) = consume(def, "Rectangle") {
        let mut v = Vals::new(r);
        let (mut x, mut y, mut w, mut h) = (0.0, 0.0, 0.0, 0.0);
        if !(v.read(c, &mut x) && v.read(c, &mut y) && v.read(c, &mut w) && v.read(c, &mut h)) {
            return Err("Rectangle has too few parameters".into());
        }
        let mut rx = 0.0;
        let has_r = v.read(c, &mut rx);
        let mut ry = rx;
        if has_r {
            v.read(c, &mut ry);
        }
        Ok(ShapeDef::new(Geom::Rect { x, y, w, h, rx, ry }))
    } else if let Some(r) = consume(def, "Ellipse") {
        let mut v = Vals::new(r);
        let (mut cx, mut cy, mut rx) = (0.0, 0.0, 0.0);
        if !(v.read(c, &mut cx) && v.read(c, &mut cy) && v.read(c, &mut rx)) {
            return Err("Ellipse has too few parameters".into());
        }
        let mut ry = rx;
        v.read(c, &mut ry);
        Ok(ShapeDef::new(Geom::Ellipse { cx, cy, rx, ry }))
    } else if let Some(r) = consume(def, "Line") {
        let mut v = Vals::new(r);
        let (mut x1, mut y1, mut x2, mut y2) = (0.0, 0.0, 0.0, 0.0);
        if !(v.read(c, &mut x1) && v.read(c, &mut y1) && v.read(c, &mut x2) && v.read(c, &mut y2)) {
            return Err("Line has too few parameters".into());
        }
        Ok(ShapeDef::new(Geom::Line { x1, y1, x2, y2 }))
    } else if let Some(r) = consume(def, "Arc") {
        let mut v = Vals::new(r);
        let (mut x1, mut y1, mut x2, mut y2) = (0.0, 0.0, 0.0, 0.0);
        if !(v.read(c, &mut x1) && v.read(c, &mut y1) && v.read(c, &mut x2) && v.read(c, &mut y2)) {
            return Err("Arc has too few parameters".into());
        }
        let mut rx = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt() / 2.0;
        let mut angle = 0.0;
        let (mut cw, mut small, mut open) = (true, true, true);
        v.read_or_default(c, &mut rx);
        let mut ry = rx;
        v.read_or_default(c, &mut ry);
        v.read_or_default(c, &mut angle);
        v.flag_or_default(c, &mut cw);
        v.flag_or_default(c, &mut small);
        v.flag_or_default(c, &mut open);
        let mut s = ShapeDef::new(Geom::Arc { x1, y1, x2, y2, rx, ry, angle, clockwise: cw, large: !small, closed: !open });
        if open {
            s.transparent_fill();
        }
        Ok(s)
    } else if let Some(r) = consume(def, "Curve") {
        let mut v = Vals::new(r);
        let (mut x1, mut y1, mut x2, mut y2, mut cx1, mut cy1) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        if !(v.read(c, &mut x1) && v.read(c, &mut y1) && v.read(c, &mut x2) && v.read(c, &mut y2) && v.read(c, &mut cx1) && v.read(c, &mut cy1))
        {
            return Err("Curve has too few parameters".into());
        }
        let (mut open, mut c2) = (true, None);
        match v.next() {
            None => {}
            Some(seventh) => {
                let mut cy2 = 0.0;
                if v.read(c, &mut cy2) {
                    let cx2 = c.number(&seventh, 0.0);
                    c2 = Some((cx2, cy2));
                    let mut o = true;
                    v.flag_or_default(c, &mut o);
                    open = o;
                } else {
                    open = c.int(&seventh) == 0;
                }
            }
        }
        let mut s = ShapeDef::new(Geom::Curve { x1, y1, x2, y2, c1: (cx1, cy1), c2, closed: !open });
        if open {
            s.transparent_fill();
        }
        Ok(s)
    } else if let Some(r) = consume(def, "Path1") {
        parse_path_def(r, c, true)
    } else if let Some(r) = consume(def, "Path") {
        parse_path_def(r, c, false)
    } else {
        Err(format!("Invalid shape: {def}"))
    }
}

fn parse_path_def(option_name: &str, c: &mut Ctx, winding: bool) -> Result<ShapeDef, String> {
    let text = c.opt.string(option_name.trim(), "");
    if text.trim().is_empty() {
        return Err(format!("Path shape has invalid parameters: {text}"));
    }
    parse_path(&text, c, winding).ok_or_else(|| format!("Path shape has invalid parameters: {text}"))
}

/// Parse a path definition: `startX, startY | LineTo x, y | ...`.
pub fn parse_path(text: &str, c: &mut Ctx, winding: bool) -> Option<ShapeDef> {
    let parts = split_top(text, '|');
    let mut it = parts.into_iter().filter(|p| !p.is_empty());
    let start_text = it.next()?;
    let start = Vals::new(&start_text).pair(c)?;
    let mut segs = Vec::new();
    let mut cur = start;
    let mut open = true;
    let (mut no_stroke, mut round_join) = (false, false);
    let mut any = false;
    for p in it {
        any = true;
        let (seg, end) = if let Some(r) = consume(&p, "LineTo") {
            let (x, y) = Vals::new(r).pair(c)?;
            (Seg::Line { x, y }, (x, y))
        } else if let Some(r) = consume(&p, "ArcTo") {
            let mut v = Vals::new(r);
            let (x, y) = v.pair(c)?;
            let mut rx = ((x - cur.0).powi(2) + (y - cur.1).powi(2)).sqrt() / 2.0;
            let mut angle = 0.0;
            let (mut cw, mut small) = (true, true);
            v.read_or_default(c, &mut rx);
            let mut ry = rx;
            v.read_or_default(c, &mut ry);
            v.read_or_default(c, &mut angle);
            v.flag_or_default(c, &mut cw);
            v.flag_or_default(c, &mut small);
            (Seg::Arc { x, y, rx, ry, angle, clockwise: cw, large: !small }, (x, y))
        } else if let Some(r) = consume(&p, "CurveTo") {
            let mut v = Vals::new(r);
            let (x, y) = v.pair(c)?;
            let c1 = v.pair(c)?;
            match v.pair(c) {
                Some(c2) => (Seg::Cubic { x, y, c1, c2 }, (x, y)),
                None => (Seg::Quad { x, y, cx: c1.0, cy: c1.1 }, (x, y)),
            }
        } else if let Some(r) = consume(&p, "SetNoStroke") {
            no_stroke = c.int(r) != 0;
            continue;
        } else if let Some(r) = consume(&p, "SetRoundJoin") {
            round_join = c.int(r) != 0;
            continue;
        } else if let Some(r) = consume(&p, "ClosePath") {
            open = c.int(r) == 0;
            continue;
        } else {
            c.warn(format!("Invalid Path type: {p}"));
            return None;
        };
        cur = end;
        segs.push(PathSeg { seg, no_stroke, round_join });
    }
    if !any {
        return None;
    }
    let mut s = ShapeDef::new(Geom::Path { start, segs, closed: !open, winding });
    if open {
        s.transparent_fill();
    }
    Some(s)
}

fn parse_cap(rest: &str, c: &mut Ctx) -> Cap {
    for (kw, cap) in [("Flat", Cap::Flat), ("Square", Cap::Square), ("Round", Cap::Round), ("Triangle", Cap::Triangle)] {
        if consume(rest, kw).is_some() {
            return cap;
        }
    }
    if !rest.trim().is_empty() {
        c.warn(format!("Invalid cap style: {rest}"));
    }
    Cap::Flat
}

fn parse_gradient(kind: &str, text: &str, c: &mut Ctx, linear_gamma: bool) -> Option<Paint> {
    let parts = split_top(text, '|');
    let descriptor = parts.first()?.clone();
    if parts.len() < 2 {
        return None;
    }
    let mut stops: Vec<Stop> = parts[1..]
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| {
            let f: Vec<&str> = p.split(';').collect();
            if f.len() == 2 {
                Stop { color: c.color(f[0]), pos: c.number(f[1], 0.0) }
            } else {
                Stop { color: Color { r: 0, g: 0, b: 0, a: 0 }, pos: 0.0 }
            }
        })
        .collect();
    if stops.len() == 1 {
        let pos = if stops[0].pos < 0.5 { 1.0 } else { 0.0 };
        stops.push(Stop { color: Color { r: 0, g: 0, b: 0, a: 0 }, pos });
    }
    if kind == "linear" {
        let a = c.number(&descriptor, 0.0);
        let angle = (360.0 + a % 360.0) % 360.0;
        Some(Paint::Linear { angle, stops, linear_gamma })
    } else {
        let mut v = Vals::new(&descriptor);
        let center = v.pair(c)?;
        let origin = v.pair(c).unwrap_or((0.0, 0.0));
        let radius = v.pair(c);
        Some(Paint::Radial { center, origin, radius, stops, linear_gamma })
    }
}

/// Apply modifiers left to right.
pub fn parse_modifiers(shape: &mut ShapeDef, mods: &[String], c: &mut Ctx, recursive: bool) {
    for m in mods {
        if m.is_empty() {
            continue;
        }
        if let Some(r) = consume(m, "Fill") {
            if let Some(col) = consume(r, "Color") {
                shape.fill = Paint::Color(c.color(col));
            } else if let Some(p) = gradient_modifier(r, c) {
                shape.fill = p;
            } else {
                c.warn(format!("Fill has invalid parameters: {r}"));
            }
        } else if let Some(r) = consume(m, "StrokeWidth") {
            let mut w = c.number(r, 0.0);
            if w < 0.0 {
                c.warn("StrokeWidth must not be negative");
                w = 0.0;
            }
            shape.stroke_width = w;
            shape.custom_stroke = true;
        } else if let Some(r) = consume(m, "StrokeStartCap") {
            shape.start_cap = parse_cap(r, c);
            shape.custom_stroke = true;
        } else if let Some(r) = consume(m, "StrokeEndCap") {
            shape.end_cap = parse_cap(r, c);
            shape.custom_stroke = true;
        } else if let Some(r) = consume(m, "StrokeDashCap") {
            shape.dash_cap = parse_cap(r, c);
            shape.custom_stroke = true;
        } else if let Some(r) = consume(m, "StrokeLineJoin") {
            let mut v = Vals::new(r);
            let Some(style) = v.next() else {
                c.warn("StrokeLineJoin has too few parameters");
                continue;
            };
            shape.join = match style.to_ascii_lowercase().as_str() {
                "miter" => Join::Miter,
                "bevel" => Join::Bevel,
                "round" => Join::Round,
                "miterorbevel" => Join::MiterOrBevel,
                _ => {
                    c.warn(format!("Invalid line join style: {style}"));
                    Join::Miter
                }
            };
            let mut limit = 10.0;
            v.read_default(c, &mut limit, 10.0);
            if limit < 0.0 {
                c.warn("Miter limit must be positive");
                limit = 10.0;
            }
            shape.miter_limit = limit;
            shape.custom_stroke = true;
        } else if let Some(r) = consume(m, "StrokeDashes") {
            shape.dashes = Vals::new(r).items.iter().map(|d| c.number(d, 0.0)).collect();
            shape.custom_stroke = true;
        } else if let Some(r) = consume(m, "StrokeDashOffset") {
            let mut o = c.number(r, 0.0);
            if o < 0.0 {
                c.warn(format!("Invalid stroke dash offset: {r}"));
                o = 0.0;
            }
            shape.dash_offset = o;
            shape.custom_stroke = true;
        } else if let Some(r) = consume(m, "Stroke") {
            if let Some(col) = consume(r, "Color") {
                shape.stroke = Paint::Color(c.color(col));
                shape.custom_stroke = true;
            } else if let Some(p) = gradient_modifier(r, c) {
                shape.stroke = p;
                shape.custom_stroke = true;
            } else {
                c.warn(format!("Stroke has invalid parameters: {r}"));
            }
        } else if let Some(r) = consume(m, "Extend") {
            if recursive {
                c.warn("Extend cannot be used recursively");
                continue;
            }
            for name in Vals::new(r).items {
                if name.is_empty() {
                    continue;
                }
                let text = c.opt.string(&name, "");
                if !text.trim().is_empty() {
                    let ext: Vec<String> = split_top(&text, '|').into_iter().filter(|p| !p.is_empty()).collect();
                    parse_modifiers(shape, &ext, c, true);
                }
            }
        } else if !parse_transform(&mut shape.transforms, m, c) {
            c.warn(format!("Invalid shape modifier: {m}"));
        }
    }
}

/// `LinearGradient[1] Opt` / `RadialGradient[1] Opt`: the rest names the
/// option holding the gradient.
fn gradient_modifier(r: &str, c: &mut Ctx) -> Option<Paint> {
    let table = [
        ("LinearGradient1", "linear", true),
        ("LinearGradient", "linear", false),
        ("RadialGradient1", "radial", true),
        ("RadialGradient", "radial", false),
    ];
    for (kw, kind, gamma) in table {
        if let Some(name) = consume(r, kw) {
            let text = c.opt.string(name.trim(), "");
            let p = parse_gradient(kind, &text, c, gamma);
            if p.is_none() {
                c.warn(format!("{kw} has invalid parameters: {text}"));
            }
            return p;
        }
    }
    None
}

/// Offset / Rotate / Scale / Skew / TransformOrder. False if `m` is none of them.
pub fn parse_transform(t: &mut Transforms, m: &str, c: &mut Ctx) -> bool {
    if let Some(r) = consume(m, "Offset") {
        let mut v = Vals::new(r);
        match v.pair(c) {
            Some(p) => t.offset = Some(p),
            None => c.warn("Offset has too few parameters"),
        }
        true
    } else if let Some(r) = consume(m, "Rotate") {
        let mut v = Vals::new(r);
        let mut deg = 0.0;
        if v.read(c, &mut deg) {
            t.rotate = Some((deg, v.pair(c)));
        } else {
            c.warn("Rotate has too few parameters");
        }
        true
    } else if let Some(r) = consume(m, "Scale") {
        let mut v = Vals::new(r);
        let (mut sx, mut sy) = (1.0, 1.0);
        if v.read_default(c, &mut sx, 1.0) && v.read_default(c, &mut sy, 1.0) {
            t.scale = Some(((sx, sy), v.pair(c)));
        } else {
            c.warn("Scale has too few parameters");
        }
        true
    } else if let Some(r) = consume(m, "Skew") {
        let mut v = Vals::new(r);
        let (mut sx, mut sy) = (1.0, 1.0);
        if v.read_default(c, &mut sx, 1.0) && v.read_default(c, &mut sy, 1.0) {
            t.skew = Some(((sx, sy), v.pair(c)));
        } else {
            c.warn("Skew has too few parameters");
        }
        true
    } else if let Some(r) = consume(m, "TransformOrder") {
        let items = Vals::new(r).items;
        let mut order: Vec<TransformKind> = Vec::new();
        let mut last: Option<TransformKind> = None;
        let mut any = false;
        for it in items.iter().filter(|i| !i.is_empty()) {
            any = true;
            let k = [("Rotate", TransformKind::Rotate), ("Scale", TransformKind::Scale), ("Skew", TransformKind::Skew), ("Offset", TransformKind::Offset)]
                .into_iter()
                .find_map(|(kw, k)| consume(it, kw).map(|_| k))
                .or(last);
            last = k;
            match k {
                None => c.warn(format!("Invalid transform type: {it}")),
                Some(k) if order.contains(&k) => c.warn("TransformOrder cannot have duplicates"),
                Some(k) => order.push(k),
            }
        }
        if !any {
            c.warn("TransformOrder has too few parameters");
        } else {
            for k in DEFAULT_ORDER {
                if !order.contains(&k) {
                    order.push(k);
                }
            }
            t.order = order;
        }
        true
    } else {
        false
    }
}
