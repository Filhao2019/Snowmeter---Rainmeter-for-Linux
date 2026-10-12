//! Skin engine integration tests: meters, update loop, bangs, mouse, rendering.

use snowmeter_core::env::Environment;
use snowmeter_core::skin::{AppRequest, MouseButton, MouseEvent, Skin};
use snowmeter_core::render_skin_png;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m3").join(name)
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

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

    fn with_fixture(name: &str) -> Fx {
        let fx = Fx::new();
        copy_dir(&fixture(name), &fx.env.skins_dir.join(name));
        fx
    }

    /// Write `skins/<name>/Main.ini`.
    fn skin(&self, name: &str, ini: &str) {
        let dir = self.env.skins_dir.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Main.ini"), ini).unwrap();
    }

    fn load(&self, name: &str) -> Skin {
        Skin::load(&self.env, name, "Main.ini").unwrap_or_else(|e| panic!("load {name}: {e}"))
    }

    fn quick(&self, name: &str, ini: &str) -> Skin {
        self.skin(name, ini);
        self.load(name)
    }
}

fn pixel(img: &image::RgbaImage, x: u32, y: u32) -> [u8; 4] {
    img.get_pixel(x, y).0
}

fn pos(s: &Skin, name: &str) -> (f64, f64) {
    let b = s.meter_base(name).unwrap_or_else(|| panic!("no meter {name}"));
    (b.rect_x, b.rect_y)
}

#[test]
fn rich_skin_renders_png_with_expected_pixels() {
    let fx = Fx::with_fixture("Rich");
    let out = fx._tmp.path().join("out.png");
    render_skin_png(&fx.env, "Rich", None, 2, &out).unwrap();
    let img = image::open(&out).unwrap().to_rgba8();
    let skin = fx.load("Rich");
    let (w, h) = skin.size();
    assert_eq!((img.width() as i32, img.height() as i32), (w, h));
    assert!(w >= 60 && h >= 60 + 10);
    // Image meter (case-insensitive backslash path), 50x40 at (10,10).
    assert_eq!(pixel(&img, 12, 12), [255, 0, 0, 255]);
    assert_eq!(pixel(&img, 59, 49), [255, 0, 0, 255]);
    assert_eq!(pixel(&img, 5, 5)[3], 0, "outside every meter stays transparent");
    // String meter: solid background in a corner, blue glyph pixels inside.
    let tb = skin.meter_base("Text").unwrap();
    let right = (tb.rect_x + tb.w) as u32;
    assert_eq!(pixel(&img, right - 2, 61), [0, 255, 0, 255]);
    let mut blue = 0;
    for y in 60..(tb.rect_y + tb.h) as u32 {
        for x in 10..right {
            let p = pixel(&img, x, y);
            if p[2] > 200 && p[0] < 80 && p[1] < 80 {
                blue += 1;
            }
        }
    }
    assert!(blue > 30, "expected blue text pixels, found {blue}");
    assert_eq!(skin.meter_text("Text").as_deref(), Some("Value 42"));
}

#[test]
fn relative_positioning_vectors() {
    let fx = Fx::new();
    let s = fx.quick(
        "Rel",
        "[A]\nMeter=String\nX=10\nY=20\nW=30\nH=40\n\
         [B]\nMeter=String\nX=5R\nY=0r\nW=8\nH=8\n\
         [C]\nMeter=String\nX=0r\nY=10R\nW=8\nH=8\n\
         [D]\nMeter=String\nX=(10*2)R\nY=-5r\nW=4\nH=4\n\
         [E]\nMeter=String\nX=10.9\nY=3\nW=4\nH=4\n",
    );
    assert_eq!(pos(&s, "A"), (10.0, 20.0));
    assert_eq!(pos(&s, "B"), (45.0, 20.0));
    assert_eq!(pos(&s, "C"), (45.0, 38.0));
    // R is relative to C's right edge: 45 + 8 + 20, r is relative to C's top.
    assert_eq!(pos(&s, "D"), (73.0, 33.0));
    assert_eq!(pos(&s, "E"), (10.0, 3.0));
}

