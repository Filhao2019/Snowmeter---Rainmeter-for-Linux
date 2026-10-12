//! AudioLevel plugin on Linux (Rainmeter `MeasureAudioLevel.cpp`).
//!
//! The Windows plugin reads WASAPI loopback/capture buffers. Here a background
//! thread runs `parec` (or `pw-record`) on the default sink monitor / default
//! source and feeds the raw float PCM to the same DSP the Windows plugin uses
//! (envelope-filtered RMS and peak, Hann-windowed FFT with attack/decay,
//! log-spaced bands). The results are published as a snapshot that `update()`
//! reads, so the GTK thread never waits for audio.
//!
//! One capture process is shared by every parent measure with the same
//! (port, device); each parent keeps its own DSP state because FFT size,
//! bands and envelopes are per-parent options. The capture stops a few
//! seconds after the last parent is gone and restarts with a backoff when the
//! tool dies.

use super::bgshared::{lookup, publish};
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::collections::HashMap;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// Sample rate and channel count requested from the capture tool.
pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// Rainmeter's `MAX_CHANNELS`; index 8 is "Sum".
const MAX_CHANNELS: usize = 8;
const CHANNEL_SUM: usize = MAX_CHANNELS;
/// No audio for this long zeroes the levels (EMPTY_TIMEOUT).
const EMPTY_TIMEOUT: Duration = Duration::from_millis(500);
/// The capture process is stopped this long after its last parent is gone.
const IDLE_STOP: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------- DSP

/// Options that shape the DSP state (everything but sensitivity and gains).
#[derive(Clone, Debug, PartialEq)]
pub struct DspConfig {
    pub fft_size: usize,
    pub fft_overlap: usize,
    pub bands: usize,
    pub freq_min: f64,
    pub freq_max: f64,
    pub env_rms: [i32; 2],
    pub env_peak: [i32; 2],
    pub env_fft: [i32; 2],
}

impl Default for DspConfig {
    fn default() -> Self {
        DspConfig {
            fft_size: 0,
            fft_overlap: 0,
            bands: 0,
            freq_min: 20.0,
            freq_max: 20000.0,
            env_rms: [300, 300],
            env_peak: [50, 2500],
            env_fft: [300, 300],
        }
    }
}

/// Filter constant for an envelope time in ms at `rate` updates per second:
/// `exp(log10(0.01) / (rate * ms * 0.001))`.
fn filter_k(rate: f64, ms: i32) -> f32 {
    ((0.01f64).log10() / (rate * ms as f64 * 0.001)).exp() as f32
}

pub struct Dsp {
    cfg: DspConfig,
    rate: u32,
    channels: usize,
    k_rms: [f32; 2],
    k_peak: [f32; 2],
    k_fft: [f32; 2],
    rms: [f32; MAX_CHANNELS],
    peak: [f32; MAX_CHANNELS],
    fft_in: Vec<Vec<f32>>,
    fft_out: Vec<Vec<f32>>,
    window: Vec<f32>,
    buf_w: usize,
    buf_p: usize,
    band_freq: Vec<f32>,
    band_out: Vec<Vec<f32>>,
    plan: Option<Arc<dyn Fft<f32>>>,
    scratch: Vec<Complex<f32>>,
    tmp: Vec<Complex<f32>>,
}

