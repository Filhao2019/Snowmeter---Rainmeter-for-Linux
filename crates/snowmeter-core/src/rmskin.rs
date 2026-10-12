//! `.rmskin` / zip / folder importer (`notes/rmskin-format.md` 7).
//!
//! Accepted inputs (more lenient than Rainmeter): `.rmskin` with the 16-byte
//! footer, footerless `.rmskin`/`.zip` (with `RMSKIN.ini`, `Rainstaller.cfg`,
//! a `Skins/` folder, or that *is* one skin root), and folders of the same
//! shapes. Archives are extracted to a staging folder first with zip-slip,
//! symlink and size protection; nothing is executed.

use crate::env::Environment;
use crate::ini::{write_key_value, IniFile};
use crate::registry::{ntfs_cmp, SkinRegistry};
use crate::settings::Settings;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

/// Rainmeter version this importer pretends to be for `MinimumRainmeter`.
pub const EMULATED_RAINMETER: &str = "4.5.27.3934";

#[derive(Clone, Debug)]
pub struct ImportOptions {
    /// Back up an existing skin root to `@Backup` (default true; ignored for merge).
    pub backup: bool,
    /// Apply `LoadType`/`Load` (activate the skin or apply the layout).
    pub load: bool,
    /// Run `fc-cache -f <dir>` after installing fonts (failure ignored).
    pub run_fc_cache: bool,
    /// Install although `MinimumRainmeter` is newer than [`EMULATED_RAINMETER`].
    pub ignore_min_version: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions { backup: true, load: true, run_fc_cache: true, ignore_min_version: false }
    }
}

/// Size limits for archives and folders.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_package_bytes: u64,
    pub max_entries: usize,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_package_bytes: 1 << 30, max_entries: 100_000, max_total_bytes: 2 << 30 }
    }
}

