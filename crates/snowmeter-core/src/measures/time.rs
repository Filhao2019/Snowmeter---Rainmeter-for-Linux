//! Time measure (MeasureTime.cpp), built on the local-FILETIME model:
//! all arithmetic is in "seconds since 1601-01-01, local wall clock".

use super::{MeasureCtx, MeasureImpl};
use crate::numparse::parse_f64_prefix;
use crate::parser::OptionReader;
use crate::timefmt::{try_format_time_in, LocaleNames};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime};

/// Seconds between 1601-01-01 and 1970-01-01.
pub const WIN_EPOCH: i64 = 11_644_473_600;

/// Offset information of the machine's time zone at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneInfo {
    /// Full UTC offset in seconds (DST included).
    pub offset: i32,
    pub is_dst: bool,
    /// Standard (non-DST) UTC offset in seconds.
    pub std_offset: i32,
}

/// Local zone at a UTC unix time, via `localtime_r`.
pub fn system_zone(utc: i64) -> ZoneInfo {
    let t: libc::time_t = utc as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    if !ok {
        return ZoneInfo { offset: 0, is_dst: false, std_offset: 0 };
    }
    let off = tm.tm_gmtoff as i32;
    let dst = tm.tm_isdst > 0;
    ZoneInfo { offset: off, is_dst: dst, std_offset: off - if dst { 3600 } else { 0 } }
}

