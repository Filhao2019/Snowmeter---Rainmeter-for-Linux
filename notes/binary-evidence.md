# Rainmeter.dll binary evidence (for Snowmeter)

Static reverse-engineering notes on the shipped Windows Rainmeter core DLL. Everything below is
**observed in the binary** (string literals, xrefs, Ghidra pseudo-code) unless marked *inference*.
No code was executed. Function names in `backticks` are names **I assigned** from the evidence (the
binary has no symbols; Ghidra calls them `FUN_<addr>`). They were not written back into a database.

## Tooling, target identity

| Item | Value |
|---|---|
| Tool | REA MCP server `rea-agents` 6.4.0 (npx), Ghidra provider **Ghidra 12.1.4 PUBLIC** (`/home/bernardo/tools/ghidra/ghidra_12.1.4_PUBLIC`), analyzer preset `ghidra-default`, ephemeral import |
| Other providers | Hopper: unavailable (`executable_missing`), IDA: unavailable (`not_configured`) |
| Target | `/home/bernardo/.wine/drive_c/Program Files/Rainmeter/Rainmeter.dll` |
| SHA-256 | `eb3461b76e1518883d3d254b92ee96bdd787e7e2a136346d19b3ba640ab3068a` |
| Size / format | 2,660,648 bytes, PE32+ x86-64 DLL (GUI), image base `0x180000000` |
| Version resource (RT_VERSION, file off 2632448, 608 B) | FileVersion **4.5.27.3934**, ProductVersion "4.5.27.3934 (64-bit)", CompanyName "Rainmeter", LegalCopyright "© 2026 Rainmeter Team", OriginalFilename "Rainmeter.dll", lang 0409/04E4 |
| Ghidra inventory | 5074 procedures, 3972 defined strings (evidence `ev_ee8b2478…`) |
| Segments | `.text 0x180001000-0x180263800` (read-only data is merged into .text: all string literals live at `0x180001000-0x18005d000`), `.data 0x180264000`, `.pdata 0x180272000`, `.idata 0x180287000`, `.rsrc 0x18028c000` |
| RTTI | Present: Ghidra recovers `MeasureCPU::vftable`, `MeterBar::vftable`, … and type descriptors such as `.?AVMeterRoundLine@@` |

Key REA evidence IDs: PE resources `ev_7c624dda…`, overview `ev_ee8b2478…`, string inventory
`ev_37e0a428…`, measure factory decompile `ev_c10d62cc…`, meter factory `ev_4d79f9e7…`, bang
dispatcher `ev_f438c1d0…`, batch decompile #1 `ev_e09f718d…`, batch decompile #2 `ev_579f1ef7…`.

### Method notes and limits
- Ghidra **does not define short UTF-16 strings** (< 4 chars) such as `sin`, `cos`, `BAR`, `CPU`,
  `ZPos`. I recovered those by reading raw bytes at the addresses (file offset = VA - 0x180001000 + 0x400 for .text).
- To avoid hundreds of REA round-trips, xrefs were triaged with a local scanner: RIP-relative `LEA`
  (`48/4C 8D /r rip+disp32`) → target, `call rel32`, `call [rip+IAT]` (IAT parsed from the import
  directory), function bounds from `.pdata`, plus `objdump -d` for a few call sites. Spot checks agree
  with Ghidra (e.g. both report string `0x180026e68` "Division by 0" used at `0x18012d8c4`). `.pdata`
  gives chunk starts; the Ghidra entry (from `batch_decompile`) is quoted where they differ.
- `FUN_1802062b0` = `_wcsicmp` (case-insensitive compare used by every name table),
  `FUN_1801ffb8c` = `operator new`, `FUN_18005df70` = `LogErrorF(skin, fmt, …)`,
  `FUN_18005e080` = `LogDebugF`, `FUN_18006b310` = `ConfigParser::ReadString(section, key, default)`,
  `FUN_18012cfa0` = `MathParser::Parse`, `FUN_1800ed680` = `GetRainmeter()` singleton.

---

## 1. Skin .ini parsing

### 1a. File reading, sections, `@Include`, `[Variables]`, `[Metadata]` — `ConfigParser::ReadIniFile` @ `0x18006d9b0`
Strings: `"Unable to read file: %s"` 0x180015178, `"Reading file: %s"` 0x1800151e0,
`"Reading file: %s (Temp: %s)"` 0x1800151a8, `"Metadata"` 0x180015208, `"@INCLUDE"` 0x180015220,
`"Variables"` 0x180014a20 (all referenced from this function).

Observed behaviour:
- Include recursion depth check: `if (100 < depth)` → error via `LoadStringW(…, 0x854, …)`.
- Uses the **Win32 INI API**: `GetPrivateProfileSectionNamesW` then `GetPrivateProfileSectionW` per
  section (so Windows INI semantics: case-insensitive section names, `key=value` lines, `;` comments).
  Some files are read through a temp copy (the "(Temp: %s)" log); *inference from 4.x source: this is
  the UTF-8 → UTF-16 conversion copy*.
