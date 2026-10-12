//! RunCommand plugin on Linux. The command line (`Program` + `Parameter`) is
//! split with the Windows rules and started directly, without a shell, on a
//! worker thread; its output becomes the measure's string value. Windows
//! shells (`cmd`, `powershell`) cannot run here and are reported instead.

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `CommandLineToArgvW` splitting: spaces separate, `"` groups, `\"` is a quote.
pub fn split_windows_args(s: &str) -> Vec<String> {
    let c: Vec<char> = s.chars().collect();
    let (mut out, mut cur) = (Vec::new(), String::new());
    let (mut in_q, mut any) = (false, false);
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '\\' => {
                let mut n = 0;
                while i < c.len() && c[i] == '\\' {
                    n += 1;
                    i += 1;
                }
                if i < c.len() && c[i] == '"' {
                    cur.extend(std::iter::repeat('\\').take(n / 2));
                    if n % 2 == 1 {
                        cur.push('"');
                        i += 1;
                    }
                } else {
                    cur.extend(std::iter::repeat('\\').take(n));
                }
                any = true;
                continue;
            }
            '"' => {
                in_q = !in_q;
                any = true;
            }
            ch if ch.is_whitespace() && !in_q => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            ch => {
                cur.push(ch);
                any = true;
            }
        }
        i += 1;
    }
    if any {
        out.push(cur);
    }
    out
}

const WINDOWS_SHELLS: &[&str] = &["cmd", "powershell", "pwsh", "command", "wscript", "cscript", "%comspec%"];

/// Executable to start for a Windows-style program string: `C:\x\curl.exe` -> `curl`.
pub fn program_name(p: &str) -> String {
    let base = p.rsplit(['\\', '/']).next().unwrap_or(p);
    let l = base.to_ascii_lowercase();
    if l.ends_with(".exe") || l.ends_with(".com") {
        // Keep the case of the real file name; PATH lookups on Linux are case sensitive.
        base[..base.len() - 4].to_string()
    } else if p.contains('\\') && !p.starts_with('/') {
        base.to_string()
    } else {
        p.to_string()
    }
}

pub fn is_windows_shell(p: &str) -> bool {
    let n = program_name(p).to_ascii_lowercase();
    WINDOWS_SHELLS.contains(&n.as_str())
}

#[derive(Default)]
struct Shared {
    value: f64,
    result: String,
    running: bool,
    pid: Option<u32>,
    finished: bool,
    killed_force: bool,
}

pub struct RunCommand {
    program: String,
    parameter: String,
    folder: String,
    output_file: String,
    timeout_ms: i64,
    finish: String,
    shared: Arc<Mutex<Shared>>,
    skin_dir: PathBuf,
    log: Vec<String>,
}

impl RunCommand {
    pub fn new() -> RunCommand {
        RunCommand {
            program: String::new(),
            parameter: String::new(),
            folder: String::new(),
            output_file: String::new(),
            timeout_ms: -1,
            finish: String::new(),
            shared: Arc::new(Mutex::new(Shared { value: -1.0, ..Default::default() })),
            skin_dir: PathBuf::new(),
            log: Vec::new(),
        }
    }

