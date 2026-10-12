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
        self.measure_names = read_measure_names(opt);
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
        self.padding[0] + self.padding[2]
    }

    pub fn pad_h(&self) -> i32 {
        self.padding[1] + self.padding[3]
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

    /// `[GetX(), GetX()+W) x [GetY(), GetY()+H)`.
    pub fn hit_test(&self, x: f64, y: f64) -> bool {
        x >= self.rect_x && y >= self.rect_y && x < self.rect_x + self.w && y < self.rect_y + self.h
    }

    /// SolidColor / SolidColor2 / GradientAngle background over the full rect.
    pub fn draw_background(&self, cr: &cairo::Context) {
        paint_background(cr, self.rect_x, self.rect_y, self.w, self.h, self.solid_color, self.solid_color2, self.gradient_angle);
    }
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
