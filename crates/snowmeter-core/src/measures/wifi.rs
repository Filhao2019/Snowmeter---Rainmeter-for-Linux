//! WifiStatus on Linux: `nmcli` (scan list, connection) and `iw` (bit rates,
//! PHY) on a worker thread, `/proc/net/wireless` as the fallback for the
//! quality. The measures only read a cached snapshot.
//!
//! Windows strings are mapped as follows: `ENCRYPTION` is `NONE`, `WEP`, `TKIP`
//! (WPA1) or `AES` (WPA2/WPA3); `AUTH` is `Open`, `WPA-Personal`,
//! `WPA2-Personal`, `WPA3-Personal` or the `-Enterprise` forms (802.1X). `PHY`
//! comes from the `iw` bit-rate flags; scan-list entries only have a band-based
//! estimate. `TXRATE`/`RXRATE` are in bit/s like Rainmeter.

use super::bgshared::Bg;
use super::extcmd::{have, run_capture};
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Net {
    pub ssid: String,
    /// 0-100
    pub signal: u32,
    /// nmcli SECURITY column (`WPA2`, `WPA1 WPA2`, `--`, `802.1X` ...)
    pub security: String,
    pub freq_mhz: u32,
    pub in_use: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Link {
    pub ssid: String,
    pub quality: u32,
    pub security: String,
    pub phy: String,
    pub tx_bps: f64,
    pub rx_bps: f64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Iface {
    pub name: String,
    pub link: Option<Link>,
    pub list: Vec<Net>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Snapshot {
    pub ifaces: Vec<Iface>,
}

// ---- parsing ----------------------------------------------------------------

/// Split one `nmcli -t` line on unescaped `:`; `\:` and `\\` are unescaped.
pub fn split_terse(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            ':' => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Output of `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY,FREQ device wifi list`.
pub fn parse_wifi_list(out: &str) -> Vec<Net> {
    out.lines()
        .filter_map(|l| {
            let f = split_terse(l);
            if f.len() < 5 {
                return None;
            }
            Some(Net {
                in_use: f[0].trim() == "*",
                ssid: f[1].clone(),
                signal: f[2].trim().parse().unwrap_or(0),
                security: f[3].trim().to_string(),
                freq_mhz: f[4].split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0),
            })
        })
        .collect()
}

/// `(interface, quality 0-100)` from `/proc/net/wireless`.
pub fn parse_proc_wireless(text: &str) -> Vec<(String, u32)> {
    text.lines()
        .skip(2)
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let link: f64 = rest.split_whitespace().nth(1)?.trim_end_matches('.').parse().ok()?;
            Some((name.trim().to_string(), (link / 70.0 * 100.0).round().clamp(0.0, 100.0) as u32))
        })
        .collect()
}

/// Facts from `iw dev <if> link`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct IwLink {
    pub connected: bool,
    pub ssid: String,
    pub signal_dbm: Option<f64>,
    pub phy: String,
    pub tx_bps: f64,
    pub rx_bps: f64,
}

fn bitrate(l: &str) -> (f64, String) {
    let rest = l.split_once(':').map(|x| x.1).unwrap_or("").trim();
    let mut it = rest.split_whitespace();
    let rate: f64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let phy = if rest.contains("EHT-MCS") {
        "802.11be"
    } else if rest.contains("HE-MCS") {
        "802.11ax"
    } else if rest.contains("VHT-MCS") {
        "802.11ac"
    } else if rest.contains("HT-MCS") || rest.contains(" MCS ") {
        "802.11n"
    } else {
        ""
    };
    (rate * 1_000_000.0, phy.to_string())
}