    fn run(&mut self) {
        let program = if self.program.trim().is_empty() { "cmd.exe /U /C".to_string() } else { self.program.clone() };
        {
            let Ok(mut s) = self.shared.lock() else { return };
            if s.running {
                s.value = 101.0;
                self.log.push(format!("Error 101: Program still running: {program}"));
                return;
            }
            s.value = 0.0;
        }
        let mut words = split_windows_args(&format!("{program} {}", self.parameter));
        if words.is_empty() {
            return;
        }
        if is_windows_shell(&words[0]) {
            self.log.push(format!("Windows shell commands are unsupported on Linux: {program} {}", self.parameter));
            if let Ok(mut s) = self.shared.lock() {
                s.value = 103.0;
            }
            return;
        }
        let exe = program_name(&words.remove(0));
        let folder = if self.folder.trim().is_empty() { None } else { Some(crate::paths::resolve_path(&self.skin_dir, &self.folder)) };
        let out_file = (!self.output_file.trim().is_empty()).then(|| crate::paths::resolve_path(&self.skin_dir, &self.output_file));
        let timeout = (self.timeout_ms >= 0).then(|| Duration::from_millis(self.timeout_ms as u64));
        let has_finish = !self.finish.is_empty();
        let shared = self.shared.clone();
        if let Ok(mut s) = shared.lock() {
            s.running = true;
            s.result.clear();
            s.killed_force = false;
        }
        let spawned = std::thread::Builder::new().name("runcommand".into()).spawn(move || {
            let (value, result) = execute(&exe, &words, folder.as_deref(), timeout, &shared);
            let mut value = value;
            if let Some(f) = &out_file {
                if std::fs::write(f, &result).is_err() {
                    value = 104.0;
                }
            }
            if let Ok(mut s) = shared.lock() {
                s.result = result;
                s.value = value;
                s.running = false;
                s.pid = None;
                s.finished = has_finish;
            }
        });
        if spawned.is_err() {
            if let Ok(mut s) = self.shared.lock() {
                s.running = false;
                s.value = 103.0;
            }
        }
    }

    fn terminate(&mut self, force: bool) {
        let pid = self.shared.lock().ok().and_then(|s| if s.running { s.pid } else { None });
        match pid {
            Some(pid) => {
                if let Ok(mut s) = self.shared.lock() {
                    s.killed_force = force;
                }
                // SAFETY: signalling the process group we created for this command.
                unsafe {
                    libc::kill(-(pid as i32), if force { libc::SIGKILL } else { libc::SIGTERM });
                }
            }
            None => {
                if let Ok(mut s) = self.shared.lock() {
                    s.value = 102.0;
                }
                self.log.push(format!("Error 102: Program not running: {}", self.program));
            }
        }
    }
}

