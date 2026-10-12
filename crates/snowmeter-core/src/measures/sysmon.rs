//! Shared /proc sampler behind the Process and UsageMonitor plugins. One
//! snapshot is cached for a short time so many measures cost one scan.

use super::cpu::{parse_proc_stat, CpuTimes};
use super::memory::parse_meminfo;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Minimum age before the cached snapshot is replaced.
const MIN_INTERVAL: Duration = Duration::from_millis(900);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcInfo {
    pub pid: u32,
    /// `comm` (at most 15 chars on Linux) or the executable name when `comm` was cut off.
    pub name: String,
    /// Share of the whole machine, 0..100.
    pub cpu: f64,
    pub rss: f64,
    pub private: f64,
    pub io_read: f64,
    pub io_write: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetIf {
    pub name: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub procs: Vec<ProcInfo>,
    /// 0..100; index 0 of `cores` is core 0.
    pub cpu_total: f64,
    pub cores: Vec<f64>,
    pub mem_total: f64,
    pub mem_available: f64,
    pub net: Vec<NetIf>,
    pub disk_read_bps: f64,
    pub disk_write_bps: f64,
    /// 0..100 share of time the disks were busy.
    pub disk_busy: f64,
    pub valid: bool,
}

#[derive(Clone, Debug, Default)]
struct RawProc {
    name: String,
    ticks: u64,
    rss: f64,
    private: f64,
    io_read: u64,
    io_write: u64,
}

#[derive(Clone, Debug, Default)]
struct Raw {
    at: Option<Instant>,
    cpu: Vec<CpuTimes>,
    procs: HashMap<u32, RawProc>,
    net: HashMap<String, (u64, u64)>,
    disk: (u64, u64, u64),
}

/// `pid (comm) state ppid ... utime stime ...` -> (comm, utime + stime).
pub fn parse_pid_stat(text: &str) -> Option<(String, u64)> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let name = text[open + 1..close].to_string();
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    // rest[0] = state (field 3), utime = field 14 -> rest[11], stime -> rest[12].
    let ut: u64 = rest.get(11)?.parse().ok()?;
    let st: u64 = rest.get(12)?.parse().ok()?;
    Some((name, ut + st))
}

/// `/proc/net/dev` -> iface -> (rx bytes, tx bytes).
pub fn parse_net_dev(text: &str) -> HashMap<String, (u64, u64)> {
    let mut out = HashMap::new();
    for line in text.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let f: Vec<u64> = rest.split_whitespace().map(|x| x.parse().unwrap_or(0)).collect();
        if f.len() >= 9 {
            out.insert(name.trim().to_string(), (f[0], f[8]));
        }
    }
    out
}

fn is_whole_disk(name: &str) -> bool {
    let digits_end = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if let Some(r) = name.strip_prefix("nvme") {
        // nvme0n1 yes, nvme0n1p2 no.
        return r.split_once('n').is_some_and(|(a, b)| digits_end(a) && digits_end(b));
    }
    if let Some(r) = name.strip_prefix("mmcblk") {
        return digits_end(r);
    }
    for p in ["sd", "vd", "xvd", "hd"] {
        if let Some(r) = name.strip_prefix(p) {
            return !r.is_empty() && r.chars().all(|c| c.is_ascii_lowercase());
        }
    }
    false
}

/// `/proc/diskstats` -> (sectors read, sectors written, ms doing I/O) over whole disks.
pub fn parse_diskstats(text: &str) -> (u64, u64, u64) {
    let mut t = (0, 0, 0);
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 13 || !is_whole_disk(f[2]) {
            continue;
        }
        let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
        t.0 += n(5);
        t.1 += n(9);
        t.2 += n(12);
    }
    t
}

