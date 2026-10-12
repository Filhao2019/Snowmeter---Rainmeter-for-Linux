use snowmeter_core::color::Color;
use snowmeter_core::env::Environment;
use snowmeter_core::parser::{ConfigParser, NoSectionValues, SectionValues, SkinIdentity};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- helpers

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m1")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

struct Loaded {
    _tmp: tempfile::TempDir,
    env: Environment,
    p: ConfigParser,
}

fn identity(env: &Environment, config: &str, file: &str) -> SkinIdentity {
    SkinIdentity {
        config: config.to_string(),
        root_config: config.split('\\').next().unwrap().to_string(),
        file: file.to_string(),
        ini_path: env.skins_dir.join(config.replace('\\', "/")).join(file),
    }
}

fn load_fixture(name: &str, file: &str) -> Loaded {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    copy_dir(&fixtures().join(name), &env.skins_dir.join(name));
    let p = ConfigParser::load(&env, &identity(&env, name, file)).unwrap();
    Loaded { _tmp: tmp, env, p }
}

/// Build a skin `T\Main.ini` from (relative path, content) pairs.
fn load_inline(files: &[(&str, &str)]) -> Loaded {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    for (rel, content) in files {
        let path = env.skins_dir.join("T").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    let p = ConfigParser::load(&env, &identity(&env, "T", "Main.ini")).unwrap();
    Loaded { _tmp: tmp, env, p }
}

struct Sv;
impl SectionValues for Sv {
    fn section_variable(&self, name: &str, param: Option<&str>) -> Option<String> {
        match (name.to_ascii_lowercase().as_str(), param) {
            ("measurestring2", None) => Some("Second".into()),
            ("measureindex", None) => Some("2".into()),
            ("m", None) => Some("30".into()),
            ("m", Some("%")) => Some("50".into()),
            ("meter", Some("XW")) => Some("40".into()),
            _ => None,
        }
    }
    fn measure_value(&self, name: &str) -> Option<f64> {
        match name.to_ascii_lowercase().as_str() {
            "meas" => Some(21.0),
            "m" => Some(30.0),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- variables

#[test]
fn old_style_variable_vectors() {
    let l = load_fixture("Basic", "Basic.ini");
    let r = |k: &str| l.p.read_string("MeterOne", k, "", None);
    assert_eq!(r("T1"), "abc");
    assert_eq!(r("T2"), "abcabc");
    assert_eq!(r("T3"), "#A#");
    assert_eq!(r("T6"), "#Fooabc");
    assert_eq!(r("T10"), "abcabc #CCC# def#A#");
    assert_eq!(r("T11"), "\n");
}

#[test]
fn new_style_nested_variables() {
    let l = load_fixture("Basic", "Basic.ini");
    let r = |k: &str| l.p.read_string("MeterOne", k, "", None);
    assert_eq!(r("T4"), "Second");
    assert_eq!(r("T5"), "[#Var2]");
    assert_eq!(r("T12"), "Second");
    assert_eq!(r("T9"), "ABC");
}

#[test]
fn forward_references() {
    let l = load_fixture("Basic", "Basic.ini");
    // new-style forward reference works, old-style keeps the literal
    assert_eq!(l.p.read_string("MeterOne", "T7", "", None), "1");
    assert_eq!(l.p.read_string("MeterOne", "T8", "", None), "#Later#");
}

#[test]
fn replace_variables_vectors() {
    let l = load_fixture("Basic", "Basic.ini");
    let rv = |s: &str| l.p.replace_variables(s);
    assert_eq!(rv("[#Var[#Index]]"), "Second");
    assert_eq!(rv("[#[#Name]]"), "Second");
    assert_eq!(rv("[#*Var[#Index]*]"), "[#Var2]");
    assert_eq!(rv("[#]"), "[#]");
    assert_eq!(rv("[\\0][\\x][\\x110000][\\NotANumber]"), "[\\0][\\x][\\x110000][\\NotANumber]");
    assert_eq!(rv("[\\*65*]"), "[\\65]");
    assert_eq!(rv("[#Chain1]"), "Final");
    assert_eq!(rv("[#Deep1]"), "[#Deep12]");
    assert_eq!(rv("[#SelfRef]"), "[#SelfRef]");
    assert_eq!(rv("x"), "x");
    assert_eq!(rv("#A#"), "abc");
    assert_eq!(rv("#CURRENTSECTION#"), "");
}

#[test]
fn variables_are_case_insensitive_and_settable() {
    let mut l = load_fixture("Basic", "Basic.ini");
    assert_eq!(l.p.get_variable("a").as_deref(), Some("abc"));
    assert_eq!(l.p.get_variable("INDEX").as_deref(), Some("2"));
    assert_eq!(l.p.get_variable("Missing"), None);
    l.p.set_variable("a", "zzz");
    assert_eq!(l.p.replace_variables("#A#"), "zzz");
    l.p.set_variable("Brand", "new");
    assert_eq!(l.p.replace_variables("[#BRAND]"), "new");
}

#[test]
fn builtins_shadow_user_variables() {
    let mut l = load_fixture("Basic", "Basic.ini");
    l.p.set_variable("CURRENTCONFIG", "hacked");
    assert_eq!(l.p.get_variable("CURRENTCONFIG").as_deref(), Some("Basic"));
}

#[test]
fn spaced_variable_values_are_trimmed() {
    let l = load_fixture("Basic", "Basic.ini");
    assert_eq!(l.p.get_variable("Spaced").as_deref(), Some("inner"));
    assert_eq!(l.p.read_string("MeterOne", "Quoted", "", None), "  padded  ");
}

#[test]
fn current_section_variable() {
    let l = load_fixture("Basic", "Basic.ini");
    assert_eq!(l.p.read_string("MeterOne", "Cur", "", None), "MeterOne MeterOne");
}

#[test]
fn environment_variables_expand() {
    std::env::set_var("SNOWMETER_TEST_ENV_A", "alpha");
    std::env::set_var("SNOWMETER_TEST_ENV_B", "beta");
    let l = load_fixture("Basic", "Basic.ini");
    assert_eq!(l.p.read_string("MeterOne", "Env", "", None), "alpha and beta");
}

// ---------------------------------------------------------------- built-ins

#[test]
fn builtin_paths_have_trailing_slash() {
    let l = load_fixture("Basic", "Basic.ini");
    let skins = l.env.skins_dir.display().to_string();
    let g = |n: &str| l.p.get_variable(n).unwrap_or_else(|| panic!("missing {n}"));
    assert_eq!(g("@"), format!("{skins}/Basic/@Resources/"));
    assert_eq!(g("SKINSPATH"), format!("{skins}/"));
    assert_eq!(g("CURRENTPATH"), format!("{skins}/Basic/"));
    assert_eq!(g("ROOTCONFIGPATH"), format!("{skins}/Basic/"));
    assert_eq!(g("SETTINGSPATH"), format!("{}/", l.env.settings_dir.display()));
    assert_eq!(g("PROGRAMPATH"), format!("{}/", l.env.program_dir.display()));
    assert!(g("PLUGINSPATH").ends_with('/'));
    assert!(g("ADDONSPATH").ends_with('/'));
    assert_eq!(g("CURRENTFILE"), "Basic.ini");
    assert_eq!(g("CURRENTCONFIG"), "Basic");
    assert_eq!(g("ROOTCONFIG"), "Basic");
    assert_eq!(g("CRLF"), "\n");
    assert_eq!(g("CURRENTSECTION"), "");
    assert!(l.p.get_variable("CONFIGEDITOR").is_some());
}

#[test]
fn builtins_in_variables_section() {
    let l = load_fixture("Basic", "Basic.ini");
    let skins = l.env.skins_dir.display().to_string();
    assert_eq!(l.p.get_variable("Path").unwrap(), format!("{skins}/Basic/@Resources/Images\\bg.png"));
    assert_eq!(l.p.get_variable("Here").unwrap(), format!("{skins}/Basic/x.png"));
}

#[test]
fn nested_config_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    let dir = env.skins_dir.join("illustro/Clock");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Clock.ini"), "[Rainmeter]\nUpdate=1\n[Variables]\nX=#CURRENTCONFIG#|#ROOTCONFIG#|#CURRENTFILE#\n").unwrap();
    let p = ConfigParser::load(&env, &identity(&env, "illustro\\Clock", "Clock.ini")).unwrap();
    assert_eq!(p.get_variable("X").unwrap(), "illustro\\Clock|illustro|Clock.ini");
    let skins = env.skins_dir.display().to_string();
    assert_eq!(p.get_variable("@").unwrap(), format!("{skins}/illustro/@Resources/"));
    assert_eq!(p.get_variable("CURRENTPATH").unwrap(), format!("{skins}/illustro/Clock/"));
    assert_eq!(p.get_variable("ROOTCONFIGPATH").unwrap(), format!("{skins}/illustro/"));
}

#[test]
fn monitor_variables() {
    let l = load_fixture("Basic", "Basic.ini");
    let g = |n: &str| l.p.get_variable(n);
    assert_eq!(g("SCREENAREAWIDTH").as_deref(), Some("1920"));
    assert_eq!(g("SCREENAREAHEIGHT").as_deref(), Some("1080"));
    assert_eq!(g("SCREENAREAX").as_deref(), Some("0"));
    assert_eq!(g("WORKAREAWIDTH").as_deref(), Some("1920"));
    assert_eq!(g("PSCREENAREAHEIGHT").as_deref(), Some("1080"));
    assert_eq!(g("VSCREENAREAWIDTH").as_deref(), Some("1920"));
    assert_eq!(g("SCREENAREAWIDTH@1").as_deref(), Some("1920"));
    assert_eq!(g("SCREENAREAWIDTH@2"), None);
    assert_eq!(g("SCREENAREAWIDTH@0"), None);
    assert_eq!(g("screenareawidth").as_deref(), Some("1920"));
}

// ---------------------------------------------------------------- sections

#[test]
fn section_names_exclude_special_sections() {
    let l = load_fixture("Basic", "Basic.ini");
    assert_eq!(l.p.section_names(), vec!["MeterOne", "Num", "Color"]);
}

// ---------------------------------------------------------------- section variables

#[test]
fn section_variables_only_with_sv() {
    let l = load_fixture("Basic", "Basic.ini");
    assert_eq!(l.p.read_string("MeterOne", "Raw", "", None), "[MeasureString2]");
    assert_eq!(l.p.read_string("MeterOne", "Raw", "", Some(&Sv)), "Second");
    assert_eq!(l.p.read_string("MeterOne", "Raw", "", Some(&NoSectionValues)), "[MeasureString2]");
    // variable holding a section reference stays literal until used with sv
    assert_eq!(l.p.read_string("MeterOne", "UsesSV", "", None), "[MeasureString2]");
    assert_eq!(l.p.read_string("MeterOne", "UsesSV", "", Some(&Sv)), "Second");
}

#[test]
fn replace_section_variables_vectors() {
    let l = load_fixture("Basic", "Basic.ini");
    let rm = |s: &str| l.p.replace_section_variables(s, &Sv);
    assert_eq!(rm("[#Var[&MeasureIndex]]"), "Second");
    assert_eq!(rm("[&MeasureString[#Index]]"), "Second");
    assert_eq!(rm("[&MeasureString[&MeasureIndex]]"), "Second");
    assert_eq!(rm("[MeasureString2] [&MeasureString2] [#Var2]"), "Second Second Second");
    assert_eq!(rm("[*MeasureString2*] [&MeasureString2]"), "[MeasureString2] Second");
    assert_eq!(rm("[MeasureString[#Index]]"), "Second");
    assert_eq!(rm("[MeasureString[MeasureIndex]]"), "[MeasureString2]");
    assert_eq!(rm("[M]"), "30");
    assert_eq!(rm("[M:%]"), "50");
    assert_eq!(rm("[Meter:XW]"), "40");
    assert_eq!(rm("[&Unknown]"), "[&Unknown]");
    assert_eq!(rm("[Unknown]"), "[Unknown]");
    assert_eq!(rm("[#]"), "[#]");
    assert_eq!(rm(&"(?:[A-Z])".repeat(1000)), "(?:[A-Z])".repeat(1000));
}

// ---------------------------------------------------------------- numbers / colors / formulas

#[test]
fn read_int_float_table() {
    let l = load_fixture("Basic", "Basic.ini");
    let i = |k: &str| l.p.read_int("Num", k, 5, None);
    let f = |k: &str| l.p.read_float("Num", k, 5.0, None);
    assert_eq!(l.p.read_int("Num", "Missing", 5, None), 5);
    assert_eq!(l.p.read_float("Num", "Missing", 5.0, None), 5.0);
    assert_eq!((i("I1"), f("I1")), (12, 12.0));
    assert_eq!((i("I2"), f("I2")), (12, 12.7));
    assert_eq!((i("I3"), f("I3")), (0, 0.0));
    assert_eq!((i("I4"), f("I4")), (0, 0.0));
    assert_eq!((i("I5"), f("I5")), (0, 16.0));
    assert_eq!((i("I6"), f("I6")), (3, 3.0));
    assert_eq!((i("I7"), f("I7")), (2, 2.9));
    assert_eq!((i("I8"), f("I8")), (-2, -2.9));
    assert_eq!((i("I9"), f("I9")), (5, 5.0));
    assert_eq!((i("I10"), f("I10")), (15, 15.0));
    assert_eq!((i("I11"), f("I11")), (1, 1000.0));
    assert_eq!((i("I12"), f("I12")), (5, 2147483648.0));
    assert_eq!((i("I13"), f("I13")), (5, 5.0));
    assert_eq!((i("I14"), f("I14")), (10, 10.0));
    assert_eq!(i("I15"), 2);
    assert_eq!(i("Neg"), -7);
    assert_eq!(i("Big"), 5);
}

#[test]
fn formulas_in_read_float_with_variables_and_measures() {
    let l = load_fixture("Basic", "Basic.ini");
    assert_eq!(l.p.read_float("Num", "F1", 0.0, None), 4.0);
    assert_eq!(l.p.read_float("Num", "F2", 0.0, Some(&Sv)), 42.0);
    // unknown measure without sv -> formula error -> default
    assert_eq!(l.p.read_float("Num", "F2", -1.0, None), -1.0);
    // division by zero -> default
    assert_eq!(l.p.read_float("Num", "F3", -1.0, Some(&Sv)), -1.0);
}

#[test]
fn parse_formula_api() {
    let l = load_fixture("Basic", "Basic.ini");
    assert_eq!(l.p.parse_formula("(1+2)*#Index#", None), Some(6.0));
    assert_eq!(l.p.parse_formula("Meas+1", Some(&Sv)), Some(22.0));
    assert_eq!(l.p.parse_formula("Meas+1", None), None);
    assert_eq!(l.p.parse_formula("1/0", None), None);
}

#[test]
fn read_color_cases() {
    let l = load_fixture("Basic", "Basic.ini");
    let d = Color { r: 7, g: 7, b: 7, a: 7 };
    assert_eq!(l.p.read_color("Color", "C1", d, None), Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(l.p.read_color("Color", "C2", d, None), Color { r: 0, g: 255, b: 0, a: 255 });
    assert_eq!(l.p.read_color("Color", "C3", d, None), d);
    assert_eq!(l.p.read_color("Color", "Nope", d, None), d);
    assert_eq!(l.p.read_color("Color", "C4", d, None), Color { r: 20, g: 20, b: 30, a: 40 });
}

#[test]
fn has_key_and_default_string() {
    let l = load_fixture("Basic", "Basic.ini");
    assert!(l.p.has_key("meterone", "t1"));
    assert!(!l.p.has_key("MeterOne", "Nope"));
    assert_eq!(l.p.read_string("MeterOne", "Nope", "dflt", None), "dflt");
}

// ---------------------------------------------------------------- MeterStyle

#[test]
fn meter_style_precedence() {
    let l = load_fixture("Styles", "Styles.ini");
    let d = Color { r: 7, g: 7, b: 7, a: 7 };
    // last listed style wins
    assert_eq!(l.p.read_string("M1", "FontColor", "", None), "2,2,2");
    assert_eq!(l.p.read_string("M1", "Only1", "", None), "a");
    // own value beats styles
    assert_eq!(l.p.read_string("M2", "FontColor", "", None), "9,9,9");
    // empty own value blocks inheritance
    assert_eq!(l.p.read_string("M3", "FontColor", "x", None), "");
    assert_eq!(l.p.read_color("M3", "FontColor", d, None), d);
    assert!(l.p.has_key("M3", "FontColor"));
    // whitespace around | is trimmed; spacing-tolerant
    assert_eq!(l.p.read_string("M3", "Only1", "", None), "a");
}

#[test]
fn meter_style_is_one_level_only() {
    let l = load_fixture("Styles", "Styles.ini");
    assert_eq!(l.p.read_string("M4", "Y", "", None), "6");
    assert_eq!(l.p.read_string("M4", "X", "none", None), "none");
}

#[test]
fn meter_style_unknown_names_skipped_and_vars_expanded() {
    let l = load_fixture("Styles", "Styles.ini");
    assert_eq!(l.p.read_string("M5", "FontColor", "", None), "1,1,1");
    assert_eq!(l.p.read_string("M5", "Size", "", None), "10");
    assert_eq!(l.p.read_string("M6", "Only1", "", None), "a");
}

#[test]
fn meter_style_values_substituted_in_reader_context() {
    let l = load_fixture("Styles", "Styles.ini");
    assert_eq!(l.p.read_string("M7", "Text", "", None), "in M7 -> M7");
    assert_eq!(l.p.read_string("M6", "Text", "", None), "in M6 -> M6");
}

#[test]
fn meter_and_measure_keys_not_inherited() {
    let l = load_fixture("Styles", "Styles.ini");
    assert_eq!(l.p.read_string("M8", "Foo", "", None), "1");
    assert_eq!(l.p.read_string("M8", "Meter", "String", None), "String");
    assert!(!l.p.has_key("M8", "Meter") || l.p.read_string("M8", "Meter", "", None) == "String");
    assert_eq!(l.p.read_string("M8", "MeterStyle", "", None), "S5");
}

#[test]
fn inherit_option() {
    let l = load_fixture("Styles", "Styles.ini");
    assert_eq!(l.p.read_string("Child", "Value", "0", None), "1");
}

#[test]
fn set_option_overrides_and_switches_style() {
    let mut l = load_fixture("Styles", "Styles.ini");
    l.p.set_option("M1", "FontColor", "5,5,5");
    assert_eq!(l.p.read_string("M1", "FontColor", "", None), "5,5,5");
    l.p.set_option("M1", "MeterStyle", "S1");
    l.p.set_option("M1", "FontColor", "");
    // An empty value deletes the option, so the style's value shows through.
    assert_eq!(l.p.read_string("M1", "FontColor", "x", None), "1,1,1");
    l.p.set_option("M5", "MeterStyle", "S2");
    assert_eq!(l.p.read_string("M5", "FontColor", "", None), "2,2,2");
    l.p.set_option("M2", "Brand", "New");
    assert_eq!(l.p.read_string("M2", "BRAND", "", None), "New");
}

// ---------------------------------------------------------------- @Include

#[test]
fn include_fixture_variables_and_order() {
    let l = load_fixture("Includes", "Main.ini");
    assert_eq!(l.p.get_variable("FromShared").as_deref(), Some("yes"));
    assert_eq!(l.p.get_variable("Inc").as_deref(), Some("included"));
    assert_eq!(l.p.get_variable("Nested").as_deref(), Some("deep"));
    // vars.inc came after Main=1 -> 2; After=3 is after the include in main -> 3
    assert_eq!(l.p.get_variable("Main").as_deref(), Some("2"));
    assert_eq!(l.p.get_variable("After").as_deref(), Some("3"));
    assert_eq!(l.p.section_names(), vec!["MeterA", "MeterInc1", "MeterInc2", "MeterB"]);
}

#[test]
fn include_section_order_vectors() {
    let names = |files: &[(&str, &str)]| load_inline(files).p.section_names();
    assert_eq!(
        names(&[("Main.ini", "[A]\n@Include=x.inc\n[B]\n"), ("x.inc", "[X1]\n[X2]\n")]),
        vec!["A", "X1", "X2", "B"]
    );
    assert_eq!(
        names(&[("Main.ini", "[A]\n@Include=x.inc\n@Include2=y.inc\n[B]\n"), ("x.inc", "[X]\n"), ("y.inc", "[Y]\n")]),
        vec!["A", "X", "Y", "B"]
    );
    // last section of an include: nested include goes to the end of the list
    assert_eq!(
        names(&[("Main.ini", "[A]\n@Include=x.inc\n[B]\n"), ("x.inc", "[X1]\n[X2]\n@Include=y.inc\n"), ("y.inc", "[Y]\n")]),
        vec!["A", "X1", "X2", "B", "Y"]
    );
    assert_eq!(
        names(&[("Main.ini", "[A]\n@Include=x.inc\n[B]\n"), ("x.inc", "[X1]\n@Include=y.inc\n[X2]\n"), ("y.inc", "[Y]\n")]),
        vec!["A", "X1", "Y", "X2", "B"]
    );
    // include in the last section of main appends
    assert_eq!(
        names(&[("Main.ini", "[A]\n[Z]\n@Include=x.inc\n"), ("x.inc", "[X1]\n[X2]\n")]),
        vec!["A", "Z", "X1", "X2"]
    );
}

#[test]
fn include_value_merge_rules() {
    let l = load_inline(&[
        ("Main.ini", "[Variables]\nA=1\n@Include=vars.inc\nB=3\n"),
        ("vars.inc", "[Variables]\nA=2\nB=2\n"),
    ]);
    assert_eq!(l.p.get_variable("A").as_deref(), Some("2"));
    assert_eq!(l.p.get_variable("B").as_deref(), Some("3"));
}

#[test]
fn duplicate_section_and_key_rules_apply_per_file() {
    let l = load_inline(&[("Main.ini", "[A]\nX=1\n[A]\nY=2\n[B]\nK=1\nk=2\n")]);
    assert_eq!(l.p.read_string("A", "X", "", None), "1");
    assert!(!l.p.has_key("A", "Y"));
    assert_eq!(l.p.read_string("B", "K", "", None), "1");
}

#[test]
fn include_paths_use_variables_and_are_relative_to_including_file() {
    let l = load_inline(&[
        ("Main.ini", "[Variables]\nSub=Sub\n@Include=#SKINSPATH#T\\sub\\ONE.inc\n@Include2=#Sub#\\two.inc\n[M]\n"),
        ("Sub/one.inc", "[Variables]\nOne=1\n@Include=three.inc\n"),
        ("Sub/three.inc", "[Variables]\nThree=3\n"),
        ("Sub/two.inc", "[Variables]\nTwo=2\n"),
    ]);
    assert_eq!(l.p.get_variable("One").as_deref(), Some("1"));
    assert_eq!(l.p.get_variable("Two").as_deref(), Some("2"));
    assert_eq!(l.p.get_variable("Three").as_deref(), Some("3"));
}

#[test]
fn include_works_in_any_section_and_missing_is_skipped() {
    let l = load_inline(&[
        ("Main.ini", "[Rainmeter]\n@Include=nope.inc\n[M]\n@IncludeFoo=x.inc\n@include_bar=\n"),
        ("x.inc", "[N]\nV=1\n"),
    ]);
    assert_eq!(l.p.section_names(), vec!["M", "N"]);
    assert_eq!(l.p.read_string("N", "V", "", None), "1");
}

#[test]
fn include_loop_terminates() {
    let l = load_inline(&[("Main.ini", "[A]\n@Include=Main.ini\nK=1\n")]);
    assert_eq!(l.p.read_string("A", "K", "", None), "1");
}

#[test]
fn utf16_skin_and_include_load() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    let dir = env.skins_dir.join("T");
    fs::create_dir_all(&dir).unwrap();
    let u16 = |s: &str| -> Vec<u8> {
        let mut v = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v
    };
    fs::write(dir.join("Main.ini"), u16("[Variables]\r\nA=\u{e9}t\u{e9}\r\n@Include=i.inc\r\n[M]\r\nText=#A##B#\r\n")).unwrap();
    fs::write(dir.join("i.inc"), "\u{feff}[Variables]\nB=ok\n".as_bytes()).unwrap();
    let p = ConfigParser::load(&env, &identity(&env, "T", "Main.ini")).unwrap();
    assert_eq!(p.read_string("M", "Text", "", None), "\u{e9}t\u{e9}ok");
}

#[test]
fn load_missing_main_file_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    assert!(ConfigParser::load(&env, &identity(&env, "T", "Main.ini")).is_err());
}
