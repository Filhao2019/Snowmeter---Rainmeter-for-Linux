//! A loaded skin: parser, measures, meters, the update loop, rendering,
//! mouse handling and bang execution.

pub mod actions;
pub mod mouse;
pub mod options;
pub(crate) mod script_host;

use crate::bangs::Action;
use crate::env::Environment;
use crate::measures::{Measure, MeasureCtx};
use crate::meters::{self, Meter, MeterInputs};
use crate::parser::{ConfigParser, OptionReader, SectionValues, SkinIdentity};
use crate::render::fonts;
use crate::render::image::{draw_image, process, to_surface, DrawParams};
use crate::render::Resources;
use options::SkinOptions;
use std::cell::{Ref, RefCell};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub use mouse::{MouseButton, MouseEvent};

/// Things a skin asks the application to do (window management, other skins).
#[derive(Clone, Debug, PartialEq)]
pub enum AppRequest {
    Activate { config: String, file: Option<String> },
    Deactivate { config: String },
    Toggle { config: String, file: Option<String> },
    /// `None` = every skin.
    Refresh { config: Option<String> },
    RefreshApp,
    Move { config: String, x: f64, y: f64 },
    SetDraggable { config: String, value: i32 },
    SetClickThrough { config: String, value: i32 },
    SetTransparency { config: String, alpha: u8 },
    ZPos { config: String, value: i32 },
    /// Run `actions` in another skin (`"*"` = all skins).
    BangOther { config: String, actions: String },
    OpenOrRun(String),
    /// The skin size changed; resize the window.
    Resize,
    Show { config: String },
    Hide { config: String },
    ToggleVisible { config: String },
    Quit,
}

pub(crate) struct MeasureEntry {
    pub name: String,
    pub m: RefCell<Measure>,
}

pub(crate) type Geom = (String, [f64; 4]);

/// Resolves `[Measure]`, `[Measure:sel]` and `[Meter:X]` for option reads and
/// formulas. A measure that is being updated (mutably borrowed) is invisible.
pub(crate) struct View<'a> {
    pub measures: &'a [MeasureEntry],
    pub geom: &'a [Geom],
}

impl View<'_> {
    pub fn find(&self, name: &str) -> Option<&MeasureEntry> {
        self.measures.iter().find(|e| e.name.eq_ignore_ascii_case(name))
    }
}

impl SectionValues for View<'_> {
    fn section_variable(&self, name: &str, param: Option<&str>) -> Option<String> {
        if let Some(e) = self.find(name) {
            return e.m.try_borrow().ok()?.section_variable_opt(param);
        }
        if let Some((prefix, suffix)) = name.rsplit_once(':') {
            if suffix.eq_ignore_ascii_case("maxvalue") || suffix.eq_ignore_ascii_case("minvalue") {
                if let Some(e) = self.find(prefix) {
                    let p = match param {
                        Some(sel) => format!("{suffix}:{sel}"),
                        None => suffix.to_string(),
                    };
                    return e.m.try_borrow().ok()?.section_variable_opt(Some(&p));
                }
            }
        }
        let (_, g) = self.geom.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?;
        let v = match param?.to_ascii_uppercase().as_str() {
            "X" => g[0],
            "Y" => g[1],
            "W" => g[2],
            "H" => g[3],
            "XW" => g[0] + g[2],
            "YH" => g[1] + g[3],
            _ => return None,
        };
        Some((v as i64).to_string())
    }

    fn measure_value(&self, name: &str) -> Option<f64> {
        self.find(name)?.m.try_borrow().ok().map(|m| m.value())
    }
}

pub(crate) struct Delayed {
    pub due: Instant,
    pub actions: Vec<Action>,
    pub section: Option<String>,
}

pub struct Skin {
    pub(crate) env: Environment,
    pub(crate) config: String,
    pub(crate) file: String,
    pub(crate) skin_dir: PathBuf,
    pub(crate) ini_path: PathBuf,
    pub(crate) parser: ConfigParser,
    pub(crate) options: SkinOptions,
    pub(crate) res: Resources,
    pub(crate) measures: Vec<MeasureEntry>,
    pub(crate) meters: Vec<Meter>,
    pub(crate) geom: Vec<Geom>,
    pub(crate) over: Vec<bool>,
    pub(crate) skin_over: bool,
    size: (i32, i32),
    counter: u64,
    requests: Vec<AppRequest>,
    pub(crate) delayed: Vec<Delayed>,
    frame: RefCell<Option<cairo::ImageSurface>>,
    dirty: bool,
    background: Option<cairo::ImageSurface>,
    messages: Vec<String>,
    loaded: bool,
}