impl Dsp {
    pub fn new(cfg: DspConfig, rate: u32, channels: usize) -> Dsp {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let f = rate as f64;
        let n = cfg.fft_size;
        let hop = if n > 0 { (n - cfg.fft_overlap.min(n - 1)) as f64 } else { 1.0 };
        let k_fft = [filter_k(f / hop, cfg.env_fft[0]), filter_k(f / hop, cfg.env_fft[1])];
        let mut d = Dsp {
            k_rms: [filter_k(f, cfg.env_rms[0]), filter_k(f, cfg.env_rms[1])],
            k_peak: [filter_k(f, cfg.env_peak[0]), filter_k(f, cfg.env_peak[1])],
            k_fft,
            rms: [0.0; MAX_CHANNELS],
            peak: [0.0; MAX_CHANNELS],
            fft_in: Vec::new(),
            fft_out: Vec::new(),
            window: Vec::new(),
            buf_w: 0,
            buf_p: 1,
            band_freq: Vec::new(),
            band_out: Vec::new(),
            plan: None,
            scratch: Vec::new(),
            tmp: Vec::new(),
            rate,
            channels,
            cfg,
        };
        if n > 0 {
            d.fft_in = vec![vec![0.0; n]; channels];
            d.fft_out = vec![vec![0.0; n / 2 + 1]; channels];
            d.buf_p = n - d.cfg.fft_overlap.min(n - 1);
            d.window = (0..n)
                .map(|i| (0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (n as f64 - 1.0)).cos())) as f32)
                .collect();
            let plan = FftPlanner::<f32>::new().plan_fft_forward(n);
            d.scratch = vec![Complex::new(0.0, 0.0); plan.get_inplace_scratch_len()];
            d.tmp = vec![Complex::new(0.0, 0.0); n];
            d.plan = Some(plan);
        }
        let nb = d.cfg.bands;
        if nb > 0 {
            let step = ((d.cfg.freq_max / d.cfg.freq_min).ln() / nb as f64) / 2f64.ln();
            let mut bf = vec![0f32; nb];
            bf[0] = (d.cfg.freq_min * 2f64.powf(step / 2.0)) as f32;
            for i in 1..nb {
                bf[i] = (bf[i - 1] as f64 * 2f64.powf(step)) as f32;
            }
            d.band_freq = bf;
            d.band_out = vec![vec![0.0; nb]; channels];
        }
        d
    }

    /// Zero the levels (no audio arrived for a while).
    pub fn reset(&mut self) {
        self.rms = [0.0; MAX_CHANNELS];
        self.peak = [0.0; MAX_CHANNELS];
        for v in self.fft_in.iter_mut().chain(self.fft_out.iter_mut()).chain(self.band_out.iter_mut()) {
            v.iter_mut().for_each(|x| *x = 0.0);
        }
    }

    /// Feed interleaved samples (`channels` per frame).
    pub fn process(&mut self, s: &[f32]) {
        let ch = self.channels;
        let frames = s.len() / ch;
        for f in 0..frames {
            for c in 0..ch {
                let x = s[f * ch + c];
                let sq = x * x;
                let a = x.abs();
                let r = self.rms[c];
                self.rms[c] = sq + self.k_rms[(sq < r) as usize] * (r - sq);
                let p = self.peak[c];
                self.peak[c] = a + self.k_peak[(a < p) as usize] * (p - a);
            }
        }
        if ch == 1 {
            self.rms[1] = self.rms[0];
            self.peak[1] = self.peak[0];
        }
        let n = self.cfg.fft_size;
        if n == 0 {
            return;
        }
        for f in 0..frames {
            for c in 0..ch {
                self.fft_in[c][self.buf_w] = s[f * ch + c];
            }
            self.buf_w = (self.buf_w + 1) % n;
            self.buf_p -= 1;
            if self.buf_p == 0 {
                self.hop();
                self.buf_p = n - self.cfg.fft_overlap.min(n - 1);
            }
        }
        if self.cfg.bands > 0 {
            self.integrate_bands();
        }
    }

    fn hop(&mut self) {
        let n = self.cfg.fft_size;
        let scalar = (1.0 / (n as f64).sqrt()) as f32;
        let Some(plan) = self.plan.clone() else { return };
        for c in 0..self.channels {
            for i in 0..n {
                let x = self.fft_in[c][(self.buf_w + i) % n] * self.window[i];
                self.tmp[i] = Complex::new(x, 0.0);
            }
            plan.process_with_scratch(&mut self.tmp, &mut self.scratch);
            for bin in 0..=n / 2 {
                let z = self.tmp[bin];
                let x1 = (z.re * z.re + z.im * z.im) * scalar;
                let x0 = self.fft_out[c][bin];
                self.fft_out[c][bin] = x1 + self.k_fft[(x1 < x0) as usize] * (x0 - x1);
            }
        }
    }

    /// Integrate the FFT bins into the log-spaced bands (Rainmeter's loop).
    fn integrate_bands(&mut self) {
        let n = self.cfg.fft_size;
        let nb = self.cfg.bands;
        let df = self.rate as f32 / n as f32;
        let scalar = 2.0 / self.rate as f32;
        for c in 0..self.channels {
            self.band_out[c].iter_mut().for_each(|x| *x = 0.0);
            let mut bin = ((self.cfg.freq_min as f32 / df).round().max(0.0)) as usize;
            let mut band = 0usize;
            let mut f0 = self.cfg.freq_min as f32;
            while bin <= n / 2 && band < nb {
                let f_lin = (bin as f32 + 0.5) * df;
                let f_log = self.band_freq[band];
                let x = self.fft_out[c][bin];
                if f_lin <= f_log {
                    self.band_out[c][band] += (f_lin - f0) * x * scalar;
                    f0 = f_lin;
                    bin += 1;
                } else {
                    self.band_out[c][band] += (f_log - f0) * x * scalar;
                    f0 = f_log;
                    band += 1;
                }
            }
        }
    }

    /// Copy the results into `s` (reusing its allocations).
    pub fn fill_snapshot(&self, s: &mut Snapshot) {
        s.rms = self.rms;
        s.peak = self.peak;
        s.channels = self.channels;
        s.rate = self.rate;
        s.fft.resize(self.fft_out.len(), Vec::new());
        for (d, src) in s.fft.iter_mut().zip(&self.fft_out) {
            d.clear();
            d.extend_from_slice(src);
        }
        s.bands.resize(self.band_out.len(), Vec::new());
        for (d, src) in s.bands.iter_mut().zip(&self.band_out) {
            d.clear();
            d.extend_from_slice(src);
        }
        if s.band_freq != self.band_freq {
            s.band_freq = self.band_freq.clone();
        }
    }
}

// ----------------------------------------------------------------- snapshot

/// Device description published by the capture thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DevInfo {
    pub name: String,
    pub id: String,
    pub list: String,
    pub format: String,
}

/// Everything `update()` needs, written by the capture thread.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub rms: [f32; MAX_CHANNELS],
    pub peak: [f32; MAX_CHANNELS],
    pub fft: Vec<Vec<f32>>,
    pub bands: Vec<Vec<f32>>,
    pub band_freq: Vec<f32>,
    pub channels: usize,
    pub rate: u32,
    /// The capture process is running (Rainmeter's "device is open").
    pub live: bool,
    pub info: Arc<DevInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Type {
    Rms,
    Peak,
    Fft,
    Band,
    FftFreq,
    BandFreq,
    Format,
    DeviceStatus,
    DeviceName,
    DeviceId,
    DeviceList,
    /// AudioLevelBeta only; always 0 here.
    BufferStatus,
}

const TYPE_NAMES: [(&str, Type); 12] = [
    ("rms", Type::Rms),
    ("peak", Type::Peak),
    ("fft", Type::Fft),
    ("band", Type::Band),
    ("fftfreq", Type::FftFreq),
    ("bandfreq", Type::BandFreq),
    ("format", Type::Format),
    ("devicestatus", Type::DeviceStatus),
    ("devicename", Type::DeviceName),
    ("deviceid", Type::DeviceId),
    ("devicelist", Type::DeviceList),
    ("bufferstatus", Type::BufferStatus),
];

/// Per-parent values applied when reading a snapshot.
#[derive(Clone, Debug)]
pub struct Params {
    pub fft_size: usize,
    pub bands: usize,
    pub sensitivity: f64,
    pub gain_rms: f64,
    pub gain_peak: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params { fft_size: 0, bands: 0, sensitivity: 35.0, gain_rms: 1.0, gain_peak: 1.0 }
    }
}

