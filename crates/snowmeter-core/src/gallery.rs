//! Skin gallery: `skins/catalog.ini` entries (`folder`, `package`, `remote`),
//! installed state, installation and the bundled-skin sync (`notes/gallery.md`).

use crate::env::Environment;
use crate::fixups;
use crate::ini::IniFile;
use crate::registry::SkinRegistry;
use crate::rmskin::{self, ImportError, ImportOptions, ImportReport, Limits};
use crate::settings::Settings;
use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Skin root bundled as a folder next to the catalog.
    Folder,
    /// `.rmskin` bundled next to the catalog.
    Package,
    /// Downloaded from the author at install time.
    Remote,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Folder => "folder",
            Kind::Package => "package",
            Kind::Remote => "remote",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Section name.
    pub name: String,
    pub kind: Kind,
    /// `folder`: skin root folder; `package`: `.rmskin`, relative to the bundled `skins/`.
    pub path: String,
    pub url: String,
    /// `remote`: folder inside the downloaded archive that is the skin root.
    pub subdir: String,
    /// Optional: skin root folder name used to detect "installed".
    pub root: String,
    pub author: String,
    pub license: String,
    pub source: String,
    pub description: String,
    pub status: String,
    pub notes: String,
    /// `FontUrl1..N` in numeric order.
    pub font_urls: Vec<String>,
    /// `PostInstall`: fix-up names separated by space, comma or `|`.
    pub post_install: Vec<String>,
}

impl Entry {
    /// Skin root folder this entry installs (used for the installed check).
    pub fn root_name(&self) -> String {
        if !self.root.is_empty() {
            return self.root.clone();
        }
        let last = |s: &str| s.replace('\\', "/").rsplit('/').find(|p| !p.is_empty()).map(str::to_string);
        match self.kind {
            Kind::Folder => last(&self.path).unwrap_or_else(|| self.name.clone()),
            Kind::Package => last(&self.path)
                .map(|f| f.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(f))
                .unwrap_or_else(|| self.name.clone()),
            Kind::Remote => last(&self.subdir).unwrap_or_else(|| self.name.clone()),
        }
    }
}

/// Parse `catalog.ini` text. Sections with a missing/unknown `Kind` are skipped.
pub fn parse_catalog(text: &str) -> Vec<Entry> {
    let ini = IniFile::parse(text);
    let mut out = Vec::new();
    for sec in ini.sections() {
        let get = |k: &str| {
            sec.entries.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_string()).unwrap_or_default()
        };
        let kind = match get("Kind").to_ascii_lowercase().as_str() {
            "folder" => Kind::Folder,
            "package" => Kind::Package,
            "remote" => Kind::Remote,
            _ => continue,
        };
        let mut fonts: Vec<(u32, String)> = sec
            .entries
            .iter()
            .filter_map(|(k, v)| {
                let n = k.get(..7).filter(|p| p.eq_ignore_ascii_case("FontUrl"))?;
                let _ = n;
                Some((k[7..].parse::<u32>().ok()?, v.trim().to_string()))
            })
            .filter(|(_, v)| !v.is_empty())
            .collect();
        fonts.sort();
        out.push(Entry {
            name: sec.name.clone(),
            kind,
            path: get("Path"),
            url: get("Url"),
            subdir: get("Subdir"),
            root: get("Root"),
            author: get("Author"),
            license: get("License"),
            source: get("Source"),
            description: get("Description"),
            status: get("Status"),
            notes: get("Notes"),
            font_urls: fonts.into_iter().map(|(_, v)| v).collect(),
            post_install: get("PostInstall")
                .split(|c: char| c.is_whitespace() || c == ',' || c == '|')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        });
    }
    out
}

/// `<bundled>/catalog.ini`; empty when it is missing.
pub fn load_catalog(bundled: &Path) -> Vec<Entry> {
    match IniFile::load(&bundled.join("catalog.ini")) {
        Ok(ini) => parse_catalog(&ini.to_ini_string()),
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                log::warn!("gallery: cannot read catalog.ini in {}: {e}", bundled.display());
            }
            Vec::new()
        }
    }
}

