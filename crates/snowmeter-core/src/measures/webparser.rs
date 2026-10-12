//! The WebParser measure (measures.md section 3.11): worker-thread fetch,
//! PCRE2 parsing, `StringIndex`, child measures (`Url=[Parent]`) and actions.

use super::{MeasureCtx, MeasureImpl};
use crate::numparse::parse_f64_prefix;
use crate::parser::OptionReader;
use pcre2::bytes::RegexBuilder;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

enum Fetched {
    Bytes(Vec<u8>),
    Downloaded(PathBuf),
}

type Slot = Arc<Mutex<Option<Result<Fetched, String>>>>;

/// What a parent published for its children.
#[derive(Clone)]
struct Published {
    generation: u64,
    /// Capture groups of the match; `None` after a regex failure.
    groups: Option<Vec<String>>,
}

thread_local! {
    static PUBLISHED: RefCell<HashMap<String, Published>> = RefCell::new(HashMap::new());
    static GENERATION: RefCell<u64> = const { RefCell::new(0) };
}

fn next_generation() -> u64 {
    GENERATION.with(|g| {
        *g.borrow_mut() += 1;
        *g.borrow()
    })
}

fn key(skin_dir: &std::path::Path, name: &str) -> String {
    format!("{}|{}", skin_dir.display(), name.to_ascii_lowercase())
}

#[derive(Default)]
struct Options {
    url: String,
    regexp: String,
    string_index: usize,
    string_index2: usize,
    update_rate: i64,
    finish: String,
    regexp_error: String,
    connect_error: String,
    download_error: String,
    error_string: String,
    log_substring_errors: bool,
    code_page: i64,
    decode_ref: i64,
    decode_code_points: bool,
    headers: Vec<String>,
    user_agent: String,
    download: bool,
    download_file: String,
}

#[derive(Default)]
pub struct WebParser {
    o: Options,
    section: String,
    /// Name of the parent when `Url` is `[Parent]`.
    parent: Option<String>,
    last_parent_generation: u64,
    slot: Option<Slot>,
    running: bool,
    counter: i64,
    result: String,
    value: f64,
    /// Whether the measure has a string value (set after the first result).
    has_string: bool,
    actions: Vec<String>,
    log: Vec<String>,
    skin_dir: PathBuf,
}

impl WebParser {
    pub fn new() -> WebParser {
        WebParser::default()
    }

    pub fn result(&self) -> &str {
        &self.result
    }

    fn fetchable(&self) -> bool {
        self.parent.is_none() && !self.o.url.is_empty() && !self.o.url.contains('[')
    }

    fn start_fetch(&mut self) {
        let url = self.o.url.clone();
        self.start_fetch_url(url);
    }

    fn start_fetch_url(&mut self, url: String) {
        let slot: Slot = Arc::new(Mutex::new(None));
        self.slot = Some(slot.clone());
        self.running = true;
        let headers = self.o.headers.clone();
        let ua = self.o.user_agent.clone();
        let download = self.o.download.then(|| self.download_path());
        std::thread::spawn(move || {
            let r = fetch(&url, &headers, &ua).and_then(|bytes| match download {
                Some(path) => {
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                    Ok(Fetched::Downloaded(path))
                }
                None => Ok(Fetched::Bytes(bytes)),
            });
            *slot.lock().unwrap() = Some(r);
        });
    }

    fn download_path(&self) -> PathBuf {
        if self.o.download_file.is_empty() {
            let name = format!("snowmeter-webparser-{}-{}", std::process::id(), self.section.to_ascii_lowercase());
            std::env::temp_dir().join(name)
        } else {
            confine(&self.skin_dir.join("DownloadFile"), &self.o.download_file)
        }
    }

    fn poll(&mut self, ctx_dir: &std::path::Path) {
        let Some(slot) = self.slot.clone() else { return };
        let Some(r) = slot.lock().unwrap().take() else { return };
        self.slot = None;
        self.running = false;
        match r {
            Err(e) => {
                self.log.push(format!("WebParser: fetching failed: {e}"));
                let a = if self.o.download { &self.o.download_error } else { &self.o.connect_error };
                if !a.is_empty() {
                    self.actions.push(a.clone());
                }
            }
            Ok(Fetched::Downloaded(p)) => {
                self.result = p.to_string_lossy().into_owned();
                self.has_string = true;
                self.finish();
            }
            Ok(Fetched::Bytes(b)) => {
                let text = decode(&b, self.o.code_page);
                self.parse_text(&text, ctx_dir, true);
            }
        }
    }