#[derive(Debug)]
pub enum ImportError {
    Io(io::Error),
    /// Not a package/skin we understand.
    NotAPackage(String),
    /// Footer present but inconsistent.
    Corrupt(String),
    /// Zip-slip, absolute path, symlink, encrypted entry.
    Unsafe(String),
    NoName,
    NoComponents,
    /// `MinimumRainmeter` is newer than the emulated version.
    TooNew { required: String, message: String },
    TooLarge(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io(e) => write!(f, "I/O error: {e}"),
            ImportError::NotAPackage(m) => write!(f, "Not a valid skin package: {m}"),
            ImportError::Corrupt(m) => write!(f, "Corrupt package: {m}"),
            ImportError::Unsafe(m) => write!(f, "Unsafe package rejected: {m}"),
            ImportError::NoName => write!(f, "Invalid package: RMSKIN.ini has no Name"),
            ImportError::NoComponents => write!(f, "Invalid package: it contains nothing to install"),
            ImportError::TooNew { message, .. } => write!(f, "{message}"),
            ImportError::TooLarge(m) => write!(f, "Package too large: {m}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<io::Error> for ImportError {
    fn from(e: io::Error) -> Self {
        ImportError::Io(e)
    }
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub name: String,
    pub author: String,
    pub version: String,
    /// Installed skin roots (on-disk spelling).
    pub installed_roots: Vec<String>,
    /// Configs (with at least one `.ini`) under the installed roots.
    pub configs: Vec<String>,
    pub layouts_installed: Vec<String>,
    /// Roots that were moved to `@Backup`.
    pub backups: Vec<String>,
    pub fonts_installed: Vec<PathBuf>,
    /// Windows plugin DLLs without a native replacement (never installed).
    pub plugins_skipped: Vec<String>,
    /// Plugin DLLs in the package that Snowmeter implements natively.
    pub plugins_native: Vec<String>,
    pub warnings: Vec<String>,
    /// `(config, file)` activated by the package's `Load=`.
    pub loaded_skin: Option<(String, String)>,
    pub loaded_layout: Option<String>,
}

pub fn import_path(env: &Environment, path: &Path) -> Result<ImportReport, ImportError> {
    import_path_with(env, path, &ImportOptions::default())
}

pub fn import_path_with(env: &Environment, path: &Path, opts: &ImportOptions) -> Result<ImportReport, ImportError> {
    import_with_limits(env, path, opts, &Limits::default())
}

pub fn import_with_limits(
    env: &Environment,
    path: &Path,
    opts: &ImportOptions,
    limits: &Limits,
) -> Result<ImportReport, ImportError> {
    let meta = fs::metadata(path)?;
    let mut warnings = Vec::new();
    if meta.is_dir() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Skin".into());
        let pkg = resolve_package(path, false, false, &name, 0, &mut warnings)?;
        return install(env, opts, pkg, 0, warnings);
    }
    if meta.len() > limits.max_package_bytes {
        return Err(ImportError::TooLarge(format!("{} bytes", meta.len())));
    }
    let bytes = fs::read(path)?;
    let (end, flags, strict) = detect_footer(&bytes)?;
    if !strict {
        warnings.push("The file has no valid RMSKIN footer; importing it as a plain archive.".into());
    }
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Skin".into());
    let staging = Staging::new(env)?;
    extract_zip(&bytes[..end], staging.path(), limits)?;
    drop(bytes);
    let pkg = resolve_package(staging.path(), strict, true, &stem, flags, &mut warnings)?;
    install(env, opts, pkg, flags, warnings)
}

// ---------------------------------------------------------------- footer

/// `(zip end offset, flags, footer valid)`.
fn detect_footer(b: &[u8]) -> Result<(usize, u8, bool), ImportError> {
    if b.len() >= 16 && &b[b.len() - 7..] == b"RMSKIN\0" {
        let size = i64::from_le_bytes(b[b.len() - 16..b.len() - 8].try_into().unwrap());
        if size != (b.len() - 16) as i64 {
            return Err(ImportError::Corrupt(format!(
                "footer says the archive is {size} bytes but the file has {} before the footer",
                b.len() - 16
            )));
        }
        return Ok((b.len() - 16, b[b.len() - 8], true));
    }
    Ok((b.len(), 0, false))
}

// --------------------------------------------------------------- staging

struct Staging(PathBuf);

impl Staging {
    fn new(env: &Environment) -> io::Result<Staging> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let p = env.settings_dir.join(format!(".import-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&p)?;
        Ok(Staging(p))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ----------------------------------------------------- case-insensitive fs

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// Joins components below `base`, reusing existing spellings (first wins).
#[derive(Default)]
struct Canon {
    dirs: HashMap<PathBuf, HashMap<String, String>>,
}

impl Canon {
    fn join(&mut self, base: &Path, comps: &[String]) -> PathBuf {
        let mut cur = base.to_path_buf();
        for c in comps {
            let entries = self.dirs.entry(cur.clone()).or_insert_with(|| {
                let mut m = HashMap::new();
                if let Ok(rd) = fs::read_dir(&cur) {
                    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                    names.sort();
                    for n in names {
                        m.entry(lower(&n)).or_insert(n);
                    }
                }
                m
            });
            let name = entries.entry(lower(c)).or_insert_with(|| c.clone()).clone();
            cur.push(name);
        }
        cur
    }
}

/// Child of `dir` whose name equals `name` ignoring case.
fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let l = lower(name);
    let mut hits: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| lower(&e.file_name().to_string_lossy()) == l)
        .map(|e| e.path())
        .collect();
    hits.sort();
    hits.into_iter().next()
}

struct Child {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// Sorted children, skipping symlinks and `__MACOSX`.
fn children(dir: &Path) -> Vec<Child> {
    let mut v: Vec<Child> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter_map(|e| {
                let ft = e.file_type().ok()?;
                if ft.is_symlink() {
                    return None;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if name == "__MACOSX" {
                    return None;
                }
                Some(Child { name, path: e.path(), is_dir: ft.is_dir() })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort_by(|a, b| ntfs_cmp(&a.name, &b.name));
    v
}

/// All regular files below `dir` as `(relative components, path)`.
fn list_files(dir: &Path) -> Vec<(Vec<String>, PathBuf)> {
    fn rec(dir: &Path, prefix: &mut Vec<String>, out: &mut Vec<(Vec<String>, PathBuf)>, depth: usize) {
        if depth > 40 {
            return;
        }
        for c in children(dir) {
            prefix.push(c.name.clone());
            if c.is_dir {
                rec(&c.path, prefix, out, depth + 1);
            } else {
                out.push((prefix.clone(), c.path));
            }
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    rec(dir, &mut Vec::new(), &mut out, 0);
    out
}

fn has_ext(name: &str, exts: &[&str]) -> bool {
    match name.rsplit_once('.') {
        Some((_, e)) => exts.iter().any(|x| e.eq_ignore_ascii_case(x)),
        None => false,
    }
}

// ------------------------------------------------------------------- zip

fn cp1252(b: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{2C6}', '\u{2030}',
        '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}',
        '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}',
        '\u{17E}', '\u{178}',
    ];
    b.iter()
        .map(|&c| match c {
            0x80..=0x9F => HIGH[(c - 0x80) as usize],
            _ => c as char,
        })
        .collect()
}

/// Entry name -> safe components, or an error for traversal/absolute paths.
fn safe_components(raw: &[u8]) -> Result<Option<Vec<String>>, ImportError> {
    let name = match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => cp1252(raw),
    };
    let n = name.replace('\\', "/");
    if n.starts_with('/') {
        return Err(ImportError::Unsafe(format!("absolute path in archive: {name}")));
    }
    let mut comps = Vec::new();
    for c in n.split('/') {
        match c {
            "" | "." => {}
            ".." => return Err(ImportError::Unsafe(format!("path traversal in archive: {name}"))),
            c if c.contains('\0') => return Err(ImportError::Unsafe("NUL in entry name".into())),
            c => comps.push(c.to_string()),
        }
    }
    if let Some(first) = comps.first() {
        let b = first.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return Err(ImportError::Unsafe(format!("drive path in archive: {name}")));
        }
        if first == "__MACOSX" {
            return Ok(None);
        }
    }
    Ok(Some(comps))
}

fn extract_zip(data: &[u8], dest: &Path, limits: &Limits) -> Result<(), ImportError> {
    let mut ar = zip::ZipArchive::new(Cursor::new(data)).map_err(|e| ImportError::NotAPackage(format!("not a zip archive ({e})")))?;
    if ar.len() > limits.max_entries {
        return Err(ImportError::TooLarge(format!("{} entries", ar.len())));
    }
    // First pass: validate every entry before writing anything.
    let mut declared: u64 = 0;
    for i in 0..ar.len() {
        let f = ar.by_index_raw(i).map_err(|e| ImportError::Corrupt(e.to_string()))?;
        safe_components(f.name_raw())?;
        if f.encrypted() {
            return Err(ImportError::Unsafe(format!("encrypted entry: {}", String::from_utf8_lossy(f.name_raw()))));
        }
        if let Some(mode) = f.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(ImportError::Unsafe(format!("symbolic link in archive: {}", String::from_utf8_lossy(f.name_raw()))));
            }
        }
        declared = declared.saturating_add(f.size());
    }
    if declared > limits.max_total_bytes {
        return Err(ImportError::TooLarge(format!("{declared} bytes uncompressed")));
    }
    let mut canon = Canon::default();
    let mut budget = limits.max_total_bytes;
    for i in 0..ar.len() {
        let mut f = ar.by_index(i).map_err(|e| ImportError::Corrupt(e.to_string()))?;
        let Some(comps) = safe_components(f.name_raw())? else { continue };
        if comps.is_empty() {
            continue;
        }
        let target = canon.join(dest, &comps);
        if f.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(p) = target.parent() {
            fs::create_dir_all(p)?;
        }
        let mut out = fs::File::create(&target)?;
        let n = io::copy(&mut (&mut f).take(budget.saturating_add(1)), &mut out)?;
        if n > budget {
            return Err(ImportError::TooLarge("archive expands beyond the size limit".into()));
        }
        budget -= n;
    }
    Ok(())
}

// --------------------------------------------------------------- package

#[derive(Default, Debug)]
struct PkgOptions {
    name: String,
    author: String,
    version: String,
    merge: bool,
    variable_files: Vec<String>,
    min_rainmeter: String,
    load_type: String,
    load: String,
    old_format: bool,
}

#[derive(Default, Debug)]
struct Package {
    opts: PkgOptions,
    /// Root name -> source folders (several when spellings differ only by case).
    skins: Vec<(String, Vec<PathBuf>)>,
    layouts: Vec<(String, PathBuf)>,
    /// Plugin file names, de-duplicated case-insensitively.
    plugins: Vec<String>,
    fonts_dir: Option<PathBuf>,
}

fn is_ignored_skin_name(n: &str) -> bool {
    crate::registry::is_ignored_root(n)
}

fn parse_bool_int(s: &str) -> bool {
    s.trim().parse::<i64>().map(|n| n != 0).unwrap_or(false)
}

fn split_list(s: &str) -> Vec<String> {
    s.split('|').map(|p| p.trim_matches([' ', '\t']).replace('\\', "/")).filter(|p| !p.is_empty()).collect()
}

fn read_ini(path: &Path) -> Result<IniFile, ImportError> {
    IniFile::load(path).map_err(ImportError::Io)
}

/// Does `dir` (recursively, limited depth) contain an `.ini`, or a `@Resources` folder?
fn looks_like_skin_root(dir: &Path) -> bool {
    fn has_ini(dir: &Path, depth: usize) -> bool {
        if depth > 6 {
            return false;
        }
        children(dir).iter().any(|c| {
            if c.is_dir {
                has_ini(&c.path, depth + 1)
            } else {
                has_ext(&c.name, &["ini"]) && !c.name.eq_ignore_ascii_case("RMSKIN.ini")
            }
        })
    }
    find_ci(dir, "@Resources").map(|p| p.is_dir()).unwrap_or(false) || has_ini(dir, 0)
}

fn find_file_depth(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    for c in children(dir) {
        if !c.is_dir && c.name.eq_ignore_ascii_case(name) {
            return Some(c.path);
        }
    }
    if depth == 0 {
        return None;
    }
    children(dir).into_iter().filter(|c| c.is_dir).find_map(|c| find_file_depth(&c.path, name, depth - 1))
}

const COMPONENT_DIRS: [&str; 6] = ["Skins", "Layouts", "Plugins", "Fonts", "Themes", "Addons"];

/// A directory whose only entry is one folder (archive wrappers like `Pkg/…`).
fn single_wrapper(dir: &Path, always: bool) -> Option<Child> {
    let mut ch = children(dir);
    if ch.len() != 1 || !ch[0].is_dir {
        return None;
    }
    let c = ch.remove(0);
    if COMPONENT_DIRS.iter().any(|d| c.name.eq_ignore_ascii_case(d)) || c.name.eq_ignore_ascii_case("@Resources") {
        return None;
    }
    let marker = find_ci(&c.path, "RMSKIN.ini").is_some()
        || find_ci(&c.path, "Rainstaller.cfg").is_some()
        || find_ci(&c.path, "Skins").is_some();
    if always || marker {
        Some(c)
    } else {
        None
    }
}

fn resolve_package(
    tree: &Path,
    strict: bool,
    archive: bool,
    fallback: &str,
    _flags: u8,
    warnings: &mut Vec<String>,
) -> Result<Package, ImportError> {
    let mut t = tree.to_path_buf();
    let mut name = fallback.to_string();
    let mut unwrapped = false;
    for _ in 0..4 {
        if let Some(ini_path) = find_ci(&t, "RMSKIN.ini").filter(|p| p.is_file()) {
            if unwrapped {
                warnings.push("RMSKIN.ini was inside a wrapper folder; the folder was unwrapped.".into());
            }
            return package_from_rmskin_ini(&t, &ini_path, fallback, warnings);
        }
        if strict {
            return Err(ImportError::NotAPackage("RMSKIN.ini was not found at the root of the archive".into()));
        }
        if let Some(cfg) = find_file_depth(&t, "Rainstaller.cfg", 3) {
            let root = cfg.parent().unwrap_or(&t).to_path_buf();
            return package_from_rainstaller(&root, &cfg, fallback, warnings);
        }
        match single_wrapper(&t, archive) {
            Some(c) => {
                name = c.name.clone();
                t = c.path;
                unwrapped = true;
            }
            None => break,
        }
    }
    // No options file: judge by content.
    let mut pkg = Package::default();
    pkg.opts.name = fallback.to_string();
    if let Some(skins) = find_ci(&t, "Skins").filter(|p| p.is_dir()) {
        warnings.push("No RMSKIN.ini; installing the Skins folder as is.".into());
        collect_components(&t, &skins, &mut pkg, warnings);
        if pkg.skins.is_empty() && pkg.layouts.is_empty() {
            return Err(ImportError::NoComponents);
        }
        return Ok(pkg);
    }
    if looks_like_skin_root(&t) {
        if archive || unwrapped {
            warnings.push(format!("No RMSKIN.ini; treating the archive as a single skin folder named \"{name}\"."));
        }
        pkg.opts.name = name.clone();
        pkg.skins.push((name, vec![t]));
        return Ok(pkg);
    }
    Err(ImportError::NotAPackage("it is not an .rmskin package, a skin folder, or a folder with a Skins directory".into()))
}

fn package_from_rmskin_ini(
    root: &Path,
    ini_path: &Path,
    _fallback: &str,
    warnings: &mut Vec<String>,
) -> Result<Package, ImportError> {
    let ini = read_ini(ini_path)?;
    let g = |k: &str| ini.get("rmskin", k).unwrap_or("").trim().to_string();
    let mut pkg = Package::default();
    pkg.opts = PkgOptions {
        name: g("Name"),
        author: g("Author"),
        version: g("Version"),
        merge: parse_bool_int(&g("MergeSkins")),
        variable_files: split_list(&g("VariableFiles")),
        min_rainmeter: g("MinimumRainmeter"),
        load_type: g("LoadType"),
        load: g("Load"),
        old_format: false,
    };
    if pkg.opts.name.is_empty() {
        return Err(ImportError::NoName);
    }
    let mw = g("MinimumWindows");
    if !mw.is_empty() {
        log::info!("rmskin: MinimumWindows={mw} treated as satisfied");
    }
    let skins = find_ci(root, "Skins").filter(|p| p.is_dir());
    collect_components(root, skins.as_deref().unwrap_or(root), &mut pkg, warnings);
    if skins.is_none() {
        pkg.skins.clear();
    }
    Ok(pkg)
}

fn package_from_rainstaller(
    root: &Path,
    cfg: &Path,
    fallback: &str,
    warnings: &mut Vec<String>,
) -> Result<Package, ImportError> {
    let ini = read_ini(cfg)?;
    let g = |k: &str| ini.get("Rainstaller", k).unwrap_or("").trim().to_string();
    let mut pkg = Package::default();
    let launch = split_list(&g("LaunchCommand")).into_iter().next().unwrap_or_default().replace('/', "\\");
    let lt = g("LaunchType");
    pkg.opts = PkgOptions {
        name: if g("Name").is_empty() { fallback.to_string() } else { g("Name") },
        author: g("Author"),
        version: g("Version"),
        merge: parse_bool_int(&g("Merge")),
        variable_files: split_list(&g("KeepVar")),
        min_rainmeter: g("MinRainmeterVer"),
        load_type: if lt.is_empty() { String::new() } else if lt.eq_ignore_ascii_case("load") { "Skin".into() } else { "Layout".into() },
        load: launch,
        old_format: true,
    };
    warnings.push("Old Rainstaller.cfg package format.".into());
    let skins = find_ci(root, "Skins").filter(|p| p.is_dir());
    collect_components(root, skins.as_deref().unwrap_or(root), &mut pkg, warnings);
    if skins.is_none() {
        pkg.skins.clear();
    }
    // Themes\<n>\*.thm become layouts (copied as .ini by the layout installer).
    if let Some(themes) = find_ci(root, "Themes").filter(|p| p.is_dir()) {
        for c in children(&themes).into_iter().filter(|c| c.is_dir && !c.name.eq_ignore_ascii_case("Backup")) {
            pkg.layouts.push((c.name, c.path));
        }
    }
    Ok(pkg)
}

/// Fill skins/layouts/plugins/fonts from the package root `t`.
fn collect_components(t: &Path, skins_dir: &Path, pkg: &mut Package, _warnings: &mut Vec<String>) {
    if skins_dir != t {
        for c in children(skins_dir).into_iter().filter(|c| c.is_dir && !is_ignored_skin_name(&c.name)) {
            match pkg.skins.iter_mut().find(|(n, _)| lower(n) == lower(&c.name)) {
                Some((_, v)) => v.push(c.path),
                None => pkg.skins.push((c.name, vec![c.path])),
            }
        }
    }
    if let Some(l) = find_ci(t, "Layouts").filter(|p| p.is_dir()) {
        for c in children(&l).into_iter().filter(|c| c.is_dir && !c.name.eq_ignore_ascii_case("Backup") && !c.name.eq_ignore_ascii_case("@Backup")) {
            if list_files(&c.path).iter().any(|(r, _)| has_ext(r.last().unwrap(), &["ini"])) {
                pkg.layouts.push((c.name, c.path));
            }
        }
    }
    if let Some(p) = find_ci(t, "Plugins").filter(|p| p.is_dir()) {
        for arch in children(&p).into_iter().filter(|c| c.is_dir) {
            if !(arch.name.eq_ignore_ascii_case("32bit") || arch.name.eq_ignore_ascii_case("64bit")) {
                continue;
            }
            for f in children(&arch.path).into_iter().filter(|c| !c.is_dir && has_ext(&c.name, &["dll"])) {
                if !pkg.plugins.iter().any(|x| x.eq_ignore_ascii_case(&f.name)) {
                    pkg.plugins.push(f.name);
                }
            }
        }
    }
    pkg.fonts_dir = find_ci(t, "Fonts").filter(|p| p.is_dir());
}

// ------------------------------------------------------------------ install

/// Built-in plugins the Rainmeter installer never copies.
const INSTALLER_BUILTIN: [&str; 25] = [
    "actiontimer", "advancedcpu", "audiolevel", "coretemp", "fileview", "folderinfo", "inputtext", "itunesplugin",
    "mediakey", "nowplaying", "perfmon", "pingplugin", "powerplugin", "process", "quoteplugin", "recyclemanager",
    "resmon", "runcommand", "speedfanplugin", "sysinfo", "usagemonitor", "webparser", "wifistatus", "win7audioplugin",
    "windowmessageplugin",
];

const FONT_EXTS: [&str; 11] = ["ttf", "otf", "ttc", "otc", "fon", "fnt", "pfb", "pfm", "woff", "woff2", "pcf"];

fn version_parts(v: &str) -> Vec<i64> {
    v.split('.')
        .map(|p| {
            let d: String = p.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
            d.parse().unwrap_or(0)
        })
        .collect()
}

fn version_gt(a: &str, b: &str) -> bool {
    let (x, y) = (version_parts(a), version_parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p > q;
        }
    }
    false
}

fn copy_file(src: &Path, dest: &Path) -> io::Result<()> {
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p)?;
    }
    fs::copy(src, dest).map(|_| ())
}

