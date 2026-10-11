# .rmskin packages, Rainmeter.ini, layouts and fonts

Sources: the Rainmeter C++ source in `rainmeter-src/` (all paths below are relative to it). The source tree is a dev build: `Version.h` holds `0.0.0` and it needs Windows 10 1903+. The Wine install is **Rainmeter 4.5.27.3934 x64**. Its `Rainmeter.dll`, which contains the installer, has the same key strings (`RMSKIN.ini`, `Rainstaller.cfg`, `RMSKIN.bmp`, `MergeSkins`, `VariableFiles`, `MinimumWindows`, `LoadType`, `ArchivePlugins`, `@Vault`, `64bit`), so this package format applies to both. Prefix has no `AppData/Roaming/Rainmeter/Rainmeter.ini` (Rainmeter was never run). `Defaults/` matches `sample-skins/` exactly (`diff -r`).

## 1. Binary format

A `.rmskin` is a normal ZIP (minizip, Deflate, `/` separators, UTF-8 flag) followed by a **16-byte footer** (`Library/DialogInstall.h:71-81`):

```c
enum PackageFlag { Backup = 0x0001 };
struct PackageFooter { __int64 size; BYTE flags; char key[7]; };   // sizeof 16, no padding
```

| Offset (L = file length) | Bytes | Field | Value written by the packager (`DialogPackage.cpp:671-679`) |
|---|---|---|---|
| L-16 .. L-9 | 8 | `size`, int64 **little-endian** | `L-16`, the ZIP length (`_ftelli64` after the ZIP is closed) |
| L-8 | 1 | `flags` | `0x00` |
| L-7 .. L-1 | 7 | `key` | `52 4D 53 4B 49 4E 00` = `"RMSKIN\0"` |

Example (270-byte ZIP): `<zip> 0e 01 00 00 00 00 00 00 | 00 | 52 4d 53 4b 49 4e 00`. Python `zipfile` and the Rust `zip` crate 9.0.1 both open the file with the footer attached; the crate tolerates trailing data (`zip-9.0.1/src/format/find_central_directory.rs:35-56`). Snowmeter should still give its ZIP reader only the bytes `[0, size)`.

**Validation** (`DialogInstall::ReadPackage`, `DialogInstall.cpp:352-570`):
1. Extension, case-insensitive (`:355-389`). `.rmskin` means read the last 16 bytes. If `strcmp(key, "RMSKIN") == 0` (so byte L-1 must be `00`), the package is **New format**. If `size != L-16` it is **rejected** (`:375-378`). If `flags != 0`, then `m_BackupPackage = !(flags & 1)` (`:380-383`), so only a non-zero value with bit 0 clear marks a "backup package" (§3.3). If the key is absent, the format stays **Old** (constructor default, `:39`). `.zip` is always Old. Any other extension is rejected.
2. minizip `unzOpen2_64` finds the central directory from the end, so the footer does no harm.
3. Pass 1, find the options file (`:424-466`). New format: `RMSKIN.ini` (case-insensitive) must be at the **archive root**; entries inside folders are skipped (`:441-445`). Old format: `Rainstaller.cfg` at any depth, and its folder becomes the package root (`:471`).
4. `ReadOptions` (§2). An empty `Name` makes the package invalid.
5. Pass 2, collect components (§3). There must be at least one, otherwise the package is invalid (`:568-569`).
6. Error text (`:97-102`, `Language/en.ini:282-283`): *"Invalid package: <file> ... The Skin Packager tool must be used to create valid .rmskin packages."*

**Plain ZIP without a footer.** Whether named `.zip` or `.rmskin`, it is treated as the **old Rainstaller format**, which needs `Rainstaller.cfg`. A ZIP that contains only `RMSKIN.ini` is therefore *rejected*, so renaming a `.zip` to `.rmskin` fails. A footer with a size mismatch is rejected outright and never retried as Old.