pub(crate) fn normalize_config(s: &str) -> String {
    s.replace('/', "\\").trim_matches('\\').to_string()
}

fn first_ini(dir: &Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.to_ascii_lowercase().ends_with(".ini"))
        .collect();
    names.sort();
    names.into_iter().next()
}

impl Skin {
    /// Load `config` (e.g. `illustro\Clock`) / `file` (e.g. `Clock.ini`), build
    /// every section and run the first, forced update plus `OnRefreshAction`.
    pub fn load(env: &Environment, config: &str, file: &str) -> Result<Skin, String> {
        let config = normalize_config(config);
        let dir = crate::paths::resolve_path(&env.skins_dir, &config.replace('\\', "/"));
        let ini_path = crate::paths::resolve_path(&dir, file);
        if !ini_path.is_file() {
            return Err(format!("Skin file {} not found", ini_path.display()));
        }
        let skin_dir = ini_path.parent().map(Path::to_path_buf).unwrap_or(dir);
        let id = SkinIdentity {
            config: config.clone(),
            root_config: config.split('\\').next().unwrap_or("").to_string(),
            file: file.to_string(),
            ini_path: ini_path.clone(),
        };
        let parser = ConfigParser::load(env, &id).map_err(|e| format!("Unable to read {}: {e}", ini_path.display()))?;
        let options = SkinOptions::read(&parser);
        let resources_dir = parser.get_variable("@").map(PathBuf::from).unwrap_or_default();
        fonts::load_skin_fonts(&resources_dir, &skin_dir, &env.program_dir, &options.local_fonts);
        let res = Resources::new(skin_dir.clone(), options.accurate_text);
        let mut skin = Skin {
            env: env.clone(),
            config,
            file: file.to_string(),
            skin_dir,
            ini_path,
            parser,
            options,
            res,
            measures: Vec::new(),
            meters: Vec::new(),
            geom: Vec::new(),
            over: Vec::new(),
            skin_over: false,
            size: (0, 0),
            counter: 0,
            requests: Vec::new(),
            delayed: Vec::new(),
            frame: RefCell::new(None),
            dirty: true,
            background: None,
            messages: Vec::new(),
            loaded: false,
        };
        let pre: Vec<String> = skin.parser.log_messages().iter().cloned().chain(skin.options.messages.clone()).collect();
        for m in pre {
            skin.note(m);
        }
        skin.create_sections();
        if skin.meters.is_empty() {
            return Err(format!("{}: no meters were created", skin.config));
        }
        skin.load_background();
        skin.over = vec![false; skin.meters.len()];
        skin.update_cycle(true);
        skin.loaded = true;
        let on_refresh = skin.options.on_refresh.clone();
        skin.execute(&on_refresh);
        Ok(skin)
    }

    /// Record a diagnostic once (log crate + `log_messages`).
    pub(crate) fn note(&mut self, msg: impl Into<String>) {
        note_to(&mut self.messages, &self.config, msg.into());
    }

    pub fn log_messages(&self) -> &[String] {
        &self.messages
    }

    fn create_sections(&mut self) {
        for name in self.parser.section_names() {
            let measure_type = self.parser.replace_variables(&self.parser.read_string_raw(&name, "Measure", ""));
            let measure_type = measure_type.trim().to_string();
            if !measure_type.is_empty() {
                let made = {
                    let view = View { measures: &self.measures, geom: &self.geom };
                    let opt = OptionReader::new(&self.parser, &name, Some(&view));
                    Measure::new(&name, &measure_type, &opt)
                };
                let mut made = made;
                for l in made.take_log() {
                    self.note(format!("[{name}] {l}"));
                }
                if !made.known_type {
                    self.note(format!("Measure={measure_type} is not valid in [{name}]"));
                    continue;
                }
                self.measures.push(MeasureEntry { name: name.clone(), m: RefCell::new(made) });
                continue;
            }
            let meter_type = self.parser.replace_variables(&self.parser.read_string_raw(&name, "Meter", ""));
            let meter_type = meter_type.trim().to_string();
            if meter_type.is_empty() {
                continue;
            }
            match meters::create_impl(&meter_type) {
                Ok(imp) => {
                    let mut meter = Meter::new(&name, &meter_type, imp);
                    {
                        let view = View { measures: &self.measures, geom: &self.geom };
                        let opt = OptionReader::new(&self.parser, &name, Some(&view));
                        meter.read_options(&opt, self.options.default_update_divider);
                    }
                    let logs = std::mem::take(&mut meter.log);
                    for l in logs {
                        self.note(format!("[{name}] {l}"));
                    }
                    self.meters.push(meter);
                }
                Err(e) => self.note(format!("{e} in [{name}]")),
            }
        }
    }

