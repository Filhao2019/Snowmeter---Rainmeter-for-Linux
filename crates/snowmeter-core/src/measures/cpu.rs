//! CPU measure (`/proc/stat`).

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

/// Idle and total jiffies of one CPU line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

/// Parse `/proc/stat`: index 0 is the aggregate `cpu` line, then `cpu0`, `cpu1`, ...
/// `idle` includes iowait; `total` excludes guest time (already in user/nice).
pub fn parse_proc_stat(text: &str) -> Vec<CpuTimes> {
    let mut all = None;
    let mut cores = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(name) = it.next() else { continue };
        if !name.starts_with("cpu") {
            continue;
        }
        let nums: Vec<u64> = it.map(|x| x.parse().unwrap_or(0)).collect();
        if nums.len() < 4 {
            continue;
        }
        let get = |i: usize| nums.get(i).copied().unwrap_or(0);
        // user nice system idle iowait irq softirq steal
        let idle = get(3) + get(4);
        let total = get(0) + get(1) + get(2) + get(3) + get(4) + get(5) + get(6) + get(7);
        let t = CpuTimes { idle, total };
        if name == "cpu" {
            all = Some(t);
        } else {
            cores.push(t);
        }
    }
    let mut out = Vec::new();
    if let Some(a) = all {
        out.push(a);
    }
    out.extend(cores);
    out
}

type Source = Box<dyn FnMut() -> Option<String>>;

pub struct CpuMeasure {
    processor: usize,
    source: Source,
    prev: (u64, u64),
    value: f64,
    log: Vec<String>,
}

impl CpuMeasure {
    pub fn new() -> CpuMeasure {
        CpuMeasure::with_source(Box::new(|| std::fs::read_to_string("/proc/stat").ok()))
    }
    /// Read the stat text from `source` instead of `/proc/stat` (tests).
    pub fn with_source(source: Source) -> CpuMeasure {
        CpuMeasure { processor: 0, source, prev: (0, 0), value: 0.0, log: Vec::new() }
    }
}

impl Default for CpuMeasure {
    fn default() -> Self {
        CpuMeasure::new()
    }
}

impl MeasureImpl for CpuMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let want = opt.int("Processor", 0);
        let count = (self.source)().map(|t| parse_proc_stat(&t).len().saturating_sub(1)).unwrap_or(0) as i64;
        let p = if want < 0 || (count > 0 && want > count) {
            self.log.push(format!("CPU: Processor={want} is not valid"));
            0
        } else {
            want
        } as usize;
        if p != self.processor {
            self.prev = (0, 0);
        }
        self.processor = p;
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let Some(text) = (self.source)() else { return self.value };
        let times = parse_proc_stat(&text);
        let Some(t) = times.get(self.processor) else { return self.value };
        let d_idle = t.idle.saturating_sub(self.prev.0);
        let d_total = t.total.saturating_sub(self.prev.1);
        self.prev = (t.idle, t.total);
        if d_total > 0 {
            self.value = (100.0 * (1.0 - d_idle as f64 / d_total as f64)).clamp(0.0, 100.0);
        }
        self.value
    }

    fn default_max(&self) -> f64 {
        100.0
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};
    use crate::parser::OptionReader;
    use std::cell::RefCell;
    use std::rc::Rc;

    const STAT1: &str = "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0 0 0\nintr 1\n";
    const STAT2: &str = "cpu  150 0 150 900 50 0 0 0 5 5\ncpu0 100 0 50 450 0 0 0 0 0 0\ncpu1 50 0 100 450 50 0 0 0 0 0\n";

    #[test]
    fn parses_stat_lines() {
        let v = parse_proc_stat(STAT1);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], CpuTimes { idle: 800, total: 1000 });
        assert_eq!(v[1], CpuTimes { idle: 400, total: 500 });
        // iowait counts as idle, guest columns are ignored
        let w = parse_proc_stat(STAT2);
        assert_eq!(w[0], CpuTimes { idle: 950, total: 1250 });
    }

    #[test]
    fn first_sample_is_load_since_boot_then_deltas() {
        let texts = Rc::new(RefCell::new(vec![STAT2.to_string(), STAT1.to_string()]));
        let t = texts.clone();
        let mut cpu = CpuMeasure::with_source(Box::new(move || t.borrow().last().cloned()));
        let s = skin("[M]\nMeasure=CPU\n");
        cpu.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        // since boot: busy 200 of 1000
        let v1 = ctx_run(&s, |c| cpu.update(c));
        assert!((v1 - 20.0).abs() < 1e-9, "{v1}");
        texts.borrow_mut().pop();
        texts.borrow_mut().push(STAT2.to_string());
        // delta: idle 150 of total 250 -> 40%
        let v2 = ctx_run(&s, |c| cpu.update(c));
        assert!((v2 - 40.0).abs() < 1e-9, "{v2}");
    }

    #[test]
    fn per_core_and_invalid_processor() {
        let mut cpu = CpuMeasure::with_source(Box::new(|| Some(STAT1.to_string())));
        let s = skin("[M]\nMeasure=CPU\nProcessor=2\n");
        cpu.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert!((ctx_run(&s, |c| cpu.update(c)) - 20.0).abs() < 1e-9);
        let s = skin("[M]\nMeasure=CPU\nProcessor=9\n");
        cpu.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(cpu.take_log().len(), 1);
    }

    #[test]
    fn real_proc_stat_gives_a_percentage() {
        let mut cpu = CpuMeasure::new();
        let v = ctx_run(&skin("[M]\nMeasure=CPU\n"), |c| cpu.update(c));
        assert!((0.0..=100.0).contains(&v), "{v}");
    }
}
