//! Chameleon plugin: colours taken from the desktop wallpaper (`Type=Desktop`)
//! or from an image file (`Type=File`, `Path=`), like the Windows plugin.
//!
//! A parent measure analyses the picture on a worker thread, and only when the
//! file changes; the result is cached per file for every skin. Child measures
//! (`Parent=`) return one colour selected by `Color=`: `Background1`,
//! `Background2`, `Foreground1`, `Foreground2`, `Average` or `Luminance`
//! (0-1, also the number value). A `Light`/`Dark` prefix (`LightBG1`, ...)
//! returns a lighter or darker shade. `Format=Hex` (default, `RRGGBB`), `Dec`
//! (`R,G,B`) or `Float` (`0.25,0.5,1`).

use super::bgshared::{self, Bg};
use super::extcmd::run_capture;
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg1: Rgb,
    pub bg2: Rgb,
    pub fg1: Rgb,
    pub fg2: Rgb,
    pub average: Rgb,
    /// Perceived brightness of the average colour, 0-1.
    pub luminance: f64,
}

/// Used when there is no picture: a neutral dark grey with white text.
pub const FALLBACK: Palette =
    Palette { bg1: [44, 44, 46], bg2: [58, 58, 60], fg1: [255, 255, 255], fg2: [200, 200, 204], average: [44, 44, 46], luminance: 0.17 };

fn brightness(c: Rgb) -> f64 {
    (0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64) / 255.0
}

