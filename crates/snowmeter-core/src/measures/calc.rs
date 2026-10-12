//! Calc measure.

use super::{MeasureCtx, MeasureImpl};
use crate::formula;
use crate::parser::OptionReader;
use std::cell::Cell;

/// Small xorshift64* generator (no external dependency).
pub struct Rng(Cell<u64>);

impl Rng {
    pub fn seeded() -> Rng {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Rng::with_seed(nanos ^ (u64::from(std::process::id()) << 32))
    }
    pub fn with_seed(seed: u64) -> Rng {
        Rng(Cell::new(seed | 1))
    }
    pub fn next_u64(&self) -> u64 {
        let mut x = self.0.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform integer in `[lo, hi]`.
    pub fn range(&self, lo: i64, hi: i64) -> i64 {
        let span = (hi as i128 - lo as i128 + 1) as u128;
        lo + (u128::from(self.next_u64()) % span) as i64
    }
}

pub struct CalcMeasure {
    formula: String,
    resolved: String,
    low: i64,
    high: i64,
    update_random: bool,
    unique_random: bool,
    deck: Vec<i64>,
    rng: Rng,
    value: f64,
    error_logged: bool,
    log: Vec<String>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl CalcMeasure {
    pub fn new() -> CalcMeasure {
        CalcMeasure::with_rng(Rng::seeded())
    }
    pub fn with_rng(rng: Rng) -> CalcMeasure {
        CalcMeasure {
            formula: String::new(),
            resolved: String::new(),
            low: 0,
            high: 100,
            update_random: false,
            unique_random: false,
            deck: Vec::new(),
            rng,
            value: 0.0,
            error_logged: false,
            log: Vec::new(),
        }
    }

    fn random(&mut self) -> i64 {
        if self.low >= self.high {
            if self.low > self.high && !self.error_logged {
                self.log.push(format!("Calc: \"LowBound\" ({}) must be less then or equal to \"HighBound\" ({})", self.low, self.high));
            }
            return self.low;
        }
        if self.unique_random && self.high - self.low < 65535 {
            if self.deck.is_empty() {
                self.deck = (self.low..=self.high).collect();
                for i in (1..self.deck.len()).rev() {
                    let j = self.rng.range(0, i as i64) as usize;
                    self.deck.swap(i, j);
                }
            }
            return self.deck.pop().unwrap_or(self.low);
        }
        self.rng.range(self.low, self.high)
    }

    /// Replace whole-word `Random` tokens by a number.
    fn replace_random(&mut self) -> String {
        let chars: Vec<char> = self.formula.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if is_word(chars[i]) {
                let start = i;
                while i < chars.len() && is_word(chars[i]) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if word.eq_ignore_ascii_case("random") {
                    out.push_str(&self.random().to_string());
                } else {
                    out.push_str(&word);
                }
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }
}

impl Default for CalcMeasure {
    fn default() -> Self {
        CalcMeasure::new()
    }
}

impl MeasureImpl for CalcMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.formula = opt.string("Formula", "");
        let (low, high) = (opt.int("LowBound", 0), opt.int("HighBound", 100));
        if (low, high) != (self.low, self.high) {
            self.deck.clear();
        }
        self.low = low;
        self.high = high;
        self.update_random = opt.bool("UpdateRandom", false);
        self.unique_random = opt.bool("UniqueRandom", false);
        self.resolved = if self.update_random { String::new() } else { self.replace_random() };
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        if self.update_random {
            self.resolved = self.replace_random();
        }
        if self.resolved.trim().is_empty() {
            self.value = 0.0;
            return 0.0;
        }
        let counter = ctx.counter as f64;
        let rng_low = self.low;
        let rng_high = self.high;
        let rng = &self.rng;
        let lookup = ctx.lookup;
        let lk = |name: &str| -> Option<f64> {
            if let Some(v) = lookup(name) {
                return Some(v);
            }
            let n = name.to_ascii_lowercase();
            if !n.is_empty() && "counter".starts_with(&n) {
                return Some(counter);
            }
            if !n.is_empty() && "random".starts_with(&n) {
                return Some(if rng_low >= rng_high { rng_low } else { rng.range(rng_low, rng_high) } as f64);
            }
            None
        };
        match formula::evaluate(&self.resolved, &lk) {
            Ok(v) => {
                self.value = v;
                self.error_logged = false;
            }
            Err(e) => {
                if !self.error_logged {
                    self.log.push(format!("Calc: {e}"));
                    self.error_logged = true;
                }
            }
        }
        self.value
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_with, skin};

