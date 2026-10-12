//! PowerPlugin (`PowerState=ACLINE|STATUS|STATUS2|LIFETIME|PERCENT|MHZ|HZ`)
//! from `/sys/class/power_supply` and cpufreq.

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    AcLine,
    Status,
    Status2,
    Lifetime,
    Percent,
    Mhz,
    Hz,
    Unknown,
}

/// What the supplies report, in Windows `SYSTEM_POWER_STATUS` terms.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PowerStatus {
    /// `Some(true)` online, `Some(false)` offline, `None` unknown.
    pub ac_online: Option<bool>,
    pub battery: Option<Battery>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    pub percent: u32,
    pub charging: bool,
    /// Seconds to empty while discharging.
    pub seconds_left: Option<u64>,
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_num(p: &Path) -> Option<f64> {
    read_trim(p)?.parse().ok()
}

/// Scan a `power_supply` directory.
pub fn read_power_supply(root: &Path) -> PowerStatus {
    let mut st = PowerStatus::default();
    let Ok(rd) = std::fs::read_dir(root) else { return st };
    let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let ty = read_trim(&d.join("type")).unwrap_or_default().to_ascii_lowercase();
        match ty.as_str() {
            "mains" | "usb" | "ups" => {
                if let Some(on) = read_num(&d.join("online")) {
                    st.ac_online = Some(st.ac_online.unwrap_or(false) || on > 0.0);
                }
            }
            "battery" if st.battery.is_none() => {
                if read_num(&d.join("present")) == Some(0.0) {
                    continue;
                }
                let status = read_trim(&d.join("status")).unwrap_or_default().to_ascii_lowercase();
                let percent = read_num(&d.join("capacity")).unwrap_or(100.0).clamp(0.0, 100.0) as u32;
                let discharging = status == "discharging";
                let seconds_left = if discharging {
                    read_num(&d.join("time_to_empty_now"))
                        .map(|s| s as u64)
                        .or_else(|| ratio(&d, "energy_now", "power_now"))
                        .or_else(|| ratio(&d, "charge_now", "current_now"))
                } else {
                    None
                };
                st.battery = Some(Battery { percent, charging: status == "charging", seconds_left });
            }
            _ => {}
        }
    }
    st
}

fn ratio(d: &Path, num: &str, den: &str) -> Option<u64> {
    let (n, m) = (read_num(&d.join(num))?, read_num(&d.join(den))?);
    (m > 0.0).then(|| (n / m * 3600.0) as u64)
}

/// `BatteryFlag` bits (1 high, 2 low, 4 critical, 8 charging, 128 none).
pub fn battery_flag(b: Option<&Battery>) -> u32 {
    match b {
        None => 128,
        Some(b) => {
            let mut f = if b.percent > 66 {
                1
            } else if b.percent < 5 {
                4
            } else if b.percent < 33 {
                2
            } else {
                0
            };
            if b.charging {
                f |= 8;
            }
            f
        }
    }
}

/// `STATUS`: 0 none, 1 charging, 2 critical, 3 low, 4 medium/high.
pub fn status_value(flag: u32) -> f64 {
    if flag & 128 != 0 {
        0.0
    } else if flag & 8 != 0 {
        1.0
    } else if flag & 4 != 0 {
        2.0
    } else if flag & 2 != 0 {
        3.0
    } else if flag == 0 || flag & 1 != 0 {
        4.0
    } else {
        0.0
    }
}

/// Current CPU frequency in MHz (core 0).
pub fn read_cpu_mhz(cpu_root: &Path, cpuinfo: &Path) -> Option<f64> {
    if let Some(khz) = read_num(&cpu_root.join("cpu0/cpufreq/scaling_cur_freq")) {
        return Some(khz / 1000.0);
    }
    let text = std::fs::read_to_string(cpuinfo).ok()?;
    text.lines().find(|l| l.starts_with("cpu MHz"))?.split(':').nth(1)?.trim().parse().ok()
}

pub struct PowerMeasure {
    state: State,
    format: String,
    power_root: PathBuf,
    cpu_root: PathBuf,
    cpuinfo: PathBuf,
    lifetime: u64,
    value: f64,
}

impl Default for PowerMeasure {
    fn default() -> Self {
        PowerMeasure::new()
    }
}

const UNKNOWN_LIFETIME: u64 = u32::MAX as u64;

impl PowerMeasure {
    pub fn new() -> PowerMeasure {
        PowerMeasure::with_roots("/sys/class/power_supply".into(), "/sys/devices/system/cpu".into(), "/proc/cpuinfo".into())
    }

    pub fn with_roots(power_root: PathBuf, cpu_root: PathBuf, cpuinfo: PathBuf) -> PowerMeasure {
        PowerMeasure {
            state: State::Unknown,
            format: "%H:%M".into(),
            power_root,
            cpu_root,
            cpuinfo,
            lifetime: UNKNOWN_LIFETIME,
            value: 0.0,
        }
    }
}

