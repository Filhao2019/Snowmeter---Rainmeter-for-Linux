//! A reusable "named image with processing options" for the meters that take
//! one or more images (Bar, Button, Histogram, Rotator, Bitmap).

use super::MeterInputs;
use crate::parser::OptionReader;
use crate::render::image::{process, to_surface, ImageOptions};
use std::path::PathBuf;

#[derive(Default)]
pub struct ImageSlot {
    pub name: String,
    pub options: ImageOptions,
    key: Option<String>,
    surface: Option<cairo::ImageSurface>,
    /// Alpha channel of the processed image (row-major), for pixel hit tests.
    alpha: Vec<u8>,
}

/// `.png` is appended when the last path component has no dot.
pub fn with_default_extension(path: &str) -> String {
    let last = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if last.contains('.') {
        path.to_string()
    } else {
        format!("{path}.png")
    }
}

impl ImageSlot {
    /// Read `<prefix>Image` plus the prefixed processing options.
    pub fn read(&mut self, opt: &OptionReader, prefix: &str, log: &mut Vec<String>) {
        self.name = opt.string(&format!("{prefix}Image"), "");
        self.options = if self.name.is_empty() {
            ImageOptions::default()
        } else {
            ImageOptions::read_prefixed(opt, prefix, log)
        };
        self.key = None;
        if self.name.is_empty() {
            self.surface = None;
        }
    }

    /// Load (or reload after a change) the image. Returns true if it changed.
    pub fn update(&mut self, m: &MeterInputs) -> bool {
        if self.name.trim().is_empty() {
            return false;
        }
        let path: PathBuf = crate::paths::resolve_path(&m.res.skin_dir, &with_default_extension(self.name.trim()));
        let mut log = Vec::new();
        let raw = m.res.images.load(&path, &mut log);
        for l in log {
            m.log(l);
        }
        let key = raw.as_ref().map(|r| format!("{}|{:?}|{:p}", path.display(), self.options, std::rc::Rc::as_ptr(r)));
        if key == self.key && (self.surface.is_some() || key.is_none()) {
            return false;
        }
        self.key = key;
        let processed = raw.map(|r| process(&r, &self.options));
        self.alpha = processed.as_ref().map(|p| p.rgba.chunks_exact(4).map(|px| px[3]).collect()).unwrap_or_default();
        self.surface = processed.and_then(|p| to_surface(&p));
        true
    }

    pub fn surface(&self) -> Option<&cairo::ImageSurface> {
        self.surface.as_ref()
    }

    /// Alpha of the pixel at `(x, y)`; 0 outside the image.
    pub fn alpha_at(&self, x: i32, y: i32) -> u8 {
        let Some((w, h)) = self.size() else { return 0 };
        if x < 0 || y < 0 || x >= w || y >= h {
            return 0;
        }
        self.alpha.get((y * w + x) as usize).copied().unwrap_or(0)
    }

    pub fn size(&self) -> Option<(i32, i32)> {
        self.surface.as_ref().map(|s| (s.width(), s.height()))
    }
}
