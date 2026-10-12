//! PingPlugin: round-trip time in milliseconds to `DestAddress`, measured with
//! `ping -c 1` on a worker thread (no shell, the address is validated first).
//! Like Rainmeter, a new ping starts every `UpdateRate` updates and only when
//! the previous one has finished; a lost ping gives `TimeoutValue`.

use super::bgshared::Bg;
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::time::Duration;

/// Probe: `(address, timeout_ms)` -> round-trip ms, `None` on loss/timeout.
pub type Probe = fn(&str, u64) -> Option<f64>;

/// Hostnames and IP literals only: no whitespace, no leading `-`.
pub fn valid_address(a: &str) -> bool {
    !a.is_empty()
        && a.len() <= 253
        && !a.starts_with('-')
        && a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_' | '%'))
}

/// Round-trip time of a `ping` reply: `time=12.3 ms` / `time<1 ms`.
pub fn parse_rtt(out: &str) -> Option<f64> {
    for l in out.lines() {
        if let Some(pos) = l.find("time=").map(|p| (p, 5)).or_else(|| l.find("time<").map(|p| (p, 5))) {
            let rest = &l[pos.0 + pos.1..];
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            if let Ok(v) = num.parse::<f64>() {
                return Some(if l[pos.0..].starts_with("time<") { 0.0 } else { v.round() });
            }
        }
    }
    None
}

fn real_probe(addr: &str, timeout_ms: u64) -> Option<f64> {
    if !valid_address(addr) {
        return None;
    }
    let secs = timeout_ms.div_ceil(1000).max(1).to_string();
    let out = super::extcmd::run_capture("ping", &["-n", "-c", "1", "-W", &secs, "--", addr], Duration::from_millis(timeout_ms + 2000))?;
    parse_rtt(&out)
}

#[derive(Clone, Default)]
struct Result_ {
    value: f64,
    finished: bool,
}

pub struct Ping {
    dest: String,
    rate: i64,
    timeout: u64,
    timeout_value: f64,
    finish: String,
    counter: i64,
    state: Bg<Result_>,
    probe: Probe,
    log: Vec<String>,
}

impl Ping {
    pub fn new() -> Ping {
        Ping::with_probe(real_probe)
    }

    pub fn with_probe(probe: Probe) -> Ping {
        Ping {
            dest: String::new(),
            rate: 32,
            timeout: 30000,
            timeout_value: 30000.0,
            finish: String::new(),
            counter: 0,
            state: Bg::new(Result_ { value: 0.0, finished: false }),
            probe,
            log: Vec::new(),
        }
    }
}

impl Default for Ping {
    fn default() -> Self {
        Ping::new()
    }
}

impl MeasureImpl for Ping {
    fn read_options(&mut self, opt: &OptionReader) {
        self.dest = opt.string("DestAddress", "").trim().to_string();
        self.rate = opt.int("UpdateRate", 32).max(1);
        self.timeout = opt.int("Timeout", 30000).clamp(1, 600_000) as u64;
        self.timeout_value = opt.float("TimeoutValue", 30000.0);
        self.finish = opt.action("FinishAction");
        if !self.dest.is_empty() && !valid_address(&self.dest) {
            self.log.push(format!("Ping: invalid DestAddress \"{}\"", self.dest));
            self.dest.clear();
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        if !self.dest.is_empty() && self.counter == 0 && !self.state.running() {
            let (dest, timeout, tv, probe) = (self.dest.clone(), self.timeout, self.timeout_value, self.probe);
            self.state.start(Duration::ZERO, move || Result_ { value: probe(&dest, timeout).unwrap_or(tv), finished: true });
        }
        self.counter += 1;
        if self.counter >= self.rate {
            self.counter = 0;
        }
        self.state.get().value
    }

    fn take_actions(&mut self) -> Vec<String> {
        let done = self.state.modify(|r| std::mem::take(&mut r.finished)).unwrap_or(false);
        if done && !self.finish.is_empty() {
            vec![self.finish.clone()]
        } else {
            Vec::new()
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    #[test]
    fn address_validation() {
        for ok in ["8.8.8.8", "example.com", "::1", "fe80::1%eth0", "my_host-1.lan"] {
            assert!(valid_address(ok), "{ok}");
        }
        for bad in ["", "-c 1", "a b", "x;rm -rf", "$(id)", "a|b", "`id`", "http://x"] {
            assert!(!valid_address(bad), "{bad}");
        }
    }

    #[test]
    fn rtt_parsing() {
        assert_eq!(parse_rtt("64 bytes from 1.1.1.1: icmp_seq=1 ttl=58 time=12.4 ms\n"), Some(12.0));
        assert_eq!(parse_rtt("64 bytes from ::1: icmp_seq=1 ttl=64 time=0.034 ms"), Some(0.0));
        assert_eq!(parse_rtt("Reply from x: time<1ms"), Some(0.0));
        assert_eq!(parse_rtt("100% packet loss"), None);
        assert_eq!(parse_rtt(""), None);
    }

    fn ok_probe(addr: &str, _t: u64) -> Option<f64> {
        (addr == "good.example").then_some(42.0)
    }

    fn wait(m: &mut Ping, sk: &crate::measures::testutil::TestSkin) -> f64 {
        for _ in 0..200 {
            let v = ctx_run(sk, |c| m.update(c));
            if !m.state.running() {
                return ctx_run(sk, |c| m.update(c));
            }
            std::thread::sleep(Duration::from_millis(5));
            let _ = v;
        }
        panic!("probe did not finish")
    }

    #[test]
    fn value_timeout_and_finish_action() {
        let sk = skin("[A]\nMeasure=Plugin\nDestAddress=good.example\nFinishAction=[!Log done]\nUpdateRate=2\n[B]\nMeasure=Plugin\nDestAddress=bad.example\nTimeoutValue=-1\n[C]\nMeasure=Plugin\nDestAddress=-c 3\n");
        let mut a = Ping::with_probe(ok_probe);
        a.read_options(&OptionReader { parser: &sk.parser, section: "A", sv: None });
        assert_eq!(wait(&mut a, &sk), 42.0);
        assert_eq!(a.take_actions(), ["[!Log done]"]);
        assert!(a.take_actions().is_empty());
        let mut b = Ping::with_probe(ok_probe);
        b.read_options(&OptionReader { parser: &sk.parser, section: "B", sv: None });
        assert_eq!(wait(&mut b, &sk), -1.0);
        let mut c = Ping::with_probe(ok_probe);
        c.read_options(&OptionReader { parser: &sk.parser, section: "C", sv: None });
        assert_eq!(ctx_run(&sk, |x| c.update(x)), 0.0);
        assert_eq!(c.take_log().len(), 1);
    }
}
