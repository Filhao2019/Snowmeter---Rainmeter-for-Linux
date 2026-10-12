//! Render tests for the M4 meter types.

use snowmeter_core::env::Environment;
use snowmeter_core::skin::Skin;
use std::path::{Path, PathBuf};

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

    fn dir(&self, name: &str) -> PathBuf {
        let d = self.env.skins_dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A skin with `ini` as Main.ini, then `ticks` updates, rendered.
    fn render(&self, name: &str, ini: &str, ticks: u32) -> (Skin, image::RgbaImage) {
        std::fs::write(self.dir(name).join("Main.ini"), ini).unwrap();
        let mut s = Skin::load(&self.env, name, "Main.ini").unwrap();
        for _ in 0..ticks {
            s.tick();
        }
        let png = self._tmp.path().join(format!("{name}.png"));
        s.render_to_surface().write_to_png(&mut std::fs::File::create(&png).unwrap()).unwrap();
        (s, image::open(&png).unwrap().to_rgba8())
    }
}

fn px(img: &image::RgbaImage, x: u32, y: u32) -> [u8; 4] {
    img.get_pixel(x, y).0
}

fn write_png(path: &Path, w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba(f(x, y));
    }
    img.save(path).unwrap();
}

#[test]
fn fixture_skin_renders_bar_roundline_shape_line() {
    let fx = Fx::new();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m4/Meters/Main.ini");
    let ini = std::fs::read_to_string(src).unwrap();
    let (_s, img) = fx.render("Meters", &ini, 10);
    // Bar at 50%: left half red, right half empty.
    assert_eq!(px(&img, 10, 10), [255, 0, 0, 255]);
    assert_eq!(px(&img, 80, 10)[3], 0);
    // RoundLine at 25%: points straight down from the centre (50, 70).
    assert_eq!(px(&img, 50, 100), [0, 0, 255, 255]);
    assert_eq!(px(&img, 80, 70)[3], 0);
    // Shapes.
    assert_eq!(px(&img, 20, 140), [255, 0, 0, 255]);
    assert_eq!(px(&img, 70, 150), [0, 255, 0, 255]);
    assert_eq!(px(&img, 10, 167), [255, 255, 0, 255], "combined union, parent colour");
    assert_eq!(px(&img, 30, 167), [255, 255, 0, 255], "union includes the second rectangle");
    // Line: constant 50% history is a horizontal line in the middle of the box.
    let ys: Vec<u32> = (180..220).filter(|&y| px(&img, 95, y) == [255, 0, 255, 255]).collect();
    assert!(!ys.is_empty() && ys.iter().all(|y| (197..=202).contains(y)), "{ys:?}");
}

#[test]
fn shape_modifiers_render() {
    let fx = Fx::new();
    let (s, img) = fx.render(
        "Mods",
        "[S]\nMeter=Shape\nX=0\nY=0\nShape=Rectangle 0,0,20,10 | Fill Color 255,0,0 | StrokeWidth 0 | Offset 30,0\n\
         Shape2=Rectangle 0,20,20,10 | Fill LinearGradient G | StrokeWidth 0\nG=0 | 0,0,255 ; 0.0 | 0,0,255 ; 1.0\n\
         Shape3=Rectangle 0,40,20,10 | Fill Color 0,0,0,0 | Stroke Color 0,255,0 | StrokeWidth 4\n",
        1,
    );
    let b = s.meter_base("S").unwrap();
    assert_eq!(b.w, 50.0, "auto size from the offset rectangle");
    assert_eq!(px(&img, 40, 5), [255, 0, 0, 255]);
    assert_eq!(px(&img, 5, 5)[3], 0, "offset moved the shape away");
    assert_eq!(px(&img, 10, 25), [0, 0, 255, 255]);
    assert_eq!(px(&img, 10, 40), [0, 255, 0, 255]);
}

