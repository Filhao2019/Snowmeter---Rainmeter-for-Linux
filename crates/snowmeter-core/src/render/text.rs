//! Text layout and drawing with Pango, following Rainmeter's DirectWrite
//! model: the line pitch is `ascent + descent + lineGap`, the gap sits above
//! the ascent, and the `AccurateText=0` GDI+ emulation adds the extras from
//! `notes/meters.md` section 2.5/2.6.

use crate::color::Color;

pub const PANGO_SCALE: f64 = pango::SCALE as f64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HAlign {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    None,
    Shadow,
    Border,
}

/// Base font of a string meter. `family` is already resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct TextFormat {
    pub family: String,
    /// Em size in pixels (`FontSize * 4/3`).
    pub em: f64,
    pub weight: i32,
    pub italic: bool,
    pub accurate: bool,
    pub antialias: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpanKind {
    Family(String),
    SizePx(f64),
    Color(Color),
    Weight(i32),
    Italic,
    Oblique,
    Underline,
    Strike,
    Stretch(i32),
    /// Extra advance per character in pixels.
    Spacing(f64),
    Shadow { dx: f64, dy: f64, color: Color },
}

/// Inline formatting over a byte range of the final text.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: SpanKind,
}

/// Wrapping box for `ClipString`.
#[derive(Clone, Copy, Debug)]
pub struct Wrap {
    pub width: f64,
    pub height: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FontMetricsPx {
    pub ascent: f64,
    pub descent: f64,
    pub gap: f64,
}

impl FontMetricsPx {
    pub fn pitch(&self) -> f64 {
        self.ascent + self.descent + self.gap
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextSize {
    pub w: f64,
    pub h: f64,
    pub lines: usize,
}

fn scratch_cr() -> cairo::Context {
    let s = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).expect("1x1 surface");
    cairo::Context::new(&s).expect("cairo context")
}

fn font_desc(fmt: &TextFormat) -> pango::FontDescription {
    let mut fd = pango::FontDescription::new();
    fd.set_family(&fmt.family);
    fd.set_absolute_size(fmt.em * PANGO_SCALE);
    fd.set_weight(pango::Weight::__Unknown(fmt.weight));
    if fmt.italic {
        fd.set_style(pango::Style::Italic);
    }
    fd
}

fn font_options(antialias: bool) -> cairo::FontOptions {
    let mut fo = cairo::FontOptions::new().expect("font options");
    fo.set_hint_metrics(cairo::HintMetrics::Off);
    fo.set_hint_style(cairo::HintStyle::None);
    fo.set_antialias(if antialias { cairo::Antialias::Gray } else { cairo::Antialias::None });
    fo
}

/// DirectWrite-style metrics approximated from Pango/FreeType.
pub fn font_metrics(fmt: &TextFormat) -> FontMetricsPx {
    let cr = scratch_cr();
    let ctx = pangocairo::functions::create_context(&cr);
    pangocairo::functions::context_set_font_options(&ctx, Some(&font_options(true)));
    ctx.set_round_glyph_positions(false);
    let m = ctx.metrics(Some(&font_desc(fmt)), None);
    let a = m.ascent() as f64 / PANGO_SCALE;
    let d = m.descent() as f64 / PANGO_SCALE;
    let g = (m.height() as f64 / PANGO_SCALE - (a + d)).max(0.0);
    FontMetricsPx { ascent: a, descent: d, gap: g }
}

#[derive(Clone, Copy)]
enum Pass {
    /// Normal colours: inline colours apply.
    Main,
    /// Everything in one colour (string effects); inline colours ignored.
    Flat,
    /// Only the shadowed span (given by index) is visible.
    Shadow(usize),
}

struct LayoutArgs<'a> {
    fmt: &'a TextFormat,
    text: &'a str,
    spans: &'a [Span],
    spacing: bool,
    wrap: Option<Wrap>,
    pass: Pass,
}

fn make_layout(cr: &cairo::Context, a: &LayoutArgs) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    let ctx = layout.context();
    pangocairo::functions::context_set_font_options(&ctx, Some(&font_options(a.fmt.antialias)));
    ctx.set_round_glyph_positions(false);
    layout.set_font_description(Some(&font_desc(a.fmt)));
    layout.set_text(a.text);
    let attrs = pango::AttrList::new();
    let push = |mut at: pango::Attribute, s: &Span| {
        at.set_start_index(s.start as u32);
        at.set_end_index(s.end as u32);
        attrs.insert(at);
    };
    if a.spacing {
        let mut at: pango::Attribute = pango::AttrInt::new_letter_spacing((a.fmt.em / 72.0 * PANGO_SCALE) as i32).into();
        at.set_start_index(0);
        at.set_end_index(u32::MAX);
        attrs.insert(at);
    }
    if let Pass::Shadow(_) = a.pass {
        let mut at: pango::Attribute = pango::AttrInt::new_foreground_alpha(1).into();
        at.set_start_index(0);
        at.set_end_index(u32::MAX);
        attrs.insert(at);
    }
    let color_attrs = |c: Color, s: &Span| {
        push(pango::AttrColor::new_foreground(c.r as u16 * 257, c.g as u16 * 257, c.b as u16 * 257).into(), s);
        push(pango::AttrInt::new_foreground_alpha((c.a as u32 * 257).max(1) as u16).into(), s);
    };
    for (i, s) in a.spans.iter().enumerate() {
        if s.start >= s.end || s.end > a.text.len() {
            continue;
        }
        match (&s.kind, a.pass) {
            (SpanKind::Family(f), _) => push(pango::AttrString::new_family(f).into(), s),
            (SpanKind::SizePx(px), _) => push(pango::AttrSize::new_size_absolute((px * PANGO_SCALE) as i32).into(), s),
            (SpanKind::Color(c), Pass::Main) => color_attrs(*c, s),
            (SpanKind::Color(_), _) => {}
            (SpanKind::Weight(w), _) => push(pango::AttrInt::new_weight(pango::Weight::__Unknown(*w)).into(), s),
            (SpanKind::Italic, _) => push(pango::AttrInt::new_style(pango::Style::Italic).into(), s),
            (SpanKind::Oblique, _) => push(pango::AttrInt::new_style(pango::Style::Oblique).into(), s),
            (SpanKind::Underline, _) => push(pango::AttrInt::new_underline(pango::Underline::Single).into(), s),
            (SpanKind::Strike, _) => push(pango::AttrInt::new_strikethrough(true).into(), s),
            (SpanKind::Stretch(n), _) => {
                let st = match n {
                    1 => pango::Stretch::UltraCondensed,
                    2 => pango::Stretch::ExtraCondensed,
                    3 => pango::Stretch::Condensed,
                    4 => pango::Stretch::SemiCondensed,
                    6 => pango::Stretch::SemiExpanded,
                    7 => pango::Stretch::Expanded,
                    8 => pango::Stretch::ExtraExpanded,
                    9 => pango::Stretch::UltraExpanded,
                    _ => pango::Stretch::Normal,
                };
                push(pango::AttrInt::new_stretch(st).into(), s)
            }
            (SpanKind::Spacing(px), _) => push(pango::AttrInt::new_letter_spacing((px * PANGO_SCALE) as i32).into(), s),
            (SpanKind::Shadow { color, .. }, Pass::Shadow(idx)) if idx == i => color_attrs(*color, s),
            (SpanKind::Shadow { .. }, _) => {}
        }
    }
    layout.set_attributes(Some(&attrs));
    if let Some(w) = a.wrap {
        layout.set_width((w.width.max(0.0) * PANGO_SCALE) as i32);
        layout.set_wrap(pango::WrapMode::WordChar);
        layout.set_ellipsize(pango::EllipsizeMode::End);
        if let Some(h) = w.height {
            layout.set_height((h.max(0.0) * PANGO_SCALE) as i32);
        }
    }
    layout
}

