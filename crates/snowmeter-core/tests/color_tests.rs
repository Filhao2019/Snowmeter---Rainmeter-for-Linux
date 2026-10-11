use snowmeter_core::color::{parse_color, Color};

fn c(r: u8, g: u8, b: u8, a: u8) -> Option<Color> {
    Some(Color { r, g, b, a })
}

#[test]
fn hex_forms() {
    assert_eq!(parse_color("FF0000"), c(255, 0, 0, 255));
    assert_eq!(parse_color("FF000080"), c(255, 0, 0, 128));
    assert_eq!(parse_color("0xFF0000"), c(255, 0, 0, 255));
    assert_eq!(parse_color("11223344"), c(17, 34, 51, 68));
    assert_eq!(parse_color("AABBCCDD"), c(170, 187, 204, 221));
    assert_eq!(parse_color("aabbcc"), c(170, 187, 204, 255));
}

#[test]
fn hex_quirks() {
    assert_eq!(parse_color("0XFF0000"), c(0, 255, 255, 255));
    assert_eq!(parse_color("FF0000 80"), c(255, 0, 0, 255));
    assert_eq!(parse_color("FF00"), c(255, 255, 255, 255));
    assert_eq!(parse_color("FF00ZZ"), c(255, 0, 255, 255));
    assert_eq!(parse_color("ZZ0000"), c(255, 255, 255, 255));
}

#[test]
fn component_lists() {
    assert_eq!(parse_color("10,20,30"), c(10, 20, 30, 255));
    assert_eq!(parse_color("10,20,30,40"), c(10, 20, 30, 40));
    assert_eq!(parse_color(" 10 , 20 , 30 "), c(10, 20, 30, 255));
    assert_eq!(parse_color("(1 + min(2, 3)),20,30"), c(3, 20, 30, 255));
    assert_eq!(parse_color("10,,30"), c(10, 0, 30, 255));
    assert_eq!(parse_color("10,20, "), c(10, 20, 255, 255));
    assert_eq!(parse_color("10,20,\r\n"), c(10, 20, 255, 255));
    assert_eq!(parse_color("10,invalid,30"), c(10, 0, 30, 255));
    assert_eq!(parse_color("10,20,30,40,50"), c(10, 20, 30, 40));
    assert_eq!(parse_color("10.7,20,30"), c(10, 20, 30, 255));
}

#[test]
fn out_of_range_is_clamped() {
    assert_eq!(parse_color("300,-5,30"), c(255, 0, 30, 255));
}

#[test]
fn empty_is_none() {
    assert_eq!(parse_color(""), None);
}
