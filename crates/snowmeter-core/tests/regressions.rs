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

// ------------------------------------------------------------ review-A fixes

use snowmeter_core::skin::Skin;

struct Fx {
    _tmp: tempfile::TempDir,
    env: Environment,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let env = Environment::with_root(tmp.path());
        std::fs::create_dir_all(&env.skins_dir).unwrap();
        Fx { _tmp: tmp, env }
    }
    fn dir(&self, name: &str) -> std::path::PathBuf {
        let d = self.env.skins_dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn skin(&self, name: &str, ini: &str) -> Skin {
        std::fs::write(self.dir(name).join("Main.ini"), ini).unwrap();
        Skin::load(&self.env, name, "Main.ini").unwrap_or_else(|e| panic!("load {name}: {e}"))
    }
}

fn write_png(path: &Path, w: u32, h: u32) {
    let mut img = image::RgbaImage::new(w, h);
    for p in img.pixels_mut() {
        *p = image::Rgba([10, 200, 30, 255]);
    }
    img.save(path).unwrap();
}

/// C1: skin-controlled sizes must be clamped; drawing must not abort.
#[test]
fn c1_huge_skin_size_is_clamped() {
    let fx = Fx::new();
    for (i, ini) in [
        "[Rainmeter]\nSkinWidth=100000\nSkinHeight=100000\n[S]\nMeter=String\nText=x\n",
        "[S]\nMeter=String\nText=x\nX=40000\nY=2000000000\n",
        "[S]\nMeter=Image\nSolidColor=1,2,3\nW=2000000000\nH=10\n",
    ]
    .iter()
    .enumerate()
    {
        let s = fx.skin(&format!("C1_{i}"), ini);
        let (w, h) = s.size();
        assert!((0..=32767).contains(&w) && (0..=32767).contains(&h), "case {i}: size {w}x{h}");
        let _ = s.render_to_surface();
    }
}

/// C2: bang recursion from update actions must not overflow the stack.
#[test]
fn c2_bang_recursion_is_bounded() {
    let fx = Fx::new();
    let mut a = fx.skin("C2a", "[Rainmeter]\nOnUpdateAction=[!Update]\n[S]\nMeter=String\nText=x\n");
    a.tick();
    let mut b = fx.skin("C2b", "[M]\nMeasure=Calc\nFormula=1\nOnUpdateAction=[!UpdateMeasure M]\n[S]\nMeter=String\nText=x\n");
    b.tick();
    let mut c = fx.skin(
        "C2c",
        "[M]\nMeasure=Calc\nFormula=1\nIfCondition=M = 1\nIfTrueAction=[!UpdateMeasure M]\nIfConditionMode=1\n[S]\nMeter=String\nText=x\n",
    );
    c.tick();
    // Fan-out: many measures that all re-run the whole update.
    let mut ini = String::new();
    for i in 0..12 {
        ini += &format!("[M{i}]\nMeasure=Calc\nFormula=1\nOnUpdateAction=[!Update][!UpdateMeasure M{}]\n", (i + 1) % 12);
    }
    ini += "[S]\nMeter=String\nText=x\n";
    let mut d = fx.skin("C2d", &ini);
    d.tick();
    assert!(d.log_messages().iter().any(|m| m.contains("nesting")), "{:?}", d.log_messages());
}

/// C3: a huge ImageCrop must not overflow or allocate gigabytes.
#[test]
fn c3_huge_image_crop() {
    let fx = Fx::new();
    let d = fx.dir("C3");
    write_png(&d.join("a.png"), 8, 8);
    let mut s = fx.skin("C3", "[I]\nMeter=Image\nImageName=a.png\nImageCrop=0,0,50000,50000\n");
    s.tick();
    let mut s2 = fx.skin("C3", "[I]\nMeter=Image\nImageName=a.png\nImageCrop=2147483647,2147483647,2147483647,10\n");
    s2.tick();
    let _ = s.render_to_surface();
}