fn line_widths(layout: &pango::Layout, text: &str, ellipsized: bool) -> Vec<f64> {
    let n = layout.line_count();
    let mut out = Vec::with_capacity(n.max(0) as usize);
    for i in 0..n {
        let Some(line) = layout.line_readonly(i) else { continue };
        let w = if ellipsized {
            line.extents().1.width() as f64 / PANGO_SCALE
        } else {
            let start = line.start_index().max(0) as usize;
            let end = (start + line.length().max(0) as usize).min(text.len());
            let slice = text.get(start..end).unwrap_or("");
            let trimmed = slice.trim_end_matches(|c: char| c.is_whitespace());
            if trimmed.is_empty() {
                0.0
            } else {
                line.index_to_x((start + trimmed.len()) as i32, false) as f64 / PANGO_SCALE
            }
        };
        out.push(w);
    }
    out
}

/// Number of lines that count for height: a single trailing `\n` is ignored.
fn counted_lines(text: &str, layout_lines: usize) -> usize {
    if text.len() > 2 && text.ends_with('\n') {
        layout_lines.saturating_sub(1).max(1)
    } else {
        layout_lines.max(1)
    }
}

/// Meter content size for `text` (meters.md 2.5), not yet truncated to ints.
pub fn measure(fmt: &TextFormat, text: &str, spans: &[Span], wrap: Option<Wrap>) -> TextSize {
    if text.is_empty() {
        return TextSize::default();
    }
    let cr = scratch_cr();
    let m = font_metrics(fmt);
    let gdi = !fmt.accurate;
    let layout = make_layout(
        &cr,
        &LayoutArgs { fmt, text, spans, spacing: gdi, wrap, pass: Pass::Main },
    );
    let widths = line_widths(&layout, text, wrap.is_some());
    let text_w = widths.iter().cloned().fold(0.0, f64::max);
    if text_w <= 0.0 {
        return TextSize::default();
    }
    let lines = counted_lines(text, layout.line_count() as usize);
    let text_h = lines as f64 * m.pitch();
    let has_nl = text.contains('\n');
    let (w, h) = if gdi {
        let e = fmt.em / 8.0 - m.gap;
        (text_w + fmt.em / 3.0, text_h + e + if has_nl { m.gap } else { 0.0 })
    } else {
        (text_w.ceil(), text_h.ceil() - if has_nl { 0.0 } else { m.gap })
    };
    TextSize { w, h, lines }
}

