//! The Histogram meter (meters.md section 10.1, `MeterHistogram.cpp`).

use super::bar::set_color;
use super::imgslot::ImageSlot;
use super::{MeterBase, MeterImpl, MeterInputs};
use crate::color::Color;
use crate::parser::OptionReader;
use crate::render::image::paint_region;
use crate::render::Resources;

pub struct HistogramMeter {
    primary_color: Color,
    secondary_color: Color,
    both_color: Color,
    primary: ImageSlot,
    secondary: ImageSlot,
    both: ImageSlot,
    autoscale: bool,
    flip: bool,
    start_left: bool,
    horizontal: bool,
    primary_values: Vec<f64>,
    secondary_values: Vec<f64>,
    pos: usize,
    has_secondary: bool,
    max_p: f64,
    min_p: f64,
    max_s: f64,
    min_s: f64,
    log: Vec<String>,
}

impl Default for HistogramMeter {
    fn default() -> Self {
        HistogramMeter {
            primary_color: Color { r: 0, g: 128, b: 0, a: 255 },
            secondary_color: Color { r: 255, g: 0, b: 0, a: 255 },
            both_color: Color { r: 255, g: 255, b: 0, a: 255 },
            primary: ImageSlot::default(),
            secondary: ImageSlot::default(),
            both: ImageSlot::default(),
            autoscale: false,
            flip: false,
            start_left: false,
            horizontal: false,
            primary_values: Vec::new(),
            secondary_values: Vec::new(),
            pos: 0,
            has_secondary: false,
            max_p: 1.0,
            min_p: 0.0,
            max_s: 0.0,
            min_s: 0.0,
            log: Vec::new(),
        }
    }
}

/// Bar length in pixels for a sample (`(int)(len * value)` clamped).
pub fn bar_len(sample: f64, min: f64, max: f64, len: i64) -> i64 {
    let range = max - min;
    let v = if range < 0.0 {
        0.0
    } else if range == 0.0 {
        1.0
    } else {
        (sample - min) / range
    };
    ((len as f64 * v) as i64).min(len).max(0)
}

fn pow2_at_least(v: f64) -> f64 {
    let mut m = 2.0;
    while m < v && m.is_finite() {
        m *= 2.0;
    }
    m
}

