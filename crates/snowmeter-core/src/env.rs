//! Locations and monitor geometry shared by every skin.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug)]
pub struct Environment {
    pub skins_dir: PathBuf,
    pub settings_dir: PathBuf,
    pub settings_file: PathBuf,
    pub program_dir: PathBuf,
    pub fonts_install_dir: PathBuf,
    /// Where gallery downloads are cached.
    pub gallery_cache_dir: PathBuf,
    /// Virtual-desktop coordinates, index 0 = primary.
    pub monitors: Vec<Rect>,
    pub work_areas: Vec<Rect>,
}

fn nonempty_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn default_monitors() -> Vec<Rect> {
    vec![Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 }]
}

impl Environment {
    /// Layout under `dir`: `skins/`, `snowmeter.ini`, `program/`, `fonts/`.
    /// Nothing outside `dir` is referenced, so tests stay out of the real home.
    pub fn with_root(dir: &Path) -> Environment {
        // Absolute, so `#@#`-style expanded paths are not re-joined to the skin dir.
        let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        let dir = abs.as_path();
        Environment {
            skins_dir: dir.join("skins"),
            settings_dir: dir.to_path_buf(),
            settings_file: dir.join("snowmeter.ini"),
            program_dir: dir.join("program"),
            fonts_install_dir: dir.join("fonts"),
            gallery_cache_dir: dir.join("cache").join("gallery"),
            monitors: default_monitors(),
            work_areas: default_monitors(),
        }
    }

    /// `SNOWMETER_CONFIG_DIR`, else `$XDG_CONFIG_HOME/snowmeter`, else
    /// `~/.config/snowmeter`. Monitors default to one 1920x1080 rect until the
    /// GTK application fills in the real ones.
    pub fn from_env() -> Environment {
        let home = nonempty_var("HOME").unwrap_or_else(|| PathBuf::from("/tmp"));
        let (root, overridden) = if let Some(dir) = nonempty_var("SNOWMETER_CONFIG_DIR") {
            (dir, true)
        } else if let Some(x) = nonempty_var("XDG_CONFIG_HOME") {
            (x.join("snowmeter"), false)
        } else {
            (home.join(".config").join("snowmeter"), false)
        };
        let mut env = Environment::with_root(&root);
        if !overridden {
            let data = nonempty_var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));
            env.fonts_install_dir = data.join("fonts").join("snowmeter");
            let cache = nonempty_var("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
            env.gallery_cache_dir = cache.join("snowmeter").join("gallery");
        }
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
            env.program_dir = dir;
        }
        env
    }
}
