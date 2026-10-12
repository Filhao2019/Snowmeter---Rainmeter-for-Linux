//! IfAbove/IfBelow/IfEqual, IfCondition and IfMatch (IfActions.cpp).

use crate::formula;
use crate::parser::OptionReader;
use pcre2::bytes::{Regex, RegexBuilder};

#[derive(Default)]
struct Item {
    value: String,
    t_action: String,
    f_action: String,
    t_committed: bool,
    f_committed: bool,
    parse_error: bool,
    /// Compiled pattern (IfMatch only).
    regex: Option<Regex>,
    regex_error: Option<String>,
}

#[derive(Default)]
pub struct IfActions {
    above_value: f64,
    below_value: f64,
    equal_value: i64,
    above_action: String,
    below_action: String,
    equal_action: String,
    above_committed: bool,
    below_committed: bool,
    equal_committed: bool,
    conditions: Vec<Item>,
    condition_mode: bool,
    matches: Vec<Item>,
    match_mode: bool,
}

/// Read the `IfXxx`, `IfXxx2`, ... chain. Returns (value, true action, false action).
fn read_chain(opt: &OptionReader, cond: &str, t: &str, f: &str, expand_value: bool) -> Vec<(String, String, String)> {
    let read_value = |key: &str| {
        if expand_value {
            opt.string(key, "")
        } else {
            opt.action(key)
        }
    };
    let mut out = Vec::new();
    let mut condition = read_value(cond);
    if condition.is_empty() {
        return out;
    }
    let mut ta = opt.action(t);
    let mut fa = opt.action(f);
    if ta.is_empty() && fa.is_empty() {
        return out;
    }
    let mut i = 1;
    loop {
        out.push((condition, ta, fa));
        i += 1;
        condition = read_value(&format!("{cond}{i}"));
        if condition.is_empty() {
            break;
        }
        ta = opt.action(&format!("{t}{i}"));
        fa = opt.action(&format!("{f}{i}"));
        if ta.is_empty() && fa.is_empty() {
            break;
        }
    }
    out
}

fn fill(items: &mut Vec<Item>, chain: Vec<(String, String, String)>, is_match: bool) {
    items.truncate(chain.len());
    for (i, (value, t, f)) in chain.into_iter().enumerate() {
        if i == items.len() {
            items.push(Item::default());
        }
        let it = &mut items[i];
        if it.value != value {
            it.parse_error = false;
        }
        it.t_action = t;
        it.f_action = f;
        if is_match {
            match RegexBuilder::new().utf(true).build(&value) {
                Ok(re) => {
                    it.regex = Some(re);
                    it.regex_error = None;
                }
                Err(e) => {
                    it.regex = None;
                    it.regex_error = Some(e.to_string());
                }
            }
        }
        it.value = value;
    }
}

impl IfActions {
    pub fn new() -> IfActions {
        IfActions::default()
    }

    /// IfAbove/IfBelow/IfEqual options.
    pub fn read_options(&mut self, opt: &OptionReader) {
        self.above_action = opt.action("IfAboveAction");
        self.above_value = opt.float("IfAboveValue", 0.0);
        self.below_action = opt.action("IfBelowAction");
        self.below_value = opt.float("IfBelowValue", 0.0);
        self.equal_action = opt.action("IfEqualAction");
        self.equal_value = opt.float("IfEqualValue", 0.0) as i64;
    }

    /// IfCondition[N] and IfMatch[N] options.
    pub fn read_condition_options(&mut self, opt: &OptionReader) {
        self.condition_mode = opt.bool("IfConditionMode", false);
        let chain = read_chain(opt, "IfCondition", "IfTrueAction", "IfFalseAction", true);
        fill(&mut self.conditions, chain, false);
        self.match_mode = opt.bool("IfMatchMode", false);
        let chain = read_chain(opt, "IfMatch", "IfMatchAction", "IfNotMatchAction", true);
        fill(&mut self.matches, chain, true);
    }

