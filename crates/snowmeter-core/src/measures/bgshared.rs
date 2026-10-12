//! Helpers for plugins that scan the file system on a worker thread and share
//! one parent measure between several child measures.

use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

type Key = (String, String);
static REGISTRY: Mutex<Option<HashMap<Key, Arc<dyn Any + Send + Sync>>>> = Mutex::new(None);

fn key(skin_dir: &Path, section: &str) -> Key {
    (skin_dir.to_string_lossy().into_owned(), section.trim().to_ascii_lowercase())
}

/// Make `value` findable by child measures of the same skin (`Path=[Section]`).
pub fn publish<T: Any + Send + Sync>(skin_dir: &Path, section: &str, value: Arc<T>) {
    if let Ok(mut g) = REGISTRY.lock() {
        g.get_or_insert_with(HashMap::new).insert(key(skin_dir, section), value);
    }
}

pub fn lookup<T: Any + Send + Sync>(skin_dir: &Path, section: &str) -> Option<Arc<T>> {
    let g = REGISTRY.lock().ok()?;
    let any = g.as_ref()?.get(&key(skin_dir, section))?.clone();
    any.downcast::<T>().ok()
}

/// `[Name]` -> `Name`.
pub fn bracket_ref(s: &str) -> Option<&str> {
    let t = s.trim();
    (t.len() >= 3 && t.starts_with('[') && t.ends_with(']')).then(|| &t[1..t.len() - 1])
}

struct State<T> {
    value: T,
    running: bool,
}

/// A value computed by a background thread. `start` never blocks for long:
/// it waits `grace` for a quick job so small folders answer on the first update.
pub struct Bg<T> {
    inner: Arc<(Mutex<State<T>>, Condvar)>,
}

impl<T> Clone for Bg<T> {
    fn clone(&self) -> Self {
        Bg { inner: self.inner.clone() }
    }
}

impl<T: Clone + Send + 'static> Bg<T> {
    pub fn new(initial: T) -> Bg<T> {
        Bg { inner: Arc::new((Mutex::new(State { value: initial, running: false }), Condvar::new())) }
    }

    pub fn running(&self) -> bool {
        self.inner.0.lock().map(|s| s.running).unwrap_or(false)
    }

    pub fn get(&self) -> T
    where
        T: Default,
    {
        self.inner.0.lock().map(|s| s.value.clone()).unwrap_or_default()
    }

    /// Run `job` on a worker unless one is already running. Returns true if started.
    pub fn start(&self, grace: Duration, job: impl FnOnce() -> T + Send + 'static) -> bool {
        {
            let Ok(mut s) = self.inner.0.lock() else { return false };
            if s.running {
                return false;
            }
            s.running = true;
        }
        let inner = self.inner.clone();
        let spawned = std::thread::Builder::new().name("snowmeter-scan".into()).spawn(move || {
            let v = job();
            if let Ok(mut s) = inner.0.lock() {
                s.value = v;
                s.running = false;
            }
            inner.1.notify_all();
        });
        if spawned.is_err() {
            if let Ok(mut s) = self.inner.0.lock() {
                s.running = false;
            }
            return false;
        }
        if let Ok(g) = self.inner.0.lock() {
            let _ = self.inner.1.wait_timeout_while(g, grace, |s| s.running);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_round_trip_and_bg() {
        let d = Path::new("/x/y");
        publish(d, "Sec", Arc::new(5u32));
        assert_eq!(lookup::<u32>(d, "sec").as_deref(), Some(&5));
        assert!(lookup::<String>(d, "sec").is_none());
        assert!(lookup::<u32>(d, "other").is_none());
        assert_eq!(bracket_ref(" [Abc] "), Some("Abc"));
        assert_eq!(bracket_ref("[]"), None);
        assert_eq!(bracket_ref("C:/x"), None);
        let b: Bg<u32> = Bg::new(0);
        assert!(b.start(Duration::from_millis(500), || 7));
        assert_eq!(b.get(), 7);
        let slow: Bg<u32> = Bg::new(1);
        assert!(slow.start(Duration::from_millis(1), || {
            std::thread::sleep(Duration::from_millis(150));
            2
        }));
        assert_eq!(slow.get(), 1);
        assert!(!slow.start(Duration::from_millis(1), || 3));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(slow.get(), 2);
        assert!(!slow.running());
    }
}
