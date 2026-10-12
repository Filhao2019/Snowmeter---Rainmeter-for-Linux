//! Number formatting shared by all measures (Measure.cpp `GetFormattedValue`).

/// AutoScale option values as used by `Measure::string_value`.
pub const AUTOSCALE_OFF: u8 = 0;
pub const AUTOSCALE_1024: u8 = 1;
pub const AUTOSCALE_1000: u8 = 2;
pub const AUTOSCALE_1024K: u8 = 3;
pub const AUTOSCALE_1000K: u8 = 4;

/// Parse the meter `AutoScale` option (`0`, `1`, `2`, `1k`, `2k`).
pub fn parse_auto_scale(s: &str) -> u8 {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" => AUTOSCALE_1024,
        "2" => AUTOSCALE_1000,
        "1k" => AUTOSCALE_1024K,
        "2k" => AUTOSCALE_1000K,
        _ => AUTOSCALE_OFF,
    }
}

/// Remove trailing zeros and a trailing `.` (`RemoveTrailingZero`).
pub fn remove_trailing_zero(s: &mut String) {
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
}

/// Median of three values (4.5 auto-max window).
pub fn median3(a: f64, b: f64, c: f64) -> f64 {
    let mut v = [a, b, c];
    v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    v[1]
}

/// Escape regular expression syntax (`\^$|()[{.+*?`), used by `[M:EscapeRegExp]`.
pub fn escape_regexp(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\^$|()[{.+*?".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// UTF-8 percent-encoding of everything except `0-9 A-Z a-z - . _ ~`.
pub fn encode_url(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Scaled value text like `1.5 G` (`GetScaledValue`).
pub fn scaled_value(auto_scale: u8, decimals: i32, value: f64) -> String {
    let base = if auto_scale == AUTOSCALE_1000 || auto_scale == AUTOSCALE_1000K { 1000.0f64 } else { 1024.0 };
    let tbl = [base.powi(4), base.powi(3), base.powi(2), base];
    let k_only = auto_scale == AUTOSCALE_1024K || auto_scale == AUTOSCALE_1000K;
    let d = decimals.clamp(0, 32) as usize;
    let (v, suffix) = if value >= tbl[0] {
        (value / tbl[0], " T")
    } else if value >= tbl[1] {
        (value / tbl[1], " G")
    } else if value >= tbl[2] {
        (value / tbl[2], " M")
    } else if k_only || value >= tbl[3] {
        (value / tbl[3], " k")
    } else {
        (value, " ")
    };
    format!("{v:.d$}{suffix}")
}

/// `GetFormattedValue` without the Substitute step.
pub fn format_value(value: f64, relative: f64, auto_scale: u8, scale: f64, decimals: i32, percentual: bool) -> String {
    if percentual {
        let d = decimals.clamp(0, 32) as usize;
        format!("{:.d$}", 100.0 * relative)
    } else if auto_scale != AUTOSCALE_OFF {
        scaled_value(auto_scale, decimals, value)
    } else {
        let v = value / scale;
        if decimals == -1 {
            let mut s = format!("{v:.5}");
            remove_trailing_zero(&mut s);
            s
        } else {
            let d = decimals.clamp(0, 32) as usize;
            format!("{v:.d$}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_scale_option_parsing() {
        assert_eq!(parse_auto_scale("0"), 0);
        assert_eq!(parse_auto_scale("1"), 1);
        assert_eq!(parse_auto_scale("2"), 2);
        assert_eq!(parse_auto_scale("1k"), 3);
        assert_eq!(parse_auto_scale("2K"), 4);
        assert_eq!(parse_auto_scale(""), 0);
        assert_eq!(parse_auto_scale("junk"), 0);
    }

    #[test]
    fn trailing_zero_removal() {
        let t = |s: &str| {
            let mut x = s.to_string();
            remove_trailing_zero(&mut x);
            x
        };
        assert_eq!(t("37.50000"), "37.5");
        assert_eq!(t("100.00000"), "100");
        assert_eq!(t("0.33333"), "0.33333");
        // C code strips zeros even without a decimal point
        assert_eq!(t("100"), "1");
        assert_eq!(t("0"), "");
        assert_eq!(t("0.00000"), "0");
    }

    #[test]
    fn autoscale_1024_table() {
        assert_eq!(scaled_value(AUTOSCALE_1024, 1, 1536.0), "1.5 k");
        assert_eq!(scaled_value(AUTOSCALE_1024, 0, 500.0), "500 ");
        assert_eq!(scaled_value(AUTOSCALE_1024, 1, 1024.0 * 1024.0 * 1.5), "1.5 M");
        assert_eq!(scaled_value(AUTOSCALE_1024, 1, 3.0 * 1024f64.powi(3)), "3.0 G");
        assert_eq!(scaled_value(AUTOSCALE_1024, 2, 2.0 * 1024f64.powi(4)), "2.00 T");
        assert_eq!(scaled_value(AUTOSCALE_1024, 0, 1023.0), "1023 ");
    }

    #[test]
    fn autoscale_1000_table() {
        assert_eq!(scaled_value(AUTOSCALE_1000, 1, 1500.0), "1.5 k");
        assert_eq!(scaled_value(AUTOSCALE_1000, 0, 999.0), "999 ");
        assert_eq!(scaled_value(AUTOSCALE_1000, 1, 2.5e9), "2.5 G");
    }

    #[test]
    fn autoscale_k_variants_always_use_kilo() {
        assert_eq!(scaled_value(AUTOSCALE_1024K, 1, 512.0), "0.5 k");
        assert_eq!(scaled_value(AUTOSCALE_1000K, 1, 500.0), "0.5 k");
        assert_eq!(scaled_value(AUTOSCALE_1024K, 1, 2048.0), "2.0 k");
    }

    #[test]
    fn negative_values_are_not_scaled() {
        assert_eq!(scaled_value(AUTOSCALE_1024, 0, -5000.0), "-5000 ");
        assert_eq!(scaled_value(AUTOSCALE_1024K, 1, -2048.0), "-2.0 k");
    }

    #[test]
    fn plain_formatting() {
        assert_eq!(format_value(37.0, 0.0, 0, 1.0, 0, false), "37");
        assert_eq!(format_value(2.5, 0.0, 0, 1.0, 0, false), "2");
        assert_eq!(format_value(0.125, 0.0, 0, 1.0, 2, false), "0.12");
        assert_eq!(format_value(37.5, 0.0, 0, 1.0, -1, false), "37.5");
        assert_eq!(format_value(100.0, 0.0, 0, 1.0, -1, false), "100");
        assert_eq!(format_value(1.0 / 3.0, 0.0, 0, 1.0, -1, false), "0.33333");
        assert_eq!(format_value(2048.0, 0.0, 0, 1024.0, 1, false), "2.0");
        assert_eq!(format_value(-3.0, 0.0, 0, 1.0, -1, false), "-3");
        assert_eq!(format_value(0.0, 0.0, 0, 1.0, -1, false), "0");
    }

    #[test]
    fn percentual_and_autoscale_formatting() {
        assert_eq!(format_value(30.0, 0.5, 0, 1.0, 0, true), "50");
        assert_eq!(format_value(30.0, 0.5, 0, 1.0, 1, true), "50.0");
        assert_eq!(format_value(1536.0, 0.0, 1, 1.0, 1, false), "1.5 k");
        // percentual wins over autoscale
        assert_eq!(format_value(1536.0, 0.25, 1, 1.0, 0, true), "25");
    }
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn median_of_three() {
        assert_eq!(median3(1.0, 5.0, 3.0), 3.0);
        assert_eq!(median3(0.0, 0.0, 9.0), 0.0);
    }

    #[test]
    fn escape_and_encode_vectors() {
        let s = " !*'();:@test&=+$,/?#[ing]";
        assert_eq!(encode_url(s), "%20%21%2A%27%28%29%3B%3A%40test%26%3D%2B%24%2C%2F%3F%23%5Bing%5D");
        assert_eq!(escape_regexp(s), " !\\*'\\(\\);:@test&=\\+\\$,/\\?#\\[ing]");
        assert_eq!(encode_url("é"), "%C3%A9");
    }
}
