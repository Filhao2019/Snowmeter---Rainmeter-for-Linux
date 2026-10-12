//! SpotifyPlugin replacement: a view of the MPRIS hub that prefers the player
//! whose identity (or bus name) contains "spotify" and falls back to the
//! other players (a browser playing open.spotify.com is one of them).
//!
//! `Type` (or `PlayerType`): Artist, Track/Title, Album, Cover, CoverWebAddress,
//! Length/Duration, Position (seconds as the value, `m:ss` as the string),
//! Progress (0-100), Volume (0-100), Shuffle, Repeat, Playing (1 playing, else
//! 0), Status (1 when a player exists), State, Player. `!CommandMeasure`:
//! PlayPause, Play, Pause, Stop, Next, Previous, SetPosition N, SetVolume N,
//! Shuffle, Repeat.

use super::web::{dispatch_player, parse_wkind, web_value, WKind};
use super::{mpris, select, Hub, Playback, PlayerData};
use crate::measures::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Playing,
    View(WKind),
}

fn parse_kind(s: &str) -> Kind {
    match s.trim().to_ascii_lowercase().as_str() {
        "playing" | "isplaying" => Kind::Playing,
        "track" | "trackname" | "name" | "song" => Kind::View(WKind::Title),
        "length" | "tracklength" => Kind::View(WKind::Duration),
        "albumart" | "cover" | "coverpath" => Kind::View(WKind::Cover),
        "coverurl" | "albumarturl" => Kind::View(WKind::CoverWebAddress),
        other => Kind::View(parse_wkind(other)),
    }
}

/// The Spotify player, else the usual best pick.
pub fn pick_spotify(players: &[PlayerData]) -> Option<&PlayerData> {
    select(players, "spotify")
}

fn spotify_value(kind: Kind, p: Option<&PlayerData>) -> (f64, Option<String>) {
    match kind {
        Kind::Playing => (f64::from(p.is_some_and(|p| p.playback == Playback::Playing)), None),
        Kind::View(k) => web_value(k, p),
    }
}

pub struct SpotifyMeasure {
    hub: Arc<dyn Hub>,
    kind: Kind,
    string: Option<String>,
    log: Vec<String>,
}

impl SpotifyMeasure {
    pub fn new() -> SpotifyMeasure {
        SpotifyMeasure::with_hub(mpris::shared_hub())
    }

    pub fn with_hub(hub: Arc<dyn Hub>) -> SpotifyMeasure {
        SpotifyMeasure { hub, kind: Kind::View(WKind::Unknown), string: None, log: Vec::new() }
    }
}

impl Default for SpotifyMeasure {
    fn default() -> Self {
        SpotifyMeasure::new()
    }
}

