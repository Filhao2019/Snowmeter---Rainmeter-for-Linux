//! The String meter (meters.md section 2).

use super::{MeterBase, MeterImpl, MeterInputs};
use crate::color::{parse_color, Color};
use crate::measures::base::parse_auto_scale;
use crate::parser::OptionReader;
use crate::render::text::{self, Effect, HAlign, Span, SpanKind, TextFormat, TextSize, VAlign, Wrap};
use crate::render::Resources;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum StringCase {
    #[default]
    None,
    Upper,
    Lower,
    Proper,
}

fn apply_case(s: &str, case: StringCase) -> String {
    match case {
        StringCase::None => s.to_string(),
        StringCase::Upper => s.to_uppercase(),
        StringCase::Lower => s.to_lowercase(),
        StringCase::Proper => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for c in s.chars() {
                if c.is_alphabetic() {
                    if start {
                        out.extend(c.to_uppercase());
                    } else {
                        out.extend(c.to_lowercase());
                    }
                    start = false;
                } else {
                    out.push(c);
                    start = !(c == '\'' || c.is_numeric());
                }
            }
            out
        }
    }
}

/// `StringAlign` (2.4): horizontal prefix, then the rest must be exactly
/// `Bottom` or `Center` for the vertical part.
pub fn parse_align(s: &str) -> (HAlign, VAlign) {
    let lower = s.trim().to_ascii_lowercase();
    for (prefix, h) in [("left", HAlign::Left), ("right", HAlign::Right), ("center", HAlign::Center), ("justify", HAlign::Justify)] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let v = match rest {
                "bottom" => VAlign::Bottom,
                "center" => VAlign::Center,
                _ => VAlign::Top,
            };
            return (h, v);
        }
    }
    (HAlign::Left, VAlign::Top)
}

#[derive(Clone, Debug)]
struct Inline {
    setting: String,
    pattern: String,
}

#[derive(Default)]
pub struct StringMeter {
    text: String,
    prefix: String,
    postfix: String,
    face: String,
    size_pt: f64,
    bold: bool,
    italic: bool,
    weight: Option<i32>,
    color: Color,
    effect_color: Color,
    effect: Option<Effect>,
    case: StringCase,
    halign: Option<HAlign>,
    valign: Option<VAlign>,
    auto_scale: u8,
    scale: f64,
    decimals: i32,
    percentual: bool,
    no_decimals: bool,
    clip_mode: u8,
    clip_w: i32,
    clip_h: i32,
    angle: f64,
    trailing_spaces: bool,
    antialias: bool,
    inline: Vec<Inline>,
    /// Built by `update`.
    final_text: String,
    spans: Vec<Span>,
    cache: RefCell<Option<(String, Info)>>,
    log: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Info {
    w: f64,
    h: f64,
    needs_clip: bool,
}

impl StringMeter {
    fn format(&self, res: &Resources) -> TextFormat {
        let (family, fam_weight) = res.fonts.resolve(&self.face);
        let weight = self.weight.or(if self.bold { Some(700) } else { fam_weight }).unwrap_or(400);
        TextFormat {
            family,
            em: self.size_pt * 4.0 / 3.0,
            weight,
            italic: self.italic,
            accurate: res.accurate_text,
            antialias: self.antialias,
        }
    }

    fn effective_decimals(&self) -> i32 {
        if self.decimals != -1 {
            self.decimals
        } else if self.no_decimals && (self.percentual || self.auto_scale == 0) {
            0
        } else {
            1
        }
    }