/// Everything `draw` needs; the box `(bx, by, bw, bh)` is the padded text box.
pub struct DrawSpec<'a> {
    pub fmt: &'a TextFormat,
    pub text: &'a str,
    pub spans: &'a [Span],
    pub color: Color,
    pub effect: Effect,
    pub effect_color: Color,
    pub halign: HAlign,
    pub valign: VAlign,
    pub bx: f64,
    pub by: f64,
    pub bw: f64,
    pub bh: f64,
    /// Wrap + ellipsis + clip to the box (ClipString).
    pub clip: bool,
    /// Radians, clockwise, about the top of the box at the aligned x.
    pub angle: f64,
}

fn set_color(cr: &cairo::Context, c: Color) {
    cr.set_source_rgba(c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0, c.a as f64 / 255.0);
}

pub fn draw(cr: &cairo::Context, d: &DrawSpec) {
    if d.text.is_empty() {
        return;
    }
    let fmt = d.fmt;
    let m = font_metrics(fmt);
    let gdi = !fmt.accurate;
    let pitch = m.pitch();
    let e = fmt.em / 8.0 - m.gap;
    let _ = cr.save();
    if d.angle != 0.0 {
        let cx = match d.halign {
            HAlign::Center => d.bx + d.bw / 2.0,
            HAlign::Right => d.bx + d.bw,
            _ => d.bx,
        };
        cr.translate(cx, d.by);
        cr.rotate(d.angle * 180.0 / 3.14159265 * std::f64::consts::PI / 180.0);
        cr.translate(-cx, -d.by);
    }
    let box_h = if d.clip { d.bh + 2.0 } else { d.bh };
    if d.clip {
        cr.rectangle(d.bx, d.by, d.bw, d.bh);
        cr.clip();
    }
    let wrap = if d.clip { Some(Wrap { width: d.bw, height: Some(box_h) }) } else { None };
    let spacing = gdi && fmt.antialias;
    let ox = d.bx
        + if gdi {
            match d.halign {
                HAlign::Left | HAlign::Justify => fmt.em / 6.0,
                HAlign::Right => -fmt.em / 6.0,
                HAlign::Center => 0.0,
            }
        } else {
            0.0
        };
    let oy = d.by
        - m.gap
        - match d.valign {
            VAlign::Top => 0.0,
            VAlign::Center => e / 2.0,
            VAlign::Bottom => e,
        };

    let pass = |pass: Pass, color: Color, dx: f64, dy: f64| {
        let layout = make_layout(cr, &LayoutArgs { fmt, text: d.text, spans: d.spans, spacing, wrap, pass });
        let widths = line_widths(&layout, d.text, d.clip);
        let n = layout.line_count();
        let block = n as f64 * pitch;
        let para = match d.valign {
            VAlign::Top => 0.0,
            VAlign::Center => (box_h - block) / 2.0,
            VAlign::Bottom => box_h - block,
        };
        let top = oy + para;
        set_color(cr, color);
        for i in 0..n {
            let Some(line) = layout.line_readonly(i) else { continue };
            let w = widths.get(i as usize).copied().unwrap_or(0.0);
            let x = match d.halign {
                HAlign::Left | HAlign::Justify => ox,
                HAlign::Center => ox + (d.bw - w) / 2.0,
                HAlign::Right => ox + d.bw - w,
            };
            let baseline = top + i as f64 * pitch + m.gap + m.ascent;
            cr.move_to(x + dx, baseline + dy);
            pangocairo::functions::show_layout_line(cr, &line);
        }
    };

    match d.effect {
        Effect::Shadow => pass(Pass::Flat, d.effect_color, 1.0, 1.0),
        Effect::Border => {
            for (dx, dy) in [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)] {
                pass(Pass::Flat, d.effect_color, dx, dy);
            }
        }
        Effect::None => {}
    }
    for (i, s) in d.spans.iter().enumerate() {
        if let SpanKind::Shadow { dx, dy, .. } = &s.kind {
            pass(Pass::Shadow(i), d.color, *dx, *dy);
        }
    }
    pass(Pass::Main, d.color, 0.0, 0.0);
    let _ = cr.restore();
}
