//! Run an external helper (`ping`, `nmcli`, `wpctl`, `ip` ...) without a
//! shell, with a hard timeout. Meant for worker threads only.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Standard output of `prog args...` if it exits successfully within `timeout`.
/// The locale is forced to `C` so the output can be parsed.
pub fn run_capture(prog: &str, args: &[&str], timeout: Duration) -> Option<String> {
    run_status(prog, args, timeout).and_then(|(ok, out)| ok.then_some(out))
}

/// `(exit_success, stdout)`; `None` when the program cannot start or times out.
pub fn run_status(prog: &str, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut child = Command::new(prog)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.by_ref().take(4 * 1024 * 1024).read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let out = reader.join().unwrap_or_default();
    let status = status?;
    Some((status.success(), String::from_utf8_lossy(&out).into_owned()))
}

/// True when `prog` can be started (found in PATH); used to pick backends.
pub fn have(prog: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_and_times_out() {
        assert_eq!(run_capture("echo", &["hi"], Duration::from_secs(5)).as_deref(), Some("hi\n"));
        assert_eq!(run_status("false", &[], Duration::from_secs(5)), Some((false, String::new())));
        assert!(run_capture("sleep", &["5"], Duration::from_millis(100)).is_none());
        assert!(run_capture("definitely-not-a-program-xyz", &[], Duration::from_secs(1)).is_none());
        assert!(have("sh") && !have("definitely-not-a-program-xyz"));
    }
}
