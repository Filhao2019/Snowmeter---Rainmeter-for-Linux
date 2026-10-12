//! FileView plugin: a paged, sorted directory listing exposed as measures
//! (name, size, date, ... of the Nth entry). The first measure owns the
//! listing; children say `Path=[Owner]`. Listing runs on a worker thread.

use super::bgshared::{bracket_ref, lookup, publish, Bg};
use super::sysinfo::local_offset;
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FILETIME_EPOCH_DELTA: i64 = 11_644_473_600;
const MAX_ENTRIES: usize = 2_000_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileInfo {
    pub name: String,
    /// Directory containing the entry, with a trailing `/`.
    pub dir: String,
    pub is_folder: bool,
    pub size: u64,
    pub ext: String,
    pub modified: i64,
    pub created: i64,
    pub accessed: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub files: Vec<FileInfo>,
    pub file_count: u64,
    pub folder_count: u64,
    pub folder_size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortType {
    Name,
    Size,
    Type,
    Date,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateType {
    Modified,
    Created,
    Accessed,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub sort: SortType,
    pub sort_date: DateType,
    pub ascending: bool,
    /// 0 none, 1 partial (counts only), 2 full (all files flat).
    pub recursive: u8,
    pub dot_dot: bool,
    pub files: bool,
    pub folders: bool,
    pub hidden: bool,
    pub extensions: Vec<String>,
    pub exclude_ext: bool,
    pub wildcard: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            sort: SortType::Name,
            sort_date: DateType::Modified,
            ascending: true,
            recursive: 0,
            dot_dot: true,
            files: true,
            folders: true,
            hidden: true,
            extensions: Vec::new(),
            exclude_ext: false,
            wildcard: "*".into(),
        }
    }
}

/// `*` / `?` glob, case-insensitive.
pub fn wildcard_match(pat: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| go(rest, &n[i..])),
            Some(('?', rest)) => !n.is_empty() && go(rest, &n[1..]),
            Some((c, rest)) => n.first().is_some_and(|x| x.eq_ignore_ascii_case(c)) && go(rest, &n[1..]),
        }
    }
    let p: Vec<char> = pat.chars().collect();
    let n: Vec<char> = name.chars().collect();
    pat == "*" || go(&p, &n)
}

fn secs(t: std::io::Result<SystemTime>) -> i64 {
    t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn with_slash(p: &str) -> String {
    if p.ends_with('/') {
        p.to_string()
    } else {
        format!("{p}/")
    }
}

/// Entries of one directory according to Rainmeter's `GetFolderInfo`.
fn read_dir_entries(dir: &str, o: &Options, rtype: u8, out: &mut Vec<FileInfo>, queue: &mut Vec<String>, l: &mut Listing) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if l.files.len() + out.len() > MAX_ENTRIES {
            return;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(md) = e.path().symlink_metadata() else { continue };
        let md = if md.file_type().is_symlink() { std::fs::metadata(e.path()).unwrap_or(md) } else { md };
        let is_folder = md.is_dir();
        if rtype == 0 && o.wildcard != "*" && !wildcard_match(&o.wildcard, &name) {
            continue;
        }
        if rtype == 2 && o.wildcard != "*" && !is_folder && !wildcard_match(&o.wildcard, &name) {
            continue;
        }
        let hidden = name.starts_with('.');
        if rtype != 1 && ((rtype != 2 && !o.files && !is_folder) || (rtype != 2 && !o.folders && is_folder) || (!o.hidden && hidden)) {
            continue;
        }
        let mut ext = String::new();
        if rtype != 1 && !is_folder {
            let mut found = false;
            if let Some(p) = name.rfind('.') {
                ext = name[p + 1..].to_string();
                found = o.extensions.iter().any(|x| x.eq_ignore_ascii_case(&ext));
            }
            if !o.extensions.is_empty() && ((!o.exclude_ext && !found) || (o.exclude_ext && found)) {
                continue;
            }
        }
        let mut f = FileInfo {
            name: name.clone(),
            dir: with_slash(dir),
            is_folder,
            ext,
            modified: secs(md.modified()),
            created: secs(md.created()),
            accessed: secs(md.accessed()),
            ..Default::default()
        };
        if is_folder {
            if rtype != 2 {
                l.folder_count += 1;
            }
            queue.push(format!("{}{}/", f.dir, name));
        } else {
            l.file_count += 1;
            f.size = md.len();
        }
        l.folder_size += f.size;
        if rtype == 0 || (rtype == 2 && !is_folder) {
            out.push(f);
        }
    }
}

