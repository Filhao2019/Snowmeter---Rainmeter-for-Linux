//! Path resolution for Windows-authored skins on a case-sensitive file system.
//!
//! `\` is a separator, and each component that does not exist verbatim is
//! matched case-insensitively against the directory listing.

use std::fs;
use std::path::{Component, Path, PathBuf};

thread_local! {
    static DIR_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Number of directory listings done by case-insensitive matching on this thread.
pub fn dir_scans() -> usize {
    DIR_SCANS.with(|c| c.get())
}

/// Find the entry of `dir` whose name equals `name` ignoring case.
/// The verbatim spelling wins; ties are broken alphabetically.
fn match_in_dir(dir: &Path, name: &str) -> Option<String> {
    if fs::symlink_metadata(dir.join(name)).is_ok() {
        return Some(name.to_string());
    }
    DIR_SCANS.with(|c| c.set(c.get() + 1));
    let lower = name.to_lowercase();
    let mut hits: Vec<String> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.to_lowercase() == lower)
        .collect();
    hits.sort();
    hits.into_iter().next()
}

fn looks_like_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Resolve a skin-authored path (`@Resources\images\BG.PNG`, `#@#`-expanded
/// absolute paths, `..\x`) against `base_dir`.
///
/// * An empty value gives an empty path.
/// * Absolute means a leading `/` (or `//`, `\\`), mirroring Rainmeter where a
///   single leading `\` is *not* absolute and stays relative to `base_dir`.
/// * Windows drive paths (`C:\x`) are returned with `/` separators unchanged;
///   they do not exist on Linux and are reported as missing by callers.
/// * Missing components keep the spelling the skin used.
pub fn resolve_path(base_dir: &Path, value: &str) -> PathBuf {
    if value.is_empty() {
        return PathBuf::new();
    }
    let v = value.replace('\\', "/");
    if looks_like_drive(&v) {
        return PathBuf::from(v);
    }
    let absolute = v.starts_with("//") || (v.starts_with('/') && !value.starts_with('\\'));
    let mut cur = if absolute { PathBuf::from("/") } else { base_dir.to_path_buf() };
    for comp in v.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                cur.pop();
            }
            name => match match_in_dir(&cur, name) {
                Some(actual) => cur.push(actual),
                None => cur.push(name),
            },
        }
    }
    cur
}

/// Return the real path for `path` if it exists, matching each component
/// case-insensitively; `None` when it does not exist at all.
pub fn find_case_insensitive(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        return Some(path.to_path_buf());
    }
    let mut cur = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(_) | Component::RootDir => cur.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                cur.push("..");
            }
            Component::Normal(name) => {
                let base = if cur.as_os_str().is_empty() { Path::new(".") } else { cur.as_path() };
                let actual = match_in_dir(base, name.to_str()?)?;
                cur.push(actual);
            }
        }
    }
    cur.exists().then_some(cur)
}