    fn build_text(&self, m: &MeterInputs) -> String {
        let mut s = self.prefix.clone();
        let dec = self.effective_decimals();
        let value = |i: usize| m.measures[i].string_value(self.auto_scale, self.scale, dec, self.percentual);
        if !m.measures.is_empty() {
            if self.text.is_empty() {
                s.push_str(&value(0));
            } else {
                let mut t = self.text.clone();
                if t.contains('%') {
                    for i in (0..m.measures.len()).rev() {
                        t = t.replace(&format!("%{}", i + 1), &value(i));
                    }
                }
                s.push_str(&t);
            }
        } else {
            s.push_str(&self.text);
        }
        s.push_str(&self.postfix);
        let s = apply_case(&s, self.case);
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '\r' => {}
                '\u{a0}' | '\u{205f}' => {
                    out.push(c);
                    out.push('\u{200b}');
                }
                _ => out.push(c),
            }
        }
        if self.trailing_spaces {
            out.push('\u{200b}');
        }
        out
    }

    fn compute(&self, base: &MeterBase, res: &Resources) -> Info {
        let fmt = self.format(res);
        let key = format!(
            "{:?}|{:?}|{:?}|{}|{}|{}|{}",
            fmt, self.final_text, self.spans, self.clip_mode, self.clip_w, self.clip_h,
            format_args!("{:?}{:?}{:?}", base.w_given, base.h_given, base.padding)
        );
        if let Some((k, info)) = self.cache.borrow().as_ref() {
            if *k == key {
                return *info;
            }
        }
        let info = self.compute_uncached(&fmt, base);
        *self.cache.borrow_mut() = Some((key, info));
        info
    }

    fn compute_uncached(&self, fmt: &TextFormat, base: &MeterBase) -> Info {
        let un = text::measure(fmt, &self.final_text, &self.spans, None);
        if self.clip_mode != 2 {
            return Info { w: un.w, h: un.h, needs_clip: false };
        }
        let wrapped = |w: f64| -> TextSize { text::measure(fmt, &self.final_text, &self.spans, Some(Wrap { width: w, height: None })) };
        let (wg, hg) = (base.w_given.map(f64::from), base.h_given.map(f64::from));
        let cap = |h: f64, user: Option<f64>| match user {
            Some(u) => h.min(u),
            None if self.clip_h >= 0 => h.min(self.clip_h as f64),
            None => h,
        };
        match (wg, hg) {
            (Some(w), h_opt) => {
                let wh = wrapped(w).h + 1.0;
                Info { w, h: cap(wh, h_opt), needs_clip: true }
            }
            (None, Some(h)) => {
                if self.clip_w == -1 {
                    Info { w: un.w, h, needs_clip: un.h > h }
                } else {
                    let w = un.w.min(self.clip_w as f64);
                    Info { w, h, needs_clip: un.w > w || wrapped(w).h + 1.0 > h }
                }
            }
            (None, None) => {
                if self.clip_w == -1 {
                    if self.clip_h >= 0 && un.h > self.clip_h as f64 {
                        Info { w: un.w, h: self.clip_h as f64, needs_clip: true }
                    } else {
                        Info { w: un.w, h: un.h, needs_clip: false }
                    }
                } else {
                    let w = un.w.min(self.clip_w as f64);
                    let wh = wrapped(w).h + 1.0;
                    let h = cap(wh, None);
                    Info { w, h, needs_clip: un.w > w || h < wh }
                }
            }
        }
    }

    fn should_clip(&self, base: &MeterBase, info: &Info) -> bool {
        self.clip_mode == 1
            || (self.clip_mode == 2 && (info.needs_clip || (base.w_given.is_some() && base.h_given.is_some())))
    }

    fn compute_spans(&mut self, res: &Resources) {
        self.spans.clear();
        for (i, inl) in self.inline.clone().iter().enumerate() {
            let ranges = pattern_ranges(&self.final_text, &inl.pattern, &mut self.log, i);
            let Some(kinds) = parse_setting(&inl.setting, res) else { continue };
            for (s, e) in ranges {
                for k in &kinds {
                    match k {
                        Setting::Kind(kind) => self.spans.push(Span { start: s, end: e, kind: kind.clone() }),
                        Setting::Case(case) => {
                            let part = apply_case_keep_len(&self.final_text[s..e], *case);
                            self.final_text.replace_range(s..e, &part);
                        }
                    }
                }
            }
        }
    }
}

fn apply_case_keep_len(s: &str, case: SettingCase) -> String {
    let mapped: Vec<char> = s.chars().collect();
    let whole = match case {
        SettingCase::Upper => apply_case(s, StringCase::Upper),
        SettingCase::Lower => apply_case(s, StringCase::Lower),
        SettingCase::Proper => apply_case(s, StringCase::Proper),
        SettingCase::Sentence => {
            let lower = s.to_lowercase();
            let mut out = String::new();
            let mut cap = true;
            for c in lower.chars() {
                if cap && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    cap = false;
                } else {
                    out.push(c);
                }
                if matches!(c, '.' | '?' | '!') {
                    cap = true;
                }
            }
            out
        }
    };
    if whole.len() == s.len() {
        return whole;
    }
    // Lengths differ: map char by char, keeping chars whose case form changes length.
    let mut out = String::with_capacity(s.len());
    for (c, w) in mapped.iter().zip(whole.chars().chain(std::iter::repeat(' '))) {
        out.push(if w.len_utf8() == c.len_utf8() { w } else { *c });
    }
    out
}