fn read_proc(root: &Path, want_io: bool) -> HashMap<u32, RawProc> {
    let mut out = HashMap::new();
    let Ok(rd) = std::fs::read_dir(root) else { return out };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let dir = e.path();
        let Some((mut name, ticks)) = std::fs::read_to_string(dir.join("stat")).ok().and_then(|t| parse_pid_stat(&t)) else {
            continue;
        };
        if name.len() >= 15 {
            if let Some(exe) = std::fs::read_to_string(dir.join("cmdline"))
                .ok()
                .and_then(|c| c.split('\0').next().map(str::to_string))
                .and_then(|a| a.rsplit(['/', '\\']).next().map(str::to_string))
                .filter(|s| !s.is_empty() && s.to_ascii_lowercase().starts_with(&name.to_ascii_lowercase()))
            {
                name = exe;
            }
        }
        let (mut rss, mut private) = (0.0, 0.0);
        if let Some(t) = std::fs::read_to_string(dir.join("statm")).ok() {
            let f: Vec<f64> = t.split_whitespace().map(|x| x.parse().unwrap_or(0.0)).collect();
            if f.len() >= 3 {
                rss = f[1] * 4096.0;
                private = (f[1] - f[2]).max(0.0) * 4096.0;
            }
        }
        let (mut io_read, mut io_write) = (0, 0);
        if want_io {
            if let Ok(t) = std::fs::read_to_string(dir.join("io")) {
                for l in t.lines() {
                    if let Some(v) = l.strip_prefix("rchar:") {
                        io_read = v.trim().parse().unwrap_or(0);
                    } else if let Some(v) = l.strip_prefix("wchar:") {
                        io_write = v.trim().parse().unwrap_or(0);
                    }
                }
            }
        }
        out.insert(pid, RawProc { name, ticks, rss, private, io_read, io_write });
    }
    out
}

fn read_raw(root: &Path, want_io: bool) -> Raw {
    let rd = |p: &str| std::fs::read_to_string(root.join(p)).unwrap_or_default();
    Raw {
        at: Some(Instant::now()),
        cpu: parse_proc_stat(&rd("stat")),
        procs: read_proc(root, want_io),
        net: parse_net_dev(&rd("net/dev")),
        disk: parse_diskstats(&rd("diskstats")),
    }
}

fn ticks_per_sec() -> f64 {
    // SAFETY: sysconf has no preconditions.
    let t = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if t > 0 {
        t as f64
    } else {
        100.0
    }
}

fn busy(prev: CpuTimes, cur: CpuTimes) -> f64 {
    let total = cur.total.saturating_sub(prev.total) as f64;
    if total <= 0.0 {
        return 0.0;
    }
    let idle = cur.idle.saturating_sub(prev.idle) as f64;
    ((total - idle) / total * 100.0).clamp(0.0, 100.0)
}

fn derive(prev: &Raw, cur: &Raw, mem: (f64, f64), tps: f64) -> Snapshot {
    let dt = match (prev.at, cur.at) {
        (Some(a), Some(b)) => b.saturating_duration_since(a).as_secs_f64(),
        _ => 0.0,
    };
    let mut s = Snapshot { mem_total: mem.0, mem_available: mem.1, valid: true, ..Default::default() };
    let ncpu = cur.cpu.len().saturating_sub(1).max(1) as f64;
    if prev.cpu.len() == cur.cpu.len() && !cur.cpu.is_empty() {
        s.cpu_total = busy(prev.cpu[0], cur.cpu[0]);
        s.cores = (1..cur.cpu.len()).map(|i| busy(prev.cpu[i], cur.cpu[i])).collect();
    }
    for (pid, p) in &cur.procs {
        let cpu = match prev.procs.get(pid) {
            Some(o) if dt > 0.0 && p.ticks >= o.ticks => (p.ticks - o.ticks) as f64 / tps / dt / ncpu * 100.0,
            _ => 0.0,
        };
        let rate = |a: u64, b: u64| if dt > 0.0 && a >= b { (a - b) as f64 / dt } else { 0.0 };
        let o = prev.procs.get(pid);
        s.procs.push(ProcInfo {
            pid: *pid,
            name: p.name.clone(),
            cpu: cpu.clamp(0.0, 100.0),
            rss: p.rss,
            private: p.private,
            io_read: o.map(|o| rate(p.io_read, o.io_read)).unwrap_or(0.0),
            io_write: o.map(|o| rate(p.io_write, o.io_write)).unwrap_or(0.0),
        });
    }
    s.procs.sort_by_key(|p| p.pid);
    for (name, (rx, tx)) in &cur.net {
        let (prx, ptx) = prev.net.get(name).copied().unwrap_or((*rx, *tx));
        let rate = |a: u64, b: u64| if dt > 0.0 && a >= b { (a - b) as f64 / dt } else { 0.0 };
        s.net.push(NetIf { name: name.clone(), rx_bps: rate(*rx, prx), tx_bps: rate(*tx, ptx) });
    }
    s.net.sort_by(|a, b| a.name.cmp(&b.name));
    if dt > 0.0 {
        let d = |a: u64, b: u64| a.saturating_sub(b) as f64;
        s.disk_read_bps = d(cur.disk.0, prev.disk.0) * 512.0 / dt;
        s.disk_write_bps = d(cur.disk.1, prev.disk.1) * 512.0 / dt;
        s.disk_busy = (d(cur.disk.2, prev.disk.2) / (dt * 1000.0) * 100.0).clamp(0.0, 100.0);
    }
    s
}

