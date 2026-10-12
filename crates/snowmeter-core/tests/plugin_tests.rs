//! Native plugin table through a loaded skin.

use snowmeter_core::env::Environment;
use snowmeter_core::skin::Skin;

#[test]
fn dll_names_redirect_to_native_measures() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    let dir = env.skins_dir.join("P");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Main.ini"),
        "[Bits]\nMeasure=Plugin\nPlugin=SysInfo.dll\nSysInfoType=OS_BITS\n\
         [Mon]\nMeasure=SysInfo\nSysInfoType=NUM_MONITORS\n\
         [Pct]\nMeasure=Plugin\nPlugin=PowerPlugin\nPowerState=PERCENT\n\
         [Unk]\nMeasure=Plugin\nPlugin=HotKey.dll\n\
         [T]\nMeter=String\nMeasureName=Bits\nText=%1\n",
    )
    .unwrap();
    let mut s = Skin::load(&env, "P", "Main.ini").unwrap();
    s.tick();
    assert_eq!(s.measure_value("Bits"), Some(64.0));
    assert!(s.measure_value("Mon").is_some());
    let pct = s.measure_value("Pct").unwrap();
    assert!((0.0..=100.0).contains(&pct));
    assert_eq!(s.measure_value("Unk"), Some(0.0));
}