/// Build the listing for `path` (a directory path with trailing `/`).
pub fn list(path: &str, o: &Options) -> Listing {
    let mut l = Listing::default();
    let root = if path.is_empty() { "/" } else { path };
    let mut entries: Vec<FileInfo> = Vec::new();
    let dotdot = o.dot_dot && o.recursive != 2 && !path.is_empty();
    let mut queue: Vec<String> = Vec::new();
    read_dir_entries(root, o, if o.recursive == 1 { 0 } else { o.recursive }, &mut entries, &mut queue, &mut l);
    if o.recursive != 0 {
        let mut i = 0;
        while i < queue.len() && l.file_count < MAX_ENTRIES as u64 {
            let dir = queue[i].clone();
            i += 1;
            let mut q2 = Vec::new();
            read_dir_entries(&dir, o, o.recursive, &mut entries, &mut q2, &mut l);
            queue.extend(q2);
        }
    }
    let asc = o.ascending;
    let key_date = |f: &FileInfo| match o.sort_date {
        DateType::Modified => f.modified,
        DateType::Created => f.created,
        DateType::Accessed => f.accessed,
    };
    entries.sort_by(|a, b| {
        use std::cmp::Ordering::*;
        if a.is_folder != b.is_folder {
            return if a.is_folder { Less } else { Greater };
        }
        let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase());
        let r = match o.sort {
            SortType::Name => by_name(),
            SortType::Size if !a.is_folder => a.size.cmp(&b.size),
            SortType::Type if !a.is_folder => a.ext.to_lowercase().cmp(&b.ext.to_lowercase()).then_with(by_name),
            SortType::Date => key_date(a).cmp(&key_date(b)),
            _ => by_name(),
        };
        if asc {
            r
        } else {
            r.reverse()
        }
    });
    if dotdot {
        l.files.push(FileInfo { name: "..".into(), dir: path.to_string(), is_folder: true, ..Default::default() });
    }
    l.files.extend(entries);
    l
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    FolderPath,
    FolderSize,
    FileCount,
    FolderCount,
    FileName,
    FileType,
    FileSize,
    FileDate,
    FilePath,
    PathToFile,
    Icon,
}

struct PState {
    path: String,
    opts: Options,
    count: usize,
    offset: usize,
    needs_update: bool,
    finish_action: String,
}

struct Parent {
    st: Mutex<PState>,
    bg: Bg<Arc<Listing>>,
    finished: Arc<AtomicBool>,
}

pub struct FileView {
    section: String,
    owner: bool,
    parent: Option<Arc<Parent>>,
    parent_ref: String,
    raw_path: String,
    pending_opts: Option<(Options, usize, String)>,
    index: usize,
    ignore_count: bool,
    kind: Kind,
    date_type: DateType,
    string: Option<String>,
    actions: Vec<String>,
    log: Vec<String>,
}

fn split_list(s: &str) -> Vec<String> {
    s.split(';').map(|x| x.trim().trim_start_matches('.').to_string()).filter(|x| !x.is_empty()).collect()
}

impl FileView {
    pub fn new() -> FileView {
        FileView {
            section: String::new(),
            owner: false,
            parent: None,
            parent_ref: String::new(),
            raw_path: String::new(),
            pending_opts: None,
            index: 0,
            ignore_count: false,
            kind: Kind::FolderPath,
            date_type: DateType::Modified,
            string: None,
            actions: Vec::new(),
            log: Vec::new(),
        }
    }

    fn true_index(&self, p: &PState) -> usize {
        if self.ignore_count {
            self.index
        } else {
            (self.index % p.count.max(1)) + p.offset
        }
    }
}

