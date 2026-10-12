//! UsageMonitor plugin on Linux: the Windows performance counters Rainmeter
//! skins ask for (Process, Processor, Memory, Network Interface, LogicalDisk)
//! mapped onto `/proc`.

use super::sysmon::{shared_snapshot, Snapshot};
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Instance(usize),
    Sum,
    Average,
    Count,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Counter {
    Cpu,
    WorkingSet,
    Private,
    Io,
    IoRead,
    IoWrite,
    // Processor
    CpuTotal,
    // Memory
    MemUsedPercent,
    MemAvailable { div: u32 },
    MemUsed,
    // Network
    NetRx,
    NetTx,
    NetTotal,
    // Disk
    DiskRead,
    DiskWrite,
    DiskTotal,
    DiskBusy,
    FreePercent,
    FreeMb,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Process,
    Processor,
    Memory,
    Network,
    Disk,
    Unsupported,
}

fn classify(category: &str, counter: &str) -> (Group, Counter) {
    let c = category.trim().to_ascii_lowercase();
    let k = counter.trim().to_ascii_lowercase();
    match c.as_str() {
        "process" => (
            Group::Process,
            match k.as_str() {
                "% processor time" | "% user time" | "% privileged time" => Counter::Cpu,
                "working set" => Counter::WorkingSet,
                "working set - private" | "private bytes" => Counter::Private,
                "io data bytes/sec" => Counter::Io,
                "io read bytes/sec" => Counter::IoRead,
                "io write bytes/sec" => Counter::IoWrite,
                _ => Counter::Unsupported,
            },
        ),
        "processor" | "processor information" | "processor utility" => (Group::Processor, Counter::CpuTotal),
        "memory" => (
            Group::Memory,
            match k.as_str() {
                "% committed bytes in use" => Counter::MemUsedPercent,
                "available bytes" => Counter::MemAvailable { div: 1 },
                "available kbytes" => Counter::MemAvailable { div: 1024 },
                "available mbytes" => Counter::MemAvailable { div: 1024 * 1024 },
                "committed bytes" | "cache bytes" => Counter::MemUsed,
                _ => Counter::Unsupported,
            },
        ),
        "network interface" | "network adapter" => (
            Group::Network,
            match k.as_str() {
                "bytes received/sec" => Counter::NetRx,
                "bytes sent/sec" => Counter::NetTx,
                "bytes total/sec" => Counter::NetTotal,
                _ => Counter::Unsupported,
            },
        ),
        "logicaldisk" | "physicaldisk" => (
            Group::Disk,
            match k.as_str() {
                "disk read bytes/sec" => Counter::DiskRead,
                "disk write bytes/sec" => Counter::DiskWrite,
                "disk bytes/sec" => Counter::DiskTotal,
                "% disk time" | "% idle time" => Counter::DiskBusy,
                "% free space" => Counter::FreePercent,
                "free megabytes" => Counter::FreeMb,
                _ => Counter::Unsupported,
            },
        ),
        _ => (Group::Unsupported, Counter::Unsupported),
    }
}

fn norm_name(s: &str) -> String {
    let l = s.trim().to_ascii_lowercase();
    l.strip_suffix(".exe").map(str::to_string).unwrap_or(l)
}

/// One instance of a counter: name and value.
type Inst = (String, f64);

/// Free-space numbers of a mount (bytes free, bytes total).
fn statvfs_root(path: &str) -> Option<(f64, f64)> {
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: statvfs fills the zeroed struct for a valid NUL-terminated path.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        let fr = s.f_frsize as f64;
        Some((s.f_bavail as f64 * fr, s.f_blocks as f64 * fr))
    }
}