#[test]
fn relative_to_hidden_meter_uses_zero_size() {
    let fx = Fx::new();
    let s = fx.quick(
        "Hid",
        "[A]\nMeter=String\nX=10\nW=30\nH=10\nHidden=1\n[B]\nMeter=String\nX=5R\nW=8\nH=8\n",
    );
    assert_eq!(pos(&s, "B").0, 15.0);
    // Hidden meters still count with width 0 for the window: 15 + 8.
    assert_eq!(s.size().0, 23);
}

#[test]
fn string_align_anchor_math() {
    let fx = Fx::new();
    let s = fx.quick(
        "Al",
        "[R]\nMeter=String\nX=100\nY=0\nW=50\nH=20\nStringAlign=Right\n\
         [Next]\nMeter=String\nX=0R\nY=0\nW=5\nH=5\n\
         [C]\nMeter=String\nX=100\nY=0\nW=51\nH=20\nStringAlign=Center\n\
         [RB]\nMeter=String\nX=0\nY=40\nW=10\nH=16\nStringAlign=RightBottom\n\
         [CC]\nMeter=String\nX=0\nY=100\nW=10\nH=21\nStringAlign=CenterCenter\n",
    );
    assert_eq!(pos(&s, "R").0, 50.0);
    // The anchor, not the aligned left edge, feeds relative positions: 100 + 50.
    // (Next follows [R], which is a right-aligned string.)
    assert_eq!(s.meter_base("Next").unwrap().x, 150.0);
    assert_eq!(pos(&s, "C").0, 75.0);
    assert_eq!(pos(&s, "RB").1, 24.0);
    assert_eq!(pos(&s, "CC"), (-5.0, 90.0));
}

#[test]
fn string_align_parsing_matches_vectors() {
    use snowmeter_core::meters::string::parse_align;
    use snowmeter_core::render::text::{HAlign as H, VAlign as V};
    assert_eq!(parse_align("centertop"), (H::Center, V::Top));
    assert_eq!(parse_align("RightCenter"), (H::Right, V::Center));
    assert_eq!(parse_align("LeftMiddle"), (H::Left, V::Top));
    assert_eq!(parse_align("Bottom"), (H::Left, V::Top));
}

#[test]
fn auto_scale_and_number_formatting_in_strings() {
    let fx = Fx::new();
    let cases = [
        ("1536", "AutoScale=1", "1.5 k"),
        ("1000", "AutoScale=1", "1000.0 "),
        ("1048576", "AutoScale=1", "1.0 M"),
        ("1000", "AutoScale=2", "1.0 k"),
        ("1500000", "AutoScale=2", "1.5 M"),
        ("2560", "AutoScale=1\nNumOfDecimals=0", "2 k"),
        ("3584", "AutoScale=1\nNumOfDecimals=0", "4 k"),
        ("50", "Scale=2", "25"),
        ("50", "Scale=2.0", "25.0"),
        ("3.14159", "NumOfDecimals=2", "3.14"),
    ];
    for (i, (value, opts, expected)) in cases.iter().enumerate() {
        let s = fx.quick(
            &format!("AS{i}"),
            &format!("[m]\nMeasure=Calc\nFormula={value}\n[s]\nMeter=String\nMeasureName=m\n{opts}\n"),
        );
        assert_eq!(s.meter_text("s").as_deref(), Some(*expected), "{value} with {opts}");
    }
    let s = fx.quick(
        "Pct",
        "[m]\nMeasure=Calc\nFormula=50\nMinValue=0\nMaxValue=200\n[s]\nMeter=String\nMeasureName=m\nPercentual=1\n",
    );
    assert_eq!(s.meter_text("s").as_deref(), Some("25"));
}

#[test]
fn text_placeholders_prefix_postfix_and_case() {
    let fx = Fx::new();
    let s = fx.quick(
        "Txt",
        "[a]\nMeasure=Calc\nFormula=1\n[b]\nMeasure=Calc\nFormula=2\n\
         [t1]\nMeter=String\nMeasureName=a\nMeasureName2=b\nText=%1 / %2\nPrefix=<\nPostfix=>\n\
         [t2]\nMeter=String\nMeasureName=nope\nText=%1\n\
         [t3]\nMeter=String\nText=hello world\nStringCase=Proper\n",
    );
    assert_eq!(s.meter_text("t1").as_deref(), Some("<1 / 2>"));
    assert_eq!(s.meter_text("t2").as_deref(), Some("%1"), "invalid MeasureName = no measure");
    assert_eq!(s.meter_text("t3").as_deref(), Some("Hello World"));
}

