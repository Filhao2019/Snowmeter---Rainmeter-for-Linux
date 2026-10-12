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

/// illustro Network: the IP line is a WebParser measure with `Substitute="":"N/A"`;
/// before the first result it must show N/A, not the numeric fallback "0".
#[test]
fn webparser_empty_result_uses_substitute() {
    let fx = Fx::new();
    let s = fx.skin(
        "Net",
        "[ip]\nMeasure=WebParser\nURL=http://127.0.0.1:9/\nUpdateRate=14400\nRegExp=(?s)^(.*)$\nStringIndex=1\nSubstitute=\"\":\"N/A\"\n\
         [S]\nMeter=String\nMeasureName=ip\nText=%1\n",
    );
    assert_eq!(s.meter_text("S").as_deref(), Some("N/A"));
}

#[test]
fn mouse_action_bangs() {
    use snowmeter_core::skin::{MouseButton, MouseEvent};
    let fx = Fx::new();
    let mut s = fx.skin(
        "Mouse",
        "[A]\nMeter=String\nText=a\nW=50\nH=20\nSolidColor=0,0,0,255\nGroup=G\nLeftMouseUpAction=[!SetVariable Hit yes]\n",
    );
    let click = |s: &mut Skin| {
        s.mouse(MouseEvent::Down { x: 5.0, y: 5.0, button: MouseButton::Left });
        s.mouse(MouseEvent::Up { x: 5.0, y: 5.0, button: MouseButton::Left });
    };
    s.execute("[!DisableMouseAction A]");
    click(&mut s);
    assert_eq!(s.parser().get_variable("Hit"), None);
    s.execute("[!EnableMouseActionGroup G]");
    click(&mut s);
    assert_eq!(s.parser().get_variable("Hit").as_deref(), Some("yes"));
    s.execute("[!SetVariable Hit no][!ClearMouseAction A]");
    click(&mut s);
    assert_eq!(s.parser().get_variable("Hit").as_deref(), Some("no"));
}

/// Container: contained meters are offset from the container, clipped to its
/// drawn alpha, excluded from window sizing and from the outer relative chain.
#[test]
fn container_clips_and_positions() {
    let fx = Fx::new();
    let ini = "[K]\nMeter=Image\nX=10\nY=10\nW=20\nH=20\nSolidColor=0,0,0,255\n\
        [I1]\nMeter=Image\nContainer=K\nX=5\nY=5\nW=100\nH=100\nSolidColor=255,0,0,255\n\
        [I2]\nMeter=Image\nContainer=K\nX=0r\nY=0r\nW=10\nH=10\nSolidColor=0,255,0,255\n\
        [N]\nMeter=Image\nX=0R\nY=40\nW=5\nH=5\nSolidColor=0,0,255,255\n";
    let s = fx.skin("Cont", ini);
    let b = |n: &str| {
        let m = s.meter_base(n).unwrap();
        (m.x, m.y)
    };
    assert_eq!(b("I1"), (15.0, 15.0));
    assert_eq!(b("I2"), (15.0, 15.0));
    assert_eq!(b("N"), (30.0, 40.0), "outer chain skips contained meters");
    assert_eq!(s.size(), (35, 45), "contained meters do not size the window");
    let png = fx._tmp.path().join("c.png");
    s.render_to_surface().unwrap().write_to_png(&mut std::fs::File::create(&png).unwrap()).unwrap();
    let img = image::open(&png).unwrap().to_rgba8();
    assert_eq!(px(&img, 20, 20)[3], 255);
    assert_eq!(px(&img, 16, 16), [0, 255, 0, 255], "I2 drawn over I1 inside the container");
    assert_eq!(px(&img, 25, 25), [255, 0, 0, 255], "I1 inside the container");
    assert_eq!(px(&img, 31, 15)[3], 0, "I1 clipped outside the container rect");
    assert_eq!(px(&img, 12, 12)[3], 0, "container itself is not drawn, only masks");
}