**Old-format key names** (`:450,495,522-533,552-557,575-630,791-828`): `Rainstaller.cfg [Rainstaller]`, `Rainstaller.bmp`, `Merge`, `KeepVar`, `MinRainmeterVer`, `LaunchType` (`Load` means skins, anything else means layout), `LaunchCommand` (a `|` list of `Cfg\File.ini`), `Themes\<n>\*.thm` (installed as `Layouts\<n>\*.ini`), `Addons\<n>\` (to `<Settings>\Addons`), `Fonts\*.ttf` (copied into every packaged skin's `@Resources\Fonts`). A backup package is recognised by a name matching `Backup-%d.%d.%d-%d.%d.rmskin` (`:581-587`).

## 2. `RMSKIN.ini` `[rmskin]`

`DialogInstall.cpp:572-643`. Read by `IniFile`, which accepts UTF-16LE (with or without BOM), UTF-8, and ANSI as a fallback; UTF-16BE is refused (`Common/IniFile.cpp:21-104`). Section and key names are case-insensitive (`:197`).

| Key | Meaning | Default | Src |
|---|---|---|---|
| `Name` | Display name. **Required.** | (invalid) | `:578-579` |
| `Author`, `Version` | Display only; never compared. | `""` | `:589-590` |
| `MergeSkins` | Int; non-zero means overlay instead of backup-and-replace (§3.3). | 0 | `:592` |
| `VariableFiles` | `|` list of paths relative to the Skins dir, e.g. `illustro\@Resources\Vars.inc`; each item trimmed of spaces and tabs (§3.4). The packager only writes it when MergeSkins is off (`DialogPackage.cpp:583-594`). | `""` | `:594-595` |
| `MinimumRainmeter` | Dotted version. If greater than the `FileVersion` of `Rainmeter.dll` (e.g. `4.5.27.3934`), the install is refused: *"Rainmeter %1 or higher is required…"* (`en.ini:289`). The packager defaults it to the current `a.b.c.rev` (`DialogPackage.cpp:99-104,1746`). | no check | `:597-607` |
| `MinimumWindows` | Compared with `major.minor.build` from `GetVersionEx` (`:1320-1328`); refused if greater (`en.ini:290`). The dev packager no longer writes it (`DialogPackage.cpp:572-597`). | no check | `:632-640` |
| `LoadType` | `Skin` (case-insensitive) means activate a skin. **Any other non-empty value** (the packager writes `Layout`) means load a layout. Empty means nothing. | `""` | `:609-630` |
| `Load` | For Skin: a single `Config\Sub\File.ini`, e.g. `illustro\Clock\Clock.ini`. For Layout: the layout folder name. | `""` | `:614-629` |

* Version compare (`:1234-1262`): split on `.`, `_wtoi` each part, missing parts count as 0.
* There are **no** `Plugins`, `Fonts`, `Layouts` or `Skins` keys; components come only from folder names. The packager keeps its own choices in `<Settings>\SkinPackager.ini` (`DialogPackage.cpp:41-46,493-556`), which is not shipped.
* **`RMSKIN.bmp`**: optional, at the root, and must be a Windows BMP (`LoadImage IMAGE_BITMAP`, `:493-504`). The packager only allows 400x60 (`DialogPackage.cpp:2010-2026`). The installer stretches it into a 266x37 DLU header and the dialog grows from 213 to 250 DLU (`:111,152-157,198-221`). It is never installed.

## 3. Archive layout and installation

Entry names are decoded as UTF-8 if GP bit 11 is set, otherwise with the ANSI code page; `/` becomes `\` (`:408-422`). The packager always writes UTF-8 names with `/` (`DialogPackage.cpp:724-747`). Rainmeter does **not** filter `..`. `<SkinPath>` defaults to `Documents\Rainmeter\Skins\` (`Rainmeter.cpp:459-479`). `<Settings>` is `%APPDATA%\Rainmeter\`, or the program dir when portable.

| Archive path | Condition | Installed to | Src |
|---|---|---|---|
| `Skins\<Root>\...` | `<Root>` is not `Backup`/`@Backup`/`@Vault` (case-insensitive, `:1132-1142`). A file directly in `Skins\` is ignored. | `<SkinPath>\<Root>\...` | `:517-521,760-764,786-790` |
| `Layouts\<Name>\…\*.ini` | `.ini` only; `<Name>` is not `Backup`/`@Backup` | `<Settings>\Layouts\<Name>\...`, then the file's `[Rainmeter]` section is **emptied** (header kept; `CleanLayoutFile` `:1112-1116`) | `:522-527,791-808` |
| `Layouts\<Name>\Wallpaper.bmp` and other non-ini files | | **ignored** | same |
| `Plugins\64bit\X.dll` (`32bit` on a 32-bit build) | directly in the arch folder; `X` is not built in | `<Settings>\Plugins\X.dll` | `:534-544,772-784` |
| `Plugins\<any>\X.dll` | `Rainmeter.data [SkinInstaller] ArchivePlugins` (default 1, `:46-48`); runs for every DLL, built-in or unchecked | copy to `<SkinPath>\@Vault\Plugins\X\<FileVersion>\<any>\X.dll` | `:750-758,987-1052` |
| `Fonts\`, `Addons\`, `Themes\` | Old format only (§1) | | `:528-557,812-828` |
| anything else (README, top-level files) | | ignored | |

* **Built-in plugins the installer ignores** (`:1167-1199`): ActionTimer, AdvancedCPU, AudioLevel, CoreTemp, FileView, FolderInfo, InputText, iTunesPlugin, MediaKey, NowPlaying, PerfMon, PingPlugin, PowerPlugin, Process, QuotePlugin, RecycleManager, ResMon, RunCommand, SpeedFanPlugin, SysInfo, UsageMonitor, WebParser, WifiStatus, Win7AudioPlugin, WindowMessagePlugin.
* The engine maps `Measure=Plugin` + `Plugin=<name or path>.dll` onto built-in measures, including `Drag&Drop` and `Mouse`. Some names change: iTunesPlugin→iTunes, PingPlugin→Ping, PowerPlugin→Power, QuotePlugin→Quote, SpeedFanPlugin→SpeedFan, Win7AudioPlugin→Audio, WindowMessagePlugin→WindowMessage, Drag&Drop→DragDrop (`Skin.cpp:2809-2846`, `Rainmeter.cpp:2029-2063`). Snowmeter must do the same mapping. Rainmeter 4.5.27 still ships 18 of them as DLLs.
* **Packager output** (`DialogPackage.cpp:565-693`): `RMSKIN.bmp`?, `RMSKIN.ini`, **one** `Skins\<Root>\` tree (hidden files skipped `:821-825`, empty dirs as entries `:858-864`), `Layouts\<n>\Rainmeter.ini`, and both `Plugins\32bit\X.dll` and `Plugins\64bit\X.dll` (both required, `:1243`). The installer accepts any number of roots and layouts.

### 3.1 Install sequence (`InstallThread`, `:907-947`)

1. Dialog: components are ticked; a plugin whose installed version is the same or newer starts unticked (`:1508-1522`). "Apply included layout"/"Load included skins" is ticked (`:1545-1558`); unticking it clears the load (`:860-865`).
2. Close Rainmeter (`SkinInstaller.cpp:189-212`).
3. Backup phase (§3.3). One extraction pass with `CREATE_ALWAYS`, which **overwrites** and creates directories (`:308-350,713-839`). Then `KeepVariables` if `!MergeSkins && BackupSkins` (`:841-844`).
4. Relaunch Rainmeter with the load action (§3.5). On failure the load is cleared (`:938-939`).

### 3.2 Case

The package root set is a case-sensitive `std::set`, but NTFS is case-insensitive, so `Skins\Foo` and `Skins\foo` merge into one folder on Windows. Snowmeter must canonicalise names case-insensitively.

### 3.3 Backup, overwrite, merge (`:647-692`)

`BackupSkins` defaults to true (`:41`). It can be unticked under Advanced, and is greyed out for merge or backup packages (`:234-241`).

| Mode | Effect on an existing `<SkinPath>\<Root>` |
|---|---|
| Default (no merge, backup on) | Old `Skins\@Backup\<Root>` goes to the Recycle Bin. `Skins\<Root>` is **moved** to `Skins\@Backup\<Root>` (`System::CopyFiles(..,true)` = FO_MOVE, `System.cpp:934-963`). Then the package is extracted into an empty folder, a **clean replace**. User-added files survive only in `@Backup`. |
| `MergeSkins=1`, or backup unticked | Overlay: package files overwrite, other files stay. **No `VariableFiles` handling**, so packaged `.inc` files overwrite the user's. |
| Backup package (flag) | `<Root>` goes to the Recycle Bin, then extract. |

Layouts and plugins are always overwritten in place. `@Backup`, `Backup` and `@Vault` are never listed as skins (`SkinRegistry.cpp:9-13`).

### 3.4 `VariableFiles` (`KeepVariables`, `:949-985`)

For each `F`: both `Skins\@Backup\F` (old) and `Skins\F` (new) must exist, and both must have a non-empty `[Variables]`. For every key present in **both**, the old value is written into the new file. Old-only keys are dropped; new-only keys keep the package default. Key matching is **case-sensitive** (`StringMap`, `Common/Map.h:22`). Other sections and comments are untouched.

### 3.5 Load action (`LaunchRainmeter`, `:1054-1110`)

* Layout: run `Rainmeter.exe !LoadLayout "<Load>"`. At startup that is the only bang allowed on the command line (`Rainmeter.cpp:101-124,580-590`). See §4.3.
* Skin: start Rainmeter, wait for it, then run `[!ActivateConfig "<Load before last \>" "<after last \>"]`, e.g. `!ActivateConfig "illustro\Clock" "Clock.ini"`. This writes `Active=N` and, on first activation, applies the skin's `Default*` options (§4.2). If there is no `\`, nothing is loaded. New format allows only one skin.

## 4. `Rainmeter.ini`

* Location: `<ProgramDir>\Rainmeter.ini` when portable, otherwise `%APPDATA%\Rainmeter\Rainmeter.ini` (`Rainmeter.cpp:225-264`). Companions: `Rainmeter.data` (favourites, installer state), `.log`, `.stats` (`:346-361`). On first run it is copied from `Defaults\Layouts\illustro default\Rainmeter.ini` (`:368-374,947-952`).
* Rainmeter.ini and every loaded layout are **rewritten as UTF-16LE with BOM** (`:2364-2394`). The sample layout is UTF-16LE with BOM and CRLF.
* Duplicate sections or keys: the **first wins** (`ConfigParser.cpp:1695,1763`). Names are case-insensitive. A `;` starts a comment only as the first non-blank character, so there are no inline comments (`Common/IniFile.cpp:222-224`).

### 4.1 `[Rainmeter]` keys (`ReadGeneralSettings`, `:1691-1838`)

`SkinPath` (env-expanded) · `ConfigEditor` · `Language` · `Logging`/`Debug` · `TrayIcon`, `TrayExecuteR/M/DR/DM` · `DisableDragging` · `DisableVersionCheck`/`DisableAutoUpdate`/`DisableRDP` · `SelectedColor` · `NetInSpeed`/`NetOutSpeed` (global NetIn/Out max in bits) · `DesktopWorkArea[@N]` (`L,T,R,B`), `DesktopWorkAreaType` · `NormalStayDesktop` (1) · `HardwareAcceleration` · `DefaultZoom`/`ForceDefaultZoom` (dev build only). The sections `[Rainmeter]` and `[TrayMeasure]` are not configs; sections for configs that don't exist are skipped but kept (`:1799-1813`).

### 4.2 Per-config sections, e.g. `[illustro\Clock]` (`Skin::ReadOptions`, `Skin.cpp:2201-2331`)

These are read by a parser that has no skin (`Skin.cpp:2548-2554`), so the skin's own `#vars#` are unavailable, but formulas and built-ins work. Unqualified monitor variables (e.g. `#SCREENAREAWIDTH#`) use the **primary** monitor (`ConfigParser.cpp` GetMonitorVariable, uses the skin's monitor only if `m_Skin`).

