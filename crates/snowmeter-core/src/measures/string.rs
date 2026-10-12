//! String measure.

use super::{MeasureCtx, MeasureImpl};
use crate::numparse::parse_f64_prefix;
use crate::parser::OptionReader;

pub struct StringMeasure {
    text: String,
    value: f64,
    published: bool,
}

impl StringMeasure {
    pub fn new() -> StringMeasure {
        StringMeasure { text: String::new(), value: 0.0, published: false }
    }
}

impl Default for StringMeasure {
    fn default() -> Self {
        StringMeasure::new()
    }
}

impl MeasureImpl for StringMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.text = opt.string("String", "");
        // The initial string is published at load unless the measure starts disabled/paused.
        if !self.published {
            self.published = !(opt.bool("Disabled", false) || opt.bool("Paused", false));
        }
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.published = true;
        self.value = parse_f64_prefix(&self.text);
        self.value
    }

    fn string_value(&self) -> Option<String> {
        self.published.then(|| self.text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    fn run(ini: &str) -> (Option<String>, f64) {
        let s = skin(ini);
        let mut m = StringMeasure::new();
        m.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        let published = m.string_value();
        let v = ctx_run(&s, |c| m.update(c));
        assert!(published.is_none() || published == m.string_value());
        (m.string_value(), v)
    }

    #[test]
    fn string_and_leading_number() {
        assert_eq!(run("[M]\nMeasure=String\nString=12abc\n"), (Some("12abc".into()), 12.0));
        assert_eq!(run("[M]\nMeasure=String\nString=abc\n"), (Some("abc".into()), 0.0));
        assert_eq!(run("[M]\nMeasure=String\nString=\" 3.5\"\n").1, 3.5);
        assert_eq!(run("[M]\nMeasure=String\n"), (Some(String::new()), 0.0));
    }

    #[test]
    fn variables_are_expanded() {
        assert_eq!(run("[Variables]\nV=7\n[M]\nMeasure=String\nString=n#V#\n").0.as_deref(), Some("n7"));
    }

    #[test]
    fn initial_string_published_at_load_unless_disabled() {
        let s = skin("[M]\nMeasure=String\nString=hi\n");
        let mut m = StringMeasure::new();
        m.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(m.string_value().as_deref(), Some("hi"));
        let s = skin("[M]\nMeasure=String\nString=hi\nDisabled=1\n");
        let mut m = StringMeasure::new();
        m.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(m.string_value(), None);
    }
}