/// What a child (or the parent itself) asks for.
#[derive(Clone, Copy, Debug)]
pub struct Query {
    pub ty: Type,
    /// 0..7 or `CHANNEL_SUM`.
    pub channel: usize,
    pub fft_idx: usize,
    pub band_idx: usize,
}

fn to_level(x: f64, sensitivity: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    (10.0 / sensitivity * x.log10() + 1.0).max(0.0)
}

/// Numeric value of a query (`UpdateAudioValue`'s switch).
pub fn eval(q: &Query, p: &Params, s: &Snapshot) -> f64 {
    let ch = |a: &[f32; MAX_CHANNELS], i: usize| a[i] as f64;
    match q.ty {
        Type::Rms => {
            if q.channel == CHANNEL_SUM {
                ((ch(&s.rms, 0).sqrt() + ch(&s.rms, 1).sqrt()) * 0.5 * p.gain_rms).clamp(0.0, 1.0)
            } else {
                (ch(&s.rms, q.channel).sqrt() * p.gain_rms).clamp(0.0, 1.0)
            }
        }
        Type::Peak => {
            if q.channel == CHANNEL_SUM {
                ((ch(&s.peak, 0) + ch(&s.peak, 1)) * 0.5 * p.gain_peak).clamp(0.0, 1.0)
            } else {
                (ch(&s.peak, q.channel) * p.gain_peak).clamp(0.0, 1.0)
            }
        }
        Type::Fft | Type::Band => {
            let (data, size, idx) = if q.ty == Type::Fft {
                (&s.fft, p.fft_size, q.fft_idx.min(p.fft_size / 2))
            } else {
                (&s.bands, p.bands, q.band_idx.min(p.bands))
            };
            if !s.live || size == 0 || data.is_empty() {
                return 0.0;
            }
            let get = |c: usize| data.get(c).and_then(|v| v.get(idx)).copied().unwrap_or(0.0) as f64;
            let x = if q.channel == CHANNEL_SUM {
                if s.channels >= 2 {
                    (get(0) + get(1)) * 0.5
                } else {
                    get(0)
                }
            } else if q.channel < s.channels {
                get(q.channel)
            } else {
                0.0
            };
            to_level(x, p.sensitivity)
        }
        Type::FftFreq => {
            if s.live && p.fft_size > 0 && q.fft_idx <= p.fft_size / 2 {
                (q.fft_idx as u64 * s.rate as u64 / p.fft_size as u64) as f64
            } else {
                0.0
            }
        }
        Type::BandFreq => {
            if s.live && p.bands > 0 && q.band_idx < p.bands {
                s.band_freq.get(q.band_idx).copied().unwrap_or(0.0) as f64
            } else {
                0.0
            }
        }
        Type::DeviceStatus => s.live as u8 as f64,
        Type::Format | Type::DeviceName | Type::DeviceId | Type::DeviceList | Type::BufferStatus => 0.0,
    }
}

// ------------------------------------------------------------------ capture

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Port {
    Output,
    Input,
}

/// A parent measure's shared state.
pub struct ParentData {
    dsp: Mutex<Dsp>,
    snap: Mutex<Snapshot>,
    params: Mutex<Params>,
    /// Next implicit `FFTIdx` / `BandIdx` for children that set none (AudioLevelBeta
    /// skins such as Fountain of Colors generate unnumbered `Type=Band` children).
    next_fft: AtomicUsize,
    next_band: AtomicUsize,
}

impl ParentData {
    pub fn new(cfg: DspConfig, params: Params) -> ParentData {
        let dsp = Dsp::new(cfg, RATE, CHANNELS);
        let mut snap = Snapshot::default();
        dsp.fill_snapshot(&mut snap);
        ParentData {
            dsp: Mutex::new(dsp),
            snap: Mutex::new(snap),
            params: Mutex::new(params),
            next_fft: AtomicUsize::new(0),
            next_band: AtomicUsize::new(0),
        }
    }

    /// Run the DSP on `samples` and publish the result.
    pub fn feed(&self, samples: &[f32]) {
        let (Ok(mut d), Ok(mut s)) = (self.dsp.lock(), self.snap.lock()) else { return };
        d.process(samples);
        d.fill_snapshot(&mut s);
    }

    fn zero(&self) {
        let (Ok(mut d), Ok(mut s)) = (self.dsp.lock(), self.snap.lock()) else { return };
        d.reset();
        d.fill_snapshot(&mut s);
    }

    fn set_status(&self, live: bool, info: &Arc<DevInfo>) {
        if let Ok(mut s) = self.snap.lock() {
            s.live = live;
            s.info = info.clone();
        }
    }

    fn reconfigure(&self, cfg: DspConfig, params: Params) {
        if let Ok(mut p) = self.params.lock() {
            *p = params;
        }
        let (Ok(mut d), Ok(mut s)) = (self.dsp.lock(), self.snap.lock()) else { return };
        if d.cfg != cfg {
            *d = Dsp::new(cfg, RATE, CHANNELS);
            d.fill_snapshot(&mut s);
        }
    }
}

/// What children find through the skin registry; weak so a removed parent
/// stops its capture.
pub struct Link {
    parent: Weak<ParentData>,
}

struct Capture {
    subs: Mutex<Vec<Weak<ParentData>>>,
}

type Key = (Port, String);
static CAPTURES: Mutex<Option<HashMap<Key, Arc<Capture>>>> = Mutex::new(None);
static WARNED_NO_TOOL: AtomicBool = AtomicBool::new(false);

fn attach(port: Port, id: &str, p: &Arc<ParentData>) {
    let Ok(mut g) = CAPTURES.lock() else { return };
    let map = g.get_or_insert_with(HashMap::new);
    let key = (port, id.to_string());
    if !map.contains_key(&key) {
        let cap = Arc::new(Capture { subs: Mutex::new(Vec::new()) });
        let (c2, k2) = (cap.clone(), key.clone());
        match std::thread::Builder::new().name("snowmeter-audiolevel".into()).spawn(move || capture_thread(c2, k2)) {
            Ok(_) => {
                map.insert(key.clone(), cap);
            }
            Err(e) => {
                log::warn!("AudioLevel: cannot start the capture thread: {e}");
                return;
            }
        }
    }
    if let Some(c) = map.get(&key) {
        if let Ok(mut subs) = c.subs.lock() {
            subs.push(Arc::downgrade(p));
        }
    }
}

