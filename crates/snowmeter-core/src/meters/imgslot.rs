//! A reusable "named image with processing options" for the meters that take
//! one or more images (Bar, Button, Histogram, Rotator, Bitmap).

use super::MeterInputs;
use crate::parser::OptionReader;
use crate::render::image::{process, stamp_of, to_surface, ImageOptions};
use std::path::PathBuf;

#[derive(Default)]
pub struct ImageSlot {
    pub name: String,
    pub options: ImageOptions,
    key: Option<String>,
    surface: Option<cairo::ImageSurface>,
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
        let stamp = stamp_of(&path);
        let key = stamp.map(|st| format!("{}|{:?}|{:?}", path.display(), self.options, st));
        if key.is_some() && key == self.key && self.surface.is_some() {
            return false;
        }
        let mut log = Vec::new();
        let raw = m.res.images.load(&path, &mut log);
        for l in log {
            m.log(l);
        }
        let key = if raw.is_some() { key } else { None };
        if key == self.key && (self.surface.is_some() || key.is_none()) {
            return false;
        }
        self.key = key;
        self.surface = raw.and_then(|r| to_surface(&process(&r, &self.options)));
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
        // ARGB32 keeps alpha in the top byte; premultiplying leaves it unchanged.
        let Some(surf) = &self.surface else { return 0 };
        let i = y as usize * surf.stride() as usize + x as usize * 4;
        let mut a = 0;
        let _ = surf.with_data(|d| {
            if let Some(px) = d.get(i..i + 4) {
                a = (u32::from_ne_bytes([px[0], px[1], px[2], px[3]]) >> 24) as u8;
            }
        });
        a
    }

    pub fn size(&self) -> Option<(i32, i32)> {
        self.surface.as_ref().map(|s| (s.width(), s.height()))
    }
}
