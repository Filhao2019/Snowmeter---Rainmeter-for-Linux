//! Native `InputText` plugin: `!CommandMeasure <m> "ExecuteBatch N"` asks the
//! application to show a text entry over the skin; on Enter the application
//! runs `Command1..N` with `$UserInput$` replaced.

use super::{MeasureCtx, MeasureImpl};
use crate::color::Color;
use crate::parser::OptionReader;

/// Everything the application needs to show the entry and run the commands.
#[derive(Clone, Debug, PartialEq)]
pub struct InputTextSpec {
    pub measure: String,
    /// Skin-relative rectangle.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub solid_color: Color,
    pub font_color: Color,
    pub font_face: String,
    pub font_size: f64,
    pub default_value: String,
    pub focus_dismiss: bool,
    /// Commands to run on Enter, in order (already selected by `ExecuteBatch`).
    pub commands: Vec<String>,
}

const MAX_COMMANDS: usize = 99;

#[derive(Default)]
pub struct InputTextMeasure {
    name: String,
    spec: Option<InputTextSpec>,
    commands: Vec<String>,
    pending: Vec<InputTextSpec>,
    log: Vec<String>,
}

impl InputTextMeasure {
    pub fn new() -> InputTextMeasure {
        InputTextMeasure::default()
    }
}

/// Indices (1-based) selected by an `ExecuteBatch` argument: `ALL`, `3`, `1-3`
/// or `1;3;5-6`. Out-of-range indices are dropped.
pub fn parse_batch(arg: &str, count: usize) -> Vec<usize> {
    let arg = arg.trim().trim_matches('"').trim();
    if arg.is_empty() || arg.eq_ignore_ascii_case("all") {
        return (1..=count).collect();
    }
    let mut out = Vec::new();
    for part in arg.split([';', ',', ' ']).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>(), b.trim().parse::<usize>()),
            None => {
                let n = part.trim().parse::<usize>();
                (n.clone(), n)
            }
        };
        if let (Ok(a), Ok(b)) = (a, b) {
            for i in a.min(b)..=a.max(b) {
                if (1..=count).contains(&i) {
                    out.push(i);
                }
            }
        }
    }
    out
}

impl MeasureImpl for InputTextMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.name = opt.section.to_string();
        self.commands.clear();
        for i in 1..=MAX_COMMANDS {
            let c = opt.string(&format!("Command{i}"), "");
            if c.is_empty() {
                if i > 1 {
                    break;
                }
                continue;
            }
            self.commands.push(c);
        }
        let text = Color { r: 255, g: 255, b: 255, a: 255 };
        let solid = Color { r: 0, g: 0, b: 0, a: 255 };
        self.spec = Some(InputTextSpec {
            measure: self.name.clone(),
            x: opt.int("X", 0) as i32,
            y: opt.int("Y", 0) as i32,
            w: opt.int("W", 200) as i32,
            h: opt.int("H", 20) as i32,
            solid_color: opt.color("SolidColor", solid),
            font_color: opt.color("FontColor", text),
            font_face: opt.string("FontFace", "Sans"),
            font_size: opt.float("FontSize", 10.0),
            default_value: opt.string("DefaultValue", "").trim_matches('"').to_string(),
            focus_dismiss: opt.bool("FocusDismiss", true),
            commands: Vec::new(),
        });
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        0.0
    }

    fn command(&mut self, args: &str) {
        let Some(spec) = self.spec.clone() else { return };
        let args = args.trim();
        let (verb, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
        let mut spec = spec;
        match verb.to_ascii_lowercase().as_str() {
            "executebatch" => {
                spec.commands = parse_batch(rest, self.commands.len()).into_iter().map(|i| self.commands[i - 1].clone()).collect();
            }
            "" => spec.commands = self.commands.first().cloned().into_iter().collect(),
            other => {
                self.log.push(format!("InputText: unknown command '{other}' on [{}]", self.name));
                return;
            }
        }
        self.pending.push(spec);
    }

    fn take_input_requests(&mut self) -> Vec<InputTextSpec> {
        std::mem::take(&mut self.pending)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_selection() {
        assert_eq!(parse_batch("ALL", 3), vec![1, 2, 3]);
        assert_eq!(parse_batch("2", 3), vec![2]);
        assert_eq!(parse_batch("1-2", 3), vec![1, 2]);
        assert_eq!(parse_batch("1;3", 3), vec![1, 3]);
        assert_eq!(parse_batch("5", 3), Vec::<usize>::new());
        assert_eq!(parse_batch("\"2-3\"", 3), vec![2, 3]);
    }
}
