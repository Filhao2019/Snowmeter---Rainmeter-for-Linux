//! Action-string parsing (`Library/CommandHandler.cpp` `ExecuteCommand` and
//! `ParseString`).
//!
//! Variable and measure substitution is the caller's job and must happen
//! before the actions run, exactly like Rainmeter expands them at execution
//! time. This module only splits an action string into its parts.

/// One executable unit of an action string.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `!Name arg arg`. `name` is lowercase, without `!` or a `Rainmeter` prefix.
    /// `!Delay n` is reported like any other bang; the executor delays the
    /// actions that follow it in the returned list.
    Bang { name: String, args: Vec<String> },
    /// Anything else: a program, URL or file to open. A bracket group whose
    /// first char is a variable key (`[#Bangs]`) is returned verbatim as
    /// `Command("[#Bangs]")` for the caller to expand and re-parse.
    Command(String),
}

fn is_space(c: char) -> bool {
    c.is_whitespace()
}

/// Parse `[!A x][!B "y z"]`, a bare `!A x`, or a plain command.
pub fn parse_actions(s: &str) -> Vec<Action> {
    let mut out = Vec::new();
    parse_into(s, true, &mut out);
    out
}

fn parse_into(command: &str, multi: bool, out: &mut Vec<Action>) {
    let command = command.trim_start_matches(is_space);
    if command.is_empty() {
        return;
    }
    let mut rest = command;
    if let Some(after) = rest.strip_prefix('!') {
        if after.len() >= 7 && after.is_char_boundary(7) && after[..7].eq_ignore_ascii_case("execute") {
            match after[7..].find('[') {
                Some(p) => rest = &after[7 + p..],
                None => return,
            }
        } else {
            let after = if after.len() >= 9 && after.is_char_boundary(9) && after[..9].eq_ignore_ascii_case("rainmeter") {
                &after[9..]
            } else {
                after
            };
            let (name, args) = match after.find(' ') {
                Some(p) => (&after[..p], split_args(&after[p + 1..])),
                None => (after, Vec::new()),
            };
            if !name.is_empty() {
                out.push(Action::Bang { name: name.to_ascii_lowercase(), args });
            }
            return;
        }
    }
    if multi && rest.starts_with('[') {
        parse_groups(rest, out);
    } else {
        out.push(Action::Command(rest.to_string()));
    }
}

/// Walk `[...]` groups counting depth; only `"""..."""` protects brackets.
fn parse_groups(text: &str, out: &mut Vec<Action>) {
    let chars: Vec<char> = text.chars().collect();
    let mut start: Option<usize> = None;
    let mut count = 0i32;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '[' => {
                if count == 0 {
                    start = Some(i);
                }
                count += 1;
            }
            ']' => {
                count -= 1;
                if count == 0 {
                    if let Some(st) = start {
                        let inner: String = chars[st + 1..i].iter().collect();
                        emit_group(&inner, out);
                    }
                }
            }
            '"' if i + 2 < chars.len() && chars[i + 1] == '"' && chars[i + 2] == '"' => {
                i += 3;
                if let Some(p) = (i..chars.len().saturating_sub(2)).find(|&p| chars[p] == '"' && chars[p + 1] == '"' && chars[p + 2] == '"') {
                    i = p + 2;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn emit_group(inner: &str, out: &mut Vec<Action>) {
    let trimmed = inner.trim_start_matches([' ', '\t', '\r', '\n']);
    if matches!(trimmed.chars().next(), Some('&' | '#' | '%' | '$' | '\\')) {
        out.push(Action::Command(format!("[{inner}]")));
    } else {
        parse_into(trimmed, false, out);
    }
}

/// Rainmeter argument splitting: spaces separate, `"x y"` groups, `"""x "y" z"""`
/// is taken verbatim, and stray `"` inside an unquoted word are dropped.
pub fn split_args(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut arg: Vec<char> = s.chars().collect();
    loop {
        let Some(pos) = arg.iter().position(|&c| c != ' ') else { break };
        let mut extra = 1;
        let end: Option<usize>;
        if arg[pos] == '"' {
            if arg.len() > pos + 2 && arg[pos + 1] == '"' && arg[pos + 2] == '"' {
                arg.drain(..pos + 3);
                extra = 4;
                end = match find_seq(&arg, &['"', '"', '"', ' ']) {
                    Some(p) => Some(p),
                    None => {
                        extra = 3;
                        rfind_seq(&arg, &['"', '"', '"'])
                    }
                };
            } else {
                arg.drain(..pos + 1);
                end = arg.iter().position(|&c| c == '"');
            }
        } else {
            arg.drain(..pos);
            end = arg.iter().position(|&c| c == ' ');
        }
        match end {
            Some(p) => {
                let word: String = arg[..p].iter().collect();
                arg.drain(..(p + extra).min(arg.len()));
                result.push(if extra == 1 { word.replace('"', "") } else { word });
            }
            None => {
                let word: String = arg.iter().collect();
                result.push(if extra == 1 { word.replace('"', "") } else { word });
                arg.clear();
                break;
            }
        }
    }
    result
}

fn find_seq(hay: &[char], needle: &[char]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind_seq(hay: &[char], needle: &[char]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}
