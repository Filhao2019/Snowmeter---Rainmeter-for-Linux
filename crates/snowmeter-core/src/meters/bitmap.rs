//! The Bitmap meter (meters.md section 10.2, `MeterBitmap.cpp`). Frame
//! transitions are not animated: the final frame is shown at once.

use super::imgslot::ImageSlot;
use super::{MeterBase, MeterImpl, MeterInputs};
use crate::parser::OptionReader;
use crate::render::image::paint_region;
use crate::render::Resources;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BitmapAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Default)]
pub struct BitmapMeter {
    image: ImageSlot,
    frames: i32,
    zero_frame: bool,
    extend: bool,
    digits: i32,
    separation: i32,
    align: BitmapAlign,
    transition_frames: i32,
    value: f64,
    log: Vec<String>,
}

/// Sanity caps for skin-supplied counts (a frame count or digit count beyond
/// these cannot be drawn anyway; unbounded values hang or overflow).
pub const MAX_FRAMES: i64 = 1 << 20;
pub const MAX_DIGITS: i64 = 64;

/// Frame shown for a relative value when not extended.
pub fn frame_for(value: f64, frames: i32, transition: i32, zero_frame: bool) -> i32 {
    let real = (frames / (transition + 1)).max(1);
    if zero_frame {
        if value > 0.0 {
            (value * (real - 1) as f64) as i32 * (transition + 1)
        } else {
            0
        }
    } else {
        (value * real as f64) as i32 * (transition + 1)
    }
}

/// The digits (least significant first) of an extended bitmap value.
pub fn digits_of(value: i64, frames: i32, transition: i32, count: i32) -> Vec<i64> {
    let real = (frames / (transition + 1)).max(1) as i64;
    let mut n = if count > 0 {
        count
    } else {
        let mut t = value.max(0);
        let mut c = 0;
        loop {
            c += 1;
            t /= if frames == 1 { 2 } else { frames as i64 };
            if t <= 0 {
                break;
            }
        }
        c
    };
    let mut v = value.max(0);
    let mut out = Vec::new();
    while n > 0 {
        out.push(v % real * (transition as i64 + 1));
        v /= if frames == 1 { 2 } else { real };
        n -= 1;
    }
    out
}

impl MeterImpl for BitmapMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        let mut log = Vec::new();
        self.image.read(opt, "Bitmap", &mut log);
        self.log.extend(log);
        self.frames = opt.int("BitmapFrames", 1).clamp(1, MAX_FRAMES as _) as i32;
        self.zero_frame = opt.bool("BitmapZeroFrame", false);
        self.extend = opt.bool("BitmapExtend", false);
        self.digits = opt.int("BitmapDigits", 0).clamp(0, MAX_DIGITS as _) as i32;
        self.separation = opt.int("BitmapSeparation", 0) as i32;
        self.align = match opt.string("BitmapAlign", "Left").trim().to_ascii_lowercase().as_str() {
            "center" => BitmapAlign::Center,
            "right" => BitmapAlign::Right,
            _ => BitmapAlign::Left,
        };
        self.transition_frames = opt.int("BitmapTransitionFrames", 0).clamp(0, MAX_FRAMES as _) as i32;
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        let mut changed = self.image.update(m);
        if let Some(me) = m.measures.first() {
            let v = if self.extend { me.value() } else { me.relative_value() };
            changed |= v != self.value;
            self.value = v;
        }
        changed
    }

    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn fixed_size(&self) -> Option<(f64, f64)> {
        let (iw, ih) = self.image.size()?;
        let extra = ((self.digits - 1) * self.separation).max(0);
        let digits = self.digits.max(1);
        let (mut w, mut h) = (iw, ih);
        if ih > iw {
            h /= self.frames.max(1);
            if self.extend {
                h = h * digits + extra;
            }
        } else {
            w /= self.frames.max(1);
            if self.extend {
                w = w * digits + extra;
            }
        }
        Some((w as f64, h as f64))
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        let Some(img) = self.image.surface() else { return };
        let (left, top, dw, dh) = base.content_rect();
        let vertical = img.height() > img.width();
        if !self.extend {
            let frame = frame_for(self.value, self.frames, self.transition_frames, self.zero_frame);
            let (sx, sy) = if vertical { (0.0, frame as f64 * dh.trunc()) } else { (frame as f64 * dw.trunc(), 0.0) };
            paint_region(cr, img, (sx, sy, dw, dh), (left, top, dw, dh));
            return;
        }
        let extra = ((self.digits - 1) * self.separation).max(0) as f64;
        let digits = self.digits.max(1) as f64;
        let (mut rw, mut rh) = (dw, dh);
        if vertical {
            rh = (dh - extra) / digits;
        } else {
            rw = (dw - extra) / digits;
        }
        let ds = digits_of(self.value as i64, self.frames, self.transition_frames, self.digits);
        let n = ds.len() as i32;
        let step = rw.trunc() as i32 + self.separation;
        let mut offset = match self.align {
            BitmapAlign::Right => 0,
            BitmapAlign::Center => n * step / 2,
            BitmapAlign::Left => n * step,
        };
        for frame in ds {
            offset -= step;
            let src = if vertical { (0.0, rh.trunc() * frame as f64, rw, rh) } else { (rw.trunc() * frame as f64, 0.0, rw, rh) };
            paint_region(cr, img, src, (left + offset as f64, top, rw, rh));
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
    fn frame_selection_vectors() {
        assert_eq!(frame_for(0.5, 10, 0, false), 5);
        assert_eq!(frame_for(0.99, 10, 0, false), 9);
        assert_eq!(frame_for(1.0, 10, 0, false), 10);
        assert_eq!(frame_for(0.0, 10, 0, true), 0);
        assert_eq!(frame_for(0.5, 10, 0, true), 4);
        assert_eq!(frame_for(0.5, 10, 1, false), 4);
    }

    #[test]
    fn extended_digits_are_least_significant_first() {
        assert_eq!(digits_of(123, 10, 0, 0), vec![3, 2, 1]);
        assert_eq!(digits_of(5, 10, 0, 3), vec![5, 0, 0]);
        assert_eq!(digits_of(0, 10, 0, 0), vec![0]);
        assert_eq!(digits_of(-4, 10, 0, 0), vec![0]);
        assert_eq!(digits_of(5, 1, 0, 0), vec![0, 0, 0]);
    }
}
