//! Scan the skins folder: configs (backslash names) and their `.ini` files in
//! Rainmeter order (`rmskin-format.md` 5).

use crate::env::Environment;
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 12;

/// One config folder, e.g. `illustro\Clock` with files `["Clock.ini"]`. A
/// folder that only has sub-configs is listed with no files.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinConfig {
    pub name: String,
    pub root: String,
    pub dir: PathBuf,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SkinRegistry {
    pub skins_dir: PathBuf,
    pub configs: Vec<SkinConfig>,
}

/// NTFS directory order: compare upper-cased UTF-16 code units.
pub fn ntfs_cmp(a: &str, b: &str) -> Ordering {
    fn key(s: &str) -> Vec<u16> {
        s.chars().flat_map(|c| c.to_uppercase()).collect::<String>().encode_utf16().collect()
    }
    key(a).cmp(&key(b))
}

pub fn is_ignored_root(name: &str) -> bool {
    ["@backup", "backup", "@vault"].iter().any(|n| name.eq_ignore_ascii_case(n))
}

fn is_ini(name: &str) -> bool {
    name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".ini")
}

impl SkinRegistry {
    pub fn scan(env: &Environment) -> SkinRegistry {
        let mut reg = SkinRegistry { skins_dir: env.skins_dir.clone(), configs: Vec::new() };
        let mut roots: Vec<String> = Vec::new();
        if let Ok(rd) = fs::read_dir(&env.skins_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if is_ignored_root(&name) || name.starts_with('.') || !e.path().is_dir() {
                    continue;
                }
                roots.push(name);
            }
        }
        roots.sort_by(|a, b| ntfs_cmp(a, b));
        for r in roots {
            reg.scan_dir(&r, &r, &env.skins_dir.join(&r), 0);
        }
        reg
    }

    /// Returns true when this folder or a descendant is a config.
    fn scan_dir(&mut self, root: &str, config: &str, dir: &Path, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let p = e.path();
                if p.is_dir() {
                    // `@Resources` directly under a root is not a config (case-sensitive).
                    if depth == 0 && name == "@Resources" || name.starts_with('.') {
                        continue;
                    }
                    subdirs.push(name);
                } else if is_ini(&name) {
                    files.push(name);
                }
            }
        }
        files.sort_by(|a, b| ntfs_cmp(a, b));
        subdirs.sort_by(|a, b| ntfs_cmp(a, b));
        let idx = self.configs.len();
        // Reserve the slot so the parent precedes its children.
        self.configs.push(SkinConfig {
            name: config.to_string(),
            root: root.to_string(),
            dir: dir.to_path_buf(),
            files: files.clone(),
        });
        let mut any = !files.is_empty();
        for s in subdirs {
            let sub = format!("{config}\\{s}");
            any |= self.scan_dir(root, &sub, &dir.join(&s), depth + 1);
        }
        if !any {
            self.configs.remove(idx);
            // Children (none were kept) cannot exist after `any == false`.
        }
        any
    }

    /// Case-insensitive lookup; `/` is accepted as separator.
    pub fn config(&self, name: &str) -> Option<&SkinConfig> {
        let n = name.trim().replace('/', "\\");
        let n = n.trim_matches('\\');
        self.configs.iter().find(|c| c.name.eq_ignore_ascii_case(n))
    }

    /// 1-based index of `file` in the config (the `Active=N` value).
    pub fn file_index(&self, config: &str, file: &str) -> Option<u32> {
        let c = self.config(config)?;
        c.files.iter().position(|f| f.eq_ignore_ascii_case(file.trim())).map(|i| i as u32 + 1)
    }

    /// File name for `Active=n` (1-based); `None` for 0 or out of range.
    pub fn file_at(&self, config: &str, n: u32) -> Option<&str> {
        if n == 0 {
            return None;
        }
        self.config(config)?.files.get(n as usize - 1).map(String::as_str)
    }

    /// Absolute path of `file` in `config`.
    pub fn ini_path(&self, config: &str, file: &str) -> Option<PathBuf> {
        let c = self.config(config)?;
        let f = c.files.iter().find(|f| f.eq_ignore_ascii_case(file.trim()))?;
        Some(c.dir.join(f))
    }

    /// Configs that have at least one `.ini`.
    pub fn loadable(&self) -> impl Iterator<Item = &SkinConfig> {
        self.configs.iter().filter(|c| !c.files.is_empty())
    }
}
