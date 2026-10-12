//! AppVolume plugin: volume of each application playing sound, from the
//! PulseAudio / PipeWire streams (`pactl list sink-inputs`).
//!
//! The parent measure (`Plugin=AppVolume`) takes a snapshot on a worker thread
//! each time it updates; streams of the same application are grouped. Child
//! measures (`Parent=`, `Index=N`, 1-based) return that app's volume (0-100,
//! -1 while muted) and its name as the string. `IgnoreSystemSound=1` skips
//! event sounds. Commands on a child: `SetVolume N`, `SetVolume +N|-N`,
//! `ChangeVolume N`, `ToggleMute`, `Mute`, `Unmute`.

use super::bgshared::{self, Bg};
use super::extcmd::{have, run_capture, run_status};
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct App {
    pub name: String,
    /// `pactl` sink-input ids of this app's streams.
    pub ids: Vec<u32>,
    /// 0-100 (can exceed 100 when boosted).
    pub volume: f64,
    pub muted: bool,
    pub event: bool,
}

/// Parse `LC_ALL=C pactl list sink-inputs`.
pub fn parse_sink_inputs(out: &str) -> Vec<App> {
    let mut streams: Vec<App> = Vec::new();
    let mut binary = String::new();
    let finish = |streams: &mut Vec<App>, binary: &mut String| {
        if let Some(last) = streams.last_mut() {
            if last.name.is_empty() {
                last.name = std::mem::take(binary);
            }
        }
        binary.clear();
    };
    for line in out.lines() {
        let l = line.trim();
        if let Some(id) = l.strip_prefix("Sink Input #") {
            finish(&mut streams, &mut binary);
            streams.push(App { ids: id.trim().parse().ok().into_iter().collect(), ..Default::default() });
            continue;
        }
        let Some(cur) = streams.last_mut() else { continue };
        if let Some(v) = l.strip_prefix("Mute:") {
            cur.muted = v.trim() == "yes";
        } else if let Some(v) = l.strip_prefix("Volume:") {
            let pcts: Vec<f64> = v.split('/').filter_map(|p| p.trim().strip_suffix('%')?.trim().parse().ok()).collect();
            if !pcts.is_empty() {
                cur.volume = pcts.iter().sum::<f64>() / pcts.len() as f64;
            }
        } else if let Some((k, v)) = l.split_once(" = ") {
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "application.name" => cur.name = v,
                "application.process.binary" => binary = v,
                "media.role" => cur.event |= v == "event",
                _ => {}
            }
        }
    }
    finish(&mut streams, &mut binary);
    // One entry per application, in first-seen order.
    let mut apps: Vec<App> = Vec::new();
    for s in streams {
        match apps.iter_mut().find(|a| a.name == s.name && a.event == s.event) {
            Some(a) => {
                a.ids.extend(s.ids);
                a.muted &= s.muted;
            }
            None => apps.push(s),
        }
    }
    apps
}

fn snapshot() -> Vec<App> {
    if !have("pactl") {
        return Vec::new();
    }
    run_capture("pactl", &["list", "sink-inputs"], Duration::from_secs(3)).map(|o| parse_sink_inputs(&o)).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    Set(f64),
    Change(f64),
    Mute(Option<bool>),
}

fn parse_command(args: &str) -> Option<Cmd> {
    let mut it = args.split_whitespace();
    let verb = it.next()?.to_ascii_lowercase();
    let arg = it.next().unwrap_or("");
    let num = || arg.trim_start_matches('+').parse::<f64>().ok();
    match verb.as_str() {
        "setvolume" if arg.starts_with(['+', '-']) => num().map(Cmd::Change),
        "setvolume" => num().map(Cmd::Set),
        "changevolume" => num().map(Cmd::Change),
        "togglemute" => Some(Cmd::Mute(None)),
        "mute" => Some(Cmd::Mute(Some(true))),
        "unmute" => Some(Cmd::Mute(Some(false))),
        _ => None,
    }
}

/// Apply `cmd` to the app's streams; runs `pactl` (call off the UI thread).
fn apply(app: &App, cmd: &Cmd) {
    for id in &app.ids {
        let id = id.to_string();
        let _ = match cmd {
            Cmd::Set(v) => run_status("pactl", &["set-sink-input-volume", &id, &format!("{}%", v.clamp(0.0, 150.0).round())], Duration::from_secs(3)),
            Cmd::Change(d) => {
                let target = (app.volume + d).clamp(0.0, 150.0).round();
                run_status("pactl", &["set-sink-input-volume", &id, &format!("{target}%")], Duration::from_secs(3))
            }
            Cmd::Mute(m) => {
                let v = match m {
                    None => "toggle",
                    Some(true) => "1",
                    Some(false) => "0",
                };
                run_status("pactl", &["set-sink-input-mute", &id, v], Duration::from_secs(3))
            }
        };
    }
}

/// What the app looks like right after `cmd`, so the skin reacts at once.
fn predict(app: &mut App, cmd: &Cmd) {
    match cmd {
        Cmd::Set(v) => app.volume = v.clamp(0.0, 150.0).round(),
        Cmd::Change(d) => app.volume = (app.volume + d).clamp(0.0, 150.0).round(),
        Cmd::Mute(m) => app.muted = m.unwrap_or(!app.muted),
    }
}

type Shared = Bg<Vec<App>>;

