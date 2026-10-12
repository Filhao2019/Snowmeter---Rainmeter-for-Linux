//! Uptime (`/proc/uptime`) with a Win32 `FormatMessage` template emulator.

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

/// Parse the first field of `/proc/uptime` into whole seconds.
pub fn parse_proc_uptime(text: &str) -> Option<i64> {
    let f: f64 = text.split_whitespace().next()?.parse().ok()?;
    Some(f.floor() as i64)
}

fn printf_int(spec: &str, arg: i64) -> Option<String> {
    let mut chars = spec.chars().peekable();
    let (mut left, mut plus, mut space, mut zero, mut alt) = (false, false, false, false, false);
    while let Some(&c) = chars.peek() {
        match c {
            '-' => left = true,
            '+' => plus = true,
            ' ' => space = true,
            '0' => zero = true,
            '#' => alt = true,
            _ => break,
        }
        chars.next();
    }
    let mut width = 0usize;
    while let Some(&c) = chars.peek() {
        if let Some(d) = c.to_digit(10) {
            width = (width * 10 + d as usize).min(1000);
            chars.next();
        } else {
            break;
        }
    }
    let mut prec: Option<usize> = None;
    if chars.peek() == Some(&'.') {
        chars.next();
        let mut p = 0usize;
        while let Some(&c) = chars.peek() {
            if let Some(d) = c.to_digit(10) {
                p = (p * 10 + d as usize).min(1000);
                chars.next();
            } else {
                break;
            }
        }
        prec = Some(p);
    }
    let rest: String = chars.collect();
    let conv = rest.trim_start_matches(|c| matches!(c, 'l' | 'h' | 'I' | 'w' | 'L' | 'q' | 'j' | 'z' | 't') || c.is_ascii_digit());
    let conv = if conv.len() == 1 { conv.chars().next()? } else { return None };
    let neg = arg < 0 && matches!(conv, 'd' | 'i');
    let mut digits = match conv {
        'd' | 'i' => arg.unsigned_abs().to_string(),
        'u' => (arg as u32 as u64).to_string(),
        'x' => format!("{:x}", arg as u32),
        'X' => format!("{:X}", arg as u32),
        'o' => format!("{:o}", arg as u32),
        'c' => return Some(char::from_u32(arg as u32).map(|c| c.to_string()).unwrap_or_default()),
        _ => return None,
    };
    if let Some(p) = prec {
        while digits.len() < p {
            digits.insert(0, '0');
        }
    }
    let prefix = match (conv, alt, arg != 0) {
        ('x', true, true) => "0x",
        ('X', true, true) => "0X",
        ('o', true, _) if !digits.starts_with('0') => "0",
        _ => "",
    };
    let sign = if neg {
        "-"
    } else if plus && matches!(conv, 'd' | 'i') {
        "+"
    } else if space && matches!(conv, 'd' | 'i') {
        " "
    } else {
        ""
    };
    let len = sign.len() + prefix.len() + digits.len();
    let pad = width.saturating_sub(len);
    Some(if left {
        format!("{sign}{prefix}{digits}{}", " ".repeat(pad))
    } else if zero && prec.is_none() {
        format!("{sign}{prefix}{}{digits}", "0".repeat(pad))
    } else {
        format!("{}{sign}{prefix}{digits}", " ".repeat(pad))
    })
}

/// Emulate `FormatMessage(FROM_STRING | ARGUMENT_ARRAY)` with integer
/// arguments `args[0]` = `%1` ... Returns `None` for a template Windows would
/// reject (missing `!spec!`, argument beyond the array, bad conversion).
pub fn format_message(fmt: &str, args: &[i64]) -> Option<String> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&n) = chars.get(i) else { break };
        match n {
            '0' => return Some(out),
            '%' => out.push('%'),
            'n' => out.push_str("\r\n"),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '.' => out.push('.'),
            '!' => out.push('!'),
            ' ' => out.push(' '),
            d if d.is_ascii_digit() => {
                let mut num = d.to_digit(10).unwrap() as usize;
                if let Some(d2) = chars.get(i + 1).and_then(|c| c.to_digit(10)) {
                    num = num * 10 + d2 as usize;
                    i += 1;
                }
                let arg = *args.get(num - 1)?;
                if chars.get(i + 1) != Some(&'!') {
                    return None; // default `!s!` dereferences the integer
                }
                let start = i + 2;
                let end = chars[start..].iter().position(|c| *c == '!')? + start;
                let spec: String = chars[start..end].iter().collect();
                out.push_str(&printf_int(&spec, arg)?);
                i = end;
            }
            other => out.push(other),
        }
        i += 1;
    }
    Some(out)
}

type Source = Box<dyn FnMut() -> Option<String>>;

pub struct UptimeMeasure {
    format: String,
    add_days: bool,
    seconds_value: Option<i64>,
    source: Source,
    string: String,
    log: Vec<String>,
    logged_invalid: bool,
}

impl UptimeMeasure {
    pub fn new() -> UptimeMeasure {
        UptimeMeasure::with_source(Box::new(|| std::fs::read_to_string("/proc/uptime").ok()))
    }
    pub fn with_source(source: Source) -> UptimeMeasure {
        UptimeMeasure {
            format: "%4!i!d %3!i!:%2!02i!".to_string(),
            add_days: true,
            seconds_value: None,
            source,
            string: String::new(),
            log: Vec::new(),
            logged_invalid: false,
        }
    }
}

impl Default for UptimeMeasure {
    fn default() -> Self {
        UptimeMeasure::new()
    }
}

