//! Port of Rainmeter's `ConfigParser`: INI + `@Include` merging, `[Variables]`,
//! built-in variables, option reading with MeterStyle fallback, substitution
//! and numeric/color parsing.

use crate::color::{parse_color_with, Color};
use crate::env::{Environment, Rect};
use crate::formula;
use crate::ini::IniFile;
use crate::numparse;
use crate::paths::resolve_path;
use crate::subst::{self, Mode};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

const MAX_INCLUDE_DEPTH: usize = 100;
/// Upper bound on files read for one skin (stops `@Include` fan-out).
const MAX_INCLUDE_FILES: usize = 500;
/// Sections that are not measures or meters.
const SPECIAL_SECTIONS: [&str; 3] = ["RAINMETER", "VARIABLES", "METADATA"];
/// Options that never come from a style.
const NON_INHERITABLE: [&str; 4] = ["METER", "MEASURE", "METERSTYLE", "@INHERIT"];

/// Lets the parser resolve `[Measure]`, `[&Measure]`, `[Meter:X]` references
/// and measure names inside formulas. Implemented by the skin.
pub trait SectionValues {
    /// `name` is the section (measure or meter) name; `param` the selector
    /// after the last `:` (`MaxValue`, `%`, `/5`, `XW`, ...). `None` returns the
    /// measure's default string. Unknown sections return `None`.
    fn section_variable(&self, name: &str, param: Option<&str>) -> Option<String>;
    /// Current numeric value of a measure, for formulas.
    fn measure_value(&self, name: &str) -> Option<f64>;
}

/// Resolves nothing; `Some(&NoSectionValues)` behaves like a skin with no sections.
pub struct NoSectionValues;

impl SectionValues for NoSectionValues {
    fn section_variable(&self, _name: &str, _param: Option<&str>) -> Option<String> {
        None
    }
    fn measure_value(&self, _name: &str) -> Option<f64> {
        None
    }
}

/// Which skin file to load.
pub struct SkinIdentity {
    /// Config path with backslashes, e.g. `illustro\Clock`.
    pub config: String,
    /// First component of `config`, e.g. `illustro`.
    pub root_config: String,
    /// File name, e.g. `Clock.ini`.
    pub file: String,
    pub ini_path: PathBuf,
}

pub struct ConfigParser {
    /// (UPPER section, UPPER key) -> raw value.
    values: HashMap<(String, String), String>,
    /// Section names in measure/meter creation order (original spelling).
    section_order: Vec<String>,
    known_sections: HashSet<String>,
    /// New sections are inserted before this (UPPER) section; `None` = at the end.
    insert_before: Option<String>,
    /// Keys read from `[Variables]` in parse order (duplicates included).
    var_list: Vec<String>,
    /// UPPER name -> (original name, value).
    variables: HashMap<String, (String, String)>,
    builtins: HashMap<String, String>,
    monitors: Vec<Rect>,
    work_areas: Vec<Rect>,
    geometry: [f64; 4],
    messages: Vec<String>,
    /// Canonical paths of the files being read (include cycle guard).
    include_stack: Vec<PathBuf>,
    /// Total files read so far (include fan-out guard).
    files_read: usize,
}

fn up(s: &str) -> String {
    s.to_ascii_uppercase()
}

