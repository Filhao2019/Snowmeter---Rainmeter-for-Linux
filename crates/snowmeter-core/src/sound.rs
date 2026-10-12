//! Rainmeter's `Play`, `PlayLoop` and `PlayStop` commands: one sound at a
//! time, played by `pw-play`, `paplay` or `aplay` (the first one installed).

use crate::measures::extcmd::have;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

static CURRENT: Mutex<Option<Child>> = Mutex::new(None);
/// Bumped by every play/stop; a loop thread ends when it no longer matches.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn player() -> Option<&'static str> {
    ["pw-play", "paplay", "aplay"].into_iter().find(|p| have(p))
}

/// `PlayStop`: end the current sound (and any loop).
pub fn stop() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(mut c) = CURRENT.lock().ok().and_then(|mut g| g.take()) {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// `Play` / `PlayLoop`: replace the current sound with `path`.
pub fn play(path: &Path, looped: bool) {
    stop();
    let Some(prog) = player() else {
        log::warn!("Play: no sound player found (install pipewire, pulseaudio-utils or alsa-utils)");
        return;
    };
    let gen = GENERATION.load(Ordering::SeqCst);
    let path: PathBuf = path.to_path_buf();
    let _ = std::thread::Builder::new().name("snowmeter-sound".into()).spawn(move || loop {
        let Ok(child) = Command::new(prog).arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
            log::warn!("Play: cannot start {prog}");
            return;
        };
        {
            let Ok(mut cur) = CURRENT.lock() else { return };
            if GENERATION.load(Ordering::SeqCst) != gen {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            *cur = Some(child);
        }
        // Wait for the sound to finish (or for a stop to take the child away).
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let Ok(mut cur) = CURRENT.lock() else { return };
            if GENERATION.load(Ordering::SeqCst) != gen {
                return;
            }
            match cur.as_mut().map(|c| c.try_wait()) {
                Some(Ok(None)) => {}
                _ => {
                    *cur = None;
                    break;
                }
            }
        }
        if !looped {
            return;
        }
    });
}

/// `Play "file"`, `PlayLoop "file"` or `PlayStop` -> (looped, file); `None`
/// when `cmd` is not a sound command. `PlayStop` gives an empty file.
pub fn parse(cmd: &str) -> Option<(bool, String)> {
    let t = cmd.trim();
    let (word, rest) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
    let file = rest.trim().trim_matches('"').trim().to_string();
    match word.to_ascii_lowercase().as_str() {
        "play" if !file.is_empty() => Some((false, file)),
        "playloop" if !file.is_empty() => Some((true, file)),
        "playstop" => Some((false, String::new())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sound_commands() {
        assert_eq!(parse("Play \"a b.wav\""), Some((false, "a b.wav".into())));
        assert_eq!(parse("PlayLoop x.wav"), Some((true, "x.wav".into())));
        assert_eq!(parse("playstop"), Some((false, String::new())));
        assert_eq!(parse("Play"), None);
        assert_eq!(parse("player.sh --x"), None);
        assert_eq!(parse("gnome-clocks"), None);
    }
}
