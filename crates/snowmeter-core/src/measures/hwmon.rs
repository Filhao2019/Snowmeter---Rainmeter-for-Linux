//! `/sys/class/hwmon` reader shared by the CoreTemp, SpeedFan and HWiNFO
//! emulations. Everything takes the root directory as an argument so the unit
//! tests can use canned trees.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Sensor {
    pub label: String,
    /// Celsius for temperatures, RPM for fans, volts for voltages.
    pub value: f64,
    /// `tempN_crit` (Celsius) when the driver exports one.
    pub crit: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Chip {
    pub name: String,
    pub temps: Vec<Sensor>,
    pub fans: Vec<Sensor>,
    pub volts: Vec<Sensor>,
}

/// Kind of reading, as used by SpeedFan and HWiNFO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Temp,
    Fan,
    Volt,
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_f64(p: &Path) -> Option<f64> {
    read_trim(p)?.parse().ok()
}

/// Drivers that describe the CPU package/cores; they sort first so that
/// "temperature 0" is the processor, as in SpeedFan.
pub fn is_cpu_chip(name: &str) -> bool {
    matches!(name, "coretemp" | "k10temp" | "zenpower" | "zenpower3" | "cpu_thermal" | "fam15h_power")
}

fn trailing_number(s: &str) -> u64 {
    let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse().unwrap_or(0)
}

/// Sensors `<prefix>N_input` of one chip directory, ordered by N.
fn collect(dir: &Path, prefix: &str, scale: f64) -> Vec<Sensor> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut nums: Vec<u64> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let rest = n.strip_prefix(prefix)?.strip_suffix("_input")?.to_string();
            rest.parse::<u64>().ok()
        })
        .collect();
    nums.sort_unstable();
    nums.dedup();
    let mut out = Vec::new();
    for n in nums {
        let Some(raw) = read_f64(&dir.join(format!("{prefix}{n}_input"))) else { continue };
        let label = read_trim(&dir.join(format!("{prefix}{n}_label"))).filter(|l| !l.is_empty()).unwrap_or_else(|| format!("{prefix}{n}"));
        let crit = read_f64(&dir.join(format!("{prefix}{n}_crit"))).map(|c| c / scale).filter(|c| *c > 0.0);
        out.push(Sensor { label, value: raw / scale, crit });
    }
    out
}

/// All hwmon chips, CPU drivers first, then by name, then by `hwmonN` number.
pub fn scan(root: &Path) -> Vec<Chip> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut dirs: Vec<(PathBuf, u64)> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .map(|p| {
            let n = trailing_number(&p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
            (p, n)
        })
        .collect();
    dirs.sort_by_key(|(_, n)| *n);
    let mut chips: Vec<Chip> = dirs
        .into_iter()
        .map(|(d, _)| Chip {
            name: read_trim(&d.join("name")).unwrap_or_default(),
            temps: collect(&d, "temp", 1000.0),
            fans: collect(&d, "fan", 1.0),
            volts: collect(&d, "in", 1000.0),
        })
        .collect();
    // Stable sort: CPU drivers first, the rest keeps (name, hwmonN) order.
    chips.sort_by(|a, b| (!is_cpu_chip(&a.name)).cmp(&!is_cpu_chip(&b.name)).then_with(|| a.name.cmp(&b.name)));
    chips
}

/// Flat list of one kind over all chips: `(chip name, sensor)`.
pub fn flat(chips: &[Chip], kind: Kind) -> Vec<(&str, &Sensor)> {
    chips
        .iter()
        .flat_map(|c| {
            let list = match kind {
                Kind::Temp => &c.temps,
                Kind::Fan => &c.fans,
                Kind::Volt => &c.volts,
            };
            list.iter().map(move |s| (c.name.as_str(), s))
        })
        .collect()
}

/// The CPU chip, if any driver for it is present.
pub fn cpu_chip(chips: &[Chip]) -> Option<&Chip> {
    chips.iter().find(|c| is_cpu_chip(&c.name) && !c.temps.is_empty())
}

/// Per-core temperatures of a CPU chip. `coretemp` exports `Core N`; AMD
/// drivers only export package-level values, which are used for every core.
pub fn core_temps(chip: &Chip) -> (Vec<f64>, bool) {
    let mut cores: Vec<(u64, f64)> = chip
        .temps
        .iter()
        .filter(|s| s.label.to_ascii_lowercase().starts_with("core "))
        .map(|s| (trailing_number(&s.label), s.value))
        .collect();
    cores.sort_by_key(|(n, _)| *n);
    if !cores.is_empty() {
        return (cores.into_iter().map(|(_, v)| v).collect(), true);
    }
    let pick = ["tdie", "tctl", "package", "temp1"]
        .iter()
        .find_map(|want| chip.temps.iter().find(|s| s.label.to_ascii_lowercase().starts_with(want)))
        .or_else(|| chip.temps.first());
    (pick.map(|s| vec![s.value]).unwrap_or_default(), false)
}

