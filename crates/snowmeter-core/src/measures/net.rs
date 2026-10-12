//! NetIn / NetOut / NetTotal (`/proc/net/dev`).

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

#[derive(Clone, Debug, PartialEq)]
pub struct IfStat {
    pub name: String,
    pub rx: u64,
    pub tx: u64,
}

/// Parse `/proc/net/dev` (receive bytes = column 1, transmit bytes = column 9).
pub fn parse_net_dev(text: &str) -> Vec<IfStat> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let nums: Vec<u64> = rest.split_whitespace().map(|x| x.parse().unwrap_or(0)).collect();
        if nums.len() < 9 {
            continue;
        }
        out.push(IfStat { name: name.trim().to_string(), rx: nums[0], tx: nums[8] });
    }
    out
}

/// Interface of the default route (lowest metric `00000000` destination that is up).
pub fn parse_route_best(text: &str) -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 7 || f[1] != "00000000" {
            continue;
        }
        let flags = u32::from_str_radix(f[3], 16).unwrap_or(0);
        if flags & 1 == 0 {
            continue;
        }
        let metric: u64 = f[6].parse().unwrap_or(u64::MAX);
        if best.as_ref().is_none_or(|(m, _)| metric < *m) {
            best = Some((metric, f[0].to_string()));
        }
    }
    best.map(|(_, n)| n)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NetKind {
    In,
    Out,
    Total,
}

type Source = Box<dyn FnMut() -> Option<String>>;
type IndexLookup = Box<dyn Fn(&str) -> Option<u32>>;

pub struct NetMeasure {
    kind: NetKind,
    interface: String,
    cumulative: bool,
    use_bits: bool,
    max_bytes: f64,
    dev: Source,
    route: Source,
    ifindex: IndexLookup,
    prev: Option<(u64, u64)>,
    cum: (u64, u64),
    value: f64,
}

impl NetMeasure {
    pub fn new(kind: NetKind) -> NetMeasure {
        NetMeasure::with_sources(
            kind,
            Box::new(|| std::fs::read_to_string("/proc/net/dev").ok()),
            Box::new(|| std::fs::read_to_string("/proc/net/route").ok()),
            Box::new(|name| std::fs::read_to_string(format!("/sys/class/net/{name}/ifindex")).ok()?.trim().parse().ok()),
        )
    }

    pub fn with_sources(kind: NetKind, dev: Source, route: Source, ifindex: IndexLookup) -> NetMeasure {
        NetMeasure {
            kind,
            interface: "best".to_string(),
            cumulative: false,
            use_bits: false,
            max_bytes: 0.0,
            dev,
            route,
            ifindex,
            prev: None,
            cum: (0, 0),
            value: 0.0,
        }
    }

    /// Byte counters (rx, tx) of the selected interface(s).
    fn select(&mut self, stats: &[IfStat]) -> (u64, u64) {
        let sum = |it: &mut dyn Iterator<Item = &IfStat>| it.fold((0u64, 0u64), |a, s| (a.0.wrapping_add(s.rx), a.1.wrapping_add(s.tx)));
        let sel = self.interface.clone();
        if let Ok(n) = sel.parse::<u32>() {
            if n == 0 {
                return sum(&mut stats.iter().filter(|s| s.name != "lo"));
            }
            return sum(&mut stats.iter().filter(|s| (self.ifindex)(&s.name) == Some(n)));
        }
        if sel == "best" {
            return match (self.route)().and_then(|t| parse_route_best(&t)) {
                Some(name) => sum(&mut stats.iter().filter(|s| s.name == name)),
                None => sum(&mut stats.iter().filter(|s| s.name != "lo")),
            };
        }
        sum(&mut stats.iter().filter(|s| s.name.eq_ignore_ascii_case(&sel)))
    }
}

