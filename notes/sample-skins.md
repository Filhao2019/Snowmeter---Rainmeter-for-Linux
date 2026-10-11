# illustro sample skins: per-skin analysis and feature inventory

Source: `sample-skins/Skins/illustro/` (`Defaults/` is byte-identical). There are 8 `.ini` files in 7 configs, plus the layout `Layouts/illustro default/Rainmeter.ini`. Engine citations refer to `rainmeter-src/Library/`.

## File facts that matter for the parser

* Every skin `.ini` is **ASCII with CRLF and no BOM**, except `Welcome/Welcome.ini`. That file is **Windows-1252**: byte `0xBB` = `»` in the `meterLink1-4` Text (`» Finding Skins`). It is invalid UTF-8, so the reader must fall back to CP1252, as `Common/IniFile.cpp:79-104` does.
* The layout is **UTF-16LE with BOM**.
* Comments are whole lines starting with `;`. Many lines are comments, and there are no inline comments.
* Images: `@Resources/Background.png` is 210x68 RGBA and is used by 7 skins. `Welcome/Background.png` is 500x282 (8-bit palette) and is used by Welcome through a relative `ImageName`.
* `[Metadata]` (Name, Author=poiru, Information, License=CC BY-NC-SA 3.0, Version=1.0.0) is present in all 8 files. The engine skips it, together with `[Rainmeter]` and `[Variables]`, when creating sections (`Skin.cpp:2800-2802`). It is for the Manage dialog only.

## Shared structure (identical in all files unless noted)

* `[Rainmeter]`: `Update=1000`, `Background=#@#Background.png`, `BackgroundMode=3` (scaled 9-slice; `Skin.h:50-54`, drawn at `Skin.cpp:3116-3155`), `BackgroundMargins=0,34,0,14` (L,T,R,B). Welcome has only `Update=1000`.
* `[Variables]`: `fontName=Trebuchet MS`, `textSize=8` (Welcome: 9), `colorBar=235,170,0,255`, `colorText=255,255,255,205`.
* **Styles** (applied through `MeterStyle=`):
  * `styleTitle`: `StringAlign=Center` (Welcome: Left), `StringCase=Upper`, `StringStyle=Bold`, `StringEffect=Shadow`, `FontEffectColor=0,0,0,50`, `FontColor=#colorText#`, `FontFace=#fontName#`, `FontSize=10`, `AntiAlias=1`, `ClipString=1`.
  * `styleLeftText` / `styleRightText`: `StringAlign=Left` / `Right`, `StringCase=None`, `Bold`, `Shadow`, `FontEffectColor=0,0,0,20`, `FontColor`, `FontFace`, `FontSize=#textSize#`, `AntiAlias=1`, `ClipString=1`.
  * `styleBar`: `BarColor=#colorBar#`, `BarOrientation=HORIZONTAL`, `SolidColor=255,255,255,15`.
  * `styleSeperator` (misspelled in the source): `SolidColor=255,255,255,15`.
* **Grid**:
  * `meterTitle` (String, styleTitle) is at `X=100 Y=12 W=190 H=18`. With center alignment, X is the centre, so the box spans 5..195 (`MeterStringBase.cpp:380-399`).
  * Rows use a left label at `X=10 W=190 H=14` and a right value at `X=200 Y=0r W=190 H=14` (right-aligned, so it spans 10..200; `0r` means the same Y as the previous meter).
  * Under each row is a 1-px Bar or Image line at `X=10 W=190 H=1`.
* **Window size** (`Skin::ResizeWindow`, `Skin.cpp:2945-3015`): `w = max(max meter right + margin.R, bg.w)` and `h = max(max meter bottom + margin.B, bg.h)`. That gives Clock, 1 Disk, Google and Recycle Bin 210x68; 2 Disks 210x88; System and Network 210x108; Welcome 500x282.

## Per-skin breakdown

