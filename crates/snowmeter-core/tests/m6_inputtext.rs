use snowmeter_core::env::Environment;
use snowmeter_core::skin::{AppRequest, Skin};
use std::path::Path;

#[test]
fn input_text_emits_request() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    let dst = env.skins_dir.join("Input");
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m6/Input/Input.ini"), dst.join("Input.ini")).unwrap();
    let mut skin = Skin::load(&env, "Input", "Input.ini").unwrap();
    skin.take_requests();
    skin.execute("[!CommandMeasure mInput \"ExecuteBatch 2\"]");
    let reqs = skin.take_requests();
    let spec = reqs
        .iter()
        .find_map(|r| match r {
            AppRequest::InputText { config, spec } => {
                assert_eq!(config, "Input");
                Some(spec.clone())
            }
            _ => None,
        })
        .expect("InputText request");
    assert_eq!((spec.x, spec.y, spec.w, spec.h), (10, 20, 100, 18));
    assert_eq!(spec.default_value, "hi");
    assert!(spec.focus_dismiss);
    assert_eq!(spec.commands.len(), 1);
    assert!(spec.commands[0].contains("$UserInput$"));
    skin.execute("[!CommandMeasure mInput \"ExecuteBatch ALL\"]");
    let reqs = skin.take_requests();
    assert!(matches!(&reqs[0], AppRequest::InputText { spec, .. } if spec.commands.len() == 2));
}

#[test]
fn input_text_is_native() {
    assert!(snowmeter_core::measures::plugin::is_native_plugin("InputText"));
}