- `[Rainmeter]` is pushed first in section order when `depth == 0`.
- Per section: `_wcsicmp(name, L"Variables")` / `_wcsicmp(name, L"Metadata")` select special handling
  (`[Variables]` keys go into the variable table; `[Metadata]` is kept but not a meter/measure).
- Values wrapped in matching `"…"` or `'…'` are unquoted (length > 1 and first==last char).
- `@Include` is matched by **prefix** on the upper-cased key (`wcsncmp(key, L"@INCLUDE", 8)`), so
  `@Include`, `@Include2`, `@IncludeFoo` all work. The include path is variable-expanded with
  `ReplaceVariables(…, 1)` (0x180068b90) before use; relative paths are resolved against the current
  file's folder (checks for leading `\\`/`//` and drive letter).

```c
// FUN_18006d9b0 (excerpt, Ghidra)
if (100 < param_4) { ... LoadStringW(hInst, 0x854, ...); }      // include depth > 100
local_1a8 = _wcsicmp(lpAppName, L"Variables");
iVar9    = _wcsicmp(lpAppName, L"Metadata");
...
if (1 < len && (first == L'"' || first == L'\'') && value[len] == first) { strip quotes }
iVar9 = wcsncmp(upperKey, L"@INCLUDE", 8);
if (iVar9 == 0 && valueLen != 0) {
    FUN_180065690(cfg);                     // refresh variables
    FUN_180068b90(cfg, &includePath, 1);    // ReplaceVariables(path, newStyle=true)
```

### 1b. Built-in variables — `ConfigParser::SetBuiltInVariables` @ `0x180065160`
Sets, in order: `PROGRAMPATH`, `PROGRAMDRIVE`, `SETTINGSPATH`, `SKINSPATH`, `PLUGINSPATH`
(= settings path + `"Plugins\\"`), `CURRENTPATH` (folder of the skin .ini), `ADDONSPATH`
(+ `"Addons\\"`), `CONFIGEDITOR`, `CURRENTFILE`, `CURRENTCONFIG`, `ROOTCONFIG`, `ROOTCONFIGPATH`,
`CRLF` (value `"\n"`), `CURRENTSECTION` (empty) and finally **`@`** (string `"@"` 0x180014a18) =
the `@Resources` path, only when a resources path is passed (`if (param_3 != 0)`).
`#@#` is therefore just the variable named `@`.
Strings: 0x180014898 … 0x1800149f8.

Monitor variables (`ConfigParser::SetMultiMonitorVariables`, 0x180066af0): `WORKAREAX/Y/WIDTH/HEIGHT`,
`SCREENAREAX/…`, `VSCREENAREAX/…`, `PWORKAREAX/…`, `PSCREENAREAX/…` and indexed forms
`WORKAREAX@%i`, `SCREENAREAWIDTH@%i`, … (0x180014ae8-0x180014e58).
Skin-position variables `CURRENTCONFIGX/Y/WIDTH/HEIGHT` (0x1800226c0-0x180022728) are set by the skin.
Mouse variables `MOUSEX`/`MOUSEY` (0x1800150a0) are resolved in 0x18006b050.

### 1c. `#Var#` (old style) substitution — `ConfigParser::ReplaceVariables` @ `0x180068b90`
Flow (matches Rainmeter 4.x): `ExpandEnvironmentVariables` (FUN_18012c2b0, `%APPDATA%`-style), lazily
build monitor variables, special-case the literal `"[#CURRENTSECTION]"` (0x180014e80) when not
new-style, run the new-style parser (0x1800697b0) and then scan for `#…#` pairs with `find(L'#')`
(`0x23`). The pair text is looked up with `FUN_1800659c0` (`GetVariable`, case-insensitive map);
found → replace, not found → leave as is and continue after the second `#`.

```c
// FUN_180068b90 (excerpt)
local_98 = FUN_1800697b0(param_1, param_2, 1);             // new-style [#var] / [&measure] pass
do {
  lVar3 = wfind(str + pos, end, 0x23);                     // first '#'
  ...
  lVar4 = wfind(str + uVar1, end, 0x23);                   // closing '#'
  name  = substr(uVar1, lVar4 - uVar1);
  value = FUN_1800659c0(cfg, name);                        // GetVariable
  if (value) FUN_180071650(str, ..., value);               // replace
} while (...);
```

### 1d. New-style `[#Var]`, `[&Measure]`, section variables, escapes — `ConfigParser::ParseVariables` @ `0x1800697b0`
Strings: `"Parsing Error: Maximum number of variable replacements reached (%llu) in string: %s"`
0x180014eb0 (used at 0x18006a8f9), `"Parsing Error: Result: %s"` 0x180014f58,
`"Cannot replace variable with itself: \"%s\""` 0x180014f90,
`"Warning: Potential self-referenced variable: %s"` 0x180014ff0,
`"Original string: %s"` 0x180015050, `"Replaced string: %s"` 0x180015078.
- Hard limit: `if (999 < ++count)` → log the "Maximum number of variable replacements" error and stop
  (i.e. 1000 replacements per string).
