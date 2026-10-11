//! C-library style number scanning (`strtol` / `strtod` prefix semantics).
//!
//! Rainmeter reads numbers with `wcstol`/`wcstod`: leading whitespace is
//! skipped, the longest valid numeric prefix is used, trailing garbage is
//! ignored and a non-numeric string reads as zero.

/// Scan an integer in `base` from `chars[start..]`.
/// Returns `(value, end_index, overflowed_i64)`; `end_index == start` means
/// that nothing was consumed (C leaves `endptr` at the start in that case).
pub(crate) fn strtol(chars: &[char], start: usize, base: u32) -> (i64, usize, bool) {
    let mut i = start;
    while i < chars.len() && is_c_space(chars[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        neg = chars[i] == '-';
        i += 1;
    }
    if base == 16
        && i + 2 < chars.len()
        && chars[i] == '0'
        && (chars[i + 1] == 'x' || chars[i + 1] == 'X')
        && chars[i + 2].is_digit(16)
    {
        i += 2;
    }
    let digits_start = i;
    let mut value: i128 = 0;
    let mut overflow = false;
    while i < chars.len() {
        match chars[i].to_digit(base) {
            Some(d) => {
                value = value * base as i128 + d as i128;
                if value > i64::MAX as i128 + 1 {
                    overflow = true;
                    value = i64::MAX as i128 + 1;
                }
                i += 1;
            }
            None => break,
        }
    }
    if i == digits_start {
        return (0, start, false);
    }
    let signed = if neg { -value } else { value };
    let clamped = signed.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    (clamped, i, overflow || signed > i64::MAX as i128)
}

/// Scan a floating point number from `chars[start..]` (decimal, exponent and
/// C99 hexadecimal forms). Returns `(value, end_index)`; nothing consumed
/// gives `(0.0, start)`.
pub(crate) fn strtod(chars: &[char], start: usize) -> (f64, usize) {
    let mut i = start;
    while i < chars.len() && is_c_space(chars[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        neg = chars[i] == '-';
        i += 1;
    }
    // Hexadecimal: 0x[hex digits][.hex digits][p[+-]dec]
    if i + 2 < chars.len()
        && chars[i] == '0'
        && (chars[i + 1] == 'x' || chars[i + 1] == 'X')
        && (chars[i + 2].is_digit(16) || (chars[i + 2] == '.' && i + 3 < chars.len() && chars[i + 3].is_digit(16)))
    {
        let mut j = i + 2;
        let mut mant = 0f64;
        while j < chars.len() && chars[j].is_digit(16) {
            mant = mant * 16.0 + chars[j].to_digit(16).unwrap() as f64;
            j += 1;
        }
        if j < chars.len() && chars[j] == '.' {
            j += 1;
            let mut scale = 1.0 / 16.0;
            while j < chars.len() && chars[j].is_digit(16) {
                mant += chars[j].to_digit(16).unwrap() as f64 * scale;
                scale /= 16.0;
                j += 1;
            }
        }
        if j < chars.len() && (chars[j] == 'p' || chars[j] == 'P') {
            let mut k = j + 1;
            let mut eneg = false;
            if k < chars.len() && (chars[k] == '+' || chars[k] == '-') {
                eneg = chars[k] == '-';
                k += 1;
            }
            let ds = k;
            let mut e: i32 = 0;
            while k < chars.len() && chars[k].is_ascii_digit() {
                e = e.saturating_mul(10).saturating_add(chars[k].to_digit(10).unwrap() as i32);
                k += 1;
            }
            if k > ds {
                mant *= 2f64.powi(if eneg { -e } else { e });
                j = k;
            }
        }
        return (if neg { -mant } else { mant }, j);
    }
    // Decimal.
    let int_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits: String = chars[int_start..i].iter().collect();
    let mut frac_digits = String::new();
    if i < chars.len() && chars[i] == '.' {
        let fs = i + 1;
        let mut j = fs;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if !int_digits.is_empty() || j > fs {
            frac_digits = chars[fs..j].iter().collect();
            i = j;
        }
    }
    if int_digits.is_empty() && frac_digits.is_empty() {
        return (0.0, start);
    }
    let mut exp = String::new();
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut k = i + 1;
        let mut sign = "";
        if k < chars.len() && (chars[k] == '+' || chars[k] == '-') {
            if chars[k] == '-' {
                sign = "-";
            }
            k += 1;
        }
        let ds = k;
        while k < chars.len() && chars[k].is_ascii_digit() {
            k += 1;
        }
        if k > ds {
            exp = format!("e{}{}", sign, chars[ds..k].iter().collect::<String>());
            i = k;
        }
    }
    let text = format!(
        "{}.{}{}",
        if int_digits.is_empty() { "0" } else { &int_digits },
        if frac_digits.is_empty() { "0" } else { &frac_digits },
        exp
    );
    let v: f64 = text.parse().unwrap_or(0.0);
    (if neg { -v } else { v }, i)
}

fn is_c_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// `ReadFloat`-style scan of a whole string: numeric prefix or 0.
pub(crate) fn parse_f64_prefix(s: &str) -> f64 {
    let chars: Vec<char> = s.chars().collect();
    strtod(&chars, 0).0
}

/// `ReadInt`-style scan with a 32-bit `long`: `None` on range error.
pub(crate) fn parse_i32_prefix(s: &str) -> Option<i64> {
    let chars: Vec<char> = s.chars().collect();
    let (v, _, overflow) = strtol(&chars, 0, 10);
    if overflow || v > i32::MAX as i64 || v < i32::MIN as i64 {
        None
    } else {
        Some(v)
    }
}