/// Copy the tree `src` into `dest` (merging, overwriting), case-insensitively.
fn copy_tree(src: &Path, dest: &Path, canon: &mut Canon, filter: &dyn Fn(&str) -> bool) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for c in children(src) {
        if c.is_dir {
            // Keep empty directories too.
            let d = canon.join(dest, &[c.name.clone()]);
            fs::create_dir_all(&d)?;
        }
    }
    for (rel, path) in list_files(src) {
        if !filter(rel.last().map(String::as_str).unwrap_or("")) {
            continue;
        }
        copy_file(&path, &canon.join(dest, &rel))?;
    }
    Ok(())
}

fn install(
    env: &Environment,
    opts: &ImportOptions,
    pkg: Package,
    flags: u8,
    warnings: Vec<String>,
) -> Result<ImportReport, ImportError> {
    let mut rep = ImportReport {
        name: pkg.opts.name.clone(),
        author: pkg.opts.author.clone(),
        version: pkg.opts.version.clone(),
        warnings,
        ..Default::default()
    };
    if !pkg.opts.min_rainmeter.is_empty()
        && version_gt(&pkg.opts.min_rainmeter, EMULATED_RAINMETER)
        && !opts.ignore_min_version
    {
        let required = pkg.opts.min_rainmeter.clone();
        return Err(ImportError::TooNew {
            message: format!("Rainmeter {required} or higher is required to install this package."),
            required,
        });
    }
    if pkg.skins.is_empty() && pkg.layouts.is_empty() && pkg.plugins.is_empty() {
        return Err(ImportError::NoComponents);
    }
    let backup_package = flags != 0 && flags & 1 == 0;
    let do_backup = opts.backup && !pkg.opts.merge && !backup_package;
    let skins_dir = &env.skins_dir;
    fs::create_dir_all(skins_dir)?;

    // --- skins
    for (root, sources) in &pkg.skins {
        let dest_name = find_ci(skins_dir, root)
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| root.clone());
        let dest = skins_dir.join(&dest_name);
        if dest.exists() {
            if backup_package {
                fs::remove_dir_all(&dest)?;
            } else if do_backup {
                let bk_dir = find_ci(skins_dir, "@Backup").unwrap_or_else(|| skins_dir.join("@Backup"));
                fs::create_dir_all(&bk_dir)?;
                let bk = bk_dir.join(&dest_name);
                if bk.exists() {
                    fs::remove_dir_all(&bk)?;
                }
                if fs::rename(&dest, &bk).is_err() {
                    copy_tree(&dest, &bk, &mut Canon::default(), &|_| true)?;
                    fs::remove_dir_all(&dest)?;
                }
                rep.backups.push(dest_name.clone());
            }
        }
        let mut canon = Canon::default();
        for s in sources {
            copy_tree(s, &dest, &mut canon, &|_| true)?;
        }
        rep.installed_roots.push(dest_name);
    }

    // --- old format: Fonts\*.ttf go into every root's @Resources\Fonts
    if pkg.opts.old_format {
        if let Some(fd) = &pkg.fonts_dir {
            for root in &rep.installed_roots {
                let mut canon = Canon::default();
                let target = canon.join(&skins_dir.join(root), &["@Resources".into(), "Fonts".into()]);
                copy_tree(fd, &target, &mut canon, &|n| has_ext(n, &["ttf"]))?;
            }
        }
    }

    // --- VariableFiles
    if do_backup {
        for item in &pkg.opts.variable_files {
            keep_variables(skins_dir, item, &mut rep.warnings);
        }
    }

    // --- layouts
    for (name, src) in &pkg.layouts {
        install_layout(env, name, src, pkg.opts.old_format)?;
        rep.layouts_installed.push(name.clone());
    }

    // --- plugins: recorded only, never installed
    for p in &pkg.plugins {
        let stem = p.rsplit_once('.').map(|(s, _)| s).unwrap_or(p);
        if crate::measures::plugin::native_plugin(stem).is_some() {
            rep.plugins_native.push(p.clone());
        } else if INSTALLER_BUILTIN.iter().any(|b| b.eq_ignore_ascii_case(stem)) {
            // Rainmeter's own built-ins: the installer ignores these too.
            continue;
        } else {
            rep.plugins_skipped.push(p.clone());
            rep.warnings.push(format!(
                "Plugin {p} is a Windows DLL and cannot run on Linux; skins that use it will show a placeholder."
            ));
        }
    }

    // --- fonts
    install_fonts(env, opts, &pkg, &rep.installed_roots.clone(), &mut rep)?;

    // --- configs of installed roots
    let reg = SkinRegistry::scan(env);
    for c in reg.loadable() {
        if rep.installed_roots.iter().any(|r| r.eq_ignore_ascii_case(&c.root)) {
            rep.configs.push(c.name.clone());
        }
    }

    // --- load action
    if opts.load && !pkg.opts.load.trim().is_empty() && !pkg.opts.load_type.trim().is_empty() {
        apply_load(env, &pkg, &mut rep);
    }
    Ok(rep)
}

