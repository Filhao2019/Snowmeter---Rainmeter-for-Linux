//! QuotePlugin: a random item of `PathName` (a text file split on
//! `Separator`, default newline) or a random file of a folder (`FileFilter`,
//! `Subfolders`). A new pick is made on every update, using the selection rule
//! of MeasureQuote.cpp (random offset, then the item after the next separator).

use super::fileview::wildcard_match;
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_FILES: usize = 20_000;
const MAX_DEPTH: usize = 12;

/// Random number in `0..=max`.
pub type Rng = Box<dyn FnMut(usize) -> usize>;

/// Time-seeded xorshift; good enough for picking quotes.
pub fn default_rng() -> Rng {
    let mut x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9E37_79B9) | 1;
    x ^= (std::process::id() as u64) << 32;
    Box::new(move |max| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) % (max as u64 + 1)) as usize
    })
}

/// Item selection of the C++ plugin: `position` is the random offset in `0..text.len()`.
pub fn pick_item(text: &str, sep: &str, position: usize) -> String {
    let mut pos = position.min(text.len().saturating_sub(1));
    while !text.is_char_boundary(pos) {
        pos -= 1;
    }
    let start = match text[pos..].find(sep) {
        Some(i) => pos + i + sep.len(),
        None => 0,
    };
    let end = text[start..].find(sep).map(|i| start + i).unwrap_or(text.len());
    text[start..end].trim_end_matches('\r').to_string()
}

fn scan(dir: &Path, filters: &[String], subfolders: bool, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if out.len() >= MAX_FILES {
            return;
        }
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if subfolders && depth < MAX_DEPTH {
                scan(&p, filters, subfolders, depth + 1, out);
            }
        } else if filters.is_empty() || filters.iter().any(|f| wildcard_match(&f.to_ascii_lowercase(), &name.to_ascii_lowercase())) {
            out.push(p);
        }
    }
}

struct Opts {
    path: String,
    filter: String,
    subfolders: bool,
    separator: String,
}

pub struct Quote {
    opts: Opts,
    dirty: bool,
    path: PathBuf,
    separator: String,
    files: Vec<PathBuf>,
    is_dir: bool,
    text: Option<String>,
    stamp: Option<(SystemTime, u64)>,
    value: String,
    rng: Rng,
}

impl Quote {
    pub fn new() -> Quote {
        Quote::with_rng(default_rng())
    }

    pub fn with_rng(rng: Rng) -> Quote {
        Quote {
            opts: Opts { path: String::new(), filter: String::new(), subfolders: true, separator: "\n".into() },
            dirty: true,
            path: PathBuf::new(),
            separator: "\n".into(),
            files: Vec::new(),
            is_dir: false,
            text: None,
            stamp: None,
            value: String::new(),
            rng,
        }
    }
}

impl Default for Quote {
    fn default() -> Self {
        Quote::new()
    }
}

impl MeasureImpl for Quote {
    fn read_options(&mut self, opt: &OptionReader) {
        self.opts = Opts {
            path: opt.string("PathName", ""),
            filter: opt.string("FileFilter", ""),
            subfolders: opt.int("Subfolders", 1) == 1,
            separator: opt.string("Separator", "\n"),
        };
        self.dirty = true;
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        if self.dirty {
            self.configure(ctx.skin_dir);
            self.dirty = false;
        }
        if self.is_dir && !self.files.is_empty() {
            let i = (self.rng)(self.files.len() - 1);
            self.value = self.files[i].to_string_lossy().into_owned();
            return 0.0;
        }
        let Ok(meta) = std::fs::metadata(&self.path) else {
            self.text = None;
            return 0.0;
        };
        let stamp = (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len());
        if self.text.is_none() || self.stamp != Some(stamp) {
            match std::fs::read(&self.path) {
                Ok(b) => {
                    self.text = Some(crate::ini::decode_text(&b));
                    self.stamp = Some(stamp);
                }
                Err(_) => {
                    self.text = None;
                    return 0.0;
                }
            }
        }
        if let Some(t) = self.text.as_ref().filter(|t| !t.is_empty()) {
            let pos = (self.rng)(t.len() - 1);
            self.value = pick_item(t, &self.separator, pos);
        }
        0.0
    }

    fn string_value(&self) -> Option<String> {
        Some(self.value.clone())
    }
}