/// Start the program, collect its output, honour the timeout. (value, output)
fn execute(exe: &str, args: &[String], folder: Option<&std::path::Path>, timeout: Option<Duration>, shared: &Arc<Mutex<Shared>>) -> (f64, String) {
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    if let Some(f) = folder {
        if f.is_dir() {
            cmd.current_dir(f);
        }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("RunCommand: cannot start {exe}: {e}");
            return (103.0, String::new());
        }
    };
    if let Ok(mut s) = shared.lock() {
        s.pid = Some(child.id());
    }
    let read = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out_t = read(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err_t = read(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let start = Instant::now();
    let mut value = 1.0;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => break,
        }
        if timeout.is_some_and(|t| start.elapsed() > t) {
            // SAFETY: signalling the process group we created for this command.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut out = out_t.join().unwrap_or_default();
    out.extend(err_t.join().unwrap_or_default());
    let text = String::from_utf8_lossy(&out).replace('\r', "");
    if text.is_empty() && value != 1.0 {
        value = 1.0;
    }
    (value, text)
}

impl Drop for RunCommand {
    fn drop(&mut self) {
        // A skin that goes away takes its running command with it.
        let pid = self.shared.lock().ok().and_then(|s| if s.running { s.pid } else { None });
        if let Some(pid) = pid {
            // SAFETY: signalling the process group we created for this command.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

impl MeasureImpl for RunCommand {
    fn read_options(&mut self, opt: &OptionReader) {
        self.program = opt.string("Program", "");
        self.parameter = opt.string("Parameter", "");
        self.folder = opt.string("StartInFolder", "");
        self.output_file = opt.string("OutputFile", "");
        self.timeout_ms = opt.int("Timeout", -1);
        self.finish = opt.action("FinishAction");
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        self.skin_dir = ctx.skin_dir.to_path_buf();
        self.shared.lock().map(|s| s.value).unwrap_or(-1.0)
    }

    fn string_value(&self) -> Option<String> {
        Some(self.shared.lock().map(|s| s.result.clone()).unwrap_or_default())
    }

    fn command(&mut self, args: &str) {
        match args.trim().to_ascii_uppercase().as_str() {
            "RUN" => self.run(),
            "CLOSE" => self.terminate(false),
            "KILL" => self.terminate(true),
            other => {
                if let Ok(mut s) = self.shared.lock() {
                    s.value = 100.0;
                }
                self.log.push(format!("Error 100: Unknown command: {other}"));
            }
        }
    }

    fn take_actions(&mut self) -> Vec<String> {
        let done = self.shared.lock().map(|mut s| std::mem::take(&mut s.finished)).unwrap_or(false);
        if done && !self.finish.is_empty() {
            vec![self.finish.clone()]
        } else {
            Vec::new()
        }
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    #[test]
    fn windows_argument_splitting() {
        assert_eq!(split_windows_args(r#"a "b c" d"#), ["a", "b c", "d"]);
        assert_eq!(split_windows_args(r#"x "he said \"hi\"" y\z"#), ["x", r#"he said "hi""#, r"y\z"]);
        assert_eq!(split_windows_args(r#"C:\dir\ "e f\\" g"#), [r"C:\dir\", r"e f\", "g"]);
        assert_eq!(split_windows_args(r#"a "" b"#), ["a", "", "b"]);
        assert!(split_windows_args("   ").is_empty());
    }

    #[test]
    fn program_names() {
        assert_eq!(program_name(r"C:\Windows\System32\curl.exe"), "curl");
        assert_eq!(program_name("Python.EXE"), "Python");
        assert_eq!(program_name("/usr/bin/env"), "/usr/bin/env");
        assert!(is_windows_shell("\"%COMSPEC%\"") || is_windows_shell("cmd.exe"));
        assert!(is_windows_shell(r"C:\Windows\System32\cmd.exe") && is_windows_shell("PowerShell.exe"));
        assert!(!is_windows_shell("curl.exe"));
    }

    fn wait_done(m: &mut RunCommand, sk: &crate::measures::testutil::TestSkin) -> f64 {
        for _ in 0..400 {
            let v = ctx_run(sk, |c| m.update(c));
            if v != 0.0 && !m.shared.lock().unwrap().running {
                return v;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("command did not finish");
    }

    #[test]
    fn runs_a_program_and_captures_output() {
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=RunCommand\nProgram=echo\nParameter=\"hello there\" 2\nFinishAction=[!Log done]\n");
        let mut m = RunCommand::new();
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        assert_eq!(ctx_run(&sk, |c| m.update(c)), -1.0);
        m.command("Run");
        assert_eq!(wait_done(&mut m, &sk), 1.0);
        assert_eq!(m.string_value().as_deref(), Some("hello there 2\n"));
        assert_eq!(m.take_actions(), ["[!Log done]"]);
        assert!(m.take_actions().is_empty());
    }

    #[test]
    fn windows_shells_and_errors_are_reported() {
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=RunCommand\nParameter=dir\n[N]\nMeasure=Plugin\nPlugin=RunCommand\nProgram=no-such-program-xyz\n");
        let mut m = RunCommand::new();
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        m.command("Run");
        assert_eq!(ctx_run(&sk, |c| m.update(c)), 103.0);
        assert!(m.take_log()[0].contains("Windows shell commands are unsupported"));
        let mut n = RunCommand::new();
        n.read_options(&OptionReader { parser: &sk.parser, section: "N", sv: None });
        n.command("Run");
        assert_eq!(wait_done(&mut n, &sk), 103.0);
        n.command("Bogus");
        n.command("Close");
        let log = n.take_log();
        assert!(log.iter().any(|l| l.contains("Unknown command")) && log.iter().any(|l| l.contains("not running")), "{log:?}");
    }

    #[test]
    fn timeout_and_kill() {
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=RunCommand\nProgram=sleep\nParameter=30\nTimeout=100\n[K]\nMeasure=Plugin\nPlugin=RunCommand\nProgram=sleep\nParameter=30\n");
        let mut m = RunCommand::new();
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        m.command("Run");
        let t = Instant::now();
        assert_eq!(wait_done(&mut m, &sk), 1.0);
        assert!(t.elapsed() < Duration::from_secs(10));
        let mut k = RunCommand::new();
        k.read_options(&OptionReader { parser: &sk.parser, section: "K", sv: None });
        k.command("Run");
        std::thread::sleep(Duration::from_millis(100));
        k.command("Run");
        assert_eq!(k.shared.lock().unwrap().value, 101.0);
        k.command("Kill");
        assert_eq!(wait_done(&mut k, &sk), 1.0);
    }
}
