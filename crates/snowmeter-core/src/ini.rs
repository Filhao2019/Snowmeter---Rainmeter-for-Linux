//! Rainmeter-style INI reading and writing (`Common/IniFile.cpp` semantics).
//!
//! * Encoding detection: UTF-16LE (BOM or heuristic), UTF-8 (BOM or strict),
//!   Windows-1252 fallback.
//! * `;` comments only at line start, `[name]` up to the first `]`, split at
//!   the first `=`, one pair of outer quotes stripped from values.
//! * First duplicate section wins entirely, first duplicate key wins.

use std::fs;
use std::io;
use std::path::Path;

/// Names longer than this many UTF-16 code units are ignored.
const MAX_NAME_UNITS: usize = 255;

const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D,
    0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
    0x0153, 0x009D, 0x017E, 0x0178,
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    Utf16Le { bom: bool },
    Utf16Be,
    Utf8Bom,
    Utf8,
    Ansi,
}

fn detect(bytes: &[u8]) -> Encoding {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Encoding::Utf16Be;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Encoding::Utf16Le { bom: true };
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Encoding::Utf8Bom;
    }
    if looks_like_utf16le(bytes) {
        return Encoding::Utf16Le { bom: false };
    }
    if std::str::from_utf8(bytes).is_ok() {
        Encoding::Utf8
    } else {
        Encoding::Ansi
    }
}

/// Stand-in for `IsTextUnicode(ASCII16 | CONTROLS)`.
fn looks_like_utf16le(bytes: &[u8]) -> bool {
    if bytes.len() < 2 || bytes.len() % 2 != 0 {
        return false;
    }
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let all_ascii = units.iter().all(|&u| u <= 0x7F);
    let has_whitespace_unit = units.iter().any(|&u| matches!(u, 0x0D | 0x0A | 0x09 | 0x20));
    all_ascii || has_whitespace_unit
}

fn decode_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => char::from_u32(CP1252_HIGH[(b - 0x80) as usize] as u32).unwrap_or('\u{FFFD}'),
            _ => b as char,
        })
        .collect()
}

fn encode_cp1252(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            let u = c as u32;
            if u < 0x80 || (0xA0..=0xFF).contains(&u) {
                u as u8
            } else if let Some(i) = CP1252_HIGH.iter().position(|&h| h as u32 == u) {
                0x80 + i as u8
            } else {
                b'?'
            }
        })
        .collect()
}

fn decode_utf16(bytes: &[u8], little: bool) -> String {
    let units = bytes.chunks_exact(2).map(|c| if little { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) });
    char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}

/// Decode raw file bytes to text, never failing. UTF-16 BOMs (either
/// endianness), UTF-8 (BOM or not) and Windows-1252 as the last resort.
pub fn decode_text(bytes: &[u8]) -> String {
    match detect(bytes) {
        Encoding::Utf16Le { bom } => decode_utf16(&bytes[if bom { 2 } else { 0 }..], true),
        Encoding::Utf16Be => decode_utf16(&bytes[2..], false),
        Encoding::Utf8Bom => String::from_utf8_lossy(&bytes[3..]).into_owned(),
        Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Encoding::Ansi => decode_cp1252(bytes),
    }
}

/// Strict variant used when loading files: stock Rainmeter rejects UTF-16BE,
/// odd-length UTF-16 and invalid UTF-8 after a BOM.
fn decode_checked(bytes: &[u8]) -> Result<String, String> {
    match detect(bytes) {
        Encoding::Utf16Be => Err("UTF-16BE files are not supported".into()),
        Encoding::Utf16Le { .. } if bytes.len() % 2 != 0 => Err("odd byte count in UTF-16 file".into()),
        Encoding::Utf8Bom => String::from_utf8(bytes[3..].to_vec()).map_err(|_| "invalid UTF-8 after BOM".to_string()),
        _ => Ok(decode_text(bytes)),
    }
}

fn encode_text(text: &str, enc: Encoding) -> Vec<u8> {
    match enc {
        Encoding::Utf16Le { bom } => {
            let mut v = Vec::new();
            if bom {
                v.extend_from_slice(&[0xFF, 0xFE]);
            }
            for u in text.encode_utf16() {
                v.extend_from_slice(&u.to_le_bytes());
            }
            v
        }
        Encoding::Utf16Be => {
            let mut v = vec![0xFE, 0xFF];
            for u in text.encode_utf16() {
                v.extend_from_slice(&u.to_be_bytes());
            }
            v
        }
        Encoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(text.as_bytes());
            v
        }
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Ansi => encode_cp1252(text),
    }
}

/// Trim every leading and trailing char <= U+0020.
fn trim_ini(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

fn strip_quotes(v: &str) -> &str {
    let b = v.as_bytes();
    if b.len() > 1 && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\'')) {
        &v[1..v.len() - 1]
    } else {
        v
    }
}

fn name_ok(s: &str) -> bool {
    s.encode_utf16().count() <= MAX_NAME_UNITS
}

/// Split into `(content, terminator)` pairs; CR, LF and CRLF all terminate.
fn split_lines(text: &str) -> Vec<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                out.push((&text[start..i], &text[i..i + 1]));
                i += 1;
                start = i;
            }
            b'\r' => {
                let len = if bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                out.push((&text[start..i], &text[i..i + len]));
                i += len;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        out.push((&text[start..], ""));
    }
    out
}

/// `[name]` header name of an already trimmed line.
fn header_name(line: &str) -> &str {
    let rest = &line[1..];
    trim_ini(rest.find(']').map_or(rest, |p| &rest[..p]))
}