/// Sampler over a `/proc`-shaped directory (the real one or a test fixture).
pub struct Sampler {
    root: PathBuf,
    prev: Raw,
    cur: Raw,
    snap: Snapshot,
    last: Option<Instant>,
    want_io: bool,
}

impl Sampler {
    pub fn new(root: &Path) -> Sampler {
        Sampler { root: root.to_path_buf(), prev: Raw::default(), cur: Raw::default(), snap: Snapshot::default(), last: None, want_io: false }
    }

    /// Take a new sample unless the cached one is fresh; always samples at least once.
    pub fn snapshot(&mut self, want_io: bool, min_interval: Duration) -> Snapshot {
        self.want_io |= want_io;
        let stale = self.last.map(|t| t.elapsed() >= min_interval).unwrap_or(true);
        if stale {
            self.sample();
        }
        self.snap.clone()
    }

    fn sample(&mut self) {
        let first = self.last.is_none();
        let raw = read_raw(&self.root, self.want_io);
        self.prev = std::mem::replace(&mut self.cur, raw);
        if first {
            // Prime with a second sample shortly after so the first answer has rates.
            std::thread::sleep(Duration::from_millis(60));
            self.prev = std::mem::replace(&mut self.cur, read_raw(&self.root, self.want_io));
        }
        let mi = parse_meminfo(&std::fs::read_to_string(self.root.join("meminfo")).unwrap_or_default());
        self.snap = derive(&self.prev, &self.cur, (mi.total, mi.available), ticks_per_sec());
        self.last = Some(Instant::now());
    }

    /// Test hook: feed two raw samples `dt` apart.
    #[cfg(test)]
    fn derive_for_test(&mut self, dt: Duration) -> Snapshot {
        self.prev = std::mem::replace(&mut self.cur, read_raw(&self.root, true));
        if let (Some(p), Some(c)) = (self.prev.at, self.cur.at.as_mut()) {
            *c = p + dt;
        }
        derive(&self.prev, &self.cur, (0.0, 0.0), 100.0)
    }
}

static SHARED: Mutex<Option<Sampler>> = Mutex::new(None);

/// Snapshot of the real `/proc`, cached for about a second.
pub fn shared_snapshot(want_io: bool) -> Snapshot {
    let Ok(mut g) = SHARED.lock() else { return Snapshot::default() };
    g.get_or_insert_with(|| Sampler::new(Path::new("/proc"))).snapshot(want_io, MIN_INTERVAL)
}