fn file_time(t: i64) -> f64 {
    (t + local_offset().0 + FILETIME_EPOCH_DELTA) as f64
}

fn format_local(t: i64) -> String {
    if t <= 0 {
        return String::new();
    }
    // SAFETY: localtime_r / strftime write into buffers we own.
    unsafe {
        let tt = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&tt, &mut tm).is_null() {
            return String::new();
        }
        let mut buf = [0u8; 128];
        let n = libc::strftime(buf.as_mut_ptr().cast(), buf.len(), c"%x %X".as_ptr(), &tm);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

fn parent_dir(path: &str) -> String {
    let t = path.trim_end_matches('/');
    match t.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(p) => format!("{}/", &t[..p]),
    }
}

impl MeasureImpl for FileView {
    fn read_options(&mut self, opt: &OptionReader) {
        self.section = opt.section.to_string();
        let raw = opt.without_sections().string("Path", "");
        self.index = (opt.int("Index", 1) - 1).max(0) as usize;
        self.ignore_count = opt.bool("IgnoreCount", false);
        self.kind = match opt.string("Type", "FolderPath").trim().to_ascii_lowercase().as_str() {
            "foldersize" => Kind::FolderSize,
            "filecount" => Kind::FileCount,
            "foldercount" => Kind::FolderCount,
            "filename" => Kind::FileName,
            "filetype" => Kind::FileType,
            "filesize" => Kind::FileSize,
            "filedate" => Kind::FileDate,
            "filepath" => Kind::FilePath,
            "pathtofile" => Kind::PathToFile,
            "icon" => Kind::Icon,
            _ => Kind::FolderPath,
        };
        self.date_type = match opt.string("DateType", "Modified").trim().to_ascii_lowercase().as_str() {
            "created" => DateType::Created,
            "accessed" => DateType::Accessed,
            _ => DateType::Modified,
        };
        if let Some(name) = bracket_ref(&raw) {
            self.owner = false;
            self.parent_ref = name.to_string();
            self.parent = None;
            return;
        }
        self.owner = true;
        self.raw_path = opt.string("Path", "");
        let mut o = Options::default();
        o.sort = match opt.string("SortType", "Name").trim().to_ascii_lowercase().as_str() {
            "size" => SortType::Size,
            "type" => SortType::Type,
            "date" => SortType::Date,
            _ => SortType::Name,
        };
        o.sort_date = match opt.string("SortDateType", "Modified").trim().to_ascii_lowercase().as_str() {
            "created" => DateType::Created,
            "accessed" => DateType::Accessed,
            _ => DateType::Modified,
        };
        let count = opt.int("Count", 1);
        o.recursive = match opt.int("Recursive", 0) {
            1 => 1,
            2 => 2,
            0 => 0,
            _ => {
                self.log.push("FileView: Invalid Recursive type".into());
                0
            }
        };
        o.ascending = opt.bool("SortAscending", true);
        o.dot_dot = opt.bool("ShowDotDot", true);
        o.files = opt.bool("ShowFile", true);
        o.folders = opt.bool("ShowFolder", true);
        o.hidden = opt.bool("ShowHidden", true);
        let inc = opt.string("Extensions", "");
        if !inc.is_empty() {
            o.extensions = split_list(&inc);
        } else {
            o.extensions = split_list(&opt.string("ExcludeExtensions", ""));
            o.exclude_ext = true;
        }
        o.wildcard = opt.string("WildcardSearch", "*");
        if o.wildcard.is_empty() {
            o.wildcard = "*".into();
        }
        self.pending_opts = Some((o, if count > 0 { count as usize } else { 1 }, opt.action("FinishAction")));
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        if self.owner {
            let resolved = crate::paths::resolve_path(ctx.skin_dir, &self.raw_path);
            let resolved = if self.raw_path.trim().is_empty() { String::new() } else { with_slash(&resolved.to_string_lossy()) };
            if self.parent.is_none() {
                let p = Arc::new(Parent {
                    st: Mutex::new(PState {
                        path: resolved.clone(),
                        opts: Options::default(),
                        count: 1,
                        offset: 0,
                        needs_update: true,
                        finish_action: String::new(),
                    }),
                    bg: Bg::new(Arc::new(Listing::default())),
                    finished: Arc::new(AtomicBool::new(false)),
                });
                publish(ctx.skin_dir, &self.section, p.clone());
                self.parent = Some(p);
            }
            let Some(p) = self.parent.clone() else { return 0.0 };
            let job = match p.st.lock() {
                Ok(mut st) => {
                    if let Some((o, c, fa)) = self.pending_opts.take() {
                        st.opts = o;
                        st.count = c;
                        st.finish_action = fa;
                        // A changed Path option (not a user navigation) restarts the listing.
                        st.path = resolved;
                        st.offset = 0;
                        st.needs_update = true;
                    }
                    if st.needs_update && !p.bg.running() {
                        st.needs_update = false;
                        Some((st.path.clone(), st.opts.clone(), !st.finish_action.is_empty()))
                    } else {
                        None
                    }
                }
                Err(_) => None,
            };
            if let Some((path, opts, has_finish)) = job {
                let fin = p.finished.clone();
                p.bg.start(Duration::from_millis(80), move || {
                    let l = Arc::new(list(&path, &opts));
                    if has_finish {
                        fin.store(true, Ordering::SeqCst);
                    }
                    l
                });
            }
        } else if self.parent.is_none() {
            self.parent = lookup::<Parent>(ctx.skin_dir, &self.parent_ref);
            if self.parent.is_none() {
                ctx.log.push(format!("FileView: Invalid Path: \"[{}]\"", self.parent_ref));
            }
        }
        self.string = None;
        let Some(p) = self.parent.clone() else { return 0.0 };
        let listing = p.bg.get();
        let (path, entry) = {
            let Ok(st) = p.st.lock() else { return 0.0 };
            (st.path.clone(), listing.files.get(self.true_index(&st)).cloned())
        };
        let mut value = 0.0;
        let mut text = String::new();
        match self.kind {
            Kind::FolderPath => text = path.clone(),
            Kind::FileCount => value = listing.file_count as f64,
            Kind::FolderCount => value = listing.folder_count as f64,
            Kind::FolderSize => value = listing.folder_size as f64,
            _ => {
                if let Some(f) = &entry {
                    let parent_path = || if f.name == ".." { path.clone() } else { f.dir.clone() };
                    match self.kind {
                        Kind::FileName => text = f.name.clone(),
                        Kind::FileType => text = f.ext.clone(),
                        Kind::FileSize => value = f.size as f64,
                        Kind::FileDate => {
                            let t = match self.date_type {
                                DateType::Modified => f.modified,
                                DateType::Created => f.created,
                                DateType::Accessed => f.accessed,
                            };
                            value = file_time(t);
                            text = format_local(t);
                        }
                        Kind::FilePath => text = if f.name == ".." { path.clone() } else { format!("{}{}", f.dir, f.name) },
                        Kind::PathToFile => text = parent_path(),
                        _ => {}
                    }
                    if self.kind == Kind::FileSize && f.is_folder {
                        text.clear();
                        self.string = Some(String::new());
                    }
                }
            }
        }
        if !matches!(self.kind, Kind::FileCount | Kind::FolderCount | Kind::FolderSize | Kind::FileSize) {
            self.string = Some(text);
        }
        value
    }

