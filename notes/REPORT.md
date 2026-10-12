# Snowmeter — final report

Snowmeter is an MIT-licensed Rust + GTK4 desktop widget app for Linux that loads **unmodified Windows Rainmeter skins** (skin folders, `.rmskin` packages, plain zips).

- **Private GitHub repo:** https://github.com/Filhao2019/snowmeter (visibility: PRIVATE; everything is pushed to `main`).
- **Machine it was built and verified on:** Linux Mint 22.3, Cinnamon on **X11**, GTK 4.14.5, Rust 1.99 (installed with rustup into `~/.cargo`, no sudo).

## Commands to run it when you're back

```bash
# Once per new terminal: make cargo available (rustup also added this to ~/.bashrc and ~/.profile)
. "$HOME/.cargo/env"

cd ~/linux-rainmeter/snowmeter
cargo build --release                       # ~1–2 min the first time
./target/release/snowmeter                  # starts the app; first run installs and shows the bundled widgets
```

In another terminal (the app keeps running):

```bash
cd ~/linux-rainmeter/snowmeter
./target/release/snowmeter list                                  # skins and which are active
./target/release/snowmeter import ~/Downloads/SomeSkin.rmskin     # install a Windows .rmskin (also .zip or a folder)
./target/release/snowmeter check "illustro\Clock"                # compatibility report, no window
./target/release/snowmeter load "illustro\Clock"                 # show a skin (optionally: load "illustro\Disk" "2 Disks.ini")
./target/release/snowmeter unload "illustro\Clock"
./target/release/snowmeter refresh                               # refresh all (or one config)
./target/release/snowmeter render "Snowmeter\Weekday" --out /tmp/w.png   # headless PNG, no window
./target/release/snowmeter quit
```