- Escape: when both the char after the opening delimiter and the char before the closing one are
  `'*'` (0x2a) the asterisks are removed and the text is kept literally (`[#*Var*]`, `#*Var*#`).
- Nesting is handled by searching right-to-left for the innermost opening bracket.
- Section-variable parameters handled in 0x180065ee0: `MaxValue`, `MinValue` (0x180014a58/70),
  `EscapeRegExp`, `EncodeUrl`, `TimeStamp`, and number formatting `"%%.%if"` (`[Measure:2]`
  decimals), `X/Y/W/H/XW/YH` for meters (0x180014a34…).

### 1e. Formulas / MathParser (Common/MathParser.cpp) — evaluator @ `0x18012cfa0`, error path `0x18012d650`
Function-name table (UTF-16, `0x180026c98-0x180026db8`): `atan2 atan sin cos abs tan ln exp sqrt log
trunc frac ceil floor asin round rad acos sgn deg min neg clamp max pi e` (26 names; the data table
pointing at them starts at `0x1802652e0`, xref from Ghidra).
Error strings: `"Syntax error"` 0x180026dc0, `"Unmatched brackets"` 0x180026de0,
`"Extra operation"` 0x180026e08, `"Internal error"` 0x180026e28, `"\"%s\" is unknown"` 0x180026e48,
`"Division by 0"` 0x180026e68 (used at 0x18012d8c4 in FUN_18012d650),
`"Invalid function parameter count"` 0x180026e90, `"Logical expression error"` 0x180026ed8.
Callers wrap errors as `"Formula: %s in key \"%s\" in [%s]"` (0x1800150c0, ~100 call sites) and
`"Formula: %s: %s"` (0x180015100). A value is treated as a formula only when it **starts with `(`**
(`if (*psVar9 != 0x28)` → plain `wcstol(…,10)` with ERANGE check), and brackets are pre-counted
(`Unmatched brackets` before parsing) — see the `Update` excerpt in §2.

### 1f. `[Rainmeter]` options — `Skin::ReadSkin` @ `0x1800ff200` (pdata chunks 0x1800ff734, 0x1800ffdec)
Keys read from section `"Rainmeter"` in this order: `AccurateText`, `AppVersion` (`"%u.%u"`,
`"%u.%u.%u"`), `SkinWidth`, `SkinHeight`, `Group`, `DragGroup`, `BackgroundMargins`, `DragMargins`,
`BackgroundMode`, `BevelType`, `BevelColor`, `BevelColor2`, `SolidColor`, `SolidColor2`,
`GradientAngle`, `DynamicWindowSize`, `Background`, `SelectedColor`, `OnRefreshAction`,
`OnCloseAction`, `OnFocusAction`, `OnUnfocusAction`, `OnUpdateAction`, `OnWakeAction`, `Update`,
`TransitionUpdate`, `DefaultUpdateDivider`, `ToolTipHidden`, `Blur`, `BlurRegion`/`BlurRegion%i`,
`LocalFont`/`LocalFont%i` (`"Unable to load font: %s"`), then iterates sections and dispatches on
`Measure` / `Meter` keys (and legacy `Plugin`).
String addresses: `BackgroundMode` 0x180022358, `DynamicWindowSize` 0x1800223a8,
`OnRefreshAction` 0x1800223f0, `OnUpdateAction` 0x180022470, `DefaultUpdateDivider` 0x180022490.
New-skin template string at 0x180017860: `"[Rainmeter]\nUpdate=1000\nAccurateText=1\n\n[Metadata]\nName=\n…"`.

### 1g. `MeterStyle`
String `"MeterStyle"` 0x18001d250 referenced from `Meter::ReadOptions`-family functions 0x1800ca1c0
(entry; pdata 0x1800ca1fa), 0x1800caf40, 0x180108f20 and 0x18012979a. *(Detail of style lookup
order not decompiled; see Snowmeter source port.)*

---

## 2. Update loop

### 2a. Defaults (from `Skin::ReadSkin` 0x1800ff200)
`Update` → `skin+0x800`, default **1000**; `TransitionUpdate` → `skin+0x804`, default **100**;
`DefaultUpdateDivider` → `skin+0x808`, default **1**. Each accepts a `(formula)`.

```c
// FUN_1800ff200 (excerpt)
psVar9 = ConfigParser_ReadString(cfg, L"Rainmeter", L"Update", L"");
if (*psVar9 != L'(') { if (*psVar9) { errno=0; v = wcstol(psVar9,0,10); if (errno != ERANGE) goto store; } goto dflt; }
... count '(' / ')' -> L"Unmatched brackets" ...
if (err || (err = MathParser_Parse(psVar9,&d,0,0))) { LogErrorF(..., L"Formula: %s in key \"%s\" in [%s]", err, L"Update"); goto dflt; }
v = (int)d;
dflt: v = 1000;
store: *(int *)(skin + 0x800) = v;
```

