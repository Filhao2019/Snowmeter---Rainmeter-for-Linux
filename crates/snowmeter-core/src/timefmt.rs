//! MSVC/UCRT-compatible `strftime` (C locale, English names).
//!
//! Rainmeter links the CRT statically and never sets `LC_TIME`, so month and
//! day names are English unless a Time measure sets `FormatLocale`, which
//! [`LocaleNames::find`] covers for the common European languages. glibc
//! differs (`%k`, `%P`, `%-d`, ... are valid there), so this is a custom formatter.

use chrono::{Datelike, NaiveDateTime, Timelike};
use std::fmt::Write;

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];
const MAX_LEN: usize = 4095;

/// Day and month names for `FormatLocale` (`%a %A %b %B`).
pub struct LocaleNames {
    pub code: &'static str,
    pub days: [&'static str; 7],
    pub short_days: [&'static str; 7],
    pub months: [&'static str; 12],
    pub short_months: [&'static str; 12],
}

const LOCALES: &[LocaleNames] = &[
    LocaleNames {
        code: "en",
        days: DAYS,
        short_days: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
        months: MONTHS,
        short_months: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
    },
    LocaleNames {
        code: "de",
        days: ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"],
        short_days: ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"],
        months: ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
        short_months: ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"],
    },
    LocaleNames {
        code: "fr",
        days: ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"],
        short_days: ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."],
        months: ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"],
        short_months: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
    },
    LocaleNames {
        code: "es",
        days: ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"],
        short_days: ["dom.", "lun.", "mar.", "mié.", "jue.", "vie.", "sáb."],
        months: ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
        short_months: ["ene.", "feb.", "mar.", "abr.", "may.", "jun.", "jul.", "ago.", "sep.", "oct.", "nov.", "dic."],
    },
    LocaleNames {
        code: "it",
        days: ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"],
        short_days: ["dom", "lun", "mar", "mer", "gio", "ven", "sab"],
        months: ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"],
        short_months: ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
    },
    LocaleNames {
        code: "pt",
        days: ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"],
        short_days: ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"],
        months: ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"],
        short_months: ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"],
    },
    LocaleNames {
        code: "ru",
        days: ["воскресенье", "понедельник", "вторник", "среда", "четверг", "пятница", "суббота"],
        short_days: ["Вс", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб"],
        months: ["Январь", "Февраль", "Март", "Апрель", "Май", "Июнь", "Июль", "Август", "Сентябрь", "Октябрь", "Ноябрь", "Декабрь"],
        short_months: ["янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"],
    },
    LocaleNames {
        code: "uk",
        days: ["неділя", "понеділок", "вівторок", "середа", "четвер", "п'ятниця", "субота"],
        short_days: ["Нд", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб"],
        months: ["Січень", "Лютий", "Березень", "Квітень", "Травень", "Червень", "Липень", "Серпень", "Вересень", "Жовтень", "Листопад", "Грудень"],
        short_months: ["січ", "лют", "бер", "кві", "тра", "чер", "лип", "сер", "вер", "жов", "лис", "гру"],
    },
];

impl LocaleNames {
    /// Names for a `FormatLocale` value such as `de`, `fr-FR`, `pt_BR` or `ua`;
    /// `None` for an empty value or an unknown language (English is used).
    pub fn find(locale: &str) -> Option<&'static LocaleNames> {
        let lang = locale.trim().split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
        let lang = if lang == "ua" { "uk".to_string() } else { lang };
        LOCALES.iter().find(|l| l.code == lang)
    }
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn year_days(y: i32) -> i32 {
    if is_leap(y) {
        366
    } else {
        365
    }
}

/// ISO-8601 (year, week) of a date given by year, 0-based yday and weekday (Sun=0).
fn iso_week(year: i32, yday: i32, wday: i32) -> (i32, i32) {
    let mut d = yday - if wday == 0 { 7 } else { wday } + 4;
    let mut y = year;
    if d < 0 {
        y -= 1;
        d += year_days(y);
    } else if d >= year_days(year) {
        d -= year_days(year);
        y += 1;
    }
    (y, d / 7 + 1)
}

struct Fields {
    names: &'static LocaleNames,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
    wday: i32,
    yday: i32,
}

/// Number with optional zero padding (`#` flag removes the padding).
fn num(out: &mut String, v: i64, width: usize, pad: char, hash: bool) {
    if hash {
        let _ = write!(out, "{v}");
    } else if pad == '0' {
        let _ = write!(out, "{v:0width$}");
    } else {
        let _ = write!(out, "{v:width$}");
    }
}

fn directive(out: &mut String, c: char, hash: bool, f: &Fields, tz: i32) -> bool {
    let h12 = if f.hour % 12 == 0 { 12 } else { f.hour % 12 };
    match c {
        'a' => out.push_str(f.names.short_days[f.wday as usize]),
        'A' => out.push_str(f.names.days[f.wday as usize]),
        'b' | 'h' => out.push_str(f.names.short_months[f.month as usize - 1]),
        'B' => out.push_str(f.names.months[f.month as usize - 1]),
        'c' => {
            if hash {
                // long date + ' ' + time
                directive(out, 'x', true, f, tz);
                out.push(' ');
                directive(out, 'X', false, f, tz);
            } else {
                // C locale: %a %b %e %T %Y
                for (i, ch) in ['a', 'b', 'e', 'T', 'Y'].iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    directive(out, *ch, false, f, tz);
                }
            }
        }
        'C' => num(out, (f.year / 100) as i64, 2, '0', hash),
        'd' => num(out, f.day as i64, 2, '0', hash),
        'D' => {
            for (i, ch) in ['m', 'd', 'y'].iter().enumerate() {
                if i > 0 {
                    out.push('/');
                }
                directive(out, *ch, hash, f, tz);
            }
        }
        'e' => num(out, f.day as i64, 2, ' ', hash),
        'F' => {
            directive(out, 'Y', hash, f, tz);
            out.push('-');
            directive(out, 'm', hash, f, tz);
            out.push('-');
            directive(out, 'd', hash, f, tz);
        }
        'g' => num(out, (iso_week(f.year, f.yday, f.wday).0.rem_euclid(100)) as i64, 2, '0', false),
        'G' => num(out, iso_week(f.year, f.yday, f.wday).0 as i64, 4, '0', false),
        'H' => num(out, f.hour as i64, 2, '0', hash),
        'I' => num(out, h12 as i64, 2, '0', hash),
        'j' => num(out, f.yday as i64 + 1, 3, '0', hash),
        'm' => num(out, f.month as i64, 2, '0', hash),
        'M' => num(out, f.min as i64, 2, '0', hash),
        'n' => out.push('\n'),
        'p' => out.push_str(if f.hour < 12 { "AM" } else { "PM" }),
        'r' => {
            directive(out, 'I', hash, f, tz);
            out.push(':');
            directive(out, 'M', hash, f, tz);
            out.push(':');
            directive(out, 'S', hash, f, tz);
            out.push(' ');
            directive(out, 'p', false, f, tz);
        }
        'R' => {
            directive(out, 'H', hash, f, tz);
            out.push(':');
            directive(out, 'M', hash, f, tz);
        }
        'S' => num(out, f.sec as i64, 2, '0', hash),
        't' => out.push('\t'),
        'T' => {
            directive(out, 'H', hash, f, tz);
            out.push(':');
            directive(out, 'M', hash, f, tz);
            out.push(':');
            directive(out, 'S', hash, f, tz);
        }
        'u' => num(out, if f.wday == 0 { 7 } else { f.wday as i64 }, 1, '0', false),
        'U' => num(out, (f.yday / 7 + i32::from(f.wday <= f.yday % 7)) as i64, 2, '0', hash),
        'V' => num(out, iso_week(f.year, f.yday, f.wday).1 as i64, 2, '0', hash),
        'w' => num(out, f.wday as i64, 1, '0', false),
        'W' => {
            let wd = (f.wday + 6) % 7;
            num(out, (f.yday / 7 + i32::from(wd <= f.yday % 7)) as i64, 2, '0', hash)
        }
        'x' => {
            if hash {
                // C locale long date "dddd, MMMM dd, yyyy"
                directive(out, 'A', false, f, tz);
                out.push_str(", ");
                directive(out, 'B', false, f, tz);
                out.push(' ');
                directive(out, 'd', false, f, tz);
                out.push_str(", ");
                directive(out, 'Y', false, f, tz);
            } else {
                for (i, ch) in ['m', 'd', 'y'].iter().enumerate() {
                    if i > 0 {
                        out.push('/');
                    }
                    directive(out, *ch, false, f, tz);
                }
            }
        }
        'X' => {
            directive(out, 'T', false, f, tz);
        }
        'y' => num(out, f.year.rem_euclid(100) as i64, 2, '0', hash),
        'Y' => num(out, f.year as i64, 4, '0', false),
        'z' => {
            let sign = if tz < 0 { '-' } else { '+' };
            let a = tz.abs();
            let _ = write!(out, "{sign}{:02}{:02}", a / 3600, a % 3600 / 60);
        }
        'Z' => {
            if tz == 0 {
                out.push_str("UTC");
            } else {
                let sign = if tz < 0 { '-' } else { '+' };
                let a = tz.abs();
                let _ = write!(out, "UTC{sign}{:02}:{:02}", a / 3600, a % 3600 / 60);
            }
        }
        '%' => out.push('%'),
        _ => return false,
    }
    true
}

/// Format `dt` like the UCRT `strftime` does with the C locale.
/// `tz_offset_secs` is the *standard* UTC offset used by `%z`/`%Z`.
/// Returns `None` for an invalid directive or output longer than 4095 chars.
pub fn try_format_time(fmt: &str, dt: &NaiveDateTime, tz_offset_secs: i32) -> Option<String> {
    try_format_time_in(fmt, dt, tz_offset_secs, None)
}

/// [`try_format_time`] with day and month names from `names` (English when `None`).
pub fn try_format_time_in(fmt: &str, dt: &NaiveDateTime, tz_offset_secs: i32, names: Option<&'static LocaleNames>) -> Option<String> {
    let f = Fields {
        names: names.unwrap_or(&LOCALES[0]),
        year: dt.year(),
        month: dt.month(),
        day: dt.day(),
        hour: dt.hour(),
        min: dt.minute(),
        sec: dt.second(),
        wday: dt.weekday().num_days_from_sunday() as i32,
        yday: dt.ordinal0() as i32,
    };
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut next = chars.next()?;
        let hash = next == '#';
        if hash {
            next = chars.next()?;
        }
        // E and O modifiers are accepted and ignored for their valid sets.
        let code = match next {
            'E' => {
                let n = chars.next()?;
                if !"cCxXyY".contains(n) {
                    return None;
                }
                n
            }
            'O' => {
                let n = chars.next()?;
                if !"deHImMSuUVwWy".contains(n) {
                    return None;
                }
                n
            }
            n => n,
        };
        if !directive(&mut out, code, hash, &f, tz_offset_secs) {
            return None;
        }
    }
    if out.len() > MAX_LEN {
        return None;
    }
    Some(out)
}

/// Like [`try_format_time`] but an invalid format gives an empty string.
pub fn format_time(fmt: &str, dt: &NaiveDateTime, tz_offset_secs: i32) -> String {
    try_format_time(fmt, dt, tz_offset_secs).unwrap_or_default()
}