To try the default Rainmeter skins: `cp -r ~/linux-rainmeter/sample-skins/Skins/illustro ~/.config/snowmeter/skins/` then `snowmeter load "illustro\Clock"` (or import them as a package). Skins live in `~/.config/snowmeter/skins/` (same layout as Rainmeter's `Skins` folder); positions and state in `~/.config/snowmeter/snowmeter.ini`. Right-click a widget for Refresh / Unload / Draggable (lock) / Click-through / Keep on screen / Open folder; Ctrl+right-click always opens that menu. A tray icon offers the same plus a skin list and Quit. Optional: `cargo install --path crates/snowmeter` to get `snowmeter` on your PATH.

## What was verified (with evidence)

| Check | Result |
|---|---|
| `cargo build --release --workspace` | clean, zero warnings |
| `cargo test --workspace` | see "Final numbers" below — all passing |
| `.rmskin` import: illustro packed as a real `.rmskin` (zip + 16-byte `RMSKIN\0` footer + `RMSKIN.ini`) into a test root outside the repo | installed, `Load=` config activated, **7/7 configs "Fully supported"** |
| `snowmeter check` on every `.ini` of `sample-skins` copied to `~/linux-rainmeter/snowmeter-test/root2` (outside the repo) | **8/8 Fully supported**; notes for Windows commands that get translated (`C:\` → file manager on `/`, `taskmgr.exe` → system monitor) |
| Real app on the live X11 desktop with 10 skins (all 7 illustro skins + 3 bundled) | every window `_NET_WM_WINDOW_TYPE_DESKTOP`, 32-bit ARGB, at the exact configured position, behind normal windows; live data (time, CPU, RAM, swap, disk, network rates, trash count, public IP via WebParser) |
| IPC from the CLI to the running app | `list`, `refresh`, `unload`, `load <config> "2 Disks.ini"` (switched variant, kept position), `quit` → app exited, no windows left |
| Screenshots | `notes/screenshots/desktop-live.png` (captured from the 10 live windows, public IP redacted), `bundled-weekday.png`, `bundled-system.png`, `bundled-disk.png` |

## What works

- **Skin engine** (stock Rainmeter 4.5.27 semantics): INI reading incl. UTF-16LE/UTF-8/CP-1252 files, case-insensitive keys, `@Include` (with `#@#`, `#SKINSPATH#`, `#CURRENTPATH#`; cycle-safe), `[Variables]`, `#Var#`, nested `[#Var]`, escapes, all built-in variables, section variables `[Measure]` with selectors (`:%`, `:/N`, `:MaxValue`, `:EscapeRegExp`, …), MeterStyle inheritance, Rainmeter's MathParser (formulas in any numeric option), `[Rainmeter]` options, update loop with `Update`/`UpdateDivider`/`DynamicVariables`, IfActions/IfCondition/IfMatch, OnUpdate/OnChange/OnRefresh/OnClose actions, Substitute/RegExpSubstitute (PCRE2).
- **Bangs:** all must-haves (`!Refresh`, `!SetVariable`, `!SetOption`, `!ToggleMeter/!ShowMeter/!HideMeter` (+Group), `!UpdateMeter/Measure`, `!Redraw`, `!ActivateConfig/!DeactivateConfig/!ToggleConfig`, `!Move`, `!Draggable`, `!ClickThrough`, `!SetTransparency`, `!ZPos`, `!CommandMeasure`, `!WriteKeyValue`, `!Enable/Disable/Pause/UnpauseMeasure`, `!Delay`, mouse-action bangs, …) and non-bang commands (URLs, files and folders via `xdg-open`, `explorer.exe`/`shell:RecycleBinFolder`/`taskmgr.exe` translations).
- **Measures:** Time (every MSVC strftime code incl. `#` flag, TimeZone, DST, TimeStamp), CPU, PhysicalMemory/Memory/SwapMemory, FreeDiskSpace (drive letters map to `/`), Uptime, Calc (incl. Random/UniqueRandom), String, NetIn/NetOut/NetTotal, Loop, Registry (stub), **Lua Script** (Lua 5.1.5 like stock Rainmeter 4.5, the SKIN/SELF/measure/meter API), **WebParser** (HTTP(S) fetch, RegExp, StringIndex children, FinishAction). Native replacements for the **SysInfo, RecycleManager, PowerPlugin, InputText and NowPlaying (MPRIS)** plugins.
- **Meters:** String (fonts, colours, 9 alignments, effects, StringCase, clipping, angle, InlineSetting basics), Image (scaling, PreserveAspectRatio, ScaleMargins, tint, alpha, greyscale, crop, flip, rotate), Bar, Line, Roundline, Shape (Rectangle, Ellipse, Line, Arc, Curve, Path, Combine, gradients, strokes, transforms), Rotator, Button, Histogram, Bitmap; Container, TransformationMatrix, BevelType.
- **Windows compatibility layer:** Windows font names mapped to installed Linux fonts; fonts in `@Resources/Fonts` and `LocalFontN` loaded per skin at runtime; case-insensitive path resolution with `\` → `/`; `.dll` plugins never run (placeholder + one log line, never a crash).
- **Importer:** `.rmskin` with/without footer, plain zip, skin folder; backups to `@Backup`, `VariableFiles`, `MergeSkins`, layouts, fonts → `~/.local/share/fonts/snowmeter/` + `fc-cache`; prints a per-config compatibility report.
- **Desktop app:** transparent widgets behind windows (X11 desktop-type windows), per-pixel click-through (like Rainmeter), dragging with position saving, right-click menu, tray icon, multi-monitor positions incl. Rainmeter's `WindowX/WindowY` formats, `KeepOnScreen`, `AlphaValue`, tooltips, single instance with a Unix-socket IPC.
- **Bundled skins:** Snowmeter\Weekday (clock, date, MON–SUN strip with today highlighted), Snowmeter\System (CPU/RAM/SWAP rings), Snowmeter\Disk (used/free bars) — plain Rainmeter skins, installed and shown on first run.

## Third-party skin test: Mond (by ApexXx-SenSei)

The original `mond__by_apexxx_sensei_dcly2hj.rmskin` (DeviantArt release, Layout package) imports unmodified and all 5 configs check **Fully supported** (after NowPlaying was added). Running live: Clock (Anurati/Quicksand fonts), Player (current MPRIS track, progress, play/pause/next/previous), Weather, Recycle Bin. Two fixes came out of it: the Image meter's `Path` now gets a trailing separator like Rainmeter (`AppendBackslashIfMissing`), and NowPlaying is implemented over MPRIS.
Not Snowmeter issues, but needed for the full look: the "MONDAY" font **Anurati** is not in the package (Windows users install it separately; it was added to the installed skin's `@Resources/Fonts`), and Mond's weather scrapes a Yahoo page that no longer exists (broken on Windows too) — `tools/mond-weather-wttr.py` repoints it at wttr.in (no API key, location by IP) and keeps the original as `Weather.ini.yahoo-original`. Mond's layout also activates its big Settings panel over the clock; it was left unloaded (`snowmeter load "Mond\Settings"` opens it). Mond's files are not in the repo (third-party).

## Skin gallery (added 2026-10-12)

`snowmeter gallery` / `snowmeter install <name>` / tray → "Get skins". 26 entries, 17 good, 9 partial (`skins/README.md` has the full table with credits and licenses):
- **Bundled in `skins/`** (license allows redistribution; each has `LICENSE` + `SOURCE.md`): Snowmeter, illustro, Enigma, ModernGadgets, SilverAzide Gadgets, Wisp, SystemFetch, Cleartext, rainmeter-todo, and Mond as its unmodified original package (CC BY-NC-ND).
- **Downloaded from the author on install** (no license, third-party art/fonts, or too large): BigSur, NThing UI, STRS, DMeloper's Block HUD, Blue Archive, Persona 5 Calendar, Mondtholomew, Lano Visualizer, Monstercat Visualizer, Fountain of Colors, SysDash, Sonder, Monterey, Notch, catppuccin, MD3 Windows.
- Existing installs receive newly bundled skins once (`[Snowmeter] BundledInstalled=`).

Plugin emulations added while making famous skins work (Rainmeter 4.5 plugin → Linux source): NowPlaying, WebNowPlaying, SpotifyPlugin, GPMDP (MPRIS), AudioLevel (`parec` monitor capture + FFT), Win7AudioPlugin (`wpctl`/`pactl`), CoreTemp/SpeedFan/HWiNFO (hwmon), PingPlugin (`ping`), WifiStatus (`nmcli`/`iw`), Process, UsageMonitor, FolderInfo, FileView, ActionTimer, ConfigActive, RunCommand (no shell; cmd/powershell unsupported), Mouse, QuotePlugin, GetActiveTitle (X11), ActiveNet, plus no-op stand-ins for EasyBlur/FrostedGlass and fallback palettes for Chameleon/SysColor/MeterialYou. Lua is now stock 4.5's Lua 5.1.5 (implicit `arg`, `dofile`). The README compatibility table is the current reference; the plugin rows in `notes/measures.md` predate these additions.

Still missing for some skins: PowershellRM, HotKey, MagickMeter, MSIAfterburner, CursorColor, TrayIcon, AppVolume, MediaPlayer.

## What doesn't work / known issues

- **Wayland:** the `wayland-layer-shell` cargo feature is only a stub (the layer-shell dev package isn't installed on this machine, so it could not be built or tested). Wayland sessions get plain windows (no keep-below, no positioning). Workaround: run with `GDK_BACKEND=x11` (XWayland). GNOME Wayland has no layer-shell at all.
- **Plugins** other than SysInfo, RecycleManager, PowerPlugin, InputText and WebParser (e.g. PowershellRM, HotKey, MagickMeter) show a placeholder (value 0 / empty). `snowmeter check` lists them.
- **Not implemented:** SnapEdges, Fade/`OnHover` effects, `AutoSelectScreen`, MaskImage, UseExifOrientation, Justify text wrapping, full gradient text, SVG/TextEdit meters (fork-only), Registry values (stub), `FormatLocale` (English names only), Net `Cumulative` persistence, Lua `ReadTextFile/WriteTextFile`, `SKIN:GetX/GetY` (return 0).
- **Approximations:** text metrics use Pango/FreeType instead of DirectWrite (±1 px for some fonts); skins whose fonts aren't installed use a mapped Linux font, so widths differ slightly (illustro's "Trebuchet MS" → Noto Sans clips one long title, as it would with any wider font). Work area = monitor area (GTK4 doesn't expose work areas).
- **Behaviour differences:** `!SetOption` applies immediately; `#Var#` in actions is expanded when the action runs; `OnCloseAction` doesn't run on refresh; skin size is capped at 8192 px per side and Line `LineCount`/history sizes are clamped; Lua `Update`/`Initialize` calls are aborted after a time budget and each Lua state is limited to 64 MB (a runaway script logs an error instead of freezing the app); `!KeepOnScreen`/`!LoadLayout` bangs are ignored (use the right-click menu / `.rmskin` layouts); `snowmeter check` exits with status 2 when a skin is not fully supported.
- Left for later (minor): the IPC server handles one client at a time; Script measures load lazily on first update; image files are stat'ed every tick.
- `Settings::save` rewrites `snowmeter.ini` as UTF-8 without comments.
- On Cinnamon (muffin), `_NET_WM_STATE_BELOW` isn't reported back by the WM, but the desktop window type already keeps widgets below normal windows.

## Research (Phase 1)

- REA was already available (no `npx rea-agents setup` needed). **REA 6.4.0 with the Ghidra 12.1.4 backend** analysed `Rainmeter.dll` (stock **4.5.27.3934**, 64-bit): measure factory (23 types), meter factory (10), bang dispatcher (104 names), timers, `@Include`/variable handling, Uptime default format, `.rmskin` footer → `notes/binary-evidence.md`.
- `rainmeter-src` turned out to be a development fork (5.0-dev); Snowmeter matches stock 4.5 (what skins target) and treats fork-only features as optional (`notes/decisions.md` #8).
- Notes: `skin-format.md`, `measures.md`, `meters.md`, `rmskin-format.md`, `sample-skins.md`, `architecture.md`, `plan.md`, `decisions.md`, `missing-deps.md`.

## Process notes

- Built with the superpowers workflow adapted for unattended work and a limited token budget (`decisions.md` #6): writing-plans → `notes/plan.md`; parallel research agents; one Sonnet implementer per milestone with TDD for the parser, variables, @Include, formulas, Time codes and importer; review at two checkpoints plus a final review, each followed by a fix wave; controller verification before every "done".
- Two API session limits interrupted the work; it was resumed from committed state each time.
- Missing system packages (no sudo) and workarounds: `notes/missing-deps.md`.

## Final numbers

- 454 tests (unit + integration), 0 failures; `cargo build --workspace --all-targets` and `cargo build --release` with zero warnings.
- ~25,900 lines of Rust in `crates/` (`snowmeter-core` engine + `snowmeter` GTK app), 3 bundled skins, 9 research/design notes.
- Reviews: checkpoint review of M1–M3 (3 Critical / 7 Important fixed) and final review of M4–M7 (2 Critical / 4 Important fixed: Lua runaway guard, clamped buffers, trash-emptying confirmation, WebParser size/timeouts, README accuracy, private IPC socket with peer-UID check). Each fix has a regression test.
- Test material outside the repo: `~/linux-rainmeter/snowmeter-test/` (`root/` = imported illustro + bundled skins with positions, `root2/` = sample-skins folder drop, `pkg/illustro_test.rmskin`). Try it with `SNOWMETER_CONFIG_DIR=~/linux-rainmeter/snowmeter-test/root ./target/release/snowmeter`. Delete the folder whenever you like.