/// TjMax: the driver's `crit` when present, else a typical 100 C.
pub fn tj_max(chip: &Chip) -> f64 {
    chip.temps.iter().filter_map(|s| s.crit).fold(None, |a: Option<f64>, c| Some(a.map_or(c, |x| x.max(c)))).unwrap_or(100.0)
}

/// First voltage that looks like the CPU core voltage.
pub fn vcore(chips: &[Chip]) -> f64 {
    for c in chips {
        for s in &c.volts {
            let l = s.label.to_ascii_lowercase();
            if l.contains("vcore") || l.contains("vddcr") || l.contains("vid") {
                return s.value;
            }
        }
    }
    0.0
}

// ---- /proc/stat load ------------------------------------------------------

/// `(idle, total)` jiffies: element 0 is the whole CPU, then one per core.
pub fn parse_proc_stat(text: &str) -> Vec<(u64, u64)> {
    text.lines()
        .filter(|l| l.starts_with("cpu"))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            it.next()?;
            let v: Vec<u64> = it.map_while(|x| x.parse().ok()).collect();
            if v.len() < 4 {
                return None;
            }
            let idle = v[3] + v.get(4).copied().unwrap_or(0);
            // guest time is already counted in user/nice
            let total: u64 = v.iter().take(8).sum();
            Some((idle, total))
        })
        .collect()
}

/// Busy percentage between two samples of the same index (0 when nothing moved).
pub fn load_between(prev: Option<(u64, u64)>, cur: (u64, u64)) -> f64 {
    let (pi, pt) = prev.unwrap_or((0, 0));
    let dt = cur.1.saturating_sub(pt);
    if dt == 0 {
        return 0.0;
    }
    let di = cur.0.saturating_sub(pi).min(dt);
    (dt - di) as f64 * 100.0 / dt as f64
}

// ---- /proc/cpuinfo --------------------------------------------------------