pub fn parse_iw_link(out: &str) -> IwLink {
    let mut r = IwLink::default();
    for l in out.lines() {
        let t = l.trim();
        if t.starts_with("Connected to") {
            r.connected = true;
        } else if let Some(s) = t.strip_prefix("SSID:") {
            r.ssid = s.trim().to_string();
        } else if let Some(s) = t.strip_prefix("signal:") {
            r.signal_dbm = s.split_whitespace().next().and_then(|v| v.parse().ok());
        } else if t.starts_with("tx bitrate:") {
            let (b, p) = bitrate(t);
            r.tx_bps = b;
            if !p.is_empty() {
                r.phy = p;
            }
        } else if t.starts_with("rx bitrate:") {
            let (b, p) = bitrate(t);
            r.rx_bps = b;
            if r.phy.is_empty() {
                r.phy = p;
            }
        }
    }
    r
}

/// dBm to a 0-100 quality like the Windows driver stack (-100 dBm = 0, -50 dBm = 100).
pub fn dbm_to_quality(dbm: f64) -> u32 {
    (2.0 * (dbm + 100.0)).round().clamp(0.0, 100.0) as u32
}

/// Band-based PHY guess for scan results.
pub fn phy_from_freq(mhz: u32) -> &'static str {
    match mhz {
        0 => "???",
        f if f >= 5955 => "802.11ax",
        f if f >= 4900 => "802.11ac",
        _ => "802.11n",
    }
}

pub fn cipher_string(security: &str) -> &'static str {
    let s = security.to_ascii_uppercase();
    if s.is_empty() || s == "--" {
        "NONE"
    } else if s.contains("WPA2") || s.contains("WPA3") || s.contains("OWE") {
        "AES"
    } else if s.contains("WPA") {
        "TKIP"
    } else if s.contains("WEP") {
        "WEP"
    } else if s.contains("802.1X") {
        "AES"
    } else {
        "???"
    }
}

pub fn auth_string(security: &str) -> &'static str {
    let s = security.to_ascii_uppercase();
    let ent = s.contains("802.1X");
    if s.contains("WPA3") {
        if ent { "WPA3-Enterprise" } else { "WPA3-Personal" }
    } else if s.contains("WPA2") {
        if ent { "WPA2-Enterprise" } else { "WPA2-Personal" }
    } else if s.contains("WPA") {
        if ent { "WPA-Enterprise" } else { "WPA-Personal" }
    } else if ent {
        "WPA2-Enterprise"
    } else if s.is_empty() || s == "--" || s.contains("WEP") || s.contains("OWE") {
        "Open"
    } else {
        "???"
    }
}

/// The `LIST` text for `style` (bit mask: 1 PHY, 2 cipher:auth, 4 quality) and `limit` entries.
pub fn format_list(nets: &[Net], style: i64, limit: usize) -> String {
    let mut sorted: Vec<&Net> = nets.iter().collect();
    sorted.sort_by(|a, b| b.signal.cmp(&a.signal));
    let mut seen: Vec<&str> = Vec::new();
    let mut out = String::new();
    for n in sorted {
        if seen.len() >= limit {
            break;
        }
        if n.ssid.is_empty() || seen.contains(&n.ssid.as_str()) {
            continue;
        }
        seen.push(&n.ssid);
        out.push_str(&n.ssid);
        if style & 1 != 0 {
            out.push_str(" @");
            out.push_str(phy_from_freq(n.freq_mhz));
        }
        if style & 2 != 0 {
            out.push_str(&format!(" ({}:{})", cipher_string(&n.security), auth_string(&n.security)));
        }
        if style & 4 != 0 {
            out.push_str(&format!(" [{}]", n.signal));
        }
        out.push('\n');
    }
    out
}

// ---- data collection --------------------------------------------------------

