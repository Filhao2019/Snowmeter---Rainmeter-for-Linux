//! The Script measure (measures.md section 3.10): Lua 5.1 (LuaJIT) with the
//! Rainmeter `SKIN` / `SELF` API. One Lua state per measure.

use super::base::format_value;
use super::{MeasureCtx, MeasureImpl};
use crate::numparse::parse_f64_prefix;
use crate::parser::OptionReader;
use mlua::{Function, HookTriggers, VmState, Lua, LuaOptions, MultiValue, StdLib, Table, Value, Variadic};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Wall-clock budget for one `Update`/`Initialize`/inline/command call.
pub const CALL_BUDGET: Duration = Duration::from_millis(200);
/// Budget for executing the script chunk when it is loaded.
pub const LOAD_BUDGET: Duration = Duration::from_secs(1);
/// Memory cap per Lua state.
const MEMORY_LIMIT: usize = 64 << 20;

/// Value of another measure as seen from Lua.
#[derive(Clone, Debug, Default)]
pub struct MeasureSnapshot {
    pub value: f64,
    pub relative: f64,
    pub min: f64,
    pub max: f64,
}

/// What a script may ask of the skin it lives in.
pub trait ScriptHost {
    fn variable(&self, name: &str) -> Option<String>;
    /// `#Var#` and `[Measure]` replaced.
    fn replace_variables(&self, text: &str) -> String;
    fn parse_formula(&self, text: &str) -> Option<f64>;
    fn measure(&self, name: &str) -> Option<MeasureSnapshot>;
    /// `GetStringValue(autoScale, scale, decimals, percentual)` of a measure.
    fn measure_string(&self, name: &str, args: (u8, f64, i32, bool)) -> Option<String>;
    /// `(x, y, w, h)` of a meter.
    fn meter_geometry(&self, name: &str) -> Option<[f64; 4]>;
    /// An option of any section, variables and measures substituted.
    fn option(&self, section: &str, key: &str, default: &str) -> String;
    fn skin_size(&self) -> (i32, i32);
    fn skin_dir(&self) -> PathBuf;
}

