//! Single-instance control socket. One text line per request (fields separated
//! by TAB, so config names may contain spaces), one reply, then the connection
//! closes. Replies start with `ok` or `err`; further lines are payload.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Ping,
    Load { config: String, file: Option<String> },
    Unload { config: String },
    List,
    Refresh { config: Option<String> },
    /// Re-read the skins folder and `snowmeter.ini` (after an import).
    Rescan,
    Quit,
}

impl Request {
    /// The wire form (no trailing newline).
    pub fn to_line(&self) -> String {
        match self {
            Request::Ping => "ping".into(),
            Request::Load { config, file: None } => format!("load\t{config}"),
            Request::Load { config, file: Some(f) } => format!("load\t{config}\t{f}"),
            Request::Unload { config } => format!("unload\t{config}"),
            Request::List => "list".into(),
            Request::Refresh { config: None } => "refresh".into(),
            Request::Refresh { config: Some(c) } => format!("refresh\t{c}"),
            Request::Rescan => "rescan".into(),
            Request::Quit => "quit".into(),
        }
    }

    pub fn parse(line: &str) -> Result<Request, String> {
        let line = line.trim_end_matches(['\r', '\n']);
        let mut it = line.split('\t');
        let verb = it.next().unwrap_or("").trim().to_ascii_lowercase();
        let a = it.next().map(str::to_string).filter(|s| !s.is_empty());
        let b = it.next().map(str::to_string).filter(|s| !s.is_empty());
        match verb.as_str() {
            "ping" => Ok(Request::Ping),
            "load" => Ok(Request::Load { config: a.ok_or("load needs a config name")?, file: b }),
            "unload" => Ok(Request::Unload { config: a.ok_or("unload needs a config name")? }),
            "list" => Ok(Request::List),
            "refresh" => Ok(Request::Refresh { config: a }),
            "rescan" => Ok(Request::Rescan),
            "quit" => Ok(Request::Quit),
            "" => Err("empty request".into()),
            other => Err(format!("unknown command '{other}'")),
        }
    }
}

/// `ok[ message]` / `err message`.
pub fn ok(msg: &str) -> String {
    if msg.is_empty() {
        "ok".into()
    } else {
        format!("ok {msg}")
    }
}

pub fn err(msg: &str) -> String {
    format!("err {msg}")
}

pub fn is_error(reply: &str) -> bool {
    reply.starts_with("err")
}

/// Strip the `ok`/`err` marker for display.
pub fn reply_body(reply: &str) -> &str {
    let r = reply.trim_end();
    for p in ["ok ", "err "] {
        if let Some(rest) = r.strip_prefix(p) {
            return rest;
        }
    }
    if r == "ok" || r == "err" {
        ""
    } else {
        r
    }
}

pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SNOWMETER_SOCKET").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    socket_path_for(std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()).map(PathBuf::from), unsafe { libc::getuid() })
}

pub fn socket_path_for(runtime_dir: Option<PathBuf>, uid: u32) -> PathBuf {
    match runtime_dir {
        Some(d) => d.join("snowmeter.sock"),
        None => PathBuf::from(format!("/tmp/snowmeter-{uid}.sock")),
    }
}

/// Send one request to the running instance; `Err` if nothing answers.
pub fn send(path: &Path, req: &Request) -> std::io::Result<String> {
    let mut s = UnixStream::connect(path)?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    s.set_write_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(req.to_line().as_bytes())?;
    s.write_all(b"\n")?;
    let mut reply = String::new();
    s.read_to_string(&mut reply)?;
    if reply.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "empty reply"));
    }
    Ok(reply)
}

/// True if an instance answers on `path`.
pub fn is_running(path: &Path) -> bool {
    path.exists() && send(path, &Request::Ping).is_ok()
}

pub enum Bind {
    Listener(UnixListener),
    AlreadyRunning,
}