#[test]
fn set_variable_needs_dynamic_variables() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "Dyn",
        "[Variables]\nTxt=a\n\
         [Static]\nMeter=String\nText=#Txt#\n\
         [Dynamic]\nMeter=String\nText=#Txt#\nDynamicVariables=1\n\
         [m]\nMeasure=Calc\nFormula=7\nDynamicVariables=1\n[Sec]\nMeter=String\nText=[m]\nDynamicVariables=1\n",
    );
    assert_eq!(s.meter_text("Static").as_deref(), Some("a"));
    assert_eq!(s.meter_text("Dynamic").as_deref(), Some("a"));
    s.execute("[!SetVariable Txt b]");
    assert_eq!(s.meter_text("Dynamic").as_deref(), Some("a"), "nothing re-read before the next update");
    s.tick();
    assert_eq!(s.meter_text("Static").as_deref(), Some("a"));
    assert_eq!(s.meter_text("Dynamic").as_deref(), Some("b"));
    assert_eq!(s.meter_text("Sec").as_deref(), Some("7"), "section variable in a dynamic option");
}

#[test]
fn show_hide_toggle_meter() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "Vis",
        "[A]\nMeter=String\nX=0\nW=30\nH=10\n[B]\nMeter=String\nX=100\nW=20\nH=10\nHidden=1\n",
    );
    assert_eq!(s.size().0, 100, "hidden B counts with width 0");
    s.execute("[!ShowMeter B]");
    assert!(!s.meter_base("B").unwrap().hidden);
    assert_eq!(s.size().0, 120);
    s.execute("[!HideMeter B]");
    assert!(s.meter_base("B").unwrap().hidden);
    s.execute("[!ToggleMeter b]");
    assert!(!s.meter_base("B").unwrap().hidden);
    s.execute("!ToggleMeter B");
    assert!(s.meter_base("B").unwrap().hidden);
    // The hidden state survives a tick (it was written to the option).
    s.tick();
    assert!(s.meter_base("B").unwrap().hidden);
    assert!(s.take_requests().contains(&AppRequest::Resize));
}

#[test]
fn set_option_changes_a_meter() {
    let fx = Fx::new();
    let mut s = fx.quick("Opt", "[S]\nMeter=String\nText=old\n");
    s.execute("[!SetOption S Text \"new text\"][!UpdateMeter S]");
    assert_eq!(s.meter_text("S").as_deref(), Some("new text"));
}

#[test]
fn if_condition_action_runs_a_bang() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "If",
        "[m]\nMeasure=Calc\nFormula=Counter\nIfCondition=m >= 2\nIfTrueAction=[!SetVariable Hit yes]\n\
         [S]\nMeter=String\nText=x\n",
    );
    assert_eq!(s.parser().get_variable("Hit"), None, "first update: Counter is 1");
    s.tick();
    assert_eq!(s.parser().get_variable("Hit").as_deref(), Some("yes"));
}

#[test]
fn actions_expand_section_variables_at_execution() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "Sec",
        "[Variables]\nBangs=[!SetVariable Z 1]\n[m]\nMeasure=Calc\nFormula=42\n[S]\nMeter=String\nText=x\n",
    );
    s.execute("[!SetVariable X \"[m]\"][!SetVariable Y [m:/2]][#Bangs]");
    assert_eq!(s.parser().get_variable("X").as_deref(), Some("42"));
    assert_eq!(s.parser().get_variable("Y").as_deref(), Some("21"));
    assert_eq!(s.parser().get_variable("Z").as_deref(), Some("1"));
    s.execute("[!SetVariable F \"(3*4)\"]");
    assert_eq!(s.parser().get_variable("F").as_deref(), Some("12"));
}