fn fetch_iface(name: &str, proc_quality: Option<u32>) -> Iface {
    let t = Duration::from_secs(8);
    let list = if have("nmcli") {
        run_capture(
            "nmcli",
            &["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY,FREQ", "device", "wifi", "list", "ifname", name, "--rescan", "no"],
            t,
        )
        .map(|o| parse_wifi_list(&o))
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    let iw = if have("iw") { run_capture("iw", &["dev", name, "link"], t).map(|o| parse_iw_link(&o)) } else { None };
    let current = list.iter().find(|n| n.in_use);
    let connected = current.is_some() || iw.as_ref().is_some_and(|i| i.connected) || proc_quality.is_some_and(|q| q > 0 && iw.is_none() && list.is_empty());
    let link = connected.then(|| {
        let iw = iw.clone().unwrap_or_default();
        let ssid = current.map(|c| c.ssid.clone()).filter(|s| !s.is_empty()).unwrap_or(iw.ssid.clone());
        let quality = current
            .map(|c| c.signal)
            .or_else(|| iw.signal_dbm.map(dbm_to_quality))
            .or(proc_quality)
            .unwrap_or(0);
        let phy = if !iw.phy.is_empty() { iw.phy.clone() } else { phy_from_freq(current.map(|c| c.freq_mhz).unwrap_or(0)).to_string() };
        Link { ssid, quality, security: current.map(|c| c.security.clone()).unwrap_or_default(), phy, tx_bps: iw.tx_bps, rx_bps: iw.rx_bps }
    });
    Iface { name: name.to_string(), link, list }
}

/// Collect everything (slow: spawns helpers). Call from a worker thread.
pub fn fetch_real() -> Snapshot {
    let mut names: Vec<(String, Option<u32>)> =
        std::fs::read_to_string("/proc/net/wireless").map(|t| parse_proc_wireless(&t)).unwrap_or_default().into_iter().map(|(n, q)| (n, Some(q))).collect();
    if names.is_empty() && have("nmcli") {
        if let Some(o) = run_capture("nmcli", &["-t", "-f", "DEVICE,TYPE", "device"], Duration::from_secs(5)) {
            names = o
                .lines()
                .map(split_terse)
                .filter(|f| f.len() >= 2 && f[1] == "wifi")
                .map(|f| (f[0].clone(), None))
                .collect();
        }
    }
    Snapshot { ifaces: names.iter().map(|(n, q)| fetch_iface(n, *q)).collect() }
}

static SHARED: Mutex<Option<(Bg<Snapshot>, Option<Instant>)>> = Mutex::new(None);
const REFRESH: Duration = Duration::from_secs(4);

/// Process-wide cached snapshot, refreshed by one worker at most every few seconds.
pub fn shared_snapshot() -> Snapshot {
    let Ok(mut g) = SHARED.lock() else { return Snapshot::default() };
    let (bg, last) = g.get_or_insert_with(|| (Bg::new(Snapshot::default()), None));
    if last.is_none_or(|t| t.elapsed() >= REFRESH) && bg.start(Duration::ZERO, fetch_real) {
        *last = Some(Instant::now());
    }
    bg.get()
}

// ---- the measure ------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    Ssid,
    Quality,
    Encryption,
    Auth,
    Phy,
    List,
    TxRate,
    RxRate,
    Unknown,
}

pub struct WifiStatus {
    ty: Ty,
    intf: usize,
    style: i64,
    limit: usize,
    source: fn() -> Snapshot,
    string: Option<String>,
    log: Vec<String>,
}

impl WifiStatus {
    pub fn new() -> WifiStatus {
        WifiStatus::with_source(shared_snapshot)
    }

    pub fn with_source(source: fn() -> Snapshot) -> WifiStatus {
        WifiStatus { ty: Ty::Unknown, intf: 0, style: 0, limit: 5, source, string: None, log: Vec::new() }
    }
}

impl Default for WifiStatus {
    fn default() -> Self {
        WifiStatus::new()
    }
}