fn find_dir_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.is_dir() {
        return Some(direct);
    }
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name) && e.path().is_dir())
        .map(|e| e.path())
}

/// Is the entry's skin root present in the skins folder?
pub fn is_installed(env: &Environment, entry: &Entry) -> bool {
    find_dir_ci(&env.skins_dir, &entry.root_name()).is_some()
}

#[derive(Clone, Debug)]
pub struct InstallOptions {
    /// Replace an already installed skin (folder: overwrite files; package: importer backup).
    pub force: bool,
    /// The bundled `skins/` directory (needed for `folder` and `package`).
    pub bundled_dir: Option<PathBuf>,
    pub run_fc_cache: bool,
    pub max_package_bytes: u64,
    pub max_font_bytes: u64,
}

impl Default for InstallOptions {
    fn default() -> Self {
        InstallOptions {
            force: false,
            bundled_dir: None,
            run_fc_cache: true,
            max_package_bytes: 100 << 20,
            max_font_bytes: 20 << 20,
        }
    }
}

#[derive(Debug)]
pub enum GalleryError {
    AlreadyInstalled(String),
    NoBundledDir,
    Missing(String),
    BadEntry(String),
    Download(String),
    TooLarge(String),
    Import(ImportError),
    Io(io::Error),
}

impl fmt::Display for GalleryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GalleryError::AlreadyInstalled(n) => write!(f, "\"{n}\" is already installed (use --force to reinstall)"),
            GalleryError::NoBundledDir => write!(f, "the bundled skins folder was not found (set SNOWMETER_BUNDLED_SKINS)"),
            GalleryError::Missing(m) => write!(f, "missing: {m}"),
            GalleryError::BadEntry(m) => write!(f, "bad catalog entry: {m}"),
            GalleryError::Download(m) => write!(f, "download failed: {m}"),
            GalleryError::TooLarge(m) => write!(f, "download too large: {m}"),
            GalleryError::Import(e) => write!(f, "import failed: {e}"),
            GalleryError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GalleryError {}

impl From<io::Error> for GalleryError {
    fn from(e: io::Error) -> Self {
        GalleryError::Io(e)
    }
}

impl From<ImportError> for GalleryError {
    fn from(e: ImportError) -> Self {
        GalleryError::Import(e)
    }
}

#[derive(Debug, Default)]
pub struct InstallReport {
    pub name: String,
    pub roots: Vec<String>,
    /// Loadable configs under the installed roots.
    pub configs: Vec<String>,
    pub import: Option<ImportReport>,
    pub fonts: Vec<PathBuf>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

impl fmt::Display for InstallReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(i) = &self.import {
            write!(f, "{i}")?;
        } else {
            writeln!(f, "Installed \"{}\": {}", self.name, self.roots.join(", "))?;
        }
        if !self.fonts.is_empty() {
            writeln!(f, "Gallery fonts installed: {}", self.fonts.len())?;
        }
        for n in &self.notes {
            writeln!(f, "{n}")?;
        }
        for w in &self.warnings {
            writeln!(f, "Warning: {w}")?;
        }
        Ok(())
    }
}

/// Recursively copy `src` into `dst`; existing files are kept unless `overwrite`.
/// Returns the number of files written.
pub fn copy_tree(src: &Path, dst: &Path, overwrite: bool) -> io::Result<usize> {
    fs::create_dir_all(dst)?;
    let mut n = 0;
    for e in fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        let ft = e.file_type()?;
        if ft.is_dir() {
            n += copy_tree(&e.path(), &to, overwrite)?;
        } else if ft.is_file() && (overwrite || !to.exists()) {
            fs::copy(e.path(), &to)?;
            n += 1;
        }
    }
    Ok(n)
}

fn safe_rel(p: &str) -> Result<PathBuf, GalleryError> {
    let mut out = PathBuf::new();
    for c in Path::new(&p.replace('\\', "/")).components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => return Err(GalleryError::BadEntry(format!("unsafe path \"{p}\""))),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(GalleryError::BadEntry("empty path".into()));
    }
    Ok(out)
}

fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    if t.is_empty() || t.starts_with('.') {
        format!("_{t}")
    } else {
        t
    }
}