#[test]
fn left_mouse_up_action_via_mouse() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "Mouse",
        "[Area]\nMeter=String\nX=10\nY=10\nW=40\nH=40\nSolidColor=0,0,0,1\n\
         LeftMouseUpAction=[!SetVariable Clicked \"$MouseX$,[$MouseY]\"]\n\
         [Ghost]\nMeter=String\nX=100\nY=10\nW=40\nH=40\nLeftMouseUpAction=[!SetVariable Ghost 1]\n",
    );
    let up = |x, y| MouseEvent::Up { button: MouseButton::Left, x, y };
    assert!(s.mouse(up(20.0, 30.0)));
    assert_eq!(s.parser().get_variable("Clicked").as_deref(), Some("10,20"));
    assert!(!s.mouse(up(2.0, 2.0)), "outside the meter");
    assert!(s.hit_mask_alpha(20.0, 30.0) > 0);
    // A fully transparent pixel is click-through even inside the meter rect.
    assert_eq!(s.hit_mask_alpha(110.0, 20.0), 0);
    assert!(!s.mouse(up(110.0, 20.0)));
    assert_eq!(s.parser().get_variable("Ghost"), None);
}

#[test]
fn mouse_over_and_leave_actions() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "Over",
        "[Area]\nMeter=String\nW=40\nH=40\nSolidColor=0,0,0,1\n\
         MouseOverAction=[!SetVariable State in]\nMouseLeaveAction=[!SetVariable State out]\n",
    );
    s.mouse(MouseEvent::Move { x: 5.0, y: 5.0 });
    assert_eq!(s.parser().get_variable("State").as_deref(), Some("in"));
    s.mouse(MouseEvent::Leave);
    assert_eq!(s.parser().get_variable("State").as_deref(), Some("out"));
}

#[test]
fn right_click_with_action_suppresses_menu_and_doubleclick_falls_back() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "Right",
        "[Area]\nMeter=String\nW=40\nH=40\nSolidColor=0,0,0,1\n\
         RightMouseDownAction=[!SetVariable R 1]\nLeftMouseDownAction=[!SetVariable L 1]\n",
    );
    assert!(s.mouse(MouseEvent::Up { button: MouseButton::Right, x: 5.0, y: 5.0 }));
    assert!(s.mouse(MouseEvent::DoubleClick { button: MouseButton::Left, x: 5.0, y: 5.0 }));
    assert_eq!(s.parser().get_variable("L").as_deref(), Some("1"));
}

#[test]
fn broken_skin_still_renders() {
    let fx = Fx::with_fixture("Broken");
    let out = fx._tmp.path().join("b.png");
    render_skin_png(&fx.env, "Broken", Some("Main.ini"), 1, &out).unwrap();
    let img = image::open(&out).unwrap().to_rgba8();
    assert_eq!(pixel(&img, 1, 31), [0, 255, 0, 255], "the good string meter is drawn");
    let s = fx.load("Broken");
    let log = s.log_messages().join("\n");
    assert!(log.contains("NoSuchMeasureType"), "{log}");
    assert!(log.contains("Hologram"), "{log}");
    assert!(log.contains("nothere.png"), "{log}");
    assert!(log.contains("DoesNotExist"), "{log}");
    assert_eq!(s.meter_text("Ok").as_deref(), Some("ok 2"));
    // Logged once, not once per tick.
    let mut s = s;
    let before = s.log_messages().len();
    s.tick();
    s.tick();
    assert_eq!(s.log_messages().len(), before);
}

#[test]
fn skin_without_meters_is_an_error_not_a_panic() {
    let fx = Fx::new();
    fx.skin("Empty", "[m]\nMeasure=Calc\n");
    assert!(Skin::load(&fx.env, "Empty", "Main.ini").is_err());
    assert!(Skin::load(&fx.env, "Missing", "Main.ini").is_err());
}

#[test]
fn utf16le_skin_file_loads() {
    let fx = Fx::new();
    let dir = fx.env.skins_dir.join("U16");
    std::fs::create_dir_all(&dir).unwrap();
    let text = "[Rainmeter]\r\nUpdate=1000\r\n[S]\r\nMeter=String\r\nText=Grüße\r\nX=3\r\nY=4\r\n";
    let mut bytes = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    std::fs::write(dir.join("Main.ini"), bytes).unwrap();
    let s = fx.load("U16");
    assert_eq!(s.meter_text("S").as_deref(), Some("Grüße"));
    assert_eq!(pos(&s, "S"), (3.0, 4.0));
}