### 2b. Timers (USER32 `SetTimer` IAT slot 0x180287848, `KillTimer` 0x180287840)
Call sites read with objdump (`SetTimer(hwnd = skin+0x298, id, elapse, NULL)`):

| Site | Timer id | Elapse | Meaning (*inferred name*) |
|---|---|---|---|
| 0x1800f9a19 (skin init), 0x1800fa322 (`!Update` bang path in skin bang switch) | 1 | `skin+0x800` (Update) | TIMER_METER |
| 0x1800f9a33 | 2 | 500 ms (0x1f4) | TIMER_MOUSE (mouse-leave polling) |
| 0x1800f94a2, 0x18010369d, 0x180103799 | 3 | 10 ms | TIMER_FADE |
| 0x1800fbe03, 0x180102c55 | 4 | `skin+0x804` (TransitionUpdate) | TIMER_TRANSITION |
| 0x1800f977b | 5 | `skin+0x81c` | TIMER_DEACTIVATE |
| 0x18005f95b (in `!Delay` handling, FUN_18005ee50) | computed | delay ms | delayed bang |

Timer 1 is armed only for `Update >= 0` (`test r8d,r8d; js skip` at 0x1800fa30a) — `Update=-1`
means "never update by timer". Sequence at 0x1800fa2e7: `KillTimer(hwnd,1); Skin::Update(false);
if (Update >= 0) SetTimer(hwnd,1,Update,0)`.

### 2c. One update tick — `Skin::Update(bool refresh)` @ `0x180102aa0`
```c
++*(int *)(skin + 0x9e0);                                   // update counter
for (measure in skin->measures /*0x970..0x978*/) {
  if (refresh) m->updateCounter(0x90) = m->updateDivider(0x8c);
  if (m->updateDivider >= 0 || refresh) {
    if (Measure_Update(m, /*rereadOptions*/ m->dynamicVariables(0x88) && counter+1 >= divider)) {
      FUN_180109fc0(m); FUN_1800ab400(m);   /* inferred: Section::DoUpdateAction, Measure::DoChangeAction */
    }
  }
}
for (meter in skin->meters /*0x988..0x990*/) if (UpdateMeter(skin, meter, &anyTransition, refresh)) { FUN_180109fc0(meter); changed = true; }
FUN_180102540(skin);                                        // DynamicWindowSize / resize
if (changed || skin->resizeState(0x928) || refresh) Redraw(skin);   // FUN_1801016b0
if (anyTransition && !transitionTimerOn) SetTimer(hwnd, 4, TransitionUpdate, 0); else if (!anyTransition) KillTimer(hwnd, 4);
if (!skin->OnUpdateAction(0x6a0).empty()) ExecuteCommand(OnUpdateAction, skin);   // FUN_18005ee50
```
- `UpdateDivider` (string 0x1800227f0) is read in `Section::ReadOptions` @ 0x180109ce0 with
  default = `skin+0x808` (DefaultUpdateDivider); `-1` disables updates except on refresh.
- Section `OnUpdateAction` (0x180022470) also read there (xref 0x180109f3a); the skin-level
  `OnUpdateAction` runs once per tick after all meters.

### 2d. WM_TIMER dispatch — `Skin::OnTimer` @ `0x1801030a0`
`wParam==1` → `Skin::Update(skin,false)`; `2` → mouse-leave/topmost checks (`GetMessagePos`,
`WindowFromPoint`, Ctrl/Shift/Alt state); `3` → fade step: alpha interpolated over
`skin+0x81c` ms (FadeDuration) using `GetTickCount64`, clamped to 255, `KillTimer(3)` + optional
`ShowWindow(SW_HIDE)` at end; `4` → transition redraw (`FUN_1801016b0`) else `KillTimer(4)`;
`5` → deactivate; **any other id** → looked up (FNV-1a hash of wParam) in the delayed-command map,
`KillTimer(id)` and `ExecuteCommand(cmd, skin)`. Delay ids come from a global counter
`DAT_180265278` (initial value 5, pre-incremented) set in `ExecuteCommand` for `!Delay <ms>`.

---

## 3. Measures

### 3a. Factory — `Measure::Create(type, skin, name)` @ `0x1800ab5d0`
Linear `_wcsicmp` chain (strings `0x180018c28-0x180018de0`), in this order:
`CPU, MediaKey, Memory, NetIn, NetOut, NetTotal, NowPlaying, PhysicalMemory, SwapMemory,
FreeDiskSpace, Uptime, Time, Plugin, Process, RecycleManager, Registry, Calc, Script, String, SysInfo,
Loop, WebParser, WifiStatus` (23 types). Fallthrough: `LogErrorF("Measure=%s is not valid in [%s]")`
(0x180018e00), returns NULL. Classes from RTTI: `MeasureCPU`, `MeasureVirtualMemory` (=SwapMemory),
`MeasureDiskSpace` (=FreeDiskSpace), `MeasureWebParser`, … (36 `.?AVMeasure*/Meter*` descriptors).
```c
iVar1 = _wcsicmp(L"CPU", type);
if (iVar1 == 0) { p = operator_new(600); Measure_ctor(p, skin, name); *p = MeasureCPU::vftable; p[0x1b] = 100.0; /*MaxValue*/ }
...
else if (_wcsicmp(L"PhysicalMemory", type) == 0) { ...; GlobalMemoryStatusEx(&ms); p[0x1b] = (double)ms.ullTotalPhys; }
...
LogErrorF(skin, L"Measure=%s is not valid in [%s]", type, name);
```
**Legacy `Measure=Plugin` remap** (`Skin::ReadSkin` 0x1800ff200 + list builder 0x1800f6660): for
`Measure=Plugin`, `Plugin=` is reduced with `PathFindFileNameW`+`PathRemoveExtensionW`; if it matches
`MediaKey, NowPlaying, Process, RecycleManager, SysInfo, WebParser, WifiStatus` the built-in measure
of that name is created instead of loading a DLL.

