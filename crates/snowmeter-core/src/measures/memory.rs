//! PhysicalMemory / Memory / SwapMemory (`/proc/meminfo`).

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

/// Values from `/proc/meminfo`, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MemInfo {
    pub total: f64,
    pub available: f64,
    pub swap_total: f64,
    pub swap_free: f64,
}

pub fn parse_meminfo(text: &str) -> MemInfo {
    let mut m = MemInfo::default();
    let (mut free, mut buffers, mut cached) = (0.0, 0.0, 0.0);
    let mut have_avail = false;
    for line in text.lines() {
        let Some((k, rest)) = line.split_once(':') else { continue };
        let kb: f64 = rest.split_whitespace().next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
        let bytes = kb * 1024.0;
        match k {
            "MemTotal" => m.total = bytes,
            "MemAvailable" => {
                m.available = bytes;
                have_avail = true;
            }
            "MemFree" => free = bytes,
            "Buffers" => buffers = bytes,
            "Cached" => cached = bytes,
            "SwapTotal" => m.swap_total = bytes,
            "SwapFree" => m.swap_free = bytes,
            _ => {}
        }
    }
    if !have_avail {
        m.available = free + buffers + cached;
    }
    m
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MemoryKind {
    /// `PhysicalMemory`
    Physical,
    /// `SwapMemory` (Rainmeter "commit charge"): RAM in use + swap in use
    Swap,
    /// `Memory`: Windows commit charge + RAM in use
    Both,
}

type Source = Box<dyn FnMut() -> Option<String>>;

pub struct MemoryMeasure {
    kind: MemoryKind,
    total: bool,
    source: Source,
    max: f64,
    captured: bool,
    value: f64,
}

impl MemoryMeasure {
    pub fn new(kind: MemoryKind) -> MemoryMeasure {
        MemoryMeasure::with_source(kind, Box::new(|| std::fs::read_to_string("/proc/meminfo").ok()))
    }
    pub fn with_source(kind: MemoryKind, source: Source) -> MemoryMeasure {
        MemoryMeasure { kind, total: false, source, max: 0.0, captured: false, value: 0.0 }
    }

    /// (used, max) for this kind.
    fn figures(&self, m: &MemInfo, captured_phys_max: f64) -> (f64, f64) {
        let phys_used = (m.total - m.available).max(0.0);
        let swap_used = (m.swap_total - m.swap_free).max(0.0);
        match self.kind {
            MemoryKind::Physical => (phys_used, captured_phys_max),
            MemoryKind::Swap => (phys_used + swap_used, m.total + m.swap_total),
            MemoryKind::Both => (2.0 * phys_used + swap_used, 2.0 * m.total + m.swap_total),
        }
    }

    fn refresh(&mut self) {
        let Some(text) = (self.source)() else { return };
        let m = parse_meminfo(&text);
        if !self.captured {
            self.max = m.total;
            self.captured = true;
        }
        let (used, max) = self.figures(&m, self.max);
        if self.kind != MemoryKind::Physical {
            self.max = max;
        }
        self.value = if self.total { self.max } else { used };
    }
}

impl MeasureImpl for MemoryMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.total = opt.bool("Total", false);
        // Make the maximum known before the first update.
        if let Some(text) = (self.source)() {
            let m = parse_meminfo(&text);
            if !self.captured {
                self.max = m.total;
                self.captured = true;
            }
            if self.kind != MemoryKind::Physical {
                self.max = self.figures(&m, self.max).1;
            }
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.refresh();
        self.value
    }

    fn forced_max(&self) -> Option<f64> {
        Some(self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    const INFO: &str = "MemTotal:       16000000 kB\nMemFree:         2000000 kB\nMemAvailable:    6000000 kB\nBuffers: 100 kB\nCached: 200 kB\nSwapTotal:       4000000 kB\nSwapFree:        3000000 kB\n";

    #[test]
    fn parses_meminfo_in_bytes() {
        let m = parse_meminfo(INFO);
        assert_eq!(m.total, 16_000_000.0 * 1024.0);
        assert_eq!(m.available, 6_000_000.0 * 1024.0);
        assert_eq!(m.swap_total, 4_000_000.0 * 1024.0);
        assert_eq!(m.swap_free, 3_000_000.0 * 1024.0);
    }

    #[test]
    fn falls_back_without_memavailable() {
        let m = parse_meminfo("MemTotal: 100 kB\nMemFree: 10 kB\nBuffers: 5 kB\nCached: 5 kB\n");
        assert_eq!(m.available, 20.0 * 1024.0);
    }

    fn run(kind: MemoryKind, ini: &str) -> (f64, Option<f64>) {
        let s = skin(ini);
        let mut mm = MemoryMeasure::with_source(kind, Box::new(|| Some(INFO.to_string())));
        mm.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        let v = ctx_run(&s, |c| mm.update(c));
        (v, mm.forced_max())
    }

    #[test]
    fn physical_memory_used_and_max() {
        let (v, max) = run(MemoryKind::Physical, "[M]\nMeasure=PhysicalMemory\n");
        assert_eq!(v, 10_000_000.0 * 1024.0);
        assert_eq!(max, Some(16_000_000.0 * 1024.0));
    }

    #[test]
    fn total_flag_returns_the_max() {
        let (v, max) = run(MemoryKind::Physical, "[M]\nMeasure=PhysicalMemory\nTotal=1\n");
        assert_eq!(Some(v), max);
    }

    #[test]
    fn swap_memory_is_commit_like() {
        let (v, max) = run(MemoryKind::Swap, "[M]\nMeasure=SwapMemory\n");
        assert_eq!(v, (10_000_000.0 + 1_000_000.0) * 1024.0);
        assert_eq!(max, Some(20_000_000.0 * 1024.0));
    }

    #[test]
    fn memory_adds_physical_and_commit() {
        let (v, max) = run(MemoryKind::Both, "[M]\nMeasure=Memory\n");
        assert_eq!(v, (20_000_000.0 + 1_000_000.0) * 1024.0);
        assert_eq!(max, Some(36_000_000.0 * 1024.0));
    }

    #[test]
    fn real_meminfo_is_sane() {
        let s = skin("[M]\nMeasure=PhysicalMemory\n");
        let mut mm = MemoryMeasure::new(MemoryKind::Physical);
        mm.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        let v = ctx_run(&s, |c| mm.update(c));
        assert!(v > 0.0 && v <= mm.forced_max().unwrap());
    }
}
