# Snowmeter architecture

Snowmeter runs unmodified Windows Rainmeter skins on Linux. This document is the
design the code follows. Behavioural details (what each option does) live in the
research notes: `skin-format.md`, `measures.md`, `meters.md`, `rmskin-format.md`,
`sample-skins.md`, `binary-evidence.md`. Those notes are derived from the
Rainmeter source (`rainmeter-src`), which is the source of truth.

## 1. Target environment

Detected with `echo $XDG_SESSION_TYPE $XDG_CURRENT_DESKTOP` → `x11 X-Cinnamon`
(Linux Mint 22.3, 1920x1080, one monitor, GTK 4.14.5, compositing on).

| Session | Window strategy |
|---|---|
| **X11 (any WM)** — primary | Undecorated GTK4 toplevel with an ARGB visual. Before mapping (in `realize`) set `_NET_WM_WINDOW_TYPE = _NET_WM_WINDOW_TYPE_DESKTOP` and `_NET_WM_STATE = BELOW, STICKY, SKIP_TASKBAR, SKIP_PAGER` through Xlib. Position with `XMoveWindow` (GTK4 has no move API). Verified with a probe on this machine: the window is 32-bit, lands at the requested coordinates and stays behind normal windows. |
| **Wayland (wlroots, KDE)** | `gtk4-layer-shell`, `Layer::Bottom`, anchored top-left with margins as the position. Optional cargo feature `wayland-layer-shell` (dev package not installed here). |
| **GNOME Wayland** | No layer-shell protocol. Fallback: ordinary undecorated window (no "keep below" possible; the compositor decides placement). Documented limitation; users can run Snowmeter under XWayland with `GDK_BACKEND=x11`, which gives the X11 path (desktop type is honoured by Mutter for X11 clients). |

Window type is configurable in `snowmeter.ini` (`[Snowmeter] WindowType=desktop|below|normal`)
for window managers that treat desktop windows badly: `below` = normal type +
`_NET_WM_STATE_BELOW`.

## 2. Workspace layout

```
snowmeter/                      (git repo, GPL-2.0)
  Cargo.toml                    workspace
  crates/
    snowmeter-core/             library: everything that does not need GTK
      src/
        lib.rs
        ini.rs                  IniDocument: ordered, case-insensitive sections/keys; encoding detection
        include.rs              @Include expansion
        vars.rs                 variable store + substitution (#Var#, [#Var], [Measure], [&Measure], [\x..])
        formula.rs              MathParser port (Calc formulas, "(...)" option values)
        values.rs               ReadFloat/ReadInt/color/rect parsing helpers
        paths.rs                Windows→Linux path translation, case-insensitive resolution
        bangs.rs                action-string tokenizer → Vec<Action>, Bang enum
        timefmt.rs              MSVC-compatible strftime incl. '#' flag
        settings.rs             snowmeter.ini (per-config state, global options)
        registry.rs             skins folder scan: configs ↔ .ini files
        skin/
          mod.rs                Skin: load, update tick, actions, bang dispatch, render entry point
          options.rs            [Rainmeter] options
          section.rs            option reader: MeterStyle inheritance, variables, formulas, DynamicVariables
          mouse.rs              mouse action options + hit testing
        measures/
          mod.rs                Measure trait, MeasureBase (Min/Max, Invert, Average, Substitute, IfActions…), factory
          ifactions.rs  substitute.rs
          time.rs cpu.rs memory.rs disk.rs uptime.rs calc.rs string.rs net.rs
          registry.rs (stub) script.rs (LuaJIT via mlua) webparser.rs plugin.rs (placeholder + native emulations)
          sysinfo.rs recycle.rs power.rs …   native replacements for common plugins
        meters/
          mod.rs                Meter trait, MeterBase (X/Y/W/H, relative positioning, SolidColor, mouse), factory
          string.rs image.rs bar.rs line.rs roundline.rs shape.rs rotator.rs button.rs histogram.rs
        render/
          text.rs               Pango layout building, StringAlign/Effect/Case/Clip, InlineSetting
          image.rs              image cache (decode with `image` crate → cairo ARGB32), tint/crop/scale
          fonts.rs              Windows font-name mapping, local font loading (fontconfig FFI)
        rmskin.rs               .rmskin/zip/folder importer
        compat.rs               compatibility report (fully / partially / unsupported)
      tests/                    integration tests + fixtures (tiny hand-written skins only)
    snowmeter/                  binary: GTK4 app + CLI
      src/
        main.rs                 clap CLI
        app.rs                  GTK Application, owns SkinWindows, routes cross-skin bangs
        window.rs               SkinWindow: DrawingArea, input region, drag, mouse → skin actions, context menu
        x11.rs                  Xlib helpers (window type/state, move, pointer query)
        layer_shell.rs          (feature wayland-layer-shell)
        ipc.rs                  Unix socket: CLI ↔ running instance
        tray.rs                 StatusNotifierItem tray icon (ksni)
  skins/                        bundled default skins (Snowmeter suite)
  notes/                        research, plan, decisions, report
```