| Key | Values | Default |
|---|---|---|
| `Active` | 0 = unloaded. N≥1 = the N-th `.ini` in the folder, in NTFS order (§5); N greater than the file count is ignored (`Rainmeter.cpp:1820-1825,1156-1167,1339-1345`) | 0 |
| `LoadOrder` | int; activation order, and stacking within the same z-level; ties keep file order (`:1827,1498-1552`) | 0 |
| `WindowX`/`WindowY` | see below | 0 |
| `AnchorX`/`AnchorY` | px (× zoom), `%` of the skin size, `R`/`B` = from the skin's right/bottom (`SkinPosition.cpp:8-19`) | 0 |
| `AlwaysOnTop` | -2 desktop, -1 bottom, 0 normal, 1 top, 2 topmost (`Skin.h:34-38`) | 0 |
| `OnHover` (old `HideOnMouseOver`) | 0 none, 1 hide, 2 fade in, 3 fade out (`Skin.h:59-62`) | 0 |
| `Draggable` · `SnapEdges` · `ClickThrough` · `KeepOnScreen` · `SavePosition` | 0/1 | 1·1·0·1·1 |
| `StartHidden` · `AutoSelectScreen` | 0/1 | 0·0 |
| `AlphaValue` (0-255) · `FadeDuration` (ms) | | 255 · 250 |
| `Zoom` (10-500, dev only) · `Group`/`DragGroup` (`|` lists) | | — |