fn keep_variables(skins_dir: &Path, item: &str, warnings: &mut Vec<String>) {
    let comps: Vec<&str> = item.split('/').filter(|c| !c.is_empty() && *c != "." && *c != "..").collect();
    if comps.is_empty() {
        return;
    }
    let rel: PathBuf = comps.iter().collect();
    let bk_dir = match find_ci(skins_dir, "@Backup") {
        Some(p) => p,
        None => return,
    };
    let find = |base: &Path| crate::paths::find_case_insensitive(&base.join(&rel)).filter(|p| p.is_file());
    let (Some(old_p), Some(new_p)) = (find(&bk_dir), find(skins_dir)) else { return };
    let (Ok(old), Ok(new)) = (IniFile::load(&old_p), IniFile::load(&new_p)) else {
        warnings.push(format!("Could not read the variable file {item}; defaults kept."));
        return;
    };
    let (Some(os), Some(ns)) = (old.section("Variables"), new.section("Variables")) else { return };
    for (k, v) in &os.entries {
        // Key matching is case-sensitive, like Rainmeter.
        if ns.entries.iter().any(|(nk, _)| nk == k) {
            if let Err(e) = write_key_value(&new_p, "Variables", k, v) {
                warnings.push(format!("Could not keep variable {k} in {item}: {e}"));
            }
        }
    }
}

