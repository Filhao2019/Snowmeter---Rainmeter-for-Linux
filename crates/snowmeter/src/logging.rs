//! env_logger to stderr, plus `~/.local/state/snowmeter/snowmeter.log` for the app.

use std::io::Write;
use std::path::PathBuf;

struct Tee {
    file: Option<std::fs::File>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(f) = self.file.as_mut() {
            let _ = f.write_all(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        if let Some(f) = self.file.as_mut() {
            let _ = f.flush();
        }
        Ok(())
    }
}

pub fn log_file_path() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(state.join("snowmeter").join("snowmeter.log"))
}

pub fn init(verbose: bool, to_file: bool) {
    let mut b = env_logger::Builder::new();
    b.filter_level(if verbose { log::LevelFilter::Debug } else { log::LevelFilter::Info });
    b.parse_default_env();
    if to_file {
        let file = log_file_path().and_then(|p| {
            std::fs::create_dir_all(p.parent()?).ok()?;
            std::fs::OpenOptions::new().create(true).append(true).open(p).ok()
        });
        b.target(env_logger::Target::Pipe(Box::new(Tee { file })));
    }
    let _ = b.try_init();
}