    fn make(ini: &str) -> (crate::measures::testutil::TestSkin, CalcMeasure) {
        let s = skin(ini);
        let mut c = CalcMeasure::with_rng(Rng::with_seed(12345));
        c.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        (s, c)
    }
    fn step(s: &crate::measures::testutil::TestSkin, c: &mut CalcMeasure, counter: u64, lk: &dyn Fn(&str) -> Option<f64>) -> f64 {
        ctx_with(s, lk, |ctx| {
            ctx.counter = counter;
            c.update(ctx)
        })
    }
    fn none(_: &str) -> Option<f64> {
        None
    }

    #[test]
    fn plain_formulas() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nFormula=1/3\n");
        assert!((step(&s, &mut c, 1, &none) - 1.0 / 3.0).abs() < 1e-12);
        let (s, mut c) = make("[M]\nMeasure=Calc\nFormula=\n");
        assert_eq!(step(&s, &mut c, 1, &none), 0.0);
        let (s, mut c) = make("[M]\nMeasure=Calc\nFormula=(2+3)*4\n");
        assert_eq!(step(&s, &mut c, 1, &none), 20.0);
    }

    #[test]
    fn counter_and_prefix_quirk() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nFormula=Counter % 2\n");
        let got: Vec<f64> = (1..=4).map(|n| step(&s, &mut c, n, &none)).collect();
        assert_eq!(got, [1.0, 0.0, 1.0, 0.0]);
        let (s, mut c) = make("[M]\nMeasure=Calc\nFormula=co\n");
        assert_eq!(step(&s, &mut c, 7, &none), 7.0);
    }

    #[test]
    fn measure_names_resolve_through_lookup() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nFormula=MeasureLoop*2 + other\n");
        let lk = |n: &str| match n.to_ascii_lowercase().as_str() {
            "measureloop" => Some(21.0),
            "other" => Some(1.0),
            _ => None,
        };
        assert_eq!(step(&s, &mut c, 1, &lk), 43.0);
    }

    #[test]
    fn measure_name_wins_over_counter_prefix() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nFormula=c\n");
        assert_eq!(step(&s, &mut c, 5, &|n: &str| (n == "c").then_some(99.0)), 99.0);
    }

    #[test]
    fn random_fixed_until_reread() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nLowBound=1\nHighBound=6\nFormula=Random\n");
        let first = step(&s, &mut c, 1, &none);
        assert!((1.0..=6.0).contains(&first));
        for n in 2..20 {
            assert_eq!(step(&s, &mut c, n, &none), first);
        }
    }

    #[test]
    fn update_random_changes_each_update() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nLowBound=1\nHighBound=1000\nUpdateRandom=1\nFormula=Random\n");
        let vals: std::collections::HashSet<i64> = (1..30).map(|n| step(&s, &mut c, n, &none) as i64).collect();
        assert!(vals.len() > 5, "{vals:?}");
        assert!(vals.iter().all(|v| (1..=1000).contains(v)));
    }

    #[test]
    fn degenerate_bounds() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nLowBound=5\nHighBound=5\nFormula=Random\n");
        assert_eq!(step(&s, &mut c, 1, &none), 5.0);
        let (s, mut c) = make("[M]\nMeasure=Calc\nLowBound=7\nHighBound=3\nFormula=Random\n");
        assert_eq!(step(&s, &mut c, 1, &none), 7.0);
        assert!(c.take_log().iter().any(|l| l.contains("LowBound")));
    }

    #[test]
    fn unique_random_is_a_permutation_per_deck() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nLowBound=1\nHighBound=3\nUniqueRandom=1\nUpdateRandom=1\nFormula=Random\n");
        for _ in 0..4 {
            let mut triple: Vec<i64> = (1..=3).map(|n| step(&s, &mut c, n, &none) as i64).collect();
            triple.sort();
            assert_eq!(triple, [1, 2, 3]);
        }
    }

    #[test]
    fn parse_error_keeps_previous_value_and_logs_once() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nFormula=1 +* 2\n");
        assert_eq!(step(&s, &mut c, 1, &none), 0.0);
        step(&s, &mut c, 2, &none);
        assert_eq!(c.take_log().len(), 1);
    }

    #[test]
    fn whole_word_random_only() {
        let (s, mut c) = make("[M]\nMeasure=Calc\nLowBound=4\nHighBound=4\nFormula=Random+Random\n");
        assert_eq!(step(&s, &mut c, 1, &none), 8.0);
    }
}
