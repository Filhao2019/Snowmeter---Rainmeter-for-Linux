//! ActiveNet (SilverAzide's helper plugin used by the Gadgets suites): network
//! adapter facts, CPU topology and battery details, all from `/sys` and `/proc`.
//!
//! * `NetAdapterType` (value = .NET `NetworkInterfaceType` id, string = its
//!   name), `NetInterfaceName`, `NetInterfaceID` (the Linux ifindex, the same
//!   number `Interface=N` takes in the Net measures), `NetStatus` (RFC 2863
//!   `operstate`: 1 up, 2 down, 3 testing, 4 unknown or no such adapter, 5
//!   dormant, 6 not present, 7 lower layer down), `NetSpeed` (bit/s, 0 when the
//!   driver does not report it). `Name` is `Best` (default route), an adapter
//!   name or an ifindex.
//! * `CpuProcessors`, `CpuPhysicalCores`, `CpuLogicalCores` (per CPU),
//!   `CpuNumaNodes`, `CpuGroups`, `CpuLogicalCoreMap`, `ThreadsPerCore` from
//!   `/proc/cpuinfo` and `/sys/devices/system/node`.
//! * `BatInstanceName`, `BatDeviceName`, `BatVoltage` (V), `BatChargeRate` (W,
//!   negative while discharging), `BatRemainingCapacity` (Wh); `Name` is the
//!   `power_supply` entry (default: the first battery).

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Roots {
    pub net: PathBuf,
    pub route: PathBuf,
    pub power: PathBuf,
    pub cpuinfo: PathBuf,
    pub numa: PathBuf,
}

impl Default for Roots {
    fn default() -> Self {
        Roots {
            net: "/sys/class/net".into(),
            route: "/proc/net/route".into(),
            power: "/sys/class/power_supply".into(),
            cpuinfo: "/proc/cpuinfo".into(),
            numa: "/sys/devices/system/node".into(),
        }
    }
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_f64(p: &Path) -> Option<f64> {
    read_trim(p)?.parse().ok()
}

// ---- network ----------------------------------------------------------------

/// The adapter named by `Name`: `Best`/empty, an ifindex, or a name.
pub fn find_adapter(r: &Roots, name: &str) -> Option<String> {
    let n = name.trim();
    let mut names: Vec<String> = std::fs::read_dir(&r.net).ok()?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    if n.is_empty() || n.eq_ignore_ascii_case("best") {
        let route = std::fs::read_to_string(&r.route).unwrap_or_default();
        if let Some(b) = super::net::parse_route_best(&route).filter(|b| names.contains(b)) {
            return Some(b);
        }
        return names
            .into_iter()
            .find(|a| a != "lo" && read_trim(&r.net.join(a).join("operstate")).as_deref() == Some("up"));
    }
    if let Ok(idx) = n.parse::<u32>() {
        return names.into_iter().find(|a| read_f64(&r.net.join(a).join("ifindex")) == Some(f64::from(idx)));
    }
    names.into_iter().find(|a| a.eq_ignore_ascii_case(n))
}

/// `(.NET NetworkInterfaceType id, name)` of an adapter.
pub fn adapter_type(r: &Roots, a: &str) -> (u32, &'static str) {
    let d = r.net.join(a);
    if d.join("wireless").exists() || d.join("phy80211").exists() {
        return (71, "Wireless80211");
    }
    match read_f64(&d.join("type")).map(|t| t as u32) {
        Some(772) => (24, "Loopback"),
        Some(1) => (6, "Ethernet"),
        Some(512) => (23, "Ppp"),
        Some(768) | Some(776) | Some(778) | Some(65534) => (131, "Tunnel"),
        Some(_) => (1, "Unknown"),
        None => (1, "Unknown"),
    }
}

/// `operstate` as the RFC 2863 number; 4 (unknown) for anything else.
pub fn oper_status(state: &str) -> u32 {
    match state.trim().to_ascii_lowercase().as_str() {
        "up" => 1,
        "down" => 2,
        "testing" => 3,
        "dormant" => 5,
        "notpresent" => 6,
        "lowerlayerdown" => 7,
        _ => 4,
    }
}

// ---- cpu ----------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Topology {
    /// `(package, core)` of each logical processor in `processor` order.
    pub logical: Vec<(u32, u32)>,
}