fn http_get(url: &str, dest: &Path, max: u64) -> Result<u64, GalleryError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .user_agent("snowmeter-gallery")
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|e| GalleryError::Download(format!("{url}: {e}")))?;
    if let Some(len) = resp.body().content_length() {
        if len > max {
            return Err(GalleryError::TooLarge(format!("{url} is {len} bytes (limit {max})")));
        }
    }
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = dest.with_extension("part");
    let result = (|| {
        let mut reader = resp.body_mut().with_config().limit(max.saturating_add(1)).reader();
        let mut out = fs::File::create(&tmp)?;
        let n = io::copy(&mut (&mut reader).take(max.saturating_add(1)), &mut out)?;
        out.flush()?;
        Ok::<u64, io::Error>(n)
    })();
    match result {
        Ok(n) if n <= max => {
            fs::rename(&tmp, dest)?;
            Ok(n)
        }
        Ok(_) => {
            let _ = fs::remove_file(&tmp);
            Err(GalleryError::TooLarge(format!("{url} exceeds {max} bytes")))
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(GalleryError::Download(format!("{url}: {e}")))
        }
    }
}

fn url_file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().unwrap_or("").to_string()
}

fn has_font_ext(n: &str) -> bool {
    let l = n.to_ascii_lowercase();
    [".ttf", ".otf", ".ttc"].iter().any(|e| l.ends_with(e))
}

/// Download a font (or a zip with fonts) and return `(file name, bytes)` pairs.
fn fetch_fonts(env: &Environment, url: &str, max: u64) -> Result<Vec<(String, Vec<u8>)>, GalleryError> {
    let name = url_file_name(url);
    let cache = env.gallery_cache_dir.join("fonts").join(sanitize(&format!("{:x}-{name}", simple_hash(url))));
    http_get(url, &cache, max)?;
    let bytes = fs::read(&cache)?;
    if bytes.starts_with(b"PK") {
        let mut ar = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| GalleryError::Download(format!("{url}: not a zip ({e})")))?;
        let mut out = Vec::new();
        for i in 0..ar.len() {
            let f = ar.by_index(i).map_err(|e| GalleryError::Download(e.to_string()))?;
            let fname = String::from_utf8_lossy(f.name_raw()).into_owned();
            let base = fname.rsplit(['/', '\\']).next().unwrap_or("").to_string();
            if f.is_dir() || !has_font_ext(&base) {
                continue;
            }
            let mut data = Vec::new();
            f.take(max.saturating_add(1)).read_to_end(&mut data)?;
            if data.len() as u64 > max {
                return Err(GalleryError::TooLarge(format!("font {base} in {url}")));
            }
            out.push((sanitize(&base), data));
        }
        if out.is_empty() {
            return Err(GalleryError::Download(format!("{url}: no font files in the archive")));
        }
        return Ok(out);
    }
    if !has_font_ext(&name) {
        return Err(GalleryError::Download(format!("{url}: not a font file")));
    }
    Ok(vec![(sanitize(&name), bytes)])
}