/// Live parents of a capture.
fn subscribers(cap: &Capture) -> Vec<Arc<ParentData>> {
    cap.subs.lock().map(|s| s.iter().filter_map(|w| w.upgrade()).collect()).unwrap_or_default()
}

/// True when the capture has had no parent for `IDLE_STOP`; it is then
/// removed from the table (lock order: table, then subscribers).
fn check_idle(cap: &Capture, key: &Key, idle_since: &mut Option<Instant>) -> bool {
    let Ok(mut g) = CAPTURES.lock() else { return false };
    let Ok(mut subs) = cap.subs.lock() else { return false };
    subs.retain(|w| w.strong_count() > 0);
    if !subs.is_empty() {
        *idle_since = None;
        return false;
    }
    let since = *idle_since.get_or_insert_with(Instant::now);
    if since.elapsed() >= IDLE_STOP {
        if let Some(m) = g.as_mut() {
            m.remove(key);
        }
        true
    } else {
        false
    }
}

fn tool_output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn default_device(port: Port) -> String {
    let sub = if port == Port::Output { "get-default-sink" } else { "get-default-source" };
    tool_output("pactl", &[sub]).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn device_list(port: Port) -> String {
    let sub = if port == Port::Output { "sinks" } else { "sources" };
    let Some(out) = tool_output("pactl", &["list", "short", sub]) else { return String::new() };
    out.lines()
        .filter_map(|l| l.split('\t').nth(1))
        .filter(|n| port == Port::Output || !n.ends_with(".monitor"))
        .map(|n| format!("{n}: {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Command lines to try for a capture of `device` (`None` = the default).
fn tool_commands(port: Port, id: &str, default_name: &str) -> Vec<(&'static str, Vec<String>)> {
    let device = if !id.is_empty() {
        if port == Port::Output && !id.ends_with(".monitor") {
            format!("{id}.monitor")
        } else {
            id.to_string()
        }
    } else if port == Port::Output {
        "@DEFAULT_MONITOR@".to_string()
    } else {
        "@DEFAULT_SOURCE@".to_string()
    };
    let parec = vec![
        "--raw".to_string(),
        "--format=float32le".to_string(),
        format!("--rate={RATE}"),
        format!("--channels={CHANNELS}"),
        "--latency-msec=20".to_string(),
        "--client-name=Snowmeter".to_string(),
        "-d".to_string(),
        device,
    ];
    let mut pw: Vec<String> =
        ["--rate", "48000", "--channels", "2", "--format", "f32", "--latency", "20ms"].map(String::from).to_vec();
    if port == Port::Output {
        pw.extend(["-P".to_string(), "stream.capture.sink=true".to_string()]);
    }
    let target = if !id.is_empty() { id.trim_end_matches(".monitor") } else { default_name.trim_end_matches(".monitor") };
    if !target.is_empty() {
        pw.extend(["--target".to_string(), target.to_string()]);
    }
    pw.push("-".to_string());
    vec![("parec", parec), ("pw-record", pw)]
}

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

enum Outcome {
    Idle,
    Ended,
    NoTool,
}

fn spawn_tool(cmd: &str, args: &[String]) -> Option<Child> {
    let mut c = Command::new(cmd);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    // Do not outlive Snowmeter.
    unsafe {
        c.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    c.spawn().ok()
}

fn run_once(cap: &Capture, key: &Key, idle_since: &mut Option<Instant>) -> Outcome {
    let (port, id) = (key.0, key.1.as_str());
    let default_name = default_device(port);
    let mut child = None;
    for (cmd, args) in tool_commands(port, id, &default_name) {
        if let Some(c) = spawn_tool(cmd, &args) {
            child = Some(c);
            break;
        }
    }
    let Some(mut child) = child else { return Outcome::NoTool };
    let Some(mut stdout) = child.stdout.take() else { return Outcome::Ended };
    let _guard = KillOnDrop(child);
    let name = if id.is_empty() { default_name.clone() } else { id.to_string() };
    let info = Arc::new(DevInfo {
        id: name.clone(),
        name,
        list: device_list(port),
        format: format!("{RATE}Hz PCM 32b {CHANNELS}ch"),
    });
    for p in subscribers(cap) {
        p.set_status(true, &info);
    }
    let fd = stdout.as_raw_fd();
    let mut buf = vec![0u8; 32768];
    let mut carry: Vec<u8> = Vec::new();
    let mut samples: Vec<f32> = Vec::new();
    let frame_bytes = 4 * CHANNELS;
    let (mut last_fill, mut last_tick, mut last_default) = (Instant::now(), Instant::now(), Instant::now());
    let mut zeroed = false;
    loop {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, 100) };
        if r < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if r > 0 {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    carry.extend_from_slice(&buf[..n]);
                    let usable = carry.len() - carry.len() % frame_bytes;
                    samples.clear();
                    samples.extend(carry[..usable].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
                    carry.drain(..usable);
                    if !samples.is_empty() {
                        for p in subscribers(cap) {
                            p.feed(&samples);
                        }
                        last_fill = Instant::now();
                        zeroed = false;
                    }
                }
            }
        } else if !zeroed && last_fill.elapsed() >= EMPTY_TIMEOUT {
            for p in subscribers(cap) {
                p.zero();
            }
            zeroed = true;
        }
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            if check_idle(cap, key, idle_since) {
                return Outcome::Idle;
            }
            // Parents attached after the tool started still need the status.
            for p in subscribers(cap) {
                if p.snap.lock().map(|s| !s.live).unwrap_or(false) {
                    p.set_status(true, &info);
                }
            }
        }
        if id.is_empty() && last_default.elapsed() >= Duration::from_secs(5) {
            last_default = Instant::now();
            let now = default_device(port);
            if !now.is_empty() && now != default_name {
                break; // the default device changed: restart on the new one
            }
        }
    }
    Outcome::Ended
}

fn capture_thread(cap: Arc<Capture>, key: Key) {
    let mut backoff = Duration::from_millis(500);
    let mut idle_since: Option<Instant> = None;
    loop {
        if check_idle(&cap, &key, &mut idle_since) {
            return;
        }
        let started = Instant::now();
        let outcome = run_once(&cap, &key, &mut idle_since);
        let no_tool = matches!(outcome, Outcome::NoTool);
        let dead = Arc::new(DevInfo::default());
        for p in subscribers(&cap) {
            p.set_status(false, &dead);
            p.zero();
        }
        match outcome {
            Outcome::Idle => return,
            Outcome::NoTool => {
                if !WARNED_NO_TOOL.swap(true, Ordering::Relaxed) {
                    log::warn!("AudioLevel: neither parec nor pw-record could be started; audio levels stay at 0");
                }
            }
            Outcome::Ended => {}
        }
        if started.elapsed() > Duration::from_secs(5) {
            backoff = Duration::from_millis(500);
        }
        let wait = if no_tool { Duration::from_secs(30) } else { backoff };
        let until = Instant::now() + wait;
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
            if check_idle(&cap, &key, &mut idle_since) {
                return;
            }
        }
        backoff = (backoff * 2).min(Duration::from_secs(10));
    }
}