#[derive(Clone, Debug, Default)]
pub struct IniFile {
    sections: Vec<IniSection>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IniSection {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl IniSection {
    fn find(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|(k, _)| k.eq_ignore_ascii_case(key))
    }
}

impl IniFile {
    pub fn parse(text: &str) -> IniFile {
        let mut sections: Vec<IniSection> = Vec::new();
        // None = before first header or inside an ignored (duplicate / bad) section.
        let mut current: Option<usize> = None;
        for (raw, _) in split_lines(text) {
            let line = trim_ini(raw);
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') {
                let name = header_name(line);
                current = if !name_ok(name) || sections.iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
                    None
                } else {
                    sections.push(IniSection { name: name.to_string(), entries: Vec::new() });
                    Some(sections.len() - 1)
                };
            } else if let (Some(idx), Some(eq)) = (current, line.find('=')) {
                let key = trim_ini(&line[..eq]);
                if key.is_empty() || !name_ok(key) || sections[idx].find(key).is_some() {
                    continue;
                }
                let value = strip_quotes(trim_ini(&line[eq + 1..]));
                sections[idx].entries.push((key.to_string(), value.to_string()));
            }
        }
        IniFile { sections }
    }

    pub fn load(path: &Path) -> io::Result<IniFile> {
        let bytes = fs::read(path)?;
        let text = decode_checked(&bytes).map_err(|m| io::Error::new(io::ErrorKind::InvalidData, m))?;
        Ok(IniFile::parse(&text))
    }

    pub fn sections(&self) -> &[IniSection] {
        &self.sections
    }

    pub fn section(&self, name: &str) -> Option<&IniSection> {
        self.sections.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let s = self.section(section)?;
        s.find(key).map(|i| s.entries[i].1.as_str())
    }

    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let si = match self.sections.iter().position(|s| s.name.eq_ignore_ascii_case(section)) {
            Some(i) => i,
            None => {
                self.sections.push(IniSection { name: section.to_string(), entries: Vec::new() });
                self.sections.len() - 1
            }
        };
        let sec = &mut self.sections[si];
        match sec.find(key) {
            Some(i) => sec.entries[i].1 = value.to_string(),
            None => sec.entries.push((key.to_string(), value.to_string())),
        }
    }

    /// Remove every entry of `[name]` but keep the header.
    pub fn clear_section(&mut self, name: &str) {
        for s in self.sections.iter_mut().filter(|s| s.name.eq_ignore_ascii_case(name)) {
            s.entries.clear();
        }
    }

    pub fn remove_section(&mut self, name: &str) {
        self.sections.retain(|s| !s.name.eq_ignore_ascii_case(name));
    }

    pub fn to_ini_string(&self) -> String {
        let mut out = String::new();
        for s in &self.sections {
            out.push_str(&format!("[{}]\n", s.name));
            for (k, v) in &s.entries {
                out.push_str(&format!("{k}={v}\n"));
            }
            out.push('\n');
        }
        out
    }
}

/// Set `key` in `[section]` of the file at `path`, leaving every other line
/// (comments, spacing, line endings, encoding) untouched. Creates the file,
/// section or key when missing.
pub fn write_key_value(path: &Path, section: &str, key: &str, value: &str) -> io::Result<()> {
    let (enc, text) = match fs::read(path) {
        Ok(bytes) => (detect(&bytes), decode_text(&bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (Encoding::Utf8, String::new()),
        Err(e) => return Err(e),
    };
    let default_nl = if text.contains("\r\n") {
        "\r\n"
    } else if text.contains('\n') || text.is_empty() {
        "\n"
    } else if text.contains('\r') {
        "\r"
    } else {
        "\n"
    };
    let mut lines: Vec<(String, String)> = split_lines(&text).into_iter().map(|(c, t)| (c.to_string(), t.to_string())).collect();

    let mut in_target = false;
    let mut target_seen = false;
    let mut header_idx: Option<usize> = None;
    let mut last_key_idx: Option<usize> = None;
    let mut replaced = false;
    for i in 0..lines.len() {
        let line = trim_ini(&lines[i].0).to_string();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            let matches = header_name(&line).eq_ignore_ascii_case(section);
            in_target = matches && !target_seen;
            if in_target {
                target_seen = true;
                header_idx = Some(i);
            }
        } else if in_target {
            if let Some(eq) = line.find('=') {
                last_key_idx = Some(i);
                let k = trim_ini(&line[..eq]);
                if !replaced && k.eq_ignore_ascii_case(key) {
                    lines[i].0 = format!("{k}={value}");
                    replaced = true;
                }
            }
        }
    }
    if !replaced {
        let new_line = (format!("{key}={value}"), default_nl.to_string());
        match (header_idx, last_key_idx) {
            (Some(h), k) => {
                let at = k.unwrap_or(h);
                if lines[at].1.is_empty() {
                    lines[at].1 = default_nl.to_string();
                }
                lines.insert(at + 1, new_line);
            }
            (None, _) => {
                if let Some(last) = lines.last_mut() {
                    if last.1.is_empty() {
                        last.1 = default_nl.to_string();
                    }
                    lines.push((String::new(), default_nl.to_string()));
                }
                lines.push((format!("[{section}]"), default_nl.to_string()));
                lines.push(new_line);
            }
        }
    }
    let mut out = String::with_capacity(text.len() + 32);
    for (c, t) in &lines {
        out.push_str(c);
        out.push_str(t);
    }
    fs::write(path, encode_text(&out, enc))
}