fn simple_hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Install a gallery entry. `settings` is reloaded from disk after the importer ran.
pub fn install(env: &Environment, settings: &mut Settings, entry: &Entry, opts: &InstallOptions) -> Result<InstallReport, GalleryError> {
    if !opts.force && is_installed(env, entry) {
        return Err(GalleryError::AlreadyInstalled(entry.name.clone()));
    }
    fs::create_dir_all(&env.skins_dir)?;
    let mut rep = InstallReport { name: entry.name.clone(), ..Default::default() };
    let import_opts = ImportOptions { run_fc_cache: opts.run_fc_cache, layout_keep_others: true, ..ImportOptions::default() };
    match entry.kind {
        Kind::Folder => {
            let bundled = opts.bundled_dir.as_deref().ok_or(GalleryError::NoBundledDir)?;
            let rel = safe_rel(&entry.path)?;
            let src = bundled.join(&rel);
            if !src.is_dir() {
                return Err(GalleryError::Missing(src.display().to_string()));
            }
            copy_tree(&src, &env.skins_dir.join(&rel), opts.force)?;
            let root = rel.to_string_lossy().into_owned();
            record_bundled(settings, &root);
            if let Err(e) = settings.save() {
                rep.warnings.push(format!("could not save settings: {e}"));
            }
            rep.roots.push(root);
        }
        Kind::Package => {
            let bundled = opts.bundled_dir.as_deref().ok_or(GalleryError::NoBundledDir)?;
            let src = bundled.join(safe_rel(&entry.path)?);
            if !src.is_file() {
                return Err(GalleryError::Missing(src.display().to_string()));
            }
            let imp = rmskin::import_path_with(env, &src, &import_opts)?;
            rep.roots = imp.installed_roots.clone();
            rep.import = Some(imp);
        }
        Kind::Remote => {
            if entry.url.is_empty() {
                return Err(GalleryError::BadEntry(format!("{}: remote entry without Url", entry.name)));
            }
            let ext = match url_file_name(&entry.url).rsplit_once('.') {
                Some((_, e)) if e.eq_ignore_ascii_case("rmskin") => "rmskin",
                _ => "zip",
            };
            let file = env.gallery_cache_dir.join(format!("{}.{ext}", sanitize(&entry.name)));
            http_get(&entry.url, &file, opts.max_package_bytes)?;
            let imp = if entry.subdir.is_empty() {
                rmskin::import_path_with(env, &file, &import_opts)?
            } else {
                let sub = safe_rel(&entry.subdir)?;
                let stage = env.gallery_cache_dir.join(format!("stage-{}", sanitize(&entry.name)));
                let _ = fs::remove_dir_all(&stage);
                fs::create_dir_all(&stage)?;
                let r = import_subdir(env, &file, &stage, &sub, &import_opts);
                let _ = fs::remove_dir_all(&stage);
                r?
            };
            rep.roots = imp.installed_roots.clone();
            rep.import = Some(imp);
        }
    }
    let root_paths: Vec<PathBuf> = rep.roots.iter().map(|r| env.skins_dir.join(r)).collect();
    let reg = SkinRegistry::scan(env);
    rep.configs = reg
        .configs
        .iter()
        .filter(|c| !c.files.is_empty() && rep.roots.iter().any(|r| r.eq_ignore_ascii_case(&c.root)))
        .map(|c| c.name.clone())
        .collect();
    for (i, url) in entry.font_urls.iter().enumerate() {
        match install_fonts(env, opts, rep.roots.first(), url) {
            Ok(mut v) => rep.fonts.append(&mut v),
            Err(e) => rep.warnings.push(format!("font {}: {e}", i + 1)),
        }
    }
    for fix in &entry.post_install {
        match fixups::run(fix, &root_paths) {
            Ok(note) => rep.notes.push(note),
            Err(e) => rep.warnings.push(format!("post-install {fix}: {e}")),
        }
    }
    *settings = Settings::load(env);
    Ok(rep)
}

fn import_subdir(env: &Environment, zip_file: &Path, stage: &Path, sub: &Path, opts: &ImportOptions) -> Result<ImportReport, GalleryError> {
    let bytes = fs::read(zip_file)?;
    let limits = Limits::default();
    rmskin::extract_zip(&bytes, stage, &limits)?;
    drop(bytes);
    let resolve = |base: &Path| -> Option<PathBuf> {
        let mut cur = base.to_path_buf();
        for c in sub.components() {
            cur = find_dir_ci(&cur, &c.as_os_str().to_string_lossy())?;
        }
        Some(cur)
    };
    let mut found = resolve(stage);
    if found.is_none() {
        // GitHub archives wrap everything in `<repo>-<ref>/`.
        let kids: Vec<PathBuf> = fs::read_dir(stage)?.flatten().map(|e| e.path()).collect();
        if let [only] = kids.as_slice() {
            if only.is_dir() {
                found = resolve(only);
            }
        }
    }
    let dir = found.ok_or_else(|| GalleryError::Missing(format!("\"{}\" inside the downloaded archive", sub.display())))?;
    Ok(rmskin::import_path_with(env, &dir, opts)?)
}

