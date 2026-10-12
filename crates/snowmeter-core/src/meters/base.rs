//! Options and behaviour shared by every meter (meters.md section 1).

use crate::color::Color;
use crate::numparse::parse_i32_prefix;
use crate::parser::OptionReader;
use crate::skin::mouse::MouseActions;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Rel {
    #[default]
    Abs,
    /// `r`: relative to the previous meter's top-left.
    TopLeft,
    /// `R`: relative to the previous meter's bottom-right.
    BottomRight,
}

/// `UpdateDivider` counter (Section.cpp).
#[derive(Clone, Copy, Debug)]
pub struct Divider {
    pub divider: i64,
    pub counter: i64,
}

impl Default for Divider {
    fn default() -> Self {
        Divider { divider: 1, counter: 1 }
    }
}

impl Divider {
    pub fn set(&mut self, d: i64) {
        if d != self.divider {
            self.divider = d;
            self.counter = d;
        }
    }

    /// Would the next `tick` fire?
    pub fn will_fire(&self) -> bool {
        self.counter + 1 >= self.divider
    }

    pub fn tick(&mut self) -> bool {
        self.counter += 1;
        if self.counter < self.divider {
            return false;
        }
        self.counter = 0;
        true
    }

    /// Forced update: reset so the next `tick` fires.
    pub fn force(&mut self) {
        self.counter = self.divider;
    }
}

#[derive(Clone, Debug, Default)]
pub struct MeterBase {
    pub name: String,
    pub type_name: String,
    pub x_value: i32,
    pub y_value: i32,
    pub x_rel: Rel,
    pub y_rel: Rel,
    /// Anchor position in skin coordinates after relative positioning.
    pub x: f64,
    pub y: f64,
    /// Drawn rect left/top (anchor with alignment applied).
    pub rect_x: f64,
    pub rect_y: f64,
    /// `m_W`/`m_H`: content plus padding.
    pub w: f64,
    pub h: f64,
    pub w_given: Option<i32>,
    pub h_given: Option<i32>,
    /// `l, t, r, b`.
    pub padding: [i32; 4],
    pub hidden: bool,
    pub solid_color: Color,
    pub solid_color2: Color,
    pub gradient_angle: f64,
    pub antialias: bool,
    pub measure_names: Vec<String>,
    pub groups: Vec<String>,
    pub divider: Divider,
    pub dynamic_variables: bool,
    pub on_update: String,
    pub mouse: MouseActions,
    /// `BevelType` 0 none, 1 up, 2 down.
    pub bevel: u8,
    pub bevel_color: Color,
    pub bevel_color2: Color,
    /// `TransformationMatrix` (identity when unset or invalid).
    pub matrix: Option<cairo::Matrix>,
    /// Stored for the front end; not shown yet.
    pub tooltip_text: String,
    pub tooltip_title: String,
}

/// Parse an `X`/`Y` option: optional trailing `r`/`R`, then `ParseInt`
/// (formula in parentheses or integer prefix).
pub fn parse_position(opt: &OptionReader, raw: &str) -> (i32, Rel) {
    let s = raw.trim();
    let (body, rel) = match s.chars().last() {
        Some('r') => (&s[..s.len() - 1], Rel::TopLeft),
        Some('R') => (&s[..s.len() - 1], Rel::BottomRight),
        _ => (s, Rel::Abs),
    };
    (parse_int_value(opt, body), rel)
}

pub fn parse_int_value(opt: &OptionReader, s: &str) -> i32 {
    let s = s.trim_start();
    if s.starts_with('(') {
        opt.parser.parse_formula(s, opt.sv).map(|v| v as i32).unwrap_or(0)
    } else {
        parse_i32_prefix(s).unwrap_or(0) as i32
    }
}

fn parse_padding(s: &str) -> [i32; 4] {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() < 4 {
        return [0; 4];
    }
    let n = |i: usize| parse_i32_prefix(parts[i].trim()).unwrap_or(0) as i32;
    [n(0), n(1), n(2), n(3)]
}

impl MeterBase {
    pub fn new(name: &str, type_name: &str) -> MeterBase {
        MeterBase {
            name: name.to_string(),
            type_name: type_name.to_string(),
            solid_color: Color::default(),
            solid_color2: Color::default(),
            ..Default::default()
        }
    }