// ------------------------------------------------------------------ measure

fn parse_channel(s: &str) -> Option<usize> {
    const NAMES: [[&str; 3]; 9] = [
        ["L", "FL", "0"],
        ["R", "FR", "1"],
        ["C", "", "2"],
        ["LFE", "Sub", "3"],
        ["BL", "", "4"],
        ["BR", "", "5"],
        ["SL", "", "6"],
        ["SR", "", "7"],
        ["Sum", "Avg", ""],
    ];
    NAMES.iter().position(|row| row.iter().any(|n| !n.is_empty() && n.eq_ignore_ascii_case(s)))
}

pub struct AudioLevel {
    section: String,
    parent_name: String,
    port: Port,
    id: String,
    cfg: DspConfig,
    params: Params,
    ty: Type,
    channel: usize,
    fft_idx: i64,
    band_idx: i64,
    /// `FFTIdx` / `BandIdx` given explicitly (otherwise numbered in creation order).
    fft_explicit: bool,
    band_explicit: bool,
    auto_fft: Option<usize>,
    auto_band: Option<usize>,
    /// Skin has `Update=-1`: the parent asks the skin to update it as audio arrives.
    push: bool,
    next_push: Instant,
    mine: Option<Arc<ParentData>>,
    link: Option<Arc<Link>>,
    missing_logged: bool,
    log: Vec<String>,
}

impl AudioLevel {
    pub fn new() -> AudioLevel {
        AudioLevel {
            section: String::new(),
            parent_name: String::new(),
            port: Port::Output,
            id: String::new(),
            cfg: DspConfig::default(),
            params: Params::default(),
            ty: Type::Rms,
            channel: CHANNEL_SUM,
            fft_idx: -1,
            band_idx: -1,
            fft_explicit: false,
            band_explicit: false,
            auto_fft: None,
            auto_band: None,
            push: false,
            next_push: Instant::now(),
            mine: None,
            link: None,
            missing_logged: false,
            log: Vec::new(),
        }
    }

    fn data(&self) -> Option<Arc<ParentData>> {
        match &self.mine {
            Some(m) => Some(m.clone()),
            None => self.link.as_ref().and_then(|l| l.parent.upgrade()),
        }
    }

    fn query(&self) -> Query {
        Query {
            ty: self.ty,
            channel: self.channel,
            fft_idx: if self.fft_explicit { self.fft_idx.max(0) as usize } else { self.auto_fft.unwrap_or(0) },
            band_idx: if self.band_explicit { self.band_idx.max(0) as usize } else { self.auto_band.unwrap_or(0) },
        }
    }

    fn read_parent_options(&mut self, opt: &OptionReader) {
        self.port = if opt.string("Port", "Output").trim().eq_ignore_ascii_case("input") { Port::Input } else { Port::Output };
        self.id = opt.string("ID", "").trim().to_string();
        let mut c = DspConfig::default();
        let size = opt.int("FFTSize", 0);
        c.fft_size = if size < 0 || size & 1 == 1 {
            self.log.push(format!("AudioLevel: Invalid FFTSize {size}: must be an even integer >= 0. (powers of 2 work best)"));
            0
        } else {
            size as usize
        };
        if c.fft_size > 0 {
            let ov = opt.int("FFTOverlap", 0);
            c.fft_overlap = if ov < 0 || ov >= c.fft_size as i64 {
                self.log.push(format!("AudioLevel: Invalid FFTOverlap {ov}: must be an integer between 0 and FFTSize({size})."));
                0
            } else {
                ov as usize
            };
        }
        let bands = opt.int("Bands", 0);
        c.bands = if bands < 0 {
            self.log.push(format!("AudioLevel: Invalid Bands {bands}: must be an integer >= 0."));
            0
        } else {
            bands as usize
        };
        c.freq_min = opt.float("FreqMin", 20.0).max(0.0);
        c.freq_max = opt.float("FreqMax", 20000.0).max(0.0);
        let env = |k: &str, d: i32| opt.int(k, d as i64).clamp(0, i32::MAX as i64) as i32;
        c.env_rms = [env("RMSAttack", 300), env("RMSDecay", 300)];
        c.env_peak = [env("PeakAttack", 50), env("PeakDecay", 2500)];
        c.env_fft = [env("FFTAttack", 300), env("FFTDecay", 300)];
        self.params = Params {
            fft_size: c.fft_size,
            bands: c.bands,
            sensitivity: opt.float("Sensitivity", 35.0).max(1.0),
            gain_rms: opt.float("RMSGain", 1.0).max(0.0),
            gain_peak: opt.float("PeakGain", 1.0).max(0.0),
        };
        self.cfg = c;
    }
}

