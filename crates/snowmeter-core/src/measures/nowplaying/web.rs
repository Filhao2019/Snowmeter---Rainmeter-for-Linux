//! WebNowPlaying / GPMDPPlugin replacement: the browser-extension bridges
//! become views of the same MPRIS hub the NowPlaying plugin uses (browsers
//! export their media sessions over MPRIS).

use super::{format_time, mpris, select, Call, Hub, LoopStatus, Playback, PlayerData};
use crate::measures::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WKind {
    Status,
    Player,
    State,
    Title,
    Artist,
    Album,
    Cover,
    CoverWebAddress,
    Duration,
    Position,
    Progress,
    Volume,
    Rating,
    Repeat,
    Shuffle,
    Unknown,
}

pub fn parse_wkind(s: &str) -> WKind {
    match s.trim().to_ascii_uppercase().as_str() {
        "STATUS" => WKind::Status,
        "PLAYER" => WKind::Player,
        "STATE" => WKind::State,
        "TITLE" => WKind::Title,
        "ARTIST" => WKind::Artist,
        "ALBUM" => WKind::Album,
        "COVER" => WKind::Cover,
        "COVERWEBADDRESS" => WKind::CoverWebAddress,
        "DURATION" => WKind::Duration,
        "POSITION" => WKind::Position,
        "PROGRESS" => WKind::Progress,
        "VOLUME" => WKind::Volume,
        "RATING" => WKind::Rating,
        "REPEAT" => WKind::Repeat,
        "SHUFFLE" => WKind::Shuffle,
        _ => WKind::Unknown,
    }
}

/// Pick the player a web measure follows: the playing one, else a paused one.
fn pick(players: &[PlayerData]) -> Option<&PlayerData> {
    select(players, "")
}

/// Number and string of one type. Without a player everything is empty/0.
pub fn web_value(kind: WKind, p: Option<&PlayerData>) -> (f64, Option<String>) {
    let text = |s: &str| (0.0, Some(s.to_string()));
    let Some(p) = p else {
        return match kind {
            WKind::Duration | WKind::Position => (0.0, Some("0:00".into())),
            WKind::Unknown | WKind::Status | WKind::State | WKind::Progress | WKind::Volume | WKind::Rating
            | WKind::Repeat | WKind::Shuffle => (0.0, None),
            _ => text(""),
        };
    };
    match kind {
        WKind::Status => (1.0, None),
        WKind::Player => text(&p.identity),
        WKind::Title => text(&p.title),
        WKind::Artist => text(&p.artist),
        WKind::Album => text(&p.album),
        WKind::Cover => text(&p.cover),
        WKind::CoverWebAddress => text(&p.cover_url),
        WKind::Duration => {
            let s = p.length_us.max(0) as f64 / 1e6;
            (s.floor(), Some(format_time(s, true)))
        }
        WKind::Position => {
            let s = p.position_us.max(0) as f64 / 1e6;
            (s.floor(), Some(format_time(s, true)))
        }
        WKind::Progress => (super::progress(p.position_us, p.length_us), None),
        WKind::Volume => ((p.volume * 100.0).round().clamp(0.0, 100.0), None),
        WKind::Rating => ((p.rating * 5.0).round().clamp(0.0, 5.0), None),
        // 0 off, 1 repeat all, 2 repeat one.
        WKind::Repeat => (
            match p.loop_status {
                LoopStatus::None => 0.0,
                LoopStatus::Playlist => 1.0,
                LoopStatus::Track => 2.0,
            },
            None,
        ),
        WKind::Shuffle => (f64::from(p.shuffle), None),
        // 0 stopped, 1 playing, 2 paused.
        WKind::State => (
            match p.playback {
                Playback::Stopped => 0.0,
                Playback::Playing => 1.0,
                Playback::Paused => 2.0,
            },
            None,
        ),
        WKind::Unknown => text(""),
    }
}

/// `!CommandMeasure` text to a player command. `Ok(None)` = accepted, nothing to send.
pub fn web_dispatch(players: &[PlayerData], args: &str) -> Result<Option<(String, Call)>, String> {
    let mut it = args.split_whitespace();
    let cmd = it.next().unwrap_or("").to_ascii_lowercase();
    let arg = it.next().unwrap_or("");
    let Some(p) = pick(players) else { return Ok(None) };
    let amount = |what: &str| -> Result<(f64, bool), String> {
        let rel = arg.starts_with('+') || arg.starts_with('-');
        arg.parse::<f64>().map(|v| (v, rel)).map_err(|_| format!("WebNowPlaying: {what} needs a number"))
    };
    let call = match cmd.as_str() {
        "playpause" | "toggleplaypause" => Call::PlayPause,
        "play" => Call::Play,
        "pause" => Call::Pause,
        "stop" => Call::Stop,
        "next" => Call::Next,
        "previous" | "prev" => Call::Previous,
        "setposition" => {
            let (v, rel) = amount("SetPosition")?;
            let cur = super::progress(p.position_us, p.length_us);
            let pct = if rel { cur + v } else { v }.clamp(0.0, 100.0);
            Call::SetPosition { track_id: p.track_id.clone(), micros: (p.length_us.max(0) as f64 * pct / 100.0) as i64 }
        }
        "setvolume" => {
            let (v, rel) = amount("SetVolume")?;
            let cur = p.volume * 100.0;
            Call::SetVolume(if rel { cur + v } else { v }.clamp(0.0, 100.0) / 100.0)
        }
        "shuffle" | "toggleshuffle" | "setshuffle" => Call::SetShuffle(!p.shuffle),
        "repeat" | "togglerepeat" | "setrepeat" => Call::SetLoop(match p.loop_status {
            LoopStatus::None => LoopStatus::Playlist,
            LoopStatus::Playlist => LoopStatus::Track,
            LoopStatus::Track => LoopStatus::None,
        }),
        // MPRIS has no rating verbs.
        "togglethumbsup" | "togglethumbsdown" | "rating" | "setrating" | "like" | "dislike" => return Ok(None),
        other => return Err(format!("WebNowPlaying: unknown command '{other}'")),
    };
    Ok(Some((p.bus.clone(), call)))
}

