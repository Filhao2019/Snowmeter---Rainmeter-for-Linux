//! SysInfo (`SysInfoType`, `SysInfoData`) on Linux.

use super::{MeasureCtx, MeasureImpl};
use crate::env::Rect;
use crate::parser::OptionReader;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Adapter {
    pub name: String,
    pub index: u32,
    pub up: bool,
    pub ipv4: Option<String>,
    pub mask: Option<String>,
}

/// Where SysInfo gets its data; replaced by fakes in tests.
pub struct SysSource {
    pub read: Box<dyn Fn(&str) -> Option<String>>,
    pub var: Box<dyn Fn(&str) -> Option<String>>,
    pub adapters: Box<dyn Fn() -> Vec<Adapter>>,
}

impl SysSource {
    pub fn real() -> SysSource {
        SysSource {
            read: Box::new(|p| std::fs::read_to_string(p).ok()),
            var: Box::new(|n| std::env::var(n).ok()),
            adapters: Box::new(list_adapters),
        }
    }
}

/// Interfaces via `getifaddrs`.
pub fn list_adapters() -> Vec<Adapter> {
    let mut out: Vec<Adapter> = Vec::new();
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills a linked list that we free with freeifaddrs.
    unsafe {
        if libc::getifaddrs(&mut ifap) != 0 {
            return out;
        }
        let mut cur = ifap;
        while !cur.is_null() {
            let a = &*cur;
            cur = a.ifa_next;
            if a.ifa_name.is_null() {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(a.ifa_name).to_string_lossy().into_owned();
            let pos = match out.iter().position(|x| x.name == name) {
                Some(p) => p,
                None => {
                    let index = libc::if_nametoindex(a.ifa_name);
                    out.push(Adapter { name, index, up: a.ifa_flags & libc::IFF_UP as u32 != 0, ..Default::default() });
                    out.len() - 1
                }
            };
            if a.ifa_addr.is_null() || (*a.ifa_addr).sa_family as i32 != libc::AF_INET {
                continue;
            }
            let ip = |sa: *const libc::sockaddr| {
                let sin = &*(sa as *const libc::sockaddr_in);
                std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)).to_string()
            };
            out[pos].ipv4 = Some(ip(a.ifa_addr));
            if !a.ifa_netmask.is_null() {
                out[pos].mask = Some(ip(a.ifa_netmask));
            }
        }
        libc::freeifaddrs(ifap);
    }
    out
}

/// Gateway (dotted quad) of the default route of `iface` from `/proc/net/route`.
pub fn parse_gateway(route: &str, iface: &str) -> Option<String> {
    for line in route.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 || f[0] != iface || f[1] != "00000000" {
            continue;
        }
        let gw = u32::from_str_radix(f[2], 16).ok()?;
        return Some(std::net::Ipv4Addr::from(gw.swap_bytes()).to_string());
    }
    None
}

/// First `nameserver` of `resolv.conf`.
pub fn parse_dns(resolv: &str) -> Option<String> {
    resolv.lines().find_map(|l| l.trim().strip_prefix("nameserver").map(|r| r.trim().to_string())).filter(|s| !s.is_empty())
}

/// `PRETTY_NAME` of `os-release`.
pub fn parse_os_release(text: &str) -> Option<String> {
    let l = text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
    Some(l.trim().trim_matches('"').to_string())
}

pub struct SysInfo {
    ty: String,
    data: String,
    src: SysSource,
    value: f64,
    string: String,
    log: Vec<String>,
}

impl Default for SysInfo {
    fn default() -> Self {
        SysInfo::new()
    }
}

impl SysInfo {
    pub fn new() -> SysInfo {
        SysInfo::with_source(SysSource::real())
    }

    pub fn with_source(src: SysSource) -> SysInfo {
        SysInfo { ty: String::new(), data: String::new(), src, value: 0.0, string: String::new(), log: Vec::new() }
    }

    fn read(&self, p: &str) -> Option<String> {
        (self.src.read)(p)
    }

    /// The adapter selected by `SysInfoData` (`Best`, an ifindex, or a name).
    fn adapter(&self, list: &[Adapter]) -> Option<Adapter> {
        let d = self.data.trim();
        if d.is_empty() || d.eq_ignore_ascii_case("best") {
            let route = self.read("/proc/net/route").unwrap_or_default();
            let best = crate::measures::net::parse_route_best(&route);
            return best.and_then(|n| list.iter().find(|a| a.name == n).cloned()).or_else(|| {
                list.iter().find(|a| a.up && a.name != "lo" && a.ipv4.is_some()).cloned()
            });
        }
        if let Ok(n) = d.parse::<u32>() {
            return list.iter().find(|a| a.index == n).cloned();
        }
        list.iter().find(|a| a.name.eq_ignore_ascii_case(d)).cloned()
    }

