//! Process-wide list of the configs that exist and the ones that are loaded.
//! The application publishes it whenever windows open or close; the
//! `ConfigActive` plugin reads it.

use std::collections::HashSet;
use std::sync::RwLock;

#[derive(Default)]
struct Lists {
    known: HashSet<String>,
    active: HashSet<String>,
}

static LISTS: RwLock<Option<Lists>> = RwLock::new(None);

fn norm(name: &str) -> String {
    let mut s = name.trim().replace('/', "\\").to_ascii_lowercase();
    while s.ends_with('\\') {
        s.pop();
    }
    s
}

/// Replace both lists (config names such as `illustro\Clock`).
pub fn publish<K, A>(known: K, active: A)
where
    K: IntoIterator,
    K::Item: AsRef<str>,
    A: IntoIterator,
    A::Item: AsRef<str>,
{
    let lists = Lists {
        known: known.into_iter().map(|s| norm(s.as_ref())).collect(),
        active: active.into_iter().map(|s| norm(s.as_ref())).collect(),
    };
    if let Ok(mut g) = LISTS.write() {
        *g = Some(lists);
    }
}

/// 1 loaded, 0 installed but not loaded, -1 unknown config.
pub fn state(config: &str) -> i32 {
    let n = norm(config);
    let Ok(g) = LISTS.read() else { return -1 };
    let Some(l) = g.as_ref() else { return -1 };
    if l.active.contains(&n) {
        1
    } else if l.known.contains(&n) {
        0
    } else {
        -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states() {
        publish(["illustro\\Clock", "Other"], ["Illustro/clock/"]);
        assert_eq!(state("ILLUSTRO\\clock"), 1);
        assert_eq!(state("other"), 0);
        assert_eq!(state("nothing"), -1);
        assert_eq!(state(""), -1);
    }
}
