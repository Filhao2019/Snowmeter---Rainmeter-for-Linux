//! Meters: the common `Meter` wrapper plus the per-type implementations
//! behind the `MeterImpl` trait. To add a type, implement `MeterImpl` and add
//! it to `create_impl`.

pub mod base;
pub mod image;
pub mod string;

use crate::measures::Measure;
use crate::parser::OptionReader;
use crate::render::text::{HAlign, VAlign};
use crate::render::Resources;
use std::cell::RefCell;

pub use base::MeterBase;

/// What a meter sees while updating: the measures bound with
/// `MeasureName`, `MeasureName2`, ... (invalid names are skipped, which
/// shifts the numbering, like Rainmeter stops at the first invalid one).
pub struct MeterInputs<'a> {
    pub measures: Vec<&'a Measure>,
    pub res: &'a Resources,
    pub log: &'a RefCell<Vec<String>>,
}

impl MeterInputs<'_> {
    pub fn log(&self, msg: String) {
        self.log.borrow_mut().push(msg);
    }
}

pub trait MeterImpl {
    /// Read the type-specific options (the base ones are read by `Meter`).
    fn read_options(&mut self, opt: &OptionReader);
    /// Called on every fired update. Returns true if anything changed.
    fn update(&mut self, m: &MeterInputs) -> bool;
    /// Content size `(w, h)` excluding padding, used for the axes where
    /// `W`/`H` were not given. `base.w_given`/`h_given` are available.
    fn auto_size(&self, base: &MeterBase, res: &Resources) -> (f64, f64);
    /// Draw into `base.content_rect()`; the background is already painted.
    fn draw(&self, cr: &cairo::Context, base: &MeterBase, res: &Resources);

    // Optional hooks.
    /// How the anchor `(X, Y)` relates to the drawn rect (String only).
    fn align(&self) -> (HAlign, VAlign) {
        (HAlign::Left, VAlign::Top)
    }
    /// The text currently shown (String meters), for tooltips and tests.
    fn text(&self) -> Option<&str> {
        None
    }
    /// Messages produced since the last call (bad options, missing files).
    fn take_log(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// A valid meter type that is not drawn yet (Bar, Line, ... until M4).
struct Unsupported;

impl MeterImpl for Unsupported {
    fn read_options(&mut self, _opt: &OptionReader) {}
    fn update(&mut self, _m: &MeterInputs) -> bool {
        false
    }
    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        (0.0, 0.0)
    }
    fn draw(&self, _cr: &cairo::Context, _base: &MeterBase, _res: &Resources) {}
}

const DEFERRED_TYPES: [&str; 8] = ["BAR", "BITMAP", "HISTOGRAM", "LINE", "ROUNDLINE", "ROTATOR", "BUTTON", "SHAPE"];

/// Factory. `Err` carries the reason a section is skipped.
pub fn create_impl(type_name: &str) -> Result<Box<dyn MeterImpl>, String> {
    let up = type_name.trim().to_ascii_uppercase();
    match up.as_str() {
        "STRING" => Ok(Box::new(string::StringMeter::default())),
        "IMAGE" => Ok(Box::new(image::ImageMeter::default())),
        t if DEFERRED_TYPES.contains(&t) => Ok(Box::new(Unsupported)),
        _ => Err(format!("Meter={type_name} is not valid")),
    }
}

/// A meter section: the shared base plus the type-specific part.
pub struct Meter {
    pub base: MeterBase,
    pub imp: Box<dyn MeterImpl>,
    /// Messages from the last read/update, drained by the skin.
    pub log: Vec<String>,
}

impl Meter {
    pub fn new(name: &str, type_name: &str, imp: Box<dyn MeterImpl>) -> Meter {
        Meter { base: MeterBase::new(name, type_name), imp, log: Vec::new() }
    }

    pub fn read_options(&mut self, opt: &OptionReader, default_divider: i64) {
        self.base.read(opt, default_divider);
        self.imp.read_options(opt);
        let l = self.imp.take_log();
        self.log.extend(l);
    }

    /// Run the type's update, then recompute `w`/`h`.
    pub fn update(&mut self, inputs: &MeterInputs) -> bool {
        let changed = self.imp.update(inputs);
        self.resize(inputs.res);
        let l = self.imp.take_log();
        self.log.extend(l);
        changed
    }

    /// `m_W`/`m_H` from the given values or the type's automatic size.
    pub fn resize(&mut self, res: &Resources) {
        let (aw, ah) = self.imp.auto_size(&self.base, res);
        let b = &mut self.base;
        b.w = match b.w_given {
            Some(w) => (w + b.pad_w()) as f64,
            None => aw.trunc() + b.pad_w() as f64,
        };
        b.h = match b.h_given {
            Some(h) => (h + b.pad_h()) as f64,
            None => ah.trunc() + b.pad_h() as f64,
        };
    }

    /// Place the drawn rect relative to the anchor (integer math like Rainmeter).
    pub fn update_rect(&mut self) {
        let (ha, va) = self.imp.align();
        let b = &mut self.base;
        let (w, h) = (b.w as i64, b.h as i64);
        b.rect_x = b.x
            + match ha {
                HAlign::Left | HAlign::Justify => 0.0,
                HAlign::Center => -((w / 2) as f64),
                HAlign::Right => -(w as f64),
            };
        b.rect_y = b.y
            + match va {
                VAlign::Top => 0.0,
                VAlign::Center => -((h / 2) as f64),
                VAlign::Bottom => -(h as f64),
            };
    }

    pub fn draw(&self, cr: &cairo::Context, res: &Resources) {
        if self.base.hidden {
            return;
        }
        let _ = cr.save();
        self.base.draw_background(cr);
        self.imp.draw(cr, &self.base, res);
        let _ = cr.restore();
    }
}