    fn monitor<'a>(&self, mons: &'a [Rect]) -> Option<&'a Rect> {
        match self.data.trim().parse::<i64>() {
            Ok(n) if n > 0 => mons.get(n as usize - 1),
            _ => mons.first(),
        }
    }

    fn compute(&mut self, monitors: &[Rect], work: &[Rect]) {
        self.value = 0.0;
        self.string.clear();
        let t = self.ty.as_str();
        let trim = |s: Option<String>| s.map(|s| s.trim().to_string()).unwrap_or_default();
        match t {
            "COMPUTER_NAME" | "HOST_NAME" => self.string = trim(self.read("/proc/sys/kernel/hostname")),
            "DOMAIN_NAME" => {
                let d = trim(self.read("/proc/sys/kernel/domainname"));
                self.string = if d == "(none)" { String::new() } else { d };
            }
            "USER_NAME" => self.string = (self.src.var)("USER").or_else(|| (self.src.var)("LOGNAME")).unwrap_or_default(),
            "OS_VERSION" | "OS_PRODUCT_NAME" => {
                self.string = self.read("/etc/os-release").and_then(|t| parse_os_release(&t)).unwrap_or_else(|| "Linux".into())
            }
            "OS_BITS" => self.value = usize::BITS as f64,
            "PAGESIZE" => self.value = 4096.0,
            "NUM_MONITORS" => self.value = monitors.len() as f64,
            "SCREEN_SIZE" | "WORK_AREA" => {
                let r = if t == "SCREEN_SIZE" { monitors.first() } else { work.first() };
                let (w, h) = r.map(|r| (r.w as i64, r.h as i64)).unwrap_or((0, 0));
                self.string = format!("{w} x {h}");
            }
            "SCREEN_WIDTH" | "SCREEN_HEIGHT" => {
                self.value = self.monitor(monitors).map(|r| if t == "SCREEN_WIDTH" { r.w } else { r.h }).unwrap_or(0.0)
            }
            "WORK_AREA_LEFT" | "WORK_AREA_TOP" | "WORK_AREA_WIDTH" | "WORK_AREA_HEIGHT" => {
                self.value = self
                    .monitor(work)
                    .map(|r| match t {
                        "WORK_AREA_LEFT" => r.x,
                        "WORK_AREA_TOP" => r.y,
                        "WORK_AREA_WIDTH" => r.w,
                        _ => r.h,
                    })
                    .unwrap_or(0.0)
            }
            "VIRTUAL_SCREEN_LEFT" | "VIRTUAL_SCREEN_TOP" | "VIRTUAL_SCREEN_WIDTH" | "VIRTUAL_SCREEN_HEIGHT" => {
                let l = monitors.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
                let tp = monitors.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
                let rt = monitors.iter().map(|r| r.x + r.w).fold(f64::NEG_INFINITY, f64::max);
                let bt = monitors.iter().map(|r| r.y + r.h).fold(f64::NEG_INFINITY, f64::max);
                self.value = if monitors.is_empty() {
                    0.0
                } else {
                    match t {
                        "VIRTUAL_SCREEN_LEFT" => l,
                        "VIRTUAL_SCREEN_TOP" => tp,
                        "VIRTUAL_SCREEN_WIDTH" => rt - l,
                        _ => bt - tp,
                    }
                };
            }
            "DNS_SERVER" => self.string = self.read("/etc/resolv.conf").and_then(|t| parse_dns(&t)).unwrap_or_default(),
            "INTERNET_CONNECTIVITY" | "INTERNET_CONNECTIVITY_V4" | "INTERNET_CONNECTIVITY_V6" | "LAN_CONNECTIVITY" | "LAN_CONNECTIVITY_V4"
            | "LAN_CONNECTIVITY_V6" => {
                let list = (self.src.adapters)();
                let lan4 = list.iter().any(|a| a.up && a.name != "lo" && a.ipv4.is_some());
                let route4 = crate::measures::net::parse_route_best(&self.read("/proc/net/route").unwrap_or_default()).is_some();
                let v6 = self
                    .read("/proc/net/ipv6_route")
                    .map(|t| t.lines().any(|l| l.starts_with("00000000000000000000000000000000 00") && !l.trim_end().ends_with(" lo")))
                    .unwrap_or(false);
                let on = match t {
                    "INTERNET_CONNECTIVITY" => lan4 && route4 || v6,
                    "INTERNET_CONNECTIVITY_V4" => lan4 && route4,
                    "INTERNET_CONNECTIVITY_V6" => v6,
                    "LAN_CONNECTIVITY" => lan4 || v6,
                    "LAN_CONNECTIVITY_V4" => lan4,
                    _ => v6,
                };
                self.value = if on { 1.0 } else { -1.0 };
            }
            "ADAPTER_DESCRIPTION" | "ADAPTER_ALIAS" | "ADAPTER_GUID" | "ADAPTER_TYPE" | "ADAPTER_STATE" | "ADAPTER_STATUS"
            | "ADAPTER_TRANSMIT_SPEED" | "ADAPTER_RECEIVE_SPEED" | "MAC_ADDRESS" | "NET_MASK" | "IP_ADDRESS" | "GATEWAY_ADDRESS"
            | "GATEWAY_ADDRESS_V4" | "GATEWAY_ADDRESS_V6" => {
                let list = (self.src.adapters)();
                let Some(a) = self.adapter(&list) else { return };
                match t {
                    "ADAPTER_DESCRIPTION" | "ADAPTER_ALIAS" => self.string = a.name.clone(),
                    "ADAPTER_GUID" => self.string = a.name.clone(),
                    "ADAPTER_TYPE" => {
                        let ty = trim(self.read(&format!("/sys/class/net/{}/type", a.name))).parse::<u32>().unwrap_or(0);
                        self.value = match ty {
                            1 => 6.0,
                            772 => 24.0,
                            _ => 0.0,
                        };
                        if self.read(&format!("/sys/class/net/{}/wireless", a.name)).is_some() {
                            self.value = 71.0;
                        }
                    }
                    "ADAPTER_STATE" | "ADAPTER_STATUS" => {
                        let st = trim(self.read(&format!("/sys/class/net/{}/operstate", a.name)));
                        self.value = if st == "up" || (st == "unknown" && a.up) { 1.0 } else { 0.0 };
                    }
                    "ADAPTER_TRANSMIT_SPEED" | "ADAPTER_RECEIVE_SPEED" => {
                        let mbit: f64 = trim(self.read(&format!("/sys/class/net/{}/speed", a.name))).parse().unwrap_or(0.0);
                        self.value = mbit.max(0.0) * 1_000_000.0;
                    }
                    "MAC_ADDRESS" => {
                        self.string = trim(self.read(&format!("/sys/class/net/{}/address", a.name))).replace(':', "-").to_ascii_uppercase()
                    }
                    "NET_MASK" => self.string = a.mask.unwrap_or_default(),
                    "IP_ADDRESS" => self.string = a.ipv4.unwrap_or_default(),
                    "GATEWAY_ADDRESS_V6" => {}
                    _ => self.string = parse_gateway(&self.read("/proc/net/route").unwrap_or_default(), &a.name).unwrap_or_default(),
                }
            }
            "TIMEZONE_ISDST" | "TIMEZONE_BIAS" | "TIMEZONE_STANDARD_BIAS" | "TIMEZONE_DAYLIGHT_BIAS" => {
                let (off, dst) = local_offset();
                self.value = match t {
                    "TIMEZONE_ISDST" => f64::from(dst),
                    "TIMEZONE_BIAS" => (-off / 60) as f64,
                    _ => 0.0,
                };
            }
            _ => {}
        }
    }
}

