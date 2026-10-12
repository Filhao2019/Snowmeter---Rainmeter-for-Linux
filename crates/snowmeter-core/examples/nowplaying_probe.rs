//! `nowplaying_probe` prints the MPRIS snapshot (read-only).
//! `nowplaying_probe --selftest` serves a throwaway player
//! (`org.mpris.MediaPlayer2.snowmetertest`) and drives it through the service.

use snowmeter_core::measures::nowplaying::mpris::{CoverCache, Service, ZbusBackend};
use snowmeter_core::measures::nowplaying::{dispatch, Hub, PlayerData};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

#[derive(Default)]
struct St {
    status: String,
    pos: i64,
    vol: f64,
    shuffle: bool,
    loop_s: String,
    log: Vec<String>,
}
type Shared = Arc<Mutex<St>>;

struct Root(Shared);
#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.0.lock().unwrap().log.push("Raise".into());
    }
    fn quit(&self) {
        self.0.lock().unwrap().log.push("Quit".into());
    }
    #[zbus(property)]
    fn identity(&self) -> String {
        "Snowmeter Test".into()
    }
}

struct Player(Shared);
#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play(&self) {
        let mut s = self.0.lock().unwrap();
        s.status = "Playing".into();
        s.log.push("Play".into());
    }
    fn pause(&self) {
        let mut s = self.0.lock().unwrap();
        s.status = "Paused".into();
        s.log.push("Pause".into());
    }
    fn play_pause(&self) {
        let mut s = self.0.lock().unwrap();
        s.status = if s.status == "Playing" { "Paused".into() } else { "Playing".into() };
        s.log.push("PlayPause".into());
    }
    fn next(&self) {
        self.0.lock().unwrap().log.push("Next".into());
    }
    fn set_position(&self, _id: ObjectPath<'_>, pos: i64) {
        let mut s = self.0.lock().unwrap();
        s.pos = pos;
        s.log.push(format!("SetPosition {pos}"));
    }
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.0.lock().unwrap().status.clone()
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        self.0.lock().unwrap().pos
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.0.lock().unwrap().vol
    }
    #[zbus(property)]
    fn set_volume(&self, v: f64) {
        let mut s = self.0.lock().unwrap();
        s.vol = v;
        s.log.push(format!("Volume {v:.2}"));
    }
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.0.lock().unwrap().shuffle
    }
    #[zbus(property)]
    fn set_shuffle(&self, v: bool) {
        self.0.lock().unwrap().shuffle = v;
    }
    #[zbus(property)]
    fn loop_status(&self) -> String {
        self.0.lock().unwrap().loop_s.clone()
    }
    #[zbus(property)]
    fn set_loop_status(&self, v: String) {
        self.0.lock().unwrap().loop_s = v;
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let v = |x: Value| OwnedValue::try_from(x).unwrap();
        HashMap::from([
            ("mpris:trackid".to_string(), v(Value::from(ObjectPath::try_from("/test/1").unwrap()))),
            ("mpris:length".to_string(), v(Value::from(200_000_000i64))),
            ("xesam:title".to_string(), v(Value::from("Test Title"))),
            ("xesam:artist".to_string(), v(Value::from(vec!["Artist A", "Artist B"]))),
            ("xesam:album".to_string(), v(Value::from("Test Album"))),
            ("xesam:trackNumber".to_string(), v(Value::from(7i32))),
        ])
    }
}

fn show(p: &PlayerData) {
    println!(
        "{} [{}] {:?} | {} - {} ({}) | #{} year={:?} len={}s pos={}s vol={:.2} shuffle={} loop={:?} cover={:?}",
        p.bus, p.identity, p.playback, p.artist, p.title, p.album, p.number, p.year, p.length_us / 1_000_000,
        p.position_us / 1_000_000, p.volume, p.shuffle, p.loop_status, p.cover
    );
}

fn main() {
    let svc = Service::spawn(Box::new(ZbusBackend::new()), CoverCache::new(CoverCache::default_dir()), Duration::from_millis(300));
    if std::env::args().any(|a| a == "--selftest") {
        selftest(svc);
        return;
    }
    std::thread::sleep(Duration::from_millis(1500));
    let snap = svc.snapshot();
    println!("{} player(s)", snap.len());
    snap.iter().for_each(show);
}

fn selftest(svc: Arc<Service>) {
    let st: Shared = Arc::new(Mutex::new(St { status: "Paused".into(), vol: 0.5, loop_s: "None".into(), ..Default::default() }));
    let _conn = zbus::blocking::connection::Builder::session()
        .unwrap()
        .name("org.mpris.MediaPlayer2.snowmetertest")
        .unwrap()
        .serve_at("/org/mpris/MediaPlayer2", Root(st.clone()))
        .unwrap()
        .serve_at("/org/mpris/MediaPlayer2", Player(st.clone()))
        .unwrap()
        .build()
        .expect("session bus");
    let wait = || std::thread::sleep(Duration::from_millis(800));
    wait();
    let mine = |svc: &Service| svc.snapshot().into_iter().find(|p| p.bus.ends_with("snowmetertest")).expect("test player visible");
    show(&mine(&svc));
    for args in ["PlayPause", "Next", "SetPosition 50", "SetVolume 80", "SetVolume -30", "SetShuffle", "SetRepeat", "OpenPlayer"] {
        let snap = svc.snapshot();
        // Target only our own player.
        let (bus, call) = dispatch(&snap, "snowmetertest", args).unwrap().unwrap();
        assert!(bus.ends_with("snowmetertest"));
        svc.send(&bus, call);
        wait();
    }
    show(&mine(&svc));
    let s = st.lock().unwrap();
    println!("player saw: {:?}", s.log);
    assert_eq!(s.log, ["PlayPause", "Next", "SetPosition 100000000", "Volume 0.80", "Volume 0.50", "Raise"]);
    assert!(s.shuffle && s.loop_s == "Playlist" && s.status == "Playing");
    println!("selftest OK");
}