fn install_fonts(env: &Environment, opts: &InstallOptions, root: Option<&String>, url: &str) -> Result<Vec<PathBuf>, GalleryError> {
    let root = root.ok_or_else(|| GalleryError::Missing("no installed skin root for the font".into()))?;
    let fonts = fetch_fonts(env, url, opts.max_font_bytes)?;
    let skin_dir = env.skins_dir.join(root).join("@Resources").join("Fonts");
    let sys_dir = env.fonts_install_dir.join(root);
    let mut out = Vec::new();
    for dir in [&skin_dir, &sys_dir] {
        fs::create_dir_all(dir)?;
        for (name, data) in &fonts {
            let p = dir.join(name);
            fs::write(&p, data)?;
            out.push(p);
        }
    }
    if opts.run_fc_cache {
        let ok = std::process::Command::new("fc-cache")
            .arg("-f")
            .arg(&sys_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            log::info!("fc-cache unavailable or failed for {}", sys_dir.display());
        }
    }
    Ok(out)
}

// ----------------------------------------------------- bundled-skin sync

const BUNDLED_KEY: &str = "BundledInstalled";

fn bundled_list(settings: &Settings) -> Option<Vec<String>> {
    settings.global(BUNDLED_KEY).map(|v| v.split('|').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect())
}

fn record_bundled(settings: &mut Settings, root: &str) {
    let mut list = bundled_list(settings).unwrap_or_default();
    if !list.iter().any(|r| r.eq_ignore_ascii_case(root)) {
        list.push(root.to_string());
        settings.set_global(BUNDLED_KEY, &list.join("|"));
    }
}

/// Skin roots shipped as folders: the catalog's `folder` entries, or every
/// top-level directory except `packages` when there is no catalog.
pub fn bundled_folder_roots(bundled: &Path) -> Vec<String> {
    let cat = load_catalog(bundled);
    if bundled.join("catalog.ini").is_file() {
        return cat.iter().filter(|e| e.kind == Kind::Folder).map(|e| e.path.trim_matches(['/', '\\']).to_string()).filter(|p| !p.is_empty()).collect();
    }
    let mut v: Vec<String> = fs::read_dir(bundled)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.retain(|n| !n.eq_ignore_ascii_case("packages"));
    v.sort();
    v
}

