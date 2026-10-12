//! Image loading (cached), the Rainmeter processing chain and drawing.

use crate::color::Color;
use crate::parser::OptionReader;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

/// Straight (non-premultiplied) RGBA8 pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct RawImage {
    pub w: i32,
    pub h: i32,
    pub rgba: Vec<u8>,
}

/// Largest image side and pixel count we ever allocate (skin-controlled sizes).
const MAX_DIM: i32 = 16384;
const MAX_PIXELS: usize = 1 << 25;

impl RawImage {
    pub fn new(w: i32, h: i32) -> RawImage {
        let (w, h) = (w.clamp(0, MAX_DIM), h.clamp(0, MAX_DIM));
        let n = (w as usize).saturating_mul(h as usize).saturating_mul(4).min(MAX_PIXELS * 4);
        // Never allocate more than the cap; callers index within `rgba.len()`.
        let (w, h) = if (w as usize) * (h as usize) > MAX_PIXELS { (0, 0) } else { (w, h) };
        RawImage { w, h, rgba: vec![0; if w == 0 || h == 0 { 0 } else { n }] }
    }
}

struct Entry {
    stamp: Option<(SystemTime, u64)>,
    img: Option<Rc<RawImage>>,
}

/// Decoded images by resolved path; reloads when size or mtime change and
/// reports a failing path only once.
#[derive(Default)]
pub struct ImageCache {
    map: RefCell<HashMap<PathBuf, Entry>>,
}