fn system_clock() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Local wall-clock Windows seconds of the DST transition in `year`
/// (`start`: standard -> DST). Expressed in the pre-transition offset.
pub fn dst_transition(zone: &dyn Fn(i64) -> ZoneInfo, year: i32, start: bool) -> Option<f64> {
    let jan1 = NaiveDate::from_ymd_opt(year, 1, 1)?.and_hms_opt(0, 0, 0)?.and_utc().timestamp();
    let mut prev_t = jan1;
    let mut prev = zone(prev_t).is_dst;
    for day in 1..=366 {
        let t = jan1 + day * 86400;
        let cur = zone(t).is_dst;
        if cur != prev && cur == start {
            let (mut lo, mut hi) = (prev_t, t);
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if zone(mid).is_dst == start {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let wall = hi + i64::from(zone(hi - 1).offset);
            return Some((wall + WIN_EPOCH) as f64);
        }
        prev = cur;
        prev_t = t;
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TzMode {
    Local,
    Hours(f64),
}

#[derive(Clone, Debug, PartialEq)]
enum Stamp {
    Current,
    Fixed(f64),
    Dst { start: bool, year: Option<i32>, next: bool },
}

const MONTHS: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
    "december",
];
const DAYS: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

/// Parse `s` with a strptime-style `fmt` into local Windows seconds.
/// Missing fields default to 1900-01-01 00:00:00; trailing input is ignored.
pub fn parse_timestamp(s: &str, fmt: &str) -> Option<f64> {
    // expand composite directives
    let mut f = String::new();
    let fc: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < fc.len() {
        if fc[i] == '%' && i + 1 < fc.len() {
            match fc[i + 1] {
                'D' | 'x' => f.push_str("%m/%d/%y"),
                'F' => f.push_str("%Y-%m-%d"),
                'T' | 'X' => f.push_str("%H:%M:%S"),
                'R' => f.push_str("%H:%M"),
                'r' => f.push_str("%I:%M:%S %p"),
                'c' => f.push_str("%a %b %e %H:%M:%S %Y"),
                c => {
                    f.push('%');
                    f.push(c);
                }
            }
            i += 2;
        } else {
            f.push(fc[i]);
            i += 1;
        }
    }
    let inp: Vec<char> = s.chars().collect();
    let mut p = 0;
    let (mut year, mut mon, mut day, mut hour, mut min, mut sec) = (1900i32, 1u32, 1u32, 0u32, 0u32, 0u32);
    let mut pm: Option<bool> = None;
    let mut yday: Option<u32> = None;
    let fchars: Vec<char> = f.chars().collect();
    let mut k = 0;

    fn read_num(inp: &[char], p: &mut usize, max_digits: usize) -> Option<u32> {
        while *p < inp.len() && inp[*p] == ' ' {
            *p += 1;
        }
        let start = *p;
        let mut v = 0u32;
        while *p < inp.len() && *p - start < max_digits && inp[*p].is_ascii_digit() {
            v = v * 10 + inp[*p].to_digit(10)?;
            *p += 1;
        }
        (*p > start).then_some(v)
    }
    fn read_name(inp: &[char], p: &mut usize, names: &[&str]) -> Option<usize> {
        let rest: String = inp[*p..].iter().collect::<String>().to_ascii_lowercase();
        for (i, n) in names.iter().enumerate() {
            if rest.starts_with(n) {
                *p += n.chars().count();
                return Some(i);
            }
        }
        for (i, n) in names.iter().enumerate() {
            if rest.starts_with(&n[..3]) {
                *p += 3;
                return Some(i);
            }
        }
        None
    }

    while k < fchars.len() {
        let c = fchars[k];
        if c.is_whitespace() {
            while p < inp.len() && inp[p].is_whitespace() {
                p += 1;
            }
            k += 1;
            continue;
        }
        if c != '%' {
            if p < inp.len() && inp[p] == c {
                p += 1;
                k += 1;
                continue;
            }
            return None;
        }
        k += 1;
        let d = *fchars.get(k)?;
        k += 1;
        match d {
            'Y' => year = read_num(&inp, &mut p, 4)? as i32,
            'y' => {
                let v = read_num(&inp, &mut p, 2)?;
                year = if v < 69 { 2000 + v as i32 } else { 1900 + v as i32 };
            }
            'm' => mon = read_num(&inp, &mut p, 2)?,
            'd' | 'e' => day = read_num(&inp, &mut p, 2)?,
            'H' => hour = read_num(&inp, &mut p, 2)?,
            'I' => hour = read_num(&inp, &mut p, 2)? % 12,
            'M' => min = read_num(&inp, &mut p, 2)?,
            'S' => sec = read_num(&inp, &mut p, 2)?,
            'j' => yday = Some(read_num(&inp, &mut p, 3)?),
            'b' | 'B' | 'h' => mon = read_name(&inp, &mut p, &MONTHS)? as u32 + 1,
            'a' | 'A' => {
                read_name(&inp, &mut p, &DAYS)?;
            }
            'p' => {
                let w: String = inp[p..].iter().take(2).collect::<String>().to_ascii_uppercase();
                match w.as_str() {
                    "AM" => pm = Some(false),
                    "PM" => pm = Some(true),
                    _ => return None,
                }
                p += 2;
            }
            'z' | 'Z' => {
                while p < inp.len() && !inp[p].is_whitespace() {
                    p += 1;
                }
            }
            'n' | 't' => {
                while p < inp.len() && inp[p].is_whitespace() {
                    p += 1;
                }
            }
            '%' => {
                if p < inp.len() && inp[p] == '%' {
                    p += 1;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if pm == Some(true) {
        hour += 12;
    }
    let date = match yday {
        Some(j) => NaiveDate::from_yo_opt(year, j)?,
        None => NaiveDate::from_ymd_opt(year, mon, day)?,
    };
    let dt = date.and_hms_opt(hour, min, sec)?;
    Some((dt.and_utc().timestamp() + WIN_EPOCH) as f64)
}

pub struct TimeMeasure {
    format: String,
    /// `FormatLocale`: day and month names (English when unset or unknown).
    names: Option<&'static LocaleNames>,
    tz: TzMode,
    dst: bool,
    stamp: Stamp,
    clock: Box<dyn Fn() -> f64>,
    zone: Box<dyn Fn(i64) -> ZoneInfo>,
    /// Local Windows seconds of the last update.
    time: f64,
    std_offset: i32,
    string: String,
    logged_invalid: bool,
    log: Vec<String>,
}

impl TimeMeasure {
    pub fn new() -> TimeMeasure {
        TimeMeasure::with_clock(Box::new(system_clock), Box::new(system_zone))
    }

    /// `clock` returns UTC unix seconds, `zone` the local zone at a UTC unix time.
    pub fn with_clock(clock: Box<dyn Fn() -> f64>, zone: Box<dyn Fn(i64) -> ZoneInfo>) -> TimeMeasure {
        TimeMeasure {
            format: String::new(),
            names: None,
            tz: TzMode::Local,
            dst: true,
            stamp: Stamp::Current,
            clock,
            zone,
            time: 0.0,
            std_offset: 0,
            string: String::new(),
            logged_invalid: false,
            log: Vec::new(),
        }
    }

    fn compute_time(&mut self) {
        let utc = (self.clock)();
        let z = (self.zone)(utc.floor() as i64);
        let tz_std = match self.tz {
            TzMode::Local => z.std_offset,
            TzMode::Hours(h) => (h * 3600.0) as i32,
        };
        self.time = match self.stamp.clone() {
            Stamp::Current => {
                let delta = match self.tz {
                    TzMode::Local => f64::from(z.offset),
                    TzMode::Hours(h) => h * 3600.0 + if self.dst && z.is_dst { 3600.0 } else { 0.0 },
                };
                self.std_offset = tz_std;
                utc + delta + WIN_EPOCH as f64
            }
            Stamp::Fixed(t) => {
                self.std_offset = z.std_offset;
                t
            }
            Stamp::Dst { start, year, next } => {
                self.std_offset = z.std_offset;
                let cur_year = DateTime::from_timestamp(utc.floor() as i64, 0).map(|d| d.year()).unwrap_or(1970);
                let y = year.unwrap_or(cur_year);
                let mut t = dst_transition(&*self.zone, y, start);
                if next {
                    let now_local = utc + f64::from(z.offset) + WIN_EPOCH as f64;
                    if t.is_none_or(|v| v <= now_local) {
                        t = dst_transition(&*self.zone, y + 1, start);
                    }
                }
                t.unwrap_or(0.0)
            }
        };
    }

    fn naive(&self) -> NaiveDateTime {
        let unix = (self.time.floor() as i64).saturating_sub(WIN_EPOCH);
        DateTime::from_timestamp(unix, 0).map(|d| d.naive_utc()).unwrap_or_default()
    }

    fn format_string(&mut self) -> Option<String> {
        let dt = self.naive();
        let lower = self.format.to_ascii_lowercase();
        let fmt = if self.format.is_empty() {
            "%H:%M:%S"
        } else if lower == "locale-time" {
            "%#I:%M:%S %p"
        } else if lower == "locale-date" {
            "%#m/%#d/%Y"
        } else {
            self.format.as_str()
        };
        let r = try_format_time_in(fmt, &dt, self.std_offset, self.names);
        if r.is_none() && !self.logged_invalid {
            self.log.push(format!("Time: \"Format={}\" invalid", self.format));
            self.logged_invalid = true;
        }
        r
    }
}

impl Default for TimeMeasure {
    fn default() -> Self {
        TimeMeasure::new()
    }
}

impl MeasureImpl for TimeMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.format = opt.string("Format", "");
        self.logged_invalid = false;
        let ts = opt.string("TimeStamp", "-1");
        let ts_fmt = opt.string("TimeStampFormat", "");
        self.stamp = if !ts_fmt.is_empty() {
            match parse_timestamp(&ts, &ts_fmt) {
                Some(t) => Stamp::Fixed(t),
                None => {
                    // Stock logs here for empty input too; a timestamp that has not
                    // arrived yet (WebParser still loading) is not worth a warning.
                    if !ts.trim().is_empty() {
                        self.log.push(format!("Invalid TimeStampFormat: {ts_fmt}"));
                    }
                    Stamp::Fixed(0.0)
                }
            }
        } else if let Some(rest) = ts.strip_prefix("DSTStart") {
            Stamp::Dst { start: true, year: rest.trim().parse().ok(), next: false }
        } else if let Some(rest) = ts.strip_prefix("DSTEnd") {
            Stamp::Dst { start: false, year: rest.trim().parse().ok(), next: false }
        } else if ts.eq_ignore_ascii_case("DSTNextStart") {
            Stamp::Dst { start: true, year: None, next: true }
        } else if ts.eq_ignore_ascii_case("DSTNextEnd") {
            Stamp::Dst { start: false, year: None, next: true }
        } else {
            // A formula such as `([Other:Timestamp]+3600)` is evaluated, as in Rainmeter.
            let v = if ts.trim_start().starts_with('(') { Some(opt.float("TimeStamp", -1.0)) } else { ts.trim().parse::<f64>().ok() };
            match v {
                Some(v) if v >= 0.0 => Stamp::Fixed(v),
                _ => Stamp::Current,
            }
        };

        self.names = LocaleNames::find(&opt.string("FormatLocale", ""));
        let tz = opt.string("TimeZone", "local");
        self.tz = if tz.eq_ignore_ascii_case("local") { TzMode::Local } else { TzMode::Hours(parse_f64_prefix(&tz)) };
        self.dst = opt.bool("DaylightSavingTime", true);
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.compute_time();
        let formatted = self.format_string();
        self.string = formatted.clone().unwrap_or_default();
        if self.format.is_empty() {
            self.time.floor()
        } else {
            parse_f64_prefix(&self.string)
        }
    }

    fn string_value(&self) -> Option<String> {
        Some(self.string.clone())
    }

    fn timestamp(&self) -> Option<i64> {
        Some(self.time.floor() as i64)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    const T: &str = "13354096089"; // 2024-03-05 07:08:09 Tuesday

    fn fixed(extra: &str) -> (f64, String, Vec<String>) {
        let s = skin(&format!("[M]\nMeasure=Time\nTimeStamp={T}\n{extra}\n"));
        let mut t = TimeMeasure::new();
        t.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        let v = ctx_run(&s, |c| t.update(c));
        let log = t.take_log();
        (v, t.string_value().unwrap(), log)
    }

    #[test]
    fn default_format_and_value() {
        let (v, s, _) = fixed("");
        assert_eq!(s, "07:08:09");
        assert_eq!(v, 13354096089.0);
    }

    #[test]
    fn format_string_and_wcstod_value() {
        for (fmt, text, val) in [
            ("%A", "Tuesday", 0.0),
            ("%a, %d %b %Y", "Tue, 05 Mar 2024", 0.0),
            ("%B %#d", "March 5", 0.0),
            ("%#d", "5", 5.0),
            ("%d.%m.%Y", "05.03.2024", 5.03),
            ("%Y%m%d", "20240305", 20240305.0),
            ("%H:%M", "07:08", 7.0),
            ("%H", "07", 7.0),
            ("%M", "08", 8.0),
            ("%I:%M %p", "07:08 AM", 7.0),
            ("%e", " 5", 5.0),
            ("locale-time", "7:08:09 AM", 7.0),
            ("LOCALE-DATE", "3/5/2024", 3.0),
        ] {
            let (v, s, log) = fixed(&format!("Format={fmt}"));
            assert_eq!((s.as_str(), v), (text, val), "{fmt}");
            assert!(log.is_empty());
        }
    }

    #[test]
    fn invalid_format_is_empty_with_one_log() {
        let s = skin(&format!("[M]\nMeasure=Time\nTimeStamp={T}\nFormat=%k\n"));
        let mut t = TimeMeasure::new();
        t.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        for _ in 0..3 {
            assert_eq!(ctx_run(&s, |c| t.update(c)), 0.0);
        }
        assert_eq!(t.string_value().as_deref(), Some(""));
        let log = t.take_log();
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("Format=%k"));
    }

    #[test]
    fn timestamp_selector_value() {
        let s = skin(&format!("[M]\nMeasure=Time\nTimeStamp={T}\nFormat=%A\n"));
        let mut t = TimeMeasure::new();
        t.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        ctx_run(&s, |c| t.update(c));
        assert_eq!(t.timestamp(), Some(13354096089));
    }

    #[test]
    fn timestamp_format_parsing() {
        assert_eq!(parse_timestamp("2024-03-05 07:08:09", "%Y-%m-%d %H:%M:%S"), Some(13354096089.0));
        assert_eq!(parse_timestamp("05/03/24 07:08:09 PM", "%d/%m/%y %I:%M:%S %p"), Some(13354096089.0 + 12.0 * 3600.0));
        assert_eq!(parse_timestamp("Tue Mar 5 2024", "%a %b %d %Y"), Some(13354096089.0 - 7.0 * 3600.0 - 8.0 * 60.0 - 9.0));
        assert_eq!(parse_timestamp("nonsense", "%Y"), None);
        // defaults: 1900-01-01
        assert_eq!(parse_timestamp("10:00", "%H:%M"), Some(-2208988800.0 + WIN_EPOCH as f64 + 36000.0));
        let (v, _, _) = fixed("");
        assert_eq!(v, 13354096089.0);
        let s = skin("[M]\nMeasure=Time\nTimeStamp=2024-03-05 07:08:09\nTimeStampFormat=%Y-%m-%d %H:%M:%S\n");
        let mut t = TimeMeasure::new();
        t.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(ctx_run(&s, |c| t.update(c)), 13354096089.0);
    }

    #[test]
    fn bad_timestamp_format_logs_and_uses_zero() {
        let s = skin("[M]\nMeasure=Time\nTimeStamp=xx\nTimeStampFormat=%Y\n");
        let mut t = TimeMeasure::new();
        t.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(ctx_run(&s, |c| t.update(c)), 0.0);
        assert!(t.take_log()[0].contains("Invalid TimeStampFormat"));
    }

    #[test]
    fn empty_timestamp_with_format_is_silent_zero() {
        let s = skin("[M]\nMeasure=Time\nTimeStamp=\nTimeStampFormat=%Y-%m-%dT%H:%M:%S\n");
        let mut t = TimeMeasure::new();
        t.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(ctx_run(&s, |c| t.update(c)), 0.0);
        assert!(t.take_log().is_empty());
    }

    // ---- synthetic US Pacific zone for 2024 ----
    fn us_zone(utc: i64) -> ZoneInfo {
        let start = NaiveDate::from_ymd_opt(2024, 3, 10).unwrap().and_hms_opt(10, 0, 0).unwrap().and_utc().timestamp();
        let end = NaiveDate::from_ymd_opt(2024, 11, 3).unwrap().and_hms_opt(9, 0, 0).unwrap().and_utc().timestamp();
        let dst = utc >= start && utc < end;
        ZoneInfo { offset: if dst { -7 * 3600 } else { -8 * 3600 }, is_dst: dst, std_offset: -8 * 3600 }
    }

    fn with_clock(ini: &str, utc: f64) -> (TimeMeasure, f64) {
        let s = skin(ini);
        let mut t = TimeMeasure::with_clock(Box::new(move || utc), Box::new(us_zone));
        t.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        let v = ctx_run(&s, |c| t.update(c));
        (t, v)
    }

    #[test]
    fn dst_transitions() {
        // 2024-06-01 12:00 UTC
        let utc = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap().and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() as f64;
        let (_, v) = with_clock("[M]\nMeasure=Time\nTimeStamp=DSTStart2024\n", utc);
        assert_eq!(v, 13354509600.0);
        let (t, v) = with_clock("[M]\nMeasure=Time\nTimeStamp=DSTEnd2024\nFormat=%Y-%m-%d %H:%M:%S\n", utc);
        assert_eq!(t.string_value().unwrap(), "2024-11-03 02:00:00");
        assert_ne!(v, 0.0);
        // current year defaults
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeStamp=DSTStart\nFormat=%F %T\n", utc);
        assert_eq!(t.string_value().unwrap(), "2024-03-10 02:00:00");
        // next end is still ahead of June
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeStamp=dstnextend\nFormat=%F\n", utc);
        assert_eq!(t.string_value().unwrap(), "2024-11-03");
        // next start already passed this year: 2025 has no rule in the fake zone -> 0 (1601)
        let (t, v) = with_clock("[M]\nMeasure=Time\nTimeStamp=DSTNextStart\nFormat=%Y\n", utc);
        assert_eq!(t.string_value().unwrap(), "1601");
        assert_eq!(v, 1601.0);
    }

    #[test]
    fn numeric_time_zone_and_dst_flag() {
        // 2024-06-01 12:00:00 UTC; machine zone is in DST
        let utc = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap().and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() as f64;
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeZone=0\nFormat=%H:%M\n", utc);
        assert_eq!(t.string_value().unwrap(), "13:00"); // +1h DST bias from the *local* machine
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeZone=0\nDaylightSavingTime=0\nFormat=%H:%M\n", utc);
        assert_eq!(t.string_value().unwrap(), "12:00");
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeZone=5.5\nDaylightSavingTime=0\nFormat=%H:%M %z\n", utc);
        assert_eq!(t.string_value().unwrap(), "17:30 +0530");
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeZone=local\nFormat=%H:%M %z\n", utc);
        assert_eq!(t.string_value().unwrap(), "05:00 -0800"); // standard offset even in summer
    }

    #[test]
    fn system_zone_is_consistent() {
        let z = system_zone(1_700_000_000);
        assert_eq!(z.offset - z.std_offset, if z.is_dst { 3600 } else { 0 });
    }

    #[test]
    fn current_time_value_is_windows_seconds() {
        let s = skin("[M]\nMeasure=Time\n");
        let mut t = TimeMeasure::new();
        t.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        let v = ctx_run(&s, |c| t.update(c));
        assert!(v > 13_300_000_000.0 && v < 14_500_000_000.0, "{v}");
    }

    #[test]
    fn format_locale_names() {
        let utc = 1_760_270_400.0; // 2025-10-12 12:00 UTC, a Sunday
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeZone=0\nDaylightSavingTime=0\nFormatLocale=de-DE\nFormat=%A %d %B (%a %b)\n", utc);
        assert_eq!(t.string_value().unwrap(), "Sonntag 12 Oktober (So Okt)");
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeZone=0\nDaylightSavingTime=0\nFormatLocale=ua\nFormat=%A %B\n", utc);
        assert_eq!(t.string_value().unwrap(), "неділя Жовтень");
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeZone=0\nDaylightSavingTime=0\nFormatLocale=xx\nFormat=%a %B\n", utc);
        assert_eq!(t.string_value().unwrap(), "Sun October");
    }

    #[test]
    fn timestamp_formula() {
        let (t, _) = with_clock("[M]\nMeasure=Time\nTimeStamp=(13300000000+3600)\nFormat=%H\n", 0.0);
        let (u, _) = with_clock("[M]\nMeasure=Time\nTimeStamp=13300000000\nFormat=%H\n", 0.0);
        let h = |m: &TimeMeasure| m.string_value().unwrap().parse::<i32>().unwrap();
        assert_eq!((h(&u) + 1) % 24, h(&t));
    }
}
