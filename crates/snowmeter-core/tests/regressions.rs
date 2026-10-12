//! Regression tests for fixes (fix-A report).

use snowmeter_core::env::Environment;
use std::path::Path;

fn px(img: &image::RgbaImage, x: u32, y: u32) -> [u8; 4] {
    img.get_pixel(x, y).0
}

/// BackgroundMode=3 with margins, root given as a RELATIVE path (as the
/// `render_png` example is run). `#@#` used to expand to a path that was joined
/// onto the skin dir a second time, so the background silently failed to load.
#[test]
fn background_mode3_with_relative_root() {
    let env = Environment::with_root(Path::new("tests/fixtures/bg"));
    assert!(env.skins_dir.is_absolute());
    let out = tempfile::tempdir().unwrap().keep().join("bg.png");
    snowmeter_core::render_skin_png(&env, "Bg", None, 1, &out).unwrap();
    let img = image::open(&out).unwrap().to_rgba8();
    let h = img.height();
    assert!(img.width() == 20 && h >= 30, "size {}x{h}", img.width());
    assert_eq!(px(&img, 10, 0), [255, 0, 0, 255], "top margin row");
    assert_eq!(px(&img, 10, 1), [255, 0, 0, 255]);
    assert_eq!(px(&img, 3, h / 2), [0, 255, 0, 255], "stretched middle");
    assert_eq!(px(&img, 19, h / 2 + 1), [0, 255, 0, 255]);
    assert_eq!(px(&img, 10, h - 1), [0, 0, 255, 255], "bottom margin row");
}