mod final_review_limits {
    use snowmeter_core::env::Environment;
    use snowmeter_core::skin::Skin;

    fn run(ini: &str, extra: &[(&str, Vec<u8>)]) -> Skin {
        let tmp = tempfile::tempdir().unwrap();
        let env = Environment::with_root(tmp.path());
        let dir = env.skins_dir.join("S");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Main.ini"), ini).unwrap();
        for (n, b) in extra {
            std::fs::write(dir.join(n), b).unwrap();
        }
        let mut s = Skin::load(&env, "S", "Main.ini").unwrap();
        s.tick();
        s.tick();
        let _ = s.render_to_surface();
        std::mem::forget(tmp);
        s
    }

    fn png() -> Vec<u8> {
        let mut v = Vec::new();
        let img = image::RgbaImage::new(4, 4);
        img.write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).unwrap();
        v
    }

    #[test]
    fn huge_line_count_is_clamped() {
        let t = std::time::Instant::now();
        run("[M]\nMeasure=Calc\nFormula=1\n[L]\nMeter=Line\nMeasureName=M\nW=100\nH=50\nLineCount=2000000000\n", &[]);
        assert!(t.elapsed().as_secs() < 5);
    }

    #[test]
    fn huge_line_width_is_clamped() {
        run("[M]\nMeasure=Calc\nFormula=1\n[L]\nMeter=Line\nMeasureName=M\nW=2000000000\nH=50\n", &[]);
    }

    #[test]
    fn huge_histogram_width_is_clamped() {
        run(
            "[M]\nMeasure=Calc\nFormula=1\n[H]\nMeter=Histogram\nMeasureName=M\nW=2000000000\nH=50\nPrimaryImage=p.png\n",
            &[("p.png", png())],
        );
    }

    #[test]
    fn huge_bitmap_counts_are_clamped() {
        let t = std::time::Instant::now();
        run(
            "[M]\nMeasure=Calc\nFormula=5\n[B]\nMeter=Bitmap\nMeasureName=M\nBitmapImage=p.png\nBitmapFrames=2000000000\nBitmapDigits=2000000000\nBitmapTransitionFrames=2000000000\nBitmapExtend=1\n",
            &[("p.png", png())],
        );
        assert!(t.elapsed().as_secs() < 5);
    }
}

#[test]
fn empty_bin_requests_confirmation_and_silent_does_not() {
    use snowmeter_core::env::Environment;
    use snowmeter_core::skin::{AppRequest, Skin};
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    let dir = env.skins_dir.join("S");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Main.ini"), "[R]\nMeasure=Plugin\nPlugin=RecycleManager\n[T]\nMeter=String\nText=x\n").unwrap();
    let mut s = Skin::load(&env, "S", "Main.ini").unwrap();
    s.tick();
    s.take_requests();
    s.execute("[!CommandMeasure R \"EmptyBin\"]");
    assert!(matches!(s.take_requests().as_slice(), [AppRequest::EmptyTrash { confirm: true }]));
    s.execute("[!CommandMeasure R \"EmptyBinSilent\"]");
    assert!(matches!(s.take_requests().as_slice(), [AppRequest::EmptyTrash { confirm: false }]));
}

/// Image `Path` without a trailing separator still joins with ImageName
/// (Rainmeter's MeterImage calls AppendBackslashIfMissing; Mond's weather icon relies on it).
#[test]
fn image_path_without_trailing_separator() {
    let fx = Fx::new();
    let d = fx.dir("ImgPath");
    std::fs::create_dir_all(d.join("Icons")).unwrap();
    write_png(&d.join("Icons").join("rain.png"), 6, 5);
    let mut s = fx.skin("ImgPath", "[I]\nMeter=Image\nPath=#CURRENTPATH#Icons\nImageName=rain\n");
    s.tick();
    let b = s.meter_base("I").unwrap();
    assert_eq!((b.w, b.h), (6.0, 5.0), "image under Path=…Icons was not found");
}