    /// (Re)read the options common to all meters.
    pub fn read(&mut self, opt: &OptionReader, default_divider: i64) {
        let (x, xr) = parse_position(opt, &opt.string("X", "0"));
        let (y, yr) = parse_position(opt, &opt.string("Y", "0"));
        self.x_value = x;
        self.y_value = y;
        self.x_rel = xr;
        self.y_rel = yr;
        let given = |key: &str| {
            let s = opt.string(key, "");
            (!s.trim().is_empty()).then(|| parse_int_value(opt, &s))
        };
        self.w_given = given("W");
        self.h_given = given("H");
        self.padding = parse_padding(&opt.string("Padding", ""));
        self.hidden = opt.int("Hidden", 0) != 0;
        self.solid_color = opt.color("SolidColor", Color::default());
        self.solid_color2 = opt.color("SolidColor2", self.solid_color);
        self.gradient_angle = opt.float("GradientAngle", 0.0);
        self.antialias = opt.int("AntiAlias", 0) != 0;
        self.bevel = opt.int("BevelType", 0).clamp(0, 2) as u8;
        self.bevel_color = opt.color("BevelColor", Color { r: 255, g: 255, b: 255, a: 255 });
        self.bevel_color2 = opt.color("BevelColor2", Color { r: 0, g: 0, b: 0, a: 255 });
        self.matrix = parse_matrix(opt);
        self.tooltip_text = opt.string("ToolTipText", "");
        self.tooltip_title = opt.string("ToolTipTitle", "");
        self.measure_names = read_measure_names(opt);
        if self.type_name.eq_ignore_ascii_case("Histogram") && self.measure_names.len() == 1 {
            let legacy = opt.string("SecondaryMeasureName", "");
            if !legacy.trim().is_empty() {
                self.measure_names.push(legacy.trim().to_string());
            }
        }
        self.groups = opt
            .string("Group", "")
            .split('|')
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty())
            .collect();
        self.divider.set(opt.int("UpdateDivider", default_divider));
        self.dynamic_variables = opt.int("DynamicVariables", 0) != 0;
        self.on_update = opt.action("OnUpdateAction");
        self.mouse = MouseActions::read(opt);
    }

    pub fn in_group(&self, g: &str) -> bool {
        self.groups.iter().any(|x| x.eq_ignore_ascii_case(g))
    }

    pub fn pad_w(&self) -> i32 {
        self.padding[0].saturating_add(self.padding[2])
    }

    pub fn pad_h(&self) -> i32 {
        self.padding[1].saturating_add(self.padding[3])
    }

    /// Width as seen by relative positioning and window sizing (0 when hidden).
    pub fn gw(&self) -> f64 {
        if self.hidden {
            0.0
        } else {
            self.w
        }
    }

    pub fn gh(&self) -> f64 {
        if self.hidden {
            0.0
        } else {
            self.h
        }
    }

    /// Padded content rect `(x, y, w, h)` of the drawn rect.
    pub fn content_rect(&self) -> (f64, f64, f64, f64) {
        (
            self.rect_x + self.padding[0] as f64,
            self.rect_y + self.padding[1] as f64,
            self.w - self.pad_w() as f64,
            self.h - self.pad_h() as f64,
        )
    }

    /// Set the cairo antialias mode from the `AntiAlias` option.
    pub fn apply_antialias(&self, cr: &cairo::Context) {
        cr.set_antialias(if self.antialias { cairo::Antialias::Default } else { cairo::Antialias::None });
    }

    /// `[GetX(), GetX()+W) x [GetY(), GetY()+H)`.
    pub fn hit_test(&self, x: f64, y: f64) -> bool {
        x >= self.rect_x && y >= self.rect_y && x < self.rect_x + self.w && y < self.rect_y + self.h
    }

    /// SolidColor / SolidColor2 / GradientAngle background over the full rect.
    pub fn draw_background(&self, cr: &cairo::Context) {
        paint_background(cr, self.rect_x, self.rect_y, self.w, self.h, self.solid_color, self.solid_color2, self.gradient_angle);
        if self.bevel != 0 {
            self.draw_bevel(cr);
        }
    }

    /// `DrawBevel`: eight 1px lines in a rect two pixels outside the meter.
    fn draw_bevel(&self, cr: &cairo::Context) {
        let (light, dark) = if self.bevel == 1 { (self.bevel_color, self.bevel_color2) } else { (self.bevel_color2, self.bevel_color) };
        let o = if self.antialias { 0.0 } else { 0.5 };
        let (l, t) = (self.rect_x - 2.0 + o, self.rect_y - 2.0 + o);
        let (r, b) = (self.rect_x + self.w + 2.0, self.rect_y + self.h + 2.0);
        let _ = cr.save();
        self.apply_antialias(cr);
        cr.set_line_width(1.0);
        cr.set_line_cap(cairo::LineCap::Butt);
        let lines = |c: Color, ls: [(f64, f64, f64, f64); 4]| {
            cr.set_source_rgba(c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0, c.a as f64 / 255.0);
            for (x1, y1, x2, y2) in ls {
                cr.move_to(x1, y1);
                cr.line_to(x2, y2);
                let _ = cr.stroke();
            }
        };
        lines(light, [(l, t, l, b), (l, t, r, t), (l + 1.0, t, l + 1.0, b - 1.0), (l, t + 1.0, r - 1.0, t + 1.0)]);
        lines(dark, [(l, b, r, b), (r, t, r, b), (l + 1.0, b - 1.0, r - 1.0, b - 1.0), (r - 1.0, t + 1.0, r - 1.0, b - 1.0)]);
        let _ = cr.restore();
    }
}

