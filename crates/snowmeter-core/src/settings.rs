//! `snowmeter.ini`: global `[Snowmeter]` keys and one section per skin config
//! (`[illustro\Clock]`) in the same shape as Rainmeter.ini (`rmskin-format.md` 4).
//! Keys are case-insensitive and keys this module does not know are preserved.

use crate::env::Environment;
use crate::ini::IniFile;
use std::io;
use std::path::{Path, PathBuf};

/// Sections that are not skin configs.
const RESERVED: [&str; 3] = ["rainmeter", "snowmeter", "traymeasure"];

pub fn is_reserved_section(name: &str) -> bool {
    RESERVED.iter().any(|r| name.eq_ignore_ascii_case(r))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowType {
    Desktop,
    Below,
    Normal,
}

impl WindowType {
    pub fn parse(s: &str) -> Option<WindowType> {
        match s.trim().to_ascii_lowercase().as_str() {
            "desktop" => Some(WindowType::Desktop),
            "below" => Some(WindowType::Below),
            "normal" => Some(WindowType::Normal),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            WindowType::Desktop => "desktop",
            WindowType::Below => "below",
            WindowType::Normal => "normal",
        }
    }
}

/// Per-config state. `active` is the 1-based index of the `.ini` file in the
/// config folder (registry order); 0 means unloaded.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigState {
    pub active: u32,
    pub window_x: String,
    pub window_y: String,
    pub draggable: bool,
    pub click_through: bool,
    pub keep_on_screen: bool,
    pub save_position: bool,
    pub alpha_value: u8,
    pub load_order: i32,
    /// -2 desktop, -1 bottom, 0 normal, 1 top, 2 topmost.
    pub always_on_top: i32,
}

impl Default for ConfigState {
    fn default() -> Self {
        ConfigState {
            active: 0,
            window_x: "0".into(),
            window_y: "0".into(),
            draggable: true,
            click_through: false,
            keep_on_screen: true,
            save_position: true,
            alpha_value: 255,
            load_order: 0,
            always_on_top: 0,
        }
    }
}

fn parse_bool(v: &str, default: bool) -> bool {
    match v.trim().parse::<i64>() {
        Ok(n) => n != 0,
        Err(_) => default,
    }
}

fn parse_int(v: &str) -> Option<i64> {
    let t = v.trim();
    t.parse::<i64>().ok().or_else(|| t.parse::<f64>().ok().map(|f| f as i64))
}

pub struct Settings {
    path: PathBuf,
    ini: IniFile,
}

