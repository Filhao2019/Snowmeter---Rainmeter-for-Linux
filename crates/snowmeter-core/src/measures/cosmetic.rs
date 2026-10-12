//! Windows cosmetic plugins: window blur (EasyBlur, FrostedGlass) asks the
//! window for blur-behind (honoured by compositors that support it, such as
//! KWin), and the system colour sources (SysColor, MeterialYou) answer from a
//! fixed dark palette so skins keep their layout and stay readable.
//! (Chameleon has its own measure in `chameleon.rs`.)

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// EasyBlur / FrostedGlass: no value; asks for window blur unless turned off.
    Blur,
    SysColor,
    MeterialYou,
}

type Rgb = (u8, u8, u8);

/// First rule whose keyword occurs in the (lowercase) request wins.
const SYSCOLOR: &[(&str, Rgb)] = &[
    ("foreground", (255, 255, 255)),
    ("text", (255, 255, 255)),
    ("background", (32, 32, 32)),
    ("window", (32, 32, 32)),
    ("base", (32, 32, 32)),
    ("dark3", (0, 66, 119)),
    ("dark2", (0, 83, 150)),
    ("dark1", (0, 99, 177)),
    ("light3", (153, 235, 255)),
    ("light2", (118, 185, 237)),
    ("light1", (76, 194, 255)),
    ("complement", (215, 120, 0)),
];
const SYSCOLOR_DEFAULT: Rgb = (0, 120, 212);

/// Material Design 3 roles for a dark scheme seeded from a soft blue.
const MATERIAL: &[(&str, Rgb)] = &[
    ("onprimarycontainer", (214, 227, 255)),
    ("onsecondarycontainer", (218, 226, 249)),
    ("ontertiarycontainer", (251, 215, 252)),
    ("onerrorcontainer", (255, 218, 214)),
    ("onsurfacevariant", (196, 198, 208)),
    ("onbackground", (227, 226, 230)),
    ("onprimary", (0, 47, 100)),
    ("onsecondary", (41, 49, 66)),
    ("ontertiary", (64, 40, 67)),
    ("onerror", (105, 0, 5)),
    ("onsurface", (227, 226, 230)),
    ("primarycontainer", (35, 71, 125)),
    ("secondarycontainer", (63, 71, 89)),
    ("tertiarycontainer", (88, 62, 91)),
    ("errorcontainer", (147, 0, 10)),
    ("surfacevariant", (68, 71, 79)),
    ("surfacecontainerhighest", (51, 53, 57)),
    ("surfacecontainerhigh", (40, 42, 46)),
    ("surfacecontainerlow", (27, 27, 31)),
    ("surfacecontainer", (30, 31, 35)),
    ("surface", (17, 19, 24)),
    ("background", (17, 19, 24)),
    ("outlinevariant", (68, 71, 79)),
    ("outline", (142, 144, 153)),
    ("primary", (169, 199, 255)),
    ("secondary", (190, 198, 220)),
    ("tertiary", (222, 188, 223)),
    ("error", (255, 180, 171)),
    ("inversesurface", (227, 226, 230)),
    ("shadow", (0, 0, 0)),
    ("scrim", (0, 0, 0)),
];
const MATERIAL_DEFAULT: Rgb = (169, 199, 255);

fn lookup(table: &[(&str, Rgb)], default: Rgb, request: &str) -> Rgb {
    let r: String = request.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    table.iter().find(|(k, _)| r.contains(k)).map(|(_, c)| *c).unwrap_or(default)
}

fn format_rgb((r, g, b): Rgb, format: &str) -> String {
    let f = format.to_ascii_lowercase();
    if f.contains("hex") {
        format!("{r:02X}{g:02X}{b:02X}")
    } else if f.contains("rgba") {
        format!("{r},{g},{b},255")
    } else {
        format!("{r},{g},{b}")
    }
}

pub struct CosmeticMeasure {
    flavor: Flavor,
    text: Option<String>,
    log: Option<String>,
    /// Blur flavor: whether the skin wants blur behind its window now.
    blur: bool,
}

impl CosmeticMeasure {
    pub fn new(flavor: Flavor) -> CosmeticMeasure {
        CosmeticMeasure { flavor, text: None, log: None, blur: false }
    }
}

/// FrostedGlass `Type=None` / EasyBlur `Blur=0` (and similar) turn blur off.
fn blur_off(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "none" | "0" | "off" | "false" | "disabled" | "disable")
}