type HostPtr = *const (dyn ScriptHost + 'static);

/// State shared between the Rust side and the Lua closures.
struct Shared {
    /// Valid only while a call into Lua is running.
    host: Cell<Option<HostPtr>>,
    bangs: RefCell<Vec<String>>,
    log: RefCell<Vec<String>>,
    section: RefCell<String>,
    value: Cell<f64>,
    string: RefCell<Option<String>>,
    /// Deadline of the Lua call in progress (checked by the instruction hook).
    deadline: Cell<Option<Instant>>,
    /// Set when the budget was exceeded during the current call.
    tripped: Cell<bool>,
}

impl Shared {
    fn with_host<R>(&self, f: impl FnOnce(&dyn ScriptHost) -> R) -> Option<R> {
        // SAFETY: the pointer is set from a live `&dyn ScriptHost` for the
        // duration of one Lua call and cleared right after it.
        self.host.get().map(|p| f(unsafe { &*p }))
    }

    fn bang(&self, text: String) {
        self.bangs.borrow_mut().push(text);
    }
}

pub struct ScriptMeasure {
    file: String,
    loaded_file: Option<String>,
    lua: Option<Lua>,
    env: Option<Table>,
    shared: Rc<Shared>,
    initialized: bool,
    value: f64,
    string: Option<String>,
    log: Vec<String>,
}

impl Default for ScriptMeasure {
    fn default() -> Self {
        ScriptMeasure::new()
    }
}

/// Lua bytes to text: UTF-8 if valid, else Windows-1252 style Latin-1.
fn lua_text(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

fn arg_text(v: &Value) -> String {
    match v {
        Value::String(s) => lua_text(&s.as_bytes()),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => num_text(*n),
        Value::Boolean(b) => b.to_string(),
        _ => String::new(),
    }
}

fn opt_num(v: &Option<Value>, default: f64) -> f64 {
    match v {
        Some(Value::Integer(i)) => *i as f64,
        Some(Value::Number(n)) => *n,
        Some(Value::String(s)) => parse_f64_prefix(&lua_text(&s.as_bytes())),
        _ => default,
    }
}

/// `%.5f` with trailing zeros removed.
fn num_text(n: f64) -> String {
    let mut s = format!("{n:.5}");
    super::base::remove_trailing_zero(&mut s);
    s
}

fn quote_arg(a: &str) -> String {
    if a.contains(' ') && !a.starts_with('"') {
        format!("\"{a}\"")
    } else {
        a.to_string()
    }
}

fn name_of(t: &Table) -> String {
    t.raw_get::<String>("__name").unwrap_or_default()
}

impl ScriptMeasure {
    pub fn new() -> ScriptMeasure {
        ScriptMeasure {
            file: String::new(),
            loaded_file: None,
            lua: None,
            env: None,
            shared: Rc::new(Shared {
                host: Cell::new(None),
                bangs: RefCell::new(Vec::new()),
                log: RefCell::new(Vec::new()),
                section: RefCell::new(String::new()),
                value: Cell::new(0.0),
                string: RefCell::new(None),
                deadline: Cell::new(None),
                tripped: Cell::new(false),
            }),
            initialized: false,
            value: 0.0,
            string: None,
            log: Vec::new(),
        }
    }

    /// The Lua state and environment, once the script is loaded.
    fn make_lua(&self) -> mlua::Result<(Lua, Table)> {
        let libs = StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::OS | StdLib::IO | StdLib::BIT;
        let lua = Lua::new_with(libs, LuaOptions::default())?;
        let g = lua.globals();
        for name in ["collectgarbage", "require", "module", "loadfile", "dofile"] {
            g.set(name, Value::Nil)?;
        }
        if let Ok(os) = g.get::<Table>("os") {
            for n in ["exit", "execute"] {
                os.set(n, Value::Nil)?;
            }
            os.set("setlocale", lua.create_function(|_, _: Variadic<Value>| Ok(Value::Nil))?)?;
        }
        if let Ok(io) = g.get::<Table>("io") {
            for n in ["popen", "stdin", "stdout", "stderr"] {
                io.set(n, Value::Nil)?;
            }
        }
        let shared = self.shared.clone();
        g.set(
            "print",
            lua.create_function(move |lua, args: Variadic<Value>| {
                let tostring: Function = lua.globals().get("tostring")?;
                let mut parts = Vec::new();
                for a in args.iter() {
                    let s: mlua::LuaString = tostring.call(a.clone())?;
                    parts.push(lua_text(&s.as_bytes()));
                }
                shared.log.borrow_mut().push(parts.join("    "));
                Ok(())
            })?,
        )?;
        let tolua = lua.create_table()?;
        tolua.set("cast", lua.create_function(|_, (v, _): (Value, Value)| Ok(v))?)?;
        g.set("tolua", tolua)?;
        install_api(&lua, &self.shared)?;
        // Memory cap; not every build has the allocator hook, so ignore failure.
        let _ = lua.set_memory_limit(MEMORY_LIMIT);
        let sh = self.shared.clone();
        lua.set_global_hook(HookTriggers::new().every_nth_instruction(10_000), move |_, _| {
            match sh.deadline.get() {
                Some(d) if Instant::now() > d => {
                    sh.tripped.set(true);
                    Err(mlua::Error::runtime("script exceeded its time budget and was aborted"))
                }
                _ => Ok(VmState::Continue),
            }
        })?;
        // `pcall` must not be able to swallow the budget abort.
        for name in ["pcall", "xpcall"] {
            if let Ok(orig) = g.get::<Function>(name) {
                let sh = self.shared.clone();
                g.set(
                    name,
                    lua.create_function(move |_, args: MultiValue| {
                        let r = orig.call::<MultiValue>(args)?;
                        if sh.tripped.get() {
                            return Err(mlua::Error::runtime("script exceeded its time budget and was aborted"));
                        }
                        Ok(r)
                    })?,
                )?;
            }
        }
        let env = lua.create_table()?;
        let meta = lua.create_table()?;
        meta.set("__index", g.clone())?;
        env.set_metatable(Some(meta))?;
        Ok((lua, env))
    }

    fn load_script(&mut self, skin_dir: &std::path::Path, log: &mut Vec<String>) {
        self.lua = None;
        self.env = None;
        self.initialized = false;
        self.loaded_file = Some(self.file.clone());
        if self.file.is_empty() {
            log.push("Script: File not valid".into());
            return;
        }
        let path = crate::paths::resolve_path(skin_dir, &self.file);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                log.push(format!("Script: File not valid ({}: {e})", path.display()));
                return;
            }
        };
        let src = decode_source(bytes);
        match self.make_lua() {
            Ok((lua, env)) => {
                self.shared.tripped.set(false);
        self.shared.deadline.set(Some(Instant::now() + LOAD_BUDGET));
                let res = lua.load(&src).set_name(format!("@{}", path.display())).set_environment(env.clone()).exec();
                self.shared.deadline.set(None);
                match res {
                    Ok(()) => {
                        self.lua = Some(lua);
                        self.env = Some(env);
                    }
                    Err(e) => {
                        log.push("Script: File not valid".into());
                        log.push(format!("Script: {e}"));
                    }
                }
            }
            Err(e) => log.push(format!("Script: cannot create Lua state: {e}")),
        }
    }

    fn set_host(&self, host: Option<&dyn ScriptHost>) {
        let ptr = host.map(|h| {
            let p: *const (dyn ScriptHost + '_) = h;
            // SAFETY: only the lifetime is erased; see `Shared::with_host`.
            unsafe { std::mem::transmute::<*const (dyn ScriptHost + '_), HostPtr>(p) }
        });
        self.shared.host.set(ptr);
    }

    fn sync_self(&self) {
        self.shared.value.set(self.value);
        *self.shared.string.borrow_mut() = self.string.clone();
    }

    /// Call a global function of the script; `None` if it does not exist.
    fn call(&mut self, func: &str, args: MultiValue, host: Option<&dyn ScriptHost>) -> Option<Result<MultiValue, String>> {
        let (lua, env) = (self.lua.as_ref()?, self.env.as_ref()?);
        let f: Value = env.get(func).ok()?;
        let Value::Function(f) = f else { return None };
        let _ = lua;
        self.sync_self();
        self.set_host(host);
        self.shared.tripped.set(false);
        self.shared.deadline.set(Some(Instant::now() + CALL_BUDGET));
        let r = f.call::<MultiValue>(args).map_err(|e| e.to_string());
        self.shared.deadline.set(None);
        self.set_host(None);
        Some(r)
    }

    fn flush_log(&mut self, ctx_log: &mut Vec<String>) {
        ctx_log.append(&mut self.log);
        ctx_log.extend(self.shared.log.borrow_mut().drain(..));
    }
}

fn decode_source(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return bytes[3..].to_vec();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units).into_bytes();
    }
    bytes
}

/// `SKIN`, the shared metatables for measure and meter objects.
fn install_api(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<()> {
    let g = lua.globals();
    let skin = lua.create_table()?;

    let sh = shared.clone();
    skin.set(
        "Bang",
        lua.create_function(move |_, (_t, first, rest): (Value, Value, Variadic<Value>)| {
            let rep = |s: String| sh.with_host(|h| h.replace_variables(&s)).unwrap_or(s);
            let mut text = rep(arg_text(&first));
            for a in rest.iter() {
                text.push(' ');
                text.push_str(&quote_arg(&rep(arg_text(a))));
            }
            sh.bang(text);
            Ok(())
        })?,
    )?;

    let measure_mt = make_measure_mt(lua, shared)?;
    let meter_mt = make_meter_mt(lua, shared)?;
    let sh = shared.clone();
    let mmt = measure_mt.clone();
    skin.set(
        "GetMeasure",
        lua.create_function(move |lua, (_t, name): (Value, String)| {
            let exists = sh.with_host(|h| h.measure(&name).is_some()).unwrap_or(false) || name.eq_ignore_ascii_case(&sh.section.borrow());
            if !exists {
                return Ok(Value::Nil);
            }
            let t = lua.create_table()?;
            t.raw_set("__name", name)?;
            t.set_metatable(Some(mmt.clone()))?;
            Ok(Value::Table(t))
        })?,
    )?;
    let sh = shared.clone();
    skin.set(
        "GetMeter",
        lua.create_function(move |lua, (_t, name): (Value, String)| {
            if sh.with_host(|h| h.meter_geometry(&name).is_none()).unwrap_or(true) {
                return Ok(Value::Nil);
            }
            let t = lua.create_table()?;
            t.raw_set("__name", name)?;
            t.set_metatable(Some(meter_mt.clone()))?;
            Ok(Value::Table(t))
        })?,
    )?;
    let sh = shared.clone();
    skin.set(
        "GetVariable",
        lua.create_function(move |_, (_t, name, def): (Value, String, Option<String>)| {
            Ok(sh.with_host(|h| h.variable(&name)).flatten().or(def))
        })?,
    )?;
    let sh = shared.clone();
    skin.set(
        "ReplaceVariables",
        lua.create_function(move |_, (_t, text): (Value, String)| Ok(sh.with_host(|h| h.replace_variables(&text)).unwrap_or(text)))?,
    )?;
    let sh = shared.clone();
    skin.set(
        "ParseFormula",
        lua.create_function(move |_, (_t, text): (Value, String)| {
            let t = text.trim();
            if t.starts_with('(') {
                Ok(sh.with_host(|h| h.parse_formula(t)).flatten())
            } else {
                Ok(t.parse::<f64>().ok())
            }
        })?,
    )?;
    let sh = shared.clone();
    skin.set(
        "MakePathAbsolute",
        lua.create_function(move |_, (_t, path): (Value, String)| {
            Ok(match sh.with_host(|h| h.skin_dir()) {
                Some(d) => crate::paths::resolve_path(&d, &path).to_string_lossy().into_owned(),
                None => path,
            })
        })?,
    )?;
    let sh = shared.clone();
    skin.set(
        "MoveWindow",
        lua.create_function(move |_, (_t, x, y): (Value, Value, Value)| {
            sh.bang(format!("!Move {} {}", arg_text(&x), arg_text(&y)));
            Ok(())
        })?,
    )?;
    skin.set("FadeWindow", lua.create_function(|_, _: Variadic<Value>| Ok(()))?)?;
    skin.set("GetX", lua.create_function(|_, _: Variadic<Value>| Ok(0))?)?;
    skin.set("GetY", lua.create_function(|_, _: Variadic<Value>| Ok(0))?)?;
    let sh = shared.clone();
    skin.set("GetW", lua.create_function(move |_, _: Variadic<Value>| Ok(sh.with_host(|h| h.skin_size().0).unwrap_or(0)))?)?;
    let sh = shared.clone();
    skin.set("GetH", lua.create_function(move |_, _: Variadic<Value>| Ok(sh.with_host(|h| h.skin_size().1).unwrap_or(0)))?)?;
    let sh = shared.clone();
    skin.set(
        "SetVariable",
        lua.create_function(move |_, (_t, n, v): (Value, String, Value)| {
            sh.bang(format!("!SetVariable {n} {}", quote_arg(&arg_text(&v))));
            Ok(())
        })?,
    )?;
    let sh = shared.clone();
    skin.set(
        "SetOption",
        lua.create_function(move |_, (_t, s, o, v): (Value, String, String, Value)| {
            sh.bang(format!("!SetOption {s} {o} {}", quote_arg(&arg_text(&v))));
            Ok(())
        })?,
    )?;
    g.set("SKIN", skin)?;

    // SELF: the measure object of this script, created on use.
    let t = lua.create_table()?;
    t.raw_set("__name", shared.section.borrow().clone())?;
    t.set_metatable(Some(measure_mt))?;
    g.set("SELF", t)?;
    Ok(())
}

fn snapshot(sh: &Shared, name: &str) -> MeasureSnapshot {
    if name.eq_ignore_ascii_case(&sh.section.borrow()) {
        let min = sh.with_host(|h| h.option(name, "MinValue", "0")).map(|s| parse_f64_prefix(&s)).unwrap_or(0.0);
        let max = sh.with_host(|h| h.option(name, "MaxValue", "1")).map(|s| parse_f64_prefix(&s)).unwrap_or(1.0);
        let v = sh.value.get();
        let range = max - min;
        let relative = if range != 0.0 { (v.clamp(min.min(max), max.max(min)) - min) / range } else { 1.0 };
        return MeasureSnapshot { value: v, relative, min, max };
    }
    sh.with_host(|h| h.measure(name)).flatten().unwrap_or_default()
}

fn make_measure_mt(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let mt = lua.create_table()?;
    let m = lua.create_table()?;
    let picks: [(&str, fn(&MeasureSnapshot) -> f64); 4] = [
        ("GetValue", |s| s.value),
        ("GetRelativeValue", |s| s.relative),
        ("GetMinValue", |s| s.min),
        ("GetMaxValue", |s| s.max),
    ];
    for (name, pick) in picks {
        let sh = shared.clone();
        m.set(name, lua.create_function(move |_, t: Table| Ok(pick(&snapshot(&sh, &name_of(&t)))))?)?;
    }
    m.set(
        "GetValueRange",
        lua.create_function({
            let sh = shared.clone();
            move |_, t: Table| {
                let s = snapshot(&sh, &name_of(&t));
                Ok(s.max - s.min)
            }
        })?,
    )?;
    m.set("GetName", lua.create_function(|_, t: Table| Ok(name_of(&t)))?)?;
    let sh = shared.clone();
    m.set(
        "GetOption",
        lua.create_function(move |_, (t, key, def): (Table, String, Option<String>)| {
            let name = name_of(&t);
            let def = def.unwrap_or_default();
            Ok(sh.with_host(|h| h.option(&name, &key, &def)).unwrap_or(def))
        })?,
    )?;
    let sh = shared.clone();
    m.set(
        "GetNumberOption",
        lua.create_function(move |_, (t, key, def): (Table, String, Option<Value>)| {
            let name = name_of(&t);
            let d = opt_num(&def, 0.0);
            let text = sh.with_host(|h| h.option(&name, &key, "")).unwrap_or_default();
            let text = text.trim().to_string();
            if text.is_empty() {
                return Ok(d);
            }
            if text.starts_with('(') {
                return Ok(sh.with_host(|h| h.parse_formula(&text)).flatten().unwrap_or(d));
            }
            Ok(parse_f64_prefix(&text))
        })?,
    )?;
    let sh = shared.clone();
    m.set(
        "GetStringValue",
        lua.create_function(move |_, (t, a, b, c, d): (Table, Option<Value>, Option<Value>, Option<Value>, Option<Value>)| {
            let name = name_of(&t);
            let args = (
                opt_num(&a, 0.0) as u8,
                opt_num(&b, 1.0),
                opt_num(&c, 0.0) as i32,
                opt_num(&d, 0.0) != 0.0,
            );
            if name.eq_ignore_ascii_case(&sh.section.borrow()) {
                if let Some(s) = sh.string.borrow().clone() {
                    return Ok(s);
                }
                let snap = snapshot(&sh, &name);
                return Ok(format_value(snap.value, snap.relative, args.0, args.1, args.2, args.3));
            }
            Ok(sh.with_host(|h| h.measure_string(&name, args)).flatten().unwrap_or_default())
        })?,
    )?;
    for (fname, bang) in [("Disable", "!DisableMeasure"), ("Enable", "!EnableMeasure")] {
        let sh = shared.clone();
        m.set(
            fname,
            lua.create_function(move |_, t: Table| {
                sh.bang(format!("{bang} {}", quote_arg(&name_of(&t))));
                Ok(())
            })?,
        )?;
    }
    let sh = shared.clone();
    m.set(
        "SetOption",
        lua.create_function(move |_, (t, o, v): (Table, String, Value)| {
            sh.bang(format!("!SetOption {} {o} {}", quote_arg(&name_of(&t)), quote_arg(&arg_text(&v))));
            Ok(())
        })?,
    )?;
    mt.set("__index", m)?;
    Ok(mt)
}

fn make_meter_mt(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let mt = lua.create_table()?;
    let m = lua.create_table()?;
    m.set("GetName", lua.create_function(|_, t: Table| Ok(name_of(&t)))?)?;
    for (fname, i) in [("GetX", 0usize), ("GetY", 1), ("GetW", 2), ("GetH", 3)] {
        let sh = shared.clone();
        m.set(
            fname,
            lua.create_function(move |_, t: Table| Ok(sh.with_host(|h| h.meter_geometry(&name_of(&t)).map(|g| g[i])).flatten().unwrap_or(0.0)))?,
        )?;
    }
    for (fname, bang) in [("Show", "!ShowMeter"), ("Hide", "!HideMeter")] {
        let sh = shared.clone();
        m.set(
            fname,
            lua.create_function(move |_, t: Table| {
                sh.bang(format!("{bang} {}", quote_arg(&name_of(&t))));
                Ok(())
            })?,
        )?;
    }
    let sh = shared.clone();
    m.set(
        "SetX",
        lua.create_function(move |_, (t, v): (Table, Value)| {
            sh.bang(format!("!SetOption {} X {}", quote_arg(&name_of(&t)), arg_text(&v)));
            Ok(())
        })?,
    )?;
    let sh = shared.clone();
    m.set(
        "SetY",
        lua.create_function(move |_, (t, v): (Table, Value)| {
            sh.bang(format!("!SetOption {} Y {}", quote_arg(&name_of(&t)), arg_text(&v)));
            Ok(())
        })?,
    )?;
    let sh = shared.clone();
    m.set(
        "SetOption",
        lua.create_function(move |_, (t, o, v): (Table, String, Value)| {
            sh.bang(format!("!SetOption {} {o} {}", quote_arg(&name_of(&t)), quote_arg(&arg_text(&v))));
            Ok(())
        })?,
    )?;
    let sh = shared.clone();
    m.set(
        "GetOption",
        lua.create_function(move |_, (t, key, def): (Table, String, Option<String>)| {
            let name = name_of(&t);
            let def = def.unwrap_or_default();
            Ok(sh.with_host(|h| h.option(&name, &key, &def)).unwrap_or(def))
        })?,
    )?;
    mt.set("__index", m)?;
    Ok(mt)
}

impl ScriptMeasure {
    /// Interpret the results of `Update()`.
    fn take_results(&mut self, vals: MultiValue) {
        let mut got_number = false;
        let mut string: Option<String> = None;
        let mut value = self.value;
        for v in vals.iter().take(2) {
            match v {
                Value::Integer(i) => {
                    value = *i as f64;
                    got_number = true;
                }
                Value::Number(n) => {
                    value = *n;
                    got_number = true;
                }
                Value::String(s) => {
                    let s = lua_text(&s.as_bytes());
                    if !got_number {
                        value = parse_f64_prefix(&s);
                    }
                    string = Some(s);
                }
                _ => {}
            }
        }
        self.value = value;
        self.string = string;
    }
}

impl MeasureImpl for ScriptMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        *self.shared.section.borrow_mut() = opt.section.to_string();
        let file = opt.string("ScriptFile", "");
        if self.loaded_file.as_deref() != Some(file.as_str()) {
            self.loaded_file = None;
        }
        self.file = file;
    }

    fn update(&mut self, ctx: &mut MeasureCtx) -> f64 {
        if self.loaded_file.is_none() {
            let mut log = Vec::new();
            self.load_script(ctx.skin_dir, &mut log);
            self.log.extend(log);
        }
        if self.lua.is_some() {
            if !self.initialized {
                self.initialized = true;
                if let Some(Err(e)) = self.call("Initialize", MultiValue::new(), ctx.host) {
                    self.log.push(format!("Script Initialize() failed: {e}"));
                }
            }
            match self.call("Update", MultiValue::new(), ctx.host) {
                Some(Ok(vals)) => self.take_results(vals),
                Some(Err(e)) => self.log.push(format!("Script Update() failed: {e}")),
                None => {}
            }
        }
        self.flush_log(ctx.log);
        self.value
    }

    fn string_value(&self) -> Option<String> {
        self.string.clone()
    }

    fn command_host(&mut self, args: &str, host: Option<&dyn ScriptHost>) {
        let (Some(lua), Some(env)) = (self.lua.as_ref(), self.env.as_ref()) else { return };
        self.sync_self();
        self.set_host(host);
        self.shared.tripped.set(false);
        self.shared.deadline.set(Some(Instant::now() + CALL_BUDGET));
        let r = lua.load(args).set_name("=CommandMeasure").set_environment(env.clone()).exec();
        self.shared.deadline.set(None);
        self.set_host(None);
        if let Err(e) = r {
            self.log.push(format!("Script: {e}"));
        }
    }

    fn command(&mut self, args: &str) {
        self.command_host(args, None);
    }

    fn inline_call(&self, call: &str) -> Option<String> {
        let open = call.find('(')?;
        let close = call.rfind(')')?;
        if close < open || !call[close + 1..].trim().is_empty() {
            return None;
        }
        let name = call[..open].trim();
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        let (lua, env) = (self.lua.as_ref()?, self.env.as_ref()?);
        let Value::Function(f) = env.get::<Value>(name).ok()? else { return None };
        let mut args = MultiValue::new();
        for a in split_args(&call[open + 1..close]) {
            args.push_back(parse_call_arg(lua, &a)?);
        }
        self.shared.tripped.set(false);
        self.shared.deadline.set(Some(Instant::now() + CALL_BUDGET));
        let r: mlua::Result<Value> = f.call(args);
        self.shared.deadline.set(None);
        match r.ok()? {
            Value::Integer(i) => Some(i.to_string()),
            Value::Number(n) => Some(num_text(n)),
            Value::Boolean(b) => Some(if b { "1" } else { "0" }.to_string()),
            Value::String(s) => Some(lua_text(&s.as_bytes())),
            _ => None,
        }
    }

    fn take_actions(&mut self) -> Vec<String> {
        std::mem::take(&mut *self.shared.bangs.borrow_mut())
    }

    fn take_log(&mut self) -> Vec<String> {
        let mut v = std::mem::take(&mut self.log);
        v.extend(self.shared.log.borrow_mut().drain(..));
        v
    }
}

fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut quote) = (0, None::<char>);
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c);
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                cur.push(c);
            }
            (None, '(') => {
                depth += 1;
                cur.push(c);
            }
            (None, ')') => {
                depth -= 1;
                cur.push(c);
            }
            (None, ',') if depth == 0 => out.push(std::mem::take(&mut cur)),
            (None, c) => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|a| a.trim().to_string()).collect()
}

fn parse_call_arg(lua: &Lua, a: &str) -> Option<Value> {
    let b = a.as_bytes();
    if b.len() >= 2 && (b[0] == b'\'' || b[0] == b'"') && b[b.len() - 1] == b[0] {
        return lua.create_string(&a[1..a.len() - 1]).ok().map(Value::String);
    }
    match a.to_ascii_lowercase().as_str() {
        "true" => return Some(Value::Boolean(true)),
        "false" => return Some(Value::Boolean(false)),
        "nil" => return Some(Value::Nil),
        _ => {}
    }
    Some(Value::Number(parse_f64_prefix(a)))
}