    fn string_value(&self) -> Option<String> {
        self.string.clone()
    }

    fn command(&mut self, args: &str) {
        let Some(p) = self.parent.clone() else { return };
        let cmd = args.trim();
        let upper = cmd.to_ascii_uppercase();
        let (word, rest) = upper.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((upper.as_str(), ""));
        let listing = p.bg.get();
        let Ok(mut st) = p.st.lock() else { return };
        if self.owner {
            let len = listing.files.len();
            let count = st.count;
            match word {
                "PAGEUP" if len > count => st.offset = st.offset.saturating_sub(count),
                "PAGEDOWN" if len > count => {
                    st.offset = if st.offset + 2 * count < len { st.offset + count } else { len - count };
                }
                "INDEXUP" if len > count => {
                    let shift = if rest.is_empty() { 1 } else { rest.parse::<usize>().unwrap_or(1).max(1) };
                    st.offset = st.offset.saturating_sub(shift);
                }
                "INDEXDOWN" if len > count => {
                    let shift = if rest.is_empty() { 1 } else { rest.parse::<usize>().unwrap_or(1).max(1) };
                    st.offset = (st.offset + shift).min(len - count);
                }
                "UPDATE" => {
                    st.offset = 0;
                    st.needs_update = true;
                }
                "PREVIOUSFOLDER" if st.opts.recursive != 2 => {
                    st.path = parent_dir(&st.path);
                    st.offset = 0;
                    st.needs_update = true;
                }
                "CONTEXTMENU" | "PROPERTIES" | "PAGEUP" | "PAGEDOWN" | "INDEXUP" | "INDEXDOWN" | "PREVIOUSFOLDER" => {}
                _ => self.log.push(format!("FileView: Unknown command: {cmd}")),
            }
            return;
        }
        let i = self.true_index(&st);
        let Some(f) = listing.files.get(i).cloned() else {
            self.log.push(format!("FileView: Unknown command: {cmd}"));
            return;
        };
        let full = if f.name == ".." { st.path.clone() } else { format!("{}{}", f.dir, f.name) };
        match word {
            "OPEN" => self.actions.push(format!("\"{full}\"")),
            "FOLLOWPATH" if st.opts.recursive != 2 => {
                if f.name == ".." {
                    st.path = parent_dir(&st.path);
                    st.offset = 0;
                    st.needs_update = true;
                } else if f.is_folder {
                    st.path = with_slash(&full);
                    st.offset = 0;
                    st.needs_update = true;
                } else {
                    self.actions.push(format!("\"{full}\""));
                }
            }
            "CONTEXTMENU" | "PROPERTIES" => {}
            _ => self.log.push(format!("FileView: Unknown command: {cmd}")),
        }
    }

