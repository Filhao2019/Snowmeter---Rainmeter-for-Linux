use snowmeter_core::compat::{check_config, check_path};
use snowmeter_core::env::Environment;
use std::fs;

fn skin(env: &Environment, rel: &str, text: &str) {
    let p = env.skins_dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn setup() -> (tempfile::TempDir, Environment) {
    let d = tempfile::tempdir().unwrap();
    let env = Environment::with_root(d.path());
    skin(
        &env,
        "Good/Clock/Clock.ini",
        "[Rainmeter]\nUpdate=1000\nUseD2D=1\nBlur=1\nLeftMouseUpAction=[!SetVariable A 1][!Refresh][!Manage]\n\
         [Variables]\nFont=Arial\n@Include=#@#Extra.inc\n\
         [mTime]\nMeasure=Time\nFormat=%H:%M\n\
         [mWeb]\nMeasure=Plugin\nPlugin=WebParser\n\
         [mCpu]\nMeasure=CPU\n\
         [Text]\nMeter=String\nMeasureName=mTime\nMouseOverAction=[explorer.exe \"C:\\\"]\nLeftMouseUpAction=[shell:Downloads]\n\
         [Bar]\nMeter=Bar\nMeasureName=mCpu\n",
    );
    skin(&env, "Good/@Resources/Extra.inc", "[Included]\nMeasure=Calc\nFormula=1+1\nIfTrueAction=[!UpdateMeasure mCpu]\n");
    skin(
        &env,
        "Plug/Net/Net.ini",
        "[Rainmeter]\n[mHot]\nMeasure=Plugin\nPlugin=Plugins\\HotKey.dll\n[mNow]\nMeasure=Process\n\
         [mFine]\nMeasure=Plugin\nPlugin=Script\n[t]\nMeter=String\nMeasureName=mHot\n",
    );
    skin(
        &env,
        "Bad/Cfg/Cfg.ini",
        "[Rainmeter]\nOnRefreshAction=[!FooBang x]\n[m]\nMeasure=Time\n[w]\nMeter=Waveform\n[b]\nMeter=Bar\nMeasureName=Nope\n\
         [x]\nMeasure=Bogus\n",
    );
    (d, env)
}

#[test]
fn fully_supported_skin() {
    let (_d, env) = setup();
    let r = check_config(&env, "good\\clock");
    assert_eq!(r.config, "Good\\Clock");
    assert!(r.fully_supported, "{r:?}");
    assert_eq!(r.to_string(), "Good\\Clock: Fully supported");
    let notes = r.notes.join("\n");
    assert!(notes.contains("UseD2D"), "{notes}");
    assert!(notes.contains("Blur"), "{notes}");
    assert!(notes.contains("!manage"), "{notes}");
    assert!(notes.contains("explorer.exe"), "{notes}");
    assert!(notes.contains("shell:Downloads"), "{notes}");
    assert!(r.unsupported_plugins.is_empty() && r.partial.is_empty());
}

#[test]
fn unknown_plugin_is_reported() {
    let (_d, env) = setup();
    let r = check_config(&env, "Plug\\Net");
    assert!(!r.fully_supported);
    assert_eq!(r.unsupported_plugins, vec!["HotKey.dll"]);
    assert!(r.partial.is_empty());
    assert_eq!(r.to_string(), "Plug\\Net: Unsupported plugins: HotKey.dll");
}

#[test]
fn unsupported_meter_and_options() {
    let (_d, env) = setup();
    let r = check_config(&env, "Bad\\Cfg");
    assert!(!r.fully_supported);
    assert!(r.partial.contains(&"Meter type \"Waveform\"".to_string()), "{:?}", r.partial);
    assert!(r.partial.contains(&"Bang !foobang".to_string()), "{:?}", r.partial);
    assert!(r.partial.contains(&"Measure type \"Bogus\"".to_string()), "{:?}", r.partial);
    assert!(r.partial.iter().any(|p| p.contains("MeasureName \"Nope\"")), "{:?}", r.partial);
    let line = r.to_string();
    assert!(line.starts_with("Bad\\Cfg: Partially supported (missing: "), "{line}");
}

#[test]
fn check_path_accepts_names_roots_and_ini_files() {
    let (d, env) = setup();
    let by_name = check_path(&env, "Good\\Clock").unwrap();
    assert_eq!(by_name.len(), 1);
    let root = check_path(&env, "good").unwrap();
    assert_eq!(root.len(), 1);
    assert_eq!(root[0].config, "Good\\Clock");
    let ini = env.skins_dir.join("Plug/Net/Net.ini");
    let by_ini = check_path(&env, ini.to_str().unwrap()).unwrap();
    assert_eq!(by_ini[0].config, "Plug\\Net");
    // A folder outside the skins dir is checked in place.
    let outside = d.path().join("elsewhere/MySkin/Sub");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("Sub.ini"), "[m]\nMeasure=Time\n[t]\nMeter=String\n").unwrap();
    let r = check_path(&env, d.path().join("elsewhere/MySkin").to_str().unwrap()).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].config, "MySkin\\Sub");
    assert!(r[0].fully_supported);
    assert!(check_path(&env, "No\\Such").is_err());
    assert!(!check_config(&env, "No\\Such").fully_supported);
}

#[test]
fn disabled_hidden_and_template_references_are_only_notes() {
    let d = tempfile::tempdir().unwrap();
    let env = Environment::with_root(d.path());
    skin(
        &env,
        "Q/Main/Main.ini",
        "[Rainmeter]\n[mOff]\nMeasure=Plugin\nPlugin=HotKey.dll\nDisabled=1\n[mOn]\nMeasure=Plugin\nPlugin=Mystery.dll\n\
         [Hid]\nMeter=Waveform\nHidden=1\n[Shown]\nMeter=String\nText=x\n",
    );
    skin(&env, "Q/Main/Item1.ini", "[Rainmeter]\n[x]\nMeasure=Plugin\nPlugin=Other.dll\n[y]\nMeter=Waveform\n");
    let r = check_config(&env, "Q\\Main");
    assert_eq!(r.unsupported_plugins, vec!["Mystery.dll"], "{r:?}");
    assert!(r.partial.is_empty(), "{:?}", r.partial);
    let notes = r.notes.join("\n");
    assert!(notes.contains("HotKey.dll") && notes.contains("Waveform") && notes.contains("Other.dll"), "{notes}");
}

#[test]
fn new_native_plugins_count_as_supported() {
    let d = tempfile::tempdir().unwrap();
    let env = Environment::with_root(d.path());
    let mut ini = String::from("[Rainmeter]\n");
    for (i, p) in [
        "WebNowPlaying.dll", "GPMDPPlugin", "EasyBlur", "FrostedGlass.dll", "Chameleon", "SysColor", "MeterialYou", "Process", "UsageMonitor",
        "FolderInfo", "FileView", "ActionTimer", "ConfigActive", "RunCommand", "Mouse",
        "CoreTemp", "SpeedFanPlugin", "HWiNFO.dll", "PingPlugin", "WifiStatus",
    ]
    .iter()
    .enumerate()
    {
        ini.push_str(&format!("[m{i}]\nMeasure=Plugin\nPlugin={p}\n"));
    }
    ini.push_str("[t]\nMeter=String\nText=x\nLeftMouseUpAction=[!RefreshGroup A][!ZPosGroup 1 A][!HideFadeGroup B]\n");
    skin(&env, "N/All/All.ini", &ini);
    let r = check_config(&env, "N\\All");
    assert!(r.fully_supported, "{r:?}");
}