### 3b. Individual measures (function = Ghidra entry)
| Measure | Evidence |
|---|---|
| **Time** | ReadOptions 0x1800bb710: `Format`, `TimeStamp` (ReadFloat, default -1), `TimeZone` (default `"local"`; numeric hours otherwise, may be a formula), `DaylightSavingTime` (ReadInt, default **1**), `TimeStampFormat`, `TimeStampLocale`, `FormatLocale`, `DSTStart/DSTEnd/DSTNextStart/DSTNextEnd`; strings 0x18001b580-0x18001b7c8. Offsets via `GetTimeZoneInformation`/`GetSystemTime` (0x1800bb040). GetString 0x1800bb470: empty Format → `wcsftime(buf,0x1000,L"%H:%M:%S")`; `"locale-time"` → `GetTimeFormatW`, `"locale-date"` → `GetDateFormatW`; else `wcsftime(Format)`; error `Time: "Format=%s" invalid`. Day/month name tables `":Sun:Sunday:Mon:…"` 0x18001b370, `":Jan:January:…"` 0x18001b420, `":AM:am:PM:pm"` 0x18001b878 (timestamp parsing). |
| **Uptime** | ReadOptions 0x1800c28d0: `Format` default **`"%4!i!d %3!i!:%2!02i!"`** (0x18001bad8; FormatMessage inserts: %4=days, %3=hours, %2=minutes, %1=seconds), `AddDaysToHours`, `SecondsValue` (0x18001bb30). Update 0x1800c2cc0: value = `GetTickCount64()/1000` s, or SecondsValue. GetString 0x1800c2d30 uses `FormatMessageW`; error `Uptime: "Format=%s" invalid`. |
| **CPU** | ReadOptions 0x1800ae1a0: `Processor` (int/formula; `CPU: Processor=%i is not valid`). Update 0x1800ae330: Processor=0 → `GetSystemTimes`, `value = 100 - idleΔ/(kernelΔ+userΔ)*100` clamped to 100; Processor=N → `NtQuerySystemInformation(8, …)` via pointer resolved from `"ntdll"`/`"NtQuerySystemInformation"` (0x1800190b0/0x1800190c0) at startup. MaxValue 100. |
| **PhysicalMemory / Memory / SwapMemory** | Update 0x1800b3030: `GlobalMemoryStatusEx`, value = `ullTotalPhys - ullAvailPhys` (bytes) unless `Total=1` (flag +0x240); MaxValue = ullTotalPhys set in factory. Memory (0x1800af860) and SwapMemory (0x1800c2f80) also call `GlobalMemoryStatusEx`. |
| **FreeDiskSpace** | ReadOptions 0x1800ae9d0: `Drive` (`FreeDiskSpace: Drive= empty`), `Type`, `Total`, `Label`, `IgnoreRemovable`, `DiskQuota`. Update 0x1800ae670: `GetDriveTypeW`, `GetDiskFreeSpaceExW`, `GetVolumeInformationW`; Type strings `Removed/Removable/Fixed/Network/CDRom/Ram/Error` (0x180019120-0x180019180). |
| **NetIn/NetOut/NetTotal** | ReadOptions 0x1800b0010: `Interface` (name, index or `"BEST"`), `Cumulative`, `MaxValue` default from Rainmeter.ini `NetInSpeed/NetOutSpeed/NetTotalSpeed` (read in 0x1800f3a30); `UseBits` 0x180019710. Log `"Using network interface: %s (IfIndex=%i)"`. Stats persistence 0x1800b0670: `[Statistics]` `Since`, `Count`, `NetStatsCount`, `NetStatsInHigh%u/InLow%u/OutHigh%u/OutLow%u`, `In%u=%llu`. `GetIfTable2` comes from **delay-loaded** `IPHLPAPI.DLL` (names at 0x180263346 / 0x1800012b0). |
| **Registry** | ReadOptions 0x1800b6220: `RegHKey` (`HKEY_CURRENT_USER` default, `HKEY_LOCAL_MACHINE`, `HKEY_CLASSES_ROOT`, `HKEY_CURRENT_CONFIG`, `HKEY_PERFORMANCE_DATA`, `HKEY_DYN_DATA`), `RegKey`, `RegValue`, `OutputType` (`Value`/`SubKeyList`/`ValueList`), `OutputDelimiter`. Update 0x1800b5d90: `RegOpenKeyExW`, `RegQueryValueExW`, `RegEnumKeyExW`, `RegEnumValueW`, binary as `%02X`. |
| **Calc** | ReadOptions 0x1800acff0: `Formula`, `LowBound`, `HighBound`, `UpdateRandom`, `UniqueRandom` (error `"LowBound" (%i) must be less then or equal to "HighBound" (%i)`), `Calc: %s` errors. Formula tokens `random` and `counter` resolved in 0x1800ad960. |
| **String** | Class `MeasureString` (factory alloc 0x280); not further decompiled. |
| **Script (Lua)** | Lua **5.1** statically linked (`"Lua 5.1"` 0x1800313a0, no lua imports). ReadOptions 0x1800b6ce0: `ScriptFile`, globals `SKIN` (0x18001a81c) / `SELF` (0x18001a830), tolua classes `Measure`, `MeterWindow`, legacy `PROPERTIES`; calls Lua `Initialize` (0x1800b6910, `Script Initialize() failed: %s`), `Update` / deprecated `GetStringValue` (0x1800b6b40). Bindings: `GetOption, GetNumberOption, GetValue, GetStringValue, GetValueRange, GetRelativeValue, GetMinValue, GetMaxValue, Enable, Disable, SetText, GetMeter, GetMeasure, GetVariable, ParseFormula, ReplaceVariables, FadeWindow, MoveWindow, MakePathAbsolute, Bang, Show, Hide, GetX, GetW` (0x1800269c8-0x180026b38). |
| **WebParser** | Built-in class `MeasureWebParser`. ReadOptions 0x1800c4120: `Url`, `RegExp`, `Header`, `FinishAction`, `OnRegExpErrorAction`, `OnConnectErrorAction`, `OnDownloadErrorAction`, `ErrorString`, `LogSubstringErrors`, `StringIndex`, `StringIndex2`, `DecodeCharacterReference`, `DecodeCodePoints`, `UpdateRate`, `CodePage`, `Download`, `DownloadFile`, `Debug`, `Debug2File`, `ForceReload`, `Flags` (`Resync, NoCookies, Hyperlink, TempFile, NoAuth, NoCacheWrite, PragmaNoCache, Secure, IgnoreCertDate, IgnoreCertName, IgnoreHTTPRedirect, IgnoreHTTPSRedirect`). `ProxyServer`/`UserAgent` read in 0x1800c3990; default UA `"Rainmeter WebParser plugin"` (0x18001bc18, `InternetOpenW` in 0x1800c3070/0x1800c3900). |
| **Plugin** | `MeasurePlugin::ReadOptions` 0x1800b3690: `Plugin=` (dir part stripped via find_last_of `"\\/"`, `.dll` appended); load order `<ProgramPath>Plugins\<name>.dll` then `<SettingsPath>Plugins\<name>.dll`, each with `SetDllDirectoryW(L"")` + `LoadLibraryW`; failure `Plugin: Unable to load "%s" (error %ld)`, success `Plugin loaded: %s`. Exports (ASCII 0x180019f88-0x18001a088): `Initialize`, `Reload`, `Update`, `GetString`, `ExecuteBang`, `Update2`, `Finalize`, plus `GetPluginAuthor`/`GetPluginVersion` (About dialog). **New API iff `Reload` exists**; otherwise old API: global plugin id counter `DAT_18026c780`, and `Update2` (double) is used when `Update` is missing. Finalize/FreeLibrary in 0x1800b3170. |

