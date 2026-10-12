//! Measures: the common `Measure` wrapper (Measure.cpp) plus the per-type
//! implementations behind the `MeasureImpl` trait.

pub mod base;
pub mod calc;
pub mod disk;
pub mod ifactions;
pub mod loop_;
pub mod memory;
pub mod net;
pub mod plugin;
pub mod registry;
pub mod string;
pub mod substitute;
#[cfg(test)]
pub(crate) mod testutil;
pub mod time;
pub mod uptime;
pub mod cpu;

use crate::env::Environment;
use crate::parser::OptionReader;
use base::{encode_url, escape_regexp, format_value, median3, remove_trailing_zero, AUTOSCALE_OFF};
use ifactions::IfActions;
use std::cell::RefCell;
use std::path::Path;
use substitute::{parse_substitute, Substitute};

/// Everything a measure may need during one update.
pub struct MeasureCtx<'a> {
    pub env: &'a Environment,
    pub skin_dir: &'a Path,
    pub update_ms: i64,
    pub log: &'a mut Vec<String>,
    /// Current value (`GetValue()`) of another measure by name, case-insensitive.
    pub lookup: &'a dyn Fn(&str) -> Option<f64>,
    /// Skin update counter (1 on the first update), used by Calc's `Counter`.
    pub counter: u64,
}

pub trait MeasureImpl {
    fn read_options(&mut self, opt: &OptionReader);
    /// Compute the raw value for this tick.
    fn update(&mut self, ctx: &mut MeasureCtx) -> f64;
    /// String value, `None` for pure-number measures. Substitute is applied by `Measure`.
    fn string_value(&self) -> Option<String> {
        None
    }
    fn default_max(&self) -> f64 {
        1.0
    }
    /// `!CommandMeasure` arguments.
    fn command(&mut self, _args: &str) {}

    /// Range the measure dictates regardless of `MinValue`/`MaxValue` options.
    fn forced_min(&self) -> Option<f64> {
        None
    }
    fn forced_max(&self) -> Option<f64> {
        None
    }
    /// True when `MaxValue` 0 means "track the maximum automatically".
    fn auto_max(&self) -> bool {
        false
    }
    /// Windows timestamp (Time measures) for `[M:TimeStamp]`.
    fn timestamp(&self) -> Option<i64> {
        None
    }
    /// `[M:Func(args)]` custom function call (Script/Plugin measures).
    fn call_function(&mut self, _call: &str) -> Option<String> {
        None
    }
    /// Messages produced while reading options; drained by `Measure`.
    fn take_log(&mut self) -> Vec<String> {
        Vec::new()
    }
}

pub struct Measure {
    pub name: String,
    pub type_name: String,
    /// False when `Measure=` named no known type (the skin should not create a section).
    pub known_type: bool,
    imp: Box<dyn MeasureImpl>,
    min: f64,
    max: f64,
    invert: bool,
    avg_size: usize,
    avg: Vec<f64>,
    avg_pos: usize,
    disabled: bool,
    paused: bool,
    divider: i64,
    counter: i64,
    dynamic: bool,
    groups: Vec<String>,
    substitute: Substitute,
    on_update: String,
    on_change: String,
    value: f64,
    value_assigned: bool,
    log_max: bool,
    median: [f64; 3],
    median_pos: usize,
    ifs: IfActions,
    old_value: Option<(f64, String)>,
    pending: Vec<String>,
    log: RefCell<Vec<String>>,
    initialized: bool,
}

fn atoi(s: &str) -> i64 {
    let s = s.trim_start();
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')) {
            end = i + 1;
        } else {
            break;
        }
    }
    s[..end].parse().unwrap_or(0)
}

impl Measure {
    /// Factory (Measure.cpp `Create` + `Measure=Plugin` redirection).
    pub fn new(name: &str, type_name: &str, opt: &OptionReader) -> Measure {
        let mut log = Vec::new();
        let (imp, known) = plugin::create_measure_impl(type_name, name, opt, &mut log);
        Measure::from_impl(name, type_name, imp, known, opt, log)
    }