fn write_png(path: &Path, w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba(f(x, y));
    }
    img.save(path).unwrap();
}

#[test]
fn image_meter_sizes_extension_and_options() {
    let fx = Fx::new();
    let dir = fx.env.skins_dir.join("Img");
    std::fs::create_dir_all(dir.join("pics")).unwrap();
    write_png(&dir.join("pics/wide.png"), 200, 100, |x, _| if x < 100 { [200, 100, 50, 255] } else { [0, 0, 255, 255] });
    std::fs::write(
        dir.join("Main.ini"),
        "[Natural]\nMeter=Image\nImageName=pics\\wide\n\
         [OnlyW]\nMeter=Image\nImageName=pics/WIDE.png\nW=50\nY=300\n\
         [Padded]\nMeter=Image\nImageName=pics/wide.png\nW=50\nPadding=5,5,5,5\nY=400\n\
         [Tint]\nMeter=Image\nImageName=pics/wide.png\nImageTint=255,128,0\nW=200\nH=100\nY=500\n\
         [Grey]\nMeter=Image\nImageName=pics/wide.png\nGreyscale=1\nW=200\nH=100\nY=700\n\
         [Crop]\nMeter=Image\nImageName=pics/wide.png\nImageCrop=-50,0,50,50,2\nY=900\n\
         [Rot]\nMeter=Image\nImageName=pics/wide.png\nImageRotate=90\nY=1000\n\
         [Fit]\nMeter=Image\nImageName=pics/wide.png\nW=100\nH=100\nPreserveAspectRatio=1\nSolidColor=255,255,255,255\nY=1300\n",
    )
    .unwrap();
    let s = fx.load("Img");
    let wh = |n: &str| {
        let b = s.meter_base(n).unwrap();
        (b.w, b.h)
    };
    assert_eq!(wh("Natural"), (200.0, 100.0), "missing extension gets .png");
    assert_eq!(wh("OnlyW"), (50.0, 25.0));
    assert_eq!(wh("Padded"), (60.0, 40.0));
    assert_eq!(wh("Crop"), (50.0, 50.0));
    assert_eq!(wh("Rot"), (100.0, 200.0));
    let img = s.render_to_surface();
    let png = fx._tmp.path().join("img.png");
    img.write_to_png(&mut std::fs::File::create(&png).unwrap()).unwrap();
    let img = image::open(&png).unwrap().to_rgba8();
    // Tint multiplies each channel: (200,100,50) * (1, 0.502, 0) = (200, 50, 0).
    let t = pixel(&img, 10, 510);
    assert!((t[0] as i32 - 200).abs() <= 1 && (t[1] as i32 - 50).abs() <= 1 && t[2] == 0, "{t:?}");
    // Greyscale (200,100,50) -> 0.299*200 + 0.587*100 + 0.114*50 = 124.2.
    let g = pixel(&img, 10, 710);
    assert!((g[0] as i32 - 124).abs() <= 1 && g[0] == g[1] && g[1] == g[2], "{g:?}");
    // Crop origin 2 (top-right), x=-50: the right-hand blue quarter.
    assert_eq!(pixel(&img, 10, 910), [0, 0, 255, 255]);
    // Fit letterboxes the 2:1 image into a square: the white SolidColor shows above it.
    assert_eq!(pixel(&img, 50, 1301), [255, 255, 255, 255]);
    assert_eq!(pixel(&img, 10, 1350), [200, 100, 50, 255]);
}

#[test]
fn image_follows_a_measure_value() {
    let fx = Fx::new();
    let dir = fx.env.skins_dir.join("ImgM");
    std::fs::create_dir_all(&dir).unwrap();
    write_png(&dir.join("img1.png"), 4, 4, |_, _| [255, 0, 0, 255]);
    write_png(&dir.join("img2.png"), 4, 4, |_, _| [0, 255, 0, 255]);
    std::fs::write(
        dir.join("Main.ini"),
        "[m]\nMeasure=Calc\nFormula=Counter\n[I]\nMeter=Image\nMeasureName=m\nImageName=img%1\n",
    )
    .unwrap();
    let mut s = fx.load("ImgM");
    let color = |s: &Skin| {
        let surf = s.render_to_surface();
        let p = fx._tmp.path().join("m.png");
        surf.write_to_png(&mut std::fs::File::create(&p).unwrap()).unwrap();
        image::open(&p).unwrap().to_rgba8().get_pixel(1, 1).0
    };
    assert_eq!(color(&s), [255, 0, 0, 255]);
    s.tick();
    assert_eq!(color(&s), [0, 255, 0, 255]);
}