impl MeasureImpl for UptimeMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.format = opt.string("Format", "%4!i!d %3!i!:%2!02i!");
        self.add_days = if self.format.contains("%4") { false } else { opt.bool("AddDaysToHours", true) };
        self.seconds_value = if opt.has("SecondsValue") { Some((opt.float("SecondsValue", 0.0) as i64).abs()) } else { None };
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let secs = match self.seconds_value {
            Some(s) => s,
            None => (self.source)().and_then(|t| parse_proc_uptime(&t)).unwrap_or(0),
        };
        let hours = if self.add_days { secs / 3600 } else { (secs / 3600) % 24 };
        let args = [secs % 60, (secs / 60) % 60, hours, secs / 86400];
        match format_message(&self.format, &args) {
            Some(s) => {
                self.string = s;
                self.logged_invalid = false;
            }
            None => {
                self.string.clear();
                if !self.logged_invalid {
                    self.log.push(format!("Uptime: \"Format={}\" invalid", self.format));
                    self.logged_invalid = true;
                }
            }
        }
        secs as f64
    }

    fn string_value(&self) -> Option<String> {
        Some(self.string.clone())
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    // 93784 s = 1 d 2 h 3 m 4 s
    const ARGS: [i64; 4] = [4, 3, 26, 1];
    const ARGS_NO_DAYS: [i64; 4] = [4, 3, 2, 1];

    #[test]
    fn format_message_vectors() {
        assert_eq!(format_message("%4!i!d %3!i!:%2!02i!", &ARGS_NO_DAYS).unwrap(), "1d 2:03");
        assert_eq!(format_message("%3!i!:%2!02i!:%1!02i!", &ARGS).unwrap(), "26:03:04");
        assert_eq!(format_message("%3!i!:%2!02i!:%1!02i!", &ARGS_NO_DAYS).unwrap(), "2:03:04");
        assert_eq!(format_message("%4!i! days, %3!i! hours", &ARGS_NO_DAYS).unwrap(), "1 days, 2 hours");
        assert_eq!(format_message("%4!i!d%n%3!i!h", &ARGS_NO_DAYS).unwrap(), "1d\r\n2h");
        assert_eq!(format_message("100%%", &ARGS).unwrap(), "100%");
    }

    #[test]
    fn format_message_invalid_templates() {
        assert_eq!(format_message("%1", &ARGS), None);
        assert_eq!(format_message("%5!i!", &ARGS), None);
        assert_eq!(format_message("%1!i", &ARGS), None);
        assert_eq!(format_message("%1!q!", &ARGS), None);
    }

    #[test]
    fn format_message_escapes_and_specs() {
        assert_eq!(format_message("a%tb%.c%!d% e", &ARGS).unwrap(), "a\tb.c!d e");
        assert_eq!(format_message("x%0y", &ARGS).unwrap(), "x");
        assert_eq!(format_message("%1!5d!|%1!-5d!|%1!+d!|%1!x!|%1!#X!", &[255]).unwrap(), "  255|255  |+255|ff|0XFF");
        assert_eq!(format_message("%1!03i!", &[7]).unwrap(), "007");
        assert_eq!(format_message("%1!.3i!", &[7]).unwrap(), "007");
        assert_eq!(format_message("%1!i!", &[-4]).unwrap(), "-4");
    }

    #[test]
    fn parses_proc_uptime() {
        assert_eq!(parse_proc_uptime("93784.55 187000.1\n"), Some(93784));
        assert_eq!(parse_proc_uptime(""), None);
    }

    fn run(ini: &str) -> (f64, Option<String>, Vec<String>) {
        let s = skin(ini);
        let mut u = UptimeMeasure::with_source(Box::new(|| Some("93784.20 1.0\n".to_string())));
        u.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        let v = ctx_run(&s, |c| u.update(c));
        let l = u.take_log();
        (v, u.string_value(), l)
    }

    #[test]
    fn default_format() {
        let (v, s, _) = run("[M]\nMeasure=Uptime\n");
        assert_eq!(v, 93784.0);
        // default AddDaysToHours=1 and no %4 ... default format has %4, so days are separate
        assert_eq!(s.as_deref(), Some("1d 2:03"));
    }

    #[test]
    fn add_days_to_hours() {
        let (_, s, _) = run("[M]\nMeasure=Uptime\nFormat=%3!i!:%2!02i!:%1!02i!\n");
        assert_eq!(s.as_deref(), Some("26:03:04"));
        let (_, s, _) = run("[M]\nMeasure=Uptime\nFormat=%3!i!:%2!02i!:%1!02i!\nAddDaysToHours=0\n");
        assert_eq!(s.as_deref(), Some("2:03:04"));
    }

    #[test]
    fn seconds_value_overrides_system_uptime() {
        let (v, s, _) = run("[M]\nMeasure=Uptime\nSecondsValue=3661\nFormat=%3!i!:%2!02i!:%1!02i!\n");
        assert_eq!(v, 3661.0);
        assert_eq!(s.as_deref(), Some("1:01:01"));
        let (v, s, _) = run("[M]\nMeasure=Uptime\nSecondsValue=-90\n");
        assert_eq!(v, 90.0);
        assert_eq!(s.as_deref(), Some("0d 0:01"));
    }

    #[test]
    fn invalid_format_gives_empty_string_and_one_log() {
        let (_, s, log) = run("[M]\nMeasure=Uptime\nFormat=%1\n");
        assert_eq!(s.as_deref(), Some(""));
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("Uptime"));
    }

    #[test]
    fn real_proc_uptime() {
        let s = skin("[M]\nMeasure=Uptime\n");
        let mut u = UptimeMeasure::new();
        u.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert!(ctx_run(&s, |c| u.update(c)) > 0.0);
    }
}