    /// Wrap a ready-made implementation (native plugins, tests). `log` holds
    /// messages produced while creating it.
    pub fn from_impl(
        name: &str,
        type_name: &str,
        imp: Box<dyn MeasureImpl>,
        known: bool,
        opt: &OptionReader,
        log: Vec<String>,
    ) -> Measure {
        let mut m = Measure {
            name: name.to_string(),
            type_name: type_name.to_string(),
            known_type: known,
            imp,
            min: 0.0,
            max: 1.0,
            invert: false,
            avg_size: 0,
            avg: Vec::new(),
            avg_pos: 0,
            disabled: false,
            paused: false,
            divider: 1,
            counter: 1,
            dynamic: false,
            groups: Vec::new(),
            substitute: Substitute::default(),
            on_update: String::new(),
            on_change: String::new(),
            value: 0.0,
            value_assigned: false,
            log_max: false,
            median: [0.0; 3],
            median_pos: 0,
            ifs: IfActions::new(),
            old_value: None,
            pending: Vec::new(),
            log: RefCell::new(log),
            initialized: false,
        };
        m.read_options(opt);
        m.initialized = true;
        m
    }

    /// (Re)read all options. Called at load and, with `DynamicVariables=1`,
    /// before each update that will run (see [`Measure::wants_reread`]).
    pub fn read_options(&mut self, opt: &OptionReader) {
        self.imp.read_options(opt);
        self.log.borrow_mut().extend(self.imp.take_log());

        self.dynamic = opt.bool("DynamicVariables", false);
        self.groups = opt
            .string("Group", "")
            .split('|')
            .filter(|g| !g.is_empty())
            .map(|g| g.to_ascii_lowercase())
            .collect();

        let was_log_max = self.log_max;
        let mut min = opt.float("MinValue", 0.0);
        let mut max = opt.float("MaxValue", self.imp.default_max());
        if let Some(m) = self.imp.forced_min() {
            min = m;
        }
        if let Some(m) = self.imp.forced_max() {
            max = m;
        }
        self.log_max = self.imp.auto_max() && max <= 0.0;
        if self.log_max {
            if was_log_max {
                min = self.min;
                max = self.max;
            } else {
                max = 1.0;
            }
        }
        self.min = min;
        self.max = max;

        self.invert = opt.bool("InvertMeasure", false);
        self.disabled = opt.bool("Disabled", false);
        self.paused = opt.bool("Paused", false);
        let default_div = opt.parser.read_int("Rainmeter", "DefaultUpdateDivider", 1, None);
        let divider = opt.int("UpdateDivider", default_div);
        if !self.initialized {
            self.counter = divider;
        }
        self.divider = divider;

        let ifa = opt.without_sections();
        self.on_update = ifa.action("OnUpdateAction");
        let old_on_change_empty = self.on_change.is_empty();
        self.on_change = ifa.action("OnChangeAction");
        self.ifs.read_options(opt);
        if !self.initialized {
            self.ifs.read_condition_options(opt);
        }
        self.avg_size = opt.int("AverageSize", 0).max(0) as usize;

        let regexp = opt.bool("RegExpSubstitute", false);
        let subs = opt.string("Substitute", "");
        let mut log = Vec::new();
        self.substitute = parse_substitute(&subs, regexp, &mut log);
        self.log.borrow_mut().extend(log);

        if self.initialized && old_on_change_empty && !self.on_change.is_empty() {
            self.do_change_action(false);
        }
    }

    /// True when `DynamicVariables=1` and this tick will actually update:
    /// the caller should then run [`Measure::read_options`] first.
    pub fn wants_reread(&self) -> bool {
        self.dynamic && self.divider >= 0 && self.counter + 1 >= self.divider
    }

    fn update_counter(&mut self, force: bool) -> bool {
        if force {
            self.counter = self.divider.max(0);
        } else if self.divider < 0 {
            return false;
        }
        self.counter += 1;
        if self.counter < self.divider {
            return false;
        }
        self.counter = 0;
        true
    }

    /// One normal skin update. `cond_opt` is used with `DynamicVariables=1`
    /// to re-read IfCondition/IfMatch after the new value is known.
    /// Returns true when the measure actually updated.
    pub fn update(&mut self, ctx: &mut MeasureCtx, cond_opt: Option<&OptionReader>) -> bool {
        self.update_impl(ctx, cond_opt, false)
    }

    /// Update ignoring the divider (`!UpdateMeasure`, forced refresh update).
    pub fn force_update(&mut self, ctx: &mut MeasureCtx, cond_opt: Option<&OptionReader>) -> bool {
        self.update_impl(ctx, cond_opt, true)
    }

