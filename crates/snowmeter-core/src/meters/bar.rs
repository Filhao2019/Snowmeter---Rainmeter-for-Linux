//! The Bar meter (meters.md section 4, `MeterBar.cpp`).

use super::imgslot::ImageSlot;
use super::{MeterBase, MeterImpl, MeterInputs};
use crate::color::Color;
use crate::parser::OptionReader;
use crate::render::image::paint_region;
use crate::render::Resources;

pub struct BarMeter {
    color: Color,
    image: ImageSlot,
    border: i32,
    horizontal: bool,
    flip: bool,
    value: f64,
    log: Vec<String>,
}

impl Default for BarMeter {
    fn default() -> Self {
        BarMeter {
            color: Color { r: 0, g: 128, b: 0, a: 255 },
            image: ImageSlot::default(),
            border: 0,
            horizontal: false,
            flip: false,
            value: 0.0,
            log: Vec::new(),
        }
    }
}

impl BarMeter {
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Fill length along the bar axis for an axis length `len`.
    pub fn bar_size(&self, len: f64) -> f64 {
        let bar = len - 2.0 * self.border as f64;
        (bar * self.value).min(bar).max(0.0)
    }
}

impl MeterImpl for BarMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        self.color = opt.color("BarColor", Color { r: 0, g: 128, b: 0, a: 255 });
        let mut log = Vec::new();
        self.image.read(opt, "Bar", &mut log);
        self.log.extend(log);
        self.border = opt.int("BarBorder", 0) as i32;
        self.flip = opt.bool("Flip", false);
        self.horizontal = opt.string("BarOrientation", "Vertical").trim().eq_ignore_ascii_case("horizontal");
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        let mut changed = self.image.update(m);
        if let Some(me) = m.measures.first() {
            let v = me.relative_value();
            changed |= v != self.value;
            self.value = v;
        }
        changed
    }

    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn fixed_size(&self) -> Option<(f64, f64)> {
        self.image.size().map(|(w, h)| (w as f64, h as f64))
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        let (left, top, w, h) = base.content_rect();
        let (right, bottom) = (left + w, top + h);
        let b = self.border as f64;
        base.apply_antialias(cr);
        let region = |s: (f64, f64, f64, f64), d: (f64, f64, f64, f64), img: &cairo::ImageSurface| {
            paint_region(cr, img, s, d);
        };
        if !self.horizontal {
            let size = self.bar_size(h);
            if let Some(img) = self.image.surface() {
                if self.flip {
                    if b > 0.0 {
                        region((0.0, 0.0, w, b), (left, top, w, b), img);
                        region((0.0, h - b, w, b), (left, top + size + b, w, b), img);
                    }
                    region((0.0, b, w, size), (left, top + b, w, size), img);
                } else {
                    if b > 0.0 {
                        region((0.0, 0.0, w, b), (left, bottom - size - 2.0 * b, w, b), img);
                        region((0.0, h - b, w, b), (left, bottom - b, w, b), img);
                    }
                    region((0.0, h - size - b, w, size), (left, bottom - size - b, w, size), img);
                }
            } else {
                let y = if self.flip { top } else { bottom - size };
                fill_rect(cr, left, y, w, size, self.color);
            }
        } else {
            let size = self.bar_size(w);
            if let Some(img) = self.image.surface() {
                if self.flip {
                    if b > 0.0 {
                        region((0.0, 0.0, b, h), (right - size - 2.0 * b, top, b, h), img);
                        region((w - b, 0.0, b, h), (right - b, top, b, h), img);
                    }
                    region((w - size - b, 0.0, size, h), (right - size - b, top, size, h), img);
                } else {
                    if b > 0.0 {
                        region((0.0, 0.0, b, h), (left, top, b, h), img);
                        region((w - b, 0.0, b, h), (left + size + b, top, b, h), img);
                    }
                    region((b, 0.0, size, h), (left + b, top, size, h), img);
                }
            } else {
                let x = if self.flip { right - size } else { left };
                fill_rect(cr, x, top, size, h, self.color);
            }
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

pub(crate) fn set_color(cr: &cairo::Context, c: Color) {
    cr.set_source_rgba(c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0, c.a as f64 / 255.0);
}

fn fill_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, c: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    set_color(cr, c);
    cr.rectangle(x, y, w, h);
    let _ = cr.fill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_fill_math() {
        let mut b = BarMeter::default();
        b.value = 0.37;
        assert!((b.bar_size(100.0) - 37.0).abs() < 1e-9);
        b.border = 5;
        assert!((b.bar_size(100.0) - 33.3).abs() < 1e-9);
        b.value = 2.0;
        assert_eq!(b.bar_size(100.0), 90.0);
        b.value = -1.0;
        assert_eq!(b.bar_size(100.0), 0.0);
    }
}