fn liberation_installed() -> bool {
    snowmeter_core::render::fonts::FontResolver::new().is_installed("Liberation Sans")
}

#[test]
fn text_metrics_follow_the_directwrite_model() {
    if !liberation_installed() {
        eprintln!("Liberation Sans missing; skipping");
        return;
    }
    let fx = Fx::new();
    let size = |accurate: u8, text: &str, fs: u32| {
        let s = fx.quick(
            &format!("TM{accurate}{fs}{}", text.len()),
            &format!("[Rainmeter]\nAccurateText={accurate}\n[S]\nMeter=String\nText={text}\nFontFace=Liberation Sans\nFontSize={fs}\n"),
        );
        let b = s.meter_base("S").unwrap();
        (b.w, b.h)
    };
    // "Hello" at 10pt: accurate 31x15, GDI emulation 35x16 (+-1 for font table rounding).
    let (w, h) = size(1, "Hello", 10);
    assert!((w - 31.0).abs() <= 1.0 && (h - 15.0).abs() <= 1.0, "{w}x{h}");
    let (w, h) = size(0, "Hello", 10);
    assert!((w - 35.0).abs() <= 1.0 && (h - 16.0).abs() <= 1.0, "{w}x{h}");
    // Empty and whitespace-only strings have no size.
    assert_eq!(size(0, " ", 10), (0.0, 0.0));
    // Two lines are taller.
    let (_, h1) = size(1, "A", 10);
    let (_, h2) = size(1, "A#CRLF#B", 10);
    assert!(h2 > 1.9 * h1 - 2.0, "{h1} {h2}");
}

