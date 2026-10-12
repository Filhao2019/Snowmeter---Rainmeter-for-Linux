//! The skin side of the Script measure API (`ScriptHost`).

use super::View;
use crate::measures::script::{MeasureSnapshot, ScriptHost};
use crate::parser::{ConfigParser, OptionReader};
use std::path::PathBuf;

pub(crate) struct SkinHost<'a> {
    pub parser: &'a ConfigParser,
    pub view: &'a View<'a>,
    pub size: (i32, i32),
    pub skin_dir: PathBuf,
}

impl ScriptHost for SkinHost<'_> {
    fn variable(&self, name: &str) -> Option<String> {
        self.parser.get_variable(name)
    }

    fn replace_variables(&self, text: &str) -> String {
        let s = self.parser.replace_variables(text);
        self.parser.replace_section_variables(&s, self.view)
    }

    fn parse_formula(&self, text: &str) -> Option<f64> {
        self.parser.parse_formula(text, Some(self.view))
    }

    fn measure(&self, name: &str) -> Option<MeasureSnapshot> {
        let e = self.view.find(name)?;
        let m = e.m.try_borrow().ok()?;
        Some(MeasureSnapshot { value: m.value(), relative: m.relative_value(), min: m.min_value(), max: m.max_value() })
    }

    fn measure_string(&self, name: &str, args: (u8, f64, i32, bool)) -> Option<String> {
        let e = self.view.find(name)?;
        let m = e.m.try_borrow().ok()?;
        Some(m.string_value(args.0, args.1, args.2, args.3))
    }

    fn meter_geometry(&self, name: &str) -> Option<[f64; 4]> {
        self.view.geom.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, g)| *g)
    }

    fn option(&self, section: &str, key: &str, default: &str) -> String {
        OptionReader::new(self.parser, section, Some(self.view)).string(key, default)
    }

    fn skin_size(&self) -> (i32, i32) {
        self.size
    }

    fn skin_dir(&self) -> PathBuf {
        self.skin_dir.clone()
    }
}
