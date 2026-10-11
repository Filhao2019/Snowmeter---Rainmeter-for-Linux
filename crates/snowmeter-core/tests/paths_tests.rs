use snowmeter_core::paths::{find_case_insensitive, resolve_path};
use std::fs;
use std::path::Path;

fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Skin/@Resources/Images")).unwrap();
    fs::write(dir.path().join("Skin/@Resources/Images/Bg.png"), b"x").unwrap();
    fs::write(dir.path().join("Skin/main.ini"), b"").unwrap();
    dir
}

#[test]
fn backslashes_and_wrong_case_resolve() {
    let d = setup();
    let base = d.path().join("Skin");
    let want = base.join("@Resources/Images/Bg.png");
    assert_eq!(resolve_path(&base, "@Resources\\images\\BG.PNG"), want);
    assert_eq!(resolve_path(&base, "@resources/IMAGES/bg.png"), want);
    let abs = format!("{}/@RESOURCES\\images\\BG.PNG", base.display());
    assert_eq!(resolve_path(Path::new("/nonexistent"), &abs), want);
}

#[test]
fn hash_at_hash_style_input_after_substitution() {
    let d = setup();
    let base = d.path().join("Skin");
    let res = format!("{}/@Resources/", base.display());
    let value = format!("{res}Images\\bg.PNG");
    assert_eq!(resolve_path(&base, &value), base.join("@Resources/Images/Bg.png"));
}

#[test]
fn missing_file_keeps_requested_spelling() {
    let d = setup();
    let base = d.path().join("Skin");
    assert_eq!(resolve_path(&base, "@Resources\\Nope\\x.png"), base.join("@Resources/Nope/x.png"));
    assert_eq!(resolve_path(&base, "nofile.png"), base.join("nofile.png"));
}

#[test]
fn dotdot_components() {
    let d = setup();
    let base = d.path().join("Skin/@Resources");
    assert_eq!(resolve_path(&base, "..\\MAIN.INI"), d.path().join("Skin/main.ini"));
    assert_eq!(resolve_path(&base, "Images\\..\\Images\\bg.png"), base.join("Images/Bg.png"));
}

#[test]
fn empty_and_leading_separator() {
    let d = setup();
    let base = d.path().join("Skin");
    assert_eq!(resolve_path(&base, ""), std::path::PathBuf::new());
    // "\foo" is relative in Rainmeter, not absolute
    assert_eq!(resolve_path(&base, "\\foo"), base.join("foo"));
}

#[test]
fn find_case_insensitive_cases() {
    let d = setup();
    let p = d.path().join("skin/@RESOURCES/images/bg.PNG");
    assert_eq!(find_case_insensitive(&p), Some(d.path().join("Skin/@Resources/Images/Bg.png")));
    assert_eq!(find_case_insensitive(&d.path().join("Skin/@Resources/Images/Bg.png")), Some(d.path().join("Skin/@Resources/Images/Bg.png")));
    assert_eq!(find_case_insensitive(&d.path().join("Skin/missing.png")), None);
}