impl HistogramMeter {
    pub fn history(&self) -> &[f64] {
        &self.primary_values
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    fn images_ready(&self) -> bool {
        let named = |s: &ImageSlot| !s.name.trim().is_empty();
        let loaded = |s: &ImageSlot| s.surface().is_some();
        !((named(&self.primary) && !loaded(&self.primary))
            || (named(&self.secondary) && !loaded(&self.secondary))
            || (named(&self.both) && !loaded(&self.both)))
    }
}

impl MeterImpl for HistogramMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        self.primary_color = opt.color("PrimaryColor", Color { r: 0, g: 128, b: 0, a: 255 });
        self.secondary_color = opt.color("SecondaryColor", Color { r: 255, g: 0, b: 0, a: 255 });
        self.both_color = opt.color("BothColor", Color { r: 255, g: 255, b: 0, a: 255 });
        let mut log = Vec::new();
        self.primary.read(opt, "Primary", &mut log);
        self.secondary.read(opt, "Secondary", &mut log);
        self.both.read(opt, "Both", &mut log);
        self.log.extend(log);
        self.autoscale = opt.bool("AutoScale", false);
        self.flip = opt.bool("Flip", false);
        self.start_left = opt.string("GraphStart", "Right").trim().eq_ignore_ascii_case("left");
        self.horizontal = opt.string("GraphOrientation", "Vertical").trim().eq_ignore_ascii_case("horizontal");
        self.primary_values.clear();
        self.secondary_values.clear();
        self.pos = 0;
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        let mut changed = self.primary.update(m);
        changed |= self.secondary.update(m);
        changed |= self.both.update(m);
        if m.measures.is_empty() {
            return changed;
        }
        let two = m.measures.len() >= 2;
        let sanity_fail = two && self.primary.surface().is_some() && (self.both.surface().is_none() || self.secondary.surface().is_none());
        if sanity_fail {
            self.log.push("Histogram: SecondaryImage and BothImage not defined".into());
        }
        let size = match self.primary.size() {
            Some((w, h)) => {
                if self.horizontal {
                    h + m.pad.1
                } else {
                    w + m.pad.0
                }
            }
            None => {
                if self.horizontal {
                    m.size.1
                } else {
                    m.size.0
                }
            }
        }
        .max(0) as usize;
        if !self.images_ready() || sanity_fail || size == 0 {
            self.primary_values.clear();
            return changed;
        }
        if self.primary_values.len() != size || self.has_secondary != two {
            self.primary_values = vec![0.0; size];
            self.secondary_values = if two { vec![0.0; size] } else { Vec::new() };
            self.has_secondary = two;
            self.pos = 0;
        }
        self.primary_values[self.pos] = m.measures[0].value();
        if two {
            self.secondary_values[self.pos] = m.measures[1].value();
        }
        self.pos = (self.pos + 1) % size;
        self.max_p = m.measures[0].max_value();
        self.min_p = m.measures[0].min_value();
        (self.max_s, self.min_s) = (0.0, 0.0);
        if two {
            self.max_s = m.measures[1].max_value();
            self.min_s = m.measures[1].min_value();
        }
        if self.autoscale {
            let mut new = self.primary_values.iter().copied().fold(0.0, f64::max);
            self.max_p = pow2_at_least(new);
            if two {
                for v in &self.secondary_values {
                    new = new.max(*v);
                }
                self.max_s = pow2_at_least(new);
            }
        }
        true
    }

    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn fixed_size(&self) -> Option<(f64, f64)> {
        self.primary.size().map(|(w, h)| (w as f64, h as f64))
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        if self.primary_values.is_empty() {
            return;
        }
        let (left, top, w, h) = base.content_rect();
        let (right, bottom) = (left + w, top + h);
        let (dw, dh) = (w as i64, h as i64);
        base.apply_antialias(cr);
        let draw = |slot: &ImageSlot, color: Color, dst: (f64, f64, f64, f64)| {
            if dst.2 <= 0.0 || dst.3 <= 0.0 {
                return;
            }
            match slot.surface() {
                None => {
                    set_color(cr, color);
                    cr.rectangle(dst.0, dst.1, dst.2, dst.3);
                    let _ = cr.fill();
                }
                Some(img) => paint_region(cr, img, (dst.0 - left, dst.1 - top, dst.2, dst.3), dst),
            }
        };
        let len = if self.horizontal { dh } else { dw };
        let cross = if self.horizontal { dw } else { dh };
        if len <= 0 || self.primary_values.len() as i64 != len {
            return;
        }
        for n in 0..len {
            // Sample index: GraphStart=Left (vertical) / Flip (horizontal) walk backwards.
            let reverse = if self.horizontal { self.flip } else { self.start_left };
            let i = if reverse { len - 1 - n } else { n };
            let idx = ((i + self.pos as i64) % len) as usize;
            let p = bar_len(self.primary_values[idx], self.min_p, self.max_p, cross);
            let step = n as f64;
            // (primary, both, secondary-rest) lengths.
            let (both_len, rest_len, rest_is_secondary) = if self.has_secondary {
                let s = bar_len(self.secondary_values[idx], self.min_s, self.max_s, cross);
                let both = p.min(s);
                if s > p {
                    (both, s - both, true)
                } else {
                    (both, p - both, false)
                }
            } else {
                (0, p, false)
            };
            let rest_total = both_len + rest_len;
            let rect_for = |from: i64, length: i64| -> (f64, f64, f64, f64) {
                let (from, length) = (from as f64, length as f64);
                if self.horizontal {
                    if self.start_left {
                        (left + from, top + step, length, 1.0)
                    } else {
                        (right - from - length, top + step, length, 1.0)
                    }
                } else if self.flip {
                    (left + step, top + from, 1.0, length)
                } else {
                    (left + step, bottom - from - length, 1.0, length)
                }
            };
            if self.has_secondary {
                draw(&self.both, self.both_color, rect_for(0, both_len));
                let (slot, color) = if rest_is_secondary { (&self.secondary, self.secondary_color) } else { (&self.primary, self.primary_color) };
                draw(slot, color, rect_for(both_len, rest_total - both_len));
            } else {
                draw(&self.primary, self.primary_color, rect_for(0, p));
            }
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_length_math() {
        assert_eq!(bar_len(50.0, 0.0, 100.0, 40), 20);
        assert_eq!(bar_len(150.0, 0.0, 100.0, 40), 40);
        assert_eq!(bar_len(-5.0, 0.0, 100.0, 40), 0);
        assert_eq!(bar_len(3.0, 5.0, 5.0, 40), 40);
        assert_eq!(bar_len(3.0, 6.0, 5.0, 40), 0);
        assert_eq!(bar_len(99.0, 0.0, 100.0, 10), 9);
    }

    #[test]
    fn autoscale_power_of_two() {
        assert_eq!(pow2_at_least(0.0), 2.0);
        assert_eq!(pow2_at_least(5.0), 8.0);
        assert_eq!(pow2_at_least(8.0), 8.0);
    }
}
