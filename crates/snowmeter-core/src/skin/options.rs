//! The `[Rainmeter]` section of a skin file (skin-format.md section 7.2).

use super::mouse::MouseActions;
use crate::color::Color;
use crate::parser::{ConfigParser, OptionReader};
use crate::render::image::ImageOptions;

#[derive(Clone, Debug)]
pub struct SkinOptions {
    pub syntax_version: i64,
    /// `Blur=1`: blur behind the window (where the compositor supports it).
    pub blur: bool,
    pub accurate_text: bool,
    pub skin_width: i32,
    pub skin_height: i32,
    pub groups: Vec<String>,
    /// `l, t, r, b`.
    pub background_margins: [i32; 4],
    /// 0 image, 1 none, 2 solid, 3 scaled image, 4 tiled image.
    pub background_mode: i32,
    pub background: String,
    pub image_path: String,
    pub background_image: ImageOptions,
    pub solid_color: Color,
    pub solid_color2: Color,
    pub gradient_angle: f64,
    pub dynamic_window_size: bool,
    /// Milliseconds; negative = no timer.
    pub update_ms: i64,
    pub default_update_divider: i64,
    pub on_refresh: String,
    pub on_update: String,
    pub on_close: String,
    pub local_fonts: Vec<String>,
    pub mouse: MouseActions,
    pub messages: Vec<String>,
}

fn rect4(s: &str) -> [i32; 4] {
    let parts: Vec<i32> = s
        .split(',')
        .map(|p| crate::numparse::parse_i32_prefix(p.trim()).unwrap_or(0) as i32)
        .collect();
    if parts.len() >= 4 {
        [parts[0], parts[1], parts[2], parts[3]]
    } else {
        [0; 4]
    }
}

impl SkinOptions {
    pub fn read(parser: &ConfigParser) -> SkinOptions {
        let opt = OptionReader::new(parser, "Rainmeter", None);
        let mut messages = Vec::new();
        let syntax_version = opt.int("SyntaxVersion", 1);
        let mut local_fonts = Vec::new();
        for n in 1.. {
            let key = if n == 1 { "LocalFont".to_string() } else { format!("LocalFont{n}") };
            let v = opt.string(&key, "");
            if v.trim().is_empty() {
                break;
            }
            local_fonts.push(v.trim().to_string());
        }
        let update = opt.string("Update", "1000");
        let update_ms = match parser.parse_formula(&update, None) {
            Some(v) if update.trim_start().starts_with('(') => v as i64,
            _ => match crate::numparse::parse_i32_prefix(update.trim()) {
                Some(v) => v,
                None => {
                    messages.push(format!("Invalid Update={update}"));
                    1000
                }
            },
        };
        let solid_color = opt.color("SolidColor", Color { r: 128, g: 128, b: 128, a: 255 });
        let background = opt.string("Background", "");
        let mut background_mode = opt.int("BackgroundMode", 0) as i32;
        if matches!(background_mode, 0 | 3 | 4) && background.trim().is_empty() {
            background_mode = 1;
        }
        SkinOptions {
            syntax_version,
            accurate_text: opt.bool("AccurateText", false) || syntax_version >= 2,
            skin_width: opt.int("SkinWidth", 0) as i32,
            skin_height: opt.int("SkinHeight", 0) as i32,
            groups: opt
                .string("Group", "")
                .split('|')
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty())
                .collect(),
            background_margins: rect4(&opt.string("BackgroundMargins", "")),
            background_mode,
            background: background.trim().to_string(),
            image_path: opt.string("ImagePath", ""),
            background_image: ImageOptions::read(&opt, &mut messages),
            solid_color,
            solid_color2: opt.color("SolidColor2", solid_color),
            gradient_angle: opt.float("GradientAngle", 0.0),
            dynamic_window_size: opt.bool("DynamicWindowSize", false),
            blur: opt.bool("Blur", false),
            update_ms,
            default_update_divider: opt.int("DefaultUpdateDivider", 1),
            on_refresh: opt.action("OnRefreshAction"),
            on_update: opt.action("OnUpdateAction"),
            on_close: opt.action("OnCloseAction"),
            local_fonts,
            mouse: MouseActions::read(&opt),
            messages,
        }
    }
}
