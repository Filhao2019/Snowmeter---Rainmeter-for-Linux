//! GetActiveTitle plugin: the title of the focused window as the string value.
//! The application publishes it (X11 `_NET_ACTIVE_WINDOW` + `_NET_WM_NAME`);
//! on a Wayland-only session the title stays empty.

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

#[derive(Default)]
pub struct GetActiveTitle {
    title: String,
}

impl GetActiveTitle {
    pub fn new() -> GetActiveTitle {
        GetActiveTitle::default()
    }
}

impl MeasureImpl for GetActiveTitle {
    fn read_options(&mut self, _opt: &OptionReader) {
        crate::activetitle::want();
    }

    /// 1 while a window has focus, else 0.
    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.title = crate::activetitle::current();
        f64::from(!self.title.is_empty())
    }

    fn string_value(&self) -> Option<String> {
        Some(self.title.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    #[test]
    fn follows_the_published_title() {
        let _g = crate::activetitle::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let sk = skin("[M]\nMeasure=Plugin\nPlugin=GetActiveTitle\n");
        let mut m = GetActiveTitle::new();
        m.read_options(&OptionReader { parser: &sk.parser, section: "M", sv: None });
        crate::activetitle::publish("Terminal \u{2014} bash");
        assert_eq!(ctx_run(&sk, |c| m.update(c)), 1.0);
        assert_eq!(m.string_value().as_deref(), Some("Terminal \u{2014} bash"));
        crate::activetitle::publish("");
        assert_eq!(ctx_run(&sk, |c| m.update(c)), 0.0);
        assert_eq!(m.string_value().as_deref(), Some(""));
    }
}
