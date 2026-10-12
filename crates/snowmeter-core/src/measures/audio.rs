//! Win7AudioPlugin (`Measure=Plugin`, `Plugin=Win7AudioPlugin`/`Audio`): master
//! volume of the default output as the value (0-100, -1 while muted) and its
//! device name as the string.
//!
//! The system is only touched through `wpctl` (PipeWire) with `pactl` as the
//! fallback, always on worker threads; the measure reads a cached snapshot.
//! Volume and mute are changed only when a skin sends a `!CommandMeasure`
//! (`SetVolume N`, `ChangeVolume N`, `ToggleMute`, `Mute`, `Unmute`,
//! `SetOutputIndex N`, `ToggleNext`, `TogglePrevious`).

use super::bgshared::Bg;
use super::extcmd::{have, run_capture};
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Output {
    /// Identifier understood by the backend (pulse sink name).
    pub id: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct AudioState {
    /// The backend answered.
    pub valid: bool,
    /// 0.0 - 1.0
    pub volume: f64,
    pub muted: bool,
    /// Description of the default output.
    pub name: String,
    pub outputs: Vec<Output>,
    pub default_index: Option<usize>,
}

/// What the plugin needs from the sound server. All calls may block.
pub trait AudioBackend: Send + Sync {
    fn state(&self) -> AudioState;
    /// `volume` is 0.0 - 1.0.
    fn set_volume(&self, volume: f64) -> Result<(), String>;
    fn set_mute(&self, mute: bool) -> Result<(), String>;
    fn set_default_output(&self, id: &str) -> Result<(), String>;
}

// ---- parsing ----------------------------------------------------------------

/// `Volume: 0.45` / `Volume: 0.45 [MUTED]` -> (volume, muted).
pub fn parse_wpctl_volume(out: &str) -> Option<(f64, bool)> {
    let rest = out.trim().strip_prefix("Volume:")?;
    let v: f64 = rest.split_whitespace().next()?.parse().ok()?;
    Some((v.clamp(0.0, 1.5), rest.contains("[MUTED]")))
}

/// First percentage of `pactl get-sink-volume`.
pub fn parse_pactl_volume(out: &str) -> Option<f64> {
    let i = out.find('%')?;
    let digits: String = out[..i].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse::<f64>().ok().map(|p| p / 100.0)
}

pub fn parse_pactl_mute(out: &str) -> Option<bool> {
    match out.trim().strip_prefix("Mute:")?.trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// `pactl list sinks` (long form) -> sink names with descriptions.
pub fn parse_pactl_sinks(out: &str) -> Vec<Output> {
    let mut v: Vec<Output> = Vec::new();
    let mut cur: Option<Output> = None;
    for l in out.lines() {
        if l.starts_with("Sink #") {
            v.extend(cur.take());
            cur = Some(Output::default());
        } else if let Some(c) = cur.as_mut() {
            let t = l.trim();
            if let Some(n) = t.strip_prefix("Name:") {
                c.id = n.trim().to_string();
            } else if let Some(d) = t.strip_prefix("Description:") {
                c.description = d.trim().to_string();
            }
        }
    }
    v.extend(cur);
    v.retain(|o| !o.id.is_empty());
    v
}

/// `node.description = "..."` from `wpctl inspect`.
pub fn parse_inspect_description(out: &str) -> Option<String> {
    let l = out.lines().find(|l| l.trim_start().trim_start_matches('*').trim_start().starts_with("node.description"))?;
    Some(l.split_once('=')?.1.trim().trim_matches('"').to_string())
}

// ---- real backend -----------------------------------------------------------

pub struct SystemBackend;

const T: Duration = Duration::from_secs(4);

impl SystemBackend {
    fn wpctl_volume() -> Option<(f64, bool)> {
        if !have("wpctl") {
            return None;
        }
        parse_wpctl_volume(&run_capture("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"], T)?)
    }
}

impl AudioBackend for SystemBackend {
    fn state(&self) -> AudioState {
        let pactl = have("pactl");
        let (volume, muted, ok) = match Self::wpctl_volume() {
            Some((v, m)) => (v, m, true),
            None if pactl => {
                let v = run_capture("pactl", &["get-sink-volume", "@DEFAULT_SINK@"], T).and_then(|o| parse_pactl_volume(&o));
                let m = run_capture("pactl", &["get-sink-mute", "@DEFAULT_SINK@"], T).and_then(|o| parse_pactl_mute(&o));
                (v.unwrap_or(0.0), m.unwrap_or(false), v.is_some())
            }
            None => (0.0, false, false),
        };
        if !ok {
            return AudioState::default();
        }
        let mut st = AudioState { valid: true, volume, muted, ..Default::default() };
        if pactl {
            st.outputs = run_capture("pactl", &["list", "sinks"], T).map(|o| parse_pactl_sinks(&o)).unwrap_or_default();
            let def = run_capture("pactl", &["get-default-sink"], T).map(|s| s.trim().to_string()).unwrap_or_default();
            st.default_index = st.outputs.iter().position(|o| o.id == def);
            if let Some(i) = st.default_index {
                st.name = st.outputs[i].description.clone();
            }
        }
        if st.name.is_empty() && have("wpctl") {
            st.name = run_capture("wpctl", &["inspect", "@DEFAULT_AUDIO_SINK@"], T).and_then(|o| parse_inspect_description(&o)).unwrap_or_default();
        }
        st
    }

    fn set_volume(&self, volume: f64) -> Result<(), String> {
        let v = volume.clamp(0.0, 1.0);
        if Self::wpctl_volume().is_some() {
            let arg = format!("{v:.2}");
            return run_capture("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &arg], T).map(|_| ()).ok_or_else(|| "wpctl set-volume failed".into());
        }
        let arg = format!("{}%", (v * 100.0).round() as i64);
        run_capture("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &arg], T).map(|_| ()).ok_or_else(|| "pactl set-sink-volume failed".into())
    }

    fn set_mute(&self, mute: bool) -> Result<(), String> {
        let flag = if mute { "1" } else { "0" };
        if Self::wpctl_volume().is_some() {
            return run_capture("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", flag], T).map(|_| ()).ok_or_else(|| "wpctl set-mute failed".into());
        }
        run_capture("pactl", &["set-sink-mute", "@DEFAULT_SINK@", flag], T).map(|_| ()).ok_or_else(|| "pactl set-sink-mute failed".into())
    }

    fn set_default_output(&self, id: &str) -> Result<(), String> {
        if id.is_empty() || id.starts_with('-') {
            return Err("invalid output".into());
        }
        run_capture("pactl", &["set-default-sink", id], T).map(|_| ()).ok_or_else(|| "pactl set-default-sink failed".into())
    }
}

// ---- commands ---------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    SetVolume(i64),
    ChangeVolume(i64),
    ToggleMute,
    Mute,
    Unmute,
    /// 1-based absolute index (clamped).
    SetOutput(i64),
    /// Relative step through the outputs, wrapping.
    StepOutput(i64),
}

/// `!CommandMeasure` argument text. `Ok(None)` is accepted but has no effect.
pub fn parse_command(args: &str) -> Result<Option<Cmd>, String> {
    let a = args.trim();
    if let Some((bang, arg)) = a.split_once(' ') {
        let n = arg.split_whitespace().next().unwrap_or("").parse::<i64>().ok();
        let kind = bang.to_ascii_lowercase();
        if !matches!(kind.as_str(), "setoutputindex" | "setvolume" | "changevolume") {
            return Err("Audio: Unknown bang".into());
        }
        let Some(n) = n.filter(|n| kind != "changevolume" || *n != 0) else {
            return Err("Audio: Incorrect number of arguments for bang".into());
        };
        return Ok(Some(match kind.as_str() {
            "setoutputindex" => Cmd::SetOutput(n),
            "setvolume" => Cmd::SetVolume(n),
            _ => Cmd::ChangeVolume(n),
        }));
    }
    match a.to_ascii_lowercase().as_str() {
        "togglenext" => Ok(Some(Cmd::StepOutput(1))),
        "toggleprevious" => Ok(Some(Cmd::StepOutput(-1))),
        "togglemute" => Ok(Some(Cmd::ToggleMute)),
        "mute" => Ok(Some(Cmd::Mute)),
        "unmute" => Ok(Some(Cmd::Unmute)),
        // Queries have no meaning for a bang; the state is in the measure value.
        "getismuted" | "getvolume" | "getoutput" => Ok(None),
        _ => Err("Audio: Unknown bang".into()),
    }
}

/// Apply one command. Reads the live state first so relative changes are exact.
pub fn execute(b: &dyn AudioBackend, cmd: &Cmd) -> Result<(), String> {
    let st = b.state();
    if !st.valid {
        return Err("Audio: no sound server found".into());
    }
    match cmd {
        Cmd::SetVolume(v) | Cmd::ChangeVolume(v) => {
            let base = if matches!(cmd, Cmd::ChangeVolume(_)) { st.volume * 100.0 } else { 0.0 };
            let target = (base + *v as f64).clamp(0.0, 100.0);
            if st.muted {
                b.set_mute(false)?;
            }
            b.set_volume(target / 100.0)
        }
        Cmd::ToggleMute => b.set_mute(!st.muted),
        Cmd::Mute => b.set_mute(true),
        Cmd::Unmute => b.set_mute(false),
        Cmd::SetOutput(i) | Cmd::StepOutput(i) => {
            let count = st.outputs.len() as i64;
            if count == 0 {
                return Err("Audio: No output device found".into());
            }
            let idx = if matches!(cmd, Cmd::StepOutput(_)) {
                let Some(cur) = st.default_index else { return Err("Audio: Could not find default output".into()) };
                (cur as i64 + i).rem_euclid(count)
            } else {
                (*i).clamp(1, count) - 1
            };
            b.set_default_output(&st.outputs[idx as usize].id)
        }
    }
}

// ---- engine: cached snapshot + serialized command queue ---------------------

#[derive(Default)]
struct Queue {
    cmds: VecDeque<Cmd>,
    draining: bool,
}

#[derive(Clone)]
pub struct Engine {
    backend: Arc<dyn AudioBackend>,
    queue: Arc<Mutex<Queue>>,
    state: Bg<AudioState>,
    last: Arc<Mutex<Option<Instant>>>,
    log: Arc<Mutex<Vec<String>>>,
    refresh: Duration,
}

impl Engine {
    pub fn new(backend: Arc<dyn AudioBackend>, refresh: Duration) -> Engine {
        Engine {
            backend,
            queue: Arc::default(),
            state: Bg::new(AudioState::default()),
            last: Arc::default(),
            log: Arc::default(),
            refresh,
        }
    }

    /// Cached state; starts a refresh on a worker when it is stale.
    pub fn snapshot(&self) -> AudioState {
        let busy = self.queue.lock().map(|q| q.draining).unwrap_or(false);
        if !busy {
            if let Ok(mut last) = self.last.lock() {
                if last.is_none_or(|t| t.elapsed() >= self.refresh) {
                    let b = self.backend.clone();
                    if self.state.start(Duration::ZERO, move || b.state()) {
                        *last = Some(Instant::now());
                    }
                }
            }
        }
        self.state.get()
    }

    /// Queue a command; a worker runs the queue in order, then refreshes the snapshot.
    pub fn send(&self, cmd: Cmd) {
        {
            let Ok(mut q) = self.queue.lock() else { return };
            q.cmds.push_back(cmd);
            if q.draining {
                return;
            }
            q.draining = true;
        }
        let (backend, queue, state, log, last) = (self.backend.clone(), self.queue.clone(), self.state.clone(), self.log.clone(), self.last.clone());
        let spawned = std::thread::Builder::new().name("snowmeter-audio".into()).spawn(move || loop {
            let next = queue.lock().ok().and_then(|mut q| {
                let c = q.cmds.pop_front();
                if c.is_none() {
                    q.draining = false;
                }
                c
            });
            let Some(cmd) = next else { break };
            if let Err(e) = execute(backend.as_ref(), &cmd) {
                if let Ok(mut l) = log.lock() {
                    l.push(if e.starts_with("Audio") { e } else { format!("Audio: {e}") });
                }
            }
            let fresh = backend.state();
            state.modify(|s| *s = fresh);
            if let Ok(mut l) = last.lock() {
                *l = Some(Instant::now());
            }
        });
        if spawned.is_err() {
            if let Ok(mut q) = self.queue.lock() {
                q.cmds.clear();
                q.draining = false;
            }
        }
    }

    pub fn idle(&self) -> bool {
        self.queue.lock().map(|q| !q.draining && q.cmds.is_empty()).unwrap_or(true)
    }

    pub fn take_log(&self) -> Vec<String> {
        self.log.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
    }
}

fn system_engine() -> Engine {
    static E: OnceLock<Engine> = OnceLock::new();
    E.get_or_init(|| Engine::new(Arc::new(SystemBackend), Duration::from_millis(1000))).clone()
}

// ---- the measure ------------------------------------------------------------

pub struct AudioMeasure {
    engine: Option<Engine>,
    string: String,
    log: Vec<String>,
}

impl AudioMeasure {
    pub fn new() -> AudioMeasure {
        AudioMeasure { engine: None, string: String::new(), log: Vec::new() }
    }

    pub fn with_engine(engine: Engine) -> AudioMeasure {
        AudioMeasure { engine: Some(engine), string: String::new(), log: Vec::new() }
    }

    fn engine(&mut self) -> Engine {
        self.engine.get_or_insert_with(system_engine).clone()
    }
}

impl Default for AudioMeasure {
    fn default() -> Self {
        AudioMeasure::new()
    }
}

impl MeasureImpl for AudioMeasure {
    fn read_options(&mut self, _opt: &OptionReader) {}

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let st = self.engine().snapshot();
        self.string = if st.valid { st.name.clone() } else { "ERROR".into() };
        if !st.valid {
            return 0.0;
        }
        if st.muted {
            -1.0
        } else {
            (st.volume * 100.0 + 0.5).floor().min(100.0)
        }
    }

    fn string_value(&self) -> Option<String> {
        Some(self.string.clone())
    }

    fn default_max(&self) -> f64 {
        100.0
    }

    fn forced_min(&self) -> Option<f64> {
        Some(0.0)
    }

    fn forced_max(&self) -> Option<f64> {
        Some(100.0)
    }

    fn command(&mut self, args: &str) {
        match parse_command(args) {
            Ok(Some(c)) => self.engine().send(c),
            Ok(None) => {}
            Err(e) => self.log.push(e),
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        let mut l = std::mem::take(&mut self.log);
        if let Some(e) = &self.engine {
            l.extend(e.take_log());
        }
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    /// In-memory sound server that records every call.
    struct Fake {
        st: Mutex<AudioState>,
        calls: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new(volume: f64, muted: bool) -> Arc<Fake> {
            let outputs = vec![
                Output { id: "alsa.speakers".into(), description: "Speakers".into() },
                Output { id: "alsa.hdmi".into(), description: "HDMI".into() },
                Output { id: "bt.head".into(), description: "Headset".into() },
            ];
            Arc::new(Fake {
                st: Mutex::new(AudioState { valid: true, volume, muted, name: "Speakers".into(), outputs, default_index: Some(0) }),
                calls: Mutex::default(),
            })
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl AudioBackend for Fake {
        fn state(&self) -> AudioState {
            self.st.lock().unwrap().clone()
        }
        fn set_volume(&self, v: f64) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("vol {:.2}", v));
            self.st.lock().unwrap().volume = v;
            Ok(())
        }
        fn set_mute(&self, m: bool) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("mute {m}"));
            self.st.lock().unwrap().muted = m;
            Ok(())
        }
        fn set_default_output(&self, id: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("out {id}"));
            let mut s = self.st.lock().unwrap();
            s.default_index = s.outputs.iter().position(|o| o.id == id);
            s.name = s.outputs[s.default_index.unwrap()].description.clone();
            Ok(())
        }
    }

    #[test]
    fn parsers() {
        assert_eq!(parse_wpctl_volume("Volume: 0.45\n"), Some((0.45, false)));
        assert_eq!(parse_wpctl_volume("Volume: 0.30 [MUTED]"), Some((0.30, true)));
        assert_eq!(parse_wpctl_volume("nope"), None);
        assert_eq!(parse_pactl_volume("Volume: front-left: 29491 /  45% / -20.00 dB,   front-right: 29491 /  45% / -20.00 dB"), Some(0.45));
        assert_eq!(parse_pactl_volume("x"), None);
        assert_eq!(parse_pactl_mute("Mute: yes\n"), Some(true));
        assert_eq!(parse_pactl_mute("Mute: no"), Some(false));
        let sinks = "Sink #50\n\tState: RUNNING\n\tName: alsa_output.pci.analog\n\tDescription: Built-in Audio Analog Stereo\n\tDriver: x\nSink #51\n\tName: alsa_output.hdmi\n\tDescription: HDMI / DisplayPort\n";
        let o = parse_pactl_sinks(sinks);
        assert_eq!(o.len(), 2);
        assert_eq!((o[1].id.as_str(), o[1].description.as_str()), ("alsa_output.hdmi", "HDMI / DisplayPort"));
        assert_eq!(parse_inspect_description("  * node.description = \"Built-in Audio\"\n").as_deref(), Some("Built-in Audio"));
    }

    #[test]
    fn command_parsing() {
        assert_eq!(parse_command("SetVolume 40"), Ok(Some(Cmd::SetVolume(40))));
        assert_eq!(parse_command("ChangeVolume -5"), Ok(Some(Cmd::ChangeVolume(-5))));
        assert_eq!(parse_command("setoutputindex 2"), Ok(Some(Cmd::SetOutput(2))));
        assert_eq!(parse_command("ToggleNext"), Ok(Some(Cmd::StepOutput(1))));
        assert_eq!(parse_command("TogglePrevious"), Ok(Some(Cmd::StepOutput(-1))));
        assert_eq!(parse_command(" ToggleMute "), Ok(Some(Cmd::ToggleMute)));
        assert_eq!(parse_command("GetIsMuted"), Ok(None));
        assert!(parse_command("ChangeVolume 0").is_err());
        assert!(parse_command("SetVolume x").is_err());
        assert!(parse_command("Explode 1").is_err());
        assert!(parse_command("Explode").is_err());
    }

    #[test]
    fn execute_semantics_with_a_fake_backend() {
        let f = Fake::new(0.5, false);
        execute(f.as_ref(), &Cmd::ChangeVolume(10)).unwrap();
        execute(f.as_ref(), &Cmd::ChangeVolume(100)).unwrap();
        execute(f.as_ref(), &Cmd::SetVolume(-3)).unwrap();
        execute(f.as_ref(), &Cmd::ToggleMute).unwrap();
        execute(f.as_ref(), &Cmd::SetVolume(30)).unwrap(); // unmutes first
        execute(f.as_ref(), &Cmd::Mute).unwrap();
        execute(f.as_ref(), &Cmd::Unmute).unwrap();
        assert_eq!(f.calls(), ["vol 0.60", "vol 1.00", "vol 0.00", "mute true", "mute false", "vol 0.30", "mute true", "mute false"]);
        let g = Fake::new(0.5, false);
        execute(g.as_ref(), &Cmd::StepOutput(-1)).unwrap();
        execute(g.as_ref(), &Cmd::StepOutput(1)).unwrap();
        execute(g.as_ref(), &Cmd::SetOutput(99)).unwrap();
        execute(g.as_ref(), &Cmd::SetOutput(0)).unwrap();
        assert_eq!(g.calls(), ["out bt.head", "out alsa.speakers", "out bt.head", "out alsa.speakers"]);
        let mut bad = AudioState::default();
        bad.valid = false;
        struct Dead;
        impl AudioBackend for Dead {
            fn state(&self) -> AudioState {
                AudioState::default()
            }
            fn set_volume(&self, _: f64) -> Result<(), String> {
                panic!("must not be called")
            }
            fn set_mute(&self, _: bool) -> Result<(), String> {
                panic!("must not be called")
            }
            fn set_default_output(&self, _: &str) -> Result<(), String> {
                panic!("must not be called")
            }
        }
        assert!(execute(&Dead, &Cmd::Mute).is_err());
    }

    fn settle(m: &mut AudioMeasure, sk: &crate::measures::testutil::TestSkin, want: impl Fn(f64) -> bool) -> f64 {
        for _ in 0..400 {
            let v = ctx_run(sk, |c| m.update(c));
            if want(v) && m.engine.as_ref().unwrap().idle() {
                return v;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("did not settle")
    }

    #[test]
    fn measure_reports_and_obeys_bangs() {
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=Win7AudioPlugin\n");
        let fake = Fake::new(0.42, false);
        let mut m = AudioMeasure::with_engine(Engine::new(fake.clone(), Duration::from_millis(1)));
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        assert_eq!(settle(&mut m, &sk, |v| v == 42.0), 42.0);
        assert_eq!(m.string_value().as_deref(), Some("Speakers"));
        assert!(fake.calls().is_empty(), "reading must never change the system");
        m.command("ChangeVolume 8");
        assert_eq!(settle(&mut m, &sk, |v| v == 50.0), 50.0);
        m.command("ToggleMute");
        assert_eq!(settle(&mut m, &sk, |v| v == -1.0), -1.0);
        m.command("ToggleNext");
        for _ in 0..400 {
            ctx_run(&sk, |c| m.update(c));
            if m.string_value().as_deref() == Some("HDMI") {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(m.string_value().as_deref(), Some("HDMI"));
        m.command("Bogus 1");
        assert_eq!(m.take_log(), ["Audio: Unknown bang"]);
        assert_eq!(fake.calls(), ["vol 0.50", "mute true", "out alsa.hdmi"]);
        assert_eq!((m.forced_min(), m.forced_max()), (Some(0.0), Some(100.0)));
    }

    #[test]
    fn invalid_backend_shows_error() {
        struct Dead;
        impl AudioBackend for Dead {
            fn state(&self) -> AudioState {
                AudioState::default()
            }
            fn set_volume(&self, _: f64) -> Result<(), String> {
                Err("x".into())
            }
            fn set_mute(&self, _: bool) -> Result<(), String> {
                Err("x".into())
            }
            fn set_default_output(&self, _: &str) -> Result<(), String> {
                Err("x".into())
            }
        }
        let sk = skin("[M]\nMeasure=Plugin\n");
        let mut m = AudioMeasure::with_engine(Engine::new(Arc::new(Dead), Duration::from_millis(1)));
        ctx_run(&sk, |c| m.update(c));
        assert_eq!(ctx_run(&sk, |c| m.update(c)), 0.0);
        assert_eq!(m.string_value().as_deref(), Some("ERROR"));
        m.command("Mute");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(m.take_log(), ["Audio: no sound server found"]);
    }
}