fn slash_dir(p: &Path) -> String {
    format!("{}/", p.to_string_lossy().trim_end_matches('/'))
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn section_lookup(sv: &dyn SectionValues, name: &str) -> Option<String> {
    sv.section_variable(name, None)
        .or_else(|| subst::split_selector(name).and_then(|(s, sel)| sv.section_variable(s, Some(sel))))
}

impl ConfigParser {
    /// Read the skin file, its `@Include`s and `[Variables]`, and define the
    /// built-in variables.
    pub fn load(env: &Environment, id: &SkinIdentity) -> io::Result<ConfigParser> {
        let mut p = ConfigParser {
            values: HashMap::new(),
            section_order: Vec::new(),
            known_sections: HashSet::new(),
            insert_before: None,
            var_list: Vec::new(),
            variables: HashMap::new(),
            builtins: HashMap::new(),
            monitors: env.monitors.clone(),
            work_areas: env.work_areas.clone(),
            geometry: [0.0; 4],
            messages: Vec::new(),
            include_stack: Vec::new(),
            files_read: 0,
        };
        p.builtins = Self::build_builtins(env, id);
        p.read_ini_file(&id.ini_path, 0)?;
        p.read_variables();
        p.insert_before = None;
        Ok(p)
    }

    fn build_builtins(env: &Environment, id: &SkinIdentity) -> HashMap<String, String> {
        let skins = slash_dir(&env.skins_dir);
        let root_path = format!("{}{}/", skins, id.root_config);
        let current_path = id.ini_path.parent().map(slash_dir).unwrap_or_default();
        let entries = [
            ("@", format!("{root_path}@Resources/")),
            ("PROGRAMPATH", slash_dir(&env.program_dir)),
            ("PROGRAMDRIVE", String::new()),
            ("SETTINGSPATH", slash_dir(&env.settings_dir)),
            ("SKINSPATH", skins.clone()),
            ("PLUGINSPATH", format!("{}Plugins/", slash_dir(&env.program_dir))),
            ("ADDONSPATH", format!("{}Addons/", slash_dir(&env.settings_dir))),
            ("CURRENTPATH", current_path),
            ("ROOTCONFIGPATH", root_path),
            ("CURRENTFILE", id.file.clone()),
            ("CURRENTCONFIG", id.config.clone()),
            ("ROOTCONFIG", id.root_config.clone()),
            ("CONFIGEDITOR", "xdg-open".to_string()),
            ("CRLF", "\n".to_string()),
        ];
        entries.into_iter().map(|(k, v)| (up(k), v)).collect()
    }

    /// Diagnostics collected while loading (unreadable includes, loops, ...).
    pub fn log_messages(&self) -> &[String] {
        &self.messages
    }

    /// Update `#CURRENTCONFIGX#`, `Y`, `WIDTH` and `HEIGHT`.
    pub fn set_window_geometry(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.geometry = [x, y, w, h];
    }

    // ------------------------------------------------------------ loading

    fn insert_section(&mut self, name: &str) {
        let idx = self
            .insert_before
            .as_ref()
            .and_then(|b| self.section_order.iter().position(|s| up(s) == *b))
            .unwrap_or(self.section_order.len());
        self.section_order.insert(idx, name.to_string());
    }

    fn read_ini_file(&mut self, path: &Path, depth: usize) -> io::Result<()> {
        if depth > MAX_INCLUDE_DEPTH {
            self.messages.push(format!("Infinite include loop at {}", path.display()));
            return Ok(());
        }
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if self.include_stack.contains(&canon) {
            self.messages.push(format!("Infinite include loop at {}", path.display()));
            return Ok(());
        }
        if self.files_read >= MAX_INCLUDE_FILES {
            if self.files_read == MAX_INCLUDE_FILES {
                self.files_read += 1;
                self.messages.push(format!("Too many @Include files, skipping {}", path.display()));
            }
            return Ok(());
        }
        self.files_read += 1;
        self.include_stack.push(canon);
        let r = self.read_ini_inner(path, depth);
        self.include_stack.pop();
        r
    }

    fn read_ini_inner(&mut self, path: &Path, depth: usize) -> io::Result<()> {
        let ini = match IniFile::load(path) {
            Ok(ini) => ini,
            Err(e) if depth == 0 => return Err(e),
            Err(e) => {
                self.messages.push(format!("Unable to read file: {} ({e})", path.display()));
                return Ok(());
            }
        };
        // Pass 1: register every section before any key (an @Include is a key).
        for sec in ini.sections() {
            if self.known_sections.insert(up(&sec.name)) {
                self.insert_section(&sec.name);
            }
        }
        // Pass 2: keys, in order.
        let section_count = ini.sections().len();
        for (index, sec) in ini.sections().iter().enumerate() {
            let is_variables = sec.name.eq_ignore_ascii_case("Variables");
            let is_metadata = sec.name.eq_ignore_ascii_case("Metadata");
            let mut reset_insert_pos = true;
            for (key, value) in &sec.entries {
                if key.chars().count() >= 8 && key.chars().take(8).collect::<String>().eq_ignore_ascii_case("@Include") {
                    if value.is_empty() {
                        continue;
                    }
                    self.read_variables();
                    let include = self.replace_variables(value);
                    let dir = path.parent().unwrap_or_else(|| Path::new("."));
                    let target = resolve_path(dir, &include);
                    if reset_insert_pos {
                        if index + 1 == section_count {
                            self.insert_before = None;
                            reset_insert_pos = false;
                        } else if let Some(i) = self.section_order.iter().position(|s| s.eq_ignore_ascii_case(&sec.name)) {
                            self.insert_before = self.section_order.get(i + 1).map(|s| up(s));
                            reset_insert_pos = false;
                        }
                    }
                    let saved = self.insert_before.clone();
                    let _ = self.read_ini_file(&target, depth + 1);
                    self.insert_before = saved;
                    continue;
                }
                if is_metadata {
                    continue;
                }
                self.values.insert((up(&sec.name), up(key)), value.clone());
                if is_variables {
                    self.var_list.push(key.clone());
                }
            }
        }
        Ok(())
    }

    /// Evaluate every `[Variables]` key read so far into the variable table.
    fn read_variables(&mut self) {
        for i in 0..self.var_list.len() {
            let name = self.var_list[i].clone();
            let raw = self.values.get(&("VARIABLES".to_string(), up(&name))).cloned().unwrap_or_default();
            let value = self.replace_variables(&raw);
            self.set_variable(&name, &value);
        }
    }

    // ---------------------------------------------------------- variables

    pub fn set_variable(&mut self, name: &str, value: &str) {
        let key = up(name);
        match self.variables.get_mut(&key) {
            Some(entry) => entry.1 = value.to_string(),
            None => {
                self.variables.insert(key, (name.to_string(), value.to_string()));
            }
        }
    }

    /// Built-ins first (they shadow user variables), then user variables.
    pub fn get_variable(&self, name: &str) -> Option<String> {
        self.lookup_variable(name, None)
    }

    fn lookup_variable(&self, name: &str, current_section: Option<&str>) -> Option<String> {
        let key = up(name);
        if key == "CURRENTSECTION" {
            return Some(current_section.unwrap_or("").to_string());
        }
        if let Some(v) = self.builtins.get(&key) {
            return Some(v.clone());
        }
        let g = &self.geometry;
        match key.as_str() {
            "CURRENTCONFIGX" => return Some(fmt_num(g[0])),
            "CURRENTCONFIGY" => return Some(fmt_num(g[1])),
            "CURRENTCONFIGWIDTH" => return Some(fmt_num(g[2])),
            "CURRENTCONFIGHEIGHT" => return Some(fmt_num(g[3])),
            _ => {}
        }
        if let Some(v) = self.monitor_variable(&key) {
            return Some(v);
        }
        self.variables.get(&key).map(|(_, v)| v.clone())
    }

    /// `SCREENAREA*`, `WORKAREA*`, `PSCREENAREA*`, `PWORKAREA*`, `VSCREENAREA*`,
    /// with `@N` (1-based) for the non-prefixed forms.
    fn monitor_variable(&self, key: &str) -> Option<String> {
        let (base, at) = match key.rfind('@') {
            Some(p) if p > 0 => (&key[..p], Some(key[p + 1..].parse::<i64>().ok()?)),
            _ => (key, None),
        };
        // (prefix, work area?, primary only?, virtual?)
        const FORMS: [(&str, bool, bool, bool); 5] = [
            ("PSCREENAREA", false, true, false),
            ("PWORKAREA", true, true, false),
            ("VSCREENAREA", false, true, true),
            ("SCREENAREA", false, false, false),
            ("WORKAREA", true, false, false),
        ];
        for (prefix, work, fixed, is_virtual) in FORMS {
            let Some(field) = base.strip_prefix(prefix) else { continue };
            if !matches!(field, "X" | "Y" | "WIDTH" | "HEIGHT") {
                continue;
            }
            if fixed && at.is_some() {
                return None;
            }
            let list = if work { &self.work_areas } else { &self.monitors };
            let rect = if is_virtual {
                let first = self.monitors.first()?;
                let (mut l, mut t, mut r, mut b) = (first.x, first.y, first.x + first.w, first.y + first.h);
                for m in &self.monitors {
                    l = l.min(m.x);
                    t = t.min(m.y);
                    r = r.max(m.x + m.w);
                    b = b.max(m.y + m.h);
                }
                Rect { x: l, y: t, w: r - l, h: b - t }
            } else {
                match at {
                    None => *list.first()?,
                    Some(n) if n >= 1 && (n as usize) <= list.len() => list[n as usize - 1],
                    Some(_) => return None,
                }
            };
            return Some(fmt_num(match field {
                "X" => rect.x,
                "Y" => rect.y,
                "WIDTH" => rect.w,
                _ => rect.h,
            }));
        }
        None
    }

    // -------------------------------------------------------- substitution

    /// `ReplaceVariables(s, isNewStyle=true)`: `%ENV%`, `[#Var]`, `[%ENV]`,
    /// `[\65]`, then `#Var#`. Section variables are left alone.
    pub fn replace_variables(&self, s: &str) -> String {
        if !s.contains(['#', '%', '[']) {
            return s.to_string();
        }
        let var = |n: &str| self.lookup_variable(n, None);
        let s = subst::expand_env_percent(s);
        let s = subst::expand_brackets(&s, Mode::HashOnly, &var, None);
        subst::expand_old_style(&s, &var)
    }

    /// `ReplaceMeasures`: `[&Measure]`, `[Measure]`, `[Meter:X]` plus the other
    /// bracket forms, resolved through `sv`. Old-style `#Var#` is not touched.
    pub fn replace_section_variables(&self, s: &str, sv: &dyn SectionValues) -> String {
        if !s.contains('[') {
            return s.to_string();
        }
        let var = |n: &str| self.lookup_variable(n, None);
        let sec = |n: &str| section_lookup(sv, n);
        let s = subst::expand_brackets(s, Mode::AllKeys, &var, Some(&sec));
        subst::expand_old_measures(&s, &sec)
    }

    /// The pipeline for an ordinary option value read in `section`.
    fn substitute(&self, raw: &str, section: &str, sv: Option<&dyn SectionValues>) -> String {
        if !raw.contains(['#', '%', '[']) {
            return raw.to_string();
        }
        let var = |n: &str| self.lookup_variable(n, Some(section));
        let s = subst::expand_env_percent(raw);
        let s = subst::expand_old_style(&s, &var);
        match sv {
            Some(sv) => {
                let sec = |n: &str| section_lookup(sv, n);
                let s = subst::expand_brackets(&s, Mode::AllKeys, &var, Some(&sec));
                subst::expand_old_measures(&s, &sec)
            }
            None => subst::expand_brackets(&s, Mode::HashOnly, &var, None),
        }
    }

    // ------------------------------------------------------------- options

    pub fn section_names(&self) -> Vec<String> {
        self.section_order
            .iter()
            .filter(|s| !SPECIAL_SECTIONS.contains(&up(s).as_str()))
            .cloned()
            .collect()
    }

    pub fn has_key(&self, section: &str, key: &str) -> bool {
        self.raw(section, key).is_some()
    }

    fn raw(&self, section: &str, key: &str) -> Option<&String> {
        self.values.get(&(up(section), up(key)))
    }

    /// `!SetOption`: overwrite (or add) a raw option value of an existing section.
    pub fn set_option(&mut self, section: &str, key: &str, value: &str) {
        if self.known_sections.contains(&up(section)) {
            if value.is_empty() {
                // An empty value deletes the option (style / default applies).
                self.values.remove(&(up(section), up(key)));
            } else {
                self.values.insert((up(section), up(key)), value.to_string());
            }
        }
    }

    /// Style sections for `section`: `@Inherit`, else `MeterStyle`, split on `|`.
    fn style_chain(&self, section: &str, sv: Option<&dyn SectionValues>) -> Vec<String> {
        let spec = ["@Inherit", "MeterStyle"]
            .iter()
            .filter_map(|k| self.raw(section, k))
            .map(|raw| self.substitute(raw, section, sv))
            .find(|s| !s.trim().is_empty());
        let Some(spec) = spec else { return Vec::new() };
        spec.split('|')
            .map(str::trim)
            .filter(|n| !n.is_empty() && self.known_sections.contains(&up(n)))
            .map(str::to_string)
            .collect()
    }

    /// Own value (an empty one counts and blocks styles), else the last listed
    /// style that defines the key. Styles are one level deep.
    fn raw_with_styles(&self, section: &str, key: &str, sv: Option<&dyn SectionValues>) -> Option<&String> {
        if let Some(v) = self.raw(section, key) {
            return Some(v);
        }
        if NON_INHERITABLE.contains(&up(key).as_str()) {
            return None;
        }
        self.style_chain(section, sv).iter().rev().find_map(|style| self.raw(style, key))
    }

    fn read_substituted(&self, section: &str, key: &str, sv: Option<&dyn SectionValues>) -> Option<String> {
        self.raw_with_styles(section, key, sv).map(|raw| self.substitute(raw, section, sv))
    }

    fn eval(&self, formula_text: &str, sv: Option<&dyn SectionValues>) -> Result<f64, String> {
        formula::evaluate(formula_text, &|n| sv.and_then(|s| s.measure_value(n)))
    }

    pub fn read_string(&self, section: &str, key: &str, default: &str, sv: Option<&dyn SectionValues>) -> String {
        self.read_substituted(section, key, sv).unwrap_or_else(|| default.to_string())
    }

    /// Raw option text with style fallback but no variable or section
    /// substitution (Rainmeter `ReadString(..., false)`), used for actions
    /// that are expanded when they execute.
    pub fn read_string_raw(&self, section: &str, key: &str, default: &str) -> String {
        self.raw_with_styles(section, key, None).cloned().unwrap_or_else(|| default.to_string())
    }

    pub fn read_float(&self, section: &str, key: &str, default: f64, sv: Option<&dyn SectionValues>) -> f64 {
        match self.read_substituted(section, key, sv) {
            None => default,
            Some(v) if v.is_empty() => default,
            Some(v) if v.starts_with('(') => self.eval(&v, sv).unwrap_or(default),
            Some(v) => numparse::parse_f64_prefix(&v),
        }
    }

    /// 32-bit `long` semantics: out-of-range values give the default.
    pub fn read_int(&self, section: &str, key: &str, default: i64, sv: Option<&dyn SectionValues>) -> i64 {
        match self.read_substituted(section, key, sv) {
            None => default,
            Some(v) if v.is_empty() => default,
            Some(v) if v.starts_with('(') => self.eval(&v, sv).map(|f| f as i64).unwrap_or(default),
            Some(v) => numparse::parse_i32_prefix(&v).unwrap_or(default),
        }
    }

    pub fn read_color(&self, section: &str, key: &str, default: Color, sv: Option<&dyn SectionValues>) -> Color {
        match self.read_substituted(section, key, sv) {
            Some(v) if !v.is_empty() => parse_color_with(&v, &|n| sv.and_then(|s| s.measure_value(n))).unwrap_or(default),
            _ => default,
        }
    }

    /// Substitute variables in `s` and evaluate it as a formula; `None` on error.
    pub fn parse_formula(&self, s: &str, sv: Option<&dyn SectionValues>) -> Option<f64> {
        let text = self.substitute(s, "", sv);
        self.eval(&text, sv).ok()
    }
}

/// Shortcut used by measures and meters to read options of one section.
pub struct OptionReader<'a> {
    pub parser: &'a ConfigParser,
    pub section: &'a str,
    pub sv: Option<&'a dyn SectionValues>,
}