/// Bind the socket. If another instance answers, report it; a stale socket
/// file is removed first.
pub fn bind(path: &Path) -> std::io::Result<Bind> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    // Never talk to (or replace) a socket that belongs to another user.
    if let Ok(md) = std::fs::symlink_metadata(path) {
        if md.uid() != unsafe { libc::geteuid() } {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "socket path is owned by another user"));
        }
    }
    if is_running(path) {
        return Ok(Bind::AlreadyRunning);
    }
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // `chmod 0600` right after bind (umask is process-wide, so not used); the
    // peer-UID check in `serve` also rejects other users during that instant.
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(Bind::Listener(listener))
}

/// UID of the process on the other end of `s` (`SO_PEERCRED`).
fn peer_uid(s: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred`/`len` are valid for the call; the fd is open.
    let rc = unsafe {
        libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len)
    };
    (rc == 0).then_some(cred.uid)
}

/// Accept connections on a background thread; `handler` turns a request into
/// the reply text (it may block while the GTK thread works).
pub fn serve<F>(listener: UnixListener, handler: F) -> std::thread::JoinHandle<()>
where
    F: Fn(Request) -> String + Send + 'static,
{
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            if peer_uid(&s) != Some(unsafe { libc::geteuid() }) {
                continue;
            }
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            let reply = match BufReader::new(&s).take(64 * 1024).read_line(&mut line) {
                Ok(0) | Err(_) => continue,
                Ok(_) => match Request::parse(&line) {
                    Ok(r) => handler(r),
                    Err(e) => err(&e),
                },
            };
            let _ = s.write_all(reply.as_bytes());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_round_trip() {
        let all = [
            Request::Ping,
            Request::Load { config: "illustro\\Recycle Bin".into(), file: None },
            Request::Load { config: "a\\b".into(), file: Some("x y.ini".into()) },
            Request::Unload { config: "a".into() },
            Request::List,
            Request::Refresh { config: None },
            Request::Refresh { config: Some("a\\b".into()) },
            Request::Rescan,
            Request::Quit,
        ];
        for r in all {
            assert_eq!(Request::parse(&r.to_line()).unwrap(), r);
        }
    }

    #[test]
    fn parse_errors() {
        assert!(Request::parse("").is_err());
        assert!(Request::parse("load").is_err());
        assert!(Request::parse("bogus").unwrap_err().contains("bogus"));
        assert_eq!(Request::parse("LIST\r\n").unwrap(), Request::List);
    }

    #[test]
    fn reply_helpers() {
        assert_eq!(ok(""), "ok");
        assert!(is_error(&err("x")));
        assert_eq!(reply_body("ok hello\n"), "hello");
        assert_eq!(reply_body("err bad"), "bad");
        assert_eq!(reply_body("ok"), "");
    }

    #[test]
    fn socket_path_fallback() {
        assert_eq!(socket_path_for(Some("/run/user/1000".into()), 1000), PathBuf::from("/run/user/1000/snowmeter.sock"));
        assert_eq!(socket_path_for(None, 42), PathBuf::from("/tmp/snowmeter-42.sock"));
    }

    #[test]
    fn server_client_and_single_instance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        // A stale file is replaced.
        std::fs::write(&path, b"").unwrap();
        let Bind::Listener(l) = bind(&path).unwrap() else { panic!("expected listener") };
        serve(l, |r| match r {
            Request::Ping => ok("pong"),
            Request::List => "ok\nA\nB\n".into(),
            _ => err("nope"),
        });
        assert_eq!(send(&path, &Request::Ping).unwrap(), "ok pong");
        assert_eq!(send(&path, &Request::List).unwrap(), "ok\nA\nB\n");
        assert!(is_error(&send(&path, &Request::Quit).unwrap()));
        assert!(matches!(bind(&path).unwrap(), Bind::AlreadyRunning));
    }

    #[test]
    fn socket_is_private_and_peer_is_checked() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.sock");
        let Bind::Listener(_l) = bind(&path).unwrap() else { panic!() };
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let c = UnixStream::connect(&path).unwrap();
        assert_eq!(peer_uid(&c), Some(unsafe { libc::geteuid() }));
    }

    #[test]
    fn nothing_listening() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_running(&dir.path().join("none.sock")));
        assert!(send(&dir.path().join("none.sock"), &Request::Ping).is_err());
    }
}