fn install_layout(env: &Environment, name: &str, src: &Path, thm: bool) -> io::Result<()> {
    let layouts = env.settings_dir.join("Layouts");
    let mut canon = Canon::default();
    for (rel, path) in list_files(src) {
        let last = rel.last().cloned().unwrap_or_default();
        let is_thm = thm && has_ext(&last, &["thm"]);
        if !has_ext(&last, &["ini"]) && !is_thm {
            continue; // wallpaper and other files are ignored
        }
        let mut rel = rel;
        if is_thm {
            let stem = last.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(last);
            *rel.last_mut().unwrap() = format!("{stem}.ini");
        }
        let mut comps = vec![name.to_string()];
        comps.extend(rel);
        let dest = canon.join(&layouts, &comps);
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p)?;
        }
        // Packaged layouts can't change global settings: empty [Rainmeter].
        match IniFile::load(&path) {
            Ok(mut ini) => {
                ini.clear_section("Rainmeter");
                fs::write(&dest, ini.to_ini_string())?;
            }
            Err(_) => {
                fs::copy(&path, &dest)?;
            }
        }
    }
    Ok(())
}

fn collect_fonts(dir: &Path) -> Vec<(Vec<String>, PathBuf)> {
    list_files(dir).into_iter().filter(|(r, _)| has_ext(r.last().map(String::as_str).unwrap_or(""), &FONT_EXTS)).collect()
}