impl<'a> OptionReader<'a> {
    pub fn new(parser: &'a ConfigParser, section: &'a str, sv: Option<&'a dyn SectionValues>) -> Self {
        OptionReader { parser, section, sv }
    }
    /// Same reader with `[Measure]` references left unexpanded (for actions).
    pub fn without_sections(&self) -> OptionReader<'a> {
        OptionReader { parser: self.parser, section: self.section, sv: None }
    }
    pub fn string(&self, key: &str, default: &str) -> String {
        self.parser.read_string(self.section, key, default, self.sv)
    }
    /// Action text, read raw: `#Var#` and `[Measure]` are expanded when it executes.
    pub fn action(&self, key: &str) -> String {
        self.parser.read_string_raw(self.section, key, "")
    }
    pub fn float(&self, key: &str, default: f64) -> f64 {
        self.parser.read_float(self.section, key, default, self.sv)
    }
    pub fn int(&self, key: &str, default: i64) -> i64 {
        self.parser.read_int(self.section, key, default, self.sv)
    }
    /// Rainmeter `ReadBool`: an integer option, non-zero is true.
    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.parser.read_int(self.section, key, i64::from(default), self.sv) != 0
    }
    pub fn color(&self, key: &str, default: Color) -> Color {
        self.parser.read_color(self.section, key, default, self.sv)
    }
    pub fn has(&self, key: &str) -> bool {
        self.parser.has_key(self.section, key)
    }
}