/// Instances of `counter` in `snap`, rolled up by name for processes.
fn instances(snap: &Snapshot, group: Group, counter: Counter, rollup: bool, mount: &str) -> Vec<Inst> {
    match group {
        Group::Process => {
            let ncpu = snap.cores.len().max(1) as f64;
            let mut map: HashMap<String, f64> = HashMap::new();
            let mut order: Vec<String> = Vec::new();
            for (i, p) in snap.procs.iter().enumerate() {
                let v = match counter {
                    Counter::Cpu => p.cpu * ncpu,
                    Counter::WorkingSet => p.rss,
                    Counter::Private => p.private,
                    Counter::Io => p.io_read + p.io_write,
                    Counter::IoRead => p.io_read,
                    Counter::IoWrite => p.io_write,
                    _ => 0.0,
                };
                let base = norm_name(&p.name);
                let key = if rollup { base } else if map.contains_key(&base) { format!("{base}#{i}") } else { base };
                if !map.contains_key(&key) {
                    order.push(key.clone());
                }
                *map.entry(key).or_insert(0.0) += v;
            }
            order.into_iter().map(|k| (k.clone(), map[&k])).collect()
        }
        Group::Processor => {
            let mut v: Vec<Inst> = snap.cores.iter().enumerate().map(|(i, c)| (i.to_string(), *c)).collect();
            v.push(("_Total".into(), snap.cpu_total));
            v
        }
        Group::Memory => {
            let used = (snap.mem_total - snap.mem_available).max(0.0);
            let v = match counter {
                Counter::MemUsedPercent => {
                    if snap.mem_total > 0.0 {
                        used / snap.mem_total * 100.0
                    } else {
                        0.0
                    }
                }
                Counter::MemAvailable { div } => snap.mem_available / f64::from(div),
                _ => used,
            };
            vec![("_Total".into(), v)]
        }
        Group::Network => {
            let mut v: Vec<Inst> = snap
                .net
                .iter()
                .map(|n| {
                    let x = match counter {
                        Counter::NetRx => n.rx_bps,
                        Counter::NetTx => n.tx_bps,
                        _ => n.rx_bps + n.tx_bps,
                    };
                    (n.name.clone(), x)
                })
                .collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        }
        Group::Disk => {
            let v = match counter {
                Counter::DiskRead => snap.disk_read_bps,
                Counter::DiskWrite => snap.disk_write_bps,
                Counter::DiskTotal => snap.disk_read_bps + snap.disk_write_bps,
                Counter::DiskBusy => snap.disk_busy,
                Counter::FreePercent => statvfs_root(mount).map(|(f, t)| if t > 0.0 { f / t * 100.0 } else { 0.0 }).unwrap_or(0.0),
                Counter::FreeMb => statvfs_root(mount).map(|(f, _)| f / (1024.0 * 1024.0)).unwrap_or(0.0),
                _ => 0.0,
            };
            vec![("_Total".into(), v)]
        }
        Group::Unsupported => Vec::new(),
    }
}

/// Apply the white/black list, pick the instance and compute (name, value).
fn select(list: &[Inst], pick: Pick, name: &str, white: &[String], black: &[String]) -> Inst {
    let allowed = |n: &str| {
        let k = norm_name(n);
        if !white.is_empty() {
            white.contains(&k)
        } else {
            !black.contains(&k)
        }
    };
    // Single-instance sources have nothing to pick.
    if let [only] = list.iter().filter(|(n, _)| n == "_Total").collect::<Vec<_>>()[..] {
        if list.len() == 1 {
            return only.clone();
        }
    }
    if !name.trim().is_empty() {
        let w = norm_name(name);
        let v: f64 = list.iter().filter(|(n, _)| norm_name(n) == w || n.eq_ignore_ascii_case(name.trim())).map(|(_, v)| *v).sum();
        return (name.trim().to_string(), v);
    }
    let mut v: Vec<&Inst> = list.iter().filter(|(n, _)| allowed(n)).collect();
    match pick {
        Pick::Sum => (String::new(), v.iter().map(|(_, x)| x).sum()),
        Pick::Average => {
            let n = v.len().max(1) as f64;
            (String::new(), v.iter().map(|(_, x)| x).sum::<f64>() / n)
        }
        Pick::Count => (String::new(), v.len() as f64),
        Pick::Instance(i) => {
            v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
            v.get(i.saturating_sub(1)).map(|(n, x)| (n.clone(), *x)).unwrap_or_default()
        }
    }
}

pub struct UsageMonitor {
    group: Group,
    counter: Counter,
    pick: Pick,
    name: String,
    raw: bool,
    percent: bool,
    rollup: bool,
    white: Vec<String>,
    black: Vec<String>,
    mount: String,
    max_default: f64,
    current: String,
    source: Option<fn() -> Snapshot>,
    log: Vec<String>,
    warned: bool,
}

