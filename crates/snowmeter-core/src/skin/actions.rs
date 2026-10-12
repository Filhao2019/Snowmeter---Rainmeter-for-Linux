//! Executing action strings and bangs (skin-format.md section 10).
//!
//! Skin-local bangs are applied directly; everything that concerns the
//! window or other skins becomes an `AppRequest`.

use super::{normalize_config, AppRequest, Delayed, Skin, View};
use crate::bangs::{parse_actions, Action};
use crate::meters::base::Rel;
use crate::parser::{OptionReader, SectionValues};
use std::time::{Duration, Instant};

const MAX_DEPTH: usize = 8;

fn flag_arg(s: &str) -> i32 {
    crate::numparse::parse_i32_prefix(s.trim()).unwrap_or(0) as i32
}

fn quote_arg(a: &str) -> String {
    if a.contains('"') {
        format!("\"\"\"{a}\"\"\"")
    } else {
        format!("\"{a}\"")
    }
}

/// `%.5f` with trailing zeros removed (the `!SetVariable` formula format).
fn format_number(v: f64) -> String {
    let mut s = format!("{v:.5}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// Number of fixed arguments of the bangs that accept a trailing `[config]`.
fn routable_argc(name: &str) -> Option<usize> {
    Some(match name {
        "redraw" | "update" => 0,
        "showmeter" | "hidemeter" | "togglemeter" | "showmetergroup" | "hidemetergroup" | "togglemetergroup" => 1,
        "enablemeasure" | "disablemeasure" | "togglemeasure" | "enablemeasuregroup" | "disablemeasuregroup"
        | "togglemeasuregroup" | "pausemeasure" | "unpausemeasure" | "togglepausemeasure" | "pausemeasuregroup"
        | "unpausemeasuregroup" | "togglepausemeasuregroup" => 1,
        "updatemeasure" | "updatemeter" | "updatemeasuregroup" | "updatemetergroup" => 1,
        "setvariable" | "commandmeasure" | "commandmeasuregroup" => 2,
        "setoption" | "setoptiongroup" | "movemeter" => 3,
        _ => return None,
    })
}

impl Skin {
    /// Run an action string with no current section.
    pub fn execute(&mut self, actions: &str) {
        self.execute_in(actions, None);
    }

    /// Run an action string; `section` is the measure/meter it belongs to.
    pub(crate) fn execute_in(&mut self, raw: &str, section: Option<&str>) {
        if raw.trim().is_empty() {
            return;
        }
        let acts = self.parse_expanded(raw, section);
        self.run_actions(acts, section.map(str::to_string), 0);
    }

    fn parse_expanded(&self, raw: &str, section: Option<&str>) -> Vec<Action> {
        let mut s = raw.to_string();
        if let Some(sec) = section {
            s = replace_ci(&s, "#CURRENTSECTION#", sec);
            s = replace_ci(&s, "[#CURRENTSECTION]", sec);
        }
        parse_actions(&self.parser.replace_variables(&s))
    }

    fn expand_section_vars(&self, s: &str) -> String {
        let view = View { measures: &self.measures, geom: &self.geom };
        self.parser.replace_section_variables(s, &view)
    }

    pub(crate) fn run_delayed(&mut self) {
        let now = Instant::now();
        let (due, rest): (Vec<Delayed>, Vec<Delayed>) = std::mem::take(&mut self.delayed).into_iter().partition(|d| d.due <= now);
        self.delayed = rest;
        for d in due {
            self.run_actions(d.actions, d.section, 0);
        }
    }

    fn run_actions(&mut self, actions: Vec<Action>, section: Option<String>, depth: usize) {
        let mut it = actions.into_iter();
        while let Some(a) = it.next() {
            match a {
                Action::Bang { name, args } if name == "delay" => {
                    let ms = args.first().and_then(|a| self.parse_num(&self.expand_section_vars(a))).unwrap_or(0.0).max(0.0);
                    let rest: Vec<Action> = it.collect();
                    if !rest.is_empty() {
                        self.delayed.push(Delayed {
                            due: Instant::now() + Duration::from_millis(ms as u64),
                            actions: rest,
                            section,
                        });
                    }
                    return;
                }
                Action::Bang { name, args } => {
                    let args: Vec<String> = args.iter().map(|a| self.expand_section_vars(a)).collect();
                    self.run_bang(&name, args, section.as_deref());
                }
                Action::Command(c) => self.run_command(&c, section.as_deref(), depth),
            }
        }
    }

    fn run_command(&mut self, c: &str, section: Option<&str>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut text = c.trim().to_string();
        if self.find_measure(&text).is_some() {
            text = format!("[{text}]");
        }
        let expanded = self.expand_section_vars(&text);
        if expanded != text && (expanded.trim_start().starts_with(['[', '!'])) {
            let acts = parse_actions(&expanded);
            self.run_actions(acts, section.map(str::to_string), depth + 1);
        } else if !expanded.trim().is_empty() {
            self.push_request(AppRequest::OpenOrRun(expanded));
        }
    }

    // ---------------------------------------------------------------- helpers

    pub(crate) fn find_measure(&self, name: &str) -> Option<usize> {
        self.measures.iter().position(|e| e.name.eq_ignore_ascii_case(name.trim()))
    }

    pub(crate) fn find_meter(&self, name: &str) -> Option<usize> {
        self.meters.iter().position(|m| m.base.name.eq_ignore_ascii_case(name.trim()))
    }

    fn parse_num(&self, s: &str) -> Option<f64> {
        let t = s.trim();
        if t.is_empty() {
            return None;
        }
        let view = View { measures: &self.measures, geom: &self.geom };
        if t.starts_with('(') {
            self.parser.parse_formula(t, Some(&view as &dyn SectionValues))
        } else {
            let v = crate::numparse::parse_f64_prefix(t);
            (t.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == '.')).then_some(v)
        }
    }

    /// `(formula)` values become `%.5f`-trimmed numbers; others are kept.
    fn eval_value(&self, v: &str) -> String {
        let t = v.trim();
        if t.starts_with('(') {
            let view = View { measures: &self.measures, geom: &self.geom };
            if let Some(n) = self.parser.parse_formula(t, Some(&view as &dyn SectionValues)) {
                return format_number(n);
            }
        }
        v.to_string()
    }

    fn measure_targets(&self, spec: &str, group: bool) -> Vec<usize> {
        if group {
            return (0..self.measures.len()).filter(|&i| self.measures[i].m.borrow().in_group(spec)).collect();
        }
        if spec.trim() == "*" {
            return (0..self.measures.len()).collect();
        }
        self.find_measure(spec).into_iter().collect()
    }

    fn meter_targets(&self, spec: &str, group: bool) -> Vec<usize> {
        if group {
            return (0..self.meters.len()).filter(|&i| self.meters[i].base.in_group(spec)).collect();
        }
        if spec.trim() == "*" {
            return (0..self.meters.len()).collect();
        }
        self.find_meter(spec).into_iter().collect()
    }

    fn is_self(&self, cfg: &str) -> bool {
        normalize_config(cfg).eq_ignore_ascii_case(&self.config)
    }

    fn reread_section(&mut self, name: &str) {
        if name.eq_ignore_ascii_case("rainmeter") {
            self.options = super::options::SkinOptions::read(&self.parser);
            return;
        }
        if let Some(i) = self.find_measure(name) {
            let view = View { measures: &self.measures, geom: &self.geom };
            let opt = OptionReader::new(&self.parser, &self.measures[i].name, Some(&view));
            self.measures[i].m.borrow_mut().read_options(&opt);
        } else if let Some(i) = self.find_meter(name) {
            let Skin { parser, measures, geom, meters, options, .. } = self;
            let view = View { measures, geom };
            let mname = meters[i].base.name.clone();
            let opt = OptionReader::new(parser, &mname, Some(&view));
            meters[i].read_options(&opt, options.default_update_divider);
            self.layout();
            self.recompute_size();
        }
    }

    fn set_meter_hidden(&mut self, i: usize, hidden: Option<bool>) {
        let new = hidden.unwrap_or(!self.meters[i].base.hidden);
        self.meters[i].base.hidden = new;
        let name = self.meters[i].base.name.clone();
        self.parser.set_option(&name, "Hidden", if new { "1" } else { "0" });
    }

    fn after_layout_change(&mut self) {
        self.layout();
        self.recompute_size();
        self.invalidate();
    }

    fn write_key_value(&mut self, args: &[String]) {
        let [section, key, value] = [&args[0], &args[1], &args[2]];
        let value = self.eval_value(value);
        let path = match args.get(3).filter(|f| !f.trim().is_empty()) {
            Some(f) => {
                if f.contains("../") || f.contains("..\\") {
                    self.note(format!("!WriteKeyValue: {f} must not contain .."));
                    return;
                }
                crate::paths::resolve_path(&self.skin_dir, f)
            }
            None => self.ini_path.clone(),
        };
        let under = |root: &std::path::Path| path.starts_with(root);
        if !under(&self.env.skins_dir) && !under(&self.env.settings_dir) {
            self.note(format!("!WriteKeyValue: {} is outside the skins and settings folders", path.display()));
            return;
        }
        if !path.is_file() {
            self.note(format!("!WriteKeyValue: {} does not exist", path.display()));
            return;
        }
        if let Err(e) = crate::ini::write_key_value(&path, section, key, &value) {
            self.note(format!("!WriteKeyValue: {}: {e}", path.display()));
        }
    }

    /// `!SetWindowPosition` coordinate (skin-format.md 11.3).
    fn window_coord(&self, spec: &str, anchor: &str, origin: f64, extent: f64, size: f64, vertical: bool) -> f64 {
        let spec = spec.trim();
        let (num_part, mods) = if spec.starts_with('(') {
            let mut depth = 0;
            let mut end = spec.len();
            for (i, c) in spec.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            (spec[..end].to_string(), &spec[end..])
        } else {
            let end = spec.find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.')).unwrap_or(spec.len());
            (spec[..end].to_string(), &spec[end..])
        };
        let v = self.parse_num(&num_part).unwrap_or(0.0);
        let percent = mods.contains('%');
        let opposite = mods.contains(if vertical { 'B' } else { 'R' });
        let off = if percent { extent * v / 100.0 } else { v };
        let a = anchor.trim();
        let anum_end = a.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(a.len());
        let av: f64 = a[..anum_end].parse().unwrap_or(0.0);
        let amods = &a[anum_end..];
        let apx = if amods.contains('%') { size * av / 100.0 } else { av };
        let apx = if amods.contains(if vertical { 'B' } else { 'R' }) { size - apx } else { apx };
        origin + if opposite { extent - off } else { off } - apx
    }

    fn set_window_position(&mut self, args: &[String]) {
        let (x, y) = (args.first().map(String::as_str).unwrap_or("0"), args.get(1).map(String::as_str).unwrap_or("0"));
        let (ax, ay) = (args.get(2).map(String::as_str).unwrap_or("0"), args.get(3).map(String::as_str).unwrap_or("0"));
        let mon = |spec: &str| -> usize {
            spec.rfind('@').and_then(|p| spec[p + 1..].trim().parse::<usize>().ok()).map(|n| n.max(1) - 1).unwrap_or(0)
        };
        let idx = mon(x).max(mon(y));
        let r = self.env.monitors.get(idx).copied().or_else(|| self.env.monitors.first().copied()).unwrap_or_default();
        let (w, h) = self.size();
        let px = self.window_coord(x.split('@').next().unwrap_or(x), ax, r.x, r.w, w as f64, false);
        let py = self.window_coord(y.split('@').next().unwrap_or(y), ay, r.y, r.h, h as f64, true);
        self.push_request(AppRequest::Move { config: self.config.clone(), x: px, y: py });
    }

    // ------------------------------------------------------------------ bangs

    pub(crate) fn run_bang(&mut self, name: &str, mut args: Vec<String>, section: Option<&str>) {
        let own = self.config.clone();
        // Bangs that address a skin through a trailing config argument.
        if let Some(argc) = routable_argc(name) {
            if name == "commandmeasure" && args.len() == 1 {
                // Legacy single-argument form: "Measure Command".
                if let Some((m, c)) = args[0].clone().split_once(' ') {
                    args = vec![m.to_string(), c.to_string()];
                }
            }
            if args.len() > argc {
                let cfg = args[argc].trim().to_string();
                if cfg.is_empty() || cfg == "*" || !self.is_self(&cfg) {
                    let fixed: Vec<String> = args[..argc].iter().map(|a| quote_arg(a)).collect();
                    let actions = format!("[!{name} {}]", fixed.join(" "));
                    self.push_request(AppRequest::BangOther { config: if cfg.is_empty() { "*".into() } else { cfg }, actions });
                    return;
                }
                args.truncate(argc);
            }
            if args.len() < argc {
                self.note(format!("Incorrect number of arguments for !{name}"));
                return;
            }
        }
        let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
        // App-level bangs: the config argument (when present) names the target.
        let target = |i: usize, args: &Vec<String>| -> String {
            args.get(i).map(|s| s.trim()).filter(|s| !s.is_empty()).map(normalize_config).unwrap_or_else(|| own.clone())
        };
        match name {
            "refresh" => {
                let t = target(0, &args);
                self.push_request(AppRequest::Refresh { config: if t == "*" { None } else { Some(t) } });
            }
            "refreshapp" => self.push_request(AppRequest::RefreshApp),
            "redraw" => self.invalidate(),
            "update" => {
                self.update_cycle(false);
            }
            "updatemeasure" | "updatemeasuregroup" => {
                for i in self.measure_targets(arg(0), name.ends_with("group")) {
                    let (acts, n) = self.step_measure(i, true);
                    for a in acts {
                        self.execute_in(&a, Some(&n));
                    }
                }
                self.invalidate();
            }
            "updatemeter" | "updatemetergroup" => {
                for i in self.meter_targets(arg(0), name.ends_with("group")) {
                    self.step_meter(i, true);
                }
                self.after_layout_change();
            }
            "setvariable" => {
                let v = self.eval_value(arg(1));
                self.parser.set_variable(arg(0), &v);
            }
            "setvariablegroup" => {
                let g = arg(2).to_string();
                if self.options.groups.iter().any(|x| x.eq_ignore_ascii_case(&g)) {
                    let v = self.eval_value(arg(1));
                    self.parser.set_variable(arg(0), &v);
                } else {
                    self.push_request(AppRequest::BangOther {
                        config: format!("Group={g}"),
                        actions: format!("[!SetVariable {} {}]", quote_arg(arg(0)), quote_arg(arg(1))),
                    });
                }
            }
            "setoption" | "setoptiongroup" => {
                let group = name.ends_with("group");
                let (sec, key, val) = if group { (None, arg(1), arg(2)) } else { (Some(arg(0)), arg(1), arg(2)) };
                let mut names = Vec::new();
                match sec {
                    Some(s) => names.push(s.to_string()),
                    None => {
                        for i in self.measure_targets(arg(0), true) {
                            names.push(self.measures[i].name.clone());
                        }
                        for i in self.meter_targets(arg(0), true) {
                            names.push(self.meters[i].base.name.clone());
                        }
                    }
                }
                for n in names {
                    self.parser.set_option(&n, key, val);
                    self.reread_section(&n);
                }
                self.invalidate();
            }
            "writekeyvalue" if args.len() >= 3 => self.write_key_value(&args),
            "showmeter" | "hidemeter" | "togglemeter" | "showmetergroup" | "hidemetergroup" | "togglemetergroup" => {
                let hide = match name.trim_end_matches("group") {
                    "showmeter" => Some(false),
                    "hidemeter" => Some(true),
                    _ => None,
                };
                let targets = self.meter_targets(arg(0), name.ends_with("group"));
                if targets.is_empty() {
                    self.note(format!("!{name}: [{}] not found", arg(0)));
                }
                for i in targets {
                    self.set_meter_hidden(i, hide);
                }
                self.after_layout_change();
            }
            "enablemeasure" | "disablemeasure" | "togglemeasure" | "enablemeasuregroup" | "disablemeasuregroup"
            | "togglemeasuregroup" | "pausemeasure" | "unpausemeasure" | "togglepausemeasure" | "pausemeasuregroup"
            | "unpausemeasuregroup" | "togglepausemeasuregroup" => {
                let base = name.trim_end_matches("group");
                let targets = self.measure_targets(arg(0), name.ends_with("group"));
                if targets.is_empty() {
                    self.note(format!("!{name}: [{}] not found", arg(0)));
                }
                for i in targets {
                    let n = self.measures[i].name.clone();
                    let mut m = self.measures[i].m.borrow_mut();
                    let (key, val) = match base {
                        "enablemeasure" => ("Disabled", false),
                        "disablemeasure" => ("Disabled", true),
                        "togglemeasure" => ("Disabled", !m.is_disabled()),
                        "pausemeasure" => ("Paused", true),
                        "unpausemeasure" => ("Paused", false),
                        _ => ("Paused", !m.is_paused()),
                    };
                    if key == "Disabled" {
                        m.set_disabled(val);
                    } else {
                        m.set_paused(val);
                    }
                    drop(m);
                    self.parser.set_option(&n, key, if val { "1" } else { "0" });
                }
            }
            "commandmeasure" | "commandmeasuregroup" | "pluginbang" => {
                let (m, cmd) = if name == "pluginbang" {
                    let t = arg(0).trim_matches('"');
                    t.split_once(' ').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((t.to_string(), String::new()))
                } else {
                    (arg(0).to_string(), arg(1).to_string())
                };
                let mut queued = Vec::new();
                for i in self.measure_targets(&m, name.ends_with("group")) {
                    let view = super::View { measures: &self.measures, geom: &self.geom };
                    let host = super::script_host::SkinHost { parser: &self.parser, view: &view, size: self.size(), skin_dir: self.skin_dir.clone() };
                    let mut ms = self.measures[i].m.borrow_mut();
                    ms.command_host(&cmd, Some(&host));
                    let acts = ms.take_actions();
                    if !acts.is_empty() {
                        queued.push((self.measures[i].name.clone(), acts));
                    }
                }
                for (n, acts) in queued {
                    for a in acts {
                        self.execute_in(&a, Some(&n));
                    }
                }
            }
            "movemeter" => {
                if let Some(i) = self.find_meter(arg(2)) {
                    let (nx, ny) = (self.parse_num_static(arg(0)) as i32, self.parse_num_static(arg(1)) as i32);
                    let b = &mut self.meters[i].base;
                    b.x_value = nx;
                    b.y_value = ny;
                    b.x_rel = Rel::Abs;
                    b.y_rel = Rel::Abs;
                    let n = b.name.clone();
                    self.parser.set_option(&n, "X", arg(0));
                    self.parser.set_option(&n, "Y", arg(1));
                    self.after_layout_change();
                }
            }
            "activateconfig" => self.push_request(AppRequest::Activate {
                config: normalize_config(arg(0)),
                file: Some(arg(1).to_string()).filter(|f| !f.is_empty()),
            }),
            "deactivateconfig" => {
                let t = target(0, &args);
                self.push_request(AppRequest::Deactivate { config: t });
            }
            "toggleconfig" => self.push_request(AppRequest::Toggle {
                config: normalize_config(arg(0)),
                file: Some(arg(1).to_string()).filter(|f| !f.is_empty()),
            }),
            "move" => {
                let t = target(2, &args);
                let (x, y) = (self.parse_num(arg(0)).unwrap_or(0.0), self.parse_num(arg(1)).unwrap_or(0.0));
                self.push_request(AppRequest::Move { config: t, x, y });
            }
            "setwindowposition" => self.set_window_position(&args),
            "zpos" | "changezpos" => {
                let t = target(1, &args);
                self.push_request(AppRequest::ZPos { config: t, value: flag_arg(arg(0)).clamp(-2, 2) });
            }
            "draggable" => {
                let t = target(1, &args);
                self.push_request(AppRequest::SetDraggable { config: t, value: flag_arg(arg(0)) });
            }
            "clickthrough" => {
                let t = target(1, &args);
                self.push_request(AppRequest::SetClickThrough { config: t, value: flag_arg(arg(0)) });
            }
            "settransparency" => {
                let t = target(1, &args);
                self.push_request(AppRequest::SetTransparency { config: t, alpha: flag_arg(arg(0)).clamp(0, 255) as u8 });
            }
            "show" | "showfade" => {
                let t = target(0, &args);
                self.push_request(AppRequest::Show { config: t });
            }
            "hide" | "hidefade" => {
                let t = target(0, &args);
                self.push_request(AppRequest::Hide { config: t });
            }
            "toggle" | "togglefade" => {
                let t = target(0, &args);
                self.push_request(AppRequest::ToggleVisible { config: t });
            }
            "log" => {
                let ty = arg(1).to_ascii_lowercase();
                let who = section.unwrap_or("");
                match ty.as_str() {
                    "error" => log::error!("[{own}] {who} {}", arg(0)),
                    "warning" => log::warn!("[{own}] {who} {}", arg(0)),
                    "debug" => log::debug!("[{own}] {who} {}", arg(0)),
                    _ => log::info!("[{own}] {who} {}", arg(0)),
                }
            }
            "quit" => self.push_request(AppRequest::Quit),
            "about" | "manage" | "debug" | "skinmenu" | "traymenu" | "skincustommenu" | "editskin" | "resetstats"
            | "lsboxhook" | "snapedges" | "keeponscreen" | "autoselectscreen" | "setclip" | "setwallpaper"
            | "loadlayout" | "showblur" | "hideblur" | "toggleblur" | "addblur" | "removeblur" | "fadeduration" => {
                log::debug!("[{own}] !{name} is not supported on Linux (ignored)");
            }
            _ => {
                self.note(format!("Unknown bang !{name}"));
            }
        }
    }

    fn parse_num_static(&self, s: &str) -> f64 {
        self.parse_num(s).unwrap_or(0.0)
    }
}

fn replace_ci(s: &str, pat: &str, rep: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let lp = pat.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(p) = lower[i..].find(&lp) {
        out.push_str(&s[i..i + p]);
        out.push_str(rep);
        i += p + pat.len();
    }
    out.push_str(&s[i..]);
    out
}
