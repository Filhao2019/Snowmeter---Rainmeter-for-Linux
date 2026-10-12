//! `Substitute` / `RegExpSubstitute` (Measure.cpp `ParseSubstitute`, `CheckSubstitute`).

use pcre2::bytes::RegexBuilder;

/// Parsed substitute pairs.
#[derive(Clone, Debug, Default)]
pub struct Substitute {
    pairs: Vec<(String, String)>,
    regexp: bool,
}

/// Port of `Measure::ExtractWord`: returns the next word and consumes it.
fn extract_word(buf: &mut Vec<char>) -> String {
    let len = buf.len();
    if len == 0 {
        return String::new();
    }
    let mut end = 0;
    while end < len && matches!(buf[end], ' ' | '\t' | '\n') {
        end += 1;
    }
    let mut ret = String::new();
    let consumed;
    if end == len {
        consumed = len;
    } else {
        buf.drain(..end);
        let len = buf.len();
        if buf[0] == '"' || buf[0] == '\'' {
            let quote = buf[0];
            let mut e = 1;
            while e < len && buf[e] != quote {
                e += 1;
            }
            if e == len {
                consumed = len; // unterminated: discard
            } else {
                ret = buf[1..e].iter().collect();
                consumed = e + 1;
            }
        } else {
            let mut e = 0;
            while e < len && !matches!(buf[e], ',' | ':' | ' ' | '\t') {
                e += 1;
            }
            if e == len {
                ret = buf.iter().collect();
                consumed = len;
            } else {
                // the separator is returned as part of the word
                e += 1;
                ret = buf[..e].iter().collect();
                consumed = e;
            }
        }
    }
    buf.drain(..consumed.min(buf.len()));
    ret
}

/// Parse an already re-quoted buffer of `"name":"value"` pairs.
pub fn parse_substitute_wrapped(buffer: &str, regexp: bool, log: &mut Vec<String>) -> Substitute {
    let mut sub = Substitute { pairs: Vec::new(), regexp };
    let mut buf: Vec<char> = buffer.chars().collect();
    let invalid = |log: &mut Vec<String>| log.push(format!("Measure: Invalid Substitute={buffer}"));
    while !buf.is_empty() {
        let mut word1 = extract_word(&mut buf);
        let sep = extract_word(&mut buf);
        if sep != ":" {
            invalid(log);
            return sub;
        }
        let word2 = extract_word(&mut buf);
        if word1 != word2 {
            if regexp && word1.is_empty() {
                word1 = "^$".to_string();
            }
            sub.pairs.push((word1, word2));
        }
        let sep2 = extract_word(&mut buf);
        if !sep2.is_empty() && sep2 != "," {
            invalid(log);
            return sub;
        }
    }
    sub
}

/// Parse the (already INI-unquoted) option value. Invalid input logs
/// `Measure: Invalid Substitute=...`; pairs parsed before the error are kept.
pub fn parse_substitute(raw: &str, regexp: bool, log: &mut Vec<String>) -> Substitute {
    if raw.is_empty() {
        return Substitute { pairs: Vec::new(), regexp };
    }
    let first = raw.chars().next();
    let last = raw.chars().last();
    let mixed = (first == Some('"') && last == Some('\'')) || (first == Some('\'') && last == Some('"'));
    if mixed {
        parse_substitute_wrapped(raw, regexp, log)
    } else {
        parse_substitute_wrapped(&format!("\"{raw}\""), regexp, log)
    }
}

fn plain_replace(s: &mut String, pat: &str, rep: &str) {
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find(pat) {
        out.push_str(&rest[..i]);
        out.push_str(rep);
        rest = &rest[i + pat.len()..];
    }
    out.push_str(rest);
    *s = out;
}