fn stamp_of(path: &Path) -> Option<(SystemTime, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

impl ImageCache {
    pub fn new() -> ImageCache {
        ImageCache::default()
    }

    pub fn load(&self, path: &Path, log: &mut Vec<String>) -> Option<Rc<RawImage>> {
        let stamp = stamp_of(path);
        let mut map = self.map.borrow_mut();
        if let Some(e) = map.get(path) {
            if e.stamp == stamp {
                return e.img.clone();
            }
        }
        let img = match image::open(path) {
            Ok(d) => {
                let rgba = d.to_rgba8();
                let (w, h) = rgba.dimensions();
                Some(Rc::new(RawImage { w: w as i32, h: h as i32, rgba: rgba.into_raw() }))
            }
            Err(e) => {
                let first = map.get(path).is_none();
                if first {
                    log.push(format!("Image {} could not be loaded: {e}", path.display()));
                }
                None
            }
        };
        map.insert(path.to_path_buf(), Entry { stamp, img: img.clone() });
        img
    }
}

/// Premultiplied BGRA (cairo ARGB32) surface from straight RGBA.
pub fn to_surface(raw: &RawImage) -> Option<cairo::ImageSurface> {
    if raw.w <= 0 || raw.h <= 0 {
        return None;
    }
    let stride = cairo::Format::ARgb32.stride_for_width(raw.w as u32).ok()?;
    let mut data = vec![0u8; stride as usize * raw.h as usize];
    for y in 0..raw.h as usize {
        for x in 0..raw.w as usize {
            let s = (y * raw.w as usize + x) * 4;
            let (r, g, b, a) = (raw.rgba[s] as u32, raw.rgba[s + 1] as u32, raw.rgba[s + 2] as u32, raw.rgba[s + 3] as u32);
            let pm = |c: u32| ((c * a + 127) / 255) as u8;
            let v = ((a << 24) | ((pm(r) as u32) << 16) | ((pm(g) as u32) << 8) | pm(b) as u32).to_ne_bytes();
            let d = y * stride as usize + x * 4;
            data[d..d + 4].copy_from_slice(&v);
        }
    }
    cairo::ImageSurface::create_for_data(data, cairo::Format::ARgb32, raw.w, raw.h, stride).ok()
}

pub fn from_surface(surf: &mut cairo::ImageSurface) -> RawImage {
    let (w, h) = (surf.width(), surf.height());
    let stride = surf.stride() as usize;
    let mut out = RawImage::new(w, h);
    surf.flush();
    if let Ok(data) = surf.data() {
        for y in 0..h as usize {
            for x in 0..w as usize {
                let s = y * stride + x * 4;
                let v = u32::from_ne_bytes([data[s], data[s + 1], data[s + 2], data[s + 3]]);
                let a = v >> 24;
                let un = |c: u32| if a == 0 { 0 } else { ((c * 255 + a / 2) / a).min(255) as u8 };
                let d = (y * w as usize + x) * 4;
                out.rgba[d] = un((v >> 16) & 255);
                out.rgba[d + 1] = un((v >> 8) & 255);
                out.rgba[d + 2] = un(v & 255);
                out.rgba[d + 3] = a as u8;
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flip {
    #[default]
    None,
    Horizontal,
    Vertical,
    Both,
}

/// The option set of GeneralImage (3.4); `prefix` supports `Mask...`/`Bar...`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageOptions {
    pub crop: Option<[i32; 4]>,
    pub crop_origin: i32,
    pub greyscale: bool,
    pub tint: Color,
    pub alpha: Option<i32>,
    pub matrix_rows: [Option<[f32; 5]>; 5],
    pub flip: Flip,
    pub rotate: f64,
}

impl Default for ImageOptions {
    fn default() -> Self {
        ImageOptions {
            crop: None,
            crop_origin: 1,
            greyscale: false,
            tint: Color { r: 255, g: 255, b: 255, a: 255 },
            alpha: None,
            matrix_rows: [None; 5],
            flip: Flip::None,
            rotate: 0.0,
        }
    }
}

impl ImageOptions {
    pub fn read(opt: &OptionReader, log: &mut Vec<String>) -> ImageOptions {
        Self::read_prefixed(opt, "", log)
    }

    /// Same options with a meter prefix (`Bar`, `Primary`, `Button`, ...).
    pub fn read_prefixed(opt: &OptionReader, prefix: &str, log: &mut Vec<String>) -> ImageOptions {
        let k = |name: &str| format!("{prefix}{name}");
        let mut o = ImageOptions::default();
        let crop = opt.string(&k("ImageCrop"), "");
        if !crop.trim().is_empty() {
            let parts: Vec<&str> = crop.split(',').collect();
            let num = |s: &str| opt.parser.parse_formula(s.trim(), opt.sv).map(|v| v as i32);
            if parts.len() >= 4 {
                let vals: Vec<Option<i32>> = parts.iter().take(5).map(|p| num(p)).collect();
                if vals[..4].iter().all(|v| v.is_some()) {
                    o.crop = Some([vals[0].unwrap(), vals[1].unwrap(), vals[2].unwrap(), vals[3].unwrap()]);
                    o.crop_origin = vals.get(4).copied().flatten().unwrap_or(1);
                    if !(1..=5).contains(&o.crop_origin) {
                        log.push(format!("[{}] ImageCrop origin {} is invalid", opt.section, o.crop_origin));
                        o.crop_origin = 1;
                    }
                }
            } else {
                log.push(format!("[{}] ImageCrop needs 4 values", opt.section));
            }
        }
        o.greyscale = opt.bool(&k("Greyscale"), false);
        o.tint = opt.color(&k("ImageTint"), o.tint);
        if opt.has(&k("ImageAlpha")) && !opt.string(&k("ImageAlpha"), "").is_empty() {
            o.alpha = Some(opt.int(&k("ImageAlpha"), 255).clamp(0, 255) as i32);
        }
        for (i, row) in o.matrix_rows.iter_mut().enumerate() {
            let s = opt.string(&k(&format!("ColorMatrix{}", i + 1)), "");
            let vals: Vec<f32> = s.split(';').filter_map(|v| v.trim().parse::<f32>().ok()).collect();
            if s.split(';').count() >= 5 && vals.len() >= 5 {
                *row = Some([vals[0], vals[1], vals[2], vals[3], vals[4]]);
            }
        }
        o.flip = match opt.string(&k("ImageFlip"), "None").to_ascii_lowercase().as_str() {
            "horizontal" => Flip::Horizontal,
            "vertical" => Flip::Vertical,
            "both" => Flip::Both,
            _ => Flip::None,
        };
        o.rotate = opt.float(&k("ImageRotate"), 0.0);
        o
    }

    fn matrix(&self) -> Option<[[f32; 4]; 5]> {
        let alpha = self.alpha.unwrap_or(self.tint.a as i32) as f32 / 255.0;
        let t = self.tint;
        let mut m = [
            [t.r as f32 / 255.0, 0.0, 0.0, 0.0],
            [0.0, t.g as f32 / 255.0, 0.0, 0.0],
            [0.0, 0.0, t.b as f32 / 255.0, 0.0],
            [0.0, 0.0, 0.0, alpha],
            [0.0, 0.0, 0.0, 0.0],
        ];
        for (i, row) in self.matrix_rows.iter().enumerate() {
            if let Some(r) = row {
                m[i] = [r[0], r[1], r[2], r[3]];
            }
        }
        let identity = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 0.0],
        ];
        if m == identity {
            None
        } else {
            Some(m)
        }
    }
}

fn crop(raw: &RawImage, c: [i32; 4], origin: i32) -> RawImage {
    let (ox, oy) = match origin {
        2 => (raw.w, 0),
        3 => (raw.w, raw.h),
        4 => (0, raw.h),
        5 => (raw.w / 2, raw.h / 2),
        _ => (0, 0),
    };
    let (cw, ch) = (c[2], c[3]);
    if cw < 0 || ch < 0 {
        return raw.clone();
    }
    if cw as i64 * ch as i64 > MAX_PIXELS as i64 || cw > MAX_DIM || ch > MAX_DIM {
        log::warn!("ImageCrop {cw}x{ch} is too large, ignored");
        return raw.clone();
    }
    let (sx, sy) = (ox as i64 + c[0] as i64, oy as i64 + c[1] as i64);
    let mut out = RawImage::new(cw, ch);
    for y in 0..ch as i64 {
        for x in 0..cw as i64 {
            let (px, py) = (sx + x, sy + y);
            if px >= 0 && py >= 0 && px < raw.w as i64 && py < raw.h as i64 {
                let s = ((py * raw.w as i64 + px) * 4) as usize;
                let d = ((y * cw as i64 + x) * 4) as usize;
                out.rgba[d..d + 4].copy_from_slice(&raw.rgba[s..s + 4]);
            }
        }
    }
    out
}

fn flip(raw: &RawImage, f: Flip) -> RawImage {
    let mut out = RawImage::new(raw.w, raw.h);
    let (fh, fv) = (matches!(f, Flip::Horizontal | Flip::Both), matches!(f, Flip::Vertical | Flip::Both));
    for y in 0..raw.h {
        for x in 0..raw.w {
            let sx = if fh { raw.w - 1 - x } else { x };
            let sy = if fv { raw.h - 1 - y } else { y };
            let s = ((sy * raw.w + sx) * 4) as usize;
            let d = ((y * raw.w + x) * 4) as usize;
            out.rgba[d..d + 4].copy_from_slice(&raw.rgba[s..s + 4]);
        }
    }
    out
}

fn rotate(raw: &RawImage, degrees: f64) -> RawImage {
    let rad = degrees.to_radians();
    let (c, s) = (rad.cos().abs(), rad.sin().abs());
    let nw = ((raw.w as f64 * c + raw.h as f64 * s) + 1e-6) as i32;
    let nh = ((raw.w as f64 * s + raw.h as f64 * c) + 1e-6) as i32;
    let (Some(src), Ok(mut dst)) = (to_surface(raw), cairo::ImageSurface::create(cairo::Format::ARgb32, nw.max(1), nh.max(1))) else {
        return raw.clone();
    };
    {
        let Ok(cr) = cairo::Context::new(&dst) else { return raw.clone() };
        cr.translate(nw as f64 / 2.0, nh as f64 / 2.0);
        cr.rotate(rad);
        cr.translate(-raw.w as f64 / 2.0, -raw.h as f64 / 2.0);
        let _ = cr.set_source_surface(&src, 0.0, 0.0);
        cr.source().set_filter(cairo::Filter::Bilinear);
        let _ = cr.paint();
    }
    from_surface(&mut dst)
}

/// Run the fixed chain: crop, greyscale, colour matrix, flip, rotate.
pub fn process(raw: &RawImage, o: &ImageOptions) -> RawImage {
    let mut img = match o.crop {
        Some(c) => crop(raw, c, o.crop_origin),
        None => raw.clone(),
    };
    if o.greyscale {
        for px in img.rgba.chunks_exact_mut(4) {
            let y = 0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32;
            let y = y.round().clamp(0.0, 255.0) as u8;
            px[0] = y;
            px[1] = y;
            px[2] = y;
        }
    }
    if let Some(m) = o.matrix() {
        for px in img.rgba.chunks_exact_mut(4) {
            let v = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0, px[3] as f32 / 255.0, 1.0];
            for k in 0..4 {
                let sum: f32 = (0..5).map(|i| v[i] * m[i][k]).sum();
                px[k] = (sum.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
    if o.flip != Flip::None {
        img = flip(&img, o.flip);
    }
    if o.rotate != 0.0 {
        img = rotate(&img, o.rotate);
    }
    img
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DrawParams {
    pub tile: bool,
    /// 0 stretch, 1 fit, 2 crop (only honoured when both W and H were given).
    pub preserve: u8,
    pub both_given: bool,
    pub margins: [i32; 4],
}

pub fn paint_region(cr: &cairo::Context, src: &cairo::ImageSurface, s: (f64, f64, f64, f64), d: (f64, f64, f64, f64)) {
    if s.2 <= 0.0 || s.3 <= 0.0 || d.2 <= 0.0 || d.3 <= 0.0 {
        return;
    }
    let _ = cr.save();
    cr.rectangle(d.0, d.1, d.2, d.3);
    cr.clip();
    cr.translate(d.0, d.1);
    cr.scale(d.2 / s.2, d.3 / s.3);
    cr.translate(-s.0, -s.1);
    let _ = cr.set_source_surface(src, 0.0, 0.0);
    let p = cr.source();
    p.set_extend(cairo::Extend::Pad);
    p.set_filter(cairo::Filter::Best);
    let _ = cr.paint();
    let _ = cr.restore();
}

/// Draw `src` into the padded rect `(x, y, w, h)` (meters.md 3.3).
pub fn draw_image(cr: &cairo::Context, src: &cairo::ImageSurface, x: f64, y: f64, w: f64, h: f64, p: &DrawParams) {
    let (iw, ih) = (src.width() as f64, src.height() as f64);
    if w <= 0.0 || h <= 0.0 || iw <= 0.0 || ih <= 0.0 {
        return;
    }
    let m = p.margins;
    if w == iw && h == ih && m == [0; 4] {
        let _ = cr.set_source_surface(src, x, y);
        cr.rectangle(x, y, w, h);
        let _ = cr.fill();
        return;
    }
    if p.tile {
        let _ = cr.save();
        cr.rectangle(x, y, w, h);
        cr.clip();
        let _ = cr.set_source_surface(src, x, y);
        cr.source().set_extend(cairo::Extend::Repeat);
        let _ = cr.paint();
        let _ = cr.restore();
        return;
    }
    if p.both_given && (p.preserve > 0) && m == [0; 4] {
        if p.preserve == 2 {
            let s = (w / iw).max(h / ih);
            let (sw, sh) = (w / s, h / s);
            paint_region(cr, src, ((iw - sw) / 2.0, (ih - sh) / 2.0, sw, sh), (x, y, w, h));
        } else {
            let s = (w / iw).min(h / ih);
            let (dw, dh) = (iw * s, ih * s);
            paint_region(cr, src, (0.0, 0.0, iw, ih), (x + (w - dw) / 2.0, y + (h - dh) / 2.0, dw, dh));
        }
        return;
    }
    if m == [0; 4] {
        paint_region(cr, src, (0.0, 0.0, iw, ih), (x, y, w, h));
        return;
    }
    let (l, t, r, b) = (m[0] as f64, m[1] as f64, m[2] as f64, m[3] as f64);
    let sx = [0.0, l, iw - r, iw];
    let sy = [0.0, t, ih - b, ih];
    let dx = [x, x + l, (x + w - r).round(), x + w];
    let dy = [y, y + t, (y + h - b).round(), y + h];
    for j in 0..3 {
        for i in 0..3 {
            paint_region(
                cr,
                src,
                (sx[i], sy[j], sx[i + 1] - sx[i], sy[j + 1] - sy[j]),
                (dx[i], dy[j], dx[i + 1] - dx[i], dy[j + 1] - dy[j]),
            );
        }
    }
}
