//! Font family resolution (Windows names -> installed Linux families) and
//! registration of skin-local font files with fontconfig.

use pango::prelude::*;
use pango::glib::translate::ToGlibPtr;
use std::collections::HashSet;
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[link(name = "fontconfig")]
extern "C" {
    fn FcConfigGetCurrent() -> *mut c_void;
    fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> c_int;
}

#[link(name = "pangoft2-1.0")]
extern "C" {
    fn pango_fc_font_map_config_changed(fontmap: *mut c_void);
}

static REGISTERED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

const FONT_EXTENSIONS: [&str; 5] = ["ttf", "otf", "ttc", "otc", "pfb"];

fn is_font_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| FONT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn collect_fonts(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_fonts(&p, out, depth + 1);
        } else if is_font_file(&p) {
            out.push(p);
        }
    }
}

/// Register font files with fontconfig and tell Pango to reload. Returns the
/// number of files that were newly added.
pub fn register_font_files(files: &[PathBuf]) -> usize {
    let mut added = 0;
    {
        let mut done = REGISTERED.lock().unwrap_or_else(|e| e.into_inner());
        for f in files {
            if done.contains(f) {
                continue;
            }
            let Some(c) = f.to_str().and_then(|s| CString::new(s).ok()) else { continue };
            // SAFETY: plain fontconfig calls with a valid NUL-terminated path.
            let ok = unsafe { FcConfigAppFontAddFile(FcConfigGetCurrent(), c.as_ptr()) };
            if ok != 0 {
                done.push(f.clone());
                added += 1;
            } else {
                log::warn!("fontconfig could not load {}", f.display());
            }
        }
    }
    // Every thread has its own default font map, so refresh ours even when
    // nothing new was added.
    super::text::clear_metrics_cache();
    let fm = pangocairo::FontMap::default();
    let ptr: *mut pango::ffi::PangoFontMap = fm.to_glib_none().0;
    // SAFETY: the default pangocairo font map on Linux is a PangoFcFontMap.
    unsafe { pango_fc_font_map_config_changed(ptr as *mut c_void) };
    added
}

/// Load every font under `<resources>/Fonts` (recursive) plus the `LocalFontN`
/// files (each tried relative to the program `Fonts` folder, then the skin).
pub fn load_skin_fonts(resources_dir: &Path, skin_dir: &Path, program_dir: &Path, local: &[String]) {
    let mut files = Vec::new();
    collect_fonts(&resources_dir.join("Fonts"), &mut files, 0);
    for name in local {
        let a = crate::paths::resolve_path(&program_dir.join("Fonts"), name);
        let b = crate::paths::resolve_path(skin_dir, name);
        let found = [a, b].into_iter().find(|p| p.is_file());
        match found {
            Some(p) => files.push(p),
            None => log::warn!("LocalFont {name} not found"),
        }
    }
    register_font_files(&files);
}

/// Windows family name -> candidate Linux families (first installed wins),
/// plus an optional weight implied by the style name.
const MAP: &[(&str, &[&str])] = &[
    ("segoe ui", &["Noto Sans", "DejaVu Sans"]),
    ("tahoma", &["Noto Sans", "DejaVu Sans"]),
    ("verdana", &["Noto Sans", "DejaVu Sans"]),
    ("trebuchet ms", &["Noto Sans", "DejaVu Sans"]),
    ("arial", &["Liberation Sans"]),
    ("helvetica", &["Liberation Sans"]),
    ("times new roman", &["Liberation Serif"]),
    ("courier new", &["Liberation Mono"]),
    ("consolas", &["DejaVu Sans Mono"]),
    ("lucida console", &["DejaVu Sans Mono"]),
    ("calibri", &["Carlito", "Liberation Sans"]),
];

const WEIGHT_SUFFIX: &[(&str, i32)] = &[
    ("semilight", 350),
    ("semibold", 600),
    ("light", 300),
    ("black", 900),
    ("bold", 700),
    ("thin", 100),
    ("medium", 500),
];

/// Knows which families exist and maps the ones that do not.
#[derive(Default)]
pub struct FontResolver {
    installed: HashSet<String>,
}

impl FontResolver {
    /// Snapshot the families known to Pango right now (call after registering
    /// skin fonts).
    pub fn new() -> FontResolver {
        let mut r = FontResolver::default();
        r.refresh();
        r
    }

    pub fn refresh(&mut self) {
        let fm = pangocairo::FontMap::default();
        self.installed = fm.list_families().iter().map(|f| f.name().to_lowercase()).collect();
    }

    pub fn with_families(names: &[&str]) -> FontResolver {
        FontResolver { installed: names.iter().map(|n| n.to_lowercase()).collect() }
    }

    pub fn is_installed(&self, family: &str) -> bool {
        self.installed.contains(&family.trim().to_lowercase())
    }

    /// Returns the family to ask Pango for and a weight implied by the
    /// requested name (`Segoe UI Light` -> 300).
    pub fn resolve(&self, requested: &str) -> (String, Option<i32>) {
        let name = requested.trim();
        let name = if name.is_empty() { "Arial" } else { name };
        if self.is_installed(name) {
            return (name.to_string(), None);
        }
        let lower = name.to_lowercase();
        let (base, weight) = WEIGHT_SUFFIX
            .iter()
            .find_map(|(suf, w)| {
                lower.strip_suffix(suf).map(|b| (b.trim().to_string(), Some(*w)))
            })
            .unwrap_or((lower.clone(), None));
        if let Some((_, cands)) = MAP.iter().find(|(k, _)| *k == base) {
            if let Some(c) = cands.iter().find(|c| self.is_installed(c)) {
                return ((*c).to_string(), weight);
            }
        } else if weight.is_some() && self.is_installed(&base) {
            return (base, weight);
        }
        (name.to_string(), None)
    }
}
