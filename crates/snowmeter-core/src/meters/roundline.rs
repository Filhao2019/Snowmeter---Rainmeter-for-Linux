//! The RoundLine meter (meters.md section 6, `MeterRoundLine.cpp`).

use super::bar::set_color;
use super::{MeterBase, MeterImpl, MeterInputs};
use crate::color::Color;
use crate::parser::OptionReader;
use crate::render::geom::arc_to;
use crate::render::Resources;
use std::f64::consts::PI;

pub struct RoundLineMeter {
    line_width: f64,
    line_length: f64,
    line_start: f64,
    start_angle: f64,
    rotation_angle: f64,
    value_remainder: i64,
    color: Color,
    solid: bool,
    control_angle: bool,
    control_start: bool,
    control_length: bool,
    start_shift: f64,
    length_shift: f64,
    value: f64,
}

impl Default for RoundLineMeter {
    fn default() -> Self {
        RoundLineMeter {
            line_width: 1.0,
            line_length: 20.0,
            line_start: -1.0,
            start_angle: 0.0,
            rotation_angle: PI * 2.0,
            value_remainder: 0,
            color: Color { r: 0, g: 0, b: 0, a: 255 },
            solid: false,
            control_angle: true,
            control_start: false,
            control_length: false,
            start_shift: 0.0,
            length_shift: 0.0,
            value: 1.0,
        }
    }
}

/// The two end points of a non-solid line: `((sx, sy), (ex, ey))`.
pub fn line_endpoints(m: &RoundLineMeter, cx: f64, cy: f64) -> ((f64, f64), (f64, f64)) {
    let (rs, rl) = m.radii();
    let angle = if m.control_angle { m.rotation_angle * m.value } else { m.rotation_angle } + m.start_angle;
    let (s, c) = angle.sin_cos();
    ((c * rs + cx, s * rs + cy), (c * rl + cx, s * rl + cy))
}

impl RoundLineMeter {
    fn radii(&self) -> (f64, f64) {
        (
            self.line_start + if self.control_start { self.start_shift * self.value } else { 0.0 },
            self.line_length + if self.control_length { self.length_shift * self.value } else { 0.0 },
        )
    }

    pub fn set_value(&mut self, v: f64) {
        self.value = v;
    }

    pub fn value(&self) -> f64 {
        self.value
    }
}

impl MeterImpl for RoundLineMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        self.line_width = opt.float("LineWidth", 1.0);
        self.line_length = opt.float("LineLength", 20.0);
        self.line_start = opt.float("LineStart", -1.0);
        self.start_angle = opt.float("StartAngle", 0.0);
        self.rotation_angle = opt.float("RotationAngle", PI * 2.0);
        let rem = if opt.has("ValueRemainder") { opt.int("ValueRemainder", 0) } else { opt.int("ValueReminder", 0) };
        self.value_remainder = rem.max(0);
        self.color = opt.color("LineColor", Color { r: 0, g: 0, b: 0, a: 255 });
        self.solid = opt.bool("Solid", false);
        self.control_angle = opt.bool("ControlAngle", true);
        self.control_start = opt.bool("ControlStart", false);
        self.control_length = opt.bool("ControlLength", false);
        self.start_shift = opt.float("StartShift", 0.0);
        self.length_shift = opt.float("LengthShift", 0.0);
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        let Some(me) = m.measures.first() else {
            let changed = self.value != 1.0;
            self.value = 1.0;
            return changed;
        };
        let v = if self.value_remainder > 0 {
            let r = self.value_remainder;
            ((me.value() as i64).rem_euclid(r) as f64) / r as f64
        } else {
            me.relative_value()
        };
        let changed = v != self.value;
        self.value = v;
        changed
    }

    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        let cx = base.rect_x + base.w / 2.0;
        let cy = base.rect_y + base.h / 2.0;
        base.apply_antialias(cr);
        set_color(cr, self.color);
        if !self.solid {
            if self.line_width <= 0.0 {
                return;
            }
            let (s, e) = line_endpoints(self, cx, cy);
            cr.set_line_width(self.line_width);
            cr.set_line_cap(cairo::LineCap::Butt);
            cr.move_to(s.0, s.1);
            cr.line_to(e.0, e.1);
            let _ = cr.stroke();
            return;
        }
        let (rs, rl) = self.radii();
        let rotation = self.rotation_angle;
        let angle = (rotation * if self.control_angle { self.value } else { 1.0 }).clamp(-2.0 * PI, 2.0 * PI) + self.start_angle;
        let (sin, cos) = angle.sin_cos();
        let (sx, sy) = (cos * rs + cx, sin * rs + cy);
        let (ex, ey) = (cos * rl + cx, sin * rl + cy);
        let sweep = (rotation * self.value).clamp(-2.0 * PI, 2.0 * PI).to_degrees();
        let inner_ccw = sweep > 0.0 || sweep < -359.935;
        let outer_cw = inner_ccw;
        let large = sweep.abs() >= 180.0;
        let (s0, c0) = (self.start_angle + 0.001).sin_cos();
        let (ix, iy) = (rs * c0 + cx, rs * s0 + cy);
        let (ox, oy) = (rl * c0 + cx, rl * s0 + cy);
        cr.new_path();
        cr.move_to(ix, iy);
        cr.line_to(ox, oy);
        arc_to(cr, (ox, oy), (ex, ey), rl, rl, sweep, large, outer_cw);
        cr.line_to(sx, sy);
        arc_to(cr, (sx, sy), (ix, iy), rs, rs, sweep, large, !outer_cw);
        cr.close_path();
        cr.set_fill_rule(cairo::FillRule::EvenOdd);
        let _ = cr.fill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rl(start: f64, len: f64) -> RoundLineMeter {
        RoundLineMeter { line_start: start, line_length: len, ..Default::default() }
    }

    #[test]
    fn quarter_value_points_down() {
        // X=0 Y=0 W=100 H=100 LineLength=50 LineStart=0, value .25: theta = pi/2.
        let mut m = rl(0.0, 50.0);
        m.value = 0.25;
        let (s, e) = line_endpoints(&m, 50.0, 50.0);
        assert!((s.0 - 50.0).abs() < 1e-9 && (s.1 - 50.0).abs() < 1e-9);
        assert!((e.0 - 50.0).abs() < 1e-3 && (e.1 - 100.0).abs() < 1e-3);
    }

    #[test]
    fn start_angle_rotates_the_line() {
        let mut m = rl(0.0, 50.0);
        m.value = 0.25;
        m.start_angle = -1.5708;
        let (_, e) = line_endpoints(&m, 50.0, 50.0);
        assert!((e.0 - 100.0).abs() < 1e-2 && (e.1 - 50.0).abs() < 1e-2);
    }

    #[test]
    fn default_line_start_is_one_pixel_behind_the_center() {
        let mut m = RoundLineMeter::default();
        m.value = 0.25;
        let (s, _) = line_endpoints(&m, 50.0, 50.0);
        assert!((s.0 - 50.0).abs() < 1e-3 && (s.1 - 49.0).abs() < 1e-3);
    }

    #[test]
    fn controlled_start_and_length() {
        let mut m = rl(0.0, 10.0);
        m.control_start = true;
        m.control_length = true;
        m.start_shift = 4.0;
        m.length_shift = 10.0;
        m.value = 0.5;
        m.rotation_angle = 0.0;
        let (s, e) = line_endpoints(&m, 0.0, 0.0);
        assert!((s.0 - 2.0).abs() < 1e-9 && (e.0 - 15.0).abs() < 1e-9);
    }
}
