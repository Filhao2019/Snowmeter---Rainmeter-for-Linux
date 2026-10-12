//! MPRIS access over the D-Bus session bus (zbus blocking API), the shared
//! polling thread and the cover cache.

use super::{Backend, Call, Hub, LoopStatus, Playback, PlayerData};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const IFACE_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const IFACE_ROOT: &str = "org.mpris.MediaPlayer2";
const IFACE_PROPS: &str = "org.freedesktop.DBus.Properties";

// ---------------------------------------------------------------- zbus backend

/// Real backend. Connects lazily; a missing session bus is logged once.
#[derive(Default)]
pub struct ZbusBackend {
    conn: Option<Connection>,
    warned: bool,
}

impl ZbusBackend {
    pub fn new() -> ZbusBackend {
        ZbusBackend::default()
    }

    fn conn(&mut self) -> Result<&Connection, String> {
        if self.conn.is_none() {
            match Connection::session() {
                Ok(c) => self.conn = Some(c),
                Err(e) => {
                    if !self.warned {
                        self.warned = true;
                        log::warn!("NowPlaying: no D-Bus session bus ({e}); values stay empty");
                    }
                    return Err(e.to_string());
                }
            }
        }
        self.conn.as_ref().ok_or_else(|| "no connection".to_string())
    }
}

fn get_all(conn: &Connection, bus: &str, iface: &str) -> Result<HashMap<String, OwnedValue>, String> {
    let msg = conn.call_method(Some(bus), PATH, Some(IFACE_PROPS), "GetAll", &iface).map_err(|e| e.to_string())?;
    msg.body().deserialize::<HashMap<String, OwnedValue>>().map_err(|e| e.to_string())
}

fn unwrap_value<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
    match v {
        Value::Value(inner) => unwrap_value(inner),
        other => other,
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    Some(match unwrap_value(v) {
        Value::U8(x) => *x as f64,
        Value::I16(x) => *x as f64,
        Value::U16(x) => *x as f64,
        Value::I32(x) => *x as f64,
        Value::U32(x) => *x as f64,
        Value::I64(x) => *x as f64,
        Value::U64(x) => *x as f64,
        Value::F64(x) => *x,
        Value::Bool(b) => f64::from(*b),
        _ => return None,
    })
}

fn as_string(v: &Value) -> Option<String> {
    match unwrap_value(v) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

/// String, or list of strings joined with ", ".
fn as_text(v: &Value) -> String {
    match unwrap_value(v) {
        Value::Array(a) => a.iter().filter_map(as_string).collect::<Vec<_>>().join(", "),
        other => as_string(other).unwrap_or_default(),
    }
}

/// Build a `PlayerData` from raw property maps (pure, testable).
pub fn player_from_props(
    bus: &str,
    root: &HashMap<String, OwnedValue>,
    props: &HashMap<String, OwnedValue>,
) -> PlayerData {
    let mut p = PlayerData { bus: bus.to_string(), ..Default::default() };
    p.identity = root.get("Identity").and_then(|v| as_string(v)).unwrap_or_default();
    p.playback = match props.get("PlaybackStatus").and_then(|v| as_string(v)).as_deref() {
        Some("Playing") => Playback::Playing,
        Some("Paused") => Playback::Paused,
        _ => Playback::Stopped,
    };
    p.loop_status = match props.get("LoopStatus").and_then(|v| as_string(v)).as_deref() {
        Some("Track") => LoopStatus::Track,
        Some("Playlist") => LoopStatus::Playlist,
        _ => LoopStatus::None,
    };
    p.shuffle = props.get("Shuffle").and_then(|v| as_f64(v)).unwrap_or(0.0) != 0.0;
    p.volume = props.get("Volume").and_then(|v| as_f64(v)).unwrap_or(0.0).max(0.0);
    p.position_us = props.get("Position").and_then(|v| as_f64(v)).unwrap_or(0.0) as i64;
    if let Some(meta) = props.get("Metadata").and_then(|v| HashMap::<String, OwnedValue>::try_from(v.try_clone().ok()?).ok()) {
        let text = |k: &str| meta.get(k).map(|v| as_text(v)).unwrap_or_default();
        p.title = text("xesam:title");
        p.artist = text("xesam:artist");
        if p.artist.is_empty() {
            p.artist = text("xesam:albumArtist");
        }
        p.album = text("xesam:album");
        p.genre = text("xesam:genre");
        p.file = text("xesam:url");
        p.cover = text("mpris:artUrl");
        p.track_id = text("mpris:trackid");
        p.number = meta.get("xesam:trackNumber").and_then(|v| as_f64(v)).unwrap_or(0.0).max(0.0) as u32;
        p.year = text("xesam:contentCreated").chars().take(4).collect();
        p.length_us = meta.get("mpris:length").and_then(|v| as_f64(v)).unwrap_or(0.0) as i64;
        p.rating = meta.get("xesam:userRating").and_then(|v| as_f64(v)).unwrap_or(0.0);
    }
    p
}

impl Backend for ZbusBackend {
    fn poll(&mut self) -> Result<Vec<PlayerData>, String> {
        let conn = self.conn()?.clone();
        let msg = conn
            .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "ListNames", &())
            .map_err(|e| e.to_string())?;
        let mut names: Vec<String> = msg.body().deserialize::<Vec<String>>().map_err(|e| e.to_string())?;
        names.retain(|n| n.starts_with(PREFIX));
        names.sort();
        let mut out = Vec::new();
        for n in names {
            let Ok(props) = get_all(&conn, &n, IFACE_PLAYER) else { continue };
            let root = get_all(&conn, &n, IFACE_ROOT).unwrap_or_default();
            out.push(player_from_props(&n, &root, &props));
        }
        Ok(out)
    }

    fn call(&mut self, bus: &str, call: &Call) -> Result<(), String> {
        let conn = self.conn()?.clone();
        let e = |r: zbus::Result<zbus::Message>| r.map(|_| ()).map_err(|e| e.to_string());
        let set = |name: &str, v: Value| {
            e(conn.call_method(Some(bus), PATH, Some(IFACE_PROPS), "Set", &(IFACE_PLAYER, name, v)))
        };
        let player = |m: &str| e(conn.call_method(Some(bus), PATH, Some(IFACE_PLAYER), m, &()));
        match call {
            Call::Play => player("Play"),
            Call::Pause => player("Pause"),
            Call::PlayPause => player("PlayPause"),
            Call::Stop => player("Stop"),
            Call::Next => player("Next"),
            Call::Previous => player("Previous"),
            Call::SetPosition { track_id, micros } => match ObjectPath::try_from(track_id.as_str()) {
                Ok(tid) => e(conn.call_method(Some(bus), PATH, Some(IFACE_PLAYER), "SetPosition", &(tid, *micros))),
                Err(_) => Err("player gave no valid track id for SetPosition".into()),
            },
            Call::SetVolume(v) => set("Volume", Value::from(*v)),
            Call::SetShuffle(b) => set("Shuffle", Value::from(*b)),
            Call::SetLoop(l) => set(
                "LoopStatus",
                Value::from(match l {
                    LoopStatus::None => "None",
                    LoopStatus::Track => "Track",
                    LoopStatus::Playlist => "Playlist",
                }),
            ),
            Call::Raise => e(conn.call_method(Some(bus), PATH, Some(IFACE_ROOT), "Raise", &())),
            Call::Quit => e(conn.call_method(Some(bus), PATH, Some(IFACE_ROOT), "Quit", &())),
        }
    }
}

