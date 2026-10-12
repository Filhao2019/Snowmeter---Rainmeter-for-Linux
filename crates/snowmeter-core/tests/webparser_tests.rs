//! WebParser tests against a local TCP server (no internet).

use snowmeter_core::env::Environment;
use snowmeter_core::skin::Skin;
use std::io::{Read, Write};
use std::net::TcpListener;

/// Serve `body` to every connection; returns the port.
fn serve(body: &'static str) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            let _ = s.write_all(resp.as_bytes());
        }
    });
    port
}

struct Fx {
    _tmp: tempfile::TempDir,
    env: Environment,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let env = Environment::with_root(tmp.path());
        std::fs::create_dir_all(&env.skins_dir).unwrap();
        Fx { _tmp: tmp, env }
    }

    fn skin(&self, ini: &str) -> Skin {
        let dir = self.env.skins_dir.join("W");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Main.ini"), ini).unwrap();
        Skin::load(&self.env, "W", "Main.ini").unwrap()
    }
}

/// Tick until `done` holds (the fetch runs on a worker thread).
fn wait(s: &mut Skin, done: impl Fn(&Skin) -> bool) {
    for _ in 0..200 {
        s.tick();
        if done(s) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("timed out: {:?}", s.log_messages());
}

#[test]
fn regexp_string_index_child_and_finish_action() {
    let port = serve("<html><b>Temp</b> <span>23.5</span> <i>sunny-7</i></html>");
    let fx = Fx::new();
    let ini = format!(
        "[Variables]\nDone=no\n\
         [Parent]\nMeasure=WebParser\nUrl=http://127.0.0.1:{port}/\nRegExp=(?siU)<span>(.*)</span>.*<i>(.*)</i>\nStringIndex=1\nFinishAction=[!SetVariable Done yes]\n\
         [Child]\nMeasure=WebParser\nUrl=[Parent]\nStringIndex=2\n\
         [Grand]\nMeasure=WebParser\nUrl=[Parent]\nStringIndex=2\nRegExp=(\\w+)-(\\d+)\nStringIndex2=2\n\
         [Sub]\nMeasure=WebParser\nUrl=[Parent]\nStringIndex=9\nLogSubstringErrors=1\n\
         [T]\nMeter=String\nMeasureName=Child\nText=%1\n"
    );
    let mut s = fx.skin(&ini);
    wait(&mut s, |s| s.parser().get_variable("Done").as_deref() == Some("yes"));
    s.tick();
    assert_eq!(s.measure_value("Parent"), Some(23.5));
    // The child's Url is "[Parent]" with the group substituted.
    wait(&mut s, |s| s.meter_text("T").as_deref() == Some("sunny-7"));
    assert_eq!(s.measure_value("Grand"), Some(7.0));
    assert!(s.log_messages().iter().any(|m| m.contains("Not enough substrings")), "{:?}", s.log_messages());
}

#[test]
fn no_match_uses_error_string_and_action() {
    let port = serve("nothing here");
    let fx = Fx::new();
    let ini = format!(
        "[Variables]\nErr=0\n[P]\nMeasure=WebParser\nUrl=http://127.0.0.1:{port}/\nRegExp=(\\d+)\nErrorString=oops\nOnRegExpErrorAction=[!SetVariable Err 1]\n\
         [T]\nMeter=String\nMeasureName=P\nText=%1\n"
    );
    let mut s = fx.skin(&ini);
    wait(&mut s, |s| s.meter_text("T").as_deref() == Some("oops"));
    assert_eq!(s.parser().get_variable("Err").as_deref(), Some("1"));
}

#[test]
fn connection_failure_runs_connect_error_action() {
    // A port nothing listens on.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let fx = Fx::new();
    let ini = format!(
        "[Variables]\nC=0\n[P]\nMeasure=WebParser\nUrl=http://127.0.0.1:{port}/\nRegExp=x\nOnConnectErrorAction=[!SetVariable C 1]\n[T]\nMeter=String\nText=x\n"
    );
    let mut s = fx.skin(&ini);
    wait(&mut s, |s| s.parser().get_variable("C").as_deref() == Some("1"));
}

#[test]
fn file_urls_and_download() {
    let fx = Fx::new();
    let data = fx._tmp.path().join("page.txt");
    std::fs::write(&data, "key=value42").unwrap();
    let ini = format!(
        "[P]\nMeasure=WebParser\nUrl=file://{}\nRegExp=key=(\\w+)\nStringIndex=1\n\
         [D]\nMeasure=WebParser\nUrl=file://{}\nDownload=1\nDownloadFile=copy.txt\n[T]\nMeter=String\nMeasureName=P\nText=%1\n",
        data.display(),
        data.display()
    );
    let mut s = fx.skin(&ini);
    wait(&mut s, |s| s.meter_text("T").as_deref() == Some("value42"));
    let copy = fx.env.skins_dir.join("W/DownloadFile/copy.txt");
    wait(&mut s, |_| copy.exists());
    assert_eq!(std::fs::read_to_string(copy).unwrap(), "key=value42");
}