/// Copy bundled `folder` roots that were never offered to this user (not in
/// `[Snowmeter] BundledInstalled`), without overwriting, and record them. A
/// root the user deleted afterwards stays deleted. Existing installs from
/// before the key existed (first run done) are seeded with `Snowmeter`.
/// Returns the roots copied now. Does not save `settings`.
pub fn sync_bundled(env: &Environment, settings: &mut Settings, bundled: &Path) -> Vec<String> {
    if bundled_list(settings).is_none() && settings.first_run_done() {
        settings.set_global(BUNDLED_KEY, "Snowmeter");
    }
    let done = bundled_list(settings).unwrap_or_default();
    let mut copied = Vec::new();
    for root in bundled_folder_roots(bundled) {
        if done.iter().any(|r| r.eq_ignore_ascii_case(&root)) {
            continue;
        }
        let Ok(rel) = safe_rel(&root) else { continue };
        let src = bundled.join(&rel);
        if !src.is_dir() {
            continue;
        }
        match copy_tree(&src, &env.skins_dir.join(&rel), false) {
            Ok(n) => {
                log::info!("gallery: copied bundled skin {root} ({n} files)");
                record_bundled(settings, &root);
                copied.push(root);
            }
            Err(e) => log::warn!("gallery: copying bundled skin {root} failed: {e}"),
        }
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    const CAT: &str = "[Suite]\nKind=folder\nPath=Suite\nAuthor=Me\nLicense=MIT\n\n\
[Pkg]\nKind=package\nPath=packages/Pkg.rmskin\nStatus=good\nFontUrl2=http://x/b.ttf\nFontUrl1=http://x/a.ttf\nPostInstall=mond-weather-wttr, other\n\n\
[Rem]\nKind=remote\nUrl=http://x/r.zip\nSubdir=Skins/RemSkin\n\n[Junk]\nKind=weird\n";

    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (n, d) in files {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(d).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn rmskin_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip_bytes(files);
        let n = z.len() as i64;
        z.extend_from_slice(&n.to_le_bytes());
        z.push(0);
        z.extend_from_slice(b"RMSKIN\0");
        z
    }

    fn write(p: &Path, s: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }

    /// Serve `routes` (path -> body) on 127.0.0.1 for the rest of the process.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { continue };
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = s.write_all(body);
                    }
                    None => {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    struct Fx {
        _t: tempfile::TempDir,
        env: Environment,
        bundled: PathBuf,
    }

    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let env = Environment::with_root(&t.path().join("cfg"));
        let bundled = t.path().join("bundled");
        write(&bundled.join("catalog.ini"), CAT.as_bytes());
        write(&bundled.join("Suite/One/One.ini"), b"[Rainmeter]\n[M]\nMeter=String\n");
        Fx { _t: t, env, bundled }
    }

    fn opts(f: &Fx) -> InstallOptions {
        InstallOptions { bundled_dir: Some(f.bundled.clone()), run_fc_cache: false, ..Default::default() }
    }

    #[test]
    fn catalog_parsing() {
        let e = parse_catalog(CAT);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].kind, Kind::Folder);
        assert_eq!((e[0].author.as_str(), e[0].license.as_str()), ("Me", "MIT"));
        assert_eq!(e[1].font_urls, vec!["http://x/a.ttf", "http://x/b.ttf"]);
        assert_eq!(e[1].post_install, vec!["mond-weather-wttr", "other"]);
        assert_eq!(e[1].root_name(), "Pkg");
        assert_eq!(e[2].subdir, "Skins/RemSkin");
        assert_eq!(e[2].root_name(), "RemSkin");
        assert_eq!(e[0].root_name(), "Suite");
    }

    #[test]
    fn folder_install_no_overwrite_and_force() {
        let f = fx();
        let entry = parse_catalog(CAT).remove(0);
        let mut s = Settings::load(&f.env);
        assert!(!is_installed(&f.env, &entry));
        let r = install(&f.env, &mut s, &entry, &opts(&f)).unwrap();
        assert_eq!(r.configs, vec!["Suite\\One"]);
        assert!(is_installed(&f.env, &entry));
        write(&f.env.skins_dir.join("Suite/One/One.ini"), b"mine");
        assert!(matches!(install(&f.env, &mut s, &entry, &opts(&f)), Err(GalleryError::AlreadyInstalled(_))));
        assert_eq!(fs::read(f.env.skins_dir.join("Suite/One/One.ini")).unwrap(), b"mine");
        let o = InstallOptions { force: true, ..opts(&f) };
        install(&f.env, &mut s, &entry, &o).unwrap();
        assert!(fs::read_to_string(f.env.skins_dir.join("Suite/One/One.ini")).unwrap().contains("Meter=String"));
    }

    #[test]
    fn package_install_keeps_other_skins_active() {
        let f = fx();
        let pkg = rmskin_bytes(&[
            ("RMSKIN.ini", b"[rmskin]\nName=Pkg\nAuthor=a\nVersion=1\nLoadType=Layout\nLoad=PL\n"),
            ("Layouts/PL/Rainmeter.ini", b"[Rainmeter]\n[Pkg\\A]\nActive=1\n"),
            ("Skins/Pkg/A/A.ini", b"[Rainmeter]\n"),
        ]);
        write(&f.bundled.join("packages/Pkg.rmskin"), &pkg);
        write(&f.env.skins_dir.join("Other/X/X.ini"), b"[Rainmeter]\n");
        let mut s = Settings::load(&f.env);
        rmskin::activate_skin(&f.env, &mut s, "Other\\X", "X.ini").unwrap();
        s.save().unwrap();
        let entry = parse_catalog(CAT).remove(1);
        let r = install(&f.env, &mut s, &entry, &opts(&f)).unwrap();
        assert_eq!(r.roots, vec!["Pkg"]);
        let names: Vec<String> = s.active_configs().into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"Other\\X".to_string()), "{names:?}");
        assert!(names.contains(&"Pkg\\A".to_string()), "{names:?}");
    }

    fn remote_entry(base: &str, url: &str, fonts: &[&str]) -> Entry {
        let mut e = parse_catalog(CAT).remove(2);
        e.url = format!("{base}{url}");
        e.font_urls = fonts.iter().map(|f| format!("{base}{f}")).collect();
        e
    }

    #[test]
    fn remote_install_with_subdir_fonts_and_size_cap() {
        let f = fx();
        let archive = zip_bytes(&[
            ("repo-abc123/README.md", b"hi"),
            ("repo-abc123/Skins/RemSkin/Main/Main.ini", b"[Rainmeter]\n[M]\nMeter=String\n"),
        ]);
        let font_zip = zip_bytes(&[("fonts/Cool.ttf", b"TTFDATA")]);
        let base = serve(vec![("/r.zip", archive.clone()), ("/plain.otf", b"OTFDATA".to_vec()), ("/f.zip", font_zip)]);
        let entry = remote_entry(&base, "/r.zip", &["/plain.otf", "/f.zip", "/missing.ttf"]);
        let mut s = Settings::load(&f.env);
        let r = install(&f.env, &mut s, &entry, &opts(&f)).unwrap();
        assert_eq!(r.roots, vec!["RemSkin"]);
        assert_eq!(r.configs, vec!["RemSkin\\Main"]);
        assert!(f.env.skins_dir.join("RemSkin/Main/Main.ini").is_file());
        assert_eq!(fs::read(f.env.skins_dir.join("RemSkin/@Resources/Fonts/plain.otf")).unwrap(), b"OTFDATA");
        assert_eq!(fs::read(f.env.skins_dir.join("RemSkin/@Resources/Fonts/Cool.ttf")).unwrap(), b"TTFDATA");
        assert_eq!(fs::read(f.env.fonts_install_dir.join("RemSkin/Cool.ttf")).unwrap(), b"TTFDATA");
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings); // the 404 font only
        // Size cap.
        let small = InstallOptions { max_package_bytes: 50, force: true, ..opts(&f) };
        assert!(matches!(install(&f.env, &mut s, &entry, &small), Err(GalleryError::TooLarge(_))));
        let tiny_fonts = InstallOptions { max_font_bytes: 3, force: true, ..opts(&f) };
        let r = install(&f.env, &mut s, &entry, &tiny_fonts).unwrap();
        assert!(r.warnings.len() >= 3, "{:?}", r.warnings);
    }

    #[test]
    fn bundled_sync_copies_new_roots_once() {
        let f = fx();
        let mut s = Settings::load(&f.env);
        assert_eq!(sync_bundled(&f.env, &mut s, &f.bundled), vec!["Suite"]);
        assert!(f.env.skins_dir.join("Suite/One/One.ini").is_file());
        assert_eq!(s.global("BundledInstalled"), Some("Suite"));
        // User deletes the root: not copied again.
        fs::remove_dir_all(f.env.skins_dir.join("Suite")).unwrap();
        assert!(sync_bundled(&f.env, &mut s, &f.bundled).is_empty());
        assert!(!f.env.skins_dir.join("Suite").exists());
        // A new bundled folder entry appears in a later version.
        let cat = format!("{CAT}\n[New]\nKind=folder\nPath=New\n");
        write(&f.bundled.join("catalog.ini"), cat.as_bytes());
        write(&f.bundled.join("New/N/N.ini"), b"[Rainmeter]\n");
        write(&f.bundled.join("packages/Pkg.rmskin"), b"x");
        assert_eq!(sync_bundled(&f.env, &mut s, &f.bundled), vec!["New"]);
        assert!(f.env.skins_dir.join("New/N/N.ini").is_file());
        assert!(!f.env.skins_dir.join("packages").exists());
        assert_eq!(s.global("BundledInstalled"), Some("Suite|New"));
        assert!(sync_bundled(&f.env, &mut s, &f.bundled).is_empty());
    }

    #[test]
    fn legacy_install_is_seeded() {
        let f = fx();
        let mut s = Settings::load(&f.env);
        s.set_first_run_done(true);
        // `Snowmeter` was copied by the old first run; other roots are new to this user.
        assert_eq!(sync_bundled(&f.env, &mut s, &f.bundled), vec!["Suite"]);
        assert_eq!(s.global("BundledInstalled"), Some("Snowmeter|Suite"));
        assert!(!f.env.skins_dir.join("Snowmeter").exists());
    }
}