    fn finish(&mut self) {
        if !self.o.finish.is_empty() {
            self.actions.push(self.o.finish.clone());
        }
    }

    /// Run `RegExp` on `text`, set the result and publish the groups.
    fn parse_text(&mut self, text: &str, dir: &std::path::Path, own_index: bool) {
        let index = if own_index { self.o.string_index } else { self.o.string_index2 };
        let groups = match run_regex(&self.o.regexp, text) {
            Ok(Some(g)) => g,
            Ok(None) => {
                self.log.push("WebParser: RegExp matching error (-1)".into());
                self.fail_regex(dir);
                return;
            }
            Err(e) => {
                self.log.push(format!("WebParser: {e}"));
                self.fail_regex(dir);
                return;
            }
        };
        let picked = match groups.get(index) {
            Some(g) => g.clone(),
            None => {
                if self.o.log_substring_errors {
                    self.log.push(format!("WebParser: Not enough substrings (StringIndex={index})"));
                }
                String::new()
            }
        };
        self.result = self.decode_entities(&picked);
        self.has_string = true;
        self.publish(dir, Some(groups));
        self.finish();
    }

    fn fail_regex(&mut self, dir: &std::path::Path) {
        self.result = self.o.error_string.clone();
        self.has_string = true;
        self.publish(dir, None);
        if !self.o.regexp_error.is_empty() {
            self.actions.push(self.o.regexp_error.clone());
        }
    }

    fn publish(&self, dir: &std::path::Path, groups: Option<Vec<String>>) {
        let k = key(dir, &self.section);
        let generation = next_generation();
        PUBLISHED.with(|p| p.borrow_mut().insert(k, Published { generation, groups }));
    }

    fn decode_entities(&self, s: &str) -> String {
        let mut out = s.to_string();
        if self.o.decode_ref != 0 {
            out = decode_references(&out, self.o.decode_ref);
        }
        if self.o.decode_code_points {
            out = decode_code_points(&out);
        }
        out
    }

    /// Child measures: derive the result from the parent's published groups.
    fn update_child(&mut self, parent: &str, dir: &std::path::Path) {
        let Some(p) = PUBLISHED.with(|m| m.borrow().get(&key(dir, parent)).cloned()) else { return };
        if p.generation == self.last_parent_generation {
            return;
        }
        self.last_parent_generation = p.generation;
        let Some(groups) = p.groups else {
            self.result = self.o.error_string.clone();
            self.has_string = true;
            self.publish(dir, None);
            if !self.o.regexp_error.is_empty() {
                self.actions.push(self.o.regexp_error.clone());
            }
            return;
        };
        let Some(source) = groups.get(self.o.string_index) else {
            if self.o.log_substring_errors {
                self.log.push(format!("WebParser: Not enough substrings (StringIndex={})", self.o.string_index));
            }
            self.result = String::new();
            self.has_string = true;
            return;
        };
        if !self.o.regexp.is_empty() {
            let source = source.clone();
            self.parse_text(&source, dir, false);
            return;
        }
        let url = replace_ci(&self.o.url, &format!("[{parent}]"), source);
        let result = self.decode_entities(&url);
        if self.o.download {
            self.start_fetch_url(result);
            return;
        }
        self.result = result;
        self.has_string = true;
        self.publish(dir, Some(groups.clone()));
        self.finish();
    }
}

fn replace_ci(s: &str, pat: &str, rep: &str) -> String {
    let (ls, lp) = (s.to_ascii_lowercase(), pat.to_ascii_lowercase());
    match ls.find(&lp) {
        Some(i) => format!("{}{}{}", &s[..i], rep, &s[i + pat.len()..]),
        None => s.to_string(),
    }
}