pub fn parse_topology(cpuinfo: &str) -> Topology {
    let mut t = Topology::default();
    for block in cpuinfo.split("\n\n") {
        let mut has = false;
        let (mut pkg, mut core) = (0u32, None);
        for l in block.lines() {
            let Some((k, v)) = l.split_once(':') else { continue };
            let v = v.trim();
            match k.trim() {
                "processor" => has = true,
                "physical id" => pkg = v.parse().unwrap_or(0),
                "core id" => core = v.parse().ok(),
                _ => {}
            }
        }
        if has {
            // Without `core id` (ARM) every logical processor is its own core.
            let c = core.unwrap_or(t.logical.len() as u32);
            t.logical.push((pkg, c));
        }
    }
    t
}

impl Topology {
    fn distinct_cores(&self) -> Vec<(u32, u32)> {
        let mut v: Vec<(u32, u32)> = Vec::new();
        for l in &self.logical {
            if !v.contains(l) {
                v.push(*l);
            }
        }
        v
    }

    pub fn sockets(&self) -> usize {
        let mut p: Vec<u32> = self.logical.iter().map(|l| l.0).collect();
        p.sort_unstable();
        p.dedup();
        p.len().max(1)
    }

    pub fn physical_per_cpu(&self) -> usize {
        (self.distinct_cores().len() / self.sockets()).max(1)
    }

    pub fn logical_per_cpu(&self) -> usize {
        (self.logical.len() / self.sockets()).max(1)
    }

    /// Physical core number of every logical processor, numbered by first appearance.
    pub fn core_map(&self) -> Vec<usize> {
        let cores = self.distinct_cores();
        self.logical.iter().map(|l| cores.iter().position(|c| c == l).unwrap_or(0)).collect()
    }

    /// Logical per physical core, or -1 when the CPU is hybrid (uneven).
    pub fn threads_per_core(&self) -> f64 {
        let map = self.core_map();
        let cores = self.distinct_cores().len();
        if cores == 0 {
            return 0.0;
        }
        let first = map.iter().filter(|c| **c == 0).count();
        if (0..cores).all(|c| map.iter().filter(|x| **x == c).count() == first) {
            first as f64
        } else {
            -1.0
        }
    }
}

pub fn numa_nodes(dir: &Path, sockets: usize) -> usize {
    let n = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.strip_prefix("node").is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
                })
                .count()
        })
        .unwrap_or(0);
    if n == 0 {
        sockets
    } else {
        n
    }
}

// ---- battery ------------------------------------------------------------------