/// WCAG relative luminance (linear light).
fn rel_luminance(c: Rgb) -> f64 {
    let lin = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (rel_luminance(a), rel_luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn distance(a: Rgb, b: Rgb) -> f64 {
    let d = |i: usize| a[i] as f64 - b[i] as f64;
    (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let m = |i: usize| (a[i] as f64 + (b[i] as f64 - a[i] as f64) * t).round().clamp(0.0, 255.0) as u8;
    [m(0), m(1), m(2)]
}

/// Palette of a list of pixels: the two most common colours as backgrounds and
/// the common colours that contrast best with them as foregrounds.
pub fn analyse(pixels: &[Rgb]) -> Palette {
    if pixels.is_empty() {
        return FALLBACK;
    }
    let n = pixels.len() as f64;
    let mut sum = [0f64; 3];
    // 4 bits per channel: 4096 buckets of (count, channel sums).
    let mut buckets = vec![(0u32, [0u64; 3]); 4096];
    for p in pixels {
        for i in 0..3 {
            sum[i] += p[i] as f64;
        }
        let k = ((p[0] as usize >> 4) << 8) | ((p[1] as usize >> 4) << 4) | (p[2] as usize >> 4);
        let b = &mut buckets[k];
        b.0 += 1;
        for i in 0..3 {
            b.1[i] += p[i] as u64;
        }
    }
    let average = [(sum[0] / n).round() as u8, (sum[1] / n).round() as u8, (sum[2] / n).round() as u8];
    let mut common: Vec<(u32, Rgb)> = buckets
        .iter()
        .filter(|b| b.0 > 0)
        .map(|b| (b.0, [(b.1[0] / b.0 as u64) as u8, (b.1[1] / b.0 as u64) as u8, (b.1[2] / b.0 as u64) as u8]))
        .collect();
    common.sort_by(|a, b| b.0.cmp(&a.0));
    let bg1 = common[0].1;
    let bg2 = common.iter().map(|c| c.1).find(|c| distance(*c, bg1) > 48.0).unwrap_or_else(|| mix(bg1, if brightness(bg1) > 0.5 { [0; 3] } else { [255; 3] }, 0.15));
    // Foregrounds: colours covering at least 0.5% of the picture, best contrast first.
    let min = (n * 0.005).max(1.0) as u32;
    let mut cands: Vec<Rgb> = common.iter().filter(|c| c.0 >= min).map(|c| c.1).collect();
    cands.sort_by(|a, b| contrast(*b, bg1).total_cmp(&contrast(*a, bg1)));
    let bw = if contrast([255; 3], bg1) >= contrast([0; 3], bg1) { [255; 3] } else { [0; 3] };
    let fg1 = cands.first().copied().filter(|c| contrast(*c, bg1) >= 3.0).unwrap_or(bw);
    let fg2 = cands.iter().copied().find(|c| distance(*c, fg1) > 48.0 && contrast(*c, bg1) >= 2.0).unwrap_or_else(|| mix(fg1, bg1, 0.3));
    Palette { bg1, bg2, fg1, fg2, average, luminance: brightness(average) }
}

/// Decode `path` and analyse a small thumbnail of it.
pub fn analyse_file(path: &Path) -> Option<Palette> {
    let img = image::open(path).ok()?.thumbnail(96, 96).into_rgb8();
    let px: Vec<Rgb> = img.pixels().map(|p| p.0).collect();
    Some(analyse(&px))
}

type Stamp = (SystemTime, u64);

/// Results by file, shared by every skin; small (a few wallpapers / covers).
static CACHE: Mutex<Vec<(PathBuf, Stamp, Palette)>> = Mutex::new(Vec::new());
/// One analysis at a time, so skins loading together decode a wallpaper once.
static WORKING: Mutex<()> = Mutex::new(());

fn cached_analysis(path: &Path) -> Option<Palette> {
    let stamp = crate::render::image::stamp_of(path)?;
    let find = || CACHE.lock().ok()?.iter().find(|(p, s, _)| p == path && *s == stamp).map(|e| e.2);
    if let Some(p) = find() {
        return Some(p);
    }
    let _busy = WORKING.lock();
    if let Some(p) = find() {
        return Some(p);
    }
    let pal = analyse_file(path)?;
    if let Ok(mut c) = CACHE.lock() {
        c.retain(|(p, _, _)| p != path);
        c.push((path.to_path_buf(), stamp, pal));
        if c.len() > 8 {
            c.remove(0);
        }
    }
    Some(pal)
}

/// `file:///a%20b.jpg` or `'/a b.jpg'` -> `/a b.jpg`.
fn clean_path(raw: &str) -> Option<PathBuf> {
    let t = raw.trim().trim_matches(['\'', '"']).trim();
    let t = t.strip_prefix("file://").unwrap_or(t);
    if t.is_empty() {
        return None;
    }
    let bytes = t.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&t[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
}

/// A wallpaper setting can name a folder (KDE wallpaper packages): take its
/// largest picture.
fn picture_in(p: PathBuf) -> Option<PathBuf> {
    if p.is_file() {
        return Some(p);
    }
    let dir = if p.join("contents/images").is_dir() { p.join("contents/images") } else { p };
    let mut files: Vec<(u64, PathBuf)> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|f| matches!(f.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("jpg" | "jpeg" | "png" | "webp" | "bmp")))
        .map(|f| (std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0), f))
        .collect();
    files.sort();
    files.pop().map(|f| f.1)
}

fn gsettings(schema: &str, key: &str) -> Option<String> {
    run_capture("gsettings", &["get", schema, key], Duration::from_secs(2)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "''")
}

fn kde_wallpaper(home: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(home.join(".config/plasma-org.kde.plasma.desktop-appletsrc")).ok()?;
    let mut in_image = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_image = l.contains("[Wallpaper][org.kde.image][General]");
        } else if in_image {
            if let Some(v) = l.strip_prefix("Image=") {
                return clean_path(v).and_then(picture_in);
            }
        }
    }
    None
}