```c
// FUN_1800b3690 (excerpt)
SetDllDirectoryW(L""); SetLastError(0);
h = LoadLibraryW(programPath + L"Plugins\\" + name);           // retry with settingsPath on NULL
init   = GetProcAddress(h, "Initialize");
reload = GetProcAddress(h, "Reload");      m->reload = reload;
m->update = GetProcAddress(h, "Update");   m->getString = GetProcAddress(h, "GetString");
m->executeBang = GetProcAddress(h, "ExecuteBang");
if (reload == NULL) {                       // old plugin API
  m->id = DAT_18026c780;
  if (m->update == NULL) { m->update = GetProcAddress(h, "Update2"); m->isUpdate2 = 1; }
```

---

## 4. Meters — `Meter::Create(type, skin, name)` @ `0x1800cb330`
`_wcsicmp` chain on upper-case names (strings 0x18001d030-0x18001d138; `BAR` at 0x18001d0a8):
`STRING` (0x698 B, ctor 0x1800de330), `IMAGE` (ctor 0x1800d10f0), `HISTOGRAM` (ctor 0x1800cf360),
`BAR` (`MeterBar`, image option name `"BarImage"`), `BITMAP` (`"BitmapImage"`), `LINE`, `ROUNDLINE`,
`ROTATOR` (`"ImageName"`), `BUTTON` (ctor 0x1800ce9a0), `SHAPE`; else
`LogErrorF("Meter=%s is not valid in [%s]")`, return NULL. Defaults visible in ctor stores:
RoundLine angle `0x401921ff2e48e8a7` (=6.2832), Rotator angle `0x401921fb54442d18` (=2π), Line scale 1.0.
Shared image-option name tables in .data: `ImageCrop, Greyscale, ImageTint, ImageAlpha,
ColorMatrix1-5, ImageFlip, ImageRotate, UseExifOrientation, ImagePath` at 0x180265040, with
`Both*` / `Primary*` / `Secondary*` variants (Histogram) at 0x1802650c0+.
Shape sub-types: `RECTANGLE, ELLIPSE, LINE, ARC, CURVE, PATH, PATH1` (0x18001e660-0x18001e7b0),
errors `"Rectangle has too few parameters"`, `"Path shape has invalid parameters: %s"`, …
Meter option errors: `BarOrientation=%s`, `BitmapAlign=%s`, `GraphStart=%s`, `GraphOrientation=%s`,
`ClipString=%i`, `StringCase=%s`, `StringStyle=%s`, `StringEffect=%s … is not valid` (0x18001d2c0-0x18001f800).