**WindowX/Y format** (`SkinPosition.cpp:27-134`): `[-]n` logical px from the monitor's left/top. Modifiers: `%` (percent of the monitor extent), `R` on X or `B` on Y (from the right/bottom edge), `@N` (monitor; `@0` = virtual screen; without it, the primary). An `@N` on one axis is copied to the other. Modifiers only count if they come after the last `#`. A formula `( … )` is evaluated first and the text after the last `)` is kept as modifiers, e.g. `(#SCREENAREAWIDTH#-200)R@2` (`ConfigParser.cpp:1539-1559`). Position = origin + (offset or extent−offset) − anchor (`:100-105`). Saved back as an int, or `%.5f%` plus the modifiers (`:60-98`).

**Skin-supplied defaults**: when there is **no section yet** (`Rainmeter.cpp:1241-1242`), the skin's `[Rainmeter]` `DefaultWindowX/Y`, `DefaultAnchorX/Y`, `DefaultAlwaysOnTop`, `DefaultOnHover`(`/HideOnMouseOver`), `DefaultDraggable`, `DefaultSnapEdges`, `DefaultClickThrough`, `DefaultKeepOnScreen`, `DefaultSavePosition`, `DefaultStartHidden`, `DefaultAutoSelectScreen`, `DefaultAlphaValue` and `DefaultFadeDuration` are written to Rainmeter.ini without the prefix (only those defined; `Skin.cpp:2209-2232,2541-2546`). Unloading writes `Active=0` (`Rainmeter.cpp:1281-1285`).

