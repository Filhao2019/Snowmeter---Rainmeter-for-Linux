//! Variable substitution passes of `ConfigParser`: `%ENV%`, old-style
//! `#Var#`, the bracket forms `[#Var]` `[&Measure]` `[%ENV]` `[\65]`, and
//! old-style `[Measure]` section references. Pure functions over lookup
//! closures so the parser decides where values come from.

use crate::numparse;

const MAX_DEPTH: usize = 10;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    /// Every key: `& # % \`.
    AllKeys,
    /// `# % \` only (used for `[Variables]`, `@Include`, plain variable replacement).
    HashOnly,
}

pub(crate) type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Environment variable with Windows names mapped to XDG equivalents.
pub(crate) fn env_var(name: &str) -> Option<String> {
    let home = || std::env::var("HOME").unwrap_or_default();
    match name.to_ascii_uppercase().as_str() {
        "APPDATA" => Some(std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| format!("{}/.config", home()))),
        "LOCALAPPDATA" => Some(std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| format!("{}/.local/share", home()))),
        "USERPROFILE" => Some(home()),
        "TEMP" | "TMP" => Some(std::env::temp_dir().to_string_lossy().trim_end_matches('/').to_string()),
        _ => std::env::var(name).ok(),
    }
}

/// `%NAME%` expansion (ExpandEnvironmentStrings); unknown names stay as is.
pub(crate) fn expand_env_percent(s: &str) -> String {
    if s.matches('%').count() < 2 {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find('%') {
        let after = &rest[a + 1..];
        match after.find('%') {
            Some(b) if b > 0 => match env_var(&after[..b]) {
                Some(v) => {
                    out.push_str(&rest[..a]);
                    out.push_str(&v);
                    rest = &after[b + 1..];
                }
                None => {
                    out.push_str(&rest[..a + 1]);
                    rest = after;
                }
            },
            _ => {
                out.push_str(&rest[..a + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Old-style `#Var#` with the `#*Var*#` escape. Values are never rescanned and
/// the closing `#` of an unknown name becomes a candidate opener.
pub(crate) fn expand_old_style(s: &str, var: Lookup) -> String {
    if s.len() < 3 || !s.contains('#') {
        return s.to_string();
    }
    let mut c: Vec<char> = s.chars().collect();
    let mut start = 0usize;
    while let Some(st) = find_from(&c, '#', start) {
        let si = st + 1;
        let Some(end) = find_from(&c, '#', si) else { break };
        let ei = end - 1;
        if si != ei && end > si && c[si] == '*' && c[ei] == '*' {
            c.remove(ei);
            c.remove(si);
            start = ei;
        } else {
            let name: String = c[si..end].iter().collect();
            match var(&name) {
                Some(value) => {
                    let v: Vec<char> = value.chars().collect();
                    start = st + v.len();
                    c.splice(st..=end, v);
                }
                None => start = end,
            }
        }
    }
    c.into_iter().collect()
}

fn find_from(c: &[char], ch: char, from: usize) -> Option<usize> {
    if from >= c.len() {
        return None;
    }
    c[from..].iter().position(|&x| x == ch).map(|p| p + from)
}

fn rfind_upto(c: &[char], ch: char, upto: usize) -> Option<usize> {
    let upto = upto.min(c.len().saturating_sub(1));
    if c.is_empty() {
        return None;
    }
    (0..=upto).rev().find(|&i| c[i] == ch)
}

fn is_var_key(c: char) -> bool {
    matches!(c, '&' | '#' | '%' | '$' | '\\')
}

/// Bracket pass (`ExpandSectionVariables`): innermost first, left to right.
pub(crate) fn expand_brackets(s: &str, mode: Mode, var: Lookup, section: Option<Lookup>) -> String {
    if !s.contains(']') {
        return s.to_string();
    }
    let mut c: Vec<char> = s.chars().collect();
    expand_chars(&mut c, mode, var, section, 0);
    c.into_iter().collect()
}

fn expand_chars(c: &mut Vec<char>, mode: Mode, var: Lookup, section: Option<Lookup>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let mut end = 0usize;
    while let Some(e) = find_from(c, ']', end) {
        end = e;
        if end > 0 {
            let ei = end - 1;
            let mut start = end;
            while start != 0 {
                let Some(st) = rfind_upto(c, '[', start - 1) else { break };
                start = st;
                let si = start + 2;
                // Escaped variable: [#*Name*]
                if si != ei && si < c.len() && c[si] == '*' && c[ei] == '*' {
                    c.remove(ei);
                    c.remove(si);
                    break;
                }
                let key_idx = start + 1;
                if end == key_idx {
                    break;
                }
                let key = c[key_idx];
                if end == key_idx + 1 {
                    break; // empty name: "[#]"
                }
                if !is_var_key(key) {
                    continue;
                }
                let name: String = c[key_idx + 1..end].iter().collect();
                let found: Option<String> = match key {
                    '&' if mode == Mode::AllKeys => section.and_then(|f| f(&name)),
                    '#' => var(&name),
                    '%' => env_var(&name),
                    '\\' => char_reference(&name),
                    _ => None,
                };
                if let Some(value) = found {
                    let mut v: Vec<char> = value.chars().collect();
                    if (key == '&' || key == '#') && depth < MAX_DEPTH {
                        expand_chars(&mut v, mode, var, section, depth + 1);
                    }
                    let vlen = v.len();
                    c.splice(start..=end, v);
                    end = (start + vlen).wrapping_sub(1);
                    break;
                }
            }
        }
        end = end.wrapping_add(1);
    }
}

/// `[\65]` decimal or `[\x42]` hex character reference.
fn char_reference(name: &str) -> Option<String> {
    let (digits, base) = match name.strip_prefix(['x', 'X']) {
        Some(rest) => (rest, 16),
        None => (name, 10),
    };
    if digits.is_empty() {
        return None;
    }
    let chars: Vec<char> = digits.chars().collect();
    let (v, end, overflow) = numparse::strtol(&chars, 0, base);
    if end != chars.len() || overflow || v <= 0 || v >= 0xFFFE {
        return None;
    }
    char::from_u32(v as u32).map(|c| c.to_string())
}

/// Old-style `[Measure]` / `[Meter:X]` references after the bracket pass.
pub(crate) fn expand_old_measures(s: &str, section: Lookup) -> String {
    if !s.contains('[') {
        return s.to_string();
    }
    let mut c: Vec<char> = s.chars().collect();
    let mut start = 0usize;
    while let Some(st) = find_from(&c, '[', start) {
        let si = st + 1;
        let Some(end) = find_from(&c, ']', si) else { break };
        let next = find_from(&c, '[', si);
        if next.is_none() || end < next.unwrap() {
            let ei = end - 1;
            if si != ei && end > si && c[si] == '*' && c[ei] == '*' {
                c.remove(ei);
                c.remove(si);
                start = ei;
            } else {
                let name: String = c[si..end].iter().collect();
                match section(&name) {
                    Some(value) => {
                        let v: Vec<char> = value.chars().collect();
                        start = st + v.len();
                        c.splice(st..=end, v);
                    }
                    None => start = end,
                }
            }
        } else {
            start = next.unwrap();
        }
    }
    c.into_iter().collect()
}

/// Split `Section:selector` at the last `:` that precedes the first `(`.
pub(crate) fn split_selector(name: &str) -> Option<(&str, &str)> {
    let limit = name.find('(').unwrap_or(name.len());
    let colon = name[..limit].rfind(':')?;
    Some((&name[..colon], &name[colon + 1..]))
}
