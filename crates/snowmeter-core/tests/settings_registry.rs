use snowmeter_core::env::Environment;
use snowmeter_core::ini::IniFile;
use snowmeter_core::registry::{ntfs_cmp, SkinRegistry};
use snowmeter_core::settings::{ConfigState, Settings, WindowType};
use std::fs;
use std::path::Path;

fn touch(p: &Path) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, "[Rainmeter]\n").unwrap();
}

#[test]
fn settings_round_trip_and_unknown_keys() {
    let d = tempfile::tempdir().unwrap();
    let env = Environment::with_root(d.path());
    fs::write(
        &env.settings_file,
        "[snowmeter]\nwindowtype=below\nFutureKey=keepme\n[illustro\\Clock]\nactive=2\nwindowx=(#SCREENAREAWIDTH#-200)R@2\nExtra=1\n",
    )
    .unwrap();
    let mut s = Settings::load(&env);
    assert_eq!(s.window_type(), WindowType::Below);
    assert!(!s.first_run_done());
    let c = s.config("ILLUSTRO\\clock").unwrap();
    assert_eq!(c.active, 2);
    assert_eq!(c.window_x, "(#SCREENAREAWIDTH#-200)R@2");
    assert!(c.draggable && c.keep_on_screen && c.save_position && !c.click_through);
    assert_eq!(c.alpha_value, 255);

    let mut n = ConfigState::default();
    n.active = 1;
    n.window_x = "50.00000%".into();
    n.click_through = true;
    n.alpha_value = 128;
    n.load_order = 3;
    n.always_on_top = -2;
    s.set_config("illustro\\Clock", &n);
    s.set_first_run_done(true);
    s.set_window_type(WindowType::Normal);
    s.save().unwrap();

    let s2 = Settings::load(&env);
    assert_eq!(s2.config("illustro\\Clock").unwrap(), n);
    assert!(s2.first_run_done());
    assert_eq!(s2.window_type(), WindowType::Normal);
    assert_eq!(s2.global("FutureKey"), Some("keepme"));
    assert_eq!(s2.config_key("illustro\\Clock", "Extra"), Some("1"));
    assert!(s2.config("Snowmeter").is_none());
    assert_eq!(s2.config_names(), vec!["illustro\\Clock".to_string()]);
}

#[test]
fn missing_file_is_empty_and_active_order() {
    let d = tempfile::tempdir().unwrap();
    let env = Environment::with_root(d.path());
    let mut s = Settings::load(&env);
    assert_eq!(s.window_type(), WindowType::Desktop);
    assert!(s.config("a\\b").is_none());
    s.set_active("B\\x", 1);
    s.set_active("A\\y", 2);
    s.set_config_key("B\\x", "LoadOrder", "5");
    s.set_active("C\\z", 0);
    assert_eq!(s.active_configs(), vec![("A\\y".to_string(), 2), ("B\\x".to_string(), 1)]);
    s.save().unwrap();
    assert!(env.settings_file.exists());
}

#[test]
fn layout_apply_deactivates_others() {
    let d = tempfile::tempdir().unwrap();
    let env = Environment::with_root(d.path());
    fs::write(&env.settings_file, "[Snowmeter]\nWindowType=desktop\n[Old\\A]\nActive=1\n[L\\A]\nActive=3\nWindowX=1\n").unwrap();
    let mut s = Settings::load(&env);
    let layout = IniFile::parse("[Rainmeter]\nSkinPath=/evil\n[L\\A]\nActive=1\nWindowX=10\n");
    s.apply_layout(&layout);
    assert_eq!(s.config("Old\\A").unwrap().active, 0);
    let a = s.config("L\\A").unwrap();
    assert_eq!((a.active, a.window_x.as_str()), (1, "10"));
    assert_eq!(s.global("SkinPath"), None);
    assert_eq!(s.window_type(), WindowType::Desktop);
}

#[test]
fn ntfs_order_uses_uppercase() {
    let mut v = vec!["A_B.ini", "AB.ini", "Clock.ini", "Clock 2.ini", "b_c.ini", "B.ini", "A.ini"];
    v.sort_by(|a, b| ntfs_cmp(a, b));
    assert_eq!(v, vec!["A.ini", "AB.ini", "A_B.ini", "B.ini", "b_c.ini", "Clock 2.ini", "Clock.ini"]);
}

#[test]
fn registry_scan_names_order_and_skips() {
    let d = tempfile::tempdir().unwrap();
    let env = Environment::with_root(d.path());
    let s = &env.skins_dir;
    touch(&s.join("illustro/Clock/Clock.ini"));
    touch(&s.join("illustro/Disk/2 Disks.ini"));
    touch(&s.join("illustro/Disk/1 Disk.ini"));
    touch(&s.join("illustro/Disk/notes.txt"));
    touch(&s.join("illustro/@Resources/Fake.ini"));
    touch(&s.join("illustro/Root.ini"));
    touch(&s.join("@Backup/old/x.ini"));
    touch(&s.join("Backup/old/x.ini"));
    touch(&s.join("@Vault/y.ini"));
    touch(&s.join("top.ini"));
    touch(&s.join("Empty/Sub/readme.txt"));
    touch(&s.join("Basic/Main/B.ini"));
    touch(&s.join("Basic/Main/A.ini"));
    touch(&s.join("Basic/Main/b_c.ini"));
    touch(&s.join("Only/Sub/S.ini"));

    let r = SkinRegistry::scan(&env);
    let names: Vec<&str> = r.configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Basic", "Basic\\Main", "illustro", "illustro\\Clock", "illustro\\Disk", "Only", "Only\\Sub"]
    );
    assert!(r.config("basic").unwrap().files.is_empty());
    assert_eq!(r.config("illustro").unwrap().files, vec!["Root.ini"]);
    assert_eq!(r.config("ILLUSTRO/disk").unwrap().files, vec!["1 Disk.ini", "2 Disks.ini"]);
    assert_eq!(r.file_index("illustro\\Disk", "2 disks.INI"), Some(2));
    assert_eq!(r.file_index("Basic\\Main", "B.ini"), Some(2));
    assert_eq!(r.file_at("Basic\\Main", 3), Some("b_c.ini"));
    assert_eq!(r.file_at("Basic\\Main", 4), None);
    assert_eq!(r.file_at("Basic\\Main", 0), None);
    assert!(r.ini_path("illustro\\Clock", "clock.ini").unwrap().ends_with("Clock/Clock.ini"));
    assert_eq!(r.loadable().count(), 5);
}