    fn load_background(&mut self) {
        self.background = None;
        let o = &self.options;
        if !matches!(o.background_mode, 0 | 3 | 4) {
            return;
        }
        let name = format!("{}{}", o.image_path, o.background);
        let path = crate::paths::resolve_path(&self.skin_dir, &name);
        let mut log = Vec::new();
        let raw = self.res.images.load(&path, &mut log);
        let surf = raw.and_then(|r| to_surface(&process(&r, &o.background_image)));
        for l in log {
            self.note(l);
        }
        self.background = surf;
    }

    // ------------------------------------------------------------ accessors

    pub fn config(&self) -> &str {
        &self.config
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn ini_path(&self) -> &Path {
        &self.ini_path
    }

    pub fn parser(&self) -> &ConfigParser {
        &self.parser
    }

    pub fn options(&self) -> &SkinOptions {
        &self.options
    }

    pub fn groups(&self) -> &[String] {
        &self.options.groups
    }

    pub fn size(&self) -> (i32, i32) {
        self.size
    }

    /// `[Rainmeter] Update` in ms; negative = no timer.
    pub fn update_interval_ms(&self) -> i64 {
        self.options.update_ms
    }

    /// Names of the meters in drawing order.
    pub fn meter_names(&self) -> Vec<String> {
        self.meters.iter().map(|m| m.base.name.clone()).collect()
    }

    pub fn meter_base(&self, name: &str) -> Option<&meters::MeterBase> {
        self.meters.iter().find(|m| m.base.name.eq_ignore_ascii_case(name)).map(|m| &m.base)
    }

    /// The text a String meter currently shows.
    pub fn meter_text(&self, name: &str) -> Option<String> {
        self.meters.iter().find(|m| m.base.name.eq_ignore_ascii_case(name))?.imp.text().map(str::to_string)
    }

    pub fn measure_value(&self, name: &str) -> Option<f64> {
        View { measures: &self.measures, geom: &self.geom }.measure_value(name)
    }

    pub fn take_requests(&mut self) -> Vec<AppRequest> {
        std::mem::take(&mut self.requests)
    }

    pub(crate) fn push_request(&mut self, r: AppRequest) {
        self.requests.push(r);
    }

    /// True once after anything changed that needs a redraw.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::replace(&mut self.dirty, false)
    }

    pub(crate) fn invalidate(&mut self) {
        self.dirty = true;
        *self.frame.borrow_mut() = None;
    }

    // ------------------------------------------------------------ update loop

    /// One Update cycle (skin-format.md 8.2).
    pub fn tick(&mut self) {
        self.run_delayed();
        self.update_cycle(false);
    }

    /// Milliseconds until the next `!Delay`ed action is due.
    pub fn next_delay_ms(&self) -> Option<u64> {
        let now = Instant::now();
        self.delayed.iter().map(|d| d.due.saturating_duration_since(now).as_millis() as u64).min()
    }

    pub(crate) fn update_cycle(&mut self, force: bool) {
        self.counter += 1;
        for i in 0..self.measures.len() {
            let (acts, name) = self.step_measure(i, force);
            for a in acts {
                self.execute_in(&a, Some(&name));
            }
        }
        let mut meter_actions = Vec::new();
        for i in 0..self.meters.len() {
            if self.step_meter(i, force) {
                let m = &self.meters[i].base;
                if !m.on_update.trim().is_empty() {
                    meter_actions.push((m.on_update.clone(), m.name.clone()));
                }
            }
        }
        self.layout();
        if force || self.options.dynamic_window_size {
            self.recompute_size();
        }
        for (a, name) in meter_actions {
            self.execute_in(&a, Some(&name));
        }
        let on_update = self.options.on_update.clone();
        self.execute(&on_update);
        self.invalidate();
    }

