//! NowPlaying plugin replacement. Every Windows `PlayerName` maps onto MPRIS
//! players on the D-Bus session bus; a shared background thread (see `mpris`)
//! publishes snapshots which measures read in `update`.

pub mod mpris;
pub mod spotify;
pub mod web;

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Playback {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoopStatus {
    #[default]
    None,
    Track,
    Playlist,
}

/// One MPRIS player as last observed.
#[derive(Clone, Debug, Default)]
pub struct PlayerData {
    pub bus: String,
    pub identity: String,
    pub playback: Playback,
    pub artist: String,
    pub album: String,
    pub title: String,
    pub number: u32,
    pub year: String,
    pub genre: String,
    pub file: String,
    /// Local path of the cover (resolved/downloaded by the service), or "".
    pub cover: String,
    /// The original `mpris:artUrl` (what WebNowPlaying calls `CoverWebAddress`).
    pub cover_url: String,
    pub length_us: i64,
    pub position_us: i64,
    /// `xesam:userRating`, 0..1.
    pub rating: f64,
    /// 0..1.
    pub volume: f64,
    pub shuffle: bool,
    pub loop_status: LoopStatus,
    pub track_id: String,
}

/// A command for one player.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    SetPosition { track_id: String, micros: i64 },
    SetVolume(f64),
    SetShuffle(bool),
    SetLoop(LoopStatus),
    Raise,
    Quit,
}

/// D-Bus access, kept behind a trait so the logic is testable.
pub trait Backend: Send {
    fn poll(&mut self) -> Result<Vec<PlayerData>, String>;
    fn call(&mut self, bus: &str, call: &Call) -> Result<(), String>;
}

/// What measures talk to: the published snapshot and the command channel.
pub trait Hub: Send + Sync {
    fn snapshot(&self) -> Vec<PlayerData>;
    fn send(&self, bus: &str, call: Call);
    /// Remember which `PlayerName` the measure `measure` uses (for `[Parent]` children).
    fn register(&self, measure: &str, player_name: &str);
    fn parent_player(&self, measure: &str) -> Option<String>;
}

/// Player for a Windows-style `PlayerName`: bus name or identity contains it
/// (case-insensitive), else first Playing, else first Paused, else any.
pub fn select<'a>(players: &'a [PlayerData], name: &str) -> Option<&'a PlayerData> {
    let n = name.trim().to_ascii_lowercase();
    if !n.is_empty() {
        let hit = players
            .iter()
            .find(|p| p.bus.to_ascii_lowercase().contains(&n) || p.identity.to_ascii_lowercase().contains(&n));
        if hit.is_some() {
            return hit;
        }
    }
    players
        .iter()
        .find(|p| p.playback == Playback::Playing)
        .or_else(|| players.iter().find(|p| p.playback == Playback::Paused))
        .or_else(|| players.first())
}