// ----------------------------------------------------------------- cover cache

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolves `mpris:artUrl` values to local paths, downloading http(s) once.
pub struct CoverCache {
    dir: PathBuf,
    done: Arc<Mutex<HashMap<String, String>>>,
    busy: Arc<Mutex<HashSet<String>>>,
}

impl CoverCache {
    pub fn new(dir: PathBuf) -> CoverCache {
        CoverCache { dir, done: Default::default(), busy: Default::default() }
    }

    pub fn default_dir() -> PathBuf {
        let base = std::env::var("XDG_CACHE_HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| format!("{}/.cache", std::env::var("HOME").unwrap_or_default()));
        PathBuf::from(base).join("snowmeter/covers")
    }

    /// Local path for `art_url`, or "" if none (yet).
    pub fn resolve(&self, art_url: &str) -> String {
        let url = art_url.trim();
        if let Some(rest) = url.strip_prefix("file://") {
            // file:///a/b or file://localhost/a/b
            let rest = rest.strip_prefix("localhost").unwrap_or(rest);
            return percent_decode(rest);
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return String::new();
        }
        if let Some(p) = self.done.lock().ok().and_then(|d| d.get(url).cloned()) {
            return p;
        }
        let target = self.dir.join(cache_name(url));
        if target.is_file() {
            let p = target.to_string_lossy().into_owned();
            if let Ok(mut d) = self.done.lock() {
                d.insert(url.to_string(), p.clone());
            }
            return p;
        }
        if self.busy.lock().map(|mut b| b.insert(url.to_string())).unwrap_or(false) {
            let (url, done) = (url.to_string(), self.done.clone());
            std::thread::spawn(move || {
                match download(&url, &target) {
                    Ok(()) => {
                        if let Ok(mut d) = done.lock() {
                            d.insert(url.clone(), target.to_string_lossy().into_owned());
                        }
                    }
                    Err(e) => log::warn!("NowPlaying: cover download failed ({url}): {e}"),
                }
                // Keep failed URLs busy so they are not retried every second.
            });
        }
        String::new()
    }
}

fn cache_name(url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = path.rsplit('.').next().map(|e| e.to_ascii_lowercase()).filter(|e| ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&e.as_str()));
    format!("{:016x}.{}", h.finish(), ext.as_deref().unwrap_or("jpg"))
}