impl Quote {
    /// Resolve the path against the skin folder and scan a folder PathName.
    fn configure(&mut self, base: &Path) {
        let path = crate::paths::resolve_path(base, &self.opts.path);
        if path != self.path {
            self.text = None;
            self.stamp = None;
        }
        self.path = path;
        self.files.clear();
        self.is_dir = self.path.is_dir();
        if self.is_dir {
            let filters: Vec<String> = self.opts.filter.split(';').filter(|f| !f.is_empty()).map(str::to_string).collect();
            scan(&self.path, &filters, self.opts.subfolders, 0, &mut self.files);
        } else {
            self.separator = if self.opts.separator.is_empty() { "\n".into() } else { self.opts.separator.clone() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    #[test]
    fn item_selection_follows_the_plugin() {
        let t = "alpha\nbeta\ngamma";
        // offset inside "alpha": next separator at 5 -> "beta"
        assert_eq!(pick_item(t, "\n", 0), "beta");
        assert_eq!(pick_item(t, "\n", 6), "gamma");
        // no separator after the offset: first item
        assert_eq!(pick_item(t, "\n", 12), "alpha");
        assert_eq!(pick_item("a%%b%%c", "%%", 1), "b");
        assert_eq!(pick_item("one", "\n", 0), "one");
        assert_eq!(pick_item("l1\r\nl2\r\nl3", "\n", 0), "l2");
        assert_eq!(pick_item("", "\n", 0), "");
        // never splits a UTF-8 character
        assert_eq!(pick_item("\u{e9}\u{e9}\nx\ny", "\n", 1), "x");
    }

    fn seq(v: Vec<usize>) -> Rng {
        let mut i = 0;
        Box::new(move |max| {
            let r = v[i % v.len()].min(max);
            i += 1;
            r
        })
    }

    fn run(m: &mut Quote, sk: &crate::measures::testutil::TestSkin) -> String {
        ctx_run(sk, |c| m.update(c));
        m.string_value().unwrap()
    }

    #[test]
    fn file_mode_rerolls_each_update_and_follows_changes() {
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=QuotePlugin\nPathName=q.txt\nSeparator=%%\n");
        let f = sk.env.skins_dir.join("T/q.txt");
        std::fs::write(&f, "one%%two%%three").unwrap();
        let mut m = Quote::with_rng(seq(vec![0, 5, 9]));
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        assert_eq!(run(&mut m, &sk), "two");
        assert_eq!(run(&mut m, &sk), "three");
        assert_eq!(run(&mut m, &sk), "one");
        std::fs::write(&f, "x%%yy%%zzz").unwrap();
        assert_eq!(run(&mut m, &sk), "yy");
        std::fs::remove_file(&f).unwrap();
        assert_eq!(run(&mut m, &sk), "yy"); // keeps the last value, like the original
    }

    #[test]
    fn folder_mode_filters_and_recurses() {
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=QuotePlugin\nPathName=quotes\nFileFilter=*.txt;*.md\n[N]\nMeasure=Plugin\nPathName=quotes\nFileFilter=*.txt\nSubfolders=0\n");
        let d = sk.env.skins_dir.join("T/quotes");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        for p in ["a.txt", "b.png", "c.MD", "sub/d.txt"] {
            std::fs::write(d.join(p), "x").unwrap();
        }
        let mut m = Quote::with_rng(seq(vec![0, 1, 2]));
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        let got: Vec<String> = (0..3).map(|_| run(&mut m, &sk).rsplit('/').next().unwrap().to_string()).collect();
        assert_eq!(got, ["a.txt", "c.MD", "d.txt"]);
        let mut n = Quote::with_rng(seq(vec![5]));
        n.read_options(&OptionReader { parser: &sk.parser, section: "N", sv: None });
        assert!(run(&mut n, &sk).ends_with("quotes/a.txt"));
        // missing path
        let mut z = Quote::new();
        assert_eq!(run(&mut z, &sk), "");
    }

    #[test]
    fn default_rng_stays_in_range() {
        let mut r = default_rng();
        assert!((0..1000).all(|_| r(6) <= 6));
        assert_eq!(r(0), 0);
        let seen: std::collections::HashSet<usize> = (0..200).map(|_| r(3)).collect();
        assert_eq!(seen.len(), 4);
    }
}