fn install_fonts(
    env: &Environment,
    opts: &ImportOptions,
    pkg: &Package,
    roots: &[String],
    rep: &mut ImportReport,
) -> io::Result<()> {
    let mut targets: Vec<(String, Vec<(Vec<String>, PathBuf)>)> = Vec::new();
    for root in roots {
        let rdir = env.skins_dir.join(root);
        let mut files = Vec::new();
        if let Some(res) = find_ci(&rdir, "@Resources") {
            if let Some(fd) = find_ci(&res, "Fonts").filter(|p| p.is_dir()) {
                files = collect_fonts(&fd);
            }
        }
        if !files.is_empty() {
            targets.push((root.clone(), files));
        }
    }
    if !pkg.opts.old_format {
        if let Some(fd) = &pkg.fonts_dir {
            let files = collect_fonts(fd);
            if !files.is_empty() {
                let label = roots.first().cloned().unwrap_or_else(|| sanitize(&pkg.opts.name));
                match targets.iter_mut().find(|(n, _)| *n == label) {
                    Some((_, v)) => v.extend(files),
                    None => targets.push((label, files)),
                }
            }
        }
    }
    for (label, files) in targets {
        let dir = env.fonts_install_dir.join(&label);
        let mut canon = Canon::default();
        for (rel, src) in files {
            let dest = canon.join(&dir, &rel);
            copy_file(&src, &dest)?;
            rep.fonts_installed.push(dest);
        }
        if opts.run_fc_cache {
            let ok = std::process::Command::new("fc-cache")
                .arg("-f")
                .arg(&dir)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                log::info!("fc-cache unavailable or failed for {}", dir.display());
            }
        }
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    if t.trim().is_empty() {
        "package".into()
    } else {
        t
    }
}

