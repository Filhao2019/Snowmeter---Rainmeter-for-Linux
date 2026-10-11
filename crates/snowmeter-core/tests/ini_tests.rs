use snowmeter_core::ini::{decode_text, write_key_value, IniFile};
use std::fs;

fn utf16le(s: &str, bom: bool) -> Vec<u8> {
    let mut v = Vec::new();
    if bom {
        v.extend_from_slice(&[0xFF, 0xFE]);
    }
    for u in s.encode_utf16() {
        v.extend_from_slice(&u.to_le_bytes());
    }
    v
}

#[test]
fn decode_utf16le_bom() {
    assert_eq!(decode_text(&utf16le("[A]\r\nK=é", true)), "[A]\r\nK=é");
}

#[test]
fn decode_utf16le_without_bom_ascii() {
    assert_eq!(decode_text(&utf16le("[A]\nK=1\n", false)), "[A]\nK=1\n");
}

#[test]
fn decode_utf16be_bom_is_decoded_leniently() {
    let mut v = vec![0xFE, 0xFF];
    for u in "ab".encode_utf16() {
        v.extend_from_slice(&u.to_be_bytes());
    }
    assert_eq!(decode_text(&v), "ab");
}

#[test]
fn decode_utf8_bom_stripped() {
    assert_eq!(decode_text(b"\xEF\xBB\xBF[A]"), "[A]");
}

#[test]
fn decode_plain_utf8() {
    assert_eq!(decode_text("caf\u{e9}".as_bytes()), "caf\u{e9}");
}

#[test]
fn decode_cp1252_fallback() {
    // C3 A9 FF is not valid UTF-8 -> Windows-1252: Ã © ÿ
    assert_eq!(decode_text(&[0xC3, 0xA9, 0xFF]), "\u{c3}\u{a9}\u{ff}");
    // 0x80 is the euro sign in cp1252, 0x93/0x94 curly quotes
    assert_eq!(decode_text(&[0x80, 0x93, 0x94]), "\u{20ac}\u{201c}\u{201d}");
}

#[test]
fn decode_bom_only_is_empty() {
    assert_eq!(decode_text(&[0xFF, 0xFE]), "");
    assert_eq!(decode_text(&[0xEF, 0xBB, 0xBF]), "");
    assert_eq!(decode_text(&[]), "");
}

#[test]
fn comments_blank_lines_and_trimming() {
    let ini = IniFile::parse("; top comment\r\n\r\n  [ Sec ]  \r\n; c\r\n  Key  =   value with spaces   \r\nOne = 1 ; not a comment\r\n");
    assert_eq!(ini.get("Sec", "Key"), Some("value with spaces"));
    assert_eq!(ini.get("Sec", "One"), Some("1 ; not a comment"));
    assert_eq!(ini.sections().len(), 1);
    assert_eq!(ini.sections()[0].name, "Sec");
}

#[test]
fn hash_is_not_a_comment_and_lines_without_equals_are_ignored() {
    let ini = IniFile::parse("[A]\n#x=1\njunk line\nK=2\n");
    assert_eq!(ini.get("A", "#x"), Some("1"));
    assert_eq!(ini.sections()[0].entries.len(), 2);
}

#[test]
fn mixed_line_endings() {
    let ini = IniFile::parse("[A]\rA=1\nB=2\r\nC=3");
    assert_eq!(ini.get("A", "A"), Some("1"));
    assert_eq!(ini.get("A", "B"), Some("2"));
    assert_eq!(ini.get("A", "C"), Some("3"));
}

#[test]
fn case_insensitive_lookup_keeps_original_case() {
    let ini = IniFile::parse("[MeterClock]\nFontColor=1,2,3\n");
    assert_eq!(ini.get("meterclock", "FONTCOLOR"), Some("1,2,3"));
    assert_eq!(ini.section("METERCLOCK").unwrap().name, "MeterClock");
    assert_eq!(ini.section("METERCLOCK").unwrap().entries[0].0, "FontColor");
}

#[test]
fn only_ascii_case_folding() {
    let ini = IniFile::parse("[ä]\nK=1\n");
    assert!(ini.get("Ä", "K").is_none());
    assert_eq!(ini.get("ä", "K"), Some("1"));
}

#[test]
fn quote_stripping() {
    let ini = IniFile::parse("[A]\nA=\"  padded  \"\nB=''\nC=\"\"\nD=\"a\" \"b\"\nE='x'\nF=\"\nG=\"half\n");
    assert_eq!(ini.get("A", "A"), Some("  padded  "));
    assert_eq!(ini.get("A", "B"), Some(""));
    assert_eq!(ini.get("A", "C"), Some(""));
    assert_eq!(ini.get("A", "D"), Some("a\" \"b"));
    assert_eq!(ini.get("A", "E"), Some("x"));
    assert_eq!(ini.get("A", "F"), Some("\""));
    assert_eq!(ini.get("A", "G"), Some("\"half"));
}

