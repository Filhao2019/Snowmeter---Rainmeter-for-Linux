//! Timer-driven and registry-driven plugins exercised through a loaded skin.

use snowmeter_core::env::Environment;
use snowmeter_core::skin::Skin;
use std::time::Duration;

fn load(ini: &str) -> (tempfile::TempDir, Skin) {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    let dir = env.skins_dir.join("Bp");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Main.ini"), ini).unwrap();
    let s = Skin::load(&env, "Bp", "Main.ini").unwrap();
    (tmp, s)
}

#[test]
fn action_timer_runs_through_the_skin() {
    let (_t, mut s) = load(
        "[Variables]\nV=0\n[M]\nMeasure=Plugin\nPlugin=ActionTimer\nActionList1=A | Wait 40 | B\nA=[!SetVariable V 1]\nB=[!SetVariable V 2]\n\
         [T]\nMeter=String\nText=#V#\nDynamicVariables=1\n",
    );
    s.tick();
    assert!(s.next_delay_ms().is_none());
    s.execute("[!CommandMeasure M \"Execute 1\"]");
    assert!(s.next_delay_ms().is_some());
    s.run_delayed();
    s.tick();
    assert_eq!(s.meter_text("T").as_deref(), Some("1"));
    assert!(s.next_delay_ms().is_some());
    std::thread::sleep(Duration::from_millis(60));
    s.run_delayed();
    s.tick();
    assert_eq!(s.meter_text("T").as_deref(), Some("2"));
    assert!(s.next_delay_ms().is_none());
    // Stop cancels a pending list.
    s.execute("[!CommandMeasure M \"Execute 1\"][!CommandMeasure M \"Stop 1\"]");
    assert!(s.next_delay_ms().is_none());
}

#[test]
fn config_active_reads_the_published_lists() {
    let (_t, mut s) = load(
        "[A]\nMeasure=Plugin\nPlugin=ConfigActive.dll\nConfigName=CaOn\\Skin\n[B]\nMeasure=Plugin\nPlugin=ConfigActive\nConfigName=CaOff\n\
         [C]\nMeasure=Plugin\nPlugin=ConfigActive\nConfigName=CaNone\n[T]\nMeter=String\nText=x\n",
    );
    snowmeter_core::active::publish(["CaOn\\Skin", "CaOff"], ["caon\\skin"]);
    s.tick();
    assert_eq!(s.measure_value("A"), Some(1.0));
    assert_eq!(s.measure_value("B"), Some(0.0));
    assert_eq!(s.measure_value("C"), Some(-1.0));
}

#[test]
fn process_measure_in_a_skin() {
    let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
    let (_t, mut s) = load(&format!(
        "[P]\nMeasure=Plugin\nPlugin=Process\nProcessName={}.exe\n[Q]\nMeasure=Plugin\nPlugin=Process\nProcessName=definitely-not-running.exe\n[T]\nMeter=String\nText=x\n",
        comm.trim()
    ));
    s.tick();
    assert_eq!(s.measure_value("P"), Some(1.0));
    assert_eq!(s.measure_value("Q"), Some(-1.0));
}