---

## 5. Bangs — dispatcher `CommandHandler::ExecuteBang(name, args, skin)` @ `0x18005ffb0`
Three tables of 24-byte entries `{uint32 bangId; const wchar_t* name; uint8 argCount | handler}`,
searched with `_wcsicmp` in this order:
1. **Skin bangs** `0x1800141f0-0x1800147d8` (63 entries) → `DoBang` 0x18005e3a0:
`Refresh Redraw Update Hide Show Toggle HideFade ShowFade ToggleFade FadeDuration HideMeter ShowMeter
ToggleMeter MoveMeter UpdateMeter DisableMouseAction ClearMouseAction EnableMouseAction
ToggleMouseAction DisableMeasure EnableMeasure ToggleMeasure PauseMeasure UnpauseMeasure
TogglePauseMeasure UpdateMeasure CommandMeasure PluginBang ShowBlur HideBlur ToggleBlur AddBlur
RemoveBlur Move SetAnchor ZPos ChangeZPos ChangeZPos ClickThrough Draggable SnapEdges KeepOnScreen
AutoSelectScreen SetTransparency SetVariable SetOption SetOptionGroup HideMeterGroup ShowMeterGroup
ToggleMeterGroup UpdateMeterGroup DisableMouseActionGroup ClearMouseActionGroup EnableMouseActionGroup
ToggleMouseActionGroup DisableMeasureGroup EnableMeasureGroup ToggleMeasureGroup PauseMeasureGroup
UnpauseMeasureGroup TogglePauseMeasureGroup UpdateMeasureGroup SkinCustomMenu`
(`ChangeZPos` appears twice: id 39 = alias of ZPos, id 40 unreachable).
2. **Config-group bangs** `0x180013fe0-0x1800141f0` (22): `RefreshGroup UpdateGroup RedrawGroup
HideGroup ShowGroup ToggleGroup HideFadeGroup ShowFadeGroup ToggleFadeGroup ZPosGroup
ClickThroughGroup DraggableGroup SnapEdgesGroup FadeDurationGroup KeepOnScreenGroup
AutoSelectScreenGroup SetTransparencyGroup SetVariableGroup DisableMouseActionSkinGroup
ClearMouseActionSkinGroup EnableMouseActionSkinGroup ToggleMouseActionSkinGroup`; the last arg is
the group; `"!%s: Incorrect number of arguments"` if too few.
3. **Custom bangs** `0x180013e10-0x180013fd8` (19, field +0x10 is a handler pointer):
`ActivateConfig(0x180060ec0) DeactivateConfig ToggleConfig DeactivateConfigGroup WriteKeyValue
LoadLayout SetClip SetWallpaper About Manage SkinMenu TrayMenu ResetStats Log RefreshApp Quit EditSkin
LsBoxHook SetWindowPosition`.
Fallthrough: `"Invalid bang: !%s"` (0x180013528).
`ExecuteCommand` @ 0x18005ee50: `!` prefix; strips legacy `!Rainmeter` prefix (`_wcsnicmp(…,L"Rainmeter",9)`,
so `!RainmeterRefresh` == `!Refresh`); `!Execute [..][..]` and bracketed `[!A][!B]` lists are
split on `[`/`]`; `!Delay <ms>` arms a one-shot timer (see 2d); `PLAY`/`LOOP `/`STOP` sound
commands; *inferred*: anything else is run as a program (helper 0x180060620 uses `"open"`
0x180013550; not traced end-to-end).
Argument errors: `"!%s: Two parameters required, only one given"`, `"!%s: Skin \"%s\" is not active"`, …
(0x180013280-0x180013d90). `!WriteKeyValue` rejects `..\`/`../` paths and read-only files.
`!Log` levels: `ERROR WARNING DEBUG NOTICE`.

---

## 6. .rmskin installer (inside Rainmeter.dll) — `DialogInstall::ReadPackage` @ `0x18007e3b0`
Footer check for `.rmskin` (extension via `PathFindExtensionW`, `.zip` also accepted):
`fopen(path,"rb")`, `fseek(f,-16,SEEK_END)`, `pos=ftell(f)`, `fread(footer,16,1,f)`. Layout of the
**last 16 bytes**: `int64 size` (must equal `pos`, i.e. file size − 16), `uint8 flags`, then the
7 bytes `"RMSKIN\0"` (ASCII at 0x1800161b8). If `flags != 0`: `backup = !(flags & 1)`.
```c
_File = _wfopen(path, L"rb");  fseek(_File, -16, SEEK_END);  lVar5 = ftell(_File);
fread(&footer, 0x10, 1, _File);  fclose(_File);
for (i = 0; i < 7; ++i) if (footer.key[i] != "RMSKIN"[i]) goto not_rmskin;   // bytes 9..15
this->packageFormat = 1;
if (footer.size != lVar5) goto invalid;
if (footer.flags) this->backupPackage = ~footer.flags & 1;
```
Package contents: `RMSKIN.ini` (new) / `Rainstaller.cfg` (old, "New format"/"Old format"),
`RMSKIN.bmp` / `Rainstaller.bmp` header image, top-level folders `Skins`, `Layouts`/`Themes`
(`.ini`/`.thm`), `Addons`, `Plugins\64bit\*.dll`, `Fonts\*.ttf`; `[rmskin]` keys `Name`, `Author`,
`Version`, `MergeSkins`, `VariableFiles`, `MinimumRainmeter`, `MinimumWindows`, `LoadType`, `Load`;
backups `Backup-%d.%d.%d-%d.%d.rmskin`. Error text: `"The Skin Packager tool must be used to create
valid .rmskin packages."`

