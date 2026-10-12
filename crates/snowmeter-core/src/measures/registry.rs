//! Registry measure: there is no registry on Linux, so this is a stub that
//! behaves like a missing key (value 0, no string).

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

#[derive(Default)]
pub struct RegistryMeasure {
    logged: bool,
    log: Vec<String>,
}

impl RegistryMeasure {
    pub fn new() -> RegistryMeasure {
        RegistryMeasure::default()
    }
}

impl MeasureImpl for RegistryMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        if !self.logged {
            self.logged = true;
            self.log.push(format!(
                "Registry: not supported on Linux (RegHKey={} RegKey={})",
                opt.string("RegHKey", "HKEY_CURRENT_USER"),
                opt.string("RegKey", "")
            ));
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        0.0
    }

    fn auto_max(&self) -> bool {
        true
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
    fn stub_is_zero_with_one_log_line() {
        let s = skin("[M]\nMeasure=Registry\nRegKey=Software\\X\nRegValue=Y\n");
        let mut r = RegistryMeasure::new();
        let opt = OptionReader { parser: &s.parser, section: "M", sv: None };
        r.read_options(&opt);
        r.read_options(&opt);
        assert_eq!(ctx_run(&s, |c| r.update(c)), 0.0);
        assert!(r.string_value().is_none());
        let log = r.take_log();
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("Software\\X"));
        assert!(r.take_log().is_empty());
    }
}
