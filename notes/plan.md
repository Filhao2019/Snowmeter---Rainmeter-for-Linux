# Snowmeter Implementation Plan

> **For agentic workers:** executed with superpowers:subagent-driven-development in a token-saving form (see `decisions.md` #6): one Sonnet implementer per milestone, milestone-level reviews. Steps use `- [ ]` checkboxes.

**Goal:** An MIT-licensed Linux desktop widget app that runs unmodified Windows Rainmeter skins (folders and `.rmskin`).

**Architecture:** Cargo workspace. `crates/snowmeter-core` = engine with no GTK (INI/ConfigParser, variables, formulas, bangs, measures, meters, Cairo/Pango rendering, importer, compat checker, headless PNG renderer). `crates/snowmeter` = GTK4 app (X11 desktop-type windows, input, context menu, tray, IPC, CLI). See `notes/architecture.md`.

**Tech stack:** Rust 1.99 (`~/.cargo/bin`), gtk4 0.11 (`v4_14`), gdk4-x11 0.11 (`xlib`), x11 2.21, cairo-rs/pango/pangocairo 0.22, image, mlua 0.12 (`lua51`,`vendored`), pcre2 0.2, ureq 3, zip (deflate only), chrono, ksni 0.3, clap 4, log + env_logger, libc.

**Spec:** the user's task prompt (summarised in Global Constraints) + research notes: `notes/skin-format.md`, `notes/measures.md`, `notes/meters.md`, `notes/rmskin-format.md`, `notes/sample-skins.md`, `notes/binary-evidence.md`. Rainmeter source in `/home/bernardo/linux-rainmeter/rainmeter-src` is the source of truth when a note is silent.

## Global Constraints

- Windows Rainmeter skins must work AS-IS: no conversion step; unknown things degrade (placeholder + log), **never panic/crash**.
- Repo root: `/home/bernardo/linux-rainmeter/snowmeter`, branch `main`. After each milestone: `cargo build --workspace` with **zero warnings**, `cargo test --workspace` green, `git commit` + `git push`.
- Never copy files from `rainmeter-src`, `sample-skins` or the Wine Rainmeter install into the repo. Test fixtures are small skins written by us under `crates/snowmeter-core/tests/fixtures/`.
- Always `export PATH=$HOME/.cargo/bin:$PATH` before cargo.
- Skins dir `~/.config/snowmeter/skins/` (same layout as Rainmeter `Skins\`); settings `~/.config/snowmeter/snowmeter.ini`. Every path in core is overridable (env `SNOWMETER_CONFIG_DIR`) so tests never touch the real home.
- Case-insensitive INI section/key names; config names use backslash like Rainmeter (`illustro\Clock`).
- TDD (test first, see it fail, then implement) for: INI parser, variables, @Include, formulas, Time format codes, `.rmskin` importer.
- Measures required: Time (all codes), CPU, PhysicalMemory/Memory/SwapMemory, FreeDiskSpace, Uptime, Calc, String, NetIn/NetOut/NetTotal, Registry (stub), Script (Lua via mlua lua51), WebParser (HTTP fetch). Meters required: String (fonts, colors, align, InlineSetting basics, StringCase), Image (scaling, tint), Bar, Line, Roundline, Shape (Rectangle, Ellipse, Line, Path basics), Rotator, Button.
- Bangs required: !Refresh, !SetVariable, !ToggleMeter, !ShowMeter, !HideMeter, !ActivateConfig, plus opening URLs/files; everything in `skin-format.md` marked must-have.
- Windows `.dll` plugins: placeholder measure + one log line, never crash.
- Match **stock Rainmeter 4.5.27** behaviour (the installed DLL). Items tagged **(fork)** in the notes are optional/lowest priority.

## Review Focus

1. Skin files saved as UTF-16LE with BOM (very common for Windows skins) → must parse like UTF-8 ones. Test in M1.
2. Paths with backslashes and wrong case (`#@#Images\Bg.PNG` vs `images/bg.png` on disk) → must resolve. Test in M1 (`paths.rs`).
3. A skin referencing a missing measure/image/font/plugin → renders the rest, logs once, no panic. Test in M3 (skin integration test with a broken fixture).
4. Variables changed by `!SetVariable` → visible to meters with `DynamicVariables=1` on the next update, not to non-dynamic ones (Rainmeter semantics). Test in M3.
5. `.rmskin` without the footer / plain zip / folder drop → importer still installs. Test in M5.

---

## Shared interfaces (all milestones must use these names)

```rust
// crates/snowmeter-core/src/ini.rs
pub fn decode_text(bytes: &[u8]) -> String;                 // UTF-16LE/BE BOM, UTF-8 (BOM or not), else Windows-1252
pub struct IniFile { /* ordered sections */ }
pub struct IniSection { pub name: String, pub entries: Vec<(String, String)> }
impl IniFile {
    pub fn parse(text: &str) -> IniFile;
    pub fn load(path: &Path) -> std::io::Result<IniFile>;
    pub fn sections(&self) -> &[IniSection];
    pub fn section(&self, name: &str) -> Option<&IniSection>; // ASCII case-insensitive
    pub fn get(&self, section: &str, key: &str) -> Option<&str>;
    pub fn set(&mut self, section: &str, key: &str, value: &str);
    pub fn remove_section(&mut self, name: &str);
    pub fn to_ini_string(&self) -> String;
}
pub fn write_key_value(path: &Path, section: &str, key: &str, value: &str) -> std::io::Result<()>;

// formula.rs  (Rainmeter MathParser port)
pub fn evaluate(expr: &str, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String>;

// color.rs
#[derive(Clone, Copy, Debug, PartialEq, Default)] pub struct Color { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
pub fn parse_color(s: &str) -> Option<Color>;              // "R,G,B[,A]" (components may be formulas) or "RRGGBB[AA]"

// paths.rs
pub fn resolve_path(base_dir: &Path, value: &str) -> PathBuf;   // '\'→'/', '..', abs paths, case-insensitive per component when the exact path is missing
pub fn find_case_insensitive(path: &Path) -> Option<PathBuf>;

// bangs.rs
#[derive(Clone, Debug, PartialEq)]
pub enum Action { Bang { name: String /* lowercase, no '!' */, args: Vec<String> }, Command(String) }
pub fn parse_actions(s: &str) -> Vec<Action>;               // "[!A x][!B "y z"]", bare "!A x", or a plain command
pub fn split_args(s: &str) -> Vec<String>;                  // Rainmeter quoting incl. """..."""

// env.rs
pub struct Environment {
    pub skins_dir: PathBuf, pub settings_dir: PathBuf, pub settings_file: PathBuf,
    pub program_dir: PathBuf, pub fonts_install_dir: PathBuf,
    pub monitors: Vec<Rect>, pub work_areas: Vec<Rect>,   // virtual-desktop coords, index 0 = primary
}
impl Environment { pub fn from_env() -> Environment; pub fn with_root(dir: &Path) -> Environment; }
#[derive(Clone, Copy, Debug, PartialEq, Default)] pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

// parser.rs  (Rainmeter ConfigParser port)
pub trait SectionValues {                                   // implemented by Skin; lets the parser resolve [Measure] refs
    fn section_variable(&self, name: &str, param: Option<&str>) -> Option<String>;
    fn measure_value(&self, name: &str) -> Option<f64>;     // for formulas
}
pub struct NoSectionValues;                                 // returns None for everything
pub struct SkinIdentity { pub config: String, pub root_config: String, pub file: String, pub ini_path: PathBuf }
pub struct ConfigParser { /* merged ini, variables, overrides from !SetOption */ }
impl ConfigParser {
    pub fn load(env: &Environment, id: &SkinIdentity) -> std::io::Result<ConfigParser>; // @Include, [Variables], built-ins
    pub fn section_names(&self) -> Vec<String>;            // file order after includes, excluding Rainmeter/Variables/Metadata
    pub fn read_string(&self, section: &str, key: &str, default: &str, sv: Option<&dyn SectionValues>) -> String; // MeterStyle fallback, #Var#, [Measure] when sv is Some
    pub fn read_float(&self, section: &str, key: &str, default: f64, sv: Option<&dyn SectionValues>) -> f64;     // "(…)" → formula
    pub fn read_int(&self, section: &str, key: &str, default: i64, sv: Option<&dyn SectionValues>) -> i64;
    pub fn read_color(&self, section: &str, key: &str, default: Color, sv: Option<&dyn SectionValues>) -> Color;
    pub fn has_key(&self, section: &str, key: &str) -> bool;
    pub fn get_variable(&self, name: &str) -> Option<String>;
    pub fn set_variable(&mut self, name: &str, value: &str);
    pub fn set_option(&mut self, section: &str, key: &str, value: &str);   // !SetOption override
    pub fn replace_variables(&self, s: &str) -> String;
    pub fn replace_section_variables(&self, s: &str, sv: &dyn SectionValues) -> String;
    pub fn parse_formula(&self, s: &str, sv: Option<&dyn SectionValues>) -> Option<f64>;
}
pub struct OptionReader<'a> { pub parser: &'a ConfigParser, pub section: &'a str, pub sv: Option<&'a dyn SectionValues> }
// OptionReader has string/float/int/bool/color(key, default) shortcuts used by measures and meters.

// timefmt.rs
pub fn format_time(fmt: &str, dt: &chrono::NaiveDateTime, tz_offset_secs: i32) -> String; // MSVC strftime incl. '#' flag

// measures/mod.rs
pub struct MeasureCtx<'a> { pub env: &'a Environment, pub skin_dir: &'a Path, pub update_ms: i64, pub log: &'a mut Vec<String> }
pub trait MeasureImpl {
    fn read_options(&mut self, opt: &OptionReader);
    fn update(&mut self, ctx: &mut MeasureCtx) -> f64;
    fn string_value(&self) -> Option<String> { None }
    fn default_max(&self) -> f64 { 1.0 }
    fn command(&mut self, _args: &str) {}
}
pub struct Measure { pub name: String, /* base options: min/max/invert/average/substitute/if-actions/divider… */ }
impl Measure {
    pub fn new(name: &str, type_name: &str, opt: &OptionReader) -> Measure;   // factory, Plugin=… handled here
    pub fn value(&self) -> f64; pub fn relative_value(&self) -> f64;
    pub fn string_value(&self, auto_scale: u8, scale: f64, decimals: i32, percentual: bool) -> String;
    pub fn min_value(&self) -> f64; pub fn max_value(&self) -> f64;
    pub fn take_actions(&mut self) -> Vec<String>;   // IfActions/OnChange/OnUpdate fired this tick
}

// meters/mod.rs
pub trait MeterImpl {
    fn read_options(&mut self, opt: &OptionReader);
    fn update(&mut self, m: &MeterInputs) -> bool;            // bound measure values; true if changed
    fn auto_size(&self, base: &MeterBase, res: &Resources) -> (f64, f64);
    fn draw(&self, cr: &cairo::Context, base: &MeterBase, res: &Resources);
}

// skin/mod.rs
pub enum AppRequest {
    Activate { config: String, file: Option<String> }, Deactivate { config: String }, Toggle { config: String },
    Refresh { config: Option<String> }, RefreshApp, Move { config: String, x: f64, y: f64 },
    SetDraggable { config: String, value: i32 }, SetClickThrough { config: String, value: i32 },
    SetTransparency { config: String, alpha: u8 }, ZPos { config: String, value: i32 },
    BangOther { config: String, actions: String }, OpenOrRun(String), Resize, Quit,
}
pub struct Skin { /* parser, measures, meters, options, outbox, size */ }
impl Skin {
    pub fn load(env: &Environment, config: &str, file: &str) -> Result<Skin, String>;
    pub fn tick(&mut self);                                     // one Update cycle
    pub fn update_interval_ms(&self) -> i64;
    pub fn size(&self) -> (i32, i32);
    pub fn render(&self, cr: &cairo::Context);
    pub fn render_to_surface(&self) -> cairo::ImageSurface;
    pub fn mouse(&mut self, ev: MouseEvent) -> bool;            // true = handled (suppresses context menu)
    pub fn execute(&mut self, actions: &str);
    pub fn take_requests(&mut self) -> Vec<AppRequest>;
}
pub fn render_skin_png(env: &Environment, config: &str, file: Option<&str>, ticks: u32, out: &Path) -> Result<(), String>;
```

---

### Milestone 1: Workspace + INI/ConfigParser foundation (TDD)

**Files:** create `Cargo.toml` (workspace), `crates/snowmeter-core/{Cargo.toml,src/lib.rs,src/ini.rs,src/formula.rs,src/color.rs,src/paths.rs,src/bangs.rs,src/env.rs,src/parser.rs}`, `crates/snowmeter/{Cargo.toml,src/main.rs}` (stub `fn main` printing version), `crates/snowmeter-core/tests/fixtures/` (tiny skins written by us).

- [ ] Workspace with both crates and all dependencies from the tech stack declared where they'll be used; `cargo build --workspace` clean.
- [ ] `ini.rs` — TDD: tests first for UTF-16LE BOM, UTF-8 BOM, CP-1252 fallback, `;` comments, case-insensitive lookup, whitespace trimming, quote stripping, duplicate-key/section rule (per `skin-format.md` §1), `write_key_value` preserving other lines.
- [ ] `formula.rs` — TDD: port every case from `skin-format.md` §4 test vectors (taken from `MathParser_Test.cpp`): precedence, `^`, `%`, bit ops, `?:`, comparisons, all functions, `pi`/`e`, hex/binary literals, division by zero, measure-name lookup through the closure.
- [ ] `color.rs`, `paths.rs`, `bangs.rs` — TDD with vectors from `skin-format.md` (§5 colors, §10 bang parsing, §12 paths). Paths tests build a temp dir with `Images/Bg.png` and resolve `#@#images\BG.PNG`-style inputs.
- [ ] `env.rs` — `Environment::from_env()` (XDG dirs, `SNOWMETER_CONFIG_DIR` override, monitors default to one 1920x1080 rect until the GTK app fills them) and `with_root(dir)` for tests.
- [ ] `parser.rs` — TDD: @Include (order, nesting, `#@#`/`#SKINSPATH#`/`#CURRENTPATH#` in include paths), `[Variables]`, all built-in variables from `skin-format.md` §3 (paths with trailing `/`), `#Var#`, nested `[#Var]`, escapes `#*Var*#`, section variables only when `sv` is Some, MeterStyle precedence (§6), formulas in `read_float`, `!SetOption` overrides. Fixture skins under `tests/fixtures/m1/`.
- [ ] `cargo test -p snowmeter-core` green, zero warnings; commit "core: INI, formulas, variables, @Include, bangs, paths"; push.

### Milestone 2: Measures (TDD for Time)

**Files:** `src/timefmt.rs`, `src/measures/{mod.rs,base.rs,substitute.rs,ifactions.rs,time.rs,cpu.rs,memory.rs,disk.rs,uptime.rs,calc.rs,string.rs,net.rs,registry.rs,plugin.rs}`.

- [ ] `timefmt.rs` — TDD: the full code table and test vectors from `measures.md` (all codes, `#` flag variants, 2024-03-05 07:08:09 Tuesday etc.), English names by default.
- [ ] `Measure` base per `measures.md` §2: MinValue/MaxValue, Invert, AverageSize, Disabled/Paused, UpdateDivider, Substitute + RegExpSubstitute (pcre2), IfAbove/IfBelow/IfEqual, IfConditionN/IfTrueAction/IfFalseAction (+IfConditionMode), IfMatchN, OnUpdateAction/OnChangeAction, AutoScale/Scale/NumOfDecimals/Percentual string formatting. Unit tests with the `measures.md` vectors.
- [ ] Measures: Time (Format, TimeZone incl. `local`, DaylightSavingTime, TimeStamp/TimeStampFormat), CPU (`/proc/stat`, Processor=N), PhysicalMemory/Memory/SwapMemory (`/proc/meminfo`, Total=1), FreeDiskSpace (`statvfs`; `Drive=C:` and other drive letters → `/`; `Drive=/path` works; Total, Type, Label, IgnoreRemovable), Uptime (`/proc/uptime`, FormatMessage-style `%N!i!` format), Calc (formula with other measures via the lookup closure, Random/LowBound/HighBound/UniqueRandom/UpdateRandom), String, NetIn/NetOut/NetTotal (`/proc/net/dev`, Interface, Cumulative), Registry (stub: 0/"" + log once), Plugin (factory: placeholder 0/"" + log once for unknown plugins; native emulation hooks table for later).
- [ ] Each measure has at least one unit test (system measures: sanity ranges; parse helpers tested with canned `/proc` text).
- [ ] Tests green, zero warnings; commit "core: measures"; push.

### Milestone 3: Skin engine, String/Image meters, headless render

**Files:** `src/skin/{mod.rs,options.rs,mouse.rs,actions.rs}`, `src/meters/{mod.rs,base.rs,string.rs,image.rs}`, `src/render/{text.rs,image.rs,fonts.rs}`, `src/lib.rs` (`render_skin_png`), test `tests/skin_engine.rs` + fixtures `tests/fixtures/m3/`.

- [ ] Meter base per `meters.md` §1: X/Y with `r`/`R` relative positioning, formulas, W/H, Hidden, SolidColor/SolidColor2/GradientAngle, Padding, MeasureName/MeasureNameN binding, MeterStyle, mouse action keys, Group, UpdateDivider, DynamicVariables; skin size from meters (DynamicWindowSize, SkinWidth/Height), `[Rainmeter]` Background/SolidColor/BackgroundMode.
- [ ] String meter per `meters.md` §2 with Pango: Text `%1..%N`, Prefix/Postfix, AutoScale/Scale/NumOfDecimals/Percentual, FontFace/FontSize (pt→px exactly as the note says), FontColor, FontWeight, StringStyle, StringAlign (9 values), StringEffect (Shadow/Border) + FontEffectColor, StringCase, ClipString(+W/H), Angle, InlineSetting/InlinePattern (Face, Size, Color, Weight, Italic, Underline, Strikethrough, Case, CharacterSpacing, Shadow, GradientColor → first colour as fallback).
- [ ] `render/fonts.rs`: Windows→Linux font mapping table used when the family is not installed (Segoe UI/Tahoma/Verdana/Trebuchet MS→"Noto Sans" then "DejaVu Sans"; Arial→Liberation Sans; Times New Roman→Liberation Serif; Courier New→Liberation Mono; Consolas/Lucida Console→DejaVu Sans Mono; Calibri→Carlito/Liberation Sans; Segoe UI Light/Semibold→Noto Sans + weight); load fonts from `@Resources/Fonts` (recursive) and `LocalFontN` via fontconfig FFI `FcConfigAppFontAddFile` + `pango_fc_font_map_config_changed` (link `fontconfig` and `pangoft2`).
- [ ] Image meter per `meters.md` §3: ImageName (+`%1`, Path, missing extension), W/H scaling, PreserveAspectRatio 0/1/2, ScaleMargins, Tile, ImageTint, ImageAlpha, Greyscale, ImageCrop, ImageFlip, ImageRotate; `image` crate decode → premultiplied cairo surface, cache by path.
- [ ] Skin: load (`ConfigParser`, measures/meters in file order, `[Rainmeter]` options), `tick()` order per `skin-format.md` §8, UpdateDivider, OnRefreshAction (first tick), OnUpdateAction, measure actions; `execute()` with all must-have bangs from `skin-format.md` §10 (skin-local ones applied directly, the rest → `AppRequest`); `[Measure]` section variables resolved in actions at execution time; non-bang commands → `AppRequest::OpenOrRun`; `mouse()` hit-testing per `skin-format.md` §9; `!WriteKeyValue` writes the ini.
- [ ] `render_skin_png()` and integration tests: fixture skin with String + Image + Calc renders to PNG with expected size and sampled pixel colours; `!SetVariable` + `DynamicVariables` test (Review Focus 4); broken-fixture test (Review Focus 3).
- [ ] Tests green, zero warnings; commit; push. **Review checkpoint A** (M1–M3) by the controller.

### Milestone 4: Remaining meters + Lua Script + WebParser + plugin emulations

**Files:** `src/meters/{bar.rs,line.rs,roundline.rs,shape.rs,rotator.rs,button.rs,histogram.rs}`, `src/measures/{script.rs,webparser.rs,sysinfo.rs,recycle.rs,power.rs}`.

- [ ] Bar, Line, Roundline, Rotator, Button, Histogram per `meters.md` (exact geometry; unit tests with the note's vectors, e.g. Roundline angles, Line history).
- [ ] Shape: Rectangle (+radii), Ellipse, Line, Arc, Curve, Path (+ `PathN` with LineTo/ArcTo/CurveTo/ClosePath), Combine (Union at least), modifiers Fill Color, Fill LinearGradient/RadialGradient, StrokeWidth, Stroke Color, StrokeLineJoin/Caps, StrokeDashes, Rotate, Scale, Skew, Offset, Extend.
- [ ] Script measure: mlua lua51; ScriptFile, `Initialize()`/`Update()` (return value → number or string), `SKIN:GetVariable/Bang/GetMeasure/GetMeter/MakePathAbsolute/ParseFormula/ReplaceVariables/GetX/GetY/GetW/GetH`, `SELF:GetOption/GetNumberOption`, measure objects `GetValue/GetStringValue/GetRelativeValue/GetMinValue/GetMaxValue/GetName/GetOption/GetNumberOption`, meter objects `GetX/GetY/GetW/GetH/SetX/SetY/SetW/SetH/Show/Hide/GetName/GetOption`, `print` → log, `!CommandMeasure` → run the code string. Errors logged, never crash.
- [ ] WebParser: worker thread with ureq (rustls), Url, RegExp (pcre2), StringIndex, child measures `Url=[Parent]`, UpdateRate, FinishAction, OnConnectErrorAction, Debug, DecodeCharacterReference, Header/UserAgent, Download (to a cache dir). A local test HTTP server (std `TcpListener` thread) for the test.
- [ ] Native plugin emulations for the plugins illustro and common skins use (see `sample-skins.md`): SysInfo (SysInfoType COMPUTER_NAME, USER_NAME, OS_VERSION, IP_ADDRESS, ADAPTER_DESCRIPTION, NUM_MONITORS, …), RecycleManager (Count/Size from `~/.local/share/Trash`, !CommandMeasure OpenBin/EmptyBin), PowerPlugin (`/sys/class/power_supply`). Everything else stays a placeholder.
- [ ] Tests green, zero warnings; commit; push.

### Milestone 5: Settings, registry, .rmskin importer (TDD), compatibility checker

**Files:** `src/settings.rs`, `src/registry.rs`, `src/rmskin.rs`, `src/compat.rs`, tests `tests/rmskin.rs` with fixtures generated in the test (zip built with the `zip` crate at test time).

- [ ] `settings.rs`: read/write `snowmeter.ini` (`[Snowmeter]` global keys incl. `WindowType`, per-config sections with Active, WindowX, WindowY, Draggable, ClickThrough, KeepOnScreen, AlphaValue, SavePosition, LoadOrder per `rmskin-format.md` §4).
- [ ] `registry.rs`: scan skins dir → configs (folder path with `\` separators) and their .ini files in Rainmeter order; `Active=N` mapping.
- [ ] `rmskin.rs` — TDD with fixtures from `rmskin-format.md` §7: footer detection, plain zip, folder import; install `Skins/*` into skins dir (back up existing to `@Backup` per note), keep `VariableFiles` values, Layouts → settings, fonts from `Fonts/` and `@Resources/Fonts` → `fonts_install_dir` + `fc-cache -f <dir>` (ignore failure), plugins → recorded as unsupported, `Load=`/`LoadType` → activate in settings. Returns `ImportReport`.
- [ ] `compat.rs`: `check_config(env, path) -> CompatReport { config, fully_supported: bool, partial: Vec<String>, unsupported_plugins: Vec<String>, notes }`, classification tables for measures/plugins/meters/bangs/options; Display impl printing "Fully supported / Partially supported (missing: …) / Unsupported plugins: …".
- [ ] Tests green; commit; push.

### Milestone 6: GTK app, CLI, IPC, tray

**Files:** `crates/snowmeter/src/{main.rs,app.rs,window.rs,x11.rs,ipc.rs,tray.rs,menu.rs}`.

- [ ] CLI (clap): `snowmeter` (start), `load <config> [file.ini]`, `unload <config>`, `list`, `refresh [config]`, `import <path>`, `check <skin|config|path>`, `render <config> [file.ini] --out f.png [--ticks N]`.
- [ ] IPC: Unix socket `$XDG_RUNTIME_DIR/snowmeter.sock`, one JSON-free text line per request/reply; CLI falls back to editing settings when no instance runs; single-instance guard.
- [ ] Window per skin (see `architecture.md` §1, §9 — the X11 recipe was proven by a probe: set `_NET_WM_WINDOW_TYPE_DESKTOP` + states in `connect_realize` via Xlib, `XMoveWindow` in `connect_map`, CSS `background: transparent`): DrawingArea painting the skin surface, resize on size change, timer per skin `Update`, input region from alpha (>0) after each render, ClickThrough, drag (XQueryPointer root coords, 3px threshold, saves WindowX/WindowY), mouse actions incl. hover/leave/scroll, right-click PopoverMenu (Refresh, Unload, Draggable, Click-through, Keep on screen, Open folder) unless the skin handles RightMouseUp (Ctrl+right-click always menu), AppRequest handling (activate/deactivate/refresh/move/bangs to other configs/open URLs with `xdg-open`), multi-monitor via GDK monitors, WindowX/WindowY formats, KeepOnScreen, AlphaValue (`set_opacity`). Optional feature `wayland-layer-shell` compiles out by default.
- [ ] Tray (ksni): Refresh all, Load/unload configs submenu, Open skins folder, Quit. Failure to register the tray must not stop the app.
- [ ] Manual check: start the app on DISPLAY=:0 with a fixture skin, confirm the window via `xprop`/`xwininfo`, capture it, quit cleanly. Commit; push. **Review checkpoint B** (M4–M6).

### Milestone 7: Bundled skins + docs

**Files:** `skins/Snowmeter/{@Resources/…,Weekday/Weekday.ini,System/System.ini,Disk/Disk.ini}`, `README.md`.

- [ ] Weekday widget: modern, day row (MON…SUN) with the current day highlighted (Time measure `%w`/`%a` + Calc/IfCondition or per-day meters), large date, clock; System (CPU + RAM bars/rings); Disk (used/free bars for `/` and home). Written as plain Rainmeter skins (they must also work on Windows Rainmeter). First run copies them into the skins dir and activates them.
- [ ] README: what it is, build/install, importing Windows skins, compatibility table, prioritized contributor TODO list.
- [ ] Commit; push.

### Verification (controller)

- [ ] Copy `sample-skins/Skins/*` to `/tmp/…/snowmeter-test/skins` (outside the repo), run `snowmeter check` on every skin, load several in the real app, capture small screenshots into `notes/screenshots/`.
- [ ] Final whole-branch review on the most capable model; one fix wave.
- [ ] `notes/REPORT.md`; final commit + push.