### Why two crates

`snowmeter-core` contains the whole engine **and** Cairo rendering, but no GTK.
That gives:
- fast unit/integration tests with no display;
- a headless renderer: `snowmeter render <config> [file.ini] --out x.png`
  loads a skin, runs N update ticks and writes a PNG. Used for verification,
  screenshots and the compatibility checker.

The binary crate only adds windows, input, tray, IPC and the CLI.

## 3. Core data model

```
Skin
 ├─ paths: SkinPaths { skins_dir, config: "illustro\\Clock", root_config, file, current_path, resources (#@#) }
 ├─ ini: IniDocument                (after @Include expansion; raw, unsubstituted)
 ├─ vars: Variables                 (built-ins + [Variables], mutable via !SetVariable)
 ├─ options: SkinOptions            ([Rainmeter] section)
 ├─ measures: Vec<MeasureSlot>      file order; MeasureSlot { base: MeasureBase, imp: Box<dyn MeasureImpl> }
 ├─ meters:   Vec<MeterSlot>        file order; MeterSlot { base: MeterBase, imp: Box<dyn MeterImpl> }
 ├─ outbox: Vec<AppRequest>         requests the app must handle (move window, activate config, …)
 └─ counters/tick state
```

- **Section reading** (`section.rs`): `SectionReader::read_string(section, key, default)`
  looks up the key in the section, then in each `MeterStyle` (right-most wins? —
  exact precedence from `skin-format.md`), then applies variable substitution
  (`#Var#` always; `[Measure]` section variables only when the section has
  `DynamicVariables=1`). `read_float` evaluates a value wrapped in `( … )` with
  `formula.rs`.
- **Measures**: `trait MeasureImpl { fn read_options(&mut self, r: &OptionReader); fn update(&mut self, ctx: &mut UpdateCtx) -> Value; fn string_value(&self) -> Option<String>; fn command(&mut self, args: &str, ctx) {} }`.
  `MeasureBase` owns the generic behaviour (MinValue/MaxValue, AverageSize,
  Invert, Substitute/RegExpSubstitute, IfActions, OnUpdate/OnChangeAction,
  UpdateDivider, Disabled/Paused, Group).
- **Meters**: `trait MeterImpl { fn read_options(..); fn update(&mut self, measures: &MeasureValues) -> bool; fn natural_size(&self) -> (f64,f64); fn draw(&self, cr: &cairo::Context, base: &MeterBase); }`.
  `MeterBase` owns X/Y/W/H (incl. `r`/`R` relative positioning), Hidden,
  SolidColor/Gradient background, Padding, mouse actions, ToolTip, Group,
  UpdateDivider.
- **Actions/bangs** (`bangs.rs`): action strings are tokenised once into
  `Vec<Action>` where `Action = Bang { name, args } | Command(String)`. The skin
  executes bangs that affect itself; everything else (another config, window
  operations, launching programs) becomes an `AppRequest` in `outbox`, which the
  app drains after every tick/mouse event.

## 4. Update loop

One `glib::timeout_add_local` per skin at the skin's `Update` interval (ms,
default 1000; `-1` = update once). Each tick calls `Skin::tick()`, which follows
Rainmeter's order (see `skin-format.md` §Update loop):

1. update measures in file order (respecting UpdateDivider / Disabled / Paused);
2. run measure actions (IfActions, OnChangeAction, OnUpdateAction);
3. update meters (respecting UpdateDivider), recompute positions/size;
4. redraw into a Cairo `ImageSurface`; the window's `DrawingArea` paints it;
5. skin `OnUpdateAction`;
6. drain `outbox` → app.

Background work (WebParser HTTP, ping) runs on worker threads and publishes
results through `Arc<Mutex<…>>`; measures pick them up on their next update and
then fire FinishAction. Lua runs on the main thread (LuaJIT via `mlua`, vendored —
the same Lua 5.1/LuaJIT runtime Rainmeter embeds, so `bit.*`, `unpack`, etc. work).

## 5. Rendering

- Cairo `ImageSurface` (ARGB32, premultiplied) per skin, size = skin size.
- Text: Pango via `pangocairo`. Font sizes follow Rainmeter's point→pixel
  conversion at 96 DPI regardless of the X server DPI (skins are designed for
  100 % Windows scaling). Font names are mapped (`render/fonts.rs`): if a family
  is not installed, a table maps common Windows fonts to metric-compatible or
  similar Linux fonts (Arial→Liberation Sans, Times New Roman→Liberation Serif,
  Courier New→Liberation Mono, Segoe UI/Tahoma/Verdana/Trebuchet MS→Noto Sans /
  DejaVu Sans, Consolas→DejaVu Sans Mono, Calibri→Carlito, …).
- Skin-bundled fonts: every font file in `@Resources/Fonts` (recursively) plus
  `LocalFontN=` files are registered at load time with
  `FcConfigAppFontAddFile` and Pango's font map is told to reload
  (`pango_fc_font_map_config_changed`). No system install needed for folder
  skins; `snowmeter import` additionally copies fonts to `~/.local/share/fonts/snowmeter/`.