/// I1: self-including files must not hang (2^100 reads).
#[test]
fn i1_include_fan_out_terminates() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let fx = Fx::new();
        let s = fx.skin("I1", "[Rainmeter]\n@Include=Main.ini\n@Include2=Main.ini\n[S]\nMeter=String\nText=ok\n");
        let _ = tx.send(s.meter_text("S"));
    });
    let r = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("loading hung");
    assert_eq!(r.as_deref(), Some("ok"));
}

/// I2: absurd AverageSize / NumOfDecimals are capped.
#[test]
fn i2_average_size_and_decimals_capped() {
    let fx = Fx::new();
    let mut s = fx.skin(
        "I2",
        "[M]\nMeasure=Calc\nFormula=1/3\nAverageSize=2000000000\n[S]\nMeter=String\nMeasureName=M\nNumOfDecimals=1000000000\n",
    );
    s.tick();
    assert!(s.meter_text("S").unwrap().len() < 100);
}

/// I4: hiding a meter must not resize a fixed-size window.
#[test]
fn i4_resize_only_with_dynamic_window_size() {
    let fx = Fx::new();
    let ini = |dynamic: &str| {
        format!("[Rainmeter]\n{dynamic}\n[A]\nMeter=String\nText=a\nW=50\nH=20\n[B]\nMeter=String\nText=b\nW=50\nH=20\nY=100\n")
    };
    let mut s = fx.skin("I4a", &ini(""));
    let before = s.size();
    s.execute("[!HideMeter B]");
    assert_eq!(s.size(), before);
    let mut d = fx.skin("I4b", &ini("DynamicWindowSize=1"));
    let before = d.size();
    d.execute("[!HideMeter B]");
    assert!(d.size().1 < before.1);
}

/// I5: `!SetOption W` takes effect on the layout right away.
#[test]
fn i5_set_option_resizes_meter() {
    let fx = Fx::new();
    let mut s = fx.skin("I5", "[A]\nMeter=String\nText=a\nW=50\nH=20\nUpdateDivider=-1\n");
    s.execute("[!SetOption A W 200]");
    assert_eq!(s.meter_base("A").unwrap().w, 200.0);
}

/// I6: the resolved image path is cached across updates.
#[test]
fn i6_image_path_resolution_is_cached() {
    let fx = Fx::new();
    let d = fx.dir("I6");
    write_png(&d.join("bar.png"), 4, 4);
    let mut s = fx.skin("I6", "[I]\nMeter=Image\nImageName=BAR.PNG\n");
    let before = snowmeter_core::paths::dir_scans();
    for _ in 0..30 {
        s.execute("[!UpdateMeter I]");
        s.tick();
    }
    assert!(snowmeter_core::paths::dir_scans() - before <= 1, "{}", snowmeter_core::paths::dir_scans() - before);
    assert_eq!(s.meter_base("I").unwrap().w, 4.0);
}

/// I7: a tick where no meter changed does not invalidate the frame.
#[test]
fn i7_no_redraw_when_nothing_changed() {
    let fx = Fx::new();
    let mut s = fx.skin("I7", "[S]\nMeter=String\nText=static\n");
    let _ = s.take_dirty();
    s.tick();
    s.tick();
    let mut t = fx.skin("I7b", "[S]\nMeter=String\nText=static\nUpdateDivider=-1\n");
    let _ = t.take_dirty();
    t.tick();
    assert!(!t.take_dirty(), "frame invalidated although no meter updated");
}

/// Minor review items: overflow-prone options must not panic.
#[test]
fn minors_extreme_values_do_not_panic() {
    let fx = Fx::new();
    let mut s = fx.skin(
        "Minors",
        "[T]\nMeasure=Time\nTimeZone=-1e300\n[U]\nMeasure=Uptime\nFormat=%1!99999999999999999999d!\n\
         [S]\nMeter=String\nText=x\nW=2147483647\nPadding=10,10,10,10\n",
    );
    s.tick();
    let _ = s.render_to_surface();
}