---

## Source vs binary differences
Source: `/home/bernardo/linux-rainmeter/rainmeter-src` (unversioned dev tree: `Version.h` = 0.0.0.0;
tip commit "WebParser: Rename bangs to !WebParser:{Refetch,ClearResults}" — I read this with one
read-only `git log -1` before noticing the no-git instruction; nothing was modified).
1. **Measure factory** (`Library/Measure.cpp:691`): source has 43 types, binary 23. Source-only (in
   the 4.5.27 install these are separate DLLs in `Program Files/Rainmeter/Plugins/`):
   `CoreTemp, ActionTimer, AdvancedCPU, Audio, AudioLevel, Mouse, PerfMon, Ping, FolderInfo,
   FileView, iTunes, Power, Quote, ResMon, UsageMonitor, RunCommand, InputText, SpeedFan,
   WindowMessage, DragDrop`. The binary's 23 are all present in source. Order differs.
2. **Meter factory** (`Library/Meter.cpp:496`): source adds `TEXTEDIT` and `SVG`; binary has neither
   (no `MeterTextEdit`/`MeterSvg` RTTI).
3. **Bangs** (`Library/CommandHandler.cpp:1185`, single `g_Bangs[]` with per-entry handler):
   all 104 binary names exist in source. Source-only: `CommandMeasureGroup, Debug,
   ForceEnableVisibleMode, Restart, SendKey, SetUpdate, SetZoomFactor` and the `Skin:` namespace
   (`Skin:Edit, Skin:Load, Skin:Toggle, Skin:Unload`); source also has `WebParser:` bangs per commit.
   Binary has the extra unreachable `ChangeZPos` id-40 entry.
4. **Variable expansion**: binary limits by count (1000 replacements, `"Maximum number of variable
   replacements reached (%llu)"`, plus `"Cannot replace variable with itself"` / `"Potential
   self-referenced variable"`); source `ConfigParser.cpp:909` limits recursion depth to 10
   (`"Maximum variable recursion depth reached (%i)"`) and adds `MonitorVariableMode`. Expect output
   differences on pathological self-referencing skins.
5. **Same in both**: include depth limit 100 (`ConfigParser.cpp:1649`), `@Include` 8-char prefix match,
   MathParser 26 functions and identical error strings (`Common/MathParser.cpp:120-244`),
   `[Rainmeter]` Update/DefaultUpdateDivider semantics, section-variable params
   (`EscapeRegExp`, `EncodeUrl`, `TimeStamp`, `MaxValue`, `MinValue`).

## Not covered (stopped early on coordinator request)
- `MeterStyle` resolution order and `Meter::ReadOptions` details (only xrefs recorded).
- `MeasureString`, `NowPlaying`, `SysInfo`, `RecycleManager`, `Process`, `MediaKey`, `Loop`, `WifiStatus` internals (strings only).
- Exact `[Metadata]` handling beyond the section-name check; DynamicWindowSize resize routine
  (`FUN_180102540`, *inferred*) not decompiled.
- WM_TIMER registration in the window proc itself (handler found via call graph, not the WndProc switch).
- No runtime observation was done; all behaviour is static.