impl UsageMonitor {
    pub fn new() -> UsageMonitor {
        UsageMonitor {
            group: Group::Unsupported,
            counter: Counter::Unsupported,
            pick: Pick::Sum,
            name: String::new(),
            raw: false,
            percent: false,
            rollup: true,
            white: Vec::new(),
            black: Vec::new(),
            mount: "/".into(),
            max_default: 0.0,
            current: String::new(),
            source: None,
            log: Vec::new(),
            warned: false,
        }
    }

    fn value_from(&mut self, snap: &Snapshot) -> f64 {
        let list = instances(snap, self.group, self.counter, self.rollup, &self.mount);
        let (name, mut value) = select(&list, self.pick, &self.name, &self.white, &self.black);
        if self.percent && !self.raw && self.pick != Pick::Count && self.group == Group::Process && self.counter == Counter::Cpu {
            // Instance values are per-core percentages; Percent scales them to the whole machine.
            value /= snap.cores.len().max(1) as f64;
        }
        self.current = if value == 0.0 && self.pick != Pick::Count && self.name.trim().is_empty() { String::new() } else { name };
        value
    }
}

fn parse_list(s: &str) -> Vec<String> {
    s.split('|').map(norm_name).filter(|x| !x.is_empty()).collect()
}

impl MeasureImpl for UsageMonitor {
    fn read_options(&mut self, opt: &OptionReader) {
        let alias = opt.string("Alias", "").trim().to_ascii_uppercase();
        let (mut cat, mut ctr, mut percent) = match alias.as_str() {
            "CPU" => ("Process", "% Processor Time", true),
            "RAM" => ("Process", "Working Set - Private", false),
            "RAMSHARED" => ("Process", "Working Set", false),
            "IO" => ("Process", "IO Data Bytes/sec", false),
            "IOREAD" => ("Process", "IO Read Bytes/sec", false),
            "IOWRITE" => ("Process", "IO Write Bytes/sec", false),
            "NETDOWN" => ("Network Interface", "Bytes Received/sec", false),
            "NETUP" => ("Network Interface", "Bytes Sent/sec", false),
            _ => ("", "", false),
        };
        let c = opt.string("Category", "");
        let k = opt.string("Counter", "");
        let (c, k) = (if c.trim().is_empty() { cat.to_string() } else { c }, if k.trim().is_empty() { ctr.to_string() } else { k });
        cat = "";
        ctr = "";
        let _ = (cat, ctr);
        let (group, counter) = classify(&c, &k);
        self.group = group;
        self.counter = counter;
        if (group == Group::Unsupported || counter == Counter::Unsupported) && !self.warned && !c.is_empty() {
            self.warned = true;
            self.log.push(format!("UsageMonitor: \"{c}\" / \"{k}\" is not supported on Linux"));
        }
        if matches!(group, Group::Processor) {
            percent = true;
        }
        let index = opt.string("Index", "");
        self.pick = match index.trim().to_ascii_uppercase().as_str() {
            "SUM" => Pick::Sum,
            "AVERAGE" => Pick::Average,
            "COUNT" => Pick::Count,
            other => match other.parse::<i64>().unwrap_or(0) {
                0 => Pick::Sum,
                -1 => Pick::Average,
                n if n > 0 => Pick::Instance(n as usize),
                _ => Pick::Sum,
            },
        };
        self.name = opt.string("Name", "");
        self.raw = opt.bool("RawValue", false);
        self.rollup = opt.bool("Rollup", true);
        let white = opt.string("Whitelist", "");
        if white.trim().is_empty() {
            self.white.clear();
            self.black = parse_list(&opt.string("Blacklist", "_Total|Idle"));
        } else {
            self.white = parse_list(&white);
            self.black.clear();
        }
        self.percent = opt.bool("Percent", percent);
        // Free-space counters can name a mount point instead of a drive letter.
        self.mount = if self.name.starts_with('/') { self.name.clone() } else { "/".into() };
        self.max_default = if (self.percent && !(self.pick == Pick::Count && self.name.is_empty())) || self.group == Group::Processor { 100.0 } else { 0.0 };
        if matches!(counter, Counter::MemUsedPercent | Counter::FreePercent | Counter::DiskBusy) {
            self.max_default = 100.0;
        }
        // Process mode "% Processor Time" is a share of the machine only with Percent.
        if self.group == Group::Process && self.counter == Counter::Cpu && !self.percent {
            self.max_default = 0.0;
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        if self.group == Group::Unsupported || self.counter == Counter::Unsupported {
            return 0.0;
        }
        let want_io = matches!(self.counter, Counter::Io | Counter::IoRead | Counter::IoWrite);
        let snap = match self.source {
            Some(f) => f(),
            None => shared_snapshot(want_io),
        };
        self.value_from(&snap)
    }

    fn string_value(&self) -> Option<String> {
        Some(self.current.clone())
    }

    fn default_max(&self) -> f64 {
        self.max_default
    }

    fn auto_max(&self) -> bool {
        true
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::sysmon::{NetIf, ProcInfo};
    use crate::measures::testutil::{ctx_run, skin};

    fn snap() -> Snapshot {
        let p = |pid: u32, name: &str, cpu: f64, rss: f64| ProcInfo { pid, name: name.into(), cpu, rss, private: rss / 2.0, ..Default::default() };
        Snapshot {
            procs: vec![p(1, "firefox", 20.0, 900.0), p(2, "firefox", 10.0, 100.0), p(3, "bash", 1.0, 50.0), p(4, "idle", 0.0, 0.0)],
            cpu_total: 40.0,
            cores: vec![30.0, 50.0],
            mem_total: 1000.0,
            mem_available: 250.0,
            net: vec![NetIf { name: "eth0".into(), rx_bps: 300.0, tx_bps: 100.0 }, NetIf { name: "lo".into(), rx_bps: 5.0, tx_bps: 5.0 }],
            disk_read_bps: 10.0,
            disk_write_bps: 20.0,
            disk_busy: 5.0,
            valid: true,
        }
    }

    fn run(ini: &str) -> (f64, String, f64) {
        let sk = skin(ini);
        let mut m = UsageMonitor::new();
        m.source = Some(snap);
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        let mut v = 0.0;
        ctx_run(&sk, |c| v = m.update(c));
        (v, m.string_value().unwrap(), m.default_max())
    }

    #[test]
    fn top_processes_by_cpu_roll_up_names() {
        let base = "[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=Process\nCounter=% Processor Time\nPercent=1\n";
        // firefox 30 % of the machine, then bash.
        assert_eq!(run(&format!("{base}Index=1\n")), (30.0, "firefox".into(), 100.0));
        assert_eq!(run(&format!("{base}Index=2\n")), (1.0, "bash".into(), 100.0));
        assert_eq!(run(&format!("{base}Index=3\n")).1, "");
        assert_eq!(run(&format!("{base}Name=Firefox.exe\n")).0, 30.0);
        assert_eq!(run(&format!("{base}Index=Sum\n")).0, 31.0);
        assert_eq!(run(&format!("{base}Index=Count\n")).0, 2.0);
    }

    #[test]
    fn aliases_and_memory() {
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nAlias=RAM\nIndex=1\n");
        assert_eq!((r.0, r.1.as_str()), (900.0 / 2.0 + 100.0 / 2.0, "firefox"));
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=Memory\nCounter=% Committed Bytes In Use\n");
        assert_eq!((r.0, r.2), (75.0, 100.0));
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=Memory\nCounter=Available MBytes\n");
        assert!(r.0 < 1.0);
    }

    #[test]
    fn processor_network_disk() {
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=Processor Information\nCounter=% Processor Time\nName=_Total\n");
        assert_eq!(r.0, 40.0);
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=Processor\nCounter=% Processor Time\nName=1\n");
        assert_eq!(r.0, 50.0);
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=Network Interface\nCounter=Bytes Received/sec\nName=eth0\n");
        assert_eq!(r.0, 300.0);
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=Network Interface\nCounter=Bytes Total/sec\nIndex=1\n");
        assert_eq!((r.0, r.1.as_str()), (400.0, "eth0"));
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=LogicalDisk\nCounter=Disk Write Bytes/sec\nName=_Total\n");
        assert_eq!(r.0, 20.0);
        let r = run("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=LogicalDisk\nCounter=% Free Space\nName=C:\n");
        assert!(r.0 >= 0.0 && r.0 <= 100.0);
    }

    #[test]
    fn unsupported_counter_is_zero_and_logged_once() {
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=UsageMonitor\nCategory=Bogus\nCounter=X\n");
        let mut m = UsageMonitor::new();
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        assert_eq!(m.take_log().len(), 1);
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        assert!(m.take_log().is_empty());
        ctx_run(&sk, |c| assert_eq!(m.update(c), 0.0));
    }
}
