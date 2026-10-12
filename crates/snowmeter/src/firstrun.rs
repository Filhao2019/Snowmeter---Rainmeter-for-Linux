//! Start-up: copy bundled skin roots into the skins folder and, on the first start, activate the defaults.

use snowmeter_core::env::Environment;
use snowmeter_core::gallery;
use snowmeter_core::registry::SkinRegistry;
use snowmeter_core::rmskin::activate_skin;
use snowmeter_core::settings::Settings;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIGS: [&str; 3] = ["Snowmeter\\Weekday", "Snowmeter\\System", "Snowmeter\\Disk"];
/// Where the default widgets open on the first start: a column near the
/// top-left corner instead of all three on top of each other.
const DEFAULT_POSITIONS: [(i32, i32); 3] = [(40, 40), (40, 240), (40, 410)];

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

/// The bundled skins directory for this process, if any.
pub fn bundled_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    find_bundled(std::env::var_os("SNOWMETER_BUNDLED_SKINS").map(PathBuf::from), exe_dir.as_deref())
}

/// Every start: copy bundled `folder` roots this user was never offered (see
/// `gallery::sync_bundled`). On the first start (`FirstRunDone` missing) also
/// activate the default widgets. Returns true if it was the first run.
pub fn run(env: &Environment, settings: &mut Settings) -> bool {
    let first = !settings.first_run_done();
    let mut changed = false;
    match bundled_dir() {
        Some(dir) => {
            let copied = gallery::sync_bundled(env, settings, &dir);
            changed = !copied.is_empty();
            if !copied.is_empty() {
                log::info!("copied bundled skins: {}", copied.join(", "));
            }
        }
        None if first => log::info!("first run: no bundled skins found"),
        None => {}
    }
    if first {
        let reg = SkinRegistry::scan(env);
        let mut activated = 0;
        for (name, (x, y)) in DEFAULT_CONFIGS.iter().zip(DEFAULT_POSITIONS) {
            if let Some(c) = reg.config(name) {
                if let Some(file) = c.files.first().cloned() {
                    match activate_skin(env, settings, name, &file) {
                        Ok((config, _)) => {
                            settings.set_config_key(&config, "WindowX", &x.to_string());
                            settings.set_config_key(&config, "WindowY", &y.to_string());
                            activated += 1;
                        }
                        Err(e) => log::warn!("first run: cannot activate {name}: {e}"),
                    }
                }
            }
        }
        // Without the bundled skins nothing could be set up: try again next start.
        if activated > 0 {
            settings.set_first_run_done(true);
            changed = true;
        }
    }
    if changed {
        if let Err(e) = settings.save() {
            log::warn!("cannot save settings: {e}");
        }
    }
    first
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
        assert_eq!(s.global("BundledInstalled"), Some("Snowmeter"));
        assert!(env.skins_dir.join("Snowmeter/Other/Other.ini").is_file());
        let mut s = s;
        assert!(!run(&env, &mut s));
    }
}