#[test]
fn split_at_first_equals() {
    let ini = IniFile::parse("[A]\nURL=http://x/?a=b=c\n");
    assert_eq!(ini.get("A", "URL"), Some("http://x/?a=b=c"));
}

#[test]
fn empty_value_is_defined_empty_empty_key_ignored() {
    let ini = IniFile::parse("[A]\nK=\n=orphan\n");
    assert_eq!(ini.get("A", "K"), Some(""));
    assert_eq!(ini.sections()[0].entries.len(), 1);
}

#[test]
fn keys_before_first_header_ignored_and_unterminated_header() {
    let ini = IniFile::parse("Stray=1\n[NoClose\nK=2\n[X] trailing\nL=3\n[]\nM=4\n");
    assert!(ini.get("", "Stray").is_none() || ini.sections()[0].name != "");
    assert_eq!(ini.get("NoClose", "K"), Some("2"));
    assert_eq!(ini.get("X", "L"), Some("3"));
    assert_eq!(ini.get("", "M"), Some("4"));
}

#[test]
fn duplicate_key_first_wins() {
    let ini = IniFile::parse("[A]\nK=1\nk=2\n");
    assert_eq!(ini.get("A", "K"), Some("1"));
    assert_eq!(ini.sections()[0].entries.len(), 1);
}

#[test]
fn duplicate_section_first_block_wins_entirely() {
    let ini = IniFile::parse("[A]\nX=1\n[B]\nZ=9\n[a]\nY=2\nX=3\n");
    assert_eq!(ini.get("A", "X"), Some("1"));
    assert_eq!(ini.get("A", "Y"), None);
    assert_eq!(ini.sections().len(), 2);
}

#[test]
fn overlong_names_ignored() {
    let long = "k".repeat(256);
    let ini = IniFile::parse(&format!("[A]\n{}=1\nok=2\n[{}]\nz=1\n", long, "s".repeat(256)));
    assert_eq!(ini.sections().len(), 1);
    assert_eq!(ini.sections()[0].entries.len(), 1);
}

#[test]
fn set_and_remove_and_serialize_roundtrip() {
    let mut ini = IniFile::parse("[A]\nK=1\n");
    ini.set("a", "k", "2");
    ini.set("A", "New", "x");
    ini.set("B", "Y", "z");
    assert_eq!(ini.get("A", "K"), Some("2"));
    assert_eq!(ini.get("A", "New"), Some("x"));
    let again = IniFile::parse(&ini.to_ini_string());
    assert_eq!(again.get("B", "Y"), Some("z"));
    assert_eq!(again.get("A", "K"), Some("2"));
    ini.remove_section("b");
    assert!(ini.section("B").is_none());
}

#[test]
fn load_utf16_file_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("skin.ini");
    fs::write(&p, utf16le("[Rainmeter]\r\nUpdate=500\r\n", true)).unwrap();
    let ini = IniFile::load(&p).unwrap();
    assert_eq!(ini.get("Rainmeter", "Update"), Some("500"));
}

#[test]
fn load_rejects_utf16be_and_missing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("be.ini");
    fs::write(&p, [0xFE, 0xFF, 0x00, 0x41]).unwrap();
    assert!(IniFile::load(&p).is_err());
    assert!(IniFile::load(&dir.path().join("nope.ini")).is_err());
}

#[test]
fn write_key_value_replaces_in_place_preserving_other_lines() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.ini");
    fs::write(&p, "; hi\r\n[A]\r\nK=1\r\nL=2\r\n\r\n[B]\r\nK=9\r\n").unwrap();
    write_key_value(&p, "a", "k", "new").unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), "; hi\r\n[A]\r\nk=new\r\nL=2\r\n\r\n[B]\r\nK=9\r\n".replace("k=new", "K=new"));
}

#[test]
fn write_key_value_adds_key_and_section() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.ini");
    fs::write(&p, "[A]\nK=1\n\n[B]\nZ=1\n").unwrap();
    write_key_value(&p, "A", "New", "2").unwrap();
    write_key_value(&p, "C", "X", "3").unwrap();
    let ini = IniFile::load(&p).unwrap();
    assert_eq!(ini.get("A", "K"), Some("1"));
    assert_eq!(ini.get("A", "New"), Some("2"));
    assert_eq!(ini.get("B", "Z"), Some("1"));
    assert_eq!(ini.get("C", "X"), Some("3"));
    // New key must land inside section A, before [B]
    let text = fs::read_to_string(&p).unwrap();
    assert!(text.find("New=2").unwrap() < text.find("[B]").unwrap());
}

#[test]
fn write_key_value_keeps_utf16_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.ini");
    fs::write(&p, utf16le("[A]\r\nK=1\r\n", true)).unwrap();
    write_key_value(&p, "A", "K", "2").unwrap();
    let bytes = fs::read(&p).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
    assert_eq!(decode_text(&bytes), "[A]\r\nK=2\r\n");
}

#[test]
fn write_key_value_creates_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("new.ini");
    write_key_value(&p, "S", "K", "v").unwrap();
    assert_eq!(IniFile::load(&p).unwrap().get("S", "K"), Some("v"));
}
