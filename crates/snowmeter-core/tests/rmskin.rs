//! `.rmskin` importer tests. Fixtures follow `notes/rmskin-format.md` 7 and are
//! built at test time with the `zip` crate.

use snowmeter_core::env::Environment;
use snowmeter_core::ini::IniFile;
use snowmeter_core::rmskin::{import_path, import_path_with, ImportError, ImportOptions, ImportReport};
use snowmeter_core::settings::Settings;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

// ------------------------------------------------------------------ helpers

struct Zip {
    w: ZipWriter<Cursor<Vec<u8>>>,
}

impl Zip {
    fn new() -> Zip {
        Zip { w: ZipWriter::new(Cursor::new(Vec::new())) }
    }
    fn file(mut self, name: &str, data: impl AsRef<[u8]>) -> Zip {
        self.w.start_file(name, SimpleFileOptions::default()).unwrap();
        self.w.write_all(data.as_ref()).unwrap();
        self
    }
    fn symlink(mut self, name: &str, target: &str) -> Zip {
        self.w.add_symlink(name, target, SimpleFileOptions::default()).unwrap();
        self
    }
    fn bytes(self) -> Vec<u8> {
        self.w.finish().unwrap().into_inner()
    }
    /// ZIP + 16-byte footer: LE i64 size, flags, "RMSKIN\0".
    fn rmskin(self, flags: u8) -> Vec<u8> {
        footer(self.bytes(), flags)
    }
}

fn footer(mut zip: Vec<u8>, flags: u8) -> Vec<u8> {
    let n = zip.len() as i64;
    footer_with(&mut zip, n, flags, b"RMSKIN\0");
    zip
}

fn footer_with(zip: &mut Vec<u8>, size: i64, flags: u8, key: &[u8; 7]) {
    zip.extend_from_slice(&size.to_le_bytes());
    zip.push(flags);
    zip.extend_from_slice(key);
}

fn ini(extra: &str) -> String {
    format!("[rmskin]\nName=Test\nAuthor=me\nVersion=1.0\nMinimumRainmeter=4.5.0\n{extra}")
}

struct Ctx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    env: Environment,
}

fn ctx() -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let env = Environment::with_root(&root.join("env"));
    Ctx { _dir: dir, root, env }
}

impl Ctx {
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.root.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        p
    }
    fn import(&self, name: &str, bytes: &[u8]) -> Result<ImportReport, ImportError> {
        let p = self.write(name, bytes);
        import_path_with(&self.env, &p, &opts())
    }
    fn skin(&self, rel: &str) -> PathBuf {
        self.env.skins_dir.join(rel)
    }
    fn settings(&self) -> Settings {
        Settings::load(&self.env)
    }
}

fn opts() -> ImportOptions {
    ImportOptions { run_fc_cache: false, ..ImportOptions::default() }
}

fn basic_zip() -> Zip {
    Zip::new().file("RMSKIN.ini", ini("")).file("Skins/Basic/Main/Main.ini", "[Rainmeter]\nUpdate=1000\n")
}

