//! ConfigActive plugin: 1 when the named config is loaded, 0 when it is
//! installed but not loaded, -1 when it does not exist.

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

#[derive(Default)]
pub struct ConfigActive {
    name: String,
}

impl ConfigActive {
    pub fn new() -> ConfigActive {
        ConfigActive::default()
    }
}

impl MeasureImpl for ConfigActive {
    fn read_options(&mut self, opt: &OptionReader) {
        let mut n = opt.string("ConfigName", "");
        if n.trim().is_empty() {
            n = opt.string("Config", "");
        }
        self.name = n;
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        f64::from(crate::active::state(&self.name))
    }

    fn forced_min(&self) -> Option<f64> {
        Some(-1.0)
    }

    fn forced_max(&self) -> Option<f64> {
        Some(1.0)
    }
}