#[derive(Clone, Copy)]
enum SettingCase {
    Upper,
    Lower,
    Proper,
    Sentence,
}

enum Setting {
    Kind(SpanKind),
    Case(SettingCase),
}

fn num(s: &str) -> f64 {
    crate::numparse::parse_f64_prefix(s.trim())
}

/// One `InlineSetting` value. `None` = unparsable (stops reading further settings).
fn parse_setting(s: &str, res: &Resources) -> Option<Vec<Setting>> {
    let s = s.trim();
    let mut parts = s.split('|');
    let name = parts.next()?.trim().to_ascii_lowercase();
    let args: Vec<&str> = parts.map(str::trim).filter(|a| !a.is_empty()).collect();
    let first = args.first().copied();
    let k = |k: SpanKind| Some(vec![Setting::Kind(k)]);
    match name.as_str() {
        "none" => Some(Vec::new()),
        "face" => k(SpanKind::Family(res.fonts.resolve(first?).0)),
        "size" => k(SpanKind::SizePx(if num(first?) <= 0.0 { 1e-6 } else { num(first?) * 4.0 / 3.0 })),
        "color" => k(SpanKind::Color(parse_color(first?)?)),
        "weight" => k(SpanKind::Weight((num(first?) as i32).clamp(1, 999))),
        "italic" => k(SpanKind::Italic),
        "oblique" => k(SpanKind::Oblique),
        "underline" => k(SpanKind::Underline),
        "strikethrough" => k(SpanKind::Strike),
        "stretch" => k(SpanKind::Stretch((num(first?) as i32).clamp(1, 9))),
        "case" => {
            let c = match first?.to_ascii_lowercase().as_str() {
                "upper" => SettingCase::Upper,
                "lower" => SettingCase::Lower,
                "proper" => SettingCase::Proper,
                "sentence" => SettingCase::Sentence,
                _ => return None,
            };
            Some(vec![Setting::Case(c)])
        }
        "characterspacing" => {
            let lead = num(first?) * 4.0 / 3.0;
            let trail = args.get(1).filter(|a| **a != "*").map(|a| num(a) * 4.0 / 3.0).unwrap_or(0.0);
            k(SpanKind::Spacing(lead + trail))
        }
        "typography" => Some(Vec::new()),
        "shadow" => {
            let x = num(first?);
            let y = args.get(1).map(|a| num(a)).unwrap_or(0.0);
            let color = args.get(3).and_then(|c| parse_color(c)).unwrap_or(Color { r: 0, g: 0, b: 0, a: 255 });
            k(SpanKind::Shadow { dx: x, dy: y, color })
        }
        "gradientcolor" | "gradientcolor1" => {
            // First stop colour as the fallback.
            let c = args.iter().skip(1).find_map(|a| parse_color(a.split(';').next().unwrap_or("")))?;
            k(SpanKind::Color(c))
        }
        _ => None,
    }
}

/// Byte ranges matched by `pattern` in `text` (captures if present, else the
/// whole match; empty matches skipped; `.*` is the whole string).
fn pattern_ranges(text: &str, pattern: &str, log: &mut Vec<String>, idx: usize) -> Vec<(usize, usize)> {
    if text.is_empty() {
        return Vec::new();
    }
    if pattern == ".*" {
        return vec![(0, text.len())];
    }
    let re = match pcre2::bytes::RegexBuilder::new().utf(true).build(pattern) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("InlinePattern{} is invalid: {e}", if idx == 0 { String::new() } else { (idx + 1).to_string() });
            if !log.contains(&msg) {
                log.push(msg);
            }
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    let mut locs = re.capture_locations();
    let mut from = 0;
    while from <= text.len() {
        match re.captures_read_at(&mut locs, text.as_bytes(), from) {
            Ok(Some(m)) => {
                if m.start() == m.end() {
                    from = m.end() + text[m.end()..].chars().next().map(char::len_utf8).unwrap_or(1);
                    continue;
                }
                let mut groups = Vec::new();
                for g in 1..locs.len() {
                    if let Some((s, e)) = locs.get(g) {
                        if e > s {
                            groups.push((s, e));
                        }
                    }
                }
                if groups.is_empty() {
                    out.push((m.start(), m.end()));
                } else {
                    out.extend(groups);
                }
                from = m.end();
            }
            _ => break,
        }
    }
    out
}