/// `mm:ss` (`m:ss` with `disable_leading_zero`), `h:mm:ss` from one hour up.
pub fn format_time(secs: f64, disable_leading_zero: bool) -> String {
    let t = if secs.is_finite() && secs > 0.0 { secs as u64 } else { 0 };
    let (h, m, s) = (t / 3600, (t / 60) % 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else if disable_leading_zero {
        format!("{m}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// 0..100.
pub fn progress(position_us: i64, length_us: i64) -> f64 {
    if length_us <= 0 {
        return 0.0;
    }
    (position_us.max(0) as f64 / length_us as f64 * 100.0).clamp(0.0, 100.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Artist,
    Album,
    Title,
    Number,
    Year,
    Genre,
    File,
    Cover,
    Duration,
    Position,
    Progress,
    Rating,
    Repeat,
    Shuffle,
    State,
    Status,
    Volume,
    Lyrics,
    Unknown,
}

fn parse_kind(s: &str) -> Kind {
    match s.trim().to_ascii_uppercase().as_str() {
        "ARTIST" => Kind::Artist,
        "ALBUM" => Kind::Album,
        "TITLE" => Kind::Title,
        "NUMBER" => Kind::Number,
        "YEAR" => Kind::Year,
        "GENRE" => Kind::Genre,
        "FILE" => Kind::File,
        "COVER" => Kind::Cover,
        "DURATION" => Kind::Duration,
        "POSITION" => Kind::Position,
        "PROGRESS" => Kind::Progress,
        "RATING" => Kind::Rating,
        "REPEAT" => Kind::Repeat,
        "SHUFFLE" => Kind::Shuffle,
        "STATE" => Kind::State,
        "STATUS" => Kind::Status,
        "VOLUME" => Kind::Volume,
        "LYRICS" => Kind::Lyrics,
        _ => Kind::Unknown,
    }
}

/// Number and string for `kind` from the chosen player (None = no player).
pub fn value_of(kind: Kind, p: Option<&PlayerData>, dlz: bool) -> (f64, Option<String>) {
    let Some(p) = p else {
        return (0.0, if matches!(kind, Kind::Duration | Kind::Position) { Some(format_time(0.0, dlz)) } else { None });
    };
    let text = |s: &str| (0.0, Some(s.to_string()));
    match kind {
        Kind::Artist => text(&p.artist),
        Kind::Album => text(&p.album),
        Kind::Title => text(&p.title),
        Kind::Genre => text(&p.genre),
        Kind::File => text(&p.file),
        Kind::Cover => text(&p.cover),
        Kind::Lyrics | Kind::Unknown => text(""),
        Kind::Number => (p.number as f64, Some(p.number.to_string())),
        Kind::Year => (p.year.parse().unwrap_or(0.0), Some(p.year.clone())),
        Kind::Duration => {
            let s = p.length_us.max(0) as f64 / 1e6;
            (s.floor(), Some(format_time(s, dlz)))
        }
        Kind::Position => {
            let s = p.position_us.max(0) as f64 / 1e6;
            (s.floor(), Some(format_time(s, dlz)))
        }
        Kind::Progress => (progress(p.position_us, p.length_us), None),
        Kind::Rating => ((p.rating * 5.0).round().clamp(0.0, 5.0), None),
        Kind::Repeat => (f64::from(p.loop_status != LoopStatus::None), None),
        Kind::Shuffle => (f64::from(p.shuffle), None),
        Kind::State => (
            match p.playback {
                Playback::Stopped => 0.0,
                Playback::Playing => 1.0,
                Playback::Paused => 2.0,
            },
            None,
        ),
        Kind::Status => (1.0, None),
        Kind::Volume => ((p.volume * 100.0).round().clamp(0.0, 100.0), None),
    }
}

/// Parse an absolute or `+N`/`-N` relative number; returns (value, relative).
fn parse_amount(s: &str) -> Option<(f64, bool)> {
    let s = s.trim();
    let rel = s.starts_with('+') || s.starts_with('-');
    s.parse::<f64>().ok().map(|v| (v, rel))
}

/// Translate `!CommandMeasure` arguments into a player command.
pub fn dispatch(players: &[PlayerData], name: &str, args: &str) -> Result<Option<(String, Call)>, String> {
    let mut it = args.split_whitespace();
    let cmd = it.next().unwrap_or("").to_ascii_lowercase();
    let arg = it.next().unwrap_or("");
    let Some(p) = select(players, name) else {
        return Ok(None);
    };
    let call = match cmd.as_str() {
        "play" => Call::Play,
        "pause" => Call::Pause,
        "playpause" => Call::PlayPause,
        "stop" => Call::Stop,
        "next" => Call::Next,
        "previous" => Call::Previous,
        "setposition" => {
            let (v, rel) = parse_amount(arg).ok_or("SetPosition needs a number")?;
            let cur = progress(p.position_us, p.length_us);
            let pct = if rel { cur + v } else { v }.clamp(0.0, 100.0);
            Call::SetPosition { track_id: p.track_id.clone(), micros: (p.length_us.max(0) as f64 * pct / 100.0) as i64 }
        }
        "setvolume" => {
            let (v, rel) = parse_amount(arg).ok_or("SetVolume needs a number")?;
            let cur = p.volume * 100.0;
            Call::SetVolume(if rel { cur + v } else { v }.clamp(0.0, 100.0) / 100.0)
        }
        "setshuffle" => Call::SetShuffle(match arg.trim() {
            "0" => false,
            "1" => true,
            _ => !p.shuffle,
        }),
        "setrepeat" => Call::SetLoop(match arg.trim() {
            "0" => LoopStatus::None,
            "1" => LoopStatus::Playlist,
            _ if p.loop_status == LoopStatus::None => LoopStatus::Playlist,
            _ => LoopStatus::None,
        }),
        "openplayer" => Call::Raise,
        "closeplayer" | "toggleplayer" => Call::Quit,
        other => return Err(format!("NowPlaying: unknown command '{other}'")),
    };
    Ok(Some((p.bus.clone(), call)))
}

pub struct NowPlayingMeasure {
    hub: Arc<dyn Hub>,
    kind: Kind,
    name: String,
    section: String,
    /// `PlayerName=[Parent]`: measure name of the parent.
    parent: Option<String>,
    dlz: bool,
    track_change: String,
    last_track: Option<String>,
    string: Option<String>,
    actions: Vec<String>,
    log: Vec<String>,
}

impl NowPlayingMeasure {
    pub fn new() -> NowPlayingMeasure {
        NowPlayingMeasure::with_hub(mpris::shared_hub())
    }

    pub fn with_hub(hub: Arc<dyn Hub>) -> NowPlayingMeasure {
        NowPlayingMeasure {
            hub,
            kind: Kind::Unknown,
            name: String::new(),
            section: String::new(),
            parent: None,
            dlz: false,
            track_change: String::new(),
            last_track: None,
            string: None,
            actions: Vec::new(),
            log: Vec::new(),
        }
    }

    fn player_name(&self) -> String {
        match &self.parent {
            Some(p) => self.hub.parent_player(p).unwrap_or_default(),
            None => self.name.clone(),
        }
    }
}

impl Default for NowPlayingMeasure {
    fn default() -> Self {
        NowPlayingMeasure::new()
    }
}

impl MeasureImpl for NowPlayingMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let raw = opt.without_sections().string("PlayerName", "");
        let raw = raw.trim();
        self.section = opt.section.to_string();
        if raw.len() > 2 && raw.starts_with('[') && raw.ends_with(']') {
            self.parent = Some(raw[1..raw.len() - 1].to_ascii_lowercase());
            self.name.clear();
        } else {
            self.parent = None;
            self.name = raw.to_string();
        }
        self.hub.register(&self.section.to_ascii_lowercase(), &self.player_name());
        let ty = opt.string("PlayerType", "");
        self.kind = parse_kind(&ty);
        if self.kind == Kind::Unknown {
            self.log.push(format!("NowPlaying: [{}] PlayerType \"{ty}\" is not valid", opt.section));
        }
        self.dlz = opt.int("DisableLeadingZero", 0) != 0;
        self.track_change = opt.action("TrackChangeAction");
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let players = self.hub.snapshot();
        let sel = select(&players, &self.player_name());
        if !self.track_change.is_empty() {
            let key = sel.map(|p| format!("{}\u{1}{}\u{1}{}\u{1}{}", p.title, p.artist, p.album, p.file));
            if let (Some(old), Some(new)) = (&self.last_track, &key) {
                if old != new {
                    self.actions.push(self.track_change.clone());
                }
            }
            if key.is_some() {
                self.last_track = key;
            }
        }
        let (v, s) = value_of(self.kind, sel, self.dlz);
        self.string = s;
        v
    }

    fn string_value(&self) -> Option<String> {
        self.string.clone()
    }

    fn forced_max(&self) -> Option<f64> {
        match self.kind {
            Kind::Progress | Kind::Volume => Some(100.0),
            Kind::Rating => Some(5.0),
            Kind::State => Some(2.0),
            _ => None,
        }
    }

    fn forced_min(&self) -> Option<f64> {
        Some(0.0)
    }

    fn command(&mut self, args: &str) {
        let players = self.hub.snapshot();
        match dispatch(&players, &self.player_name(), args) {
            Ok(Some((bus, call))) => self.hub.send(&bus, call),
            Ok(None) => {}
            Err(e) => self.log.push(e),
        }
    }

    fn take_actions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.actions)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeHub {
        players: Mutex<Vec<PlayerData>>,
        sent: Mutex<Vec<(String, Call)>>,
        names: Mutex<HashMap<String, String>>,
    }
    impl Hub for FakeHub {
        fn snapshot(&self) -> Vec<PlayerData> {
            self.players.lock().unwrap().clone()
        }
        fn send(&self, bus: &str, call: Call) {
            self.sent.lock().unwrap().push((bus.into(), call));
        }
        fn register(&self, m: &str, n: &str) {
            self.names.lock().unwrap().insert(m.into(), n.into());
        }
        fn parent_player(&self, m: &str) -> Option<String> {
            self.names.lock().unwrap().get(m).cloned()
        }
    }

    fn player(bus: &str, identity: &str, pb: Playback) -> PlayerData {
        PlayerData {
            bus: bus.into(),
            identity: identity.into(),
            playback: pb,
            title: "Song".into(),
            artist: "Band".into(),
            album: "Rec".into(),
            number: 3,
            year: "1999".into(),
            length_us: 200_000_000,
            position_us: 50_000_000,
            rating: 0.6,
            volume: 0.5,
            track_id: "/t/1".into(),
            ..Default::default()
        }
    }

    #[test]
    fn time_formatting() {
        assert_eq!(format_time(65.0, false), "01:05");
        assert_eq!(format_time(65.0, true), "1:05");
        assert_eq!(format_time(3725.0, false), "1:02:05");
        assert_eq!(format_time(-4.0, false), "00:00");
    }

    #[test]
    fn progress_math() {
        assert_eq!(progress(50, 200), 25.0);
        assert_eq!(progress(500, 200), 100.0);
        assert_eq!(progress(5, 0), 0.0);
    }

    #[test]
    fn selection_rules() {
        let ps = vec![
            player("org.mpris.MediaPlayer2.vlc", "VLC", Playback::Paused),
            player("org.mpris.MediaPlayer2.spotify", "Spotify", Playback::Stopped),
            player("org.mpris.MediaPlayer2.chrome", "Chrome", Playback::Playing),
        ];
        assert_eq!(select(&ps, "SPOTIFY").unwrap().identity, "Spotify");
        assert_eq!(select(&ps, "vlc").unwrap().identity, "VLC");
        assert_eq!(select(&ps, "Winamp").unwrap().identity, "Chrome");
        assert_eq!(select(&ps[..2], "Winamp").unwrap().identity, "VLC");
        assert_eq!(select(&ps[1..2], "").unwrap().identity, "Spotify");
        assert!(select(&[], "x").is_none());
    }

    #[test]
    fn metadata_mapping() {
        let p = player("b", "I", Playback::Playing);
        assert_eq!(value_of(Kind::Title, Some(&p), false).1.as_deref(), Some("Song"));
        assert_eq!(value_of(Kind::Number, Some(&p), false).0, 3.0);
        assert_eq!(value_of(Kind::Year, Some(&p), false).0, 1999.0);
        assert_eq!(value_of(Kind::Duration, Some(&p), false), (200.0, Some("03:20".into())));
        assert_eq!(value_of(Kind::Position, Some(&p), true), (50.0, Some("0:50".into())));
        assert_eq!(value_of(Kind::Progress, Some(&p), false).0, 25.0);
        assert_eq!(value_of(Kind::Rating, Some(&p), false).0, 3.0);
        assert_eq!(value_of(Kind::Volume, Some(&p), false).0, 50.0);
        assert_eq!(value_of(Kind::State, Some(&p), false).0, 1.0);
        assert_eq!(value_of(Kind::Status, Some(&p), false).0, 1.0);
        assert_eq!(value_of(Kind::Status, None, false).0, 0.0);
        assert_eq!(value_of(Kind::State, None, false).0, 0.0);
        assert_eq!(value_of(Kind::Lyrics, Some(&p), false).1.as_deref(), Some(""));
    }

    #[test]
    fn command_dispatch() {
        let mut p = player("bus", "I", Playback::Playing);
        let ps = vec![p.clone()];
        let d = |a: &str| dispatch(&ps, "", a).unwrap().map(|x| x.1);
        assert_eq!(d("PlayPause"), Some(Call::PlayPause));
        assert_eq!(d("next"), Some(Call::Next));
        assert_eq!(d("SetPosition 50"), Some(Call::SetPosition { track_id: "/t/1".into(), micros: 100_000_000 }));
        assert_eq!(d("SetPosition +25"), Some(Call::SetPosition { track_id: "/t/1".into(), micros: 100_000_000 }));
        assert_eq!(d("SetVolume 80"), Some(Call::SetVolume(0.8)));
        assert_eq!(d("SetVolume -20"), Some(Call::SetVolume(0.3)));
        assert_eq!(d("SetVolume +900"), Some(Call::SetVolume(1.0)));
        assert_eq!(d("SetShuffle"), Some(Call::SetShuffle(true)));
        assert_eq!(d("SetRepeat -1"), Some(Call::SetLoop(LoopStatus::Playlist)));
        p.loop_status = LoopStatus::Track;
        assert_eq!(dispatch(&[p], "", "SetRepeat -1").unwrap().unwrap().1, Call::SetLoop(LoopStatus::None));
        assert_eq!(d("ClosePlayer"), Some(Call::Quit));
        assert_eq!(d("OpenPlayer"), Some(Call::Raise));
        assert!(dispatch(&ps, "", "Bogus").is_err());
        assert!(dispatch(&[], "", "Play").unwrap().is_none());
    }

    #[test]
    fn measure_reads_snapshot_and_sends_commands() {
        let hub = Arc::new(FakeHub::default());
        hub.players.lock().unwrap().push(player("org.mpris.MediaPlayer2.x", "X", Playback::Paused));
        let s = skin("[Parent]\nMeasure=Plugin\nPlugin=NowPlaying\nPlayerName=Winamp\nPlayerType=TITLE\n[Child]\nMeasure=Plugin\nPlugin=NowPlaying\nPlayerName=[Parent]\nPlayerType=STATE\nTrackChangeAction=!Log hi\n");
        let mk = |sec: &str| {
            let mut m = NowPlayingMeasure::with_hub(hub.clone());
            m.read_options(&OptionReader { parser: &s.parser, section: sec, sv: None });
            m
        };
        let (mut a, mut b) = (mk("Parent"), mk("Child"));
        ctx_run(&s, |c| {
            assert_eq!(a.update(c), 0.0);
            assert_eq!(a.string_value().as_deref(), Some("Song"));
            assert_eq!(b.update(c), 2.0);
        });
        assert_eq!(b.player_name(), "Winamp");
        a.command("Next");
        assert_eq!(hub.sent.lock().unwrap()[0], ("org.mpris.MediaPlayer2.x".to_string(), Call::Next));
        // Track change fires the action once.
        hub.players.lock().unwrap()[0].title = "Other".into();
        ctx_run(&s, |c| {
            b.update(c);
        });
        assert_eq!(b.take_actions(), vec!["!Log hi".to_string()]);
    }
}
