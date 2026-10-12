//! Group bangs, bang argument parsing and inline script calls in bang arguments.

use snowmeter_core::env::Environment;
use snowmeter_core::skin::{AppRequest, Skin};

fn setup() -> (tempfile::TempDir, Environment) {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    std::fs::create_dir_all(&env.skins_dir).unwrap();
    (tmp, env)
}

fn load(env: &Environment, name: &str, ini: &str) -> Skin {
    let dir = env.skins_dir.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Main.ini"), ini).unwrap();
    Skin::load(env, name, "Main.ini").unwrap()
}

#[test]
fn config_group_bangs_become_group_requests() {
    let (_t, env) = setup();
    let mut s = load(&env, "G", "[Rainmeter]\nGroup=A | B\n[S]\nMeter=String\nText=x\n");
    s.execute(
        "[!RefreshGroup A][!RedrawGroup A][!UpdateGroup B][!DeactivateConfigGroup A][!ShowGroup A][!HideGroup A]\
         [!ToggleGroup A][!ShowFadeGroup A][!HideFadeGroup A][!ToggleFadeGroup A][!ZPosGroup 1 A][!SetTransparencyGroup 128 A]\
         [!DraggableGroup 0 A][!ClickThroughGroup 1 A][!KeepOnScreenGroup 1 A][!SnapEdgesGroup 1 A]",
    );
    let r = s.take_requests();
    let g = || "Group=A".to_string();
    assert!(r.contains(&AppRequest::Refresh { config: Some(g()) }), "{r:?}");
    assert!(r.contains(&AppRequest::BangOther { config: g(), actions: "[!Redraw]".into() }));
    assert!(r.contains(&AppRequest::BangOther { config: "Group=B".into(), actions: "[!Update]".into() }));
    assert!(r.contains(&AppRequest::Deactivate { config: g() }));
    assert_eq!(r.iter().filter(|q| **q == AppRequest::Show { config: g() }).count(), 2);
    assert_eq!(r.iter().filter(|q| **q == AppRequest::Hide { config: g() }).count(), 2);
    assert_eq!(r.iter().filter(|q| **q == AppRequest::ToggleVisible { config: g() }).count(), 2);
    assert!(r.contains(&AppRequest::ZPos { config: g(), value: 1 }));
    assert!(r.contains(&AppRequest::SetTransparency { config: g(), alpha: 128 }));
    assert!(r.contains(&AppRequest::SetDraggable { config: g(), value: 0 }));
    assert!(r.contains(&AppRequest::SetClickThrough { config: g(), value: 1 }));
    assert_eq!(r.len(), 14, "{r:?}");
    assert_eq!(s.groups(), ["A", "B"]);
}

#[test]
fn group_bang_without_group_does_not_panic() {
    let (_t, env) = setup();
    let mut s = load(&env, "G2", "[S]\nMeter=String\n");
    s.execute("[!ZPosGroup 1][!RefreshGroup][!ShowGroup]");
    assert!(s.take_requests().is_empty());
}

#[test]
fn meter_and_measure_group_bangs_work() {
    let (_t, env) = setup();
    let mut s = load(
        &env,
        "G3",
        "[M]\nMeasure=Calc\nFormula=1\nGroup=Mg\n[A]\nMeter=String\nText=a\nGroup=Mt\n[B]\nMeter=String\nText=b\nGroup=Mt\n",
    );
    s.tick();
    s.execute("[!HideMeterGroup Mt]");
    s.execute("[!ShowMeterGroup Mt]");
    s.execute("[!DisableMeasureGroup Mg][!TogglePauseMeasureGroup Mg][!UpdateMeterGroup Mt][!UpdateMeasureGroup Mg]");
}

#[test]
fn write_key_value_file_with_spaces() {
    let (_t, env) = setup();
    let mut s = load(&env, "Fountain of Colors", "[Variables]\nA=1\n[S]\nMeter=String\n");
    let dir = env.skins_dir.join("Fountain of Colors").join("@Resources");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("My Vars.inc");
    std::fs::write(&f, "[Variables]\nA=1\n").unwrap();
    // Quoted.
    s.execute("[!WriteKeyValue Variables A 2 \"#@#My Vars.inc\"]");
    assert!(std::fs::read_to_string(&f).unwrap().contains("A=2"));
    // Quoted absolute path from a variable expanding to a path with spaces.
    let abs = f.to_string_lossy().to_string();
    s.execute(&format!("[!WriteKeyValue Variables A 3 \"{abs}\"]"));
    assert!(std::fs::read_to_string(&f).unwrap().contains("A=3"));
    // Unquoted last argument containing spaces.
    s.execute("[!WriteKeyValue Variables A 4 #@#My Vars.inc]");
    assert!(std::fs::read_to_string(&f).unwrap().contains("A=4"));
    s.execute(&format!("[!WriteKeyValue Variables A 5 {abs}]"));
    assert!(std::fs::read_to_string(&f).unwrap().contains("A=5"));
}

#[test]
fn inline_script_call_in_bang_argument() {
    let (_t, env) = setup();
    let dir = env.skins_dir.join("Sd");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Main.ini"),
        "[Variables]\nV=7\n[Sc]\nMeasure=Script\nScriptFile=s.lua\n[Src]\nMeasure=Calc\nFormula=1234\n\
         [T]\nMeter=String\nText=none\n",
    )
    .unwrap();
    std::fs::write(dir.join("s.lua"), "function Update() return 0 end\nfunction Fmt(n, d, u) return n .. '/' .. d .. '/' .. u end\n").unwrap();
    let mut s = Skin::load(&env, "Sd", "Main.ini").unwrap();
    s.tick();
    s.execute("[!SetOption T Text \"[&Sc:Fmt([&Src],3,'1k')]\"]");
    s.tick();
    assert_eq!(s.meter_text("T").as_deref(), Some("1234/3/1k"));
    // Same call with spaces and no quotes around the whole argument.
    s.execute("[!SetOption T Text \"[&Sc:Fmt(5, 6, 'x y')]\"]");
    s.tick();
    assert_eq!(s.meter_text("T").as_deref(), Some("5/6/x y"));
    s.execute("[!SetOption T Text [&Sc:Fmt([&Src],9,'z')]]");
    s.tick();
    assert_eq!(s.meter_text("T").as_deref(), Some("1234/9/z"));
    // A call to an undefined function leaves the text alone and does not panic.
    s.execute("[!SetOption T Text \"[&Sc:Nope(1)]\"]");
}