pub fn cpuinfo_name(text: &str) -> String {
    text.lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// `cpu MHz` per logical processor.
pub fn cpuinfo_mhz(text: &str) -> Vec<f64> {
    text.lines()
        .filter(|l| l.starts_with("cpu MHz"))
        .filter_map(|l| l.split_once(':')?.1.trim().parse().ok())
        .collect()
}

/// Current clock of logical CPU `idx` in MHz (cpufreq first, cpuinfo second).
pub fn cpu_mhz(cpu_root: &Path, cpuinfo: &Path, idx: usize) -> f64 {
    if let Some(khz) = read_f64(&cpu_root.join(format!("cpu{idx}/cpufreq/scaling_cur_freq"))) {
        return khz / 1000.0;
    }
    std::fs::read_to_string(cpuinfo).ok().and_then(|t| cpuinfo_mhz(&t).get(idx).copied()).unwrap_or(0.0)
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::Path;

    pub fn put(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    /// hwmon0 nvme, hwmon1 coretemp (2 cores), hwmon2 nct6798 (fan, volts).
    pub fn intel_tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        put(r, "hwmon0/name", "nvme\n");
        put(r, "hwmon0/temp1_input", "41850\n");
        put(r, "hwmon0/temp1_label", "Composite\n");
        put(r, "hwmon1/name", "coretemp\n");
        put(r, "hwmon1/temp1_input", "50000\n");
        put(r, "hwmon1/temp1_label", "Package id 0\n");
        put(r, "hwmon1/temp1_crit", "100000\n");
        put(r, "hwmon1/temp2_input", "47000\n");
        put(r, "hwmon1/temp2_label", "Core 0\n");
        put(r, "hwmon1/temp3_input", "49000\n");
        put(r, "hwmon1/temp3_label", "Core 1\n");
        put(r, "hwmon2/name", "nct6798\n");
        put(r, "hwmon2/fan1_input", "1200\n");
        put(r, "hwmon2/fan2_input", "850\n");
        put(r, "hwmon2/in0_input", "1056\n");
        put(r, "hwmon2/in0_label", "Vcore\n");
        put(r, "hwmon2/in1_input", "12096\n");
        put(r, "hwmon2/temp1_input", "33000\n");
        put(r, "hwmon3/name", "hidpp_battery_0\n");
        t
    }

    pub fn amd_tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        put(r, "hwmon0/name", "nvme\n");
        put(r, "hwmon0/temp1_input", "30000\n");
        put(r, "hwmon1/name", "k10temp\n");
        put(r, "hwmon1/temp1_input", "62500\n");
        put(r, "hwmon1/temp1_label", "Tctl\n");
        put(r, "hwmon1/temp2_input", "60500\n");
        put(r, "hwmon1/temp2_label", "Tdie\n");
        t
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn scan_orders_cpu_first_then_by_name() {
        let t = intel_tree();
        let chips = scan(t.path());
        let names: Vec<&str> = chips.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["coretemp", "hidpp_battery_0", "nct6798", "nvme"]);
        let temps = flat(&chips, Kind::Temp);
        assert_eq!(temps.len(), 5);
        assert_eq!(temps[0].1.label, "Package id 0");
        assert_eq!(temps[0].1.value, 50.0);
        assert_eq!(temps[0].1.crit, Some(100.0));
        assert_eq!(temps[3].1.label, "temp1");
        assert_eq!(flat(&chips, Kind::Fan).iter().map(|f| f.1.value).collect::<Vec<_>>(), [1200.0, 850.0]);
        let v = flat(&chips, Kind::Volt);
        assert!((v[0].1.value - 1.056).abs() < 1e-9 && (v[1].1.value - 12.096).abs() < 1e-9);
        assert!((vcore(&chips) - 1.056).abs() < 1e-9);
    }

    #[test]
    fn core_temps_per_core_and_package_only() {
        let t = intel_tree();
        let chips = scan(t.path());
        let (c, per) = core_temps(cpu_chip(&chips).unwrap());
        assert!(per);
        assert_eq!(c, [47.0, 49.0]);
        assert_eq!(tj_max(cpu_chip(&chips).unwrap()), 100.0);
        let a = amd_tree();
        let chips = scan(a.path());
        let (c, per) = core_temps(cpu_chip(&chips).unwrap());
        assert!(!per);
        assert_eq!(c, [60.5]);
        assert_eq!(tj_max(cpu_chip(&chips).unwrap()), 100.0);
    }

    #[test]
    fn missing_root_is_empty() {
        assert!(scan(Path::new("/nonexistent/hwmon")).is_empty());
        assert!(cpu_chip(&[]).is_none());
        assert_eq!(vcore(&[]), 0.0);
    }

    #[test]
    fn proc_stat_load() {
        let a = "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0 0 0\nintr 1\n";
        let b = "cpu  150 0 150 900 0 0 0 0 0 0\ncpu0 100 0 100 400 0 0 0 0 0 0\ncpu1 50 0 50 500 0 0 0 0 0 0\n";
        let (sa, sb) = (parse_proc_stat(a), parse_proc_stat(b));
        assert_eq!(sa.len(), 3);
        assert_eq!(load_between(Some(sa[0]), sb[0]), 50.0);
        assert_eq!(load_between(Some(sa[1]), sb[1]), 100.0);
        assert_eq!(load_between(Some(sa[2]), sb[2]), 0.0);
        assert_eq!(load_between(Some(sa[2]), sa[2]), 0.0);
        assert!((load_between(None, sa[0]) - 20.0).abs() < 1e-9);
        assert!(parse_proc_stat("garbage\ncpu x\n").is_empty());
    }

    #[test]
    fn cpuinfo_and_cpufreq() {
        let info = "processor : 0\nmodel name\t: AMD  Ryzen 5   5600X\ncpu MHz\t\t: 3700.123\nprocessor : 1\ncpu MHz : 4200.0\n";
        assert_eq!(cpuinfo_name(info), "AMD Ryzen 5 5600X");
        assert_eq!(cpuinfo_mhz(info), [3700.123, 4200.0]);
        let t = tempfile::tempdir().unwrap();
        put(t.path(), "cpuinfo", info);
        put(t.path(), "cpu/cpu1/cpufreq/scaling_cur_freq", "2800000\n");
        let (root, ci) = (t.path().join("cpu"), t.path().join("cpuinfo"));
        assert_eq!(cpu_mhz(&root, &ci, 1), 2800.0);
        assert_eq!(cpu_mhz(&root, &ci, 0), 3700.123);
        assert_eq!(cpu_mhz(&root, &ci, 9), 0.0);
    }
}
