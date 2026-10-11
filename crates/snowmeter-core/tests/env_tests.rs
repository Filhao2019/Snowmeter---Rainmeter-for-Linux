use snowmeter_core::env::{Environment, Rect};

#[test]
fn with_root_layout() {
    let dir = tempfile::tempdir().unwrap();
    let e = Environment::with_root(dir.path());
    assert_eq!(e.skins_dir, dir.path().join("skins"));
    assert_eq!(e.settings_dir, dir.path());
    assert_eq!(e.settings_file, dir.path().join("snowmeter.ini"));
    assert!(e.program_dir.starts_with(dir.path()));
    assert!(e.fonts_install_dir.starts_with(dir.path()));
    assert_eq!(e.monitors, vec![Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 }]);
    assert_eq!(e.work_areas, e.monitors);
}

// All env-var driven checks in one test so they cannot race each other.
#[test]
fn from_env_honours_overrides() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("SNOWMETER_CONFIG_DIR", dir.path());
    let e = Environment::from_env();
    assert_eq!(e.skins_dir, dir.path().join("skins"));
    assert_eq!(e.settings_file, dir.path().join("snowmeter.ini"));
    std::env::remove_var("SNOWMETER_CONFIG_DIR");

    let xdg = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", xdg.path());
    let e = Environment::from_env();
    assert_eq!(e.settings_dir, xdg.path().join("snowmeter"));
    assert_eq!(e.skins_dir, xdg.path().join("snowmeter").join("skins"));
    std::env::remove_var("XDG_CONFIG_HOME");
}
