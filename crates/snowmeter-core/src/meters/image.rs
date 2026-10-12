//! The Image meter (meters.md section 3).

use super::{MeterBase, MeterImpl, MeterInputs};
use crate::parser::OptionReader;
use crate::render::image::{draw_image, process, to_surface, DrawParams, ImageOptions};
use crate::render::Resources;
use std::path::PathBuf;

#[derive(Default)]
pub struct ImageMeter {
    image_name: String,
    image_path: String,
    tile: bool,
    preserve: u8,
    margins: [i32; 4],
    options: ImageOptions,
    log: Vec<String>,
    /// What the current surface was built from.
    loaded_key: Option<String>,
    surface: Option<cairo::ImageSurface>,
}

fn with_default_extension(path: &str) -> String {
    let last = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if last.contains('.') {
        path.to_string()
    } else {
        format!("{path}.png")
    }
}

impl ImageMeter {
    fn file_name(&self, m: &MeterInputs) -> String {
        let fmt = |i: usize| m.measures[i].string_value(0, 1.0, 0, false);
        if m.measures.is_empty() {
            return self.image_name.clone();
        }
        if self.image_name.is_empty() {
            return fmt(0);
        }
        let mut name = self.image_name.clone();
        let mut replaced = false;
        if name.contains('%') {
            for i in (0..m.measures.len()).rev() {
                let tag = format!("%{}", i + 1);
                if name.contains(&tag) {
                    name = name.replace(&tag, &fmt(i));
                    replaced = true;
                }
            }
        }
        if replaced {
            name
        } else {
            fmt(0)
        }
    }
}

impl MeterImpl for ImageMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        self.image_name = opt.string("ImageName", "");
        let mut path = opt.string("ImagePath", "");
        if path.is_empty() {
            path = opt.string("Path", "");
        }
        self.image_path = path;
        self.tile = opt.bool("Tile", false);
        self.preserve = opt.int("PreserveAspectRatio", 0).clamp(0, 255) as u8;
        let mut log = Vec::new();
        self.options = ImageOptions::read(opt, &mut log);
        self.log.extend(log);
        let margins = opt.string("ScaleMargins", "");
        let parts: Vec<i32> = margins.split(',').map(|p| crate::numparse::parse_i32_prefix(p.trim()).unwrap_or(0) as i32).collect();
        self.margins = if parts.len() >= 4 { [parts[0], parts[1], parts[2], parts[3]] } else { [0; 4] };
        self.loaded_key = None;
        if opt.has("MaskImageName") {
            self.log.push(format!("[{}] MaskImageName is not supported", opt.section));
        }
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        let name = self.file_name(m);
        if name.trim().is_empty() {
            let changed = self.surface.is_some();
            self.surface = None;
            self.loaded_key = None;
            return changed;
        }
        let full = with_default_extension(&format!("{}{}", self.image_path, name.trim()));
        let path: PathBuf = crate::paths::resolve_path(&m.res.skin_dir, &full);
        let mut log = Vec::new();
        let raw = m.res.images.load(&path, &mut log);
        for l in log {
            m.log(l);
        }
        let key = raw.as_ref().map(|r| format!("{}|{:?}|{}x{}|{:p}", path.display(), self.options, r.w, r.h, std::rc::Rc::as_ptr(r)));
        if key == self.loaded_key && self.surface.is_some() {
            return false;
        }
        self.loaded_key = key;
        self.surface = raw.and_then(|r| to_surface(&process(&r, &self.options)));
        true
    }

    fn auto_size(&self, base: &MeterBase, _res: &Resources) -> (f64, f64) {
        let (iw, ih) = self.surface.as_ref().map(|s| (s.width() as i64, s.height() as i64)).unwrap_or((0, 0));
        let pw = base.pad_w() as i64;
        let ph = base.pad_h() as i64;
        match (base.w_given, base.h_given) {
            (Some(_), Some(_)) => (0.0, 0.0),
            (Some(w), None) => {
                let mw = w as i64 + pw;
                let h = if self.tile { ih } else if iw > 0 { mw * ih / iw } else { 0 };
                (0.0, h as f64)
            }
            (None, Some(h)) => {
                let mh = h as i64 + ph;
                let w = if self.tile { iw } else if ih > 0 { mh * iw / ih } else { 0 };
                (w as f64, 0.0)
            }
            (None, None) => (iw as f64, ih as f64),
        }
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        let Some(surf) = &self.surface else { return };
        let (x, y, w, h) = base.content_rect();
        draw_image(
            cr,
            surf,
            x,
            y,
            w,
            h,
            &DrawParams {
                tile: self.tile,
                preserve: self.preserve,
                both_given: base.w_given.is_some() && base.h_given.is_some(),
                margins: self.margins,
            },
        );
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}