impl MeasureImpl for NetMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let iface = opt.string("Interface", "Best").trim().to_ascii_lowercase();
        if iface != self.interface {
            self.prev = None;
        }
        self.interface = if iface.is_empty() { "best".to_string() } else { iface };
        self.cumulative = opt.bool("Cumulative", false);
        self.use_bits = opt.bool("UseBits", false);

        let (own, glob_in, glob_out) = match self.kind {
            NetKind::In => ("NetInSpeed", "NetInSpeed", ""),
            NetKind::Out => ("NetOutSpeed", "NetOutSpeed", ""),
            NetKind::Total => ("NetTotalSpeed", "NetInSpeed", "NetOutSpeed"),
        };
        let mut speed = if opt.has("MaxValue") { opt.float("MaxValue", 0.0) } else { opt.float(own, 0.0) };
        if speed <= 0.0 {
            let g = |k: &str| if k.is_empty() { 0.0 } else { opt.parser.read_float("Rainmeter", k, 0.0, None) };
            speed = g(glob_in) + g(glob_out);
        }
        self.max_bytes = if self.use_bits { speed } else { speed / 8.0 };
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let Some(text) = (self.dev)() else { return self.value };
        let stats = parse_net_dev(&text);
        let cur = self.select(&stats);
        let (d_in, d_out) = match self.prev {
            None => (0, 0),
            Some(p) => (cur.0.checked_sub(p.0).unwrap_or(0), cur.1.checked_sub(p.1).unwrap_or(0)),
        };
        self.prev = Some(cur);
        self.cum = (self.cum.0.wrapping_add(d_in), self.cum.1.wrapping_add(d_out));
        let (a, b) = if self.cumulative { self.cum } else { (d_in, d_out) };
        let bytes = match self.kind {
            NetKind::In => a as f64,
            NetKind::Out => b as f64,
            NetKind::Total => a as f64 + b as f64,
        };
        self.value = if self.use_bits { bytes * 8.0 } else { bytes };
        self.value
    }

    fn default_max(&self) -> f64 {
        0.0
    }

    fn forced_max(&self) -> Option<f64> {
        (self.max_bytes > 0.0).then_some(self.max_bytes)
    }

    fn auto_max(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn dev(lo: u64, eth_rx: u64, eth_tx: u64, wl_rx: u64, wl_tx: u64) -> String {
        format!(
            "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    lo: {lo} 10 0 0 0 0 0 0 {lo} 10 0 0 0 0 0 0\n  eth0: {eth_rx} 10 0 0 0 0 0 0 {eth_tx} 10 0 0 0 0 0 0\n wlan0: {wl_rx} 10 0 0 0 0 0 0 {wl_tx} 10 0 0 0 0 0 0\n"
        )
    }

    const ROUTE: &str = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\nwlan0\t00000000\t0100A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\neth0\t00000000\t0100A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\neth0\t0000A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";

    #[test]
    fn parses_net_dev_and_route() {
        let s = parse_net_dev(&dev(5, 100, 200, 300, 400));
        assert_eq!(s.len(), 3);
        assert_eq!(s[1], IfStat { name: "eth0".into(), rx: 100, tx: 200 });
        assert_eq!(parse_route_best(ROUTE).as_deref(), Some("eth0"));
        assert_eq!(parse_route_best("Iface\n"), None);
    }

    fn measure(kind: NetKind, ini: &str) -> (crate::measures::testutil::TestSkin, NetMeasure, Rc<RefCell<String>>) {
        let text = Rc::new(RefCell::new(dev(0, 1000, 500, 10, 20)));
        let t = text.clone();
        let mut n = NetMeasure::with_sources(
            kind,
            Box::new(move || Some(t.borrow().clone())),
            Box::new(|| Some(ROUTE.to_string())),
            Box::new(|name| match name {
                "eth0" => Some(2),
                "wlan0" => Some(3),
                _ => None,
            }),
        );
        let s = skin(ini);
        n.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        (s, n, text)
    }

    #[test]
    fn value_is_per_update_delta_first_is_zero() {
        let (s, mut n, text) = measure(NetKind::In, "[M]\nMeasure=NetIn\n");
        assert_eq!(ctx_run(&s, |c| n.update(c)), 0.0);
        *text.borrow_mut() = dev(0, 1600, 900, 50, 60);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 600.0); // best = eth0 only
        *text.borrow_mut() = dev(0, 1000, 900, 50, 60);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 0.0); // counter went backwards
    }

    #[test]
    fn out_total_and_bits() {
        let (s, mut n, text) = measure(NetKind::Total, "[M]\nMeasure=NetTotal\nUseBits=1\n");
        ctx_run(&s, |c| n.update(c));
        *text.borrow_mut() = dev(0, 1100, 700, 0, 0);
        assert_eq!(ctx_run(&s, |c| n.update(c)), (100.0 + 200.0) * 8.0);
        let (s, mut n, text) = measure(NetKind::Out, "[M]\nMeasure=NetOut\n");
        ctx_run(&s, |c| n.update(c));
        *text.borrow_mut() = dev(0, 1100, 700, 0, 0);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 200.0);
    }

    #[test]
    fn interface_selection() {
        // all interfaces except lo
        let (s, mut n, text) = measure(NetKind::In, "[M]\nMeasure=NetIn\nInterface=0\n");
        ctx_run(&s, |c| n.update(c));
        *text.borrow_mut() = dev(999, 1100, 500, 30, 20);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 120.0);
        // by name, case-insensitive
        let (s, mut n, text) = measure(NetKind::In, "[M]\nMeasure=NetIn\nInterface=WLAN0\n");
        ctx_run(&s, |c| n.update(c));
        *text.borrow_mut() = dev(0, 5000, 500, 30, 20);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 20.0);
        // by ifindex
        let (s, mut n, text) = measure(NetKind::In, "[M]\nMeasure=NetIn\nInterface=3\n");
        ctx_run(&s, |c| n.update(c));
        *text.borrow_mut() = dev(0, 5000, 500, 30, 20);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 20.0);
        // unknown name -> 0
        let (s, mut n, text) = measure(NetKind::In, "[M]\nMeasure=NetIn\nInterface=nope\n");
        ctx_run(&s, |c| n.update(c));
        *text.borrow_mut() = dev(0, 5000, 500, 30, 20);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 0.0);
    }

    #[test]
    fn cumulative_accumulates_positive_deltas() {
        let (s, mut n, text) = measure(NetKind::In, "[M]\nMeasure=NetIn\nCumulative=1\n");
        ctx_run(&s, |c| n.update(c));
        *text.borrow_mut() = dev(0, 1100, 500, 0, 0);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 100.0);
        *text.borrow_mut() = dev(0, 1150, 500, 0, 0);
        assert_eq!(ctx_run(&s, |c| n.update(c)), 150.0);
    }

    #[test]
    fn max_speed_rules() {
        // bits/s -> bytes/s unless UseBits
        let (_s, n, _) = measure(NetKind::In, "[M]\nMeasure=NetIn\nMaxValue=8000\n");
        assert_eq!(n.forced_max(), Some(1000.0));
        let (_s, n, _) = measure(NetKind::In, "[M]\nMeasure=NetIn\nMaxValue=8000\nUseBits=1\n");
        assert_eq!(n.forced_max(), Some(8000.0));
        let (_s, n, _) = measure(NetKind::Out, "[M]\nMeasure=NetOut\nNetOutSpeed=16000\n");
        assert_eq!(n.forced_max(), Some(2000.0));
        let (_s, n, _) = measure(NetKind::Total, "[Rainmeter]\nNetInSpeed=8000\nNetOutSpeed=8000\n[M]\nMeasure=NetTotal\n");
        assert_eq!(n.forced_max(), Some(2000.0));
        let (_s, n, _) = measure(NetKind::In, "[M]\nMeasure=NetIn\n");
        assert_eq!(n.forced_max(), None);
        assert!(n.auto_max());
    }

    #[test]
    fn real_proc_net_dev_sanity() {
        let s = skin("[M]\nMeasure=NetIn\n");
        let mut n = NetMeasure::new(NetKind::Total);
        n.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(ctx_run(&s, |c| n.update(c)), 0.0);
        assert!(ctx_run(&s, |c| n.update(c)) >= 0.0);
    }
}