#[test]
fn string_text_draws_inside_its_box_and_alignment_moves_it() {
    if !liberation_installed() {
        return;
    }
    let fx = Fx::new();
    let left_most_ink = |align: &str| {
        let s = fx.quick(
            &format!("Ink{align}"),
            &format!("[S]\nMeter=String\nText=Hi\nFontFace=Liberation Sans\nFontSize=12\nFontColor=255,255,255\nX=100\nW=100\nH=30\nStringAlign={align}\nSolidColor=0,0,0,255\n"),
        );
        let p = fx._tmp.path().join(format!("ink{align}.png"));
        s.render_to_surface().write_to_png(&mut std::fs::File::create(&p).unwrap()).unwrap();
        let img = image::open(&p).unwrap().to_rgba8();
        let mut min_x = u32::MAX;
        let mut max_x = 0;
        for (x, _, px) in img.enumerate_pixels() {
            if px[0] > 128 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
        (min_x, max_x)
    };
    let (l0, _) = left_most_ink("Left");
    let (c0, c1) = left_most_ink("Center");
    let (_, r1) = left_most_ink("Right");
    // StringAlign moves the anchor, so a W=100 Right string is drawn left of X.
    assert!((100..112).contains(&l0), "left ink starts at the anchor: {l0}");
    assert!(c0 >= 90 && c1 <= 110, "centered on the anchor: {c0}..{c1}");
    assert!((90..=100).contains(&r1), "right aligned ink ends at the anchor: {r1}");
}

#[test]
fn inline_settings_and_clip_string() {
    if !liberation_installed() {
        return;
    }
    let fx = Fx::new();
    let s = fx.quick(
        "Inl",
        "[S]\nMeter=String\nText=Hello World\nFontFace=Liberation Sans\nFontSize=12\nFontColor=0,0,0\n\
         InlineSetting=Color | 255,0,0\nInlinePattern=World\nInlineSetting2=Weight | 700\nInlinePattern2=(Hel)lo\n\
         InlineSetting3=Case | Upper\nInlinePattern3=Hello\n\
         [C]\nMeter=String\nText=A very long line of text that must be wrapped\nFontFace=Liberation Sans\nFontSize=10\n\
         ClipString=2\nW=60\nH=40\nY=50\n",
    );
    assert_eq!(s.meter_text("S").as_deref(), Some("HELLO World"));
    let b = s.meter_base("C").unwrap();
    assert_eq!((b.w, b.h), (60.0, 40.0));
    // Render without panicking.
    let _ = s.render_to_surface();
}

#[test]
fn write_key_value_edits_the_skin_ini() {
    let fx = Fx::new();
    let mut s = fx.quick("Wkv", "[Variables]\nA=1\n[S]\nMeter=String\nText=x\n");
    s.execute("[!WriteKeyValue Variables A \"(2+3)\"][!WriteKeyValue Variables B hello]");
    let text = std::fs::read_to_string(s.ini_path()).unwrap();
    assert!(text.contains("A=5"), "{text}");
    assert!(text.contains("B=hello"), "{text}");
    // Path escapes are refused.
    s.execute("[!WriteKeyValue Variables C 1 \"..\\evil.ini\"]");
    assert!(!fx.env.skins_dir.join("evil.ini").exists());
}

#[test]
fn window_and_config_bangs_become_app_requests() {
    let fx = Fx::new();
    let mut s = fx.quick("Req", "[S]\nMeter=String\nW=100\nH=50\n");
    s.execute(
        "[!ActivateConfig \"illustro\\Clock\" Clock.ini][!Move 10 20][!ZPos 1][!Draggable 0][!SetTransparency 300][!Refresh]\
         [!DeactivateConfig][!SetVariable A 1 \"Other\\Skin\"][https://example.com]",
    );
    let r = s.take_requests();
    assert!(r.contains(&AppRequest::Activate { config: "illustro\\Clock".into(), file: Some("Clock.ini".into()) }), "{r:?}");
    assert!(r.contains(&AppRequest::Move { config: "Req".into(), x: 10.0, y: 20.0 }));
    assert!(r.contains(&AppRequest::ZPos { config: "Req".into(), value: 1 }));
    assert!(r.contains(&AppRequest::SetDraggable { config: "Req".into(), value: 0 }));
    assert!(r.contains(&AppRequest::SetTransparency { config: "Req".into(), alpha: 255 }));
    assert!(r.contains(&AppRequest::Refresh { config: Some("Req".into()) }));
    assert!(r.contains(&AppRequest::Deactivate { config: "Req".into() }));
    assert!(r.iter().any(|q| matches!(q, AppRequest::BangOther { config, .. } if config == "Other\\Skin")));
    assert!(r.contains(&AppRequest::OpenOrRun("https://example.com".into())));
    assert_eq!(s.parser().get_variable("A"), None, "forwarded to another skin, not applied here");
}

#[test]
fn set_window_position_uses_monitor_geometry() {
    let fx = Fx::new();
    let mut s = fx.quick("Pos", "[S]\nMeter=String\nW=200\nH=100\n");
    s.execute("[!SetWindowPosition 10R 25% 10 0]");
    let r = s.take_requests();
    // Primary monitor is 1920x1080: x = 1920 - 10 - 10, y = 25% of 1080.
    assert_eq!(r, vec![AppRequest::Move { config: "Pos".into(), x: 1900.0, y: 270.0 }]);
}

#[test]
fn skin_options_size_background_and_update() {
    let fx = Fx::new();
    let s = fx.quick(
        "Opts",
        "[Rainmeter]\nUpdate=250\nSkinWidth=300\nBackgroundMode=2\nSolidColor=10,20,30,255\nBackgroundMargins=5,6,7,8\n\
         [S]\nMeter=String\nX=0\nY=0\nW=40\nH=20\n",
    );
    assert_eq!(s.update_interval_ms(), 250);
    // Height: max(top margin, meter bottom) + bottom margin.
    assert_eq!(s.size(), (300, 20 + 8));
    let p = fx._tmp.path().join("o.png");
    s.render_to_surface().write_to_png(&mut std::fs::File::create(&p).unwrap()).unwrap();
    let img = image::open(&p).unwrap().to_rgba8();
    assert_eq!(pixel(&img, 299, 0), [10, 20, 30, 255]);
}

#[test]
fn update_divider_and_negative_divider() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "Div",
        "[m]\nMeasure=Calc\nFormula=Counter\n\
         [Every]\nMeter=String\nMeasureName=m\n\
         [Third]\nMeter=String\nMeasureName=m\nUpdateDivider=3\n\
         [Never]\nMeter=String\nMeasureName=m\nUpdateDivider=-1\n",
    );
    let t = |s: &Skin| (s.meter_text("Every").unwrap(), s.meter_text("Third").unwrap(), s.meter_text("Never").unwrap());
    assert_eq!(t(&s), ("1".into(), "1".into(), "1".into()));
    s.tick();
    assert_eq!(t(&s), ("2".into(), "1".into(), "1".into()));
    s.tick();
    s.tick();
    assert_eq!(t(&s), ("4".into(), "4".into(), "1".into()));
}