    fn take_actions(&mut self) -> Vec<String> {
        let mut out = std::mem::take(&mut self.actions);
        if self.owner {
            if let Some(p) = &self.parent {
                if p.finished.swap(false, Ordering::SeqCst) {
                    if let Ok(st) = p.st.lock() {
                        if !st.finish_action.is_empty() {
                            out.push(st.finish_action.clone());
                        }
                    }
                }
            }
        }
        out
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
        std::fs::create_dir_all(r.join("zdir")).unwrap();
        std::fs::create_dir_all(r.join("adir")).unwrap();
        std::fs::write(r.join("b.txt"), "12345").unwrap();
        std::fs::write(r.join("a.log"), "1").unwrap();
        std::fs::write(r.join(".dot"), "").unwrap();
        std::fs::write(r.join("adir/inner.txt"), "123").unwrap();
        d
    }

    fn names(l: &Listing) -> Vec<&str> {
        l.files.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.TXT", "a.txt") && wildcard_match("a?c", "abc") && !wildcard_match("*.log", "a.txt"));
        assert!(wildcard_match("*", "anything"));
    }

    #[test]
    fn listing_sorts_folders_first_and_filters() {
        let d = tree();
        let path = format!("{}/", d.path().display());
        let l = list(&path, &Options::default());
        assert_eq!(names(&l), ["..", "adir", "zdir", ".dot", "a.log", "b.txt"]);
        assert_eq!((l.file_count, l.folder_count, l.folder_size), (3, 2, 6));
        let o = Options { hidden: false, dot_dot: false, files: false, ..Default::default() };
        assert_eq!(names(&list(&path, &o)), ["adir", "zdir"]);
        let o = Options { sort: SortType::Size, ascending: false, dot_dot: false, ..Default::default() };
        assert_eq!(names(&list(&path, &o)), ["zdir", "adir", "b.txt", "a.log", ".dot"]);
        let o = Options { extensions: vec!["txt".into()], dot_dot: false, ..Default::default() };
        assert_eq!(names(&list(&path, &o)), ["adir", "zdir", "b.txt"]);
        let o = Options { extensions: vec!["txt".into()], exclude_ext: true, dot_dot: false, ..Default::default() };
        assert_eq!(names(&list(&path, &o)), ["adir", "zdir", ".dot", "a.log"]);
        let o = Options { recursive: 2, ..Default::default() };
        let l = list(&path, &o);
        assert_eq!(names(&l), [".dot", "a.log", "b.txt", "inner.txt"]);
        let o = Options { recursive: 1, dot_dot: false, ..Default::default() };
        let l = list(&path, &o);
        assert_eq!(names(&l), ["adir", "zdir", ".dot", "a.log", "b.txt"]);
        assert_eq!(l.file_count, 4);
        assert_eq!(parent_dir("/a/b/"), "/a/");
        assert_eq!(parent_dir("/a/"), "/");
    }