/// Settings keys copied from a skin's `[Rainmeter] Default*` options when a
/// config is activated for the first time.
const DEFAULT_KEYS: [&str; 13] = [
    "WindowX", "WindowY", "AnchorX", "AnchorY", "AlwaysOnTop", "OnHover", "Draggable", "SnapEdges", "ClickThrough",
    "KeepOnScreen", "SavePosition", "StartHidden", "AutoSelectScreen",
];

/// Mark `config`/`file` active in `settings` (index from the registry) and,
/// when the config has no section yet, apply the skin's `Default*` options.
/// Returns the canonical `(config, file)`. Does not save.
pub fn activate_skin(env: &Environment, settings: &mut Settings, config: &str, file: &str) -> Result<(String, String), String> {
    let reg = SkinRegistry::scan(env);
    let cfg = reg.config(config).ok_or_else(|| format!("Config \"{config}\" not found"))?;
    let idx = reg.file_index(config, file).ok_or_else(|| format!("\"{file}\" not found in {}", cfg.name))?;
    let fname = reg.file_at(config, idx).unwrap_or(file).to_string();
    let name = cfg.name.clone();
    if !settings.has_config(&name) {
        if let Some(path) = reg.ini_path(&name, &fname) {
            if let Ok(ini) = IniFile::load(&path) {
                let get = |k: &str| ini.get("Rainmeter", &format!("Default{k}")).map(str::trim).filter(|v| !v.is_empty());
                for k in DEFAULT_KEYS {
                    if let Some(v) = get(k) {
                        settings.set_config_key(&name, k, v);
                    }
                }
                if let Some(v) = get("AlphaValue") {
                    settings.set_config_key(&name, "AlphaValue", v);
                }
                if let Some(v) = get("FadeDuration") {
                    settings.set_config_key(&name, "FadeDuration", v);
                }
                if let Some(v) = ini.get("Rainmeter", "DefaultHideOnMouseOver").map(str::trim).filter(|v| !v.is_empty()) {
                    if get("OnHover").is_none() {
                        settings.set_config_key(&name, "HideOnMouseOver", v);
                    }
                }
            }
        }
    }
    settings.set_active(&name, idx);
    Ok((name, fname))
}