/// Captures of the first match as strings; `Ok(None)` for no match.
fn run_regex(pattern: &str, text: &str) -> Result<Option<Vec<String>>, String> {
    let re = RegexBuilder::new().utf(true).build(pattern).map_err(|e| format!("RegExp error: {e}"))?;
    let caps = re.captures(text.as_bytes()).map_err(|e| format!("RegExp error: {e}"))?;
    Ok(caps.map(|c| {
        (0..c.len()).map(|i| c.get(i).map(|m| String::from_utf8_lossy(m.as_bytes()).into_owned()).unwrap_or_default()).collect()
    }))
}

fn decode(bytes: &[u8], code_page: i64) -> String {
    match code_page {
        1200 => {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        }
        1201 => {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        }
        1250..=1258 | 28591 => bytes.iter().map(|&b| b as char).collect(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

const NAMED: &[(&str, char)] = &[
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
    ("copy", '\u{a9}'),
    ("reg", '\u{ae}'),
    ("deg", '\u{b0}'),
    ("plusmn", '\u{b1}'),
    ("laquo", '\u{ab}'),
    ("raquo", '\u{bb}'),
    ("ndash", '\u{2013}'),
    ("mdash", '\u{2014}'),
    ("lsquo", '\u{2018}'),
    ("rsquo", '\u{2019}'),
    ("ldquo", '\u{201c}'),
    ("rdquo", '\u{201d}'),
    ("hellip", '\u{2026}'),
    ("euro", '\u{20ac}'),
    ("pound", '\u{a3}'),
    ("yen", '\u{a5}'),
    ("cent", '\u{a2}'),
    ("eacute", '\u{e9}'),
    ("egrave", '\u{e8}'),
    ("aacute", '\u{e1}'),
    ("agrave", '\u{e0}'),
    ("uuml", '\u{fc}'),
    ("ouml", '\u{f6}'),
    ("auml", '\u{e4}'),
    ("szlig", '\u{df}'),
];

/// `mode`: 1 numeric + named, 2 numeric only, 3 named only.
pub fn decode_references(s: &str, mode: i64) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let decoded = tail.find(';').filter(|&e| e <= 12).and_then(|e| {
            let body = &tail[1..e];
            let ch = if let Some(num) = body.strip_prefix('#') {
                if mode == 3 {
                    return None;
                }
                let v = match num.strip_prefix(['x', 'X']) {
                    Some(h) => u32::from_str_radix(h, 16).ok()?,
                    None => num.parse::<u32>().ok()?,
                };
                if v >= 0xFFFE {
                    return None;
                }
                char::from_u32(v)?
            } else {
                if mode == 2 {
                    return None;
                }
                NAMED.iter().find(|(n, _)| *n == body).map(|(_, c)| *c)?
            };
            Some((ch, e + 1))
        });
        match decoded {
            Some((ch, len)) => {
                out.push(ch);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `\uXXXX` (BMP below U+FFFE) and `\UXXXXXXXX`.
pub fn decode_code_points(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\\' && i + 1 < b.len() && (b[i + 1] == 'u' || b[i + 1] == 'U') {
            let n = if b[i + 1] == 'u' { 4 } else { 8 };
            if i + 2 + n <= b.len() {
                let hex: String = b[i + 2..i + 2 + n].iter().collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().filter(|v| *v < 0xFFFE).and_then(char::from_u32) {
                    out.push(c);
                    i += 2 + n;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Largest response (HTTP body or local file) a WebParser accepts.
pub const MAX_RESPONSE: u64 = 16 << 20;

/// Read a regular file of at most `MAX_RESPONSE` bytes (never blocks on FIFOs
/// or devices such as `/dev/zero`).
fn read_local_file(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let md = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !md.is_file() {
        return Err("not a regular file".into());
    }
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_RESPONSE + 1).read_to_end(&mut buf))
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_RESPONSE {
        return Err(format!("file is larger than {} MiB", MAX_RESPONSE >> 20));
    }
    Ok(buf)
}

/// Join a skin-supplied relative file name under `base`, dropping any root,
/// drive and `..` components so the result cannot leave `base`.
fn confine(base: &std::path::Path, name: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for c in std::path::Path::new(&name.replace('\\', "/")).components() {
        if let std::path::Component::Normal(p) = c {
            out.push(p);
        }
    }
    out
}

/// Percent-encode the bytes of `url` that are not valid in a URI (spaces,
/// quotes, `<>\\^`{|}`, control and non-ASCII bytes), as WinInet does, and leave
/// existing `%XX` escapes and reserved delimiters alone.
pub fn escape_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for b in url.bytes() {
        let bad = b <= 0x20 || b >= 0x7F || matches!(b, b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}');
        if bad {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

fn fetch(url: &str, headers: &[String], ua: &str) -> Result<Vec<u8>, String> {
    if let Some(p) = url.strip_prefix("file://") {
        let path = p.strip_prefix('/').map(|r| format!("/{r}")).unwrap_or_else(|| p.to_string());
        return read_local_file(std::path::Path::new(&path)).map_err(|e| format!("{path}: {e}"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .user_agent(ua)
        .http_status_as_error(false)
        .build()
        .into();
    let url = escape_url(url);
    let mut req = agent.get(&url);
    for h in headers {
        if let Some((k, v)) = h.split_once(':') {
            req = req.header(k.trim(), v.trim());
        }
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status >= 400 {
        return Err(format!("HTTP status {status}"));
    }
    resp.body_mut().with_config().limit(MAX_RESPONSE).read_to_vec().map_err(|e| e.to_string())
}

impl MeasureImpl for WebParser {
    fn read_options(&mut self, opt: &OptionReader) {
        self.section = opt.section.to_string();
        let raw = opt.without_sections();
        let o = &mut self.o;
        o.url = raw.string("Url", "").trim().to_string();
        o.regexp = opt.string("RegExp", "");
        o.string_index = opt.int("StringIndex", 0).max(0) as usize;
        o.string_index2 = opt.int("StringIndex2", 0).max(0) as usize;
        o.update_rate = opt.int("UpdateRate", 600).max(1);
        o.finish = opt.action("FinishAction");
        o.regexp_error = opt.action("OnRegExpErrorAction");
        o.connect_error = opt.action("OnConnectErrorAction");
        o.download_error = opt.action("OnDownloadErrorAction");
        o.error_string = opt.string("ErrorString", "");
        o.log_substring_errors = opt.bool("LogSubstringErrors", true);
        o.code_page = opt.int("CodePage", 0);
        o.decode_ref = opt.int("DecodeCharacterReference", 0);
        o.decode_code_points = opt.bool("DecodeCodePoints", false);
        o.user_agent = opt.string("UserAgent", "Rainmeter WebParser plugin");
        o.download = opt.bool("Download", false);
        o.download_file = opt.string("DownloadFile", "");
        o.headers.clear();
        for n in 1.. {
            let key = if n == 1 { "Header".to_string() } else { format!("Header{n}") };
            let h = opt.string(&key, "");
            if h.is_empty() {
                break;
            }
            o.headers.push(h);
        }
        // `Url` containing `[Parent]` makes this a child of that measure.
        self.parent = self
            .o
            .url
            .split_once('[')
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(name, _)| name.to_string())
            .filter(|n| !n.is_empty());
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        self.skin_dir = ctx.skin_dir.to_path_buf();
        let dir = ctx.skin_dir.to_path_buf();
        self.poll(&dir);
        if let Some(parent) = self.parent.clone() {
            self.update_child(&parent, &dir);
        } else if self.fetchable() {
            if self.counter == 0 && !self.running {
                self.start_fetch();
            }
            self.counter += 1;
            if self.counter >= self.o.update_rate {
                self.counter = 0;
            }
        }
        self.value = parse_f64_prefix(&self.result);
        ctx.log.append(&mut self.log);
        self.value
    }

    fn string_value(&self) -> Option<String> {
        // WebParser always has a string value (empty until the first result), so
        // `Substitute="":"N/A"` applies instead of the numeric fallback "0".
        Some(self.result.clone())
    }

    fn command(&mut self, args: &str) {
        match args.trim().to_ascii_lowercase().as_str() {
            "update" => {
                self.counter = 0;
                self.slot = None;
                self.running = false;
            }
            "reset" => {
                self.result.clear();
                self.has_string = false;
                self.value = 0.0;
                let k = key(&self.skin_dir, &self.section);
                PUBLISHED.with(|p| p.borrow_mut().remove(&k));
            }
            other => self.log.push(format!("WebParser: unknown command '{other}'")),
        }
    }

    fn take_actions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.actions)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn urls_with_unsafe_characters_are_percent_encoded() {
        assert_eq!(super::escape_url("http://h/a b?q=\"x y\"&r=1%202#f"), "http://h/a%20b?q=%22x%20y%22&r=1%202#f");
        assert_eq!(super::escape_url("http://h/\u{e9}|{}"), "http://h/%C3%A9%7C%7B%7D");
        assert_eq!(super::escape_url("http://h/p?a=b,c;d:e@f/g"), "http://h/p?a=b,c;d:e@f/g");
    }

    #[test]
    fn request_with_spaces_and_quotes_reaches_the_server() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = c.read(&mut buf).unwrap();
            c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
            String::from_utf8_lossy(&buf[..n]).lines().next().unwrap().to_string()
        });
        let body = super::fetch(&format!("http://127.0.0.1:{port}/a b?q=\"x\" y"), &[], "t").unwrap();
        assert_eq!(body, b"ok");
        assert_eq!(h.join().unwrap(), "GET /a%20b?q=%22x%22%20y HTTP/1.1");
    }

    #[test]
    fn file_url_is_bounded_and_regular_only() {
        assert!(super::fetch("file:///dev/zero", &[], "t").is_err());
        let t = tempfile::tempdir().unwrap();
        let big = t.path().join("big");
        std::fs::File::create(&big).unwrap().set_len(super::MAX_RESPONSE + 1).unwrap();
        assert!(super::fetch(&format!("file://{}", big.display()), &[], "t").is_err());
        let ok = t.path().join("ok");
        std::fs::write(&ok, "hi").unwrap();
        assert_eq!(super::fetch(&format!("file://{}", ok.display()), &[], "t").unwrap(), b"hi");
    }

    #[test]
    fn download_file_cannot_escape_its_folder() {
        let base = std::path::Path::new("/skin/DownloadFile");
        for n in ["../x", "/etc/passwd", "a/../../x", "..\\..\\x"] {
            let p = super::confine(base, n);
            assert!(p.starts_with(base) && !p.components().any(|c| c.as_os_str() == ".."), "{n} -> {p:?}");
        }
        assert_eq!(super::confine(base, "sub/f.txt"), base.join("sub/f.txt"));
    }

    use super::*;

    #[test]
    fn regex_groups_and_errors() {
        let g = run_regex("(?siU)<b>(.*)</b>(\\d+)", "x <b>hi</b>42").unwrap().unwrap();
        assert_eq!(g, vec!["<b>hi</b>4", "hi", "4"]);
        assert_eq!(run_regex("zzz", "abc").unwrap(), None);
        assert!(run_regex("(unclosed", "abc").is_err());
        assert_eq!(run_regex("", "abc").unwrap().unwrap(), vec![""]);
    }

    #[test]
    fn entity_decoding_modes() {
        assert_eq!(decode_references("a &amp; b &#65; &#x42; &#128512;", 1), "a & b A B &#128512;");
        assert_eq!(decode_references("&amp;&#65;", 2), "&amp;A");
        assert_eq!(decode_references("&amp;&#65;", 3), "&&#65;");
        assert_eq!(decode_code_points("x\\u0041y\\U00000042"), "xAyB");
    }

    #[test]
    fn decodes_code_pages() {
        assert_eq!(decode(&[0x61, 0xe9], 1252), "a\u{e9}");
        assert_eq!(decode(&[0x61, 0x00, 0x62, 0x00], 1200), "ab");
        assert_eq!(decode("é".as_bytes(), 0), "é");
    }

    #[test]
    fn child_url_replacement_is_case_insensitive() {
        assert_eq!(replace_ci("http://x/[Parent].png", "[parent]", "a b"), "http://x/a b.png");
    }
}
