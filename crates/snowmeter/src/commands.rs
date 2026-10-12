//! Non-GUI subcommands: talk to the running instance over IPC or fall back to
//! editing `snowmeter.ini` / working locally.

use crate::ipc::{self, Request};
use crate::Cmd;
use snowmeter_core::env::Environment;
use snowmeter_core::registry::SkinRegistry;
use snowmeter_core::rmskin::{self, ImportError, ImportOptions};
use snowmeter_core::settings::Settings;
use std::io::{BufRead, IsTerminal, Write};

/// `list` output: one line per config, `*` marks active ones.
pub fn format_list(reg: &SkinRegistry, active: &[(String, String)]) -> String {
    let mut out = String::new();
    for c in reg.configs.iter().filter(|c| !c.files.is_empty()) {
        let on = active.iter().find(|(n, _)| n.eq_ignore_ascii_case(&c.name));
        match on {
            Some((_, f)) => out.push_str(&format!("* {} [{}]\n", c.name, f)),
            None => out.push_str(&format!("  {}\n", c.name)),
        }
    }
    out
}

/// Active `(config, file)` pairs according to the settings file.
pub fn active_from_settings(reg: &SkinRegistry, settings: &Settings) -> Vec<(String, String)> {
    settings
        .active_configs()
        .into_iter()
        .filter_map(|(name, n)| reg.file_at(&name, n).map(|f| (name, f.to_string())))
        .collect()
}

fn print_reply(reply: &str) -> i32 {
    let body = ipc::reply_body(reply);
    if ipc::is_error(reply) {
        eprintln!("snowmeter: {body}");
        1
    } else {
        if !body.is_empty() {
            println!("{}", body.trim_end());
        }
        0
    }
}

const NOT_RUNNING_HINT: &str = "Snowmeter is not running; the change takes effect the next time it starts (run `snowmeter`).";

pub fn run(cmd: Cmd) -> i32 {
    let env = Environment::from_env();
    let sock = ipc::socket_path();
    match cmd {
        Cmd::Start => unreachable!("handled in main"),
        Cmd::Load { config, file } => {
            if let Ok(r) = ipc::send(&sock, &Request::Load { config: config.clone(), file: file.clone() }) {
                return print_reply(&r);
            }
            let mut settings = Settings::load(&env);
            let reg = SkinRegistry::scan(&env);
            let file = match file.or_else(|| reg.config(&config).and_then(|c| c.files.first().cloned())) {
                Some(f) => f,
                None => {
                    eprintln!("snowmeter: config \"{config}\" not found or has no .ini files");
                    return 1;
                }
            };
            match rmskin::activate_skin(&env, &mut settings, &config, &file) {
                Ok((name, f)) => match settings.save() {
                    Ok(()) => {
                        println!("{name} [{f}] marked active. {NOT_RUNNING_HINT}");
                        0
                    }
                    Err(e) => {
                        eprintln!("snowmeter: cannot write settings: {e}");
                        1
                    }
                },
                Err(e) => {
                    eprintln!("snowmeter: {e}");
                    1
                }
            }
        }
        Cmd::Unload { config } => {
            if let Ok(r) = ipc::send(&sock, &Request::Unload { config: config.clone() }) {
                return print_reply(&r);
            }
            let mut settings = Settings::load(&env);
            let name = settings.config_names().into_iter().find(|n| n.eq_ignore_ascii_case(&config.replace('/', "\\")));
            match name {
                Some(n) => {
                    settings.set_active(&n, 0);
                    if let Err(e) = settings.save() {
                        eprintln!("snowmeter: cannot write settings: {e}");
                        return 1;
                    }
                    println!("{n} marked inactive. {NOT_RUNNING_HINT}");
                    0
                }
                None => {
                    eprintln!("snowmeter: \"{config}\" is not a known config");
                    1
                }
            }
        }
        Cmd::List => {
            if let Ok(r) = ipc::send(&sock, &Request::List) {
                return print_reply(&r);
            }
            let reg = SkinRegistry::scan(&env);
            let settings = Settings::load(&env);
            print!("{}", format_list(&reg, &active_from_settings(&reg, &settings)));
            0
        }
        Cmd::Refresh { config } => match ipc::send(&sock, &Request::Refresh { config }) {
            Ok(r) => print_reply(&r),
            Err(_) => {
                eprintln!("snowmeter: not running (start it with `snowmeter`)");
                1
            }
        },
        Cmd::Quit => match ipc::send(&sock, &Request::Quit) {
            Ok(r) => print_reply(&r),
            Err(_) => {
                eprintln!("snowmeter: not running");
                1
            }
        },
        Cmd::Import { path } => import(&env, &sock, &path),
        Cmd::Check { target } => match snowmeter_core::compat::check_path(&env, &target) {
            Ok(reports) => {
                for r in &reports {
                    println!("{r}");
                    for n in &r.notes {
                        println!("    note: {n}");
                    }
                }
                if reports.iter().all(|r| r.fully_supported) {
                    0
                } else {
                    2
                }
            }
            Err(e) => {
                eprintln!("snowmeter: {e}");
                1
            }
        },
        Cmd::Render { config, file, out, ticks } => {
            match snowmeter_core::render_skin_png(&env, &config, file.as_deref(), ticks, &out) {
                Ok(()) => {
                    println!("wrote {}", out.display());
                    0
                }
                Err(e) => {
                    eprintln!("snowmeter: {e}");
                    1
                }
            }
        }
    }
}

fn confirm(question: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn import(env: &Environment, sock: &std::path::Path, path: &std::path::Path) -> i32 {
    let mut opts = ImportOptions::default();
    let report = loop {
        match rmskin::import_path_with(env, path, &opts) {
            Ok(r) => break r,
            Err(ImportError::TooNew { required, message }) if !opts.ignore_min_version => {
                eprintln!("snowmeter: {message} (requires Rainmeter {required})");
                if confirm("Install anyway?") {
                    opts.ignore_min_version = true;
                    continue;
                }
                return 1;
            }
            Err(e) => {
                eprintln!("snowmeter: import failed: {e}");
                return 1;
            }
        }
    };
    print!("{report}");
    if ipc::send(sock, &Request::Rescan).is_ok() {
        println!("The running instance was asked to rescan.");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use snowmeter_core::registry::SkinConfig;

    #[test]
    fn list_format() {
        let reg = SkinRegistry {
            skins_dir: "/x".into(),
            configs: vec![
                SkinConfig { name: "a".into(), root: "a".into(), dir: "/x/a".into(), files: vec![] },
                SkinConfig { name: "a\\One".into(), root: "a".into(), dir: "/x/a/One".into(), files: vec!["One.ini".into()] },
                SkinConfig { name: "a\\Two".into(), root: "a".into(), dir: "/x/a/Two".into(), files: vec!["Two.ini".into()] },
            ],
        };
        let out = format_list(&reg, &[("A\\ONE".into(), "One.ini".into())]);
        assert_eq!(out, "* a\\One [One.ini]\n  a\\Two\n");
    }

    #[test]
    fn offline_settings_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let env = Environment::with_root(tmp.path());
        let dir = env.skins_dir.join("S").join("One");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("One.ini"), "[Rainmeter]\n[M]\nMeter=String\nText=x\n").unwrap();
        let mut settings = Settings::load(&env);
        let (name, file) = rmskin::activate_skin(&env, &mut settings, "S\\One", "One.ini").unwrap();
        settings.save().unwrap();
        let reg = SkinRegistry::scan(&env);
        let active = active_from_settings(&reg, &Settings::load(&env));
        assert_eq!(active, vec![(name, file)]);
    }
}