/// `TransformationMatrix=m11;m12;m21;m22;dx;dy`.
fn parse_matrix(opt: &OptionReader) -> Option<cairo::Matrix> {
    let text = opt.string("TransformationMatrix", "");
    if text.trim().is_empty() {
        return None;
    }
    let parts: Vec<&str> = text.split(';').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut v = [0.0; 6];
    for (i, p) in parts.iter().enumerate() {
        let p = p.trim();
        v[i] = if p.starts_with('(') { opt.parser.parse_formula(p, opt.sv)? } else { crate::numparse::parse_f64_prefix(p) };
    }
    Some(cairo::Matrix::new(v[0], v[1], v[2], v[3], v[4], v[5]))
}

fn read_measure_names(opt: &OptionReader) -> Vec<String> {
    let mut names = Vec::new();
    let first = opt.string("MeasureName", "");
    if first.trim().is_empty() {
        return names;
    }
    names.push(first.trim().to_string());
    for n in 2.. {
        let s = opt.string(&format!("MeasureName{n}"), "");
        if s.trim().is_empty() {
            break;
        }
        names.push(s.trim().to_string());
    }
    names
}

/// `EdgePoint(theta)` of the gradient math: centre plus half the rect's
/// extent along the angle.
pub fn edge_point(x: f64, y: f64, w: f64, h: f64, degrees: f64) -> (f64, f64) {
    let t = degrees.to_radians();
    let l = (w * t.cos().abs() + h * t.sin().abs()) / 2.0;
    (x + w / 2.0 + l * t.cos(), y + h / 2.0 + l * t.sin())
}

#[allow(clippy::too_many_arguments)]
pub fn paint_background(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, c1: Color, c2: Color, angle: f64) {
    if (c1.a == 0 && c2.a == 0) || w <= 0.0 || h <= 0.0 {
        return;
    }
    let rgba = |c: Color| (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0, c.a as f64 / 255.0);
    if c1 == c2 {
        let (r, g, b, a) = rgba(c1);
        cr.set_source_rgba(r, g, b, a);
    } else {
        let (sx, sy) = edge_point(x, y, w, h, angle + 180.0);
        let (ex, ey) = edge_point(x, y, w, h, angle);
        let grad = cairo::LinearGradient::new(sx, sy, ex, ey);
        let (r, g, b, a) = rgba(c1);
        grad.add_color_stop_rgba(0.0, r, g, b, a);
        let (r, g, b, a) = rgba(c2);
        grad.add_color_stop_rgba(1.0, r, g, b, a);
        let _ = cr.set_source(&grad);
    }
    let _ = cr.save();
    cr.set_antialias(cairo::Antialias::None);
    cr.rectangle(x, y, w, h);
    let _ = cr.fill();
    let _ = cr.restore();
}