#[test]
fn rotator_rotates_image_by_value() {
    let fx = Fx::new();
    let dir = fx.dir("Rot");
    // 20x4 needle: opaque red, pointing right from its centre.
    write_png(&dir.join("needle.png"), 20, 4, |_, _| [255, 0, 0, 255]);
    let ini = "[m]\nMeasure=Calc\nFormula=25\nMaxValue=100\n\
        [R]\nMeter=Rotator\nMeasureName=m\nImageName=needle.png\nX=0\nY=0\nW=60\nH=60\nOffsetX=2\nOffsetY=2\n";
    let (_s, img) = fx.render("Rot", ini, 2);
    // Pivot at (2,2) in the image placed at the meter centre (30,30); 25% = 90 degrees: points down.
    assert_eq!(px(&img, 30, 40), [255, 0, 0, 255]);
    assert_eq!(px(&img, 40, 30)[3], 0);
}

#[test]
fn bitmap_shows_the_selected_frame() {
    let fx = Fx::new();
    let dir = fx.dir("Bmp");
    // 4 frames of 10x10 stacked vertically: red, green, blue, white.
    let cols = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255]];
    write_png(&dir.join("f.png"), 10, 40, |_, y| cols[(y / 10) as usize]);
    let ini = "[m]\nMeasure=Calc\nFormula=50\nMaxValue=100\n\
        [B]\nMeter=Bitmap\nMeasureName=m\nBitmapImage=f.png\nBitmapFrames=4\nX=0\nY=0\n";
    let (s, img) = fx.render("Bmp", ini, 2);
    assert_eq!(s.meter_base("B").unwrap().h, 10.0);
    assert_eq!(px(&img, 5, 5), [0, 0, 255, 255], "0.5 * 4 frames = frame 2");
}

#[test]
fn button_draws_the_normal_frame() {
    let fx = Fx::new();
    let dir = fx.dir("Btn");
    let cols = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    write_png(&dir.join("b.png"), 30, 90, |_, y| cols[(y / 30) as usize]);
    let ini = "[B]\nMeter=Button\nButtonImage=b.png\nX=0\nY=0\n";
    let (s, img) = fx.render("Btn", ini, 1);
    let b = s.meter_base("B").unwrap();
    assert_eq!((b.w, b.h), (30.0, 30.0));
    assert_eq!(px(&img, 15, 15), [255, 0, 0, 255]);
}

#[test]
fn histogram_draws_primary_bars() {
    let fx = Fx::new();
    let ini = "[m]\nMeasure=Calc\nFormula=100\nMaxValue=100\n\
        [H]\nMeter=Histogram\nMeasureName=m\nX=0\nY=0\nW=20\nH=10\nPrimaryColor=255,0,0,255\n";
    let (_s, img) = fx.render("Hist", ini, 30);
    assert_eq!(px(&img, 19, 5), [255, 0, 0, 255]);
}

#[test]
fn transformation_matrix_and_bevel() {
    let fx = Fx::new();
    // A 10x10 red square translated by (30, 0) via the matrix.
    let (_s, img) = fx.render(
        "Mx",
        "[C]\nMeter=String\nX=0\nY=0\nW=60\nH=1\n[A]\nMeter=String\nX=10\nY=10\nW=10\nH=10\nSolidColor=255,0,0\nTransformationMatrix=1;0;0;1;30;0\n\
         [B]\nMeter=String\nX=10\nY=40\nW=10\nH=10\nSolidColor=0,0,255\nBevelType=1\nBevelColor=0,255,0,255\n",
        1,
    );
    assert_eq!(px(&img, 45, 15), [255, 0, 0, 255]);
    assert_eq!(px(&img, 15, 15)[3], 0);
    assert_eq!(px(&img, 15, 45), [0, 0, 255, 255]);
    // Light bevel line two pixels outside the left edge.
    assert_eq!(px(&img, 8, 45), [0, 255, 0, 255]);
}