impl MeasureImpl for SpotifyMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let mut ty = opt.string("Type", "");
        if ty.trim().is_empty() {
            ty = opt.string("PlayerType", "");
        }
        self.kind = parse_kind(&ty);
        if self.kind == Kind::View(WKind::Unknown) {
            self.log.push(format!("SpotifyPlugin: [{}] Type \"{ty}\" is not valid", opt.section));
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        let players = self.hub.snapshot();
        let (v, s) = spotify_value(self.kind, pick_spotify(&players));
        self.string = s;
        v
    }

    fn string_value(&self) -> Option<String> {
        self.string.clone()
    }

    fn forced_max(&self) -> Option<f64> {
        match self.kind {
            Kind::Playing => Some(1.0),
            Kind::View(WKind::Progress | WKind::Volume) => Some(100.0),
            Kind::View(WKind::State | WKind::Repeat) => Some(2.0),
            _ => None,
        }
    }

    fn forced_min(&self) -> Option<f64> {
        Some(0.0)
    }

    fn command(&mut self, args: &str) {
        let players = self.hub.snapshot();
        match dispatch_player(pick_spotify(&players), args, "SpotifyPlugin") {
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
    use crate::measures::nowplaying::Call;
    use crate::measures::testutil::{ctx_run, skin};
    use std::sync::Mutex;

    struct FakeHub {
        players: Vec<PlayerData>,
        sent: Mutex<Vec<(String, Call)>>,
    }

    impl Hub for FakeHub {
        fn snapshot(&self) -> Vec<PlayerData> {
            self.players.clone()
        }
        fn send(&self, bus: &str, call: Call) {
            self.sent.lock().unwrap().push((bus.to_string(), call));
        }
        fn register(&self, _: &str, _: &str) {}
        fn parent_player(&self, _: &str) -> Option<String> {
            None
        }
    }

    fn player(bus: &str, ident: &str, pb: Playback, title: &str) -> PlayerData {
        PlayerData {
            bus: bus.into(),
            identity: ident.into(),
            playback: pb,
            title: title.into(),
            artist: format!("artist of {title}"),
            length_us: 200_000_000,
            position_us: 50_000_000,
            volume: 0.7,
            ..Default::default()
        }
    }

    fn hub() -> Arc<FakeHub> {
        Arc::new(FakeHub {
            players: vec![
                player("org.mpris.MediaPlayer2.vlc", "VLC media player", Playback::Playing, "Movie"),
                player("org.mpris.MediaPlayer2.spotify", "Spotify", Playback::Paused, "Song"),
            ],
            sent: Mutex::default(),
        })
    }

    fn eval(h: &Arc<FakeHub>, ini: &str) -> (f64, Option<String>) {
        let sk = skin(&format!("[M]\nMeasure=Plugin\nPlugin=SpotifyPlugin\n{ini}\n"));
        let mut m = SpotifyMeasure::with_hub(h.clone());
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        let v = ctx_run(&sk, |c| m.update(c));
        (v, m.string_value())
    }

    #[test]
    fn prefers_spotify_over_the_playing_player() {
        let h = hub();
        assert_eq!(eval(&h, "Type=Track").1.as_deref(), Some("Song"));
        assert_eq!(eval(&h, "Type=Artist").1.as_deref(), Some("artist of Song"));
        assert_eq!(eval(&h, "Type=Playing").0, 0.0);
        assert_eq!(eval(&h, "Type=Length"), (200.0, Some("3:20".into())));
        assert_eq!(eval(&h, "Type=Position").0, 50.0);
        assert_eq!(eval(&h, "Type=Progress").0, 25.0);
        assert_eq!(eval(&h, "Type=Volume").0, 70.0);
        assert_eq!(eval(&h, "Type=Player").1.as_deref(), Some("Spotify"));
    }

    #[test]
    fn falls_back_and_handles_no_player() {
        let only_vlc = Arc::new(FakeHub { players: vec![player("org.mpris.MediaPlayer2.vlc", "VLC", Playback::Playing, "Movie")], sent: Mutex::default() });
        assert_eq!(eval(&only_vlc, "Type=Title").1.as_deref(), Some("Movie"));
        assert_eq!(eval(&only_vlc, "Type=Playing").0, 1.0);
        let none = Arc::new(FakeHub { players: vec![], sent: Mutex::default() });
        assert_eq!(eval(&none, "Type=Track"), (0.0, Some(String::new())));
        assert_eq!(eval(&none, "Type=Playing").0, 0.0);
    }

    #[test]
    fn commands_go_to_spotify() {
        let h = hub();
        let sk = skin("[M]\nMeasure=Plugin\nType=Track\n");
        let mut m = SpotifyMeasure::with_hub(h.clone());
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        m.command("PlayPause");
        m.command("Next");
        m.command("SetVolume 40");
        m.command("Bogus");
        let sent = h.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 3);
        assert!(sent.iter().all(|(b, _)| b == "org.mpris.MediaPlayer2.spotify"));
        assert_eq!(sent[0].1, Call::PlayPause);
        assert_eq!(sent[2].1, Call::SetVolume(0.4));
        assert_eq!(m.take_log(), ["SpotifyPlugin: unknown command 'bogus'"]);
    }

    #[test]
    fn invalid_type_logs_once() {
        let h = hub();
        let sk = skin("[M]\nMeasure=Plugin\nType=Zzz\n");
        let mut m = SpotifyMeasure::with_hub(h);
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        assert_eq!(m.take_log().len(), 1);
        assert!(m.take_log().is_empty());
    }
}