/// Executable-style name comparison used by the Process plugin: case-insensitive,
/// with or without `.exe`.
pub fn same_process_name(running: &str, wanted: &str) -> bool {
    let norm = |s: &str| {
        let l = s.trim().to_ascii_lowercase();
        l.strip_suffix(".exe").map(str::to_string).unwrap_or(l)
    };
    let (a, w) = (norm(running), norm(wanted));
    if a.is_empty() || w.is_empty() {
        return false;
    }
    // comm is cut at 15 chars: a longer wanted name still matches its prefix.
    a == w || (a.len() >= 15 && w.starts_with(&a)) || (w.len() >= 15 && a.starts_with(&w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_line_with_awkward_comm() {
        let t = "123 (my (prog)) S 1 123 123 0 -1 4194560 100 0 0 0 40 2 0 0 20 0 1 0 5 1000 50 18446744073709551615";
        assert_eq!(parse_pid_stat(t), Some(("my (prog)".to_string(), 42)));
        assert_eq!(parse_pid_stat("garbage"), None);
        assert_eq!(parse_pid_stat("1 (x) S 1"), None);
    }

    #[test]
    fn net_and_disk_parsers() {
        let net = "Inter-|   Receive\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n";
        assert_eq!(parse_net_dev(net).get("eth0"), Some(&(1000, 2000)));
        let ds = "   8       0 sda 100 0 2000 50 30 0 800 20 0 70 80\n   8       1 sda1 100 0 1000 50 30 0 400 20 0 70 80\n 259 0 nvme0n1 5 0 10 1 5 0 20 1 0 9 10\n 259 1 nvme0n1p1 5 0 10 1 5 0 20 1 0 9 10\n";
        assert_eq!(parse_diskstats(ds), (2010, 820, 79));
        assert!(is_whole_disk("mmcblk0") && !is_whole_disk("mmcblk0p1") && !is_whole_disk("loop0") && !is_whole_disk("dm-0"));
    }

    #[test]
    fn names_match_like_windows() {
        assert!(same_process_name("firefox", "Firefox.exe"));
        assert!(same_process_name("Firefox.exe", "firefox"));
        assert!(!same_process_name("firefox", "firefox-bin"));
        assert!(same_process_name("gnome-shell-cal", "gnome-shell-calendar-server.exe"));
        assert!(!same_process_name("", ""));
    }

    #[test]
    fn real_proc_has_this_process() {
        let mut s = Sampler::new(Path::new("/proc"));
        let snap = s.snapshot(false, Duration::ZERO);
        assert!(snap.valid && snap.mem_total > 0.0);
        let me = std::process::id();
        let p = snap.procs.iter().find(|p| p.pid == me).expect("self");
        assert!(p.rss > 0.0);
    }

    #[test]
    fn rates_from_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("net")).unwrap();
        std::fs::create_dir_all(r.join("7")).unwrap();
        let w = |p: &str, t: &str| std::fs::write(r.join(p), t).unwrap();
        w("stat", "cpu 100 0 100 800 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0\n");
        w("meminfo", "MemTotal: 1000 kB\nMemAvailable: 400 kB\n");
        w("net/dev", "h\nh\n eth0: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
        w("diskstats", "");
        w("7/stat", "7 (worker) S 1 7 7 0 -1 0 0 0 0 0 0 0 0 20 0 1 0 5 1000 50 0");
        w("7/statm", "100 50 10 0 0 0 0");
        let mut s = Sampler::new(r);
        s.prev = read_raw(r, true);
        s.cur = s.prev.clone();
        // Second sample: 100 more ticks for pid 7 over 1 s on 2 cores = 50 %.
        w("stat", "cpu 200 0 200 1000 0 0 0 0\ncpu0 100 0 100 500 0 0 0 0\ncpu1 100 0 100 500 0 0 0 0\n");
        w("net/dev", "h\nh\n eth0: 5000 0 0 0 0 0 0 0 3000 0 0 0 0 0 0 0\n");
        w("7/stat", "7 (worker) S 1 7 7 0 -1 0 0 0 0 0 80 20 0 20 0 1 0 5 1000 50 0");
        let snap = s.derive_for_test(Duration::from_secs(1));
        assert_eq!(snap.cores.len(), 2);
        assert!((snap.cpu_total - 50.0).abs() < 0.01, "{}", snap.cpu_total);
        let p = snap.procs.iter().find(|p| p.pid == 7).unwrap();
        assert_eq!(p.name, "worker");
        assert!((p.cpu - 50.0).abs() < 0.01, "{}", p.cpu);
        assert_eq!(p.rss, 50.0 * 4096.0);
        assert_eq!(p.private, 40.0 * 4096.0);
        assert_eq!(snap.net[0], NetIf { name: "eth0".into(), rx_bps: 5000.0, tx_bps: 3000.0 });
    }
}