    fn update_impl(&mut self, ctx: &mut MeasureCtx, cond_opt: Option<&OptionReader>, force: bool) -> bool {
        if self.paused {
            return false;
        }
        if self.disabled {
            self.value = 0.0;
            self.ifs.set_state(0.0);
            return false;
        }
        if !self.update_counter(force) {
            return false;
        }

        self.value = self.imp.update(ctx);
        self.value_assigned = true;
        self.log.borrow_mut().extend(self.imp.take_log());
        if let Some(m) = self.imp.forced_max() {
            if !self.log_max {
                self.max = m;
            }
        }
        if let Some(m) = self.imp.forced_min() {
            self.min = m;
        }

        if self.avg_size > 0 {
            if self.avg.len() != self.avg_size {
                self.avg = vec![self.value; self.avg_size];
                self.avg_pos = 0;
            }
            self.avg[self.avg_pos] = self.value;
            self.avg_pos = (self.avg_pos + 1) % self.avg_size;
            self.value = self.avg.iter().sum::<f64>() / self.avg_size as f64;
        } else {
            self.avg.clear();
        }

        if self.log_max {
            self.median[self.median_pos] = self.value;
            self.median_pos = (self.median_pos + 1) % 3;
            let med = median3(self.median[0], self.median[1], self.median[2]);
            self.max = self.max.max(med);
            self.min = self.min.min(med);
        }

        if self.dynamic {
            if let Some(o) = cond_opt {
                self.ifs.read_condition_options(o);
            }
        }

        // If-actions see the raw (non-inverted) value.
        let own_name = self.name.clone();
        let own_value = self.get_value();
        let lookup = ctx.lookup;
        let lk = |n: &str| {
            if n.eq_ignore_ascii_case(&own_name) {
                Some(own_value)
            } else {
                lookup(n)
            }
        };
        let subject = self.string_only().unwrap_or_default();
        let mut fired = Vec::new();
        self.ifs.evaluate(self.value, &subject, &lk, &mut fired, ctx.log);
        self.pending.extend(fired);

        if !self.on_update.is_empty() {
            self.pending.push(self.on_update.clone());
        }
        self.do_change_action(true);
        true
    }

    fn do_change_action(&mut self, execute: bool) {
        if self.on_change.is_empty() || !self.value_assigned {
            return;
        }
        let new = (self.get_value(), self.string_only().unwrap_or_default());
        match &self.old_value {
            None => self.old_value = Some(new),
            Some(old) if execute => {
                if *old != new {
                    self.pending.push(self.on_change.clone());
                    self.old_value = Some(new);
                }
            }
            Some(_) => self.old_value = Some(new),
        }
    }