impl MeasureImpl for PowerMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.state = match opt.string("PowerState", "").trim().to_ascii_uppercase().as_str() {
            "ACLINE" => State::AcLine,
            "STATUS" => State::Status,
            "STATUS2" => State::Status2,
            "LIFETIME" => State::Lifetime,
            "MHZ" => State::Mhz,
            "HZ" => State::Hz,
            "PERCENT" => State::Percent,
            _ => State::Unknown,
        };
        if self.state == State::Lifetime {
            self.format = opt.string("Format", "%H:%M");
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.value = match self.state {
            State::Mhz | State::Hz => {
                let mhz = read_cpu_mhz(&self.cpu_root, &self.cpuinfo).unwrap_or(0.0);
                if self.state == State::Mhz {
                    mhz
                } else {
                    mhz * 1_000_000.0
                }
            }
            State::Unknown => 0.0,
            s => {
                let p = read_power_supply(&self.power_root);
                match s {
                    State::AcLine => f64::from(p.ac_online == Some(true)),
                    State::Status => status_value(battery_flag(p.battery.as_ref())),
                    State::Status2 => battery_flag(p.battery.as_ref()) as f64,
                    State::Percent => p.battery.as_ref().map(|b| b.percent as f64).unwrap_or(100.0),
                    _ => {
                        self.lifetime = p.battery.and_then(|b| b.seconds_left).unwrap_or(UNKNOWN_LIFETIME);
                        self.lifetime as f64
                    }
                }
            }
        };
        self.value
    }

    fn string_value(&self) -> Option<String> {
        if self.state != State::Lifetime {
            return None;
        }
        if self.lifetime == UNKNOWN_LIFETIME {
            return Some("Unknown".into());
        }
        let (h, m, s) = (self.lifetime / 3600, (self.lifetime / 60) % 60, self.lifetime % 60);
        let dt = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).and_then(|d| d.and_hms_opt(h as u32, m as u32, s as u32));
        Some(dt.map(|dt| crate::timefmt::format_time(&self.format, &dt, 0)).unwrap_or_default())
    }

    fn forced_max(&self) -> Option<f64> {
        match self.state {
            State::AcLine => Some(1.0),
            State::Status => Some(4.0),
            State::Status2 => Some(255.0),
            State::Percent => Some(100.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn reads_ac_and_discharging_battery() {
        let t = tempfile::tempdir().unwrap();
        put(t.path(), "AC/type", "Mains\n");
        put(t.path(), "AC/online", "0\n");
        put(t.path(), "BAT0/type", "Battery\n");
        put(t.path(), "BAT0/status", "Discharging\n");
        put(t.path(), "BAT0/capacity", "42\n");
        put(t.path(), "BAT0/energy_now", "20000000\n");
        put(t.path(), "BAT0/power_now", "10000000\n");
        let p = read_power_supply(t.path());
        assert_eq!(p.ac_online, Some(false));
        let b = p.battery.unwrap();
        assert_eq!((b.percent, b.charging, b.seconds_left), (42, false, Some(7200)));
        assert_eq!(status_value(battery_flag(Some(&b))), 4.0);
    }

    #[test]
    fn charging_low_and_missing_battery() {
        let low = Battery { percent: 10, charging: false, seconds_left: None };
        assert_eq!(battery_flag(Some(&low)), 2);
        assert_eq!(status_value(2), 3.0);
        let chg = Battery { percent: 10, charging: true, seconds_left: None };
        assert_eq!(status_value(battery_flag(Some(&chg))), 1.0);
        assert_eq!(status_value(battery_flag(Some(&Battery { percent: 2, charging: false, seconds_left: None }))), 2.0);
        assert_eq!(battery_flag(None), 128);
        assert_eq!(status_value(128), 0.0);
        let t = tempfile::tempdir().unwrap();
        assert_eq!(read_power_supply(t.path()), PowerStatus::default());
    }

    #[test]
    fn cpu_frequency_from_cpufreq_or_cpuinfo() {
        let t = tempfile::tempdir().unwrap();
        put(t.path(), "cpu/cpu0/cpufreq/scaling_cur_freq", "2400000\n");
        assert_eq!(read_cpu_mhz(&t.path().join("cpu"), &t.path().join("none")), Some(2400.0));
        put(t.path(), "cpuinfo", "model name : x\ncpu MHz\t\t: 1234.5\n");
        assert_eq!(read_cpu_mhz(&t.path().join("nocpu"), &t.path().join("cpuinfo")), Some(1234.5));
    }

    #[test]
    fn measure_values_and_lifetime_string() {
        let t = tempfile::tempdir().unwrap();
        put(t.path(), "ps/BAT0/type", "Battery\n");
        put(t.path(), "ps/BAT0/status", "Discharging\n");
        put(t.path(), "ps/BAT0/capacity", "80\n");
        put(t.path(), "ps/BAT0/time_to_empty_now", "5400\n");
        let ini = "[P]\nMeasure=Plugin\nPlugin=PowerPlugin\nPowerState=LIFETIME\n[Q]\nPowerState=PERCENT\n";
        let s = crate::measures::testutil::skin(ini);
        let run = |section: &str| {
            let mut m = PowerMeasure::with_roots(t.path().join("ps"), t.path().join("cpu"), t.path().join("ci"));
            let opt = OptionReader::new(&s.parser, section, None);
            m.read_options(&opt);
            let v = crate::measures::testutil::ctx_run(&s, |c| m.update(c));
            (v, m.string_value())
        };
        assert_eq!(run("P"), (5400.0, Some("01:30".to_string())));
        assert_eq!(run("Q").0, 80.0);
    }
}