    /// Evaluate everything for one update. `value` is the raw (non-inverted)
    /// value, `string` the measure's string value or "". Actions to execute
    /// are appended to `out` in firing order.
    pub fn evaluate(
        &mut self,
        value: f64,
        string: &str,
        lookup: &dyn Fn(&str) -> Option<f64>,
        out: &mut Vec<String>,
        log: &mut Vec<String>,
    ) {
        if !self.equal_action.is_empty() {
            if value as i64 == self.equal_value {
                if !self.equal_committed {
                    self.equal_committed = true;
                    out.push(self.equal_action.clone());
                }
            } else {
                self.equal_committed = false;
            }
        }
        if !self.above_action.is_empty() {
            if value > self.above_value {
                if !self.above_committed {
                    self.above_committed = true;
                    out.push(self.above_action.clone());
                }
            } else {
                self.above_committed = false;
            }
        }
        if !self.below_action.is_empty() {
            if value < self.below_value {
                if !self.below_committed {
                    self.below_committed = true;
                    out.push(self.below_action.clone());
                }
            } else {
                self.below_committed = false;
            }
        }

        let mode = self.condition_mode;
        for (idx, item) in self.conditions.iter_mut().enumerate() {
            if item.value.is_empty() || (item.t_action.is_empty() && item.f_action.is_empty()) {
                continue;
            }
            match formula::evaluate(&item.value, lookup) {
                Err(msg) => {
                    if !item.parse_error {
                        let n = if idx == 0 { String::new() } else { (idx + 1).to_string() };
                        log.push(format!("{msg}: IfCondition{n}={}", item.value));
                        item.parse_error = true;
                    }
                }
                Ok(result) => {
                    item.parse_error = false;
                    if result == 1.0 {
                        item.f_committed = false;
                        if mode || !item.t_committed {
                            item.t_committed = true;
                            out.push(item.t_action.clone());
                        }
                    } else if result == 0.0 {
                        item.t_committed = false;
                        if mode || !item.f_committed {
                            item.f_committed = true;
                            out.push(item.f_action.clone());
                        }
                    }
                }
            }
        }

        let mode = self.match_mode;
        for (idx, item) in self.matches.iter_mut().enumerate() {
            if item.value.is_empty() || (item.t_action.is_empty() && item.f_action.is_empty()) {
                continue;
            }
            let Some(re) = &item.regex else {
                if !item.parse_error {
                    let n = if idx == 0 { String::new() } else { (idx + 1).to_string() };
                    let e = item.regex_error.clone().unwrap_or_default();
                    log.push(format!("Error: \"{e}\" in IfMatch{n}={}", item.value));
                    item.parse_error = true;
                }
                continue;
            };
            item.parse_error = false;
            if re.is_match(string.as_bytes()).unwrap_or(false) {
                item.f_committed = false;
                if mode || !item.t_committed {
                    item.t_committed = true;
                    out.push(item.t_action.clone());
                }
            } else {
                item.t_committed = false;
                if mode || !item.f_committed {
                    item.f_committed = true;
                    out.push(item.f_action.clone());
                }
            }
        }
    }

