//! Loop measure.

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

pub struct LoopMeasure {
    start: i64,
    end: i64,
    increment: i64,
    count: i64,
    invert: bool,
    value: i64,
    skip_first: bool,
    loops: i64,
    finished: bool,
}

impl LoopMeasure {
    pub fn new() -> LoopMeasure {
        LoopMeasure { start: 1, end: 100, increment: 1, count: 0, invert: false, value: 1, skip_first: true, loops: 0, finished: false }
    }
    fn reset(&mut self) {
        self.value = self.start;
        self.skip_first = true;
        self.loops = 0;
        self.finished = false;
    }
}

impl Default for LoopMeasure {
    fn default() -> Self {
        LoopMeasure::new()
    }
}

impl MeasureImpl for LoopMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let (s, e, i, c, inv) = (
            opt.int("StartValue", 1),
            opt.int("EndValue", 100),
            opt.int("Increment", 1),
            opt.int("LoopCount", 0),
            opt.bool("InvertMeasure", false),
        );
        if (s, e, i, c, inv) != (self.start, self.end, self.increment, self.count, self.invert) || (self.loops == 0 && self.skip_first) {
            self.start = s;
            self.end = e;
            self.increment = i;
            self.count = c;
            self.invert = inv;
            self.reset();
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        if self.skip_first {
            self.skip_first = false;
            return self.value as f64;
        }
        if self.finished {
            return self.value as f64;
        }
        let at_end = if self.increment >= 0 { self.value >= self.end } else { self.value <= self.end };
        if at_end {
            self.loops += 1;
            if self.count > 0 && self.loops >= self.count {
                self.finished = true;
                return self.value as f64;
            }
            self.value = self.start;
        } else {
            let next = self.value + self.increment;
            let past = if self.increment >= 0 { next >= self.end } else { next <= self.end };
            self.value = if past { self.end } else { next };
        }
        self.value as f64
    }

    fn forced_min(&self) -> Option<f64> {
        Some(self.start.min(self.end) as f64)
    }
    fn forced_max(&self) -> Option<f64> {
        Some(self.start.max(self.end) as f64)
    }

    fn command(&mut self, args: &str) {
        if args.trim().eq_ignore_ascii_case("reset") {
            self.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    fn seq(ini: &str, n: usize) -> Vec<i64> {
        let s = skin(ini);
        let mut l = LoopMeasure::new();
        l.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        (0..n).map(|_| ctx_run(&s, |c| l.update(c)) as i64).collect()
    }

    #[test]
    fn wraps_after_end() {
        assert_eq!(seq("[M]\nMeasure=Loop\nStartValue=1\nEndValue=5\n", 11), [1, 2, 3, 4, 5, 1, 2, 3, 4, 5, 1]);
    }

    #[test]
    fn increment_clamps_to_end() {
        assert_eq!(seq("[M]\nMeasure=Loop\nStartValue=1\nEndValue=6\nIncrement=2\n", 7), [1, 3, 5, 6, 1, 3, 5]);
    }

    #[test]
    fn loop_count_stops() {
        assert_eq!(seq("[M]\nMeasure=Loop\nStartValue=1\nEndValue=5\nLoopCount=1\n", 8), [1, 2, 3, 4, 5, 5, 5, 5]);
    }

    #[test]
    fn range_is_forced() {
        let s = skin("[M]\nMeasure=Loop\nStartValue=2\nEndValue=9\n");
        let mut l = LoopMeasure::new();
        l.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!((l.forced_min(), l.forced_max()), (Some(2.0), Some(9.0)));
    }
}
