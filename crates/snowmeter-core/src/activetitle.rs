//! Title of the focused window, published by the application (which owns the
//! display connection) and read by the `GetActiveTitle` plugin. The plugin
//! also records that someone is reading, so the application only polls the
//! window system while a skin uses it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static TITLE: Mutex<String> = Mutex::new(String::new());
static LAST_READ_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Replace the published title (empty when nothing is focused).
pub fn publish(title: &str) {
    if let Ok(mut t) = TITLE.lock() {
        if *t != title {
            *t = title.to_string();
        }
    }
}

/// Mark the title as wanted without reading it (a measure was created).
pub fn want() {
    LAST_READ_MS.store(now_ms(), Ordering::Relaxed);
}

/// The last published title; counts as a read.
pub fn current() -> String {
    want();
    TITLE.lock().map(|t| t.clone()).unwrap_or_default()
}

/// True if a measure read or asked for the title in the last `window_ms`.
pub fn wanted_within(window_ms: u64) -> bool {
    let last = LAST_READ_MS.load(Ordering::Relaxed);
    last != 0 && now_ms().saturating_sub(last) <= window_ms
}

/// Tests share the process-wide state; they take this lock.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_read_and_demand() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        publish("Firefox");
        assert_eq!(current(), "Firefox");
        assert!(wanted_within(5_000));
        publish("");
        assert_eq!(current(), "");
        LAST_READ_MS.store(1, Ordering::Relaxed);
        assert!(!wanted_within(5_000));
        want();
        assert!(wanted_within(5_000));
    }
}