impl MeasureImpl for CosmeticMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let (table, default, fmt_key) = match self.flavor {
            Flavor::Blur => {
                self.text = None;
                self.blur = !blur_off(&opt.string("Type", "Blur")) && !blur_off(&opt.string("Blur", "1"));
                return;
            }
            Flavor::SysColor => (SYSCOLOR, SYSCOLOR_DEFAULT, "Format"),
            Flavor::MeterialYou => (MATERIAL, MATERIAL_DEFAULT, "Format"),
        };
        // The plugins differ in which option names the colour; look at all the likely ones.
        let request = ["Color", "ColorType", "Type", "Role", "Name", "Palette", "Index"]
            .iter()
            .map(|k| opt.string(k, ""))
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let mut fmt = opt.string(fmt_key, "");
        if fmt.trim().is_empty() {
            fmt = opt.string("OutputFormat", "");
        }
        self.text = Some(format_rgb(lookup(table, default, &request), &fmt));
        if self.log.is_none() {
            let who = match self.flavor {
                Flavor::SysColor => "SysColor",
                _ => "MeterialYou",
            };
            log::debug!("{who}: using a fixed fallback palette on Linux");
            self.log = Some(String::new());
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        0.0
    }

    fn string_value(&self) -> Option<String> {
        self.text.clone()
    }

    fn wants_blur(&self) -> Option<bool> {
        (self.flavor == Flavor::Blur).then_some(self.blur)
    }

    fn command(&mut self, args: &str) {
        if self.flavor != Flavor::Blur {
            return;
        }
        // FrostedGlass "Type None|Blur|Acrylic", EasyBlur "Blur 0|1", plus Enable/Disable/Toggle.
        let a = args.trim().to_ascii_lowercase();
        let last = a.split_whitespace().last().unwrap_or("");
        self.blur = match a.split_whitespace().next().unwrap_or("") {
            "toggle" | "toggleblur" => !self.blur,
            "disable" | "disableblur" => false,
            "enable" | "enableblur" => true,
            "type" | "blur" | "settype" => !blur_off(last),
            _ => self.blur,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::skin;
    
    fn text(flavor: Flavor, ini: &str) -> Option<String> {
        let s = skin(ini);
        let mut m = CosmeticMeasure::new(flavor);
        m.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        m.string_value()
    }

    #[test]
    fn colour_lookup() {
        assert_eq!(lookup(MATERIAL, MATERIAL_DEFAULT, "OnPrimaryContainer"), (214, 227, 255));
        assert_eq!(lookup(MATERIAL, MATERIAL_DEFAULT, "on_surface"), (227, 226, 230));
        assert_eq!(lookup(SYSCOLOR, SYSCOLOR_DEFAULT, "Background"), (32, 32, 32));
        assert_eq!(format_rgb((1, 2, 255), "Hex"), "0102FF");
        assert_eq!(format_rgb((1, 2, 3), ""), "1,2,3");
        assert_eq!(format_rgb((1, 2, 3), "RGBA"), "1,2,3,255");
    }

    #[test]
    fn measures_answer_with_colours() {
        assert_eq!(text(Flavor::SysColor, "[M]\nMeasure=Plugin\nPlugin=SysColor\nType=Foreground\nFormat=Hex\n").as_deref(), Some("FFFFFF"));
        assert_eq!(text(Flavor::MeterialYou, "[M]\nMeasure=Plugin\nPlugin=MeterialYou\nType=Primary\n").as_deref(), Some("169,199,255"));
        assert_eq!(text(Flavor::Blur, "[M]\nMeasure=Plugin\nPlugin=EasyBlur\nBlur=1\n"), None);
        assert_eq!(text(Flavor::Blur, "[M]\nMeasure=Plugin\nPlugin=FrostedGlass\nType=Blur\n"), None);
    }

    #[test]
    fn blur_requests() {
        let s = skin("[M]\nMeasure=Plugin\nPlugin=FrostedGlass\nType=Acrylic\n[N]\nMeasure=Plugin\nPlugin=FrostedGlass\nType=None\n");
        let mut m = CosmeticMeasure::new(Flavor::Blur);
        m.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(m.wants_blur(), Some(true));
        m.command("Type None");
        assert_eq!(m.wants_blur(), Some(false));
        m.command("Toggle");
        assert_eq!(m.wants_blur(), Some(true));
        let mut n = CosmeticMeasure::new(Flavor::Blur);
        n.read_options(&OptionReader { parser: &s.parser, section: "N", sv: None });
        assert_eq!(n.wants_blur(), Some(false));
        assert_eq!(CosmeticMeasure::new(Flavor::SysColor).wants_blur(), None);
    }

    #[test]
    fn names_are_native() {
        use crate::measures::plugin::is_native_plugin;
        for n in ["EasyBlur", "FrostedGlass.dll", "Chameleon", "SysColor", "MeterialYou", "MaterialYou.dll"] {
            assert!(is_native_plugin(n), "{n}");
        }
    }
}