impl MeasureImpl for AudioLevel {
    fn read_options(&mut self, opt: &OptionReader) {
        self.section = opt.section.to_string();
        self.parent_name = opt.string("Parent", "").trim().to_string();
        if self.parent_name.is_empty() {
            self.read_parent_options(opt);
            if let Some(m) = &self.mine {
                m.reconfigure(self.cfg.clone(), self.params.clone());
            }
        } else {
            self.mine = None;
        }
        let ch = opt.string("Channel", "");
        if !ch.trim().is_empty() {
            match parse_channel(ch.trim()) {
                Some(c) => self.channel = c,
                None => self.log.push(format!(
                    "AudioLevel: Invalid Channel: '{ch}', must be a number between 0 and 7, or one of: L, R, C, LFE, BL, BR, SL, SR, Sum"
                )),
            }
        }
        let ty = opt.string("Type", "");
        if !ty.trim().is_empty() {
            match TYPE_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(ty.trim())) {
                Some((_, t)) => self.ty = *t,
                None => self.log.push(format!(
                    "AudioLevel: Invalid Type: '{ty}', must be one of: RMS, Peak, FFT, Band, FFTFreq, BandFreq, Format, DeviceStatus, DeviceName, DeviceID, DeviceList"
                )),
            }
        }
        self.fft_explicit = opt.has("FFTIdx");
        self.band_explicit = opt.has("BandIdx");
        self.fft_idx = opt.int("FFTIdx", self.fft_idx).max(0);
        self.band_idx = opt.int("BandIdx", self.band_idx).max(0);
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        if self.parent_name.is_empty() {
            if self.mine.is_none() {
                let pd = Arc::new(ParentData::new(self.cfg.clone(), self.params.clone()));
                publish(ctx.skin_dir, &self.section, Arc::new(Link { parent: Arc::downgrade(&pd) }));
                attach(self.port, &self.id, &pd);
                self.mine = Some(pd);
            }
        } else if self.link.is_none() {
            self.link = lookup::<Link>(ctx.skin_dir, &self.parent_name);
            if self.link.is_none() && !self.missing_logged {
                self.missing_logged = true;
                ctx.log.push(format!("AudioLevel: Couldn't find Parent measure '{}'.", self.parent_name));
            }
        }
        if self.parent_name.is_empty() && ctx.update_ms < 0 {
            self.push = true;
        }
        let Some(pd) = self.data() else { return 0.0 };
        if self.auto_fft.is_none() && !self.fft_explicit && self.ty == Type::Fft {
            self.auto_fft = Some(pd.next_fft.fetch_add(1, Ordering::Relaxed));
        }
        if self.auto_band.is_none() && !self.band_explicit && self.ty == Type::Band {
            self.auto_band = Some(pd.next_band.fetch_add(1, Ordering::Relaxed));
        }
        let (Ok(p), Ok(s)) = (pd.params.lock(), pd.snap.lock()) else { return 0.0 };
        eval(&self.query(), &p, &s)
    }

    fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        (self.push && self.mine.is_some()).then(|| self.next_push.saturating_duration_since(now))
    }

    fn poll_timers(&mut self, now: Instant) -> Vec<String> {
        if self.push && self.mine.is_some() && now >= self.next_push {
            self.next_push = now + Duration::from_millis(33);
            return vec![format!("[!UpdateMeasure \"{}\"]", self.section)];
        }
        Vec::new()
    }

    fn string_value(&self) -> Option<String> {
        if !matches!(self.ty, Type::Format | Type::DeviceName | Type::DeviceId | Type::DeviceList) {
            return None;
        }
        let pd = self.data()?;
        let s = pd.snap.lock().ok()?;
        let i = &s.info;
        Some(match self.ty {
            Type::Format if s.live => i.format.clone(),
            Type::DeviceName if s.live => i.name.clone(),
            Type::DeviceId if s.live => i.id.clone(),
            Type::DeviceList if s.live => i.list.clone(),
            _ => String::new(),
        })
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    fn sine(freq: f64, amp: f32, frames: usize, offset: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = (i + offset) as f64 / RATE as f64;
            let x = amp * (2.0 * std::f64::consts::PI * freq * t).sin() as f32;
            v.push(x);
            v.push(x);
        }
        v
    }

    fn argmax(v: &[f32]) -> usize {
        v.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).map(|(i, _)| i).unwrap()
    }

    fn fft_cfg() -> DspConfig {
        DspConfig { fft_size: 1024, ..Default::default() }
    }

    #[test]
    fn sine_peaks_in_the_right_fft_bin() {
        let mut d = Dsp::new(fft_cfg(), RATE, 2);
        // 1031.25 Hz is exactly bin 22 of a 1024-point FFT at 48 kHz.
        let f = 22.0 * RATE as f64 / 1024.0;
        for blk in 0..40 {
            d.process(&sine(f, 0.5, 960, blk * 960));
        }
        let mut s = Snapshot::default();
        d.fill_snapshot(&mut s);
        assert_eq!(argmax(&s.fft[0]), 22);
        assert_eq!(argmax(&s.fft[1]), 22);
        let p = Params { fft_size: 1024, ..Default::default() };
        s.live = true;
        let q = |i| Query { ty: Type::Fft, channel: 0, fft_idx: i, band_idx: 0 };
        let peak = eval(&q(22), &p, &s);
        assert!(peak > 0.5 && peak <= 1.0, "peak level {peak}");
        assert!(eval(&q(200), &p, &s) < peak * 0.5);
        let sum = Query { channel: CHANNEL_SUM, ..q(22) };
        assert!((eval(&sum, &p, &s) - peak).abs() < 1e-6);
        // FFTFreq of that bin.
        let ff = Query { ty: Type::FftFreq, ..q(22) };
        assert_eq!(eval(&ff, &p, &s), 1031.0);
    }

    #[test]
    fn sine_peaks_in_the_right_band() {
        let cfg = DspConfig { fft_size: 2048, bands: 16, ..Default::default() };
        let mut d = Dsp::new(cfg, RATE, 2);
        for blk in 0..60 {
            d.process(&sine(1000.0, 0.5, 960, blk * 960));
        }
        let mut s = Snapshot::default();
        d.fill_snapshot(&mut s);
        let b = argmax(&s.bands[0]);
        let lo = if b == 0 { 20.0 } else { s.band_freq[b - 1] };
        assert!(lo <= 1000.0 && 1000.0 <= s.band_freq[b], "band {b} spans {lo}..{}", s.band_freq[b]);
        s.live = true;
        let p = Params { fft_size: 2048, bands: 16, ..Default::default() };
        let q = Query { ty: Type::Band, channel: CHANNEL_SUM, fft_idx: 0, band_idx: b };
        assert!(eval(&q, &p, &s) > 0.3);
        let far = Query { band_idx: 0, ..q };
        assert!(eval(&far, &p, &s) < eval(&q, &p, &s));
        // Band edges are log spaced from FreqMin to FreqMax.
        assert!((s.band_freq[15] as f64 - 20000.0 * 2f64.powf(-(((20000.0f64 / 20.0).log2() / 16.0) / 2.0))).abs() < 1.0);
    }

    #[test]
    fn silence_is_zero() {
        let cfg = DspConfig { fft_size: 1024, bands: 8, ..Default::default() };
        let mut d = Dsp::new(cfg, RATE, 2);
        d.process(&vec![0.0; 960 * 2 * 20]);
        let mut s = Snapshot::default();
        d.fill_snapshot(&mut s);
        s.live = true;
        let p = Params { fft_size: 1024, bands: 8, ..Default::default() };
        for ty in [Type::Rms, Type::Peak, Type::Fft, Type::Band] {
            for ch in [0, 1, CHANNEL_SUM] {
                assert_eq!(eval(&Query { ty, channel: ch, fft_idx: 5, band_idx: 3 }, &p, &s), 0.0, "{ty:?}");
            }
        }
    }

    #[test]
    fn rms_and_peak_attack_and_decay() {
        let mut d = Dsp::new(DspConfig::default(), RATE, 2);
        // Peak attack is 50 ms: after 50 ms of full scale the filter is 1 - e^-2.
        d.process(&vec![1.0; 2400 * 2]);
        let mut s = Snapshot::default();
        d.fill_snapshot(&mut s);
        let p = Params::default();
        let peak = Query { ty: Type::Peak, channel: 0, fft_idx: 0, band_idx: 0 };
        let v = eval(&peak, &p, &s);
        assert!((v - (1.0 - (-2.0f64).exp())).abs() < 0.01, "{v}");
        // RMS attack is 300 ms, on the squared signal, then square-rooted.
        let mut d = Dsp::new(DspConfig::default(), RATE, 2);
        d.process(&vec![0.5; 14400 * 2]);
        d.fill_snapshot(&mut s);
        let rms = Query { ty: Type::Rms, channel: CHANNEL_SUM, fft_idx: 0, band_idx: 0 };
        let want = (0.25 * (1.0 - (-2.0f64).exp())).sqrt();
        assert!((eval(&rms, &p, &s) - want).abs() < 0.01);
        // Decay: peak decays slowly (2.5 s), RMS falls faster (300 ms).
        let before = eval(&peak, &p, &s);
        d.process(&vec![0.0; 14400 * 2]);
        d.fill_snapshot(&mut s);
        let after = eval(&peak, &p, &s);
        assert!(after < before && after > before * 0.5, "{before} -> {after}");
        assert!(eval(&rms, &p, &s) < want * 0.5);
        // Gain scales and clamps.
        let g = Params { gain_peak: 100.0, ..Default::default() };
        assert_eq!(eval(&peak, &g, &s), 1.0);
        // Zero attack follows the input instantly.
        let cfg = DspConfig { env_peak: [0, 2500], ..Default::default() };
        let mut d = Dsp::new(cfg, RATE, 2);
        d.process(&[0.7, 0.7]);
        d.fill_snapshot(&mut s);
        assert!((eval(&peak, &p, &s) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn reset_clears_levels() {
        let mut d = Dsp::new(fft_cfg(), RATE, 2);
        d.process(&sine(1000.0, 0.5, 4800, 0));
        d.reset();
        let mut s = Snapshot::default();
        d.fill_snapshot(&mut s);
        assert!(s.rms.iter().chain(&s.peak).all(|x| *x == 0.0));
        assert!(s.fft[0].iter().all(|x| *x == 0.0));
    }

    #[test]
    fn options_parse_like_rainmeter() {
        let s = skin(
            "[P]\nMeasure=Plugin\nPlugin=AudioLevelBeta\nPort=Input\nID=mic\nFFTSize=1023\nBands=-2\nSensitivity=0.5\nRMSGain=2\n\
             [Q]\nMeasure=AudioLevel\nFFTSize=2048\nFFTOverlap=1024\nBands=10\nFreqMin=100\nFreqMax=5000\nSensitivity=50\n\
             [C]\nMeasure=AudioLevel\nParent=Q\nType=band\nChannel=r\nBandIdx=3\n[D]\nMeasure=AudioLevel\nParent=Q\nType=Nope\nChannel=9\n",
        );
        let mut p = AudioLevel::new();
        p.read_options(&OptionReader::new(&s.parser, "P", None));
        assert_eq!(p.port, Port::Input);
        assert_eq!(p.id, "mic");
        assert_eq!((p.cfg.fft_size, p.cfg.bands), (0, 0));
        assert_eq!(p.params.sensitivity, 1.0);
        assert_eq!(p.params.gain_rms, 2.0);
        assert_eq!(p.take_log().len(), 2);
        let mut q = AudioLevel::new();
        q.read_options(&OptionReader::new(&s.parser, "Q", None));
        assert_eq!(q.cfg.fft_size, 2048);
        assert_eq!(q.cfg.fft_overlap, 1024);
        assert_eq!((q.cfg.freq_min, q.cfg.freq_max), (100.0, 5000.0));
        assert_eq!(q.params.sensitivity, 50.0);
        assert_eq!((q.ty, q.channel), (Type::Rms, CHANNEL_SUM));
        let mut c = AudioLevel::new();
        c.read_options(&OptionReader::new(&s.parser, "C", None));
        assert_eq!((c.ty, c.channel, c.band_idx), (Type::Band, 1, 3));
        let mut d = AudioLevel::new();
        d.read_options(&OptionReader::new(&s.parser, "D", None));
        assert_eq!(d.take_log().len(), 2);
        assert_eq!(parse_channel("Sub"), Some(3));
        assert_eq!(parse_channel("avg"), Some(CHANNEL_SUM));
    }

    #[test]
    fn child_reads_parent_snapshot_through_the_registry() {
        let s = skin("[Q]\nMeasure=AudioLevel\nFFTSize=1024\n[C]\nMeasure=AudioLevel\nParent=Q\nType=FFT\nChannel=L\nFFTIdx=22\n[F]\nMeasure=AudioLevel\nParent=Q\nType=Format\n[S]\nMeasure=AudioLevel\nParent=Q\nType=DeviceStatus\n[X]\nMeasure=AudioLevel\nParent=Missing\n");
        // Build the parent by hand so no capture process is started.
        let mut q = AudioLevel::new();
        q.read_options(&OptionReader::new(&s.parser, "Q", None));
        let pd = Arc::new(ParentData::new(q.cfg.clone(), q.params.clone()));
        let dir = s.env.skins_dir.join("T");
        publish(&dir, "Q", Arc::new(Link { parent: Arc::downgrade(&pd) }));
        let f = 22.0 * RATE as f64 / 1024.0;
        for blk in 0..40 {
            pd.feed(&sine(f, 0.5, 960, blk * 960));
        }
        let mk = |sec: &str| {
            let mut m = AudioLevel::new();
            m.read_options(&OptionReader::new(&s.parser, sec, None));
            m
        };
        let (mut c, mut fm, mut st, mut x) = (mk("C"), mk("F"), mk("S"), mk("X"));
        // Not live yet: FFT is 0, DeviceStatus 0, Format empty.
        assert_eq!(ctx_run(&s, |ctx| c.update(ctx)), 0.0);
        assert_eq!(ctx_run(&s, |ctx| st.update(ctx)), 0.0);
        pd.set_status(true, &Arc::new(DevInfo { name: "n".into(), id: "i".into(), list: "l".into(), format: "48000Hz PCM 32b 2ch".into() }));
        assert!(ctx_run(&s, |ctx| c.update(ctx)) > 0.5);
        assert_eq!(ctx_run(&s, |ctx| st.update(ctx)), 1.0);
        ctx_run(&s, |ctx| fm.update(ctx));
        assert_eq!(fm.string_value().as_deref(), Some("48000Hz PCM 32b 2ch"));
        assert_eq!(c.string_value(), None);
        assert_eq!(ctx_run(&s, |ctx| x.update(ctx)), 0.0);
        assert!(x.missing_logged);
        // A dropped parent reads as 0.
        drop(pd);
        assert_eq!(ctx_run(&s, |ctx| c.update(ctx)), 0.0);
    }

    #[test]
    fn unnumbered_band_children_get_consecutive_indexes_and_parent_pushes_updates() {
        let s = skin("[P]\nMeasure=AudioLevel\nBands=4\nFFTSize=64\n[A]\nMeasure=AudioLevel\nParent=P\nType=Band\n[B]\nMeasure=AudioLevel\nParent=P\nType=Band\n[C]\nMeasure=AudioLevel\nParent=P\nType=Band\nBandIdx=3\n");
        let mut p = AudioLevel::new();
        p.read_options(&OptionReader::new(&s.parser, "P", None));
        let pd = Arc::new(ParentData::new(p.cfg.clone(), p.params.clone()));
        let dir = s.env.skins_dir.join("T");
        publish(&dir, "P", Arc::new(Link { parent: Arc::downgrade(&pd) }));
        let mk = |sec: &str| {
            let mut m = AudioLevel::new();
            m.read_options(&OptionReader::new(&s.parser, sec, None));
            m
        };
        let (mut a, mut b, mut c) = (mk("A"), mk("B"), mk("C"));
        for m in [&mut a, &mut b, &mut c] {
            ctx_run(&s, |ctx| m.update(ctx));
        }
        assert_eq!((a.query().band_idx, b.query().band_idx, c.query().band_idx), (0, 1, 3));
        // Update=-1 skins: the parent requests its own update.
        p.mine = Some(pd.clone());
        p.push = true;
        let t = Instant::now();
        assert_eq!(p.poll_timers(t), ["[!UpdateMeasure \"P\"]"]);
        assert!(p.poll_timers(t).is_empty());
        assert!(p.next_wakeup(t).is_some());
    }

    #[test]
    fn tool_commands_pick_the_monitor() {
        let c = tool_commands(Port::Output, "", "sink.x");
        assert!(c[0].1.contains(&"@DEFAULT_MONITOR@".to_string()));
        assert!(c[1].1.contains(&"sink.x".to_string()));
        let c = tool_commands(Port::Output, "alsa_output.foo", "");
        assert!(c[0].1.contains(&"alsa_output.foo.monitor".to_string()));
        let c = tool_commands(Port::Input, "", "src");
        assert!(c[0].1.contains(&"@DEFAULT_SOURCE@".to_string()));
        assert!(!c[1].1.contains(&"stream.capture.sink=true".to_string()));
    }
}