    /// Actions fired this tick (If*, OnUpdate, OnChange), unexpanded. The skin executes them.
    pub fn take_actions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending)
    }

    /// Log lines produced since the last call.
    pub fn take_log(&mut self) -> Vec<String> {
        self.log.borrow_mut().extend(self.imp.take_log());
        std::mem::take(&mut *self.log.borrow_mut())
    }

    /// `GetValue()`: the value after `InvertMeasure`.
    pub fn value(&self) -> f64 {
        self.get_value()
    }

    fn get_value(&self) -> f64 {
        if self.invert {
            self.max - self.value + self.min
        } else {
            self.value
        }
    }

    /// The raw value before inversion.
    pub fn raw_value(&self) -> f64 {
        self.value
    }

    /// `GetRelativeValue()`: 0..1 position within [min, max]; 1.0 when the range is empty.
    pub fn relative_value(&self) -> f64 {
        let range = self.max - self.min;
        if range != 0.0 {
            (self.get_value().min(self.max).max(self.min) - self.min) / range
        } else {
            1.0
        }
    }

    pub fn min_value(&self) -> f64 {
        self.min
    }
    pub fn max_value(&self) -> f64 {
        self.max
    }

    /// The measure's own string (after Substitute), `None` for number-only measures.
    pub fn string_only(&self) -> Option<String> {
        self.imp.string_value().map(|s| {
            let mut log = Vec::new();
            let r = self.substitute.apply(&s, &mut log);
            self.log.borrow_mut().extend(log);
            r
        })
    }

    /// True when the measure has a string value (`GetStringValue() != null`).
    pub fn is_string_measure(&self) -> bool {
        self.imp.string_value().is_some()
    }

    /// `GetStringOrFormattedValue`: the string value if there is one, else the
    /// formatted number (then Substitute).
    pub fn string_value(&self, auto_scale: u8, scale: f64, decimals: i32, percentual: bool) -> String {
        if let Some(s) = self.string_only() {
            return s;
        }
        let text = format_value(self.get_value(), self.relative_value(), auto_scale, scale, decimals, percentual);
        let mut log = Vec::new();
        let r = self.substitute.apply(&text, &mut log);
        self.log.borrow_mut().extend(log);
        r
    }

    /// `[M]` / `[&M]`: `GetStringOrFormattedValue(OFF, 1, -1, false)`.
    pub fn default_string(&self) -> String {
        self.string_value(AUTOSCALE_OFF, 1.0, -1, false)
    }

    /// Section-variable selector (`[M]`, `[M:%]`, `[M:/1000]`, `[M:2]`,
    /// `[M:MaxValue]`, `[M:MaxValue:/5, 2]`, `[M:EscapeRegExp]`, ...).
    /// `param` is the text after the last `:`; for names like
    /// `Measure:MaxValue` the caller passes `Some("MaxValue:" + rest)`.
    /// `None` means the selector is invalid and the text stays unchanged.
    pub fn section_variable_opt(&self, param: Option<&str>) -> Option<String> {
        let Some(p) = param else { return Some(self.default_string()) };
        let first_alpha = p.chars().next().is_some_and(|c| c.is_alphabetic());
        #[derive(PartialEq)]
        enum Kind {
            Raw,
            Percent,
            Max,
            Min,
        }
        let mut kind = Kind::Raw;
        let mut sel = p;
        if first_alpha {
            let lower = p.to_ascii_lowercase();
            let (key, rest) = match lower.find(':') {
                Some(i) => (&lower[..i], Some(&p[i + 1..])),
                None => (lower.as_str(), None),
            };
            match (key, rest) {
                ("maxvalue", r) => {
                    kind = Kind::Max;
                    sel = r.unwrap_or("");
                }
                ("minvalue", r) => {
                    kind = Kind::Min;
                    sel = r.unwrap_or("");
                }
                ("escaperegexp", None) => return Some(escape_regexp(&self.string_only().unwrap_or_default())),
                ("encodeurl", None) => return Some(encode_url(&self.string_only().unwrap_or_default())),
                ("timestamp", None) => match self.imp.timestamp() {
                    Some(t) => return Some(t.to_string()),
                    None => sel = "",
                },
                _ => return None,
            }
        }

        let mut scale = 1.0;
        let mut dec_sz: Option<&str> = sel.find(',').map(|i| &sel[i + 1..]);
        if sel.starts_with('%') {
            if kind == Kind::Max || kind == Kind::Min {
                return None;
            }
            kind = Kind::Percent;
        } else if let Some(rest) = sel.strip_prefix('/') {
            let s = atoi(rest);
            if s == 0 {
                return None;
            }
            scale = s as f64;
        } else {
            if dec_sz.is_some() {
                return None;
            }
            dec_sz = Some(sel);
        }
        let value = match kind {
            Kind::Percent => self.relative_value() * 100.0,
            Kind::Max => self.max / scale,
            Kind::Min => self.min / scale,
            Kind::Raw => self.get_value() / scale,
        };
        let mut decimals = 10usize;
        let mut specified = false;
        if let Some(d) = dec_sz {
            let d = d.trim_start();
            if !d.is_empty() {
                decimals = atoi(d).clamp(0, 32) as usize;
                specified = true;
            }
        }
        let mut out = format!("{value:.decimals$}");
        if !specified {
            remove_trailing_zero(&mut out);
        }
        Some(out)
    }

    /// Like [`Measure::section_variable_opt`]; invalid selectors give "".
    pub fn section_variable(&self, param: Option<&str>) -> String {
        self.section_variable_opt(param).unwrap_or_default()
    }

    /// `[M:Func(args)]` for Script/Plugin measures.
    pub fn call_function(&mut self, call: &str) -> Option<String> {
        self.imp.call_function(call)
    }

    /// `!CommandMeasure`.
    pub fn command(&mut self, args: &str) {
        self.imp.command(args);
        self.log.borrow_mut().extend(self.imp.take_log());
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    /// `!DisableMeasure` / `!EnableMeasure` (the skin also writes the option back).
    pub fn set_disabled(&mut self, v: bool) {
        self.disabled = v;
    }
    /// `!PauseMeasure` / `!UnpauseMeasure`.
    pub fn set_paused(&mut self, v: bool) {
        self.paused = v;
    }
    /// `Group` names, lowercase.
    pub fn groups(&self) -> &[String] {
        &self.groups
    }
    pub fn in_group(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g.eq_ignore_ascii_case(group))
    }
    pub fn dynamic_variables(&self) -> bool {
        self.dynamic
    }
    pub fn update_divider(&self) -> i64 {
        self.divider
    }
}