fn names_in(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(p).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

// ------------------------------------------------------------------ fixtures

#[test]
fn f01_basic_with_footer() {
    let c = ctx();
    let r = c.import("basic.rmskin", &basic_zip().rmskin(0)).unwrap();
    assert!(c.skin("Basic/Main/Main.ini").is_file());
    assert_eq!(r.name, "Test");
    assert_eq!(r.author, "me");
    assert_eq!(r.installed_roots, vec!["Basic"]);
    assert!(r.configs.contains(&"Basic\\Main".to_string()));
    assert!(r.loaded_skin.is_none() && r.loaded_layout.is_none());
    assert!(c.settings().config("Basic\\Main").is_none());
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}

#[test]
fn f02_no_footer_installs_with_warning() {
    let c = ctx();
    for name in ["nofooter.zip", "nofooter.rmskin"] {
        let c2 = ctx();
        let r = c2.import(name, &basic_zip().bytes()).unwrap();
        assert!(c2.skin("Basic/Main/Main.ini").is_file());
        assert!(!r.warnings.is_empty());
    }
    drop(c);
}

#[test]
fn f03_bad_size_refused() {
    let c = ctx();
    let mut z = basic_zip().bytes();
    let n = z.len() as i64 - 1; // len-17 once the 16-byte footer is added
    footer_with(&mut z, n, 0, b"RMSKIN\0");
    let e = c.import("badsize.rmskin", &z).unwrap_err();
    assert!(matches!(e, ImportError::Corrupt(_)), "{e:?}");
    assert!(!c.env.skins_dir.exists());
}

#[test]
fn f04_bad_key_behaves_like_footerless() {
    let c = ctx();
    let mut z = basic_zip().bytes();
    let n = z.len() as i64;
    footer_with(&mut z, n, 0, b"RMSKIX\0");
    let r = c.import("badkey.rmskin", &z).unwrap();
    assert!(c.skin("Basic/Main/Main.ini").is_file());
    assert!(!r.warnings.is_empty());
}

#[test]
fn f05_nested_options_file() {
    let c = ctx();
    let z = || Zip::new().file("Pkg/RMSKIN.ini", ini("")).file("Pkg/Skins/Basic/Main/Main.ini", "[Rainmeter]\n");
    let e = c.import("nested.rmskin", &z().rmskin(0)).unwrap_err();
    assert!(matches!(e, ImportError::NotAPackage(_) | ImportError::Corrupt(_)), "{e:?}");
    assert!(!c.skin("Basic").exists());
    let r = c.import("nested2.rmskin", &z().bytes()).unwrap();
    assert!(c.skin("Basic/Main/Main.ini").is_file());
    assert!(!r.warnings.is_empty());
}

#[test]
fn f06_missing_name_refused() {
    let c = ctx();
    let z = Zip::new().file("RMSKIN.ini", "[rmskin]\nAuthor=x\n").file("Skins/B/B.ini", "[Rainmeter]\n").rmskin(0);
    assert!(matches!(c.import("noname.rmskin", &z).unwrap_err(), ImportError::NoName));
    assert!(!c.skin("B").exists());
}

#[test]
fn f07_minimum_rainmeter_too_new() {
    let c = ctx();
    let z = Zip::new()
        .file("RMSKIN.ini", "[rmskin]\nName=T\nMinimumRainmeter=99.0\n")
        .file("Skins/B/B.ini", "[Rainmeter]\n")
        .rmskin(0);
    let p = c.write("minver.rmskin", &z);
    match import_path_with(&c.env, &p, &opts()).unwrap_err() {
        ImportError::TooNew { required, message } => {
            assert_eq!(required, "99.0");
            assert!(message.contains("99.0"), "{message}");
        }
        e => panic!("{e:?}"),
    }
    assert!(!c.skin("B").exists());
    let o = ImportOptions { ignore_min_version: true, ..opts() };
    import_path_with(&c.env, &p, &o).unwrap();
    assert!(c.skin("B/B.ini").is_file());
}

#[test]
fn f08_minimum_windows_ignored() {
    let c = ctx();
    let z = Zip::new()
        .file("RMSKIN.ini", "[rmskin]\nName=T\nMinimumWindows=10.0.99999\n")
        .file("Skins/B/B.ini", "[Rainmeter]\n")
        .rmskin(0);
    c.import("minwin.rmskin", &z).unwrap();
    assert!(c.skin("B/B.ini").is_file());
}

fn load_zip(extra_b: &str) -> Zip {
    Zip::new()
        .file("RMSKIN.ini", ini("LoadType=Skin\nLoad=Basic\\Main\\B.ini\n"))
        .file("Skins/Basic/Main/A.ini", "[Rainmeter]\n")
        .file("Skins/Basic/Main/B.ini", format!("[Rainmeter]\n{extra_b}"))
        .file("Skins/Basic/Main/b_c.ini", "[Rainmeter]\n")
}

#[test]
fn f09_load_skin_sets_active_index() {
    let c = ctx();
    let r = c.import("load-skin.rmskin", &load_zip("").rmskin(0)).unwrap();
    assert_eq!(r.loaded_skin, Some(("Basic\\Main".to_string(), "B.ini".to_string())));
    let s = c.settings().config("Basic\\Main").unwrap();
    assert_eq!(s.active, 2);
}

#[test]
fn f09b_load_false_does_not_activate() {
    let c = ctx();
    let p = c.write("l.rmskin", &load_zip("").rmskin(0));
    let o = ImportOptions { load: false, ..opts() };
    let r = import_path_with(&c.env, &p, &o).unwrap();
    assert!(r.loaded_skin.is_none());
    assert!(c.settings().config("Basic\\Main").is_none());
}

#[test]
fn f10_default_options_applied_to_new_section() {
    let c = ctx();
    let z = load_zip("DefaultWindowX=100\nDefaultWindowY=50\nDefaultAlwaysOnTop=1\n");
    c.import("d.rmskin", &z.rmskin(0)).unwrap();
    let s = c.settings();
    let st = s.config("Basic\\Main").unwrap();
    assert_eq!((st.active, st.window_x.as_str(), st.window_y.as_str(), st.always_on_top), (2, "100", "50", 1));
    // An existing section keeps its position.
    let mut s2 = s;
    s2.set_config_key("Basic\\Main", "WindowX", "7");
    s2.save().unwrap();
    c.import("d2.rmskin", &load_zip("DefaultWindowX=100\n").rmskin(0)).unwrap();
    assert_eq!(c.settings().config("Basic\\Main").unwrap().window_x, "7");
}

fn layout_zip() -> Zip {
    Zip::new()
        .file("RMSKIN.ini", ini("LoadType=Layout\nLoad=L Layout\n"))
        .file("Layouts/L Layout/Rainmeter.ini", "[Rainmeter]\nSkinPath=/evil\n[L\\A]\nActive=1\nWindowX=10\n")
        .file("Layouts/L Layout/Wallpaper.bmp", b"BM0000")
        .file("Skins/L/A/A.ini", "[Rainmeter]\n")
}

#[test]
fn f11_layout_installed_and_applied() {
    let c = ctx();
    fs::create_dir_all(&c.env.settings_dir).unwrap();
    fs::write(&c.env.settings_file, "[Snowmeter]\nWindowType=below\n[Rainmeter]\nSkinPath=/mine\n[Other\\X]\nActive=1\n").unwrap();
    let r = c.import("layout.rmskin", &layout_zip().rmskin(0)).unwrap();
    assert_eq!(r.layouts_installed, vec!["L Layout"]);
    assert_eq!(r.loaded_layout.as_deref(), Some("L Layout"));
    let lay = c.env.settings_dir.join("Layouts/L Layout");
    assert!(!lay.join("Wallpaper.bmp").exists());
    let li = IniFile::load(&lay.join("Rainmeter.ini")).unwrap();
    assert!(li.section("Rainmeter").is_some());
    assert!(li.section("Rainmeter").unwrap().entries.is_empty());
    assert_eq!(li.get("L\\A", "WindowX"), Some("10"));
    assert!(c.env.settings_dir.join("Layouts/@Backup").is_dir());
    let st = c.settings();
    assert_eq!(st.global("SkinPath"), None);
    let ini = IniFile::load(&c.env.settings_file).unwrap();
    assert_eq!(ini.get("Rainmeter", "SkinPath"), Some("/mine"));
    assert_eq!(st.window_type(), snowmeter_core::settings::WindowType::Below);
    let a = st.config("L\\A").unwrap();
    assert_eq!((a.active, a.window_x.as_str()), (1, "10"));
    assert_eq!(st.config("Other\\X").unwrap().active, 0);
}

#[test]
fn f12_plugins_never_installed() {
    let c = ctx();
    let z = Zip::new()
        .file("RMSKIN.ini", ini(""))
        .file("Skins/P/P.ini", "[Rainmeter]\n")
        .file("Plugins/32bit/Foo.dll", b"MZ")
        .file("Plugins/64bit/Foo.dll", b"MZ")
        .file("Plugins/64bit/WebParser.dll", b"MZ");
    let r = c.import("plugins.rmskin", &z.rmskin(0)).unwrap();
    assert_eq!(r.plugins_skipped, vec!["Foo.dll"]);
    assert_eq!(r.plugins_native, vec!["WebParser.dll"]);
    assert!(r.warnings.iter().any(|w| w.contains("Foo")));
    assert!(!r.warnings.iter().any(|w| w.contains("WebParser")));
    let mut found = Vec::new();
    for e in walk(&c.root.join("env")) {
        if e.extension().map(|x| x.eq_ignore_ascii_case("dll")).unwrap_or(false) {
            found.push(e);
        }
    }
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn f12b_native_plugin_noted() {
    let c = ctx();
    let z = Zip::new().file("RMSKIN.ini", ini("")).file("Skins/P/P.ini", "[Rainmeter]\n").file("Plugins/64bit/SysInfo.dll", b"MZ");
    let r = c.import("p.rmskin", &z.rmskin(0)).unwrap();
    assert!(r.plugins_skipped.is_empty());
    assert_eq!(r.plugins_native, vec!["SysInfo.dll"]);
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(p) {
        for e in rd.flatten() {
            let q = e.path();
            if q.is_dir() {
                out.extend(walk(&q));
            } else {
                out.push(q);
            }
        }
    }
    out
}

#[test]
fn f13_fonts_copied_and_cache_skipped() {
    let c = ctx();
    let z = Zip::new()
        .file("RMSKIN.ini", ini(""))
        .file("Skins/F/@Resources/Fonts/T.ttf", b"font-T")
        .file("Skins/F/@Resources/Fonts/sub/U.OTF", b"font-U")
        .file("Skins/F/@Resources/Fonts/readme.txt", b"hi")
        .file("Fonts/O.ttf", b"font-O")
        .file("Skins/F/S/S.ini", "[Rainmeter]\n");
    let r = c.import("fonts.rmskin", &z.rmskin(0)).unwrap();
    let dir = &c.env.fonts_install_dir;
    assert_eq!(fs::read(dir.join("F/T.ttf")).unwrap(), b"font-T");
    assert!(dir.join("F/sub/U.OTF").is_file());
    assert!(dir.join("F/O.ttf").is_file());
    assert!(!dir.join("F/readme.txt").exists());
    assert_eq!(r.fonts_installed.len(), 3);
    assert!(c.skin("F/@Resources/Fonts/T.ttf").is_file());
}

#[test]
fn f13b_fc_cache_failure_is_ignored() {
    // Runs fc-cache (if present) on a directory under the temp root only.
    let c = ctx();
    let z = Zip::new().file("RMSKIN.ini", ini("")).file("Skins/F/@Resources/Fonts/T.ttf", b"not a font").file("Skins/F/F.ini", "[Rainmeter]\n");
    let p = c.write("f.rmskin", &z.rmskin(0));
    import_path(&c.env, &p).unwrap();
    assert!(c.env.fonts_install_dir.join("F/T.ttf").is_file());
}

fn vars_zip(vars: &str, extra: &str) -> Zip {
    Zip::new()
        .file("RMSKIN.ini", ini(extra))
        .file("Skins/V/V.ini", "[Rainmeter]\n")
        .file("Skins/V/@Resources/Vars.inc", vars.to_string())
}

#[test]
fn f15_variable_files_preserved() {
    let c = ctx();
    c.import("v1.rmskin", &vars_zip("[Variables]\nColor=red\nSize=10\nOld=1\n", "").rmskin(0)).unwrap();
    let vf = c.skin("V/@Resources/Vars.inc");
    fs::write(&vf, "[Variables]\nColor=blue\nSize=10\nOld=1\n").unwrap();
    fs::write(c.skin("V/mine.txt"), "x").unwrap();
    let r = c
        .import(
            "v2.rmskin",
            &vars_zip("[Variables]\nColor=green\nSize=12\nNew=5\n", "VariableFiles=V\\@Resources\\Vars.inc\n").rmskin(0),
        )
        .unwrap();
    let i = IniFile::load(&vf).unwrap();
    assert_eq!(i.get("Variables", "Color"), Some("blue"));
    assert_eq!(i.get("Variables", "Size"), Some("10"));
    assert_eq!(i.get("Variables", "New"), Some("5"));
    assert_eq!(i.get("Variables", "Old"), None);
    assert!(c.skin("@Backup/V/mine.txt").is_file());
    assert!(!c.skin("V/mine.txt").exists());
    assert_eq!(r.backups, vec!["V"]);
}

#[test]
fn f16_merge_overlays() {
    let c = ctx();
    c.import("v1.rmskin", &vars_zip("[Variables]\nColor=red\n", "").rmskin(0)).unwrap();
    fs::write(c.skin("V/mine.txt"), "x").unwrap();
    c.import(
        "v2.rmskin",
        &vars_zip("[Variables]\nColor=green\n", "MergeSkins=1\nVariableFiles=V\\@Resources\\Vars.inc\n").rmskin(0),
    )
    .unwrap();
    let i = IniFile::load(&c.skin("V/@Resources/Vars.inc")).unwrap();
    assert_eq!(i.get("Variables", "Color"), Some("green"));
    assert!(c.skin("V/mine.txt").is_file());
    assert!(!c.skin("@Backup").exists());
}

#[test]
fn f17_backup_replaces_previous_backup() {
    let c = ctx();
    c.import("v1.rmskin", &vars_zip("[Variables]\nA=1\n", "").rmskin(0)).unwrap();
    fs::write(c.skin("V/mine.txt"), "x").unwrap();
    c.import("v2.rmskin", &vars_zip("[Variables]\nA=2\n", "").rmskin(0)).unwrap();
    assert!(c.skin("@Backup/V/mine.txt").is_file());
    assert!(!c.skin("V/mine.txt").exists());
    c.import("v3.rmskin", &vars_zip("[Variables]\nA=3\n", "").rmskin(0)).unwrap();
    assert!(!c.skin("@Backup/V/mine.txt").exists());
    let i = IniFile::load(&c.skin("@Backup/V/@Resources/Vars.inc")).unwrap();
    assert_eq!(i.get("Variables", "A"), Some("2"));
}

#[test]
fn f18_backup_package_flag_replaces_without_backup() {
    let c = ctx();
    c.import("v1.rmskin", &vars_zip("[Variables]\nA=1\n", "").rmskin(0)).unwrap();
    fs::write(c.skin("V/mine.txt"), "x").unwrap();
    c.import("bk.rmskin", &vars_zip("[Variables]\nA=2\n", "").rmskin(2)).unwrap();
    assert!(!c.skin("V/mine.txt").exists());
    assert!(!c.skin("@Backup").exists());
    assert_eq!(IniFile::load(&c.skin("V/@Resources/Vars.inc")).unwrap().get("Variables", "A"), Some("2"));
}

#[test]
fn f19_ignored_roots_and_loose_files() {
    let c = ctx();
    let z = Zip::new()
        .file("RMSKIN.ini", ini(""))
        .file("Skins/@Backup/x.ini", "")
        .file("Skins/@Vault/y.ini", "")
        .file("Skins/top.ini", "")
        .file("README.txt", "")
        .file("Skins/R/R.ini", "[Rainmeter]\n");
    let r = c.import("ignored.rmskin", &z.rmskin(0)).unwrap();
    assert_eq!(names_in(&c.env.skins_dir), vec!["R"]);
    assert_eq!(r.installed_roots, vec!["R"]);
}

#[test]
fn f20_zip_slip_rejected() {
    for bad in ["Skins/R/../../evil.ini", "/abs.ini", "Skins/../../evil.ini", "C:/evil.ini", "Skins/R/..\\..\\evil.ini"] {
        let c = ctx();
        let z = Zip::new().file("RMSKIN.ini", ini("")).file("Skins/R/R.ini", "[Rainmeter]\n").file(bad, "x");
        let e = c.import("slip.rmskin", &z.rmskin(0)).unwrap_err();
        assert!(matches!(e, ImportError::Unsafe(_)), "{bad}: {e:?}");
        assert!(!c.skin("R").exists(), "{bad}");
        assert!(!c.root.join("evil.ini").exists() && !c.root.join("env/evil.ini").exists());
    }
}

#[test]
fn f20b_symlink_rejected() {
    let c = ctx();
    let z = Zip::new().file("RMSKIN.ini", ini("")).file("Skins/R/R.ini", "[Rainmeter]\n").symlink("Skins/R/link", "/etc/passwd");
    let e = c.import("sym.rmskin", &z.rmskin(0)).unwrap_err();
    assert!(matches!(e, ImportError::Unsafe(_)), "{e:?}");
    assert!(!c.skin("R").exists());
}

#[test]
fn f22_case_insensitive_roots() {
    let c = ctx();
    fs::create_dir_all(c.skin("FOO")).unwrap();
    fs::write(c.skin("FOO/old.txt"), "o").unwrap();
    let z = Zip::new().file("RMSKIN.ini", ini("")).file("Skins/Foo/a.ini", "[Rainmeter]\n").file("Skins/foo/Img/b.png", "p");
    let r = c.import("case.rmskin", &z.rmskin(0)).unwrap();
    assert_eq!(names_in(&c.env.skins_dir), vec!["@Backup", "FOO"]);
    assert!(c.skin("FOO/a.ini").is_file() && c.skin("FOO/Img/b.png").is_file());
    assert!(c.skin("@Backup/FOO/old.txt").is_file());
    assert_eq!(r.installed_roots, vec!["FOO"]);
}

#[test]
fn f23_old_format_rainstaller() {
    let c = ctx();
    let z = Zip::new()
        .file("MySkin/Rainstaller.cfg", "[Rainstaller]\nName=Old\nLaunchType=Load\nLaunchCommand=Old\\A\\A.ini\n")
        .file("MySkin/Skins/Old/A/A.ini", "[Rainmeter]\n")
        .file("MySkin/Fonts/f.ttf", b"ff");
    let r = c.import("old.zip", &z.bytes()).unwrap();
    assert!(c.skin("Old/A/A.ini").is_file());
    assert!(c.skin("Old/@Resources/Fonts/f.ttf").is_file());
    assert!(c.env.fonts_install_dir.join("Old/f.ttf").is_file());
    assert_eq!(r.loaded_skin, Some(("Old\\A".to_string(), "A.ini".to_string())));
    assert_eq!(c.settings().config("Old\\A").unwrap().active, 1);
}

#[test]
fn f24_utf16_options_file() {
    let c = ctx();
    let mut u: Vec<u8> = vec![0xFF, 0xFE];
    for w in ini("").encode_utf16() {
        u.extend_from_slice(&w.to_le_bytes());
    }
    let z = Zip::new().file("RMSKIN.ini", u).file("Skins/U/U.ini", "[Rainmeter]\n");
    let r = c.import("utf16.rmskin", &z.rmskin(0)).unwrap();
    assert_eq!(r.name, "Test");
    assert!(c.skin("U/U.ini").is_file());
}

// ------------------------------------------------------- lenient inputs

#[test]
fn folder_containing_skins_dir() {
    let c = ctx();
    let src = c.root.join("pack");
    fs::create_dir_all(src.join("Skins/Alpha/Cfg")).unwrap();
    fs::create_dir_all(src.join("Skins/Beta")).unwrap();
    fs::write(src.join("Skins/Alpha/Cfg/Cfg.ini"), "[Rainmeter]\n").unwrap();
    fs::write(src.join("Skins/Beta/b.ini"), "[Rainmeter]\n").unwrap();
    let r = import_path_with(&c.env, &src, &opts()).unwrap();
    assert_eq!(r.installed_roots, vec!["Alpha", "Beta"]);
    assert!(c.skin("Alpha/Cfg/Cfg.ini").is_file() && c.skin("Beta/b.ini").is_file());
    assert!(src.join("Skins/Alpha/Cfg/Cfg.ini").is_file(), "source untouched");
}

#[test]
fn folder_that_is_a_single_skin_root() {
    let c = ctx();
    let src = c.root.join("MySkin");
    fs::create_dir_all(src.join("@Resources/Fonts")).unwrap();
    fs::create_dir_all(src.join("Sub")).unwrap();
    fs::write(src.join("Sub/Sub.ini"), "[Rainmeter]\n").unwrap();
    fs::write(src.join("@Resources/Fonts/Q.ttf"), "q").unwrap();
    let r = import_path_with(&c.env, &src, &opts()).unwrap();
    assert_eq!(r.installed_roots, vec!["MySkin"]);
    assert!(c.skin("MySkin/Sub/Sub.ini").is_file());
    assert!(c.env.fonts_install_dir.join("MySkin/Q.ttf").is_file());
    assert!(r.configs.contains(&"MySkin\\Sub".to_string()));
}

#[test]
fn zip_that_is_a_single_skin_root_with_wrapper() {
    let c = ctx();
    let z = Zip::new().file("illustro/Clock/Clock.ini", "[Rainmeter]\n").file("illustro/@Resources/a.inc", "x");
    let r = c.import("illustro.zip", &z.bytes()).unwrap();
    assert!(c.skin("illustro/Clock/Clock.ini").is_file());
    assert_eq!(r.installed_roots, vec!["illustro"]);
    assert!(!r.warnings.is_empty());
}

#[test]
fn zip_with_skins_dir_but_no_options_file() {
    let c = ctx();
    let z = Zip::new().file("Skins/Q/Q.ini", "[Rainmeter]\n");
    c.import("q.rmskin", &z.bytes()).unwrap();
    assert!(c.skin("Q/Q.ini").is_file());
}

#[test]
fn garbage_is_refused() {
    let c = ctx();
    assert!(matches!(c.import("x.rmskin", b"not a zip at all, definitely not").unwrap_err(), ImportError::NotAPackage(_)));
    let z = Zip::new().file("readme.txt", "hello");
    assert!(c.import("y.zip", &z.bytes()).is_err());
    let missing = c.root.join("nope.rmskin");
    assert!(matches!(import_path(&c.env, &missing).unwrap_err(), ImportError::Io(_)));
}

#[test]
fn f25_multiple_variants_and_resources_not_a_config() {
    let c = ctx();
    let z = Zip::new()
        .file("RMSKIN.ini", ini(""))
        .file("Skins/illustro/Disk/1 Disk.ini", "[Rainmeter]\n")
        .file("Skins/illustro/Disk/2 Disks.ini", "[Rainmeter]\n")
        .file("Skins/illustro/Clock/Clock.ini", "[Rainmeter]\n")
        .file("Skins/illustro/@Resources/Images/x.png", "x");
    let r = c.import("i.rmskin", &z.rmskin(0)).unwrap();
    let mut cfgs = r.configs.clone();
    cfgs.sort();
    assert_eq!(cfgs, vec!["illustro\\Clock", "illustro\\Disk"]);
    assert_eq!(r.compat.len(), 2);
    assert!(r.compat.iter().all(|c| c.fully_supported));
    assert!(r.to_string().contains("illustro\\Clock: Fully supported"), "{r}");
}