impl Settings {
    /// Read `env.settings_file`; a missing or unreadable file is empty settings.
    pub fn load(env: &Environment) -> Settings {
        let ini = match IniFile::load(&env.settings_file) {
            Ok(i) => i,
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("settings: cannot read {}: {e}", env.settings_file.display());
                }
                IniFile::parse("")
            }
        };
        Settings { path: env.settings_file.clone(), ini }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write the file (temp file + rename).
    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("ini.tmp");
        std::fs::write(&tmp, self.ini.to_ini_string())?;
        std::fs::rename(&tmp, &self.path)
    }

    pub fn ini(&self) -> &IniFile {
        &self.ini
    }

    // ------------------------------------------------------------- globals

    pub fn window_type(&self) -> WindowType {
        self.ini.get("Snowmeter", "WindowType").and_then(WindowType::parse).unwrap_or(WindowType::Desktop)
    }

    pub fn set_window_type(&mut self, t: WindowType) {
        self.ini.set("Snowmeter", "WindowType", t.as_str());
    }

    pub fn first_run_done(&self) -> bool {
        self.ini.get("Snowmeter", "FirstRunDone").map(|v| parse_bool(v, false)).unwrap_or(false)
    }

    pub fn set_first_run_done(&mut self, done: bool) {
        self.ini.set("Snowmeter", "FirstRunDone", if done { "1" } else { "0" });
    }

    pub fn global(&self, key: &str) -> Option<&str> {
        self.ini.get("Snowmeter", key)
    }

    pub fn set_global(&mut self, key: &str, value: &str) {
        self.ini.set("Snowmeter", key, value);
    }

    // ------------------------------------------------------------- configs

    /// Names of all config sections in file order.
    pub fn config_names(&self) -> Vec<String> {
        self.ini.sections().iter().filter(|s| !is_reserved_section(&s.name)).map(|s| s.name.clone()).collect()
    }

    pub fn has_config(&self, config: &str) -> bool {
        !is_reserved_section(config) && self.ini.section(config).is_some()
    }

    /// State of a config; `None` when it has no section yet.
    pub fn config(&self, config: &str) -> Option<ConfigState> {
        if is_reserved_section(config) {
            return None;
        }
        let sec = self.ini.section(config)?;
        let get = |k: &str| self.ini.get(&sec.name, k);
        let d = ConfigState::default();
        Some(ConfigState {
            active: get("Active").and_then(parse_int).map(|n| n.clamp(0, u32::MAX as i64) as u32).unwrap_or(d.active),
            window_x: get("WindowX").map(|s| s.trim().to_string()).unwrap_or(d.window_x),
            window_y: get("WindowY").map(|s| s.trim().to_string()).unwrap_or(d.window_y),
            draggable: get("Draggable").map(|v| parse_bool(v, d.draggable)).unwrap_or(d.draggable),
            click_through: get("ClickThrough").map(|v| parse_bool(v, d.click_through)).unwrap_or(d.click_through),
            keep_on_screen: get("KeepOnScreen").map(|v| parse_bool(v, d.keep_on_screen)).unwrap_or(d.keep_on_screen),
            save_position: get("SavePosition").map(|v| parse_bool(v, d.save_position)).unwrap_or(d.save_position),
            alpha_value: get("AlphaValue").and_then(parse_int).map(|n| n.clamp(0, 255) as u8).unwrap_or(d.alpha_value),
            load_order: get("LoadOrder").and_then(parse_int).map(|n| n as i32).unwrap_or(d.load_order),
            always_on_top: get("AlwaysOnTop").and_then(parse_int).map(|n| n as i32).unwrap_or(d.always_on_top),
        })
    }

    /// Write every known key of `state` into the config's section, leaving
    /// other keys untouched. Creates the section when missing.
    pub fn set_config(&mut self, config: &str, state: &ConfigState) {
        if is_reserved_section(config) {
            return;
        }
        let b = |v: bool| if v { "1" } else { "0" };
        let i = &mut self.ini;
        i.set(config, "Active", &state.active.to_string());
        i.set(config, "WindowX", &state.window_x);
        i.set(config, "WindowY", &state.window_y);
        i.set(config, "Draggable", b(state.draggable));
        i.set(config, "ClickThrough", b(state.click_through));
        i.set(config, "KeepOnScreen", b(state.keep_on_screen));
        i.set(config, "SavePosition", b(state.save_position));
        i.set(config, "AlphaValue", &state.alpha_value.to_string());
        i.set(config, "LoadOrder", &state.load_order.to_string());
        i.set(config, "AlwaysOnTop", &state.always_on_top.to_string());
    }

    /// `Active=N` only (creates the section with just that key when missing).
    pub fn set_active(&mut self, config: &str, n: u32) {
        if !is_reserved_section(config) {
            self.ini.set(config, "Active", &n.to_string());
        }
    }

    /// Set a single raw key of a config section.
    pub fn set_config_key(&mut self, config: &str, key: &str, value: &str) {
        if !is_reserved_section(config) {
            self.ini.set(config, key, value);
        }
    }

    pub fn config_key(&self, config: &str, key: &str) -> Option<&str> {
        self.ini.get(config, key)
    }

    /// `(config, active file index)` for every active config, ordered by
    /// `LoadOrder` (ties keep file order).
    pub fn active_configs(&self) -> Vec<(String, u32)> {
        let mut v: Vec<(String, u32, i32)> = self
            .config_names()
            .into_iter()
            .filter_map(|c| self.config(&c).filter(|s| s.active > 0).map(|s| (c, s.active, s.load_order)))
            .collect();
        v.sort_by_key(|t| t.2); // stable
        v.into_iter().map(|(c, a, _)| (c, a)).collect()
    }

    /// Apply a layout (a Rainmeter.ini-shaped file): configs missing from the
    /// layout become inactive, layout config sections replace ours, and the
    /// global `[Rainmeter]`/`[Snowmeter]` sections of the layout are ignored.
    pub fn apply_layout(&mut self, layout: &IniFile) {
        self.apply_layout_inner(layout, true);
    }

    /// Set the layout's config sections only; other configs are not deactivated.
    pub fn apply_layout_keep_others(&mut self, layout: &IniFile) {
        self.apply_layout_inner(layout, false);
    }

    fn apply_layout_inner(&mut self, layout: &IniFile, deactivate_others: bool) {
        let layout_names: Vec<&str> =
            layout.sections().iter().filter(|s| !is_reserved_section(&s.name)).map(|s| s.name.as_str()).collect();
        for c in self.config_names().into_iter().filter(|_| deactivate_others) {
            if !layout_names.iter().any(|n| n.eq_ignore_ascii_case(&c)) {
                self.ini.set(&c, "Active", "0");
            }
        }
        for sec in layout.sections().iter().filter(|s| !is_reserved_section(&s.name)) {
            self.ini.remove_section(&sec.name);
            for (k, v) in &sec.entries {
                self.ini.set(&sec.name, k, v);
            }
            if sec.entries.is_empty() {
                self.ini.set(&sec.name, "Active", "0");
            }
        }
    }
}