### 4.3 Layouts: `<Settings>\Layouts\<name>\Rainmeter.ini` (+ optional `Wallpaper.bmp`)

A layout is a full copy of Rainmeter.ini. The Manage dialog can save one, optionally dropping `Active=0` sections, adding the wallpaper, or making it empty (`DialogManage.cpp:1871-1925`). Legacy `Themes\*.thm` folders are migrated (`Rainmeter.cpp:1002-1046`). **`LoadLayout`** (`:1919-1987`):
1. Unload all skins.
2. Unless the name is `@Backup`, copy Rainmeter.ini to `Layouts\@Backup\Rainmeter.ini`.
3. **Overwrite Rainmeter.ini with the layout**. Configs not in the layout are inactive.
4. Restore from the backup any non-empty `SkinPath`, `ConfigEditor`, `Logging`, `DisableVersionCheck`, `DisableAutoUpdate`, `Language`, `NormalStayDesktop`, `SelectedColor`, `HardwareAcceleration`, `DefaultZoom`, `ForceDefaultZoom`. `TrayExecute*` are restored only if the layout's are empty (`:1989-1997`).
5. Set `Wallpaper.bmp` as the wallpaper if present.
6. Reload, then activate configs with `Active>0` in `LoadOrder` order.

Because packaged layouts have an empty `[Rainmeter]`, they can't change global settings.