/// System batteries (not peripherals), sorted by name.
pub fn batteries(power: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(power) else { return Vec::new() };
    let mut v: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| {
            let d = e.path();
            read_trim(&d.join("type")).is_some_and(|t| t.eq_ignore_ascii_case("battery"))
                && read_trim(&d.join("scope")).is_none_or(|s| !s.eq_ignore_ascii_case("device"))
                && read_f64(&d.join("present")).is_none_or(|p| p > 0.0)
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    NetAdapterType,
    NetInterfaceName,
    NetInterfaceId,
    NetStatus,
    NetSpeed,
    CpuProcessors,
    CpuPhysicalCores,
    CpuLogicalCores,
    CpuNumaNodes,
    CpuGroups,
    CpuLogicalCoreMap,
    ThreadsPerCore,
    BatInstanceName,
    BatDeviceName,
    BatVoltage,
    BatChargeRate,
    BatRemainingCapacity,
    Unknown,
}

fn parse_ty(s: &str) -> Ty {
    match s.trim().to_ascii_lowercase().as_str() {
        "netadaptertype" | "adaptertype" | "netadaptertypeid" | "adaptertypeid" => Ty::NetAdapterType,
        "netinterfacename" | "interfacename" => Ty::NetInterfaceName,
        "netinterfaceid" | "interfaceid" => Ty::NetInterfaceId,
        "netstatus" => Ty::NetStatus,
        "netspeed" => Ty::NetSpeed,
        "cpuprocessors" | "processors" => Ty::CpuProcessors,
        "cpuphysicalcores" | "physicalcores" => Ty::CpuPhysicalCores,
        "cpulogicalcores" => Ty::CpuLogicalCores,
        "cpunumanodes" => Ty::CpuNumaNodes,
        "cpugroups" => Ty::CpuGroups,
        "cpulogicalcoremap" => Ty::CpuLogicalCoreMap,
        "cputhreadspercore" | "threadspercore" => Ty::ThreadsPerCore,
        "batinstancename" => Ty::BatInstanceName,
        "batdevicename" => Ty::BatDeviceName,
        "batvoltage" => Ty::BatVoltage,
        "batchargerate" => Ty::BatChargeRate,
        "batremainingcapacity" => Ty::BatRemainingCapacity,
        _ => Ty::Unknown,
    }
}

pub struct ActiveNet {
    ty: Ty,
    name: String,
    roots: Roots,
    string: Option<String>,
    log: Vec<String>,
}

impl ActiveNet {
    pub fn new() -> ActiveNet {
        ActiveNet::with_roots(Roots::default())
    }

    pub fn with_roots(roots: Roots) -> ActiveNet {
        ActiveNet { ty: Ty::Unknown, name: String::new(), roots, string: None, log: Vec::new() }
    }

    fn battery(&self) -> Option<PathBuf> {
        let list = batteries(&self.roots.power);
        let n = self.name.trim();
        let pick = if n.is_empty() { list.first().cloned() } else { list.into_iter().find(|b| b.eq_ignore_ascii_case(n)) };
        pick.map(|b| self.roots.power.join(b))
    }
}

impl Default for ActiveNet {
    fn default() -> Self {
        ActiveNet::new()
    }
}

impl MeasureImpl for ActiveNet {
    fn read_options(&mut self, opt: &OptionReader) {
        let t = opt.string("Type", "");
        self.ty = parse_ty(&t);
        if self.ty == Ty::Unknown {
            self.log.push(format!("ActiveNet: Type={t} is not valid"));
        }
        self.name = opt.string("Name", "");
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.string = None;
        let r = &self.roots;
        let cpu = || parse_topology(&std::fs::read_to_string(&r.cpuinfo).unwrap_or_default());
        match self.ty {
            Ty::Unknown => 0.0,
            Ty::NetAdapterType | Ty::NetInterfaceName | Ty::NetInterfaceId | Ty::NetStatus | Ty::NetSpeed => {
                let Some(a) = find_adapter(r, &self.name) else {
                    if self.ty == Ty::NetStatus {
                        return 4.0;
                    }
                    if matches!(self.ty, Ty::NetAdapterType | Ty::NetInterfaceName) {
                        self.string = Some(String::new());
                    }
                    return 0.0;
                };
                let d = r.net.join(&a);
                match self.ty {
                    Ty::NetAdapterType => {
                        let (id, name) = adapter_type(r, &a);
                        self.string = Some(name.to_string());
                        f64::from(id)
                    }
                    Ty::NetInterfaceName => {
                        self.string = Some(a);
                        0.0
                    }
                    Ty::NetInterfaceId => read_f64(&d.join("ifindex")).unwrap_or(0.0),
                    Ty::NetStatus => f64::from(oper_status(&read_trim(&d.join("operstate")).unwrap_or_default())),
                    _ => read_f64(&d.join("speed")).filter(|s| *s > 0.0).map(|s| s * 1_000_000.0).unwrap_or(0.0),
                }
            }
            Ty::CpuProcessors => cpu().sockets() as f64,
            Ty::CpuPhysicalCores => cpu().physical_per_cpu() as f64,
            Ty::CpuLogicalCores => cpu().logical_per_cpu() as f64,
            Ty::CpuNumaNodes => numa_nodes(&r.numa, cpu().sockets()) as f64,
            Ty::CpuGroups => (cpu().logical.len().div_ceil(64)).max(1) as f64,
            Ty::ThreadsPerCore => cpu().threads_per_core(),
            Ty::CpuLogicalCoreMap => {
                let map = cpu().core_map();
                self.string = Some(map.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","));
                map.len() as f64
            }
            Ty::BatInstanceName | Ty::BatDeviceName => {
                let b = self.battery();
                self.string = Some(match (&b, self.ty) {
                    (Some(d), Ty::BatDeviceName) => {
                        read_trim(&d.join("model_name")).filter(|m| !m.is_empty()).unwrap_or_else(|| d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                    }
                    (Some(d), _) => d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    _ => String::new(),
                });
                0.0
            }
            Ty::BatVoltage | Ty::BatChargeRate | Ty::BatRemainingCapacity => {
                let Some(d) = self.battery() else { return 0.0 };
                let volts = read_f64(&d.join("voltage_now")).map(|v| v / 1e6);
                match self.ty {
                    Ty::BatVoltage => volts.unwrap_or(0.0),
                    Ty::BatChargeRate => {
                        let w = read_f64(&d.join("power_now")).map(|p| p / 1e6).or_else(|| Some(read_f64(&d.join("current_now"))? / 1e6 * volts?)).unwrap_or(0.0);
                        if read_trim(&d.join("status")).is_some_and(|s| s.eq_ignore_ascii_case("discharging")) {
                            -w.abs()
                        } else {
                            w.abs()
                        }
                    }
                    _ => read_f64(&d.join("energy_now"))
                        .map(|e| e / 1e6)
                        .or_else(|| Some(read_f64(&d.join("charge_now"))? / 1e6 * volts?))
                        .unwrap_or(0.0),
                }
            }
        }
    }

    fn string_value(&self) -> Option<String> {
        self.string.clone()
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::hwmon::fixtures::put;
    use crate::measures::testutil::{ctx_run, skin};

    /// eth0 (up, best), wlan0 (down, wireless), lo, docker0 (unknown); 2 sockets x 2 cores x 2 threads.
    fn tree() -> (tempfile::TempDir, Roots) {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        for (n, idx, ty, st) in [("eth0", 2, 1, "up"), ("wlan0", 3, 1, "down"), ("lo", 1, 772, "unknown"), ("docker0", 4, 1, "unknown")] {
            put(r, &format!("net/{n}/ifindex"), &format!("{idx}\n"));
            put(r, &format!("net/{n}/type"), &format!("{ty}\n"));
            put(r, &format!("net/{n}/operstate"), &format!("{st}\n"));
        }
        put(r, "net/eth0/speed", "1000\n");
        put(r, "net/wlan0/speed", "-1\n");
        put(r, "net/wlan0/wireless/link", "0\n");
        put(r, "route", "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\nwlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\neth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\n");
        let mut ci = String::new();
        for p in 0..8 {
            // Linux order: threads of one core are 4 apart.
            let (pkg, core) = ((p % 4) / 2, p % 2);
            ci.push_str(&format!("processor\t: {p}\nmodel name\t: X\nphysical id\t: {pkg}\ncore id\t\t: {core}\nsiblings\t: 4\n\n"));
        }
        put(r, "cpuinfo", &ci);
        put(r, "node/node0/x", "");
        put(r, "node/node1/x", "");
        put(r, "node/possible", "0-1\n");
        put(r, "ps/BAT0/type", "Battery\n");
        put(r, "ps/BAT0/status", "Discharging\n");
        put(r, "ps/BAT0/model_name", "DELL 1234\n");
        put(r, "ps/BAT0/voltage_now", "11400000\n");
        put(r, "ps/BAT0/power_now", "9120000\n");
        put(r, "ps/BAT0/energy_now", "40500000\n");
        put(r, "ps/BAT1/type", "Battery\n");
        put(r, "ps/BAT1/status", "Charging\n");
        put(r, "ps/BAT1/voltage_now", "7000000\n");
        put(r, "ps/BAT1/current_now", "2000000\n");
        put(r, "ps/BAT1/charge_now", "3000000\n");
        put(r, "ps/hidpp_battery_0/type", "Battery\n");
        put(r, "ps/hidpp_battery_0/scope", "Device\n");
        put(r, "ps/AC/type", "Mains\n");
        let roots = Roots { net: r.join("net"), route: r.join("route"), power: r.join("ps"), cpuinfo: r.join("cpuinfo"), numa: r.join("node") };
        (t, roots)
    }

    fn eval(roots: &Roots, ini: &str) -> (f64, Option<String>) {
        let sk = skin(&format!("[M]\nMeasure=Plugin\nPlugin=ActiveNet\n{ini}\n"));
        let mut m = ActiveNet::with_roots(roots.clone());
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        let v = ctx_run(&sk, |c| m.update(c));
        (v, m.string_value())
    }

    #[test]
    fn adapter_selection() {
        let (_t, r) = tree();
        assert_eq!(find_adapter(&r, "Best").as_deref(), Some("eth0"));
        assert_eq!(find_adapter(&r, "").as_deref(), Some("eth0"));
        assert_eq!(find_adapter(&r, "WLAN0").as_deref(), Some("wlan0"));
        assert_eq!(find_adapter(&r, "3").as_deref(), Some("wlan0"));
        assert_eq!(find_adapter(&r, "Realtek PCIe GbE"), None);
    }

    #[test]
    fn network_types() {
        let (_t, r) = tree();
        assert_eq!(eval(&r, "Type=NetAdapterType\nName=Best"), (6.0, Some("Ethernet".into())));
        assert_eq!(eval(&r, "Type=NetAdapterType\nName=wlan0"), (71.0, Some("Wireless80211".into())));
        assert_eq!(eval(&r, "Type=NetAdapterType\nName=lo"), (24.0, Some("Loopback".into())));
        assert_eq!(eval(&r, "Type=NetInterfaceName\nName=Best").1.as_deref(), Some("eth0"));
        assert_eq!(eval(&r, "Type=NetInterfaceID\nName=Best"), (2.0, None));
        assert_eq!(eval(&r, "Type=NetStatus\nName=eth0").0, 1.0);
        assert_eq!(eval(&r, "Type=NetStatus\nName=wlan0").0, 2.0);
        assert_eq!(eval(&r, "Type=NetStatus\nName=docker0").0, 4.0);
        assert_eq!(eval(&r, "Type=NetStatus\nName=nothing").0, 4.0);
        assert_eq!(eval(&r, "Type=NetSpeed\nName=eth0").0, 1e9);
        assert_eq!(eval(&r, "Type=NetSpeed\nName=wlan0").0, 0.0);
        assert_eq!(eval(&r, "Type=NetInterfaceName\nName=nothing"), (0.0, Some(String::new())));
        assert_eq!(oper_status("lowerlayerdown"), 7);
    }

    #[test]
    fn cpu_types() {
        let (_t, r) = tree();
        assert_eq!(eval(&r, "Type=CpuProcessors").0, 2.0);
        assert_eq!(eval(&r, "Type=CpuPhysicalCores").0, 2.0);
        assert_eq!(eval(&r, "Type=CpuLogicalCores").0, 4.0);
        assert_eq!(eval(&r, "Type=CpuNumaNodes").0, 2.0);
        assert_eq!(eval(&r, "Type=CpuGroups").0, 1.0);
        assert_eq!(eval(&r, "Type=ThreadsPerCore").0, 2.0);
        assert_eq!(eval(&r, "Type=CpuLogicalCoreMap"), (8.0, Some("0,1,2,3,0,1,2,3".into())));
        // The Gadgets formula: sockets * logical per cpu / numa nodes.
        let total = 2.0 * 4.0 / 2.0;
        assert_eq!(total, 4.0);
        // No NUMA info falls back to the socket count; no cpuinfo gives safe values.
        let mut r2 = r.clone();
        r2.numa = "/nonexistent".into();
        assert_eq!(eval(&r2, "Type=CpuNumaNodes").0, 2.0);
        r2.cpuinfo = "/nonexistent".into();
        assert_eq!(eval(&r2, "Type=CpuProcessors").0, 1.0);
        assert_eq!(eval(&r2, "Type=ThreadsPerCore").0, 0.0);
        assert_eq!(eval(&r2, "Type=CpuGroups").0, 1.0);
    }

    #[test]
    fn hybrid_cpu_has_no_thread_ratio() {
        let t = parse_topology("processor : 0\ncore id : 0\n\nprocessor : 1\ncore id : 0\n\nprocessor : 2\ncore id : 1\n\n");
        assert_eq!(t.threads_per_core(), -1.0);
        let arm = parse_topology("processor : 0\n\nprocessor : 1\n\n");
        assert_eq!((arm.sockets(), arm.physical_per_cpu(), arm.threads_per_core()), (1, 2, 1.0));
    }

    #[test]
    fn battery_types() {
        let (_t, r) = tree();
        assert_eq!(batteries(&r.power), ["BAT0", "BAT1"]);
        assert_eq!(eval(&r, "Type=BatInstanceName").1.as_deref(), Some("BAT0"));
        assert_eq!(eval(&r, "Type=BatDeviceName").1.as_deref(), Some("DELL 1234"));
        assert_eq!(eval(&r, "Type=BatDeviceName\nName=BAT1").1.as_deref(), Some("BAT1"));
        assert_eq!(eval(&r, "Type=BatVoltage").0, 11.4);
        assert_eq!(eval(&r, "Type=BatChargeRate").0, -9.12);
        assert_eq!(eval(&r, "Type=BatRemainingCapacity").0, 40.5);
        assert_eq!(eval(&r, "Type=BatVoltage\nName=BAT1").0, 7.0);
        assert_eq!(eval(&r, "Type=BatChargeRate\nName=BAT1").0, 14.0);
        assert_eq!(eval(&r, "Type=BatRemainingCapacity\nName=BAT1").0, 21.0);
        assert_eq!(eval(&r, "Type=BatVoltage\nName=NOPE").0, 0.0);
        let mut none = r.clone();
        none.power = "/nonexistent".into();
        assert_eq!(eval(&none, "Type=BatInstanceName"), (0.0, Some(String::new())));
    }

    #[test]
    fn unknown_type_logs() {
        let sk = skin("[M]\nMeasure=Plugin\nType=Bogus\n");
        let mut m = ActiveNet::new();
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        assert_eq!(m.take_log().len(), 1);
        assert_eq!(ctx_run(&sk, |c| m.update(c)), 0.0);
    }
}