    /// Update one measure; returns its pending actions and name.
    pub(crate) fn step_measure(&mut self, i: usize, force: bool) -> (Vec<String>, String) {
        let size = self.size;
        let Skin { parser, measures, geom, env, skin_dir, options, counter, messages, config, .. } = self;
        let view = View { measures, geom };
        let host = script_host::SkinHost { parser, view: &view, size, skin_dir: skin_dir.clone() };
        let entry = &measures[i];
        let name = entry.name.clone();
        let opt = OptionReader::new(parser, &name, Some(&view));
        let mut m = entry.m.borrow_mut();
        if !force && m.wants_reread() {
            m.read_options(&opt);
        }
        let mut log = Vec::new();
        let lookup = |n: &str| view.measure_value(n);
        let mut ctx = MeasureCtx { env, skin_dir, update_ms: options.update_ms, log: &mut log, lookup: &lookup, counter: *counter, host: Some(&host) };
        if force {
            m.force_update(&mut ctx, Some(&opt));
        } else {
            m.update(&mut ctx, Some(&opt));
        }
        let acts = m.take_actions();
        let mut all = log;
        all.extend(m.take_log());
        for l in all {
            note_to(messages, config, format!("[{name}] {l}"));
        }
        (acts, name)
    }

    /// Update one meter (divider, DynamicVariables re-read, measure binding).
    /// Returns true if it updated.
    pub(crate) fn step_meter(&mut self, i: usize, force: bool) -> bool {
        let Skin { parser, measures, geom, res, options, meters, messages, config, .. } = self;
        let m = &mut meters[i];
        if m.base.divider.divider < 0 && !force {
            return false;
        }
        if force {
            m.base.divider.force();
        }
        let view = View { measures, geom };
        let name = m.base.name.clone();
        if m.base.dynamic_variables && m.base.divider.will_fire() {
            let opt = OptionReader::new(parser, &name, Some(&view));
            m.read_options(&opt, options.default_update_divider);
        }
        if !m.base.divider.tick() {
            return false;
        }
        let guards: Vec<Ref<Measure>> = m
            .base
            .measure_names
            .iter()
            .map_while(|n| view.find(n))
            .filter_map(|e| e.m.try_borrow().ok())
            .collect();
        if guards.len() < m.base.measure_names.len() {
            let bad = &m.base.measure_names[guards.len()];
            note_to(messages, config, format!("[{name}] MeasureName={bad} is not valid"));
        }
        let log = RefCell::new(Vec::new());
        {
            let b = &m.base;
            let size = (
                b.w_given.map(|v| v + b.pad_w()).unwrap_or(b.w as i32),
                b.h_given.map(|v| v + b.pad_h()).unwrap_or(b.h as i32),
            );
            let inputs = MeterInputs { measures: guards.iter().map(|g| &**g).collect(), res, log: &log, size, pad: (b.pad_w(), b.pad_h()) };
            m.update(&inputs);
        }
        drop(guards);
        let mut all = log.into_inner();
        all.extend(std::mem::take(&mut m.log));
        for l in all {
            note_to(messages, config, format!("[{name}] {l}"));
        }
        true
    }

    /// Relative positioning (`r`/`R`) in file order, then the drawn rects.
    pub(crate) fn layout(&mut self) {
        let mut prev: Option<(f64, f64, f64, f64)> = None;
        for m in &mut self.meters {
            let b = &mut m.base;
            let pick = |rel: meters::base::Rel, value: i32, p: Option<(f64, f64)>, pw: f64| match (rel, p) {
                (meters::base::Rel::TopLeft, Some((px, _))) => px + value as f64,
                (meters::base::Rel::BottomRight, Some((px, _))) => px + pw + value as f64,
                _ => value as f64,
            };
            let px = prev.map(|(x, y, _, _)| (x, y));
            b.x = pick(b.x_rel, b.x_value, px, prev.map(|p| p.2).unwrap_or(0.0));
            let py = prev.map(|(_, y, _, _)| (y, 0.0));
            b.y = pick(b.y_rel, b.y_value, py, prev.map(|p| p.3).unwrap_or(0.0));
            m.update_rect();
            let b = &m.base;
            prev = Some((b.x, b.y, b.gw(), b.gh()));
        }
        self.geom = self
            .meters
            .iter()
            .map(|m| (m.base.name.clone(), [m.base.rect_x, m.base.rect_y, m.base.w, m.base.h]))
            .collect();
    }