/// The current wallpaper file on GNOME, Cinnamon, MATE, KDE Plasma, Xfce, feh or nitrogen.
pub fn wallpaper_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_lowercase();
    if desktop.contains("kde") {
        if let Some(p) = kde_wallpaper(&home) {
            return Some(p);
        }
    }
    if desktop.contains("xfce") {
        let list = run_capture("xfconf-query", &["-c", "xfce4-desktop", "-l"], Duration::from_secs(2)).unwrap_or_default();
        if let Some(prop) = list.lines().find(|l| l.ends_with("last-image")) {
            if let Some(p) = run_capture("xfconf-query", &["-c", "xfce4-desktop", "-p", prop], Duration::from_secs(2)).and_then(|v| clean_path(&v)) {
                return Some(p);
            }
        }
    }
    let dark = gsettings("org.gnome.desktop.interface", "color-scheme").is_some_and(|v| v.contains("dark"));
    let mut keys = vec![("org.gnome.desktop.background", "picture-uri")];
    if dark {
        keys.insert(0, ("org.gnome.desktop.background", "picture-uri-dark"));
    }
    keys.extend([("org.cinnamon.desktop.background", "picture-uri"), ("org.mate.background", "picture-filename")]);
    for (schema, key) in keys {
        if let Some(p) = gsettings(schema, key).and_then(|v| clean_path(&v)).and_then(picture_in) {
            return Some(p);
        }
    }
    if let Some(p) = kde_wallpaper(&home) {
        return Some(p);
    }
    if let Ok(t) = std::fs::read_to_string(home.join(".fehbg")) {
        if let Some(p) = t.rsplit('\'').nth(1).and_then(clean_path).filter(|p| p.is_file()) {
            return Some(p);
        }
    }
    if let Ok(t) = std::fs::read_to_string(home.join(".config/nitrogen/bg-saved.cfg")) {
        if let Some(p) = t.lines().find_map(|l| l.strip_prefix("file=")).and_then(clean_path).filter(|p| p.is_file()) {
            return Some(p);
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Desktop,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Bg1,
    Bg2,
    Fg1,
    Fg2,
    Average,
    Luminance,
}

/// How often the wallpaper setting is looked at again.
const DESKTOP_RECHECK: Duration = Duration::from_secs(15);

pub struct Chameleon {
    own: String,
    // parent
    source: Source,
    path: String,
    last_file: Option<(PathBuf, Option<Stamp>)>,
    next_check: Option<Instant>,
    result: Bg<Option<Palette>>,
    published: bool,
    // child
    parent: String,
    pick: Pick,
    shade: Option<bool>,
    format: String,
    text: String,
}

impl Chameleon {
    pub fn new() -> Chameleon {
        Chameleon {
            own: String::new(),
            source: Source::Desktop,
            path: String::new(),
            last_file: None,
            next_check: None,
            result: Bg::new(None),
            published: false,
            parent: String::new(),
            pick: Pick::Bg1,
            shade: None,
            format: String::new(),
            text: String::new(),
        }
    }

    fn start(&self, grace: Duration, job: impl FnOnce() -> Option<Palette> + Send + 'static) {
        let result = self.result.clone();
        let prev = result.get();
        self.result.start(grace, move || job().or(prev));
    }

    fn update_parent(&mut self, ctx: &MeasureCtx) {
        if !self.published {
            bgshared::publish(ctx.skin_dir, &self.own, Arc::new(self.result.clone()));
            self.published = true;
        }
        // The first analysis may block briefly so the skin starts with its colours.
        let grace = if self.result.get().is_none() { Duration::from_millis(1500) } else { Duration::ZERO };
        match self.source {
            Source::File => {
                let path = crate::paths::resolve_path(ctx.skin_dir, self.path.trim());
                let now = (path.clone(), crate::render::image::stamp_of(&path));
                if self.path.trim().is_empty() || self.last_file.as_ref() == Some(&now) {
                    return;
                }
                self.last_file = Some(now);
                self.start(grace, move || cached_analysis(&path));
            }
            Source::Desktop => {
                if self.next_check.is_some_and(|t| Instant::now() < t) {
                    return;
                }
                self.next_check = Some(Instant::now() + DESKTOP_RECHECK);
                self.start(grace, || wallpaper_path().and_then(|p| cached_analysis(&p)));
            }
        }
    }
}

impl Default for Chameleon {
    fn default() -> Self {
        Chameleon::new()
    }
}

fn parse_pick(s: &str) -> (Pick, Option<bool>) {
    let l = s.trim().to_ascii_lowercase();
    let shade = if l.starts_with("light") {
        Some(true)
    } else if l.starts_with("dark") {
        Some(false)
    } else {
        None
    };
    let pick = if l.contains("lum") || l.contains("bright") {
        Pick::Luminance
    } else if l.contains("avg") || l.contains("average") {
        Pick::Average
    } else if l.contains("fg2") || l.contains("foreground2") {
        Pick::Fg2
    } else if l.contains("fg") || l.contains("foreground") {
        Pick::Fg1
    } else if l.contains("bg2") || l.contains("background2") {
        Pick::Bg2
    } else {
        Pick::Bg1
    };
    (pick, shade)
}

pub fn format_color(c: Rgb, format: &str) -> String {
    match format.trim().to_ascii_lowercase().as_str() {
        "dec" | "decimal" | "rgb" => format!("{},{},{}", c[0], c[1], c[2]),
        "float" => format!("{:.3},{:.3},{:.3}", c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0),
        _ => format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
    }
}

impl MeasureImpl for Chameleon {
    fn read_options(&mut self, opt: &OptionReader) {
        self.own = opt.section.to_string();
        self.parent = opt.string("Parent", "").trim().to_string();
        let (pick, shade) = parse_pick(&opt.string("Color", "Background1"));
        self.pick = pick;
        self.shade = shade;
        self.format = opt.string("Format", "Hex");
        self.source = if opt.string("Type", "Desktop").trim().eq_ignore_ascii_case("file") { Source::File } else { Source::Desktop };
        self.path = opt.string("Path", "");
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        let pal = if self.parent.is_empty() {
            self.update_parent(ctx);
            self.result.get()
        } else {
            match bgshared::lookup::<Bg<Option<Palette>>>(ctx.skin_dir, &self.parent) {
                Some(bg) => bg.get(),
                None => None,
            }
        };
        let pal = pal.unwrap_or(FALLBACK);
        if self.pick == Pick::Luminance {
            let l = pal.luminance.clamp(0.0, 0.999_999);
            self.text = format!("{l:.6}");
            return l;
        }
        let mut c = match self.pick {
            Pick::Bg1 => pal.bg1,
            Pick::Bg2 => pal.bg2,
            Pick::Fg1 => pal.fg1,
            Pick::Fg2 => pal.fg2,
            _ => pal.average,
        };
        match self.shade {
            Some(true) => c = mix(c, [255; 3], 0.5),
            Some(false) => c = mix(c, [0; 3], 0.5),
            None => {}
        }
        self.text = format_color(c, &self.format);
        brightness(c)
    }

    fn string_value(&self) -> Option<String> {
        Some(self.text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_of_two_colour_picture() {
        let mut px = vec![[20u8, 30, 120]; 900];
        px.extend(vec![[250u8, 240, 230]; 100]);
        let p = analyse(&px);
        assert!(distance(p.bg1, [20, 30, 120]) < 20.0, "{p:?}");
        assert!(distance(p.fg1, [250, 240, 230]) < 20.0, "{p:?}");
        assert!(p.luminance < 0.4);
        assert_eq!(analyse(&[]), FALLBACK);
    }

    #[test]
    fn single_colour_picture_gets_readable_text() {
        let p = analyse(&vec![[240u8, 240, 240]; 50]);
        assert_eq!(p.fg1, [0, 0, 0]);
        assert!(p.luminance > 0.9);
    }

    #[test]
    fn names_formats_and_paths() {
        assert_eq!(parse_pick("Foreground1"), (Pick::Fg1, None));
        assert_eq!(parse_pick("LightBG2"), (Pick::Bg2, Some(true)));
        assert_eq!(parse_pick("Luminance"), (Pick::Luminance, None));
        assert_eq!(parse_pick("Average"), (Pick::Average, None));
        assert_eq!(format_color([255, 16, 0], ""), "FF1000");
        assert_eq!(format_color([255, 16, 0], "Dec"), "255,16,0");
        assert_eq!(clean_path("'file:///home/a%20b/x.jpg'"), Some(PathBuf::from("/home/a b/x.jpg")));
        assert_eq!(clean_path("''"), None);
    }
}
