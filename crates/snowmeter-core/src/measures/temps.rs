//! Hardware-sensor plugins on top of `/sys/class/hwmon`: `CoreTemp`,
//! `SpeedFan` and `HWiNFO`.
//!
//! Mapping (documented because the Windows programs have no Linux twin):
//! * CoreTemp: per-core temperature from `coretemp` (`Core N`); AMD drivers
//!   (`k10temp`, `zenpower`) only export Tdie/Tctl, which is reported for every
//!   core index. Load comes from `/proc/stat` (busy share since the previous
//!   update of that measure; the first update is the average since boot), clocks
//!   from cpufreq or `/proc/cpuinfo`. The bus speed is not exposed by Linux and is
//!   fixed at 100 MHz, so `BusMultiplier` = clock / 100.
//! * SpeedFan: `SpeedFanNumber` indexes the flat list of all `temp*_input` /
//!   `fan*_input` / `in*_input` files; CPU drivers first, then chips by name.
//! * HWiNFO: `HWiNFOEntryId` is mapped like a SpeedFan number: bits 24-27 give
//!   the reading class (1 temperature, 2 voltage, 3 fan, anything else
//!   temperature), the low 24 bits the index in that flat list. The sensor id is
//!   ignored. Min/Max/Avg are tracked from the readings seen by the measure.

use super::hwmon::{self, Chip, Kind};
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::path::PathBuf;

/// Where the sensors are read from (overridable for tests).
#[derive(Clone, Debug)]
pub struct Sources {
    pub hwmon: PathBuf,
    pub proc_stat: PathBuf,
    pub cpuinfo: PathBuf,
    pub cpu_root: PathBuf,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            hwmon: "/sys/class/hwmon".into(),
            proc_stat: "/proc/stat".into(),
            cpuinfo: "/proc/cpuinfo".into(),
            cpu_root: "/sys/devices/system/cpu".into(),
        }
    }
}

const BUS_MHZ: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CtType {
    Temperature,
    MaxTemperature,
    TjMax,
    Load,
    Vid,
    CpuSpeed,
    BusSpeed,
    BusMultiplier,
    CpuName,
    CoreSpeed,
    CoreBusMultiplier,
    Tdp,
    Power,
}

fn parse_ct_type(s: &str) -> Option<CtType> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "temperature" => CtType::Temperature,
        "maxtemperature" => CtType::MaxTemperature,
        "tjmax" => CtType::TjMax,
        "load" => CtType::Load,
        "vid" => CtType::Vid,
        "cpuspeed" => CtType::CpuSpeed,
        "busspeed" => CtType::BusSpeed,
        "busmultiplier" => CtType::BusMultiplier,
        "cpuname" => CtType::CpuName,
        "corespeed" => CtType::CoreSpeed,
        "corebusmultiplier" => CtType::CoreBusMultiplier,
        "tdp" => CtType::Tdp,
        "power" => CtType::Power,
        _ => return None,
    })
}

pub struct CoreTemp {
    ty: CtType,
    index: usize,
    src: Sources,
    prev_load: Vec<(u64, u64)>,
    string: Option<String>,
    log: Vec<String>,
}

impl CoreTemp {
    pub fn new() -> CoreTemp {
        CoreTemp::with_sources(Sources::default())
    }

    pub fn with_sources(src: Sources) -> CoreTemp {
        CoreTemp { ty: CtType::Temperature, index: 0, src, prev_load: Vec::new(), string: None, log: Vec::new() }
    }

    fn load(&mut self) -> f64 {
        let text = std::fs::read_to_string(&self.src.proc_stat).unwrap_or_default();
        let cur = hwmon::parse_proc_stat(&text);
        // Element 0 is the total; Core Temp's index 0 is the first core.
        let v = match cur.get(self.index + 1) {
            Some(c) => hwmon::load_between(self.prev_load.get(self.index + 1).copied(), *c),
            None => 0.0,
        };
        self.prev_load = cur;
        v
    }
}