#[test]
fn on_refresh_and_on_update_actions_run() {
    let fx = Fx::new();
    let mut s = fx.quick(
        "OnR",
        "[Rainmeter]\nOnRefreshAction=[!SetVariable R 1]\nOnUpdateAction=[!SetVariable U #U#x]\n\
         [Variables]\nU=\n[S]\nMeter=String\nText=x\n",
    );
    assert_eq!(s.parser().get_variable("R").as_deref(), Some("1"));
    let u1 = s.parser().get_variable("U").unwrap();
    s.tick();
    assert_ne!(s.parser().get_variable("U").unwrap(), u1);
}

#[test]
fn delay_defers_the_rest_of_the_actions() {
    let fx = Fx::new();
    let mut s = fx.quick("Dly", "[S]\nMeter=String\nText=x\n");
    s.execute("[!SetVariable A 1][!Delay 30][!SetVariable B 1]");
    assert_eq!(s.parser().get_variable("A").as_deref(), Some("1"));
    assert_eq!(s.parser().get_variable("B"), None);
    std::thread::sleep(std::time::Duration::from_millis(60));
    s.tick();
    assert_eq!(s.parser().get_variable("B").as_deref(), Some("1"));
}

#[test]
fn skin_local_fonts_directory_does_not_break_loading() {
    let fx = Fx::new();
    let dir = fx.env.skins_dir.join("Fonts1/@Resources/Fonts/sub");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("junk.ttf"), b"not a font").unwrap();
    std::fs::write(fx.env.skins_dir.join("Fonts1/Main.ini"), "[Rainmeter]\nLocalFont=missing.ttf\n[S]\nMeter=String\nText=ok\n").unwrap();
    let s = fx.load("Fonts1");
    assert_eq!(s.meter_text("S").as_deref(), Some("ok"));
}

#[test]
fn font_resolver_maps_windows_families_only_when_missing() {
    use snowmeter_core::render::fonts::FontResolver;
    let r = FontResolver::with_families(&["Noto Sans", "Liberation Sans", "Liberation Mono", "DejaVu Sans Mono", "Carlito"]);
    assert_eq!(r.resolve("Segoe UI"), ("Noto Sans".to_string(), None));
    assert_eq!(r.resolve("Segoe UI Light"), ("Noto Sans".to_string(), Some(300)));
    assert_eq!(r.resolve("Segoe UI Semibold"), ("Noto Sans".to_string(), Some(600)));
    assert_eq!(r.resolve("Arial"), ("Liberation Sans".to_string(), None));
    assert_eq!(r.resolve("Courier New").0, "Liberation Mono");
    assert_eq!(r.resolve("Consolas").0, "DejaVu Sans Mono");
    assert_eq!(r.resolve("Calibri").0, "Carlito");
    // Installed families are never remapped.
    assert_eq!(r.resolve("liberation sans"), ("liberation sans".to_string(), None));
    // Unknown families are passed through for fontconfig to substitute.
    assert_eq!(r.resolve("Zapfino Deluxe").0, "Zapfino Deluxe");
    let fallback = FontResolver::with_families(&["DejaVu Sans"]);
    assert_eq!(fallback.resolve("Tahoma").0, "DejaVu Sans");
    let carlito_missing = FontResolver::with_families(&["Liberation Sans"]);
    assert_eq!(carlito_missing.resolve("Calibri").0, "Liberation Sans");
}