#[cfg(test)]
mod option_reader_tests {
    use super::*;

    fn parser(ini: &str) -> (tempfile::TempDir, ConfigParser) {
        let tmp = tempfile::tempdir().unwrap();
        let env = Environment::with_root(tmp.path());
        let dir = env.skins_dir.join("T");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Main.ini"), ini).unwrap();
        let id = SkinIdentity {
            config: "T".into(),
            root_config: "T".into(),
            file: "Main.ini".into(),
            ini_path: dir.join("Main.ini"),
        };
        let p = ConfigParser::load(&env, &id).unwrap();
        (tmp, p)
    }

    #[test]
    fn reads_typed_options_with_defaults() {
        let (_t, p) = parser(
            "[Variables]\nV=7\n[M]\nMeasure=Calc\nS=hello #V#\nF=(1+2)\nI=12\nB=1\nB0=0\nC=255,0,0\nEmpty=\n",
        );
        let o = OptionReader { parser: &p, section: "M", sv: None };
        assert_eq!(o.string("S", ""), "hello 7");
        assert_eq!(o.string("Missing", "dflt"), "dflt");
        assert_eq!(o.float("F", 0.0), 3.0);
        assert_eq!(o.float("Missing", 2.5), 2.5);
        assert_eq!(o.int("I", 0), 12);
        assert_eq!(o.int("Missing", -4), -4);
        assert!(o.bool("B", false));
        assert!(!o.bool("B0", true));
        assert!(o.bool("Missing", true));
        assert_eq!(o.color("C", Color::default()), Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(o.action("S"), "hello #V#");
        assert!(o.has("empty"));
        assert!(!o.has("Missing"));
    }

    #[test]
    fn sections_are_expanded_only_with_sv() {
        struct Sv;
        impl SectionValues for Sv {
            fn section_variable(&self, name: &str, _p: Option<&str>) -> Option<String> {
                (name.eq_ignore_ascii_case("Other")).then(|| "42".to_string())
            }
            fn measure_value(&self, _n: &str) -> Option<f64> {
                None
            }
        }
        let (_t, p) = parser("[M]\nText=[&Other]\n");
        let sv = Sv;
        let with = OptionReader::new(&p, "M", Some(&sv));
        assert_eq!(with.string("Text", ""), "42");
        assert_eq!(with.without_sections().string("Text", ""), "[&Other]");
    }
}