fn next_char_boundary(s: &str, mut i: usize) -> usize {
    i += 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

impl Substitute {
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    /// Apply every pair in order to `s`.
    pub fn apply(&self, s: &str, log: &mut Vec<String>) -> String {
        if self.pairs.is_empty() {
            return s.to_string();
        }
        let mut out = s.to_string();
        for (pat, rep) in &self.pairs {
            if !self.regexp {
                if !pat.is_empty() {
                    plain_replace(&mut out, pat, rep);
                } else if out.is_empty() {
                    out = rep.clone();
                }
                continue;
            }
            match RegexBuilder::new().utf(true).build(pat) {
                Err(e) => {
                    if !pat.is_empty() {
                        plain_replace(&mut out, pat, rep);
                    } else if out.is_empty() {
                        out = rep.clone();
                    }
                    log.push(format!("Substitute: {e}"));
                }
                Ok(re) => out = regex_replace(&re, &out, rep),
            }
        }
        out
    }
}

/// Global replace that skips empty matches (unless the subject is empty),
/// substitutes `\N` back references, and resumes after the replacement.
fn regex_replace(re: &pcre2::bytes::Regex, input: &str, rep: &str) -> String {
    let mut s = input.to_string();
    let mut offset = 0usize;
    let mut locs = re.capture_locations();
    loop {
        if offset > s.len() {
            break;
        }
        // Find the first non-empty match at or after `offset`.
        let mut from = offset;
        let found = loop {
            match re.captures_read_at(&mut locs, s.as_bytes(), from) {
                Ok(Some(m)) => {
                    if m.start() == m.end() && !s.is_empty() {
                        if m.start() >= s.len() {
                            break None;
                        }
                        from = next_char_boundary(&s, m.start());
                        continue;
                    }
                    break Some((m.start(), m.end()));
                }
                _ => break None,
            }
        };
        let Some((ms, me)) = found else { break };
        let mut ngroups = 0;
        for i in 0..locs.len() {
            if locs.get(i).is_some() {
                ngroups = i + 1;
            }
        }
        let mut result = rep.to_string();
        if ngroups > 1 {
            for j in (0..ngroups).rev() {
                let Some((gs, ge)) = locs.get(j) else { continue };
                if ge <= gs {
                    continue;
                }
                let text = s[gs..ge].to_string();
                plain_replace(&mut result, &format!("\\{j}"), &text);
            }
        }
        s.replace_range(ms..me, &result);
        offset = ms + result.len();
        let _ = me;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(raw: &str, re: bool) -> (Substitute, Vec<String>) {
        let mut log = Vec::new();
        let s = parse_substitute(raw, re, &mut log);
        (s, log)
    }
    fn run(raw: &str, re: bool, input: &str) -> String {
        let (s, _) = sub(raw, re);
        s.apply(input, &mut Vec::new())
    }

    #[test]
    fn sequential_pairs() {
        assert_eq!(run(r#"a":"b","b":"c"#, false, "ab"), "cc");
    }
    #[test]
    fn no_rescan_of_inserted_text() {
        assert_eq!(run(r#"aa":"a"#, false, "aaaa"), "aa");
    }
    #[test]
    fn empty_pattern_only_fills_empty_string() {
        assert_eq!(run(r#"":"N/A"#, false, ""), "N/A");
        assert_eq!(run(r#"":"N/A"#, false, "x"), "x");
    }
    #[test]
    fn separators_inside_quotes_are_literal() {
        assert_eq!(run(r#"a,b":"c:d"#, false, "a,b"), "c:d");
        assert_eq!(run(r#"a":"  "#, false, "a"), "  ");
    }
    #[test]
    fn mixed_quote_styles() {
        // value starts with " and ends with ' so it is not re-wrapped
        assert_eq!(run(r#""first":'second'"#, false, "first"), "second");
        let mut l = Vec::new(); assert_eq!(parse_substitute_wrapped(r#"'"':''"#, false, &mut l).apply(r#"say "hi""#, &mut l), "say hi");
    }
    #[test]
    fn invalid_substitutes_log_and_do_nothing() {
        for raw in ["a':'b", "a:b"] {
            let (s, log) = sub(raw, false);
            assert!(s.is_empty(), "{raw}");
            assert_eq!(log.len(), 1, "{raw}");
            assert!(log[0].contains("Invalid Substitute"), "{log:?}");
            assert_eq!(s.apply("a", &mut Vec::new()), "a");
        }
    }
    #[test]
    fn identical_pair_is_dropped() {
        let (s, _) = sub(r#"a":"a,"b":"c"#, false);
        assert_eq!(s.pairs().len(), 1);
    }
    #[test]
    fn earlier_pairs_kept_on_error() {
        let (s, log) = sub(r#"a":"b","c"#, false);
        assert_eq!(s.pairs(), &[("a".to_string(), "b".to_string())]);
        assert_eq!(log.len(), 1);
    }
    #[test]
    fn regexp_backreferences() {
        assert_eq!(run(r#"(\d+)":"<\1>"#, true, "a12b"), "a<12>b");
        assert_eq!(
            run(r#"^(.*?)\s*-\s*(.*)$":"\2 by \1"#, true, "Artist - Title"),
            "Title by Artist"
        );
    }
    #[test]
    fn regexp_never_matches_empty_in_non_empty_subject() {
        assert_eq!(run(r#"a*":"x"#, true, "baaac"), "bxc");
    }
    #[test]
    fn regexp_empty_pattern_matches_empty_subject_only() {
        assert_eq!(run(r#"":"Empty"#, true, ""), "Empty");
        assert_eq!(run(r#"":"Empty"#, true, "z"), "z");
    }
    #[test]
    fn regexp_unset_group_stays_literal() {
        assert_eq!(run(r#"(a)|(b)":"[\2]"#, true, "a"), r"[\2]");
    }
    #[test]
    fn regexp_invalid_pattern_falls_back_to_plain() {
        let (s, _) = sub(r#"[":"x"#, true);
        let mut log = Vec::new();
        assert_eq!(s.apply("a[b[", &mut log), "axbx");
        assert!(!log.is_empty());
    }
    #[test]
    fn regexp_inline_flags() {
        assert_eq!(run(r#"(?i)TUE":"Di"#, true, "Tue"), "Di");
        assert_eq!(run(r#"(?U)<.+>":"X"#, true, "<a><b>"), "XX");
    }
    #[test]
    fn regexp_replacement_not_rescanned() {
        assert_eq!(run(r#"a":"aa"#, true, "aa"), "aaaa");
    }
}