fn download(url: &str, target: &Path) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .timeout_connect(Some(Duration::from_secs(8)))
        .http_status_as_error(true)
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let bytes = resp.body_mut().with_config().limit(16 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = target.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, target).map_err(|e| e.to_string())
}

// --------------------------------------------------------------------- service

#[derive(Default)]
struct Shared {
    players: Vec<(PlayerData, Instant)>,
    names: HashMap<String, String>,
}

/// Background poller implementing [`Hub`].
pub struct Service {
    shared: Arc<Mutex<Shared>>,
    tx: Mutex<Sender<(String, Call)>>,
}

impl Service {
    /// Start the polling thread (period `period`) over `backend`.
    pub fn spawn(mut backend: Box<dyn Backend>, covers: CoverCache, period: Duration) -> Arc<Service> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = channel::<(String, Call)>();
        let sh = shared.clone();
        let spawned = std::thread::Builder::new().name("nowplaying".into()).spawn(move || loop {
            match backend.poll() {
                Ok(mut list) => {
                    for p in &mut list {
                        p.cover = covers.resolve(&p.cover);
                    }
                    let now = Instant::now();
                    if let Ok(mut s) = sh.lock() {
                        s.players = list.into_iter().map(|p| (p, now)).collect();
                    }
                }
                Err(_) => {
                    if let Ok(mut s) = sh.lock() {
                        s.players.clear();
                    }
                }
            }
            match rx.recv_timeout(period) {
                Ok((bus, call)) => {
                    if let Err(e) = backend.call(&bus, &call) {
                        log::warn!("NowPlaying: {call:?} on {bus} failed: {e}");
                    }
                    // Let the player settle, then refresh at once.
                    std::thread::sleep(Duration::from_millis(120));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        });
        if let Err(e) = spawned {
            log::warn!("NowPlaying: cannot start thread: {e}");
        }
        Arc::new(Service { shared, tx: Mutex::new(tx) })
    }
}

impl Hub for Service {
    fn snapshot(&self) -> Vec<PlayerData> {
        let Ok(s) = self.shared.lock() else { return Vec::new() };
        s.players
            .iter()
            .map(|(p, at)| {
                let mut p = p.clone();
                if p.playback == Playback::Playing {
                    let mut pos = p.position_us + at.elapsed().as_micros() as i64;
                    if p.length_us > 0 {
                        pos = pos.min(p.length_us);
                    }
                    p.position_us = pos;
                }
                p
            })
            .collect()
    }

    fn send(&self, bus: &str, call: Call) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send((bus.to_string(), call));
        }
    }

    fn register(&self, measure: &str, player_name: &str) {
        if let Ok(mut s) = self.shared.lock() {
            s.names.insert(measure.to_string(), player_name.to_string());
        }
    }

    fn parent_player(&self, measure: &str) -> Option<String> {
        self.shared.lock().ok()?.names.get(measure).cloned()
    }
}

/// The process-wide service over the real session bus (started on first use).
pub fn shared_hub() -> Arc<dyn Hub> {
    static HUB: OnceLock<Arc<Service>> = OnceLock::new();
    HUB.get_or_init(|| Service::spawn(Box::new(ZbusBackend::new()), CoverCache::new(CoverCache::default_dir()), Duration::from_secs(1)))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        calls: Arc<Mutex<Vec<(String, Call)>>>,
    }
    impl Backend for Fake {
        fn poll(&mut self) -> Result<Vec<PlayerData>, String> {
            Ok(vec![PlayerData { bus: "b".into(), cover: "file:///tmp/a%20b.png".into(), playback: Playback::Paused, ..Default::default() }])
        }
        fn call(&mut self, bus: &str, call: &Call) -> Result<(), String> {
            self.calls.lock().unwrap().push((bus.into(), call.clone()));
            Ok(())
        }
    }

    #[test]
    fn service_publishes_and_forwards_commands() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let dir = tempfile::tempdir().unwrap();
        let svc = Service::spawn(Box::new(Fake { calls: calls.clone() }), CoverCache::new(dir.path().into()), Duration::from_millis(20));
        let t0 = Instant::now();
        while svc.snapshot().is_empty() && t0.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let snap = svc.snapshot();
        assert_eq!(snap[0].cover, "/tmp/a b.png");
        svc.send("b", Call::Next);
        let t0 = Instant::now();
        while calls.lock().unwrap().is_empty() && t0.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(calls.lock().unwrap()[0], ("b".to_string(), Call::Next));
    }

    #[test]
    fn cover_urls() {
        let c = CoverCache::new(PathBuf::from("/nonexistent-cache"));
        assert_eq!(c.resolve("file:///x/y%C3%A9.jpg"), "/x/yé.jpg");
        assert_eq!(c.resolve(""), "");
        assert_eq!(c.resolve("data:image/png;base64,AA"), "");
        assert!(cache_name("http://h/a.png?x=1").ends_with(".png"));
        assert!(cache_name("http://h/a").ends_with(".jpg"));
    }
}