### `illustro\Clock` (`Clock.ini`): shows the time, weekday and date
| Section | Type | Options |
|---|---|---|
| measureTime | Measure=Time | `Format=%H:%M` |
| measureDate | Measure=Time | `Format=%d.%m.%Y` |
| measureDay | Measure=Time | `Format=%A` (full weekday name) |
| styleTitle, styleLeftText, styleRightText, styleSeperator | styles | see above |
| meterTitle | Meter=String | styleTitle, `MeasureName=measureTime`, `Text=%1` |
| meterDay | Meter=String | styleLeftText, `MeasureName=measureDay`, X=10 Y=40, `Text=%1` |
| meterDate | Meter=String | styleRightText, `MeasureName=measureDate`, X=200 `Y=0r`, `Text=%1` |
| meterSeperator | Meter=Image | styleSeperator (`SolidColor`), X=10 Y=52 W=190 H=1 |

### `illustro\Disk`: `1 Disk.ini` (Active index 1) and `2 Disks.ini` (index 2, used by the layout). Shows used/total space and a bar per drive
| Section | Type | Options |
|---|---|---|
| [Variables] | | `disk1=C:` (2 Disks adds `disk2=D:`) |
| measureTotalDisk1 / measureTotalDisk2 | Measure=FreeDiskSpace | `Drive=#diskN#`, `Total=1`, `UpdateDivider=120` |
| measureUsedDisk1 / measureUsedDisk2 | Measure=FreeDiskSpace | `Drive=#diskN#`, `InvertMeasure=1` (free inverted = used), `UpdateDivider=120` |
| styleTitle, styleLeftText, styleRightText, styleBar | styles | |
| meterTitle | String | `Text=Disk` / `Disks` (shown as DISK(S) via StringCase=Upper) |
| meterLabelDiskN | String | styleLeftText, `Text=#diskN#\` (displays `C:\`); in 2 Disks, `meterLabelDisk2` also has `LeftMouseUpAction=["#disk2#\"]` |
| meterValueDiskN | String | styleRightText, `MeasureName=measureUsedDiskN`, `MeasureName2=measureTotalDiskN`, `Text=%1B/%2B used`, `NumOfDecimals=1`, `AutoScale=1`; Disk1 has `LeftMouseUpAction=["#disk1#\"]` (opens the drive in Explorer) |
| meterBarDiskN | Meter=Bar | styleBar, `MeasureName=measureUsedDiskN`; Y=52 (Disk2: label Y=60, bar Y=72) |

### `illustro\Google` (`Google.ini`): click "Search..." to type a query, which opens a Google search in the browser
| Section | Type | Options |
|---|---|---|
| measureInput | Measure=Plugin, `Plugin=InputText` (mapped to the built-in `InputText`, `Skin.cpp:2817-2846`) | `SolidColor=20,20,20,255`, `StringAlign=Left`, `StringCase`, `StringStyle=Bold`, `StringEffect`, `FontEffectColor`, `FontColor=#colorText#`, `FontFace`, `FontSize=#textSize#`, `X=10 Y=38 W=188 H=17` (box geometry relative to the skin), `FocusDismiss=1`, `DefaultValue=""`, `Command1=["http://www.google.com/search?q=$UserInput$"]`, `UpdateDivider=86400`. InputText ignores `StringCase`, `StringEffect` and `FontEffectColor`; it supports DefaultValue, X, Y, W, H, FontFace, FontSize, FontColor, SolidColor, StringStyle, StringAlign, InputLimit, InputNumber, Password, FocusDismiss and TopMost (`MeasureInputText.cpp:88-106`) |
| styleTitle, styleLeftText, styleSeperator | styles | |
| meterTitle | String | `Text=Google` |
| meterSearch | String | `Text=Search...`, `LeftMouseUpAction=[!CommandMeasure measureInput "ExecuteBatch 1"]` |
| meterSeperator | Image | Y=52 |

### `illustro\Network` (`Network.ini`): shows the public IP and upload/download speed with bars
| Section | Type | Options |
|---|---|---|
| [Variables] | | `maxDownload=10485760`, `MaxUpload=10485760` (bits). The variable is defined as **`MaxUpload`** but used as **`#maxUpload#`**, so variable names must be case-insensitive |
| measureIP | Measure=WebParser | `URL=https://checkip.amazonaws.com/`, `UpdateRate=14400` (× Update, every 4 h; default 600), `RegExp=(?s)^(.*)$`, `StringIndex=1`, `Substitute="":"N/A"` |
| measureNetIn | Measure=NetIn | `NetInSpeed=#maxDownload#`, a per-measure fallback for `MaxValue` in **bits**, divided by 8 unless `UseBits` (`MeasureNet.cpp:203-268`); `Interface` defaults to BEST |
| measureNetOut | Measure=NetOut | `NetOutSpeed=#maxUpload#` |
| styleTitle, styleLeftText, styleRightText, styleBar, styleSeperator | styles | |
| meterTitle / meterIPLabel / meterIPValue | String | `Text=Network`, `Text=IP Address`, `MeasureName=measureIP` with `Text=%1` |
| meterSeperator | Image | Y=52 |
| meterUploadLabel/Value, meterDownloadLabel/Value | String | `Text=%1B/s`, `NumOfDecimals=1`, `AutoScale=1`, measures NetOut / NetIn |
| meterUploadBar / meterDownloadBar | Bar | NetOut at Y=72, NetIn at Y=92 |

### `illustro\Recycle Bin` (`Recycle Bin.ini`; the config name contains a space): shows the item count and size; click to open, right-click to empty
| Section | Type | Options |
|---|---|---|
| measureBinItems | Measure=RecycleManager | `RecycleType=Count`, `UpdateDivider=3` |
| measureBinSize | Measure=RecycleManager | `RecycleType=Size`, `UpdateDivider=3` |
| meterTitle | String | `Text=Recycle Bin`, `LeftMouseUpAction=[!CommandMeasure measureBinItems "OpenBin"]`, `RightMouseUpAction=[!CommandMeasure measureBinItems "EmptyBin"]`, `ToolTipText=Left-click to open Recycle Bin#CRLF#Right-click to empty` |
| meterBinItems | String | `MeasureName=measureBinItems`, `Text=%1 items` |
| meterBinSize | String | `MeasureName=measureBinSize`, `Text=%1B`, `AutoScale=1` |
| meterSeperator | Image | Y=52 |

The RMB action means right-click on the title **does not open the context menu**; Ctrl+right-click still does (`Skin.cpp:5312-5317`). Commands: `OpenBin`, `EmptyBin` (with confirmation), `EmptyBinSilent` (`MeasureRecycleManager.cpp:198-239`).

### `illustro\System` (`System.ini`): CPU, RAM and swap percentages with bars; click the title for Task Manager
| Section | Type | Options |
|---|---|---|
| measureCPU | Measure=CPU | `Processor=0` (average of all cores; max 100; `MeasureCPU.cpp:50-72`) |
| measureRAM | Measure=PhysicalMemory | `UpdateDivider=20`; value = TotalPhys−AvailPhys, max TotalPhys (`MeasurePhysicalMemory.cpp:12-41`) |
| measureSWAP | Measure=SwapMemory | `UpdateDivider=20`; value = TotalPageFile−AvailPageFile (the **commit charge**), max = commit limit (`MeasureVirtualMemory.cpp:12-43`) |
| meterTitle | String | `Text=System`, `LeftMouseUpAction=["taskmgr.exe"]`, `ToolTipText=Open Task Manager` |
| meterValueCPU | String | `Text=%1%` (the measure is already 0-100, no decimals) |
| meterValueRAM / meterValueSWAP | String | `Text=%1%`, `Percentual=1` (100×relative value) |
| meterLabel* / meterBar* | String / Bar | rows at Y=40/60/80, bars at Y=52/72/92 |

### `illustro\Welcome` (`Welcome.ini`): a 500x282 welcome panel with text and four links
No `Background=` is set, so the mode becomes BGMODE_COPY: transparent except for the meters (`Skin.cpp:2612-2622`).

| Section | Type | Options |
|---|---|---|
| meterBackground | Meter=Image | `ImageName=Background.png` (relative to the **config folder**, `GeneralImage.cpp:348`), X=0 Y=0, sized from the image |
| meterMidLine | Image | `SolidColor=255,255,255,80`, W=1 H=200 X=245 Y=60 |
| meterTitle | String | styleTitle (Left), X=20 Y=18 W=400 H=18, `FontSize=11` override, `Text=Welcome to Rainmeter!` |
| meterIllustroTitle / meterLinksTitle | String | `FontColor=255,217,120,255`, `FontSize=10` |
| meterIllustroLine / meterLinksLine | String | W=225, H=175 / H=80, `FontSize=9`, long wrapped text containing `#CRLF##CRLF#`. ClipString=1 with fixed W and H means **word wrap plus clipping** |
| meterLink1..4 | String | `FontColor` 255,217,120 / 255,174,69, `Text=» …`, `LeftMouseUpAction=["http://rainmeter.net/discover"]`, `[".../manual/getting-started"]`, `[".../manual"]`, `["http://rainmeter.net/forum"]` |

### Layout `illustro default`
`[Rainmeter]` is empty. Clock (`Active=1`), Disk (`Active=2`, so `2 Disks.ini`), System and Welcome are listed; **Google, Network and Recycle Bin are absent, so inactive**. It uses `WindowX=(#SCREENAREAWIDTH#-200)` (a formula with a built-in monitor variable, primary monitor) with WindowY 0 / 176 / 68. Welcome uses `WindowX/Y=50.000000%` and `AnchorX/Y=50.000000%` (centred). Details are in `rmskin-format.md` §4.

## Feature inventory

Skin abbreviations: Cl=Clock, D1=1 Disk, D2=2 Disks, G=Google, N=Network, R=Recycle Bin, S=System, W=Welcome, L=layout.

| Feature | Used by | What Snowmeter must support (Linux data source) |
|---|---|---|
| INI encodings: ASCII/CP1252 skins, UTF-16LE layout | W (CP1252), L | Detect BOM/UTF-16, try UTF-8, fall back to CP1252 |
| `;` comment lines, sections, first duplicate wins | all | INI parser (rmskin-format §4) |
| `[Metadata]` | all | Parse for a "skin info" UI; never create a section for it |
| `[Variables]`, `#var#`, **case-insensitive names** | all; N relies on `MaxUpload` vs `#maxUpload#` | Variable table keyed by upper-case name |
| `#@#` | all but W | `<SkinPath>/<root>/@Resources/` with a trailing separator; use `/` and case-insensitive file resolution |
| `#CRLF#` | R, W | Expands to `\n` (`ConfigParser.cpp:242`) |
| `#SCREENAREAWIDTH#`, `( )` formulas, `%`, AnchorX/Y | L | Monitor geometry from GDK (logical px); formula parser; SkinPosition rules |
| `Active=N` (incl. N=2 = `2 Disks.ini`), configs with spaces | L, D, R | NTFS-order file list (upper-case ordinal sort) |
| `Update=1000`, `UpdateDivider` (3, 20, 120, 86400) | all | Per-skin timer; measure updates every N ticks |
| `Background` + `BackgroundMode=3` + `BackgroundMargins` | all but W | 9-slice scaled PNG (Cairo); window size rule from ResizeWindow |
| No Background (BGMODE_COPY) | W | Fully transparent window: an ARGB/RGBA visual and a compositor; input region from painted pixels |
| `MeterStyle` | all | Style inheritance (meter options override the style) |
| Meter=String: `Text` with `%1`/`%2`, `MeasureName`/`MeasureName2`, `StringAlign` (X anchor shift), `StringCase=Upper`, `StringStyle=Bold`, `StringEffect=Shadow` + `FontEffectColor`, `FontColor` (RGBA), `FontFace`, `FontSize` (points), `AntiAlias`, `ClipString=1` (ellipsis/wrap in a fixed box) | all | Pango layout; shadow = text offset by 1 px in the effect colour; upper-casing locale-independent |
| `NumOfDecimals`, `AutoScale=1` (1024 steps, suffix `" k"`, `" M"`, `" G"`, `" T"`, so the output is `12.3 GB`), `Percentual=1` | D1, D2, N, R, S | `Measure::GetFormattedValue`/`GetScaledValue` (`Measure.cpp:575-635`); default decimals 0 unless autoscaled (`MeterString.cpp:70`) |
| Relative position `Y=0r` | all but W | Relative to the previous meter's top-left |
| Meter=Image: `SolidColor` fill, `ImageName` (relative path), W/H defaulting to the image size | Cl, G, N, R, W | PNG via gdk-pixbuf; relative to the config folder; case-insensitive lookup |
| Meter=Bar: `BarColor`, `BarOrientation=HORIZONTAL`, `SolidColor` background | D1, D2, N, S | Fill fraction = the measure's relative value |
| `ToolTipText` (multi-line) | R, S | GTK tooltip on the meter rect |
| `LeftMouseUpAction`, `RightMouseUpAction` (suppresses the context menu; Ctrl overrides) | D1, D2, G, R, S, W | Hit-testing per meter; hand cursor over meters with actions (`MouseActionCursor`, default on) |
| Quoted run command `["…"]` | D1, D2, S, W, G (Command1) | `RunCommand`/`RunFile` = ShellExecute (`CommandHandler.cpp:513-579`). Linux: URLs and folders via `gio open` / `xdg-open` as a single argv element; executables via the alias table below |
| `!CommandMeasure <measure> "<args>"` | G, R | Dispatch to the measure's `Command()` |
| Measure=Time, `Format` = C `strftime` | Cl | `%A` gives **English** names by default (C locale unless `FormatLocale`, `MeasureTime.cpp:436-453`); use chrono with the C locale |
| Measure=FreeDiskSpace: `Drive`, `Total`, `InvertMeasure` | D1, D2 | `statvfs`: total = `f_blocks*f_frsize`, free = `f_bavail*f_frsize` (DiskQuota=1 means caller-available bytes, `MeasureDiskSpace.cpp:87-94`); `max = total`; an unknown drive gives value 0 and max 0 |
| Measure=CPU `Processor=0` | S | `/proc/stat` aggregate `cpu` line: busy delta / total delta × 100 |
| Measure=PhysicalMemory | S | `/proc/meminfo`: `MemTotal − MemAvailable`, max `MemTotal` |
| Measure=SwapMemory | S | Windows value is the commit charge. Linux: `SwapTotal − SwapFree`, max `SwapTotal`; or `Committed_AS`/`CommitLimit` for fidelity. Guard SwapTotal=0 (range 0 means relative value 0) |
| Measure=NetIn/NetOut, `NetInSpeed`/`NetOutSpeed` | N | `/sys/class/net/<if>/statistics/{rx,tx}_bytes` deltas per second; `Interface=BEST` = the default-route interface from `/proc/net/route`; max = bits/8 |
| Measure=WebParser: `URL`, `UpdateRate`, `RegExp` (PCRE), `StringIndex`, `Substitute` | N | Async HTTP (rustls); a PCRE-compatible regex (`fancy-regex`/`pcre2`); the response keeps the trailing `\n` inside `(.*)` |
| `Substitute="":"N/A"` | N | Empty string → `N/A` |
| Measure=RecycleManager `Count`/`Size`; `OpenBin`/`EmptyBin` | R | See the Recycle Bin row below |
| Measure=Plugin `Plugin=InputText`; `ExecuteBatch 1`, `$UserInput$`, `DefaultValue`, `FocusDismiss`, box X/Y/W/H/colours/fonts | G | Map to native InputText: a borderless GTK entry overlaid at skin coordinates; Enter commits and runs Command1 with `$UserInput$` replaced; Esc or focus loss (FocusDismiss) cancels |
| Font `Trebuchet MS` | all | **Not installed here** (`fc-match` → Noto Sans). Use the ttf-mscorefonts package or a fontconfig alias; layout metrics differ, and ClipString hides overflow |
| `AlwaysOnTop`, `Draggable`, `SnapEdges`, `KeepOnScreen`, `SavePosition` (defaults) | L, all | Defaults matter because KeepOnScreen pulls back the 10-px overhang at `WindowX=W−200` |

## Windows-specific items and Linux translations

| Item | Where | Windows behaviour | Proposed Linux translation |
|---|---|---|---|
| `disk1=C:`, `disk2=D:` in `Drive=` | D1, D2 | `GetDiskFreeSpaceEx("C:\")` | **Drive-letter map** in Snowmeter settings: `C:` → `/` by default, `D:` → the first other mounted real filesystem (e.g. `/home` if separate, else `/media/$USER/*` or `/mnt/*`), user-editable. Also accept POSIX paths directly (`Drive=/home`). Unmapped letters give 0, like a missing drive on Windows |
| `Text=#disk1#\` | D1, D2 | Shows `C:\` | Leave the text alone (do not rewrite skin text) |
| `LeftMouseUpAction=["C:\"]` | D1, D2 | Explorer opens the drive | Map the drive letter to its mount point, then `gio open <path>`. In general, translate `X:\rest` paths in run commands through the same map, converting `\` to `/` with case-insensitive resolution |
| `["taskmgr.exe"]` | S | Task Manager | **Executable alias table**: `taskmgr.exe` → first available of `gnome-system-monitor` (present here), `plasma-systemmonitor`, `xfce4-taskmanager`, `mate-system-monitor`, or a terminal with `htop`. Other common ones: `explorer.exe` → `xdg-open ~`, `control.exe` → `cinnamon-settings`/`gnome-control-center`, `notepad.exe` → `$VISUAL`/`xdg-open` |
| RecycleManager Count/Size | R | `SHQueryRecycleBin` over all drives (items, bytes) | freedesktop Trash: `$XDG_DATA_HOME/Trash/files/` (here `~/.local/share/Trash`) plus `$topdir/.Trash/$UID/files` and `$topdir/.Trash-$UID/files` on other mounts. Count = top-level entries in `files/`; Size = recursive byte sum (use `directorysizes` when present). Refresh on a `gio` file monitor rather than polling, like `HasRecycleBinChanged` |
| `OpenBin` | R | `explorer.exe /N,::{645FF040-5081-101B-9F08-00AA002F954E}` | `gio open trash:///` |
| `EmptyBin` / `EmptyBinSilent` | R | `SHEmptyRecycleBin` with or without a confirmation dialog | Show Snowmeter's own GTK confirm dialog, then `gio trash --empty`; the Silent variant skips the dialog |
| `["http://…"]` links, Google `Command1` | W, G | ShellExecute opens the browser | `gio open <url>` as a single argv element (no shell). `$UserInput$` is inserted raw, as Rainmeter does; optionally percent-encode spaces |
| InputText (Windows Edit control) | G | A native edit box over the skin | GTK4 popover or undecorated window with an `Entry`, placed at skin X/Y (scaled), `SolidColor` background with alpha, font from FontFace/Size/Style |
| `Measure=SwapMemory` semantics | S | Commit charge (RAM + pagefile) | `SwapTotal−SwapFree` (matches the label); document the difference |
| `Interface=BEST`, NetIn/Out | N | `GetIfTable2` best interface | Default route from `/proc/net/route`, counters from `/sys/class/net` |
| `Trebuchet MS` | all | Windows core font | fontconfig fallback or mscorefonts; optional per-family alias table in Snowmeter |
| `%A` and other `strftime` codes | Cl | MSVC `wcsftime` in the C locale (supports the `%#` modifier) | chrono with the English C locale; map MSVC's `%#d`/`%#H` (no leading zero) to `%-d`/`%-H` |
| Monitor variables / `%` positions | L | Primary monitor, logical 96-DPI px | GDK monitor 0 / primary geometry in logical px; X11 workarea from `_NET_WORKAREA` for WORKAREA* |
| Per-pixel-alpha desktop window (`AlwaysOnTop=0`, desktop level) | all | Layered window | X11 ARGB window of type `_NET_WM_WINDOW_TYPE_DESKTOP`/below, plus a compositor (see `decisions.md` #4) |
