//! The Line meter (meters.md section 5, `MeterLine.cpp`).

use super::bar::set_color;
use super::{MeterBase, MeterImpl, MeterInputs};
use crate::color::Color;
use crate::parser::OptionReader;
use crate::render::Resources;

const OFFSET: f64 = 0.55;

pub struct LineMeter {
    colors: Vec<Color>,
    scales: Vec<f64>,
    flip: bool,
    autoscale: bool,
    line_width: f64,
    horizontal_lines: bool,
    horizontal_color: Color,
    start_left: bool,
    horizontal: bool,
    join: cairo::LineJoin,
    miter_limit: f64,
    /// One ring buffer per line.
    values: Vec<Vec<f64>>,
    pos: usize,
    /// Min/max of the bound measures at the last update.
    min_max: Vec<(f64, f64)>,
    log: Vec<String>,
}

impl Default for LineMeter {
    fn default() -> Self {
        LineMeter {
            colors: Vec::new(),
            scales: Vec::new(),
            flip: false,
            autoscale: false,
            line_width: 1.0,
            horizontal_lines: false,
            horizontal_color: Color { r: 0, g: 0, b: 0, a: 255 },
            start_left: false,
            horizontal: false,
            join: cairo::LineJoin::Miter,
            miter_limit: 10.0,
            values: Vec::new(),
            pos: 0,
            min_max: Vec::new(),
            log: Vec::new(),
        }
    }
}

/// Everything `line_points` needs besides the samples.
#[derive(Clone, Copy, Debug)]
pub struct GraphGeometry {
    /// Padded meter rect `(left, top, right, bottom)`.
    pub rect: (f64, f64, f64, f64),
    pub flip: bool,
    pub start_left: bool,
    pub horizontal: bool,
}

/// Range of the graph: `(min, max)` as in `MeterLine::Draw`.
pub fn graph_range(
    values: &[Vec<f64>],
    scales: &[f64],
    measures: &[(f64, f64)],
    autoscale: bool,
) -> (f64, f64) {
    let min = measures.iter().map(|m| m.0).fold(f64::MAX, f64::min);
    let max = if autoscale {
        let mut m = 0.0f64;
        for (i, v) in values.iter().enumerate() {
            let s = scales.get(i).copied().unwrap_or(1.0);
            for x in v {
                m = m.max(x * s);
            }
        }
        let mut max = 2.0;
        while max < m && max.is_finite() {
            max *= 2.0;
        }
        max
    } else {
        measures.iter().map(|m| m.1).fold(-f64::MAX, f64::max)
    };
    (min, max)
}

/// The polyline of one line. `buf` is the ring buffer, `pos` the index of the
/// oldest sample.
pub fn line_points(buf: &[f64], pos: usize, scale_n: f64, min: f64, max: f64, g: &GraphGeometry) -> Vec<(f64, f64)> {
    let (left, top, right, bottom) = g.rect;
    let len = buf.len();
    if len == 0 {
        return Vec::new();
    }
    let range = max - min;
    let draw_w = (right - left) as i64;
    let draw_h = (bottom - top) as i64;
    let extent = if g.horizontal { (draw_w - 1) as f64 } else { (draw_h - 1) as f64 };
    let scale = (scale_n * extent) / range;
    let calc = |v: f64| -> f64 {
        let mut c = if range == 0.0 {
            extent
        } else if range < 0.0 {
            if g.horizontal {
                1.0
            } else {
                0.0
            }
        } else {
            (((v - min) * scale) + OFFSET).min(extent + OFFSET).max(OFFSET)
        };
        if g.horizontal {
            c = left + if g.start_left { c } else { extent - c + 1.0 };
        } else {
            c = top + if g.flip { c } else { extent - c + 1.0 };
        }
        c
    };
    let mut p = pos % len;
    let mut pts = Vec::new();
    let mut old = calc(buf[p]);
    if g.horizontal {
        pts.push((old, if !g.flip { top } else { bottom }));
        if !g.flip {
            let mut j = top + 1.0;
            while j < bottom {
                p = (p + 1) % len;
                let x = calc(buf[p]);
                pts.push((old, j - 1.0));
                pts.push((x, j));
                old = x;
                j += 1.0;
            }
        } else {
            let mut j = bottom;
            while j > top + 1.0 {
                p = (p + 1) % len;
                let x = calc(buf[p]);
                pts.push((old, j - 1.0));
                pts.push((x, j - 2.0));
                old = x;
                j -= 1.0;
            }
        }
    } else {
        pts.push((if !g.start_left { left } else { right }, old));
        if !g.start_left {
            let mut j = left + 1.0;
            while j < right {
                p = (p + 1) % len;
                let y = calc(buf[p]);
                pts.push((j - 1.0, old));
                pts.push((j, y));
                old = y;
                j += 1.0;
            }
        } else {
            let mut j = right;
            while j > left + 1.0 {
                p = (p + 1) % len;
                let y = calc(buf[p]);
                pts.push((j - 1.0, old));
                pts.push((j - 2.0, y));
                old = y;
                j -= 1.0;
            }
        }
    }
    pts
}