/// `(utc offset seconds, is_dst)` of the local zone now.
fn local_offset() -> (i64, bool) {
    // SAFETY: localtime_r writes into the zeroed tm we pass.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        (tm.tm_gmtoff, tm.tm_isdst > 0)
    }
}

impl MeasureImpl for SysInfo {
    fn read_options(&mut self, opt: &OptionReader) {
        self.ty = opt.string("SysInfoType", "").trim().to_ascii_uppercase();
        self.data = opt.string("SysInfoData", "");
        if self.ty.is_empty() {
            self.log.push("SysInfo: SysInfoType is not valid".into());
        }
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        let (m, w) = (&ctx.env.monitors, &ctx.env.work_areas);
        self.compute(m, w);
        self.value
    }

    fn string_value(&self) -> Option<String> {
        // IP_ADDRESS always has a (possibly empty) string.
        (!self.string.is_empty() || self.ty == "IP_ADDRESS").then(|| self.string.clone())
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fake(files: &[(&str, &str)], adapters: Vec<Adapter>) -> SysSource {
        let map: HashMap<String, String> = files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        SysSource {
            read: Box::new(move |p| map.get(p).cloned()),
            var: Box::new(|n| (n == "USER").then(|| "bob".to_string())),
            adapters: Box::new(move || adapters.clone()),
        }
    }

    fn eth() -> Vec<Adapter> {
        vec![
            Adapter { name: "lo".into(), index: 1, up: true, ipv4: Some("127.0.0.1".into()), mask: Some("255.0.0.0".into()) },
            Adapter { name: "eth0".into(), index: 2, up: true, ipv4: Some("192.168.1.20".into()), mask: Some("255.255.255.0".into()) },
        ]
    }

    const ROUTE: &str = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
        eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\n\
        eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\n";

    fn run(ty: &str, data: &str, files: &[(&str, &str)]) -> (f64, String) {
        let mut s = SysInfo::with_source(fake(files, eth()));
        s.ty = ty.to_string();
        s.data = data.to_string();
        let mons = vec![Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 }, Rect { x: 1920.0, y: 0.0, w: 1280.0, h: 1024.0 }];
        let work = vec![Rect { x: 0.0, y: 30.0, w: 1920.0, h: 1050.0 }];
        s.compute(&mons, &work);
        (s.value, s.string.clone())
    }