pub struct WebNowPlayingMeasure {
    hub: Arc<dyn Hub>,
    kind: WKind,
    string: Option<String>,
    log: Vec<String>,
}

impl WebNowPlayingMeasure {
    pub fn new() -> WebNowPlayingMeasure {
        WebNowPlayingMeasure::with_hub(mpris::shared_hub())
    }

    pub fn with_hub(hub: Arc<dyn Hub>) -> WebNowPlayingMeasure {
        WebNowPlayingMeasure { hub, kind: WKind::Unknown, string: None, log: Vec::new() }
    }
}

impl Default for WebNowPlayingMeasure {
    fn default() -> Self {
        WebNowPlayingMeasure::new()
    }
}

impl MeasureImpl for WebNowPlayingMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let mut ty = opt.string("PlayerType", "");
        if ty.trim().is_empty() {
            ty = opt.string("Type", "");
        }
        self.kind = parse_wkind(&ty);
        if self.kind == WKind::Unknown {
            self.log.push(format!("WebNowPlaying: [{}] PlayerType \"{ty}\" is not valid", opt.section));
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let players = self.hub.snapshot();
        let (v, s) = web_value(self.kind, pick(&players));
        self.string = s;
        v
    }

    fn string_value(&self) -> Option<String> {
        self.string.clone()
    }

    fn forced_max(&self) -> Option<f64> {
        match self.kind {
            WKind::Progress | WKind::Volume => Some(100.0),
            WKind::Rating => Some(5.0),
            WKind::State | WKind::Repeat => Some(2.0),
            _ => None,
        }
    }

    fn forced_min(&self) -> Option<f64> {
        Some(0.0)
    }

    fn command(&mut self, args: &str) {
        let players = self.hub.snapshot();
        match web_dispatch(&players, args) {
            Ok(Some((bus, call))) => self.hub.send(&bus, call),
            Ok(None) => {}
            Err(e) => self.log.push(e),
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pb: Playback) -> PlayerData {
        PlayerData {
            bus: "org.mpris.MediaPlayer2.chromium".into(),
            identity: "Chromium".into(),
            playback: pb,
            title: "T".into(),
            artist: "A".into(),
            cover_url: "https://x/y.jpg".into(),
            cover: "/tmp/y.jpg".into(),
            length_us: 100_000_000,
            position_us: 25_000_000,
            volume: 0.4,
            loop_status: LoopStatus::Track,
            track_id: "/t".into(),
            ..Default::default()
        }
    }

    #[test]
    fn values() {
        let pl = p(Playback::Playing);
        assert_eq!(web_value(WKind::Player, Some(&pl)).1.as_deref(), Some("Chromium"));
        assert_eq!(web_value(WKind::CoverWebAddress, Some(&pl)).1.as_deref(), Some("https://x/y.jpg"));
        assert_eq!(web_value(WKind::Duration, Some(&pl)), (100.0, Some("1:40".into())));
        assert_eq!(web_value(WKind::Progress, Some(&pl)).0, 25.0);
        assert_eq!(web_value(WKind::Repeat, Some(&pl)).0, 2.0);
        assert_eq!(web_value(WKind::State, Some(&pl)).0, 1.0);
        assert_eq!(web_value(WKind::Status, None).0, 0.0);
        assert_eq!(web_value(WKind::Title, None).1.as_deref(), Some(""));
        assert_eq!(parse_wkind("coverwebaddress"), WKind::CoverWebAddress);
        assert_eq!(parse_wkind("zzz"), WKind::Unknown);
    }

    #[test]
    fn commands() {
        let ps = vec![p(Playback::Playing)];
        let d = |a: &str| web_dispatch(&ps, a).map(|o| o.map(|x| x.1));
        assert_eq!(d("PlayPause"), Ok(Some(Call::PlayPause)));
        assert_eq!(d("Next"), Ok(Some(Call::Next)));
        assert_eq!(d("SetVolume 80"), Ok(Some(Call::SetVolume(0.8))));
        assert_eq!(d("SetPosition 50"), Ok(Some(Call::SetPosition { track_id: "/t".into(), micros: 50_000_000 })));
        assert_eq!(d("Shuffle"), Ok(Some(Call::SetShuffle(true))));
        assert_eq!(d("Repeat"), Ok(Some(Call::SetLoop(LoopStatus::None))));
        assert_eq!(d("ToggleThumbsUp"), Ok(None));
        assert!(d("SetVolume x").is_err());
        assert!(d("Bogus").is_err());
        assert_eq!(web_dispatch(&[], "Next"), Ok(None));
    }

    #[test]
    fn plugin_names_resolve() {
        use crate::measures::plugin::{canonical_plugin_name, is_native_plugin};
        for n in ["WebNowPlaying", "WebNowPlaying.dll", "GPMDPPlugin", "GPMDPPlugin.dll"] {
            assert_eq!(canonical_plugin_name(n), Some("webnowplaying"), "{n}");
            assert!(is_native_plugin(n), "{n}");
        }
    }
}
