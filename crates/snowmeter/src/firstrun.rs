//! First start: copy the bundled skins into the skins folder and activate the defaults.

use snowmeter_core::env::Environment;
use snowmeter_core::registry::SkinRegistry;
use snowmeter_core::rmskin::activate_skin;
use snowmeter_core::settings::Settings;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIGS: [&str; 3] = ["Snowmeter\\Weekday", "Snowmeter\\System", "Snowmeter\\Disk"];

/// First existing of `$SNOWMETER_BUNDLED_SKINS`, `<exe_dir>/../share/snowmeter/skins`,
/// `<exe_dir>/../../skins` (the repo's `skins/` during development).
pub fn find_bundled(env_override: Option<PathBuf>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = env_override.filter(|p| !p.as_os_str().is_empty()) {
        candidates.push(p);
    }
    if let Some(d) = exe_dir {
        candidates.push(d.join("../share/snowmeter/skins"));
        candidates.push(d.join("../../skins"));
    }
    candidates.into_iter().find(|p| p.is_dir())
}

/// Recursively copy `src` into `dst`, never replacing an existing file.
/// Returns the number of files copied.
pub fn copy_no_overwrite(src: &Path, dst: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut n = 0;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        let ft = e.file_type()?;
        if ft.is_dir() {
            n += copy_no_overwrite(&e.path(), &to)?;
        } else if ft.is_file() && !to.exists() {
            std::fs::copy(e.path(), &to)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Run the first-run setup when `FirstRunDone` is missing. Returns true if it ran.
pub fn run(env: &Environment, settings: &mut Settings) -> bool {
    if settings.first_run_done() {
        return false;
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let bundled = find_bundled(std::env::var_os("SNOWMETER_BUNDLED_SKINS").map(PathBuf::from), exe_dir.as_deref());
    match bundled {
        Some(dir) => match copy_no_overwrite(&dir, &env.skins_dir) {
            Ok(n) => log::info!("first run: copied {n} bundled skin files from {}", dir.display()),
            Err(e) => log::warn!("first run: copying bundled skins from {} failed: {e}", dir.display()),
        },
        None => log::info!("first run: no bundled skins found"),
    }
    let reg = SkinRegistry::scan(env);
    for name in DEFAULT_CONFIGS {
        if let Some(c) = reg.config(name) {
            if let Some(file) = c.files.first().cloned() {
                if let Err(e) = activate_skin(env, settings, name, &file) {
                    log::warn!("first run: cannot activate {name}: {e}");
                }
            }
        }
    }
    settings.set_first_run_done(true);
    if let Err(e) = settings.save() {
        log::warn!("first run: cannot save settings: {e}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn bundled_lookup_order() {
        let t = tempfile::tempdir().unwrap();
        let exe = t.path().join("a/b");
        std::fs::create_dir_all(&exe).unwrap();
        assert_eq!(find_bundled(None, Some(&exe)), None);
        std::fs::create_dir_all(t.path().join("skins")).unwrap();
        assert!(find_bundled(None, Some(&exe)).unwrap().ends_with("../../skins"));
        std::fs::create_dir_all(t.path().join("a/share/snowmeter/skins")).unwrap();
        assert!(find_bundled(None, Some(&exe)).unwrap().ends_with("../share/snowmeter/skins"));
        let o = t.path().join("over");
        std::fs::create_dir_all(&o).unwrap();
        assert_eq!(find_bundled(Some(o.clone()), Some(&exe)), Some(o));
        assert_eq!(find_bundled(Some(t.path().join("missing")), None), None);
    }

    #[test]
    fn copy_keeps_existing_files() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("src/Snowmeter/System/System.ini"), "new");
        write(&t.path().join("src/Snowmeter/System/@Resources/x.txt"), "x");
        write(&t.path().join("dst/Snowmeter/System/System.ini"), "mine");
        let n = copy_no_overwrite(&t.path().join("src"), &t.path().join("dst")).unwrap();
        assert_eq!(n, 1);
        assert_eq!(std::fs::read_to_string(t.path().join("dst/Snowmeter/System/System.ini")).unwrap(), "mine");
        assert!(t.path().join("dst/Snowmeter/System/@Resources/x.txt").exists());
    }

    #[test]
    fn first_run_activates_defaults() {
        let t = tempfile::tempdir().unwrap();
        let env = Environment::with_root(&t.path().join("cfg"));
        write(&t.path().join("bundle/Snowmeter/System/System.ini"), "[Rainmeter]\n[M]\nMeter=String\n");
        write(&t.path().join("bundle/Snowmeter/Other/Other.ini"), "[Rainmeter]\n[M]\nMeter=String\n");
        std::env::set_var("SNOWMETER_BUNDLED_SKINS", t.path().join("bundle"));
        let mut s = Settings::load(&env);
        assert!(run(&env, &mut s));
        std::env::remove_var("SNOWMETER_BUNDLED_SKINS");
        let s = Settings::load(&env);
        assert!(s.first_run_done());
        let active: Vec<String> = s.active_configs().into_iter().map(|(n, _)| n).collect();
        assert_eq!(active, vec!["Snowmeter\\System".to_string()]);
        let mut s = s;
        assert!(!run(&env, &mut s));
    }
}