impl Default for CoreTemp {
    fn default() -> Self {
        CoreTemp::new()
    }
}

impl MeasureImpl for CoreTemp {
    fn read_options(&mut self, opt: &OptionReader) {
        let t = opt.string("CoreTempType", "Temperature");
        self.ty = parse_ct_type(&t).unwrap_or_else(|| {
            self.log.push(format!("CoreTemp: CoreTempType={t} is not valid"));
            CtType::Temperature
        });
        self.index = opt.int("CoreTempIndex", 0).max(0) as usize;
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.string = None;
        let idx = self.index;
        match self.ty {
            CtType::Load => return self.load(),
            CtType::CpuName => {
                let t = std::fs::read_to_string(&self.src.cpuinfo).unwrap_or_default();
                self.string = Some(hwmon::cpuinfo_name(&t));
                return 0.0;
            }
            CtType::CpuSpeed => return hwmon::cpu_mhz(&self.src.cpu_root, &self.src.cpuinfo, 0),
            CtType::BusSpeed => return BUS_MHZ,
            CtType::BusMultiplier => return hwmon::cpu_mhz(&self.src.cpu_root, &self.src.cpuinfo, 0) / BUS_MHZ,
            CtType::CoreSpeed => return hwmon::cpu_mhz(&self.src.cpu_root, &self.src.cpuinfo, idx),
            CtType::CoreBusMultiplier => return hwmon::cpu_mhz(&self.src.cpu_root, &self.src.cpuinfo, idx) / BUS_MHZ,
            CtType::Tdp | CtType::Power => return 0.0,
            _ => {}
        }
        let chips = hwmon::scan(&self.src.hwmon);
        match self.ty {
            CtType::Vid => {
                let v = hwmon::vcore(&chips);
                self.string = Some(format!("{v:.4}"));
                v
            }
            ty => {
                let Some(chip) = hwmon::cpu_chip(&chips) else { return 0.0 };
                let (cores, per_core) = hwmon::core_temps(chip);
                match ty {
                    CtType::Temperature => {
                        if per_core {
                            cores.get(idx).copied().unwrap_or(0.0)
                        } else {
                            cores.first().copied().unwrap_or(0.0)
                        }
                    }
                    CtType::MaxTemperature => cores.iter().copied().fold(0.0, f64::max),
                    CtType::TjMax => hwmon::tj_max(chip),
                    _ => 0.0,
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

// ---- SpeedFan -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scale {
    C,
    F,
    K,
}

pub fn convert_temp(c: f64, scale: char) -> f64 {
    match scale.to_ascii_uppercase() {
        'F' => c * 9.0 / 5.0 + 32.0,
        'K' => c + 273.15,
        _ => c,
    }
}

/// Reading `n` of `kind` over all chips.
pub fn nth_reading(chips: &[Chip], kind: Kind, n: usize) -> Option<(String, String, f64)> {
    hwmon::flat(chips, kind).get(n).map(|(chip, s)| (chip.to_string(), s.label.clone(), s.value))
}

pub struct SpeedFan {
    kind: Kind,
    number: usize,
    scale: Scale,
    root: PathBuf,
    log: Vec<String>,
}

impl SpeedFan {
    pub fn new() -> SpeedFan {
        SpeedFan::with_root("/sys/class/hwmon".into())
    }

    pub fn with_root(root: PathBuf) -> SpeedFan {
        SpeedFan { kind: Kind::Temp, number: 0, scale: Scale::C, root, log: Vec::new() }
    }
}

impl Default for SpeedFan {
    fn default() -> Self {
        SpeedFan::new()
    }
}

impl MeasureImpl for SpeedFan {
    fn read_options(&mut self, opt: &OptionReader) {
        let t = opt.string("SpeedFanType", "TEMPERATURE");
        match t.trim().to_ascii_uppercase().as_str() {
            "TEMPERATURE" => {
                self.kind = Kind::Temp;
                let s = opt.string("SpeedFanScale", "C");
                self.scale = match s.trim().to_ascii_uppercase().as_str() {
                    "C" => Scale::C,
                    "F" => Scale::F,
                    "K" => Scale::K,
                    _ => {
                        self.log.push(format!("SpeedFan: SpeedFanScale={s} is not valid"));
                        Scale::C
                    }
                };
            }
            "FAN" => self.kind = Kind::Fan,
            "VOLTAGE" => self.kind = Kind::Volt,
            _ => self.log.push(format!("SpeedFan: SpeedFanType={t} is not valid")),
        }
        self.number = opt.int("SpeedFanNumber", 0).max(0) as usize;
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let chips = hwmon::scan(&self.root);
        let Some((_, _, v)) = nth_reading(&chips, self.kind, self.number) else { return 0.0 };
        match (self.kind, self.scale) {
            (Kind::Temp, Scale::F) => convert_temp(v, 'F'),
            (Kind::Temp, Scale::K) => convert_temp(v, 'K'),
            _ => v,
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

// ---- HWiNFO -------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HwType {
    Current,
    Min,
    Max,
    Avg,
    SensorName,
    EntryLabel,
    Unit,
}

/// Hex (`0xF0001000`, `F0001000`) or decimal id.
pub fn parse_id(s: &str) -> Option<u64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let h = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"));
    match h {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => t.parse().ok().or_else(|| u64::from_str_radix(t, 16).ok()),
    }
}

/// `(kind, index)` for a HWiNFO reading id.
pub fn entry_to_reading(id: u64) -> (Kind, usize) {
    let kind = match (id >> 24) & 0xF {
        2 => Kind::Volt,
        3 => Kind::Fan,
        _ => Kind::Temp,
    };
    (kind, (id & 0xFF_FFFF) as usize)
}

pub struct HwInfo {
    ty: HwType,
    entry: Option<u64>,
    root: PathBuf,
    min: f64,
    max: f64,
    sum: f64,
    n: u64,
    string: Option<String>,
    log: Vec<String>,
}

impl HwInfo {
    pub fn new() -> HwInfo {
        HwInfo::with_root("/sys/class/hwmon".into())
    }

    pub fn with_root(root: PathBuf) -> HwInfo {
        HwInfo { ty: HwType::Current, entry: None, root, min: 0.0, max: 0.0, sum: 0.0, n: 0, string: None, log: Vec::new() }
    }
}

impl Default for HwInfo {
    fn default() -> Self {
        HwInfo::new()
    }
}

impl MeasureImpl for HwInfo {
    fn read_options(&mut self, opt: &OptionReader) {
        let t = opt.string("HWiNFOType", "CurrentValue");
        self.ty = match t.trim().to_ascii_lowercase().as_str() {
            "currentvalue" | "value" | "" => HwType::Current,
            "valuemin" | "min" => HwType::Min,
            "valuemax" | "max" => HwType::Max,
            "valueavg" | "avg" => HwType::Avg,
            "sensorname" | "sensornamecustom" => HwType::SensorName,
            "entrylabel" | "entrylabelcustom" | "entrylabeloriginal" => HwType::EntryLabel,
            "unit" => HwType::Unit,
            _ => {
                self.log.push(format!("HWiNFO: HWiNFOType={t} is not valid"));
                HwType::Current
            }
        };
        self.entry = parse_id(&opt.string("HWiNFOEntryId", ""));
        if self.entry.is_none() {
            self.log.push("HWiNFO: HWiNFOEntryId is missing or invalid".into());
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.string = None;
        let Some(id) = self.entry else { return 0.0 };
        let (kind, idx) = entry_to_reading(id);
        let chips = hwmon::scan(&self.root);
        let Some((chip, label, v)) = nth_reading(&chips, kind, idx) else {
            if matches!(self.ty, HwType::SensorName | HwType::EntryLabel | HwType::Unit) {
                self.string = Some(String::new());
            }
            return 0.0;
        };
        if self.n == 0 {
            self.min = v;
            self.max = v;
        }
        self.min = self.min.min(v);
        self.max = self.max.max(v);
        self.sum += v;
        self.n += 1;
        match self.ty {
            HwType::Current => v,
            HwType::Min => self.min,
            HwType::Max => self.max,
            HwType::Avg => self.sum / self.n as f64,
            HwType::SensorName => {
                self.string = Some(chip);
                0.0
            }
            HwType::EntryLabel => {
                self.string = Some(label);
                0.0
            }
            HwType::Unit => {
                self.string = Some(match kind {
                    Kind::Temp => "\u{b0}C",
                    Kind::Fan => "RPM",
                    Kind::Volt => "V",
                }
                .into());
                0.0
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
    use crate::measures::hwmon::fixtures::*;
    use crate::measures::testutil::{ctx_run, skin};

    fn run(m: &mut dyn MeasureImpl, ini: &str) -> f64 {
        let sk = skin(&format!("[M]\nMeasure=Plugin\n{ini}\n"));
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        ctx_run(&sk, |c| m.update(c))
    }

    fn src(t: &tempfile::TempDir) -> Sources {
        put(t.path(), "stat1", "cpu  100 0 100 800 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0\n");
        put(t.path(), "stat2", "cpu  150 0 150 900 0 0 0 0\ncpu0 100 0 100 400 0 0 0 0\ncpu1 50 0 50 500 0 0 0 0\n");
        put(t.path(), "cpuinfo", "model name : Test CPU @ 3GHz\ncpu MHz : 3000.0\ncpu MHz : 2000.0\n");
        Sources {
            hwmon: t.path().join("hw"),
            proc_stat: t.path().join("stat1"),
            cpuinfo: t.path().join("cpuinfo"),
            cpu_root: t.path().join("nocpu"),
        }
    }

    #[test]
    fn coretemp_intel() {
        let hw = intel_tree();
        let t = tempfile::tempdir().unwrap();
        let mut s = src(&t);
        s.hwmon = hw.path().to_path_buf();
        let v = |ini: &str| run(&mut CoreTemp::with_sources(s.clone()), ini);
        assert_eq!(v("CoreTempType=Temperature\nCoreTempIndex=1"), 49.0);
        assert_eq!(v("CoreTempIndex=0"), 47.0);
        assert_eq!(v("CoreTempIndex=7"), 0.0);
        assert_eq!(v("CoreTempType=MaxTemperature"), 49.0);
        assert_eq!(v("CoreTempType=TjMax"), 100.0);
        assert_eq!(v("CoreTempType=CpuSpeed"), 3000.0);
        assert_eq!(v("CoreTempType=CoreSpeed\nCoreTempIndex=1"), 2000.0);
        assert_eq!(v("CoreTempType=BusSpeed"), 100.0);
        assert_eq!(v("CoreTempType=BusMultiplier"), 30.0);
        assert!((v("CoreTempType=Vid") - 1.056).abs() < 1e-9);
        let mut n = CoreTemp::with_sources(s.clone());
        run(&mut n, "CoreTempType=CpuName");
        assert_eq!(n.string_value().as_deref(), Some("Test CPU @ 3GHz"));
        let mut bad = CoreTemp::with_sources(s);
        assert_eq!(run(&mut bad, "CoreTempType=Nope"), 47.0);
        assert!(bad.take_log()[0].contains("not valid"));
    }

    #[test]
    fn coretemp_amd_and_missing() {
        let hw = amd_tree();
        let t = tempfile::tempdir().unwrap();
        let mut s = src(&t);
        s.hwmon = hw.path().to_path_buf();
        assert_eq!(run(&mut CoreTemp::with_sources(s.clone()), "CoreTempIndex=5"), 60.5);
        s.hwmon = t.path().join("missing");
        assert_eq!(run(&mut CoreTemp::with_sources(s), "CoreTempType=MaxTemperature"), 0.0);
    }

    #[test]
    fn coretemp_load_uses_deltas() {
        let t = tempfile::tempdir().unwrap();
        let s = src(&t);
        let mut m = CoreTemp::with_sources(s.clone());
        let sk = skin("[M]\nMeasure=Plugin\nCoreTempType=Load\nCoreTempIndex=0\n");
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        let first = ctx_run(&sk, |c| m.update(c));
        assert!((first - 20.0).abs() < 1e-9);
        m.src.proc_stat = t.path().join("stat2");
        assert_eq!(ctx_run(&sk, |c| m.update(c)), 100.0);
    }

    #[test]
    fn speedfan_numbers_and_scales() {
        let hw = intel_tree();
        let v = |ini: &str| run(&mut SpeedFan::with_root(hw.path().to_path_buf()), ini);
        assert_eq!(v("SpeedFanType=Temperature\nSpeedFanNumber=0"), 50.0);
        assert!((v("SpeedFanNumber=3\nSpeedFanScale=F") - 91.4).abs() < 1e-9);
        assert!((v("SpeedFanNumber=0\nSpeedFanScale=K") - 323.15).abs() < 1e-9);
        assert_eq!(v("SpeedFanType=Fan\nSpeedFanNumber=1"), 850.0);
        assert!((v("SpeedFanType=Voltage\nSpeedFanNumber=1") - 12.096).abs() < 1e-9);
        assert_eq!(v("SpeedFanType=Fan\nSpeedFanNumber=9"), 0.0);
        let mut bad = SpeedFan::with_root("/nonexistent".into());
        assert_eq!(run(&mut bad, "SpeedFanType=Quux\nSpeedFanScale=Z"), 0.0);
        assert_eq!(bad.take_log().len(), 1);
    }

    #[test]
    fn hwinfo_mapping_and_stats() {
        let hw = intel_tree();
        let root = hw.path().to_path_buf();
        assert_eq!(parse_id("0x1000002"), Some(0x1000002));
        assert_eq!(parse_id("F0001000"), Some(0xF000_1000));
        assert_eq!(parse_id("12"), Some(12));
        assert_eq!(parse_id("zz"), None);
        assert_eq!(entry_to_reading(0x3000001), (Kind::Fan, 1));
        assert_eq!(entry_to_reading(0x2000000), (Kind::Volt, 0));
        assert_eq!(entry_to_reading(0x1000002), (Kind::Temp, 2));
        let v = |ini: &str| run(&mut HwInfo::with_root(root.clone()), ini);
        assert_eq!(v("HWiNFOSensorId=0xF0000300\nHWiNFOEntryId=0x1000001"), 47.0);
        assert_eq!(v("HWiNFOEntryId=0x3000000\nHWiNFOType=ValueMax"), 1200.0);
        let mut l = HwInfo::with_root(root.clone());
        run(&mut l, "HWiNFOEntryId=0x1000001\nHWiNFOType=EntryLabel");
        assert_eq!(l.string_value().as_deref(), Some("Core 0"));
        let mut u = HwInfo::with_root(root.clone());
        run(&mut u, "HWiNFOEntryId=0x2000000\nHWiNFOType=Unit");
        assert_eq!(u.string_value().as_deref(), Some("V"));
        let mut s = HwInfo::with_root(root);
        run(&mut s, "HWiNFOEntryId=0x1000000\nHWiNFOType=SensorName");
        assert_eq!(s.string_value().as_deref(), Some("coretemp"));
        let mut bad = HwInfo::with_root("/nonexistent".into());
        assert_eq!(run(&mut bad, "HWiNFOType=Bogus"), 0.0);
        assert_eq!(bad.take_log().len(), 2);
    }
}