    #[test]
    fn names_and_os() {
        let f = [("/proc/sys/kernel/hostname", "box\n"), ("/etc/os-release", "NAME=X\nPRETTY_NAME=\"Test OS 1\"\n"), ("/proc/sys/kernel/domainname", "(none)\n")];
        assert_eq!(run("COMPUTER_NAME", "", &f).1, "box");
        assert_eq!(run("HOST_NAME", "", &f).1, "box");
        assert_eq!(run("DOMAIN_NAME", "", &f).1, "");
        assert_eq!(run("USER_NAME", "", &f).1, "bob");
        assert_eq!(run("OS_VERSION", "", &f).1, "Test OS 1");
        assert_eq!(run("OS_BITS", "", &f).0, 64.0);
    }

    #[test]
    fn network_values_for_best_and_named_adapters() {
        let f = [("/proc/net/route", ROUTE), ("/sys/class/net/eth0/address", "aa:bb:cc:00:11:22\n"), ("/etc/resolv.conf", "# x\nnameserver 9.9.9.9\n")];
        assert_eq!(run("IP_ADDRESS", "", &f).1, "192.168.1.20");
        assert_eq!(run("NET_MASK", "Best", &f).1, "255.255.255.0");
        assert_eq!(run("GATEWAY_ADDRESS", "", &f).1, "192.168.1.1");
        assert_eq!(run("MAC_ADDRESS", "2", &f).1, "AA-BB-CC-00-11-22");
        assert_eq!(run("IP_ADDRESS", "lo", &f).1, "127.0.0.1");
        assert_eq!(run("ADAPTER_DESCRIPTION", "", &f).1, "eth0");
        assert_eq!(run("DNS_SERVER", "", &f).1, "9.9.9.9");
        assert_eq!(run("IP_ADDRESS", "99", &f).1, "");
        assert_eq!(run("INTERNET_CONNECTIVITY", "", &f).0, 1.0);
        assert_eq!(run("INTERNET_CONNECTIVITY", "", &[]).0, -1.0);
        assert_eq!(run("LAN_CONNECTIVITY_V4", "", &[]).0, 1.0);
    }

    #[test]
    fn screen_values() {
        assert_eq!(run("NUM_MONITORS", "", &[]).0, 2.0);
        assert_eq!(run("SCREEN_SIZE", "", &[]).1, "1920 x 1080");
        assert_eq!(run("WORK_AREA", "", &[]).1, "1920 x 1050");
        assert_eq!(run("SCREEN_WIDTH", "", &[]).0, 1920.0);
        assert_eq!(run("SCREEN_WIDTH", "2", &[]).0, 1280.0);
        assert_eq!(run("WORK_AREA_TOP", "", &[]).0, 30.0);
        assert_eq!(run("VIRTUAL_SCREEN_WIDTH", "", &[]).0, 3200.0);
    }

    #[test]
    fn gateway_and_dns_parsers() {
        assert_eq!(parse_gateway(ROUTE, "eth0").as_deref(), Some("192.168.1.1"));
        assert_eq!(parse_gateway(ROUTE, "wlan0"), None);
        assert_eq!(parse_dns("search x\nnameserver 1.1.1.1\nnameserver 8.8.8.8"), Some("1.1.1.1".into()));
    }

    #[test]
    fn real_adapter_listing_has_loopback() {
        assert!(list_adapters().iter().any(|a| a.name == "lo" && a.ipv4.as_deref() == Some("127.0.0.1")));
    }
}