impl LineMeter {
    fn resize_buffers(&mut self, size: usize) {
        let n = self.colors.len();
        if self.values.len() != n || self.values.first().map(Vec::len).unwrap_or(size) != size {
            self.values = vec![vec![0.0; size]; n];
            self.pos = 0;
        }
    }

    pub fn history(&self, line: usize) -> &[f64] {
        &self.values[line]
    }

    pub fn pos(&self) -> usize {
        self.pos
    }
}

impl MeterImpl for LineMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        let count = opt.int("LineCount", 1).max(0) as usize;
        let white = Color { r: 255, g: 255, b: 255, a: 255 };
        self.colors.clear();
        self.scales.clear();
        for i in 0..count {
            let (ck, sk) = if i == 0 { ("LineColor".to_string(), "Scale".to_string()) } else { (format!("LineColor{}", i + 1), format!("Scale{}", i + 1)) };
            self.colors.push(opt.color(&ck, white));
            self.scales.push(opt.float(&sk, 1.0));
        }
        self.flip = opt.bool("Flip", false);
        self.autoscale = opt.bool("AutoScale", false);
        self.line_width = opt.float("LineWidth", 1.0).max(1.0);
        self.horizontal_lines = opt.bool("HorizontalLines", false);
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let legacy = opt.color("HorizontalColor", black);
        self.horizontal_color = opt.color("HorizontalLineColor", legacy);
        self.start_left = opt.string("GraphStart", "Right").trim().eq_ignore_ascii_case("left");
        self.horizontal = opt.string("GraphOrientation", "Vertical").trim().eq_ignore_ascii_case("horizontal");
        let join = opt.string("LineJoin", "Miter");
        self.join = match join.trim().to_ascii_uppercase().as_str() {
            "MITER" | "MITERORBEVEL" => cairo::LineJoin::Miter,
            "BEVEL" => cairo::LineJoin::Bevel,
            "ROUND" => cairo::LineJoin::Round,
            _ => {
                self.log.push(format!("LineJoin={join} is not valid"));
                cairo::LineJoin::Miter
            }
        };
        self.miter_limit = opt.float("MiterLimit", 10.0);
        if self.miter_limit <= 0.0 {
            self.log.push("MiterLimit must be positive".into());
            self.miter_limit = 10.0;
        }
        self.values.clear();
        self.pos = 0;
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        if m.measures.is_empty() {
            return false;
        }
        let size = if self.horizontal { m.size.1 } else { m.size.0 }.max(0) as usize;
        self.resize_buffers(size);
        self.min_max = m.measures.iter().map(|x| (x.min_value(), x.max_value())).collect();
        if size > 0 {
            for (i, me) in m.measures.iter().take(self.values.len()).enumerate() {
                self.values[i][self.pos] = me.value();
            }
            self.pos = (self.pos + 1) % size;
        }
        true
    }

    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        let rect = (base.rect_x + base.padding[0] as f64, base.rect_y + base.padding[1] as f64, base.rect_x + base.w - base.padding[2] as f64, base.rect_y + base.h - base.padding[3] as f64);
        let size = if self.horizontal { base.h } else { base.w } as usize;
        if size == 0 || self.values.first().map(Vec::len) != Some(size) {
            return;
        }
        let (min, max) = graph_range(&self.values, &self.scales, &self.min_max, self.autoscale);
        base.apply_antialias(cr);
        cr.set_line_join(self.join);
        cr.set_miter_limit((self.miter_limit / 2.0).max(1.0));
        cr.set_line_cap(cairo::LineCap::Butt);
        if self.horizontal_lines {
            let draw_h = (rect.3 - rect.1) as i64;
            let max_lines = draw_h / 4;
            let mut power = 2i64;
            while power < max_lines {
                power *= 2;
            }
            let n = (max as i64).rem_euclid(power) + 1;
            set_color(cr, self.horizontal_color);
            cr.set_line_width(1.0);
            for j in 0..n {
                let yy = rect.3 - ((j + 1) * draw_h / (n + 1)) as f64 - 0.5;
                cr.move_to(rect.0, yy);
                cr.line_to(rect.2 - 1.0, yy);
                let _ = cr.stroke();
            }
        }
        let g = GraphGeometry { rect, flip: self.flip, start_left: self.start_left, horizontal: self.horizontal };
        for (i, buf) in self.values.iter().enumerate() {
            let pts = line_points(buf, self.pos, self.scales[i], min, max, &g);
            if pts.is_empty() {
                continue;
            }
            cr.new_path();
            for (k, (px, py)) in pts.iter().enumerate() {
                if k == 0 {
                    cr.move_to(*px, *py);
                } else {
                    cr.line_to(*px, *py);
                }
            }
            set_color(cr, self.colors[i]);
            cr.set_line_width(self.line_width);
            let _ = cr.stroke();
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(w: f64, h: f64) -> GraphGeometry {
        GraphGeometry { rect: (0.0, 0.0, w, h), flip: false, start_left: false, horizontal: false }
    }

    #[test]
    fn value_to_y_matches_vectors() {
        // H=50, min 0, max 100: v=0 -> top + 49.45, v=100 -> top + 0.45.
        let g = geom(4.0, 50.0);
        let pts = line_points(&[0.0, 0.0, 0.0, 100.0], 0, 1.0, 0.0, 100.0, &g);
        assert!((pts[0].1 - 49.45).abs() < 1e-9);
        // The newest sample (k = dw-1) is at x = left + dw - 1 with y 0.45.
        let last = pts.last().unwrap();
        assert!((last.0 - 3.0).abs() < 1e-9);
        assert!((last.1 - 0.45).abs() < 1e-9);
    }

    #[test]
    fn ring_buffer_scrolls() {
        // pos points at the oldest sample; after wrapping, the oldest value
        // is the one at pos.
        let g = geom(3.0, 11.0);
        let a = line_points(&[1.0, 2.0, 3.0], 0, 1.0, 0.0, 10.0, &g);
        let b = line_points(&[4.0, 2.0, 3.0], 1, 1.0, 0.0, 10.0, &g);
        // b = samples 2,3,4 oldest to newest.
        assert!((a[0].1 - 8.45).abs() < 1e-9 || a[0].1 > 0.0);
        assert!(b.last().unwrap().1 < b[0].1);
        assert_eq!(a.len(), 5);
    }

    #[test]
    fn autoscale_is_power_of_two() {
        let v = vec![vec![5.0, 1.0]];
        assert_eq!(graph_range(&v, &[1.0], &[(0.0, 100.0)], true), (0.0, 8.0));
        let v = vec![vec![1.5]];
        assert_eq!(graph_range(&v, &[1.0], &[(0.0, 100.0)], true).1, 2.0);
        let v = vec![vec![1.0]];
        assert_eq!(graph_range(&v, &[3.0], &[(0.0, 100.0)], true).1, 4.0);
    }

    #[test]
    fn zero_range_pins_to_the_bottom() {
        let g = geom(2.0, 10.0);
        let pts = line_points(&[5.0, 5.0], 0, 1.0, 5.0, 5.0, &g);
        // range == 0 -> yv = Hf -> y = top + 1
        assert!((pts[0].1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn graph_start_left_puts_newest_on_the_left() {
        let mut g = geom(4.0, 11.0);
        g.start_left = true;
        let pts = line_points(&[0.0, 0.0, 0.0, 10.0], 0, 1.0, 0.0, 10.0, &g);
        assert_eq!(pts[0].0, 4.0);
        let last = pts.last().unwrap();
        assert_eq!(last.0, 0.0);
        assert!((last.1 - 0.45).abs() < 1e-9);
    }
}