Sample `illustro default`: `[Rainmeter]` (empty); `[illustro\Clock] Active=1 WindowX=(#SCREENAREAWIDTH#-200) WindowY=0`; `[illustro\Disk] Active=2` (= `2 Disks.ini`) `WindowY=176`; `[illustro\System] Active=1 WindowY=68` (same X); `[illustro\Welcome] Active=1 WindowX=50.000000% WindowY=50.000000% AnchorX=50.000000% AnchorY=50.000000%` (centred). The skins are 210 px wide, so X = W−200 overhangs by 10 px and the default `KeepOnScreen=1` pulls them back in. The Y values follow the heights Clock 68 / System 108.

## 5. Config names and `Active=N` (`SkinRegistry.cpp:217-352`)

* A config is the folder path under Skins joined with `\`: `Skins\illustro\Clock\Clock.ini` is config `illustro\Clock`, file `Clock.ini`. The root config is the first component. `#@#` = `<SkinPath>\<root>\@Resources\` (`Skin.cpp:2524`).
* Not configs: `.ini` files at the Skins top level (`:257`); top-level `@Backup`, `Backup`, `@Vault` (case-insensitive); `@Resources` **only directly under a root**, compared case-**sensitively** (`:252`). A folder counts if it or a descendant has an `.ini`. A root that has only sub-configs is a file-less entry. Only `*.ini` files are variants. Lookups are case-insensitive (`:37-104`).
* **File order** = `FindFirstFile` order with no sort (`:227-304`). On NTFS that is an ordinal comparison of **upper-cased** UTF-16 names. Snowmeter must sort by `upcase` code units, not lowercase: `AB.ini` < `A_B.ini`; `1 Disk.ini` < `2 Disks.ini`; `Clock 2.ini` < `Clock.ini` (space 0x20 < `.` 0x2E). `Active=N` is the 1-based index. Refresh rewrites it from the loaded file name (`Rainmeter.cpp:1866-1885,1582-1594`), so Snowmeter should store the file name internally and treat N only as import/export.

## 6. Fonts (`Skin::ReadSkin`, `Skin.cpp:2680-2756`)