    /// Window size from the meters (meters.md 1.11).
    pub(crate) fn recompute_size(&mut self) {
        let o = &self.options;
        let m = o.background_margins;
        let (mut w, mut h) = (m[0] as f64, m[1] as f64);
        for me in &self.meters {
            w = w.max(me.base.rect_x + me.base.gw());
            h = h.max(me.base.rect_y + me.base.gh());
        }
        w += m[2] as f64;
        h += m[3] as f64;
        if let Some(bg) = &self.background {
            let (bw, bh) = (bg.width() as f64, bg.height() as f64);
            if o.background_mode == 0 {
                (w, h) = (bw, bh);
            } else {
                w = w.max(bw);
                h = h.max(bh);
            }
        }
        if o.skin_width > 0 {
            w = o.skin_width as f64;
        }
        if o.skin_height > 0 {
            h = o.skin_height as f64;
        }
        let new = (w as i32, h as i32);
        if new != self.size {
            self.size = new;
            if self.loaded {
                self.requests.push(AppRequest::Resize);
            }
        }
        self.parser.set_window_geometry(0.0, 0.0, new.0 as f64, new.1 as f64);
    }

    // ------------------------------------------------------------- rendering

    pub fn render(&self, cr: &cairo::Context) {
        let (w, h) = self.size;
        if w <= 0 || h <= 0 {
            return;
        }
        let o = &self.options;
        match (o.background_mode, &self.background) {
            (2, _) => meters::base::paint_background(cr, 0.0, 0.0, w as f64, h as f64, o.solid_color, o.solid_color2, o.gradient_angle),
            (0, Some(bg)) => draw_image(cr, bg, 0.0, 0.0, w as f64, h as f64, &DrawParams::default()),
            (3, Some(bg)) => {
                draw_image(cr, bg, 0.0, 0.0, w as f64, h as f64, &DrawParams { margins: o.background_margins, ..Default::default() })
            }
            (4, Some(bg)) => draw_image(cr, bg, 0.0, 0.0, w as f64, h as f64, &DrawParams { tile: true, ..Default::default() }),
            _ => {}
        }
        for m in &self.meters {
            m.draw(cr, &self.res);
        }
    }

    /// Render a fresh ARGB32 frame of `size()`.
    pub fn render_to_surface(&self) -> cairo::ImageSurface {
        let (w, h) = self.size;
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w.max(1), h.max(1)).expect("surface");
        if let Ok(cr) = cairo::Context::new(&surf) {
            self.render(&cr);
        }
        surf
    }

    pub(crate) fn with_frame<R>(&self, f: impl FnOnce(&mut cairo::ImageSurface) -> R) -> R {
        let mut slot = self.frame.borrow_mut();
        let surf = slot.get_or_insert_with(|| self.render_to_surface());
        f(surf)
    }
}

pub(crate) fn note_to(messages: &mut Vec<String>, config: &str, msg: String) {
    if messages.contains(&msg) {
        return;
    }
    log::warn!("[{config}] {msg}");
    messages.push(msg);
}

impl SectionValues for Skin {
    fn section_variable(&self, name: &str, param: Option<&str>) -> Option<String> {
        View { measures: &self.measures, geom: &self.geom }.section_variable(name, param)
    }

    fn measure_value(&self, name: &str) -> Option<f64> {
        View { measures: &self.measures, geom: &self.geom }.measure_value(name)
    }
}

/// Load `config`/`file` (default: the first `.ini` in the folder), tick
/// `ticks` times and write the frame as PNG.
pub fn render_skin_png(env: &Environment, config: &str, file: Option<&str>, ticks: u32, out: &Path) -> Result<(), String> {
    let file = match file {
        Some(f) => f.to_string(),
        None => {
            let dir = crate::paths::resolve_path(&env.skins_dir, &normalize_config(config).replace('\\', "/"));
            first_ini(&dir).ok_or_else(|| format!("No .ini file found for {config}"))?
        }
    };
    let mut skin = Skin::load(env, config, &file)?;
    for _ in 0..ticks {
        skin.tick();
    }
    let (w, h) = skin.size();
    if w <= 0 || h <= 0 {
        return Err(format!("{config}: skin size is {w}x{h}, nothing to render"));
    }
    let surf = skin.render_to_surface();
    let mut f = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    surf.write_to_png(&mut f).map_err(|e| format!("PNG write failed: {e}"))
}