impl MeterImpl for StringMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        self.text = opt.string("Text", "");
        self.prefix = opt.string("Prefix", "");
        self.postfix = opt.string("Postfix", "");
        let face = opt.string("FontFace", "Arial");
        self.face = if face.trim().is_empty() { "Arial".into() } else { face };
        let size = opt.float("FontSize", 10.0);
        self.size_pt = if size < 0.0 { 10.0 } else { size };
        match opt.string("StringStyle", "Normal").to_ascii_lowercase().as_str() {
            "bold" => (self.bold, self.italic) = (true, false),
            "italic" => (self.bold, self.italic) = (false, true),
            "bolditalic" => (self.bold, self.italic) = (true, true),
            _ => (self.bold, self.italic) = (false, false),
        }
        let w = opt.int("FontWeight", 0);
        self.weight = (1..=999).contains(&w).then_some(w as i32);
        self.color = opt.color("FontColor", Color { r: 0, g: 0, b: 0, a: 255 });
        self.effect_color = opt.color("FontEffectColor", Color { r: 0, g: 0, b: 0, a: 255 });
        self.effect = match opt.string("StringEffect", "None").to_ascii_lowercase().as_str() {
            "shadow" => Some(Effect::Shadow),
            "border" => Some(Effect::Border),
            _ => None,
        };
        self.case = match opt.string("StringCase", "None").to_ascii_lowercase().as_str() {
            "upper" => StringCase::Upper,
            "lower" => StringCase::Lower,
            "proper" => StringCase::Proper,
            _ => StringCase::None,
        };
        let (h, v) = parse_align(&opt.string("StringAlign", "Left"));
        self.halign = Some(h);
        self.valign = Some(v);
        self.auto_scale = parse_auto_scale(&opt.string("AutoScale", "0"));
        let scale_s = opt.string("Scale", "1");
        self.no_decimals = !scale_s.contains('.');
        self.scale = if scale_s.trim().is_empty() {
            1.0
        } else if scale_s.trim_start().starts_with('(') {
            opt.parser.parse_formula(&scale_s, opt.sv).unwrap_or(1.0)
        } else {
            crate::numparse::parse_f64_prefix(&scale_s)
        };
        self.decimals = opt.int("NumOfDecimals", -1) as i32;
        self.percentual = opt.bool("Percentual", false);
        let clip = opt.int("ClipString", 0);
        if (0..=2).contains(&clip) {
            self.clip_mode = clip as u8;
        } else {
            self.log.push(format!("[{}] ClipString={clip} is not valid", opt.section));
        }
        self.clip_w = if self.clip_mode == 2 { opt.int("ClipStringW", -1) as i32 } else { -1 };
        self.clip_h = if self.clip_mode == 2 { opt.int("ClipStringH", -1) as i32 } else { -1 };
        self.angle = opt.float("Angle", 0.0);
        self.trailing_spaces = opt.bool("TrailingSpaces", false);
        self.antialias = opt.bool("AntiAlias", false);
        self.inline.clear();
        for n in 1.. {
            let sfx = if n == 1 { String::new() } else { n.to_string() };
            let setting = opt.string(&format!("InlineSetting{sfx}"), "");
            if setting.trim().is_empty() {
                break;
            }
            let pattern = opt.string(&format!("InlinePattern{sfx}"), ".*");
            self.inline.push(Inline { setting, pattern: if pattern.is_empty() { ".*".into() } else { pattern } });
        }
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        let mut old = self.build_text(m);
        std::mem::swap(&mut self.final_text, &mut old);
        self.compute_spans(m.res);
        old != self.final_text
    }

    fn auto_size(&self, base: &MeterBase, res: &Resources) -> (f64, f64) {
        let info = self.compute(base, res);
        (info.w, info.h)
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, res: &Resources) {
        let fmt = self.format(res);
        let info = self.compute(base, res);
        let (bx, by, bw, bh) = base.content_rect();
        text::draw(
            cr,
            &text::DrawSpec {
                fmt: &fmt,
                text: &self.final_text,
                spans: &self.spans,
                color: self.color,
                effect: self.effect.unwrap_or(Effect::None),
                effect_color: self.effect_color,
                halign: self.halign.unwrap_or(HAlign::Left),
                valign: self.valign.unwrap_or(VAlign::Top),
                bx,
                by,
                bw,
                bh,
                clip: self.should_clip(base, &info),
                angle: self.angle,
            },
        );
    }

    fn align(&self) -> (HAlign, VAlign) {
        (self.halign.unwrap_or(HAlign::Left), self.valign.unwrap_or(VAlign::Top))
    }

    fn text(&self) -> Option<&str> {
        Some(&self.final_text)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}