1. **Automatic**: every regular file directly in `<root>\@Resources\Fonts\` is loaded, non-recursive, with no extension filter (`:2680-2719`). Rainmeter 4.x needs no `LocalFont` for these.
2. **`LocalFont`, `LocalFont2`, …** from the skin's `[Rainmeter]`, stopping at the **first missing number**. Values are variable-expanded. Each is tried as `<ProgramDir>\Fonts\<v>`, then (if relative) as `<SkinPath>\<config>\<v>`, i.e. relative to the **config** folder (`:2721-2756`, `Skin.cpp:6042-6058`).
3. **Scope**: per-skin collection; nothing is installed system-wide. The installer's "Install fonts to system" toggle (`m_SystemFonts`) is **dead code** (`DialogInstall.cpp:275-277`, never read).
4. **Precedence**: a family that is installed on the system wins. Otherwise the skin collection is searched by **GDI family name** (`Common/Gfx/TextFormat.cpp:547-571`), which is why skins use names like `FontFace=Roboto Light`.
5. **Types**: DirectWrite custom collection (`Common/Gfx/FontCollection.cpp:40-69`), so effectively `.ttf`, `.otf`, `.ttc`, `.otc`. The old installer copied `*.ttf` only.

On Linux, Pango 1.52.1 is present, and `pango_font_map_add_font_file` needs 1.56. Use a per-skin `FcConfig` with `FcConfigAppFontAddFile` for `@Resources/Fonts/*.{ttf,otf,ttc,otc}` and the `LocalFontN` files, then `pango_fc_font_map_set_config` on a per-skin `PangoCairoFontMap`. A simpler v1 is process-wide `FcConfigAppFontAddFile(NULL, …)`. Match fontconfig `family` and `fullname` so GDI-style names resolve. Never write to `~/.local/share/fonts`.

## 7. Snowmeter import algorithm

Suggested paths: settings `$XDG_CONFIG_HOME/snowmeter/Rainmeter.ini` and `…/Layouts/`; skins `SkinPath` or `$XDG_DATA_HOME/snowmeter/Skins/` (with `@Backup`, `@Vault`); plugins stored in `$XDG_DATA_HOME/snowmeter/Plugins/{32,64}bit/`; "Recycle Bin" means `gio trash`.

1. **Classify.** A directory goes to step 9. A file: if the last 16 bytes have `[9..16]=="RMSKIN\0"` and `LE i64 [0..8] == len-16`, it is a strict package (keep `flags`). If the key matches but the size does not, **refuse**. Otherwise it is footerless; judge by content, not extension.
2. **Open the ZIP** over `[0,size)`. Reject encrypted or symlink entries, and cap total size and entry count.
3. **Normalise names**: UTF-8 if bit 11, else strict UTF-8 falling back to CP1252. `\` becomes `/`. Reject `..`, absolute paths and drive letters. Skip `__MACOSX/`.
4. **Options file.** Strict: root `RMSKIN.ini`, or refuse. Footerless: (a) root `RMSKIN.ini`, with a warning; (b) one wrapper folder containing `RMSKIN.ini`, strip it and warn; (c) `Rainstaller.cfg` anywhere, use old keys with its dir as root; (d) heuristic: a `Skins/` dir, or a single top folder with `*.ini` or `@Resources/`, becomes `Skins/<folder>/`; otherwise refuse.
5. **Parse**: require `Name`. `MergeSkins`, and `VariableFiles` split on `|`, trimmed, `\` to `/`. Compare `MinimumRainmeter` against an **emulated version** constant (e.g. `4.5.27.3934`); on failure show Rainmeter's message with "Install anyway". Treat `MinimumWindows` as satisfied (emulate `10.0.22631`) and log it. Backup package if `flags!=0 && !(flags&1)`.
6. **Components** (§3): skin roots (ignored names excluded, grouped case-insensitively and matched to any existing folder's spelling); `Layouts/<n>/**/*.ini`; `Plugins/{32,64}bit/*.dll` minus built-ins; old-format extras. If there are none, refuse.
7. **Dialog**: header BMP, name/author/version, components with Add / Replace / Backup-and-replace. Plugins show "Windows DLL, stored, not loaded" or "native replacement". Load checkbox (default on). Advanced "Backup skins" (default on).
8. **Install**, staged in a temp dir on the same filesystem:
   1. Unload running skins of affected roots.
   2. Default mode: trash `@Backup/<Root>`, then `rename(<Root>, @Backup/<Root>)`. Backup package: trash `<Root>`. Merge or backup off: keep it.
   3. Extract with overwrite, canonicalising each path component case-insensitively (first spelling wins).
   4. Layout `.ini` files only, then empty their `[Rainmeter]`.
   5. Store plugin DLLs; warn for each skin that uses them.
   6. Old format: `Fonts/*.ttf` into each root's `@Resources/Fonts`, `.thm` to `.ini`; ignore `Addons`.
   7. `VariableFiles` (default mode only), per §3.4.
9. **Bare folder**: if it has `Skins/` or `Layouts/`, handle it as an extracted package. If it contains `*.ini` or `@Resources/`, it is one root and is copied to `SkinPath/<name>/` with the default backup rules. If it contains several qualifying sub-folders, import each one.
10. **Rescan** (§5), then load if ticked and successful. Skin: activate config+file, write `Active`, apply `Default*` if the section is new. Layout: `LoadLayout` (§4.3), asking before applying a wallpaper. For a bare folder with no load action, offer the first `.ini` (Snowmeter extra).
11. **Report** what was installed, backed up, skipped, unsafe or unsupported.

### Test fixtures (Python `zipfile` + `struct.pack('<qB7s', n, flags, b'RMSKIN\0')`)

| # | Fixture | Expected |
|---|---|---|
| 1 | `basic.rmskin`: flags 0, `Name/Author/Version/MinimumRainmeter=4.5.0`, `Skins/Basic/Main/Main.ini` | Installed at `SkinPath/Basic/Main/Main.ini`; no load |
| 2 | `nofooter.zip` / `.rmskin` (same bytes as #1, no footer) | Rainmeter: invalid. Snowmeter: install with a warning |
| 3 | `badsize.rmskin` (size = len-17) | Refused as corrupt; not retried as Old |
| 4 | `badkey.rmskin` (`RMSKIX\0`) | Behaves like #2 |
| 5 | `nested.rmskin` (`Pkg/RMSKIN.ini`, with footer) | Refused when strict. A footerless copy is unwrapped with a warning |
| 6 | `noname.rmskin` | Refused |
| 7 | `minver.rmskin` (`MinimumRainmeter=99.0`) | "Rainmeter 99.0 … required" message plus override |
| 8 | `minwin.rmskin` (`MinimumWindows=10.0.99999`) | Installs; logged |
| 9 | `load-skin.rmskin`: `LoadType=Skin`, `Load=Basic\Main\B.ini`, files `A.ini B.ini b_c.ini` | `Basic\Main` loaded with `B.ini`, `Active=2` (NTFS order A, B, b_c) |
| 10 | #9 plus `B.ini` with `DefaultWindowX=100 DefaultWindowY=50 DefaultAlwaysOnTop=1` | New section gets `WindowX=100 WindowY=50 AlwaysOnTop=1` |
| 11 | `layout.rmskin`: `Layouts/L Layout/Rainmeter.ini` (`[Rainmeter] SkinPath=/evil`, `[L\A] Active=1 WindowX=10`) + `Wallpaper.bmp`, `LoadType=Layout`, `Load=L Layout` | Layout installed with an empty `[Rainmeter]` and no wallpaper; old ini copied to `Layouts/@Backup`; user `SkinPath` kept; `L\A` at x=10 |
| 12 | `plugins.rmskin`: `Plugins/32bit/Foo.dll`, `Plugins/64bit/Foo.dll`, `Plugins/64bit/WebParser.dll` | `Foo` stored, not loaded, with a warning; `WebParser` ignored |
| 13 | `fonts.rmskin`: `Skins/F/@Resources/Fonts/T.ttf` + `readme.txt`; top-level `Fonts/O.ttf`; `F/S/S.ini` using T's family | `T` available only to root `F`; `readme.txt` ignored; top-level `Fonts/` ignored |
| 14 | `localfont.rmskin`: `LocalFont=#@#X.otf`, `LocalFont3=…`, no `LocalFont2` | `X.otf` loaded; `LocalFont3` ignored |
| 15 | `vars-v1` then `vars-v2`: v1 `[Variables] Color=red Size=10 Old=1`, user edits to `Color=blue`; v2 `Color=green Size=12 New=5`, `VariableFiles=V\@Resources\Vars.inc` | Result `Color=blue Size=10 New=5`, no `Old`; `@Backup/V` holds the old tree |
| 16 | v2 with `MergeSkins=1` | No backup; `Color=green`; user's extra files stay |
| 17 | v2 without `VariableFiles`; user file `V/mine.txt` | `mine.txt` only in `@Backup/V`; a second install trashes the previous backup |
| 18 | `backupflag.rmskin` (flags `0x02`) | Root trashed then replaced; no `@Backup` |
| 19 | `ignored.rmskin`: `Skins/@Backup/x.ini`, `Skins/@Vault/y.ini`, `Skins/top.ini`, `README.txt`, `Skins/R/R.ini` | Only `R` installed |
| 20 | `zipslip.rmskin`: `Skins/R/../../evil.ini`, `/abs.ini`, a symlink entry | Rejected; nothing written outside SkinPath |
| 21 | `cp1252.zip` (old format, name byte `0xE9`, no UTF-8 flag) | Name decoded as `é` |
| 22 | `case.rmskin`: `Skins/Foo/a.ini`, `Skins/foo/Img/b.png`, existing `SkinPath/FOO/` | All files go into `FOO/`; one backup |
| 23 | `old.zip`: `MySkin/Rainstaller.cfg` (`LaunchType=Load LaunchCommand=Old\A\A.ini`), `MySkin/Skins/Old/A/A.ini`, `MySkin/Fonts/f.ttf` | `Old` installed, `f.ttf` copied to `Old/@Resources/Fonts`, `Old\A` loaded |
| 24 | `utf16.rmskin` (RMSKIN.ini in UTF-16LE with BOM) | Parsed |
| 25 | folder copy of `sample-skins/Skins/illustro` | Root `illustro` with 7 configs; Disk has 2 variants; `@Resources` is not a config |
| 26 | `sample-skins/Layouts/illustro default` applied as a layout | Clock, `2 Disks`, System, Welcome active; Welcome centred; the others at the right edge |
