# Snowmeter

Snowmeter is a desktop customization tool for Linux that reads Windows
[Rainmeter](https://www.rainmeter.net/) skins. It is written in Rust with GTK 4
and Cairo, and it follows the behaviour of stock Rainmeter 4.5: the same
`.ini` skin format, the same measures, meters, variables and bangs, the same
`.rmskin` packages. Skins sit on the desktop as transparent windows and update
live.

Snowmeter is licensed under the GPL-2.0 (see `LICENSE`). It is an independent
project and is not affiliated with, endorsed by or derived from the Rainmeter
project or its authors.

## Bundled skins

Three plain Rainmeter skins ship in `skins/Snowmeter/`. They use only standard
options, so they also run on Windows Rainmeter 4.5. They share one palette in
`@Resources/Variables.inc`.

| Skin | Preview |
|---|---|
| `Snowmeter\Weekday`: clock, full date and a Mon-Sun strip with today highlighted | ![Weekday](notes/screenshots/bundled-weekday.png) |
| `Snowmeter\System`: CPU, memory and swap rings | ![System](notes/screenshots/bundled-system.png) |
| `Snowmeter\Disk`: used and free space for `/` and `/home` | ![Disk](notes/screenshots/bundled-disk.png) |

To use them, copy `skins/Snowmeter` into `~/.config/snowmeter/skins/`, then
`snowmeter load 'Snowmeter\Weekday'`.

## Requirements

- Rust (stable) installed with [rustup](https://rustup.rs/)
- GTK 4.14 or newer with development files
- Debian, Ubuntu and Linux Mint packages:

```
sudo apt install libgtk-4-dev libcairo2-dev libpango1.0-dev libfontconfig-dev build-essential
```

- Optional, for Wayland compositors with layer-shell: `libgtk4-layer-shell-dev`
  (build with `--features wayland-layer-shell`)

## Build and install

```
cargo build --release
target/release/snowmeter            # start the app
cargo install --path crates/snowmeter   # optional
```

## Usage

`snowmeter` with no arguments starts the app. It loads every skin marked
`Active=N` in `snowmeter.ini` (on first start, the bundled skins), opens the
control socket and shows a tray icon. The other commands talk to the running
instance over a Unix socket (`$XDG_RUNTIME_DIR/snowmeter.sock`).

| Command | Running instance | Not running |
|---|---|---|
| `snowmeter load <config> [file.ini]` | loads the skin | marks it active in settings and prints a hint |
| `snowmeter unload <config>` | unloads it | marks it inactive |
| `snowmeter list` | configs and active state | same, read from disk |
| `snowmeter refresh [config]` | refreshes | error "not running" |
| `snowmeter import <file.rmskin or folder>` | runs locally, then asks the instance to refresh | runs locally |
| `snowmeter check <skin>` | prints a compatibility report, no window | same |
| `snowmeter render <config> [ini] --out f.png` | headless render to PNG | same |

Right-click a widget for Refresh, Unload, Draggable, Click-through, Keep on
screen and Open skin folder. A skin's own `RightMouseUpAction` replaces the
menu; Ctrl+right-click always opens it. The tray menu lists active skins and
offers Refresh all, Load skin and Quit.

## Where things live

| What | Path |
|---|---|
| Skins (same layout as Rainmeter's `Skins` folder) | `~/.config/snowmeter/skins/` |
| Settings and per-skin state (`[Config\Name]` sections) | `~/.config/snowmeter/snowmeter.ini` |
| Imported fonts | `~/.local/share/fonts/snowmeter/` |
| Log | `~/.local/state/snowmeter/snowmeter.log` |

## Importing Windows skins

- `snowmeter import MySkin.rmskin` installs a package: skins, fonts, layouts and
  `VariableFiles` are handled, previous versions are backed up, unsafe paths are
  rejected, Windows plugin DLLs are never installed.
- Or drop a skin folder (with its `@Resources`) into `~/.config/snowmeter/skins/`
  and run `snowmeter refresh`.
- `snowmeter check <skin>` reports whether a skin is fully supported, lists
  missing features and unsupported plugins. `import` runs the same check.

## Compatibility

| Area | Supported | Partial | Not supported |
|---|---|---|---|
| Measures | CPU, PhysicalMemory, SwapMemory, Memory, FreeDiskSpace, Uptime, Time, Calc, String, NetIn, NetOut, NetTotal, Registry (stub values), Loop | Plugin measures (see below) | Windows-only measures such as the Windows performance counter plugins |
| Meters | String, Image, Bar, Bitmap, Button, Histogram, Line, Rotator, Roundline, Shape | Shape: gradient gamma is not applied, triangle line caps draw as square | none of the stock 4.5 types is missing |
| Native plugins | WebParser, Script (Lua), SysInfo, RecycleManager (freedesktop Trash), PowerPlugin | SysInfo (some idle and logon time values return 0), Script (no file read/write API, inline `[Script:Func()]` has no skin access) | InputText, NowPlaying, HotKey, other third-party DLLs (shown as placeholders; the skin still loads) |
| Bangs | variables, options, update/redraw/refresh, show/hide/toggle meters and measures, pause/enable/disable measures, CommandMeasure, mouse-action bangs, Delay, Log, Quit, Move/ZPos/Draggable/ClickThrough/Fade, Activate/Deactivate/ToggleConfig, WriteKeyValue | window-position bangs depend on the compositor | accepted and ignored: About, Manage, Debug, SkinMenu, TrayMenu, EditSkin, SetWallpaper, LoadLayout, Blur bangs, SnapEdges, KeepOnScreen and similar Windows window features |
| Variables | `#var#`, `[Measure]`, `[&Measure:]`, built-ins (`#@#`, `#CURRENTPATH#`, `#SCREENAREAWIDTH#` ...), `@Include`, formulas `( )` | | |
| Skin windows | transparent windows, backgrounds with 9-slice margins, dragging, click-through, tooltips text, per-skin position saved | Z-order: Desktop/Below work on X11 and layer-shell; Normal everywhere | Windows blur, Aero effects, snap to edges |
| Packages | `.rmskin` with footer, plain zip, folders, layouts, fonts | `Min version` checks (you may install anyway) | Windows plugin DLLs |
| Drive letters | `C:` maps to `/`; Linux paths work in `Drive=` | other letters | |

The authoritative lists are in `crates/snowmeter-core/src/compat.rs`,
`measures/plugin.rs`, `meters/mod.rs` and `skin/actions.rs`.

## Desktop environments

- **X11** (Cinnamon, MATE, Xfce, KDE, GNOME on Xorg): full support, including
  below-all-windows widgets, positioning and click-through.
- **Wayland on wlroots compositors (Sway, Hyprland ...) and KDE**: build with
  `--features wayland-layer-shell` and install `libgtk4-layer-shell-dev` so widgets
  can sit on the background layer.
- **GNOME Wayland**: the compositor does not let apps place windows or set a
  background layer, so widgets cannot be positioned or kept below other windows.
  Run with `GDK_BACKEND=x11 snowmeter` to use XWayland, which works.

## Contributing: prioritized TODO

1. InputText plugin (native entry overlaid on the skin). `illustro\Google` and
   many search-box skins need it.
2. NowPlaying plugin via MPRIS (D-Bus). The most common third-party plugin in
   music skins.
3. Meter ToolTip display and `Container` support polish (tooltip text is parsed
   but not shown everywhere).
4. Missing window-manager features: snap to edges, keep-on-screen on Wayland,
   multi-monitor `@N` positioning checks.
5. Plugins often used by popular suites: HotKey, FileView, Process, iTunes
   replacements, AudioLevel (PipeWire), Win7Audio, Quote.
6. `ReadTextFile`/`WriteTextFile` in the Lua API and skin access from inline
   `[Script:Func()]` calls.
7. Shape: gamma-correct gradients and triangle caps.
8. Registry measure: map to a sensible Linux store or settings file.
9. A skin manager window (list, load, unload, settings) beyond the tray menu.
10. Packaging: `.deb`, Flatpak, AppImage; CI.

Run `cargo test --workspace` before sending changes. Compare against real
Rainmeter behaviour (stock 4.5) when in doubt.