- Images are decoded with the pure-Rust `image` crate (PNG, JPEG, BMP, GIF, ICO,
  TIFF, WebP) and cached per path+mtime.

## 6. Windows-compatibility layer

- **Paths**: values are kept as written; at the point of use `paths::resolve`
  converts `\` to `/`, collapses `..`, and resolves each component
  case-insensitively against the filesystem (cached). `C:\…` absolute paths are
  mapped to the user's home where meaningful, otherwise logged as unresolvable.
- **Commands** (non-bang actions): URLs → `xdg-open`; existing files/folders →
  `xdg-open`; `explorer.exe <path>` → `xdg-open <path>`;
  `shell:RecycleBinFolder` → `xdg-open trash:///`; `notepad.exe`/`write.exe` →
  default text editor; anything else → spawned if found on `PATH`, else logged.
- **Plugins**: `.dll` plugins can never run. `plugin.rs` looks up a table of
  native re-implementations (SysInfo, RecycleManager, PowerPlugin,
  WebParser (built-in in Rainmeter too), …). Unknown plugins become a
  placeholder measure (value 0, empty string) and are logged once; a meter bound
  to a placeholder still renders.
- **Registry**: stub measure (value 0 / empty string), logged.

## 7. Files and folders

| Purpose | Path |
|---|---|
| Skins (same layout as Rainmeter `Skins\`) | `~/.config/snowmeter/skins/` |
| Settings (Rainmeter.ini equivalent) | `~/.config/snowmeter/snowmeter.ini` |
| Per-skin config state | `[<config>]` sections in `snowmeter.ini` using Rainmeter's names (`[illustro\Clock]`, `Active=1`, `WindowX=`, `WindowY=`, `Draggable=`, `ClickThrough=`, …) |
| Imported fonts | `~/.local/share/fonts/snowmeter/` |
| Log | `~/.local/state/snowmeter/snowmeter.log` + stderr |
| IPC socket | `$XDG_RUNTIME_DIR/snowmeter.sock` |

Built-in variables: `#SKINSPATH#` = `~/.config/snowmeter/skins/` (trailing `/`),
`#SETTINGSPATH#` = `~/.config/snowmeter/`, `#PROGRAMPATH#` = directory of the
executable, `#@#` = `<root config>/@Resources/`, `#CURRENTPATH#` = folder of the
loaded .ini (trailing `/`). Paths use `/`; skins that append `\`-separated
fragments still work because of the path resolver.

## 8. Processes, CLI and IPC

`snowmeter` (no args) starts the GTK app: loads every config with `Active=N` in
`snowmeter.ini` (first start: loads the bundled default skins), opens the IPC
socket and the tray icon.

Other subcommands talk to the running instance over the Unix socket (one
line-based request, one reply):

| Command | Running instance | Not running |
|---|---|---|
| `load <config> [file.ini]` | loads it | marks Active in settings, prints a hint |
| `unload <config>` | unloads | marks inactive |
| `list` | configs + active state | same (from disk) |
| `refresh [config]` | refreshes | error "not running" |
| `import <file.rmskin\|folder>` | — runs locally, then asks instance to refresh registry | runs locally |
| `check <skin>` | — runs locally (compat report, no window) | same |
| `render <config> [ini] --out f.png` | — headless | same |

Widget right-click menu (GTK `PopoverMenu`): Refresh, Unload, Draggable (lock),
Click-through, Keep on screen, Open skin folder. Tray menu: list of active
configs, Refresh all, Load skin… (submenu of configs), Quit. As in Rainmeter, a
skin's own `RightMouseUpAction` replaces the menu; Ctrl+right-click always opens it.

## 9. Input and window behaviour

- Click-through on transparent pixels (Rainmeter layered-window semantics):
  after each redraw the input region is rebuilt from the alpha channel (pixels
  with alpha > 0), so `SolidColor=0,0,0,1` makes an area clickable exactly as on
  Windows. `ClickThrough=1` sets an empty input region.
- Dragging: left-button press records pointer root position (XQueryPointer)
  and window position; motion beyond 3 px starts a drag and moves the window with
  `XMoveWindow`; mouse-up after a drag does not fire `LeftMouseUpAction`; the new
  position is written to `snowmeter.ini`. Disabled when `Draggable=0`.
- Multi-monitor: positions are virtual-desktop coordinates; `WindowX/Y` formats
  (`%`, `R`, `B`, `@N`) are resolved against GDK monitor geometry;
  `KeepOnScreen=1` clamps to the containing monitor.

## 10. Testing strategy

- Unit tests next to the code for ini, include, vars, formula, values, bangs,
  timefmt, paths, rmskin, compat.
- Integration tests in `crates/snowmeter-core/tests/` load small fixture skins
  written for this project and assert measure values / meter geometry / rendered
  pixel samples via the headless renderer.
- Verification on the desktop: real windows on DISPLAY :0, captured with
  `xwd`/GDK window capture into `notes/screenshots/` (small PNGs).