impl MeasureImpl for WifiStatus {
    fn read_options(&mut self, opt: &OptionReader) {
        self.intf = opt.int("WifiIntfID", 0).max(0) as usize;
        self.style = opt.int("WifiListStyle", 0);
        if !(0..=7).contains(&self.style) {
            self.log.push(format!("WifiStatus: WifiListStyle={} is not valid", self.style));
            self.style = 0;
        }
        let lim = opt.int("WifiListLimit", 5);
        self.limit = if lim <= 0 {
            self.log.push(format!("WifiStatus: WifiListLimit={lim} is not valid"));
            5
        } else {
            lim as usize
        };
        self.ty = match opt.string("WifiInfoType", "").trim().to_ascii_uppercase().as_str() {
            "SSID" => Ty::Ssid,
            "QUALITY" => Ty::Quality,
            "ENCRYPTION" => Ty::Encryption,
            "AUTH" => Ty::Auth,
            "PHY" => Ty::Phy,
            "LIST" => Ty::List,
            "TXRATE" => Ty::TxRate,
            "RXRATE" => Ty::RxRate,
            other => {
                self.log.push(format!("WifiStatus: WifiInfoType={other} is not valid"));
                Ty::Unknown
            }
        };
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.string = None;
        if self.ty == Ty::Unknown {
            return 0.0;
        }
        let snap = (self.source)();
        let Some(i) = snap.ifaces.get(self.intf) else { return 0.0 };
        if self.ty == Ty::List {
            self.string = Some(format_list(&i.list, self.style, self.limit));
            return 0.0;
        }
        let Some(l) = &i.link else {
            // Not connected: text types read "-1", numbers 0.
            if matches!(self.ty, Ty::Ssid | Ty::Encryption | Ty::Auth | Ty::Phy) {
                self.string = Some("-1".into());
            }
            return 0.0;
        };
        match self.ty {
            Ty::Ssid => self.string = Some(l.ssid.clone()),
            Ty::Phy => self.string = Some(l.phy.clone()),
            Ty::Encryption => self.string = Some(cipher_string(&l.security).into()),
            Ty::Auth => self.string = Some(auth_string(&l.security).into()),
            Ty::Quality => return f64::from(l.quality),
            Ty::TxRate => return l.tx_bps,
            Ty::RxRate => return l.rx_bps,
            Ty::List | Ty::Unknown => {}
        }
        0.0
    }

    fn string_value(&self) -> Option<String> {
        self.string.clone().filter(|s| !s.is_empty())
    }