    /// Disabled-measure state reset (`IfActions::SetState`).
    pub fn set_state(&mut self, value: f64) {
        if self.equal_value != value as i64 {
            self.equal_committed = false;
        }
        if self.above_value <= value {
            self.above_committed = false;
        }
        if self.below_value >= value {
            self.below_committed = false;
        }
        for it in self.conditions.iter_mut().chain(self.matches.iter_mut()) {
            it.t_committed = false;
            it.f_committed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::skin;

    fn none(_: &str) -> Option<f64> {
        None
    }

    fn make(extra: &str) -> (crate::measures::testutil::TestSkin, IfActions) {
        let s = skin(&format!("[M]\nMeasure=Calc\n{extra}\n"));
        let mut ia = IfActions::new();
        {
            let opt = OptionReader { parser: &s.parser, section: "M", sv: None };
            ia.read_options(&opt);
            ia.read_condition_options(&opt);
        }
        (s, ia)
    }

    /// Run the values through and return the fired action per tick ("-" for none).
    fn run(ia: &mut IfActions, values: &[f64], lookup: &dyn Fn(&str) -> Option<f64>) -> Vec<String> {
        values
            .iter()
            .map(|v| {
                let mut out = Vec::new();
                ia.evaluate(*v, "", lookup, &mut out, &mut Vec::new());
                if out.is_empty() {
                    "-".to_string()
                } else {
                    out.join("+")
                }
            })
            .collect()
    }

    #[test]
    fn if_above_is_edge_triggered() {
        let (_s, mut ia) = make("IfAboveValue=50\nIfAboveAction=[!A]");
        assert_eq!(run(&mut ia, &[60.0, 70.0, 40.0, 60.0], &none), ["[!A]", "-", "-", "[!A]"]);
    }

    #[test]
    fn if_below_is_edge_triggered() {
        let (_s, mut ia) = make("IfBelowValue=10\nIfBelowAction=B");
        assert_eq!(run(&mut ia, &[5.0, 3.0, 20.0, 1.0], &none), ["B", "-", "-", "B"]);
    }

    #[test]
    fn if_equal_truncates_toward_zero_and_resets() {
        let (_s, mut ia) = make("IfEqualValue=5\nIfEqualAction=E");
        assert_eq!(run(&mut ia, &[5.9, 5.2, 6.0, 5.0], &none), ["E", "-", "-", "E"]);
        let (_s, mut ia) = make("IfEqualValue=0\nIfEqualAction=Z");
        assert_eq!(run(&mut ia, &[-0.5], &none), ["Z"]);
    }

    #[test]
    fn if_condition_latching() {
        let (_s, mut ia) = make("IfCondition=MeasureCPU > 50\nIfTrueAction=T\nIfFalseAction=F");
        let cpu = std::cell::Cell::new(0.0);
        let lk = |n: &str| n.eq_ignore_ascii_case("MeasureCPU").then(|| cpu.get());
        let mut got = Vec::new();
        for v in [10.0, 60.0, 70.0, 40.0, 30.0] {
            cpu.set(v);
            got.push(run(&mut ia, &[0.0], &lk).remove(0));
        }
        assert_eq!(got, ["F", "T", "-", "F", "-"]);
    }

    #[test]
    fn if_condition_mode_one_fires_every_update() {
        let (_s, mut ia) = make("IfConditionMode=1\nIfCondition=MeasureCPU > 50\nIfTrueAction=T\nIfFalseAction=F");
        let cpu = std::cell::Cell::new(0.0);
        let lk = |n: &str| n.eq_ignore_ascii_case("MeasureCPU").then(|| cpu.get());
        let mut got = Vec::new();
        for v in [10.0, 60.0, 70.0, 40.0, 30.0] {
            cpu.set(v);
            got.push(run(&mut ia, &[0.0], &lk).remove(0));
        }
        assert_eq!(got, ["F", "T", "T", "F", "F"]);
    }

    #[test]
    fn if_condition_with_non_boolean_result_does_nothing() {
        let (_s, mut ia) = make("IfCondition=MeasureCPU\nIfTrueAction=T\nIfFalseAction=F");
        let lk = |n: &str| n.eq_ignore_ascii_case("MeasureCPU").then(|| 37.0);
        assert_eq!(run(&mut ia, &[0.0, 0.0], &lk), ["-", "-"]);
    }

    #[test]
    fn if_condition_chain_stops_without_actions() {
        let (_s, mut ia) = make(
            "IfCondition=1\nIfTrueAction=T1\nIfCondition2=1\nIfCondition3=1\nIfTrueAction3=T3\nIfFalseAction3=F3",
        );
        assert_eq!(run(&mut ia, &[0.0], &none), ["T1"]);
    }

    #[test]
    fn if_condition2_with_actions_is_evaluated_in_order() {
        let (_s, mut ia) = make(
            "IfCondition=1\nIfTrueAction=T1\nIfCondition2=0\nIfFalseAction2=F2\nIfCondition3=1\nIfTrueAction3=T3",
        );
        assert_eq!(run(&mut ia, &[0.0], &none), ["T1+F2+T3"]);
    }

    #[test]
    fn first_condition_without_actions_disables_chain() {
        let (_s, mut ia) = make("IfCondition=1\nIfCondition2=1\nIfTrueAction2=X");
        assert_eq!(run(&mut ia, &[0.0], &none), ["-"]);
    }

    #[test]
    fn if_condition_parse_error_logs_once() {
        let (_s, mut ia) = make("IfCondition=1 +* 2\nIfTrueAction=T");
        let mut log = Vec::new();
        for _ in 0..3 {
            ia.evaluate(0.0, "", &none, &mut Vec::new(), &mut log);
        }
        assert_eq!(log.len(), 1, "{log:?}");
        assert!(log[0].contains("IfCondition=1 +* 2"));
    }

    #[test]
    fn if_match_uses_string_value() {
        let (_s, mut ia) = make("IfMatch=^ab\nIfMatchAction=Y\nIfNotMatchAction=N");
        let mut out = Vec::new();
        ia.evaluate(0.0, "abc", &none, &mut out, &mut Vec::new());
        ia.evaluate(0.0, "abd", &none, &mut out, &mut Vec::new());
        ia.evaluate(0.0, "xyz", &none, &mut out, &mut Vec::new());
        ia.evaluate(0.0, "xyz", &none, &mut out, &mut Vec::new());
        assert_eq!(out, ["Y", "N"]);
    }

    #[test]
    fn if_match_empty_subject_matches_anchored_empty() {
        let (_s, mut ia) = make("IfMatch=^$\nIfMatchAction=Y\nIfNotMatchAction=N");
        assert_eq!(run(&mut ia, &[1.0, 2.0], &none), ["Y", "-"]);
    }

    #[test]
    fn if_match_invalid_regex_logs_once_and_is_skipped() {
        let (_s, mut ia) = make("IfMatch=(\nIfMatchAction=Y\nIfNotMatchAction=N");
        let mut log = Vec::new();
        let mut out = Vec::new();
        for _ in 0..2 {
            ia.evaluate(0.0, "a", &none, &mut out, &mut log);
        }
        assert!(out.is_empty());
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn set_state_resets_latches() {
        // faithful to IfActions::SetState: Above resets when AboveValue <= value
        let (_s, mut ia) = make("IfAboveValue=-5\nIfAboveAction=A");
        assert_eq!(run(&mut ia, &[60.0, 60.0], &none), ["A", "-"]);
        ia.set_state(0.0);
        assert_eq!(run(&mut ia, &[60.0], &none), ["A"]);
        let (_s, mut ia) = make("IfAboveValue=50\nIfAboveAction=A");
        assert_eq!(run(&mut ia, &[60.0], &none), ["A"]);
        ia.set_state(0.0);
        assert_eq!(run(&mut ia, &[60.0], &none), ["-"]);
    }

    #[test]
    fn actions_are_read_raw() {
        let (_s, mut ia) = make("Variables=1\nIfAboveValue=1\nIfAboveAction=[!SetVariable X #Y#][M]");
        assert_eq!(run(&mut ia, &[5.0], &none), ["[!SetVariable X #Y#][M]"]);
    }
}
