//! Color parsing (`Common/ParseUtil.cpp` `ParseColor`).
//!
//! Accepts `R,G,B[,A]` (each component may be a `(formula)`) or hex
//! `RRGGBB[AA]` with an optional lowercase `0x` prefix. Missing components
//! default to 255, non-numeric list components read as 0.

use crate::formula;
use crate::numparse;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Parse a color string; formulas cannot reference measures here.
pub fn parse_color(s: &str) -> Option<Color> {
    parse_color_with(s, &|_| None)
}

/// Like [`parse_color`] but formula components may resolve names via `lookup`.
pub fn parse_color_with(s: &str, lookup: &dyn Fn(&str) -> Option<f64>) -> Option<Color> {
    if s.trim().is_empty() {
        return None;
    }
    let mut rgba = [255i32; 4];
    if s.contains(',') {
        parse_list(s, &mut rgba, lookup);
    } else {
        parse_hex(s, &mut rgba);
    }
    let c = |v: i32| v.clamp(0, 255) as u8;
    Some(Color { r: c(rgba[0]), g: c(rgba[1]), b: c(rgba[2]), a: c(rgba[3]) })
}

fn parse_list(s: &str, out: &mut [i32; 4], lookup: &dyn Fn(&str) -> Option<f64>) {
    let chars: Vec<char> = s.chars().collect();
    let mut pos = 0;
    for slot in out.iter_mut() {
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }
        if pos >= chars.len() {
            break;
        }
        let start = pos;
        let mut depth = 0;
        while pos < chars.len() && !(chars[pos] == ',' && depth == 0) {
            match chars[pos] {
                '(' => depth += 1,
                ')' if depth > 0 => depth -= 1,
                _ => {}
            }
            pos += 1;
        }
        let comp: String = chars[start..pos].iter().collect();
        pos += 1; // the comma
        *slot = parse_component(&comp, lookup);
    }
}

fn parse_component(comp: &str, lookup: &dyn Fn(&str) -> Option<f64>) -> i32 {
    let t = comp.trim_start();
    if t.starts_with('(') {
        return formula::evaluate(t, lookup).map(|v| v as i32).unwrap_or(0);
    }
    let chars: Vec<char> = t.chars().collect();
    let (v, _, _) = numparse::strtol(&chars, 0, 10);
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn parse_hex(s: &str, out: &mut [i32; 4]) {
    let chars: Vec<char> = s.chars().collect();
    let mut start = 0;
    if chars.len() >= 2 && chars[0] == '0' && chars[1] == 'x' {
        start = 2;
    }
    let rest = &chars[start..];
    let has_alpha = rest.len() >= 8 && !rest[6].is_whitespace();
    if !(has_alpha || rest.len() >= 6) {
        return;
    }
    let count = if has_alpha { 4 } else { 3 };
    let mut pos = 0;
    for slot in out.iter_mut().take(count) {
        while pos < rest.len() && rest[pos].is_whitespace() {
            pos += 1;
        }
        let mut digits = 0;
        let mut v = 0;
        while digits < 2 && pos < rest.len() {
            match rest[pos].to_digit(16) {
                Some(d) => {
                    v = v * 16 + d as i32;
                    pos += 1;
                    digits += 1;
                }
                None => break,
            }
        }
        if digits == 0 {
            break;
        }
        *slot = v;
    }
}