    fn forced_max(&self) -> Option<f64> {
        (self.ty == Ty::Quality).then_some(100.0)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    const LIST: &str = "*:Home\\:5G:78:WPA2:5180 MHz\n :Home\\:5G:60:WPA2:2437 MHz\n :Cafe:41:--:2412 MHz\n :Corp:90:WPA2 802.1X:5745 MHz\n :Old:20:WEP:2412 MHz\n ::15:WPA2:2412 MHz\n";

    #[test]
    fn terse_and_list_parsing() {
        assert_eq!(split_terse("a\\:b:c\\\\d:"), ["a:b", "c\\d", ""]);
        let l = parse_wifi_list(LIST);
        assert_eq!(l.len(), 6);
        assert!(l[0].in_use && l[0].ssid == "Home:5G" && l[0].signal == 78 && l[0].freq_mhz == 5180);
        assert!(!l[1].in_use);
        assert!(parse_wifi_list("garbage\n").is_empty());
    }

    #[test]
    fn list_formatting() {
        let l = parse_wifi_list(LIST);
        assert_eq!(format_list(&l, 0, 3), "Corp\nHome:5G\nCafe\n");
        assert_eq!(format_list(&l, 0, 1), "Corp\n");
        assert_eq!(format_list(&l, 7, 2), "Corp @802.11ac (AES:WPA2-Enterprise) [90]\nHome:5G @802.11ac (AES:WPA2-Personal) [78]\n");
        assert_eq!(format_list(&l, 6, 10).lines().nth(3).unwrap(), "Old (WEP:Open) [20]");
        assert_eq!(format_list(&[], 0, 5), "");
    }

    #[test]
    fn security_strings() {
        assert_eq!((cipher_string("--"), auth_string("--")), ("NONE", "Open"));
        assert_eq!((cipher_string("WPA1"), auth_string("WPA1")), ("TKIP", "WPA-Personal"));
        assert_eq!((cipher_string("WPA2 WPA3"), auth_string("WPA2 WPA3")), ("AES", "WPA3-Personal"));
        assert_eq!(auth_string("WPA1 WPA2 802.1X"), "WPA2-Enterprise");
        assert_eq!(cipher_string("WEP"), "WEP");
    }

    #[test]
    fn proc_wireless_and_iw() {
        let w = "Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE\n face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22\nwlan0: 0000   49.  -61.  -256        0      0      0      0      0        0\n";
        assert_eq!(parse_proc_wireless(w), [("wlan0".to_string(), 70)]);
        assert!(parse_proc_wireless("").is_empty());
        let iw = "Connected to aa:bb:cc:dd:ee:ff (on wlan0)\n\tSSID: Home\n\tfreq: 5180\n\tsignal: -55 dBm\n\trx bitrate: 866.7 MBit/s VHT-MCS 9 80MHz short GI VHT-NSS 2\n\ttx bitrate: 650.0 MBit/s VHT-MCS 7\n";
        let r = parse_iw_link(iw);
        assert!(r.connected && r.ssid == "Home" && r.phy == "802.11ac");
        assert_eq!((r.tx_bps, r.rx_bps, r.signal_dbm), (650e6, 866.7e6, Some(-55.0)));
        assert_eq!(dbm_to_quality(-55.0), 90);
        assert_eq!(dbm_to_quality(-120.0), 0);
        assert!(!parse_iw_link("Not connected.\n").connected);
    }

    fn connected() -> Snapshot {
        Snapshot {
            ifaces: vec![Iface {
                name: "wlan0".into(),
                link: Some(Link { ssid: "Home".into(), quality: 78, security: "WPA2".into(), phy: "802.11ac".into(), tx_bps: 650e6, rx_bps: 866e6 }),
                list: parse_wifi_list(LIST),
            }],
        }
    }

    fn disconnected() -> Snapshot {
        Snapshot { ifaces: vec![Iface { name: "wlan0".into(), link: None, list: Vec::new() }] }
    }

    fn eval(src: fn() -> Snapshot, ini: &str) -> (f64, Option<String>) {
        let sk = skin(&format!("[M]\nMeasure=Plugin\nPlugin=WifiStatus\n{ini}\n"));
        let mut m = WifiStatus::with_source(src);
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        let v = ctx_run(&sk, |c| m.update(c));
        (v, m.string_value())
    }

    #[test]
    fn measure_values() {
        assert_eq!(eval(connected, "WifiInfoType=SSID").1.as_deref(), Some("Home"));
        assert_eq!(eval(connected, "WifiInfoType=QUALITY").0, 78.0);
        assert_eq!(eval(connected, "WifiInfoType=ENCRYPTION").1.as_deref(), Some("AES"));
        assert_eq!(eval(connected, "WifiInfoType=AUTH").1.as_deref(), Some("WPA2-Personal"));
        assert_eq!(eval(connected, "WifiInfoType=PHY").1.as_deref(), Some("802.11ac"));
        assert_eq!(eval(connected, "WifiInfoType=TXRATE").0, 650e6);
        assert_eq!(eval(connected, "WifiInfoType=LIST\nWifiListLimit=2").1.as_deref(), Some("Corp\nHome:5G\n"));
        assert_eq!(eval(disconnected, "WifiInfoType=SSID").1.as_deref(), Some("-1"));
        assert_eq!(eval(disconnected, "WifiInfoType=QUALITY"), (0.0, None));
        assert_eq!(eval(|| Snapshot::default(), "WifiInfoType=SSID"), (0.0, None));
        assert_eq!(eval(connected, "WifiInfoType=SSID\nWifiIntfID=3"), (0.0, None));
        assert_eq!(eval(connected, "WifiInfoType=Bogus"), (0.0, None));
    }
}