/// Replace the settings' configs with the layout `Layouts/<name>/Rainmeter.ini`
/// after saving a copy to `Layouts/@Backup/`.
pub fn apply_layout_by_name(env: &Environment, settings: &mut Settings, name: &str) -> Result<(), String> {
    let layouts = env.settings_dir.join("Layouts");
    let dir = find_ci(&layouts, name).filter(|p| p.is_dir()).ok_or_else(|| format!("Layout \"{name}\" not found"))?;
    let file = find_ci(&dir, "Rainmeter.ini")
        .or_else(|| children(&dir).into_iter().find(|c| !c.is_dir && has_ext(&c.name, &["ini"])).map(|c| c.path))
        .ok_or_else(|| format!("Layout \"{name}\" has no .ini file"))?;
    let layout = IniFile::load(&file).map_err(|e| e.to_string())?;
    if !name.eq_ignore_ascii_case("@Backup") && env.settings_file.is_file() {
        let bk = layouts.join("@Backup");
        fs::create_dir_all(&bk).map_err(|e| e.to_string())?;
        let fname = env.settings_file.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "snowmeter.ini".into());
        fs::copy(&env.settings_file, bk.join(fname)).map_err(|e| e.to_string())?;
    }
    settings.apply_layout(&layout);
    Ok(())
}

fn apply_load(env: &Environment, pkg: &Package, rep: &mut ImportReport) {
    let mut settings = Settings::load(env);
    let load = pkg.opts.load.trim();
    if pkg.opts.load_type.trim().eq_ignore_ascii_case("skin") {
        let norm = load.replace('/', "\\");
        let Some((config, file)) = norm.rsplit_once('\\') else {
            rep.warnings.push(format!("Load=\"{load}\" has no config folder; nothing was loaded."));
            return;
        };
        match activate_skin(env, &mut settings, config, file) {
            Ok(done) => {
                if let Err(e) = settings.save() {
                    rep.warnings.push(format!("Could not save settings: {e}"));
                } else {
                    rep.loaded_skin = Some(done);
                }
            }
            Err(e) => rep.warnings.push(format!("Could not load the skin: {e}")),
        }
    } else {
        match apply_layout_by_name(env, &mut settings, load) {
            Ok(()) => match settings.save() {
                Ok(()) => rep.loaded_layout = Some(load.to_string()),
                Err(e) => rep.warnings.push(format!("Could not save settings: {e}")),
            },
            Err(e) => rep.warnings.push(format!("Could not apply the layout: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tiny_zip(files: &[(&str, usize)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (n, len) in files {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&vec![b'x'; *len]).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn limits_are_enforced() {
        let d = tempfile::tempdir().unwrap();
        let z = tiny_zip(&[("a.ini", 100), ("b.ini", 100), ("c.ini", 100)]);
        let few = Limits { max_entries: 2, ..Limits::default() };
        assert!(matches!(extract_zip(&z, d.path(), &few), Err(ImportError::TooLarge(_))));
        let small = Limits { max_total_bytes: 150, ..Limits::default() };
        assert!(matches!(extract_zip(&z, d.path(), &small), Err(ImportError::TooLarge(_))));
        extract_zip(&z, d.path(), &Limits::default()).unwrap();
        assert!(d.path().join("a.ini").is_file());
    }

    #[test]
    fn names_are_sanitised() {
        assert!(safe_components(b"Skins/R/../../x").is_err());
        assert!(safe_components(b"/abs").is_err());
        assert!(safe_components(b"C:\\x").is_err());
        assert_eq!(safe_components(b"a\\b/./c").unwrap().unwrap(), vec!["a", "b", "c"]);
        assert_eq!(safe_components(b"__MACOSX/x").unwrap(), None);
        // CP1252 fallback when the bytes are not UTF-8.
        assert_eq!(safe_components(b"caf\xe9.ini").unwrap().unwrap(), vec!["caf\u{e9}.ini"]);
    }

    #[test]
    fn version_compare() {
        assert!(version_gt("99.0", "4.5.27.3934"));
        assert!(version_gt("4.5.28", "4.5.27.3934"));
        assert!(!version_gt("4.5", "4.5.0.0"));
        assert!(!version_gt("4.5.27.3934", "4.5.27.3934"));
    }

    #[test]
    fn footer_detection() {
        let z = tiny_zip(&[("a", 1)]);
        assert_eq!(detect_footer(&z).unwrap(), (z.len(), 0, false));
        let mut f = z.clone();
        f.extend_from_slice(&(z.len() as i64).to_le_bytes());
        f.push(2);
        f.extend_from_slice(b"RMSKIN\0");
        assert_eq!(detect_footer(&f).unwrap(), (z.len(), 2, true));
        let n = f.len();
        f[n - 16] ^= 1;
        assert!(matches!(detect_footer(&f), Err(ImportError::Corrupt(_))));
    }
}