    #[test]
    fn measures_page_follow_and_open() {
        let d = tree();
        let ini = format!(
            "[FvP]\nMeasure=Plugin\nPlugin=FileView\nPath={}\nCount=2\nShowDotDot=0\nType=FolderPath\n\
             [FvN0]\nMeasure=Plugin\nPlugin=FileView\nPath=[FvP]\nType=FileName\nIndex=1\n\
             [FvN1]\nMeasure=Plugin\nPlugin=FileView\nPath=[FvP]\nType=FileName\nIndex=2\n\
             [FvS]\nMeasure=Plugin\nPlugin=FileView\nPath=[FvP]\nType=FileSize\nIndex=1\n\
             [FvT]\nMeasure=Plugin\nPlugin=FileView\nPath=[FvP]\nType=FileCount\n",
            d.path().display()
        );
        let sk = skin(&ini);
        let mk = |sec: &str| {
            let mut m = FileView::new();
            m.read_options(&OptionReader { parser: &sk.parser, section: sec, sv: None });
            m
        };
        let (mut p, mut n0, mut n1, mut sz, mut tot) = (mk("FvP"), mk("FvN0"), mk("FvN1"), mk("FvS"), mk("FvT"));
        let upd = |p: &mut FileView, n0: &mut FileView, n1: &mut FileView, sz: &mut FileView, tot: &mut FileView| -> f64 {
            ctx_run(&sk, |c| {
                p.update(c);
                n0.update(c);
                n1.update(c);
                sz.update(c);
                tot.update(c)
            })
        };
        assert_eq!(upd(&mut p, &mut n0, &mut n1, &mut sz, &mut tot), 3.0);
        assert_eq!(p.string_value().as_deref(), Some(format!("{}/", d.path().display()).as_str()));
        assert_eq!((n0.string_value().as_deref(), n1.string_value().as_deref()), (Some("adir"), Some("zdir")));
        // Page down shows entries 3 and 4.
        p.command("PageDown");
        upd(&mut p, &mut n0, &mut n1, &mut sz, &mut tot);
        assert_eq!((n0.string_value().as_deref(), n1.string_value().as_deref()), (Some(".dot"), Some("a.log")));
        // Sizes are numbers: ".dot" is empty.
        assert_eq!(sz.string_value(), None);
        n1.command("Open");
        let acts = n1.take_actions();
        assert_eq!(acts, vec![format!("\"{}/a.log\"", d.path().display())]);
        p.command("PageUp");
        upd(&mut p, &mut n0, &mut n1, &mut sz, &mut tot);
        // Follow into the first folder, then back up.
        n0.command("FollowPath");
        upd(&mut p, &mut n0, &mut n1, &mut sz, &mut tot);
        assert!(p.string_value().unwrap().ends_with("/adir/"));
        assert_eq!(n0.string_value().as_deref(), Some("inner.txt"));
        p.command("PreviousFolder");
        upd(&mut p, &mut n0, &mut n1, &mut sz, &mut tot);
        assert_eq!(n0.string_value().as_deref(), Some("adir"));
        // Bad commands only log.
        n0.command("Bogus");
        assert_eq!(n0.take_log().len(), 1);
        let mut orphan = FileView::new();
        orphan.command("Open");
    }
}
