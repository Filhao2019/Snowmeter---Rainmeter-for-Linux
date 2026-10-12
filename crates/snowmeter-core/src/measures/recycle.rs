//! RecycleManager on the XDG Trash (`Count`, `Size`, `OpenBin`, `EmptyBin`).

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::path::{Path, PathBuf};

/// `$XDG_DATA_HOME/Trash/files` (default `~/.local/share/Trash/files`).
pub fn default_trash_files() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("Trash/files")
}

fn tree_size(p: &Path) -> u64 {
    let Ok(md) = std::fs::symlink_metadata(p) else { return 0 };
    if md.is_dir() {
        std::fs::read_dir(p).map(|rd| rd.filter_map(|e| e.ok()).map(|e| tree_size(&e.path())).sum()).unwrap_or(0)
    } else {
        md.len()
    }
}

/// `(item count, total bytes)` of a trash `files` directory.
pub fn trash_stats(files: &Path) -> (u64, u64) {
    let Ok(rd) = std::fs::read_dir(files) else { return (0, 0) };
    let mut count = 0;
    let mut size = 0;
    for e in rd.filter_map(|e| e.ok()) {
        count += 1;
        size += tree_size(&e.path());
    }
    (count, size)
}

/// Internal action strings turned into `AppRequest::EmptyTrash` by the skin.
pub const EMPTY_CONFIRM: &str = "snowmeter:empty-trash";
pub const EMPTY_SILENT: &str = "snowmeter:empty-trash-silent";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Count,
    Size,
}

pub struct RecycleMeasure {
    kind: Kind,
    files: PathBuf,
    actions: Vec<String>,
    log: Vec<String>,
}

impl Default for RecycleMeasure {
    fn default() -> Self {
        RecycleMeasure::new()
    }
}

impl RecycleMeasure {
    pub fn new() -> RecycleMeasure {
        RecycleMeasure::with_dir(default_trash_files())
    }

    pub fn with_dir(files: PathBuf) -> RecycleMeasure {
        RecycleMeasure { kind: Kind::Count, files, actions: Vec::new(), log: Vec::new() }
    }
}

impl MeasureImpl for RecycleMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let t = opt.string("RecycleType", "COUNT");
        self.kind = match t.trim().to_ascii_uppercase().as_str() {
            "SIZE" => Kind::Size,
            "COUNT" | "" => Kind::Count,
            other => {
                self.log.push(format!("RecycleManager: RecycleType={other} is not valid"));
                Kind::Count
            }
        };
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let (count, size) = trash_stats(&self.files);
        match self.kind {
            Kind::Count => count as f64,
            Kind::Size => size as f64,
        }
    }

    fn command(&mut self, args: &str) {
        match args.trim().to_ascii_lowercase().as_str() {
            "openbin" => self.actions.push("trash:///".into()),
            // The front end asks for confirmation (`EmptyBin`) or not
            // (`EmptyBinSilent`) and then empties the trash.
            "emptybin" => self.actions.push(EMPTY_CONFIRM.into()),
            "emptybinsilent" => self.actions.push(EMPTY_SILENT.into()),
            other => self.log.push(format!("RecycleManager: unknown command '{other}'")),
        }
    }

    fn take_actions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.actions)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_items_and_sums_sizes() {
        let t = tempfile::tempdir().unwrap();
        let files = t.path().join("files");
        std::fs::create_dir_all(files.join("dir")).unwrap();
        std::fs::write(files.join("a.txt"), "12345").unwrap();
        std::fs::write(files.join("dir/b.txt"), "123").unwrap();
        assert_eq!(trash_stats(&files), (2, 8));
        assert_eq!(trash_stats(&t.path().join("missing")), (0, 0));
    }

    #[test]
    fn open_bin_queues_a_run_action() {
        let mut m = RecycleMeasure::with_dir(PathBuf::from("/nonexistent"));
        m.command("OpenBin");
        assert_eq!(m.take_actions(), vec!["trash:///".to_string()]);
        assert!(m.take_log().is_empty());
        m.command("Nonsense");
        assert_eq!(m.take_log().len(), 1);
    }

    #[test]
    fn empty_bin_asks_first_and_silent_does_not() {
        let mut m = RecycleMeasure::with_dir(PathBuf::from("/nonexistent"));
        m.command("EmptyBin");
        assert_eq!(m.take_actions(), vec![EMPTY_CONFIRM.to_string()]);
        m.command("EmptyBinSilent");
        assert_eq!(m.take_actions(), vec![EMPTY_SILENT.to_string()]);
    }

    #[test]
    fn size_type_reads_option() {
        let t = tempfile::tempdir().unwrap();
        let files = t.path().join("files");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("a"), "1234567").unwrap();
        let s = crate::measures::testutil::skin("[R]\nMeasure=RecycleManager\nRecycleType=Size\n");
        let mut m = RecycleMeasure::with_dir(files);
        m.read_options(&OptionReader::new(&s.parser, "R", None));
        assert_eq!(crate::measures::testutil::ctx_run(&s, |c| m.update(c)), 7.0);
    }
}