pub struct AppVolume {
    own: String,
    parent: String,
    index: usize,
    ignore_events: bool,
    apps: Shared,
    published: bool,
    skin_dir: std::path::PathBuf,
    current: Option<App>,
    log: Vec<String>,
}

impl AppVolume {
    pub fn new() -> AppVolume {
        AppVolume {
            own: String::new(),
            parent: String::new(),
            index: 1,
            ignore_events: false,
            apps: Bg::new(Vec::new()),
            published: false,
            skin_dir: Default::default(),
            current: None,
            log: Vec::new(),
        }
    }

    fn shared(&self) -> Option<Arc<Shared>> {
        if self.parent.is_empty() {
            Some(Arc::new(self.apps.clone()))
        } else {
            bgshared::lookup::<Shared>(&self.skin_dir, &self.parent)
        }
    }
}

impl Default for AppVolume {
    fn default() -> Self {
        AppVolume::new()
    }
}

impl MeasureImpl for AppVolume {
    fn read_options(&mut self, opt: &OptionReader) {
        self.own = opt.section.to_string();
        self.parent = opt.string("Parent", "").trim().to_string();
        self.index = opt.int("Index", 1).max(1) as usize;
        self.ignore_events = opt.bool("IgnoreSystemSound", true);
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        self.skin_dir = ctx.skin_dir.to_path_buf();
        if self.parent.is_empty() {
            if !self.published {
                bgshared::publish(ctx.skin_dir, &self.own, Arc::new(self.apps.clone()));
                self.published = true;
                if !have("pactl") {
                    self.log.push("AppVolume: pactl not found (install pulseaudio-utils or pipewire-pulse)".into());
                }
            }
            let ignore = self.ignore_events;
            let grace = if self.current.is_none() { Duration::from_millis(500) } else { Duration::ZERO };
            self.apps.start(grace, move || snapshot().into_iter().filter(|a| !(ignore && a.event)).collect());
            self.current = Some(App::default());
            return self.apps.get().len() as f64;
        }
        let apps = self.shared().map(|s| s.get()).unwrap_or_default();
        self.current = apps.get(self.index - 1).cloned();
        match &self.current {
            Some(a) if a.muted => -1.0,
            Some(a) => a.volume.round(),
            None => 0.0,
        }
    }

    fn string_value(&self) -> Option<String> {
        Some(if self.parent.is_empty() { String::new() } else { self.current.as_ref().map(|a| a.name.clone()).unwrap_or_default() })
    }

    fn default_max(&self) -> f64 {
        100.0
    }

    fn command(&mut self, args: &str) {
        let Some(cmd) = parse_command(args) else {
            self.log.push(format!("AppVolume: unknown command \"{args}\""));
            return;
        };
        let (Some(app), Some(shared)) = (self.current.clone().filter(|a| !a.ids.is_empty()), self.shared()) else { return };
        shared.modify(|apps| {
            if let Some(a) = apps.iter_mut().find(|a| a.ids == app.ids) {
                predict(a, &cmd);
            }
        });
        let _ = std::thread::Builder::new().name("snowmeter-appvolume".into()).spawn(move || apply(&app, &cmd));
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "Sink Input #42
\tDriver: PipeWire
\tMute: no
\tVolume: front-left: 52429 /  80% / -5.81 dB,   front-right: 52429 /  80% / -5.81 dB
\t        balance 0.00
\tProperties:
\t\tapplication.name = \"Firefox\"
\t\tapplication.process.binary = \"firefox\"
Sink Input #43
\tMute: yes
\tVolume: mono: 65536 / 100% / 0.00 dB
\tProperties:
\t\tapplication.process.binary = \"mpv\"
Sink Input #44
\tMute: no
\tVolume: front-left: 65536 / 100% / 0.00 dB,   front-right: 32768 /  50% / -18.06 dB
\tProperties:
\t\tapplication.name = \"Firefox\"
Sink Input #45
\tMute: no
\tVolume: mono: 65536 / 100% / 0.00 dB
\tProperties:
\t\tapplication.name = \"GNOME Shell\"
\t\tmedia.role = \"event\"
";

    #[test]
    fn groups_streams_by_application() {
        let apps = parse_sink_inputs(OUT);
        assert_eq!(apps.len(), 3);
        assert_eq!(apps[0].name, "Firefox");
        assert_eq!(apps[0].ids, vec![42, 44]);
        assert_eq!(apps[0].volume, 80.0);
        assert!(!apps[0].muted);
        assert_eq!(apps[1].name, "mpv");
        assert!(apps[1].muted);
        assert!(apps[2].event);
    }

    #[test]
    fn commands() {
        assert_eq!(parse_command("SetVolume 40"), Some(Cmd::Set(40.0)));
        assert_eq!(parse_command("SetVolume +5"), Some(Cmd::Change(5.0)));
        assert_eq!(parse_command("SetVolume -5"), Some(Cmd::Change(-5.0)));
        assert_eq!(parse_command("ChangeVolume 3"), Some(Cmd::Change(3.0)));
        assert_eq!(parse_command("ToggleMute"), Some(Cmd::Mute(None)));
        assert_eq!(parse_command("bogus"), None);
        let mut a = App { volume: 98.0, ..Default::default() };
        predict(&mut a, &Cmd::Change(5.0));
        assert_eq!(a.volume, 103.0);
        predict(&mut a, &Cmd::Mute(None));
        assert!(a.muted);
    }
}
