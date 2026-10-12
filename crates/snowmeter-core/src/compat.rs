//! Compatibility checker: reads a config's `.ini` files (with `@Include`,
//! through the real `ConfigParser`, never executing anything) and reports what
//! Snowmeter can and cannot run.

use crate::bangs::{parse_actions, Action};
use crate::env::Environment;
use crate::measures::plugin::{canonical_plugin_name, is_known_measure_type, is_native_plugin};
use crate::meters::is_supported_meter;
use crate::parser::{ConfigParser, SkinIdentity};
use crate::registry::{SkinConfig, SkinRegistry};
use crate::skin::actions::{bang_support, BangSupport};
use std::collections::HashSet;
use std::fmt;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompatReport {
    /// Config name, e.g. `illustro\Clock`.
    pub config: String,
    /// The `.ini` files that were checked.
    pub files: Vec<String>,
    /// No missing features and no unsupported plugins.
    pub fully_supported: bool,
    /// Things that are missing: unsupported meter/measure types, unknown
    /// bangs, `MeasureName` pointing at missing measures, unreadable files.
    pub partial: Vec<String>,
    /// Windows plugins without a native replacement (placeholder measures).
    pub unsupported_plugins: Vec<String>,
    /// Harmless degradations: ignored options/bangs, translated Windows commands, include problems.
    pub notes: Vec<String>,
}

impl fmt::Display for CompatReport {
    /// One line: `<config>: Fully supported` / `Partially supported (missing: …)` /
    /// `Unsupported plugins: …` (both joined by `; ` when both apply).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.config)?;
        if self.fully_supported {
            return write!(f, "Fully supported");
        }
        let mut parts = Vec::new();
        if !self.partial.is_empty() {
            parts.push(format!("Partially supported (missing: {})", self.partial.join(", ")));
        }
        if !self.unsupported_plugins.is_empty() {
            parts.push(format!("Unsupported plugins: {}", self.unsupported_plugins.join(", ")));
        }
        write!(f, "{}", parts.join("; "))
    }
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

fn plugin_display(name: &str) -> String {
    name.rsplit(['\\', '/']).next().unwrap_or(name).trim().to_string()
}

/// Windows-only commands that Snowmeter translates (explorer, shell:, cmd, .exe …).
fn windows_command(cmd: &str) -> Option<String> {
    let c = cmd.trim().trim_matches('"');
    let l = c.to_ascii_lowercase();
    let first = l.split_whitespace().next().unwrap_or("").trim_matches('"');
    let drive = l.len() > 2 && l.as_bytes()[1] == b':' && l.as_bytes()[0].is_ascii_alphabetic();
    if l.starts_with("shell:")
        || first.ends_with(".exe")
        || first.ends_with(".bat")
        || first.ends_with(".cmd")
        || first.ends_with(".lnk")
        || first == "cmd"
        || drive
        || l.starts_with("::{")
    {
        let shown: String = c.chars().take(60).collect();
        Some(shown)
    } else {
        None
    }
}

const BLUR_OPTIONS: [&str; 5] = ["BLUR", "BLURMODE", "BLURREGION", "AEROBLUR", "BLURREGIONX"];

fn check_file(env: &Environment, cfg: &SkinConfig, file: &str, rep: &mut CompatReport) {
    let id = SkinIdentity {
        config: cfg.name.clone(),
        root_config: cfg.root.clone(),
        file: file.to_string(),
        ini_path: cfg.dir.join(file),
    };
    let parser = match ConfigParser::load(env, &id) {
        Ok(p) => p,
        Err(e) => {
            push_unique(&mut rep.partial, format!("{file} cannot be read ({e})"));
            return;
        }
    };
    for m in parser.log_messages() {
        push_unique(&mut rep.notes, format!("{file}: {m}"));
    }
    let sections = parser.section_names();
    let mut measures: HashSet<String> = HashSet::new();
    for s in &sections {
        if parser.has_key(s, "Measure") {
            measures.insert(s.to_ascii_lowercase());
        }
    }
    let mut scan: Vec<String> = sections.clone();
    scan.push("Rainmeter".to_string());
    for sec in &scan {
        let entries = parser.section_entries(sec);
        // Measure / plugin classification.
        if parser.has_key(sec, "Measure") {
            let ty = parser.read_string(sec, "Measure", "", None);
            let ty = ty.trim();
            if ty.eq_ignore_ascii_case("plugin") {
                let p = parser.read_string(sec, "Plugin", "", None);
                if p.trim().is_empty() {
                    push_unique(&mut rep.partial, format!("[{sec}] Measure=Plugin has no Plugin option"));
                } else if !is_native_plugin(&p) {
                    push_unique(&mut rep.unsupported_plugins, plugin_display(&p));
                }
            } else if !is_known_measure_type(ty) {
                push_unique(&mut rep.partial, format!("Measure type \"{ty}\""));
            } else if canonical_plugin_name(ty).is_some() && !is_native_plugin(ty) {
                // Plugin used by its internal measure name (Measure=Process).
                push_unique(&mut rep.unsupported_plugins, ty.to_string());
            }
        }
        // Meter type.
        if parser.has_key(sec, "Meter") {
            let ty = parser.read_string(sec, "Meter", "", None);
            let ty = ty.trim();
            if !is_supported_meter(ty) {
                push_unique(&mut rep.partial, format!("Meter type \"{ty}\""));
            }
            for (k, v) in &entries {
                let is_mn = k.strip_prefix("MEASURENAME").map(|r| r.chars().all(|c| c.is_ascii_digit())).unwrap_or(false);
                if is_mn {
                    let v = parser.replace_variables(v);
                    let v = v.trim();
                    if !v.is_empty() && !v.contains(['#', '[']) && !measures.contains(&v.to_ascii_lowercase()) {
                        push_unique(&mut rep.partial, format!("[{sec}] {} \"{v}\" refers to a missing measure", k.to_ascii_lowercase().replace("measurename", "MeasureName")));
                    }
                }
            }
        }
        // Options that only exist on Windows.
        if sec.eq_ignore_ascii_case("Rainmeter") {
            for (k, v) in &entries {
                if BLUR_OPTIONS.contains(&k.as_str()) && v.trim() != "0" {
                    push_unique(&mut rep.notes, format!("{} is ignored (no window blur)", pretty(k)));
                } else if k == "USED2D" {
                    push_unique(&mut rep.notes, "UseD2D is ignored (Snowmeter always uses its Cairo renderer)".into());
                }
            }
        }
        // Actions: bangs and commands.
        for (k, v) in &entries {
            if !k.contains("ACTION") {
                continue;
            }
            for a in parse_actions(&parser.replace_variables(v)) {
                match a {
                    Action::Bang { name, .. } => {
                        if name.contains(['#', '[']) {
                            continue;
                        }
                        match bang_support(&name) {
                            BangSupport::Supported => {}
                            BangSupport::Ignored => push_unique(&mut rep.notes, format!("!{name} has no effect on Linux")),
                            BangSupport::Unknown => push_unique(&mut rep.partial, format!("Bang !{name}")),
                        }
                    }
                    Action::Command(c) => {
                        if let Some(w) = windows_command(&c) {
                            push_unique(&mut rep.notes, format!("Windows command translated: {w}"));
                        }
                    }
                }
            }
        }
    }
}

