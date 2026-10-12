//! FolderInfo plugin: size, file count and folder count of a directory,
//! scanned on a worker thread at most every ten seconds.

use super::bgshared::{bracket_ref, lookup, publish, Bg};
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use pcre2::bytes::{Regex, RegexBuilder};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MIN_RESCAN: Duration = Duration::from_secs(10);
/// Safety valve against runaway trees.
const MAX_ENTRIES: u64 = 20_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub size: u64,
    pub files: u64,
    pub folders: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Params {
    pub path: PathBuf,
    pub subfolders: bool,
    pub hidden: bool,
    pub filter: String,
}

/// Walk `p.path` like Rainmeter's `CalculateSize` (hidden = dot files here).
pub fn scan(p: &Params) -> Stats {
    let mut st = Stats::default();
    if p.path.as_os_str().is_empty() {
        return st;
    }
    let filter: Option<Regex> = if p.filter.is_empty() { None } else { RegexBuilder::new().build(&p.filter).ok() };
    let mut queue = vec![p.path.clone()];
    let mut seen = 0u64;
    while let Some(dir) = queue.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return st;
            }
            let name = e.file_name();
            let name = name.to_string_lossy();
            if !p.hidden && name.starts_with('.') {
                continue;
            }
            let Ok(md) = e.path().symlink_metadata() else { continue };
            if md.is_dir() {
                st.folders += 1;
                if p.subfolders {
                    queue.push(e.path());
                }
            } else {
                if let Some(f) = &filter {
                    if !f.is_match(name.as_bytes()).unwrap_or(false) {
                        continue;
                    }
                }
                st.files += 1;
                st.size += md.len();
            }
        }
    }
    st
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Info {
    FileCount,
    FolderCount,
    FolderSize,
}

/// State shared by an owning measure and the children that say `Folder=[Owner]`.
struct Parent {
    stats: Bg<Stats>,
    last: Mutex<(Option<Instant>, Params)>,
}

pub struct FolderInfo {
    info: Info,
    folder: String,
    subfolders: bool,
    hidden: bool,
    filter: String,
    section: String,
    parent: Option<Arc<Parent>>,
    owner: bool,
    log: Vec<String>,
}

impl FolderInfo {
    pub fn new() -> FolderInfo {
        FolderInfo {
            info: Info::FileCount,
            folder: String::new(),
            subfolders: false,
            hidden: false,
            filter: String::new(),
            section: String::new(),
            parent: None,
            owner: false,
            log: Vec::new(),
        }
    }
}

impl MeasureImpl for FolderInfo {
    fn read_options(&mut self, opt: &OptionReader) {
        self.section = opt.section.to_string();
        self.info = match opt.string("InfoType", "FileCount").trim().to_ascii_lowercase().as_str() {
            "foldersize" | "foldersizestr" => Info::FolderSize,
            "foldercount" | "foldercountstr" => Info::FolderCount,
            _ => Info::FileCount,
        };
        let folder = opt.without_sections().string("Folder", "");
        if bracket_ref(&folder).is_some() {
            self.owner = false;
            self.folder = folder;
            self.parent = None;
            return;
        }
        self.owner = true;
        self.folder = opt.string("Folder", "");
        self.filter = opt.string("RegExpFilter", "");
        self.subfolders = opt.bool("IncludeSubFolders", false);
        self.hidden = opt.bool("IncludeHiddenFiles", false);
        if !self.filter.is_empty() && RegexBuilder::new().build(&self.filter).is_err() {
            self.log.push(format!("FolderInfo: invalid RegExpFilter \"{}\"", self.filter));
            self.filter.clear();
        }
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        if !self.owner {
            if self.parent.is_none() {
                self.parent = bracket_ref(&self.folder).and_then(|n| lookup::<Parent>(ctx.skin_dir, n));
                if self.parent.is_none() {
                    ctx.log.push(format!("FolderInfo: Invalid Folder={}", self.folder));
                }
            }
        } else {
            let params = Params {
                path: crate::paths::resolve_path(ctx.skin_dir, &self.folder),
                subfolders: self.subfolders,
                hidden: self.hidden,
                filter: self.filter.clone(),
            };
            let parent = match &self.parent {
                Some(p) => p.clone(),
                None => {
                    let p = Arc::new(Parent { stats: Bg::new(Stats::default()), last: Mutex::new((None, Params::default())) });
                    publish(ctx.skin_dir, &self.section, p.clone());
                    self.parent = Some(p.clone());
                    p
                }
            };
            let due = match parent.last.lock() {
                Ok(l) => l.0.map(|t| t.elapsed() > MIN_RESCAN).unwrap_or(true) || l.1 != params,
                Err(_) => false,
            };
            if due && !parent.stats.running() {
                if let Ok(mut l) = parent.last.lock() {
                    *l = (Some(Instant::now()), params.clone());
                }
                parent.stats.start(Duration::from_millis(80), move || scan(&params));
            }
        }
        let Some(p) = &self.parent else { return 0.0 };
        let s = p.stats.get();
        match self.info {
            Info::FileCount => s.files as f64,
            Info::FolderCount => s.folders as f64,
            Info::FolderSize => s.size as f64,
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        std::fs::create_dir_all(r.join("sub/deeper")).unwrap();
        std::fs::create_dir_all(r.join(".hiddendir")).unwrap();
        std::fs::write(r.join("a.txt"), "12345").unwrap();
        std::fs::write(r.join(".hidden"), "xx").unwrap();
        std::fs::write(r.join("sub/b.log"), "123").unwrap();
        std::fs::write(r.join("sub/deeper/c.txt"), "1").unwrap();
        d
    }

    #[test]
    fn scan_counts() {
        let d = tree();
        let p = |sub, hid, f: &str| Params { path: d.path().to_path_buf(), subfolders: sub, hidden: hid, filter: f.into() };
        assert_eq!(scan(&p(false, false, "")), Stats { size: 5, files: 1, folders: 1 });
        assert_eq!(scan(&p(true, false, "")), Stats { size: 9, files: 3, folders: 2 });
        assert_eq!(scan(&p(true, true, "")), Stats { size: 11, files: 4, folders: 3 });
        assert_eq!(scan(&p(true, false, "\\.txt$")), Stats { size: 6, files: 2, folders: 2 });
        assert_eq!(scan(&p(false, false, "")).size, 5);
        assert_eq!(scan(&Params::default()), Stats::default());
        assert_eq!(scan(&Params { path: "/nonexistent-dir".into(), ..Default::default() }), Stats::default());
    }

    #[test]
    fn measures_share_a_parent() {
        let d = tree();
        let ini = format!(
            "[FiP]\nMeasure=Plugin\nPlugin=FolderInfo\nFolder={}\nInfoType=FileCount\nIncludeSubFolders=1\n\
             [C]\nMeasure=Plugin\nPlugin=FolderInfo\nFolder=[FiP]\nInfoType=FolderSize\n\
             [Bad]\nMeasure=Plugin\nPlugin=FolderInfo\nFolder=[Nope]\n",
            d.path().display()
        );
        let sk = skin(&ini);
        let mk = |sec: &str| {
            let mut m = FolderInfo::new();
            m.read_options(&OptionReader { parser: &sk.parser, section: sec, sv: None });
            m
        };
        let (mut p, mut c, mut bad) = (mk("FiP"), mk("C"), mk("Bad"));
        ctx_run(&sk, |ctx| {
            assert_eq!(p.update(ctx), 3.0);
            assert_eq!(c.update(ctx), 9.0);
            assert_eq!(bad.update(ctx), 0.0);
        });
    }
}
