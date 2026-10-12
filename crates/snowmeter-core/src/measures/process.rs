//! Process plugin: 1 while a process with `ProcessName` runs, -1 otherwise.

use super::sysmon::{same_process_name, shared_snapshot, Snapshot};
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

#[derive(Default)]
pub struct ProcessMeasure {
    name: String,
    source: Option<fn() -> Snapshot>,
}

impl ProcessMeasure {
    pub fn new() -> ProcessMeasure {
        ProcessMeasure::default()
    }
}

/// 1 if any process in `snap` matches `wanted`, else -1.
pub fn running_value(snap: &Snapshot, wanted: &str) -> f64 {
    if snap.procs.iter().any(|p| same_process_name(&p.name, wanted)) {
        1.0
    } else {
        -1.0
    }
}

impl MeasureImpl for ProcessMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.name = opt.string("ProcessName", "");
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        if self.name.trim().is_empty() {
            return -1.0;
        }
        let snap = match self.source {
            Some(f) => f(),
            None => shared_snapshot(false),
        };
        running_value(&snap, &self.name)
    }

    fn forced_min(&self) -> Option<f64> {
        Some(-1.0)
    }

    fn forced_max(&self) -> Option<f64> {
        Some(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::sysmon::ProcInfo;
    use crate::measures::testutil::{ctx_run, skin};

    fn fake() -> Snapshot {
        Snapshot {
            procs: vec![ProcInfo { pid: 1, name: "Firefox".into(), ..Default::default() }, ProcInfo { pid: 2, name: "bash".into(), ..Default::default() }],
            valid: true,
            ..Default::default()
        }
    }

    #[test]
    fn matches_with_and_without_exe_case_insensitive() {
        let s = fake();
        assert_eq!(running_value(&s, "firefox.exe"), 1.0);
        assert_eq!(running_value(&s, "FIREFOX"), 1.0);
        assert_eq!(running_value(&s, "chrome.exe"), -1.0);
        assert_eq!(running_value(&s, ""), -1.0);
    }

    #[test]
    fn measure_reads_option_and_uses_the_sampler() {
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=Process\nProcessName=bash.exe\n[N]\nMeasure=Plugin\nPlugin=Process\n");
        let mut m = ProcessMeasure { name: String::new(), source: Some(fake) };
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        ctx_run(&sk, |c| assert_eq!(m.update(c), 1.0));
        let mut n = ProcessMeasure { name: String::new(), source: Some(fake) };
        n.read_options(&OptionReader { parser: &sk.parser, section: "N", sv: None });
        ctx_run(&sk, |c| assert_eq!(n.update(c), -1.0));
        // The real sampler sees this very process.
        let me = std::env::current_exe().unwrap();
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
        let mut r = ProcessMeasure { name: comm.trim().to_string(), source: None };
        ctx_run(&sk, |c| assert_eq!(r.update(c), 1.0, "{me:?}"));
    }
}
