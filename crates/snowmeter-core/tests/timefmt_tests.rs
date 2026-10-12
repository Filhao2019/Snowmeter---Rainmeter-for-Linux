use chrono::NaiveDate;
use snowmeter_core::timefmt::{format_time, try_format_time};

fn t() -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2024, 3, 5).unwrap().and_hms_opt(7, 8, 9).unwrap()
}
fn f(fmt: &str) -> String {
    format_time(fmt, &t(), 0)
}
fn at(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32, fmt: &str) -> String {
    let dt = NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, s).unwrap();
    format_time(fmt, &dt, 0)
}

#[test]
fn names_and_basic_codes() {
    assert_eq!(f("%a"), "Tue");
    assert_eq!(f("%A"), "Tuesday");
    assert_eq!(f("%b"), "Mar");
    assert_eq!(f("%h"), "Mar");
    assert_eq!(f("%B"), "March");
    assert_eq!(f("%C"), "20");
    assert_eq!(f("%d"), "05");
    assert_eq!(f("%e"), " 5");
    assert_eq!(f("%H"), "07");
    assert_eq!(f("%I"), "07");
    assert_eq!(f("%j"), "065");
    assert_eq!(f("%m"), "03");
    assert_eq!(f("%M"), "08");
    assert_eq!(f("%p"), "AM");
    assert_eq!(f("%S"), "09");
    assert_eq!(f("%y"), "24");
    assert_eq!(f("%Y"), "2024");
    assert_eq!(f("%%"), "%");
    assert_eq!(f("%n"), "\n");
    assert_eq!(f("%t"), "\t");
}

#[test]
fn composite_codes() {
    assert_eq!(f("%D"), "03/05/24");
    assert_eq!(f("%F"), "2024-03-05");
    assert_eq!(f("%T"), "07:08:09");
    assert_eq!(f("%R"), "07:08");
    assert_eq!(f("%r"), "07:08:09 AM");
    assert_eq!(f("%c"), "Tue Mar  5 07:08:09 2024");
    assert_eq!(f("%x"), "03/05/24");
    assert_eq!(f("%X"), "07:08:09");
}

#[test]
fn week_codes() {
    assert_eq!(f("%U %W %V %G %g %u %w"), "09 10 10 2024 24 2 2");
    assert_eq!(at(2024, 12, 30, 0, 0, 0, "%G %V %U %W %j"), "2025 01 52 53 365");
    assert_eq!(at(2021, 1, 1, 0, 0, 0, "%G %V %U %W"), "2020 53 00 00");
    assert_eq!(at(2024, 3, 3, 0, 0, 0, "%u %w"), "7 0"); // Sunday
}

#[test]
fn hash_flag() {
    assert_eq!(f("%#d"), "5");
    assert_eq!(f("%#H:%M"), "7:08");
    assert_eq!(f("%#I"), "7");
    assert_eq!(f("%#j"), "65");
    assert_eq!(f("%#m"), "3");
    assert_eq!(f("%#M"), "8");
    assert_eq!(f("%#S"), "9");
    assert_eq!(f("%#y"), "24");
    assert_eq!(f("%#U"), "9");
    assert_eq!(f("%#W"), "10");
    assert_eq!(f("%#V"), "10");
    assert_eq!(f("%#e"), "5");
    assert_eq!(f("%#D"), "3/5/24");
    assert_eq!(f("%#F"), "2024-3-5");
    assert_eq!(f("%#T"), "7:8:9");
    assert_eq!(f("%#R"), "7:8");
    assert_eq!(f("%#r"), "7:8:9 AM");
    assert_eq!(f("%#c"), "Tuesday, March 05, 2024 07:08:09");
    assert_eq!(f("%#x"), "Tuesday, March 05, 2024");
    assert_eq!(f("%#X"), "07:08:09");
    assert_eq!(f("%#a"), "Tue");
    assert_eq!(f("%#A"), "Tuesday");
    assert_eq!(f("%#B"), "March");
    assert_eq!(f("%#Y"), "2024");
    assert_eq!(f("%#C"), "20");
    assert_eq!(f("%#p"), "AM");
    assert_eq!(f("%#%"), "%");
    assert_eq!(at(2005, 1, 2, 0, 0, 0, "%y %#y"), "05 5");
}

#[test]
fn mixed_formats_from_vectors() {
    assert_eq!(f("%a, %d %b %Y"), "Tue, 05 Mar 2024");
    assert_eq!(f("%B %#d"), "March 5");
    assert_eq!(f("%d.%m.%Y"), "05.03.2024");
    assert_eq!(f("%Y%m%d"), "20240305");
    assert_eq!(f("%I:%M %p"), "07:08 AM");
    assert_eq!(f("literal text"), "literal text");
    assert_eq!(f(""), "");
}

#[test]
fn twelve_hour_clock() {
    assert_eq!(at(2024, 3, 5, 12, 0, 0, "%I %p"), "12 PM");
    assert_eq!(at(2024, 3, 5, 0, 30, 0, "%I %p"), "12 AM");
    assert_eq!(at(2024, 3, 5, 13, 0, 0, "%I %p"), "01 PM");
    assert_eq!(at(2024, 3, 5, 23, 59, 59, "%#I:%M %p"), "11:59 PM");
}

#[test]
fn timezone_codes() {
    let dt = t();
    assert_eq!(format_time("%z", &dt, 0), "+0000");
    assert_eq!(format_time("%z", &dt, -8 * 3600), "-0800");
    assert_eq!(format_time("%z", &dt, 3600 + 1800), "+0130");
}

#[test]
fn ignored_e_and_o_modifiers() {
    assert_eq!(f("%Ec"), f("%c"));
    assert_eq!(f("%Ex"), f("%x"));
    assert_eq!(f("%Od"), "05");
    assert_eq!(f("%OH:%OM:%OS"), "07:08:09");
    assert_eq!(f("%Oy"), "24");
}

#[test]
fn invalid_directives_give_empty_string() {
    for bad in ["%k", "%P", "%s", "%-d", "abc%", "%i", "%f", "%q", "%N", "%L", "%+", "%E", "%Ez", "%Oa"] {
        assert_eq!(f(bad), "", "format {bad:?} must be invalid");
        assert!(try_format_time(bad, &t(), 0).is_none(), "{bad:?}");
    }
    assert!(try_format_time("%H", &t(), 0).is_some());
}

#[test]
fn output_longer_than_4095_is_empty() {
    let long = "x".repeat(5000);
    assert_eq!(f(&long), "");
    let ok = "x".repeat(4095);
    assert_eq!(f(&ok).len(), 4095);
}

#[test]
fn non_ascii_literals_pass_through() {
    assert_eq!(f("é%Hü"), "é07ü");
}