fn pretty(upper: &str) -> &'static str {
    match upper {
        "BLUR" => "Blur",
        "BLURMODE" => "BlurMode",
        "BLURREGION" | "BLURREGIONX" => "BlurRegion",
        _ => "AeroBlur",
    }
}

fn report_for(env: &Environment, cfg: &SkinConfig) -> CompatReport {
    let mut rep = CompatReport { config: cfg.name.clone(), ..Default::default() };
    if cfg.files.is_empty() {
        rep.notes.push("no .ini files in this folder".into());
    }
    for f in &cfg.files {
        rep.files.push(f.clone());
        check_file(env, cfg, f, &mut rep);
    }
    rep.fully_supported = rep.partial.is_empty() && rep.unsupported_plugins.is_empty();
    rep
}

/// Check one config by name (`illustro\Clock`, any case); every variant file
/// is examined. An unknown config yields a report with a `partial` entry.
pub fn check_config(env: &Environment, config: &str) -> CompatReport {
    let reg = SkinRegistry::scan(env);
    match reg.config(config) {
        Some(c) => report_for(env, c),
        None => CompatReport {
            config: config.to_string(),
            partial: vec!["config not found".into()],
            ..Default::default()
        },
    }
}

fn under_skins(env: &Environment, p: &Path) -> Option<String> {
    let abs = std::path::absolute(p).ok()?;
    let skins = std::path::absolute(&env.skins_dir).ok()?;
    let rel = abs.strip_prefix(&skins).ok()?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    Some(parts.join("\\"))
}

fn reports_under(env: &Environment, prefix: &str) -> Vec<CompatReport> {
    let reg = SkinRegistry::scan(env);
    let p = prefix.trim_matches('\\').to_ascii_lowercase();
    reg.configs
        .iter()
        .filter(|c| {
            let n = c.name.to_ascii_lowercase();
            n == p || n.starts_with(&format!("{p}\\"))
        })
        .filter(|c| !c.files.is_empty())
        .map(|c| report_for(env, c))
        .collect()
}

/// Check a config name (`illustro\Clock`), a skin root or config folder, or
/// an `.ini` path. Folders and ini files outside `env.skins_dir` are checked
/// in place. Returns one report per config found.
pub fn check_path(env: &Environment, target: &str) -> Result<Vec<CompatReport>, String> {
    let t = target.trim();
    // 1. A config (or root) name.
    let name = t.replace('/', "\\");
    let by_name = reports_under(env, &name);
    if !by_name.is_empty() {
        return Ok(by_name);
    }
    // 2. A path.
    let p = Path::new(t);
    if p.is_file() {
        let dir = p.parent().ok_or_else(|| format!("{t}: no parent folder"))?;
        return check_dir(env, dir).map(|v| v.into_iter().collect());
    }
    if p.is_dir() {
        return check_dir(env, p);
    }
    Err(format!("{t}: not a config name, skin folder or .ini file"))
}

fn check_dir(env: &Environment, dir: &Path) -> Result<Vec<CompatReport>, String> {
    if let Some(rel) = under_skins(env, dir) {
        if !rel.is_empty() {
            let r = reports_under(env, &rel);
            if !r.is_empty() {
                return Ok(r);
            }
        }
    }
    // Outside the skins folder: treat `dir` as a skin root of a temporary view.
    let abs = std::path::absolute(dir).map_err(|e| e.to_string())?;
    let parent = abs.parent().ok_or_else(|| "folder has no parent".to_string())?;
    let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| "bad folder".to_string())?;
    let mut view = env.clone();
    view.skins_dir = parent.to_path_buf();
    let r = reports_under(&view, &name);
    if r.is_empty() {
        Err(format!("{}: no .ini skin files found", dir.display()))
    } else {
        Ok(r)
    }
}
