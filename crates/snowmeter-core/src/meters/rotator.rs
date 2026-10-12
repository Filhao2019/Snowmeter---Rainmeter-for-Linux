//! The Rotator meter (meters.md section 8, `MeterRotator.cpp`).

use super::imgslot::ImageSlot;
use super::{MeterBase, MeterImpl, MeterInputs};
use crate::parser::OptionReader;
use crate::render::Resources;
use std::f64::consts::PI;

pub struct RotatorMeter {
    image: ImageSlot,
    offset_x: f64,
    offset_y: f64,
    start_angle: f64,
    rotation_angle: f64,
    value_remainder: i64,
    value: f64,
    log: Vec<String>,
}

impl Default for RotatorMeter {
    fn default() -> Self {
        RotatorMeter {
            image: ImageSlot::default(),
            offset_x: 0.0,
            offset_y: 0.0,
            start_angle: 0.0,
            rotation_angle: PI * 2.0,
            value_remainder: 0,
            value: 0.0,
            log: Vec::new(),
        }
    }
}

impl RotatorMeter {
    /// Rotation in degrees for the current value.
    pub fn degrees(&self) -> f64 {
        (self.rotation_angle * self.value + self.start_angle).to_degrees()
    }

    pub fn set_value(&mut self, v: f64) {
        self.value = v;
    }
}

impl MeterImpl for RotatorMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        let mut log = Vec::new();
        self.image.name = String::new();
        self.image.read(opt, "", &mut log);
        // The Rotator's image option is `ImageName`.
        self.image.name = opt.string("ImageName", "");
        self.log.extend(log);
        self.offset_x = opt.float("OffsetX", 0.0);
        self.offset_y = opt.float("OffsetY", 0.0);
        self.start_angle = opt.float("StartAngle", 0.0);
        self.rotation_angle = opt.float("RotationAngle", PI * 2.0);
        let typo = opt.int("ValueReminder", 0);
        self.value_remainder = opt.int("ValueRemainder", typo).max(0);
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        let mut changed = self.image.update(m);
        if let Some(me) = m.measures.first() {
            let v = if self.value_remainder > 0 {
                let r = self.value_remainder;
                ((me.value() as i64).rem_euclid(r) as f64) / r as f64
            } else {
                me.relative_value()
            };
            changed |= v != self.value;
            self.value = v;
        }
        changed
    }

    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        let Some(img) = self.image.surface() else { return };
        let (left, top, _, _) = base.content_rect();
        let cx = left + base.w / 2.0;
        let cy = top + base.h / 2.0;
        cr.translate(cx, cy);
        cr.rotate(self.degrees().to_radians());
        cr.translate(-self.offset_x, -self.offset_y);
        let _ = cr.set_source_surface(img, 0.0, 0.0);
        cr.source().set_filter(cairo::Filter::Best);
        let _ = cr.paint();
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_value_is_ninety_degrees() {
        let mut r = RotatorMeter::default();
        r.value = 0.25;
        assert!((r.degrees() - 90.0).abs() < 1e-9);
        r.start_angle = PI / 2.0;
        assert!((r.degrees() - 180.0).abs() < 1e-9);
    }

    #[test]
    fn image_pixels_map_to_the_center() {
        // Offset (10,10) on the meter center (100,100); angle 90 degrees:
        // image (110,10) -> (100,200).
        let (cx, cy, ox, oy) = (100.0f64, 100.0f64, 10.0f64, 10.0f64);
        let a = 90.0f64.to_radians();
        let (px, py) = (110.0 - ox, 10.0 - oy);
        let x = a.cos() * px - a.sin() * py + cx;
        let y = a.sin() * px + a.cos() * py + cy;
        assert!((x - 100.0).abs() < 1e-9 && (y - 200.0).abs() < 1e-9);
    }
}
