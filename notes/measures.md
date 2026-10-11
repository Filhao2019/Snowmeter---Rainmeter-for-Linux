# Measures — implementation spec (Snowmeter)
Behavior-level spec for every Rainmeter measure, derived from source. Goal: unmodified Windows skins behave identically.

## 0. Sources and conventions
- **Compatibility baseline = Rainmeter 4.5.27.3934**: that is the version installed in `~/.wine/drive_c/Program Files/Rainmeter/` (`Rainmeter.dll` FileVersion 4.5.27.3934), and skins in the wild target 4.5.x. Its source is tag `v4.5.27.3934` in `rainmeter-src` (`git show v4.5.27.3934:Library/<file>`).
- **`rainmeter-src` HEAD is Rainmeter 5.0 development** (2026-10, many former plugins folded into `Rainmeter.dll`). Cites without a prefix are HEAD `Library/…:line`. `4.5:` prefix means the v4.5.27 tag. When HEAD differs from 4.5 in a way a skin could notice, the note says so. Implement 4.5 semantics, and accept HEAD-only additions only when they don't conflict.
- "Windows timestamp" means seconds since 1601-01-01 00:00:00 (FILETIME / 10^7).
- Numeric options (`ReadInt/ReadFloat/ReadUInt`) accept a formula in parentheses, e.g. `MaxValue=(1024*1024)` (`ConfigParser::ParseDouble` → `ParseUtil::ParseDouble`, ConfigParser.cpp:1597).
- Measure names, option names and enum values are case-insensitive unless noted otherwise.
- The INI reader strips one pair of matching outer quotes, either `"…"` or `'…'`, from every value (Common/StringUtil.cpp:64-78, called from Common/IniFile.h:149; in 4.5 the stripping is in ConfigParser.cpp:2077).

---

## 1. How a measure type is selected
### 1.1 `Measure=` → class (`Measure::Create`, Measure.cpp:691-871)
The `Measure=` value is compared case-insensitively. An unknown name logs `Measure=%s is not valid in [%s]` and **no section object is created** (Measure.cpp:868). Meters that then reference it get nothing, and `[Name]` stays literal text.

| `Measure=` | Class | In 4.5? |
|---|---|---|
| `CPU` | MeasureCPU | yes |
| `Memory` | MeasureMemory | yes |
| `PhysicalMemory` | MeasurePhysicalMemory | yes |
| `SwapMemory` | MeasureVirtualMemory | yes (**there is no `VirtualMemory` name**) |
| `NetIn` / `NetOut` / `NetTotal` | MeasureNetIn/Out/Total (base MeasureNet) | yes (**there is no plain `Net` name**) |
| `FreeDiskSpace` | MeasureDiskSpace | yes |
| `Uptime` | MeasureUptime | yes |
| `Time` | MeasureTime | yes |
| `Calc` | MeasureCalc | yes |
| `String` | MeasureString | yes |
| `Registry` | MeasureRegistry | yes |
| `Script` | MeasureScript | yes |
| `Plugin` | MeasurePlugin (see 1.2) | yes |
| `Loop` | MeasureLoop | yes |
| `WebParser`, `NowPlaying`, `SysInfo`, `Process`, `RecycleManager`, `WifiStatus`, `MediaKey` | internal | yes (internal in 4.5 too) |
| `ActionTimer`, `AdvancedCPU`, `AudioLevel`, `CoreTemp`, `FileView`, `FolderInfo`, `InputText`, `iTunes`, `Mouse`, `PerfMon`, `Ping`, `Power`, `Quote`, `ResMon`, `RunCommand`, `SpeedFan`, `UsageMonitor`, `Audio`, `WindowMessage`, `DragDrop` | internal | **HEAD only**. In 4.5 these exist only as `Measure=Plugin` + `Plugin=X.dll`. |

The `Measure=` value is read without section-variable expansion (`{ .sectionVariables = false }`, Skin.cpp:2806).

### 1.2 `Measure=Plugin` redirection (Skin.cpp:2817-2846; 4.5: Skin.cpp:2670-2683)
If `Measure=Plugin`, Rainmeter reads `Plugin=`, takes the file name, strips the extension (`Plugins\WebParser.dll`, `WebParser.dll`, `webparser` all become `WebParser`), and compares it case-insensitively with the "old default plugins" list. On a match, `Measure=` is replaced by the internal name.
- **4.5 list** (4.5: Rainmeter.cpp:1930-1943): `MediaKey, NowPlaying, Process, RecycleManager, SysInfo, WebParser, WifiStatus`. The name is used unchanged as the measure name.
- **HEAD list** (Rainmeter.cpp:2029-2062): the 4.5 list plus `ActionTimer, AdvancedCPU, AudioLevel, CoreTemp, Drag&Drop→DragDrop, FileView, FolderInfo, InputText, iTunesPlugin→iTunes, Mouse, PerfMon, PingPlugin→Ping, PowerPlugin→Power, QuotePlugin→Quote, ResMon, RunCommand, SpeedFanPlugin→SpeedFan, UsageMonitor, Win7AudioPlugin→Audio, WindowMessagePlugin→WindowMessage`.
- **Snowmeter** should use the HEAD list. On Linux no DLL can load, so every plugin name we implement natively must be redirected, and the DLL file names (`PingPlugin`, `PowerPlugin`, `Win7AudioPlugin`, …) must map to the measure names.
- Anything else stays `MeasurePlugin` (third-party DLL). See §3.12 for the "DLL missing" behavior.

---

## 2. Base Measure behavior (all measure types)
### 2.1 Lifecycle and per-update order
1. **Load** (Skin.cpp:2909-2932): every measure's `ReadOptions` runs first (in INI section order). Then meters `ReadOptions`+`Initialize`, then measures `Initialize()` (Script runs `Initialize()` here). Measure options are read before meters so that Substitute is already active.
2. **Each skin update** (`Skin::Update`, Skin.cpp:3426-3467):
   - If any Net/SysInfo measure exists: refresh the interface table and the cumulative stats once (Skin.cpp:3450-3455).
   - For each measure **in INI order**: `UpdateMeasure` → `Measure::Update`. If it returns true, run `OnUpdateAction`, then `OnChangeAction` check (Skin.cpp:3460-3466).
   - Then all meters update. Then redraw.
3. `Measure::Update(rereadOptions)` (Measure.cpp:440-517): (1) `rereadOptions` is true when `DynamicVariables=1` and this tick will actually update: `(counter+1) >= UpdateDivider` (Skin.cpp:3386). Options are re-read **before** the paused/divider checks (Measure.cpp:442-445). (2) `Paused` → return false. The value is frozen and no actions run. (3) `Disabled` → `m_Value = 0`, reset all If* "committed" latches (`IfActions::SetState`, IfActions.cpp:413-449), return false. No OnUpdate/OnChange. (4) UpdateDivider counter (§2.6). Not due → return false. (5) `UpdateValue()` (type-specific) sets `m_Value` (raw, before inversion). (6) AverageSize smoothing (§2.4). (7) Auto-max tracking when `m_LogMaxValue` (§2.3). (8) If DynamicVariables: re-read IfCondition/IfMatch options **now**, so `[Self]` inside them sees the new value (Measure.cpp:495-499). (9) IfAbove/IfBelow/IfEqual, then IfCondition*, then IfMatch* (IfActions.cpp:290-411), all using raw `m_Value` (**not inverted**, Measure.cpp:503). (10) Return true. Then OnUpdateAction and OnChangeAction run.
- Measures earlier in the file already have this tick's value when a later measure (Calc, IfCondition, `[M]` in actions) reads them. Later ones still hold the previous tick's value.

### 2.2 Common options

| Option | Default | Notes |
|---|---|---|
| `Measure` | — | type (§1) |
| `MinValue` | 0.0 | Measure.cpp:100,142. Some types override it (Loop). |
| `MaxValue` | 1.0 | Measure.cpp:101,143. Many types override it, see each type. |
| `InvertMeasure` | 0 | Measure.cpp:137 |
| `AverageSize` | 0 | Measure.cpp:156-165 |
| `Disabled` | 0 | Measure.cpp:139. `!DisableMeasure`/`!EnableMeasure` also write the option value back (Measure.cpp:200-214). |
| `Paused` | 0 | Measure.cpp:140, same write-back |
| `UpdateDivider` | `[Rainmeter] DefaultUpdateDivider` (default 1) | Section.cpp:32-38 |
| `DynamicVariables` | 0 | Section.cpp:40 |
| `OnUpdateAction` | "" | Section.cpp:42. Action string: `[M]` is resolved at execution time. |
| `OnChangeAction` | "" | Measure.cpp:154 |
| `Group` | "" | Section.cpp:44-45. Pipe-separated names (Group.cpp, `ForEachToken(groups, L'|')`), case-insensitive. Used by `!…Group` bangs. |
| `Substitute` | "" | Measure.cpp:168-175 |
| `RegExpSubstitute` | 0 | Measure.cpp:167 |
| `IfAboveValue/IfAboveAction`, `IfBelowValue/IfBelowAction`, `IfEqualValue/IfEqualAction` | 0 / "" | IfActions.cpp:90-113 |
| `IfCondition[N]`, `IfTrueAction[N]`, `IfFalseAction[N]`, `IfConditionMode` | "" / 0 | IfActions.cpp:115-177 |
| `IfMatch[N]`, `IfMatchAction[N]`, `IfNotMatchAction[N]`, `IfMatchMode` | "" / 0 | IfActions.cpp:179-226 |
| `NumberConversionFormat` | `Default` | **HEAD only** (Measure.cpp:186-198), for String/Registry/WebParser string→number conversion. `Locale` = user separators. 4.5 always uses C-locale `wcstod`/`_wtof`. |

Action options (`*Action`) are read raw. `[Measure]` and `#Var#` are expanded when the action **executes** (no DynamicVariables needed). `IfCondition`/`IfMatch` expressions are read with measure substitution at read time, so a `[Measure]` inside them is frozen unless `DynamicVariables=1` (IfActions.cpp:117-118; 4.5 reads them with `ReadString(...)` default replace=true).

### 2.3 Value accessors and Min/Max
- `GetValue()` = `InvertMeasure ? (Max − v + Min) : v` (Measure.cpp:519-528). Everything external uses GetValue: meters, `[M]`, Calc/IfCondition references to *other* measures, Lua `GetValue`. The measure's **own** If-actions use raw `m_Value`.
- `GetRelativeValue()` = `(clamp(GetValue(), Min, Max) − Min) / (Max − Min)`. If `Max == Min` it returns **1.0** (Measure.cpp:530-547).
- **Auto max (`m_LogMaxValue`)**, used by Plugin (when the plugin reports max 0 and no MaxValue is given), Net (NetXSpeed 0), Registry (MaxValue 0). The initial max is 1.0, then after each update: ▸ **4.5**: keep the last `MEDIAN_SIZE=3` values, take the median of those 3 (the window starts filled with zeros), then `Max = max(Max, median)` and `Min = min(Min, median)` (4.5: Measure.cpp:61, 489-506). This makes it spike-resistant: one outlier never moves Max. ▸ **HEAD**: `Max = max(Max, v)`, `Min = min(Min, v)` with no median (Measure.cpp:485-489). ▸ Snowmeter: implement the 4.5 median-of-3, because it changes bar/histogram scaling.

### 2.4 AverageSize (Measure.cpp:458-481)
`N>0`: ring buffer of the last N raw values. On the first fill or a resize, the buffer is filled with the current value. `m_Value` becomes the arithmetic mean. Smoothing applies after `UpdateValue` and before auto-max and If-actions. It is applied only on ticks where the measure actually updates (divider honored). The string value is not affected (string measures keep their string).

### 2.5 Disabled / Paused
- Disabled: value 0 every tick. If-latches reset, so when the measure is re-enabled, actions fire again on the first evaluation. The string value is **not cleared**: e.g. a disabled Time measure still returns its last formatted string, while a String measure disabled from load returns "" (MeasureString.cpp:27-31). OnUpdate/OnChange do not run.
- Paused: neither value nor string changes, no actions run, and the divider counter does not advance.

### 2.6 UpdateDivider (Section.cpp:30-60, Skin.cpp:3374-3390)
- The counter starts equal to the divider, so the **first** update always runs. Afterwards the measure updates every Nth skin update: `++counter; if counter < divider → skip; counter = 0`.
- `UpdateDivider=-1` (any negative value): never updated by the normal loop (`updateDivider >= 0 || force`). It updates only on the forced refresh update and on `!UpdateMeasure`/`!UpdateMeasureGroup` (force resets the counter). In practice it is "update once at load, then on demand".
- `UpdateDivider=0`: `++counter` gives `counter >= 0`, so it updates every tick (same as 1).
- HEAD only: updates skipped while the skin is hidden are credited back (`AdvanceUpdateCounter`, Skin.cpp:3435-3441). Optional.

### 2.7 OnUpdateAction / OnChangeAction
- `OnUpdateAction` runs after every **successful** `Update()` (divider due, not paused/disabled).
- `OnChangeAction` (Measure.cpp:664-687): compares `(GetValue(), GetStringValue() or "")` with the previous snapshot. The **first** successful update only stores the snapshot and never fires. After that it fires whenever the number **or** the string differs from the last update (exact `!=` on doubles). A substitute that changes the string counts. If OnChangeAction appears later via DynamicVariables/!SetOption, the snapshot is re-seeded without firing (Measure.cpp:177-181).
- Ordering is OnUpdateAction, then OnChangeAction. Both run after the measure's If-actions in the same tick.

### 2.8 Substitute (plain)
**Parsing** (behavior is the same in 4.5 `ExtractWord` and HEAD `ParseSubstitute`, Measure.cpp:338-438; HEAD tests Measure_Test.cpp:33-94): • Syntax: `"pattern":"replacement"[,"pattern":"replacement"…]`. Each token is quoted with `"…"` or `'…'`. There are **no escapes**, so a token cannot contain its own quote character. Use the other quote type: `'"':''` is a pattern `"` → "". • Because the INI reader strips the outer `"…"` pair, 4.5 re-adds quotes unless the raw value is `"…'` or `'…"` (4.5: Measure.cpp ReadOptions `subs.insert(0,1,'"')`). Model it this way: re-wrap the (stripped) value in `"…"` unless it starts with `"` and ends with `'` (or the reverse), then tokenize `word ':' word (',' word ':' word)*`, skipping whitespace between tokens. • So `Substitute="a":"b","c":"d"` is valid. `Substitute='a':'b'` and `Substitute=a:b` are **invalid**: they log `Measure: Invalid Substitute=…` (Measure.cpp:173), and only the pairs parsed before the error are kept (in 4.5 the earlier pairs were already pushed). • Separators and commas inside quotes are literal: `"a,b":"c:d"` maps `a,b`→`c:d` (Measure_Test.cpp:47-52). Whitespace inside quotes is kept: `"a":"  "` maps to two spaces. • A pair whose pattern equals its replacement is dropped (Measure.cpp:413). • With `RegExpSubstitute=1`, an empty pattern `""` becomes `^$` (Measure.cpp:416-419).
**Application** (`CheckSubstitute`, Measure.cpp:247-334). Pairs run **sequentially in order**: each pair works on the output of the previous one. • Plain mode: for each pair with a non-empty pattern, replace all occurrences, case-sensitive, scanning left to right and resuming **after** the inserted replacement, so no re-scan (Measure.cpp:233-244). Empty pattern: if the current string is empty, it becomes the replacement (`"":"N/A"`). Otherwise the pair does nothing. • **Substitute applies** to every string a measure hands out: `GetStringValue()` of string-type measures and the **formatted number** from `GetFormattedValue` (Measure.cpp:602), *after* AutoScale/decimals formatting. So `Substitute=" k":"KB"` acts on "1.5 k". It does **not** change `m_Value` (numeric value, Calc references, IfCondition, bars are unaffected). IfMatch sees the substituted string value (it calls `GetStringValue`).

### 2.9 RegExpSubstitute=1
- PCRE (pcre16), compiled once per pair with **no flags** in HEAD (Library/Pcre.h:28). 4.5 used `PCRE_UTF16` and compiled on every call. No implicit case-insensitivity, multiline or dotall: skins write `(?i)`, `(?s)`, `(?U)` inline. **Must support PCRE syntax**: inline flags incl. `(?U)` ungreedy, lookbehind, `\d`, possessive quantifiers. Recommend the `pcre2` crate.
- Loop per pair (Measure.cpp:283-328): start at offset 0. Match with `PCRE_NOTEMPTY` unless the subject is empty, so the empty-pattern-`^$` case only matches an empty string. Build the replacement: for groups j = rc−1 down to 0, replace every literal `\j` in the replacement with group j's text; groups that didn't participate or are empty are **left as literal `\j`**. Replace the whole match and continue searching from `matchStart + len(replacement)`. Repeat until there is no match. This is a global replace that never re-matches inside inserted text.
- Backreference quirk: substitution goes from the highest group down and uses plain string search, so with ≥10 groups `\1` would also hit the start of `\10`. Groups ≥10 are replaced first, so it works. Only `\0`-`\99`-style decimal refs exist. There is **no** `$1` syntax and no escaping of `\`.
- Invalid regex: logs a notice `Substitute: <pcre error>` and that pair falls back to a **plain** substitute (Measure.cpp:425-430, test Measure_Test.cpp:83-88).
- Matching works on UTF-16 code units on Windows. Use char-based matching in Rust and accept the minor difference for astral characters.

### 2.10 IfAbove / IfBelow / IfEqual (IfActions.cpp:90-113, 234-288)
- Active only if at least one of the three `…Action` options is non-empty. Values are read with ReadFloat (formulas allowed).
- `IfEqual`: `(int64)value == (int64)IfEqualValue`, **truncation toward zero**, so 5.9 equals 5 and −0.5 equals 0.
- `IfAbove`: `value > IfAboveValue`. `IfBelow`: `value < IfBelowValue`.
- **Edge-triggered**: the action fires when the condition becomes true and then not again until it has been false at least once (latch). Evaluation order is Equal, Above, Below.
- Uses raw (non-inverted) `m_Value` after averaging.
- On a disabled tick the latches reset when the condition is not met at value 0 (IfActions.cpp:415-434).

### 2.11 IfCondition (IfActions.cpp:115-177, 297-351)
- `IfCondition`, `IfCondition2`, `IfCondition3`, … Numbering starts at the unnumbered key. There is no `IfCondition1`. The chain stops at the first missing `IfConditionN`, or at the first N where **both** `IfTrueActionN` and `IfFalseActionN` are empty. If the first `IfCondition` has no actions, no conditions are evaluated at all.
- The expression is evaluated by the skin MathParser (same as Calc: `= != <> < > <= >= && || ! ?:`, functions, see the formula note). **Bare measure names** resolve to that measure's `GetValue()` (inverted if InvertMeasure), case-insensitive (Skin.cpp:6110-6136). `$UserInput$`-style `$var$` resolve to mouse/input dollar variables. `[M]` is substituted textually at read time (§2.2).
- Result `== 1.0` exactly → "true". `== 0.0` exactly → "false". **Any other result (2, 0.5, −1) → nothing fires and the latches are unchanged.** For example `IfCondition=MeasureCPU` with CPU=37 never fires. Comparisons yield 0/1.
- Latching: per condition, the true action fires on a transition into true (and on the first evaluation), the false action on a transition into false (and on the first evaluation). The very first update therefore fires whichever branch applies. With `IfConditionMode=1` the matching branch fires **every update**.
- Parse error: logged once (`<err>: IfCondition=<expr>` or `IfConditionN`) until the expression changes. No action.
- Conditions are evaluated in order N=1,2,… inside the same tick. Actions execute immediately, synchronously.

### 2.12 IfMatch (IfActions.cpp:179-226, 353-410)
- Same numbering and latching rules (`IfMatch`, `IfMatch2`…, `IfMatchAction[N]`, `IfNotMatchAction[N]`, `IfMatchMode`).
- The subject is `GetStringValue()` **or "" for measures without a string value**: CPU, Calc, Loop and similar pure-number measures always match against the empty string (IfActions.cpp:357; 4.5 same, it used `GetStringValue()` which returns nullptr). The subject is the substituted string.
- PCRE, no flags, unanchored search (`rc > 0` = match). A compile error is logged once and the item is skipped.
- `[M:EscapeRegExp]` exists to embed literal text in an IfMatch (§2.14).

### 2.13 String value vs number, and formatting
Every measure has a double value. Some also have a string value (`GetStringValue()` returns non-null): Time, Uptime, String, Script (when it returned a string), WebParser, Registry (non-empty), FreeDiskSpace (Type/Label), Plugin (when the DLL's GetString returns non-null), and most info-type built-ins.
`GetStringOrFormattedValue(autoScale, scale, decimals, percentual)` (Measure.cpp:561-566): the string value if present (**meter formatting options are then ignored**), else `GetFormattedValue`:
- `percentual` (Measure.cpp:579-582): `format("%.{decimals}f", 100 * GetRelativeValue())`.
- `autoScale != off` (Measure.cpp:605-639). Thresholds use the 1024 table (AutoScale=1, or 1k) or the 1000 table (AutoScale=2, or 2k): `v ≥ T⁴ → v/T⁴ " T"`, `≥ T³ → " G"`, `≥ T² → " M"`, `≥ T¹` (or always, for the `k` variants) `→ " k"`, else `v " "`. The suffix is space + letter, and the no-unit case appends a **single trailing space**. Lowercase k. Negative values never reach a threshold, so they are not scaled, except with the `k` variants, which always divide by 1024/1000.
- Otherwise: `GetValue()/scale` with `decimals`. `decimals == -1` means `%.5f`, then trailing zeros are removed, and a trailing `.` too (Measure.cpp:589-596, 641-660).
- Then **Substitute** is applied (Measure.cpp:602).
- Number → text is printf `%.Nf` with correct rounding of the exact binary value: 2.5→"2", 0.125@2→"0.12". Rust `format!("{:.N$}")` matches. The decimal point is always `.`.
**Where the meter options live** (MeterString.cpp:38-61, 70; Meter.cpp:620-645). `AutoScale` (0/1/2/1k/2k, default 0), `Scale` (default 1), `NumOfDecimals` (default −1), `Percentual` (default 0) are **meter** options. Effective decimals: `NumOfDecimals` if set. Otherwise 0 when `Scale` has no `.` **and** (Percentual or AutoScale off). Otherwise 1. So `AutoScale=1` with no NumOfDecimals gives "1.5 G", and plain gives "37". `Text=` with `%1`, `%2`… gets each measure's `GetStringOrFormattedValue` with the same parameters. Image meters use `(OFF, 1, 0, false)` (MeterImage.cpp:147).

### 2.14 `[MeasureName]` and section variables (ConfigParser.cpp:263-469, 836-900)
- `[M]` (and `[&M]` new-style) → `GetStringOrFormattedValue(AUTOSCALE_OFF, 1.0, -1, false)`: the string value if any, else the number with up to 5 decimals and trailing zeros trimmed ("37.5", "100", "0.33333"), then Substitute. The lookup is case-insensitive. Unknown names stay literal. `[*M*]` escapes to the literal `[M]`.
- Number selectors (no Substitute, ignore string value): ▸ `[M:]` / `[M:dec]`: GetValue with `dec` decimals. Default is 10 decimals with trailing zeros trimmed. Decimals are clamped 0..32. ▸ `[M:%]`, `[M:%,dec]`: 100×relative. ▸ `[M:/scale]`, `[M:/scale,dec]`: integer scale. ▸ `[M:MaxValue]`, `[M:MinValue]` (also `:MaxValue:/scale,dec`).
- `[M:EscapeRegExp]` / `[M:EncodeUrl]` operate on the string value (or ""). `[TimeM:TimeStamp]` → integer Windows timestamp of the Time measure (ConfigParser.cpp:397-401).
- `[ScriptM:Func(args)]` / `[PluginM:Func(args)]` → custom function call (§3.10, §3.12). HEAD also has `[ScriptM:var]` → Lua global.
- `[Meter:X]`, `:Y`, `:W`, `:H`, `:XW`, `:YH` are meter variables.
- Options are substituted at read time (load/refresh), and every update if `DynamicVariables=1`. Actions are substituted at execution.

---

## 3. Measures in detail
### 3.1 Time (MeasureTime.cpp)
**Options**
| Option | Default | Meaning |
|---|---|---|
| `Format` | "" | strftime pattern (MSVC UCRT semantics, table below), or the case-insensitive whole values `locale-time` / `locale-date` |
| `FormatLocale` | "" (C locale) | `_wcreate_locale(LC_TIME, name)`. `local` = user locale. Invalid → error `Invalid FormatLocale: …`, C locale used (MeasureTime.cpp:436-455) |
| `TimeZone` | `local` | `local`, or a number = hours offset from UTC (float, e.g. `5.5`, `-3`). **Only read when TimeStamp is not a valid number** (MeasureTime.cpp:362-378) |
| `DaylightSavingTime` | 1 | only with numeric TimeZone. Adds **one** DST bias (`-DaylightBias` minutes, normally +60) when **the local machine** is currently in DST, not the target zone (MeasureTime.cpp:486-500) |
| `TimeStamp` | -1 | number ≥ 0 = fixed Windows timestamp (seconds since 1601, local wall clock, fractional allowed). Negative/invalid = current time. Also `DSTStart[YYYY]`, `DSTEnd[YYYY]` (**case-sensitive prefix**, `wcsncmp`), `DSTNextStart`, `DSTNextEnd` (case-insensitive) (MeasureTime.cpp:335-356) |
| `TimeStampFormat` | "" | if set, `TimeStamp` is a date string parsed with this strptime-style format into a fixed local timestamp |
| `TimeStampLocale` | `C` | locale for parsing month/day names |

**Internal time** `m_Time` = FILETIME (100 ns since 1601-01-01) **in local wall-clock time**: • Current time: `UTC FILETIME + m_Delta` (MeasureTime.cpp:150-161). `m_Delta` = local−UTC offset for `local` (MeasureTime.cpp:466-483), or `TimeZone*3600 s` (+DST bias) for numeric zones. `m_Delta` is recomputed **only** in ReadOptions and on WM_TIMECHANGE (Skin.cpp:5778-5788). Snowmeter may recompute every update (needed at DST switch on Linux, which has no WM_TIMECHANGE equivalent). • Fixed: `m_Time = TimeStamp * 10^7` with **no zone conversion** (MeasureTime.cpp:211-214). • DSTStart/End: the local wall-clock time of the current zone's transition in year YYYY (default: current UTC year), computed from `TIME_ZONE_INFORMATION.DaylightDate/StandardDate` (MeasureTime.cpp:123-148). E.g. US 2024 DSTStart = 2024-03-10 02:00:00 (value 13354509600), DSTEnd = 2024-11-03 02:00:00. `DSTNextStart/End` = this year's transition if still in the future, else next year's. A zone without DST gives m_Time = 0 (1601-01-01 00:00:00, value 0). • Linux: find the UTC instant of the transition by scanning `localtime_r` `tm_isdst` changes (binary search inside the year), then express it in the **pre-transition** offset.
**Numeric value** (MeasureTime.cpp:217-269): • `Format` empty → `floor(m_Time/10^7)` = local-wall-clock seconds since 1601. 2024-03-05 07:08:09 local → **13354096089**. Unix-style local seconds = value − 11644473600. • `Format` non-empty → `wcstod(formatted string)`: skips leading whitespace, parses the longest numeric prefix (C locale), and gives 0 if there is none. `%H`→7, `%M`→8, `%#d`→5, `%Y%m%d`→20240305, `%H:%M`→7, `%A`→0, `%e`→5. `locale-date` "3/5/2024" → 3. • `[M:TimeStamp]` → `floor(m_Time/10^7)` regardless of Format (ConfigParser.cpp:397-401).
**String value** (MeasureTime.cpp:272-320): • `Format` empty → `%H:%M:%S` ("07:08:09"). • `locale-time` → `GetTimeFormat(LOCALE_USER_DEFAULT, 0)`, the user's long time format. en-US gives `h:mm:ss tt` → "7:08:09 AM". • `locale-date` → `GetDateFormat(LOCALE_USER_DEFAULT, 0)`, the user's short date. en-US gives `M/d/yyyy` → "3/5/2024". On Linux use the user's `LC_TIME` `%X`/`%x` (or `D_T_FMT`) as an approximation. Doc: en-US defaults. • Else → strftime. Then Substitute. • **The tm handed to strftime always has `tm_isdst = 0`** and correct `tm_wday`/`tm_yday` (MeasureTime.cpp:285-293).
**Formatter semantics (critical)** • Rainmeter.dll links the CRT **statically** (`objdump -p Rainmeter.dll` shows no ucrtbase/msvcrt imports). It never sets `LC_TIME` (only `setlocale(LC_NUMERIC,"C")`, Skin.cpp:307). So **without FormatLocale, all names are English "C" locale regardless of the OS language.** Do NOT use glibc `strftime` with the process locale. Implement a custom formatter. • glibc and MSVC differ: glibc `%#a` means "swap case" (→"TUE"), glibc `%c` differs, glibc supports `%k %l %P %s %-d`. In UCRT all of these are **invalid**. • **Invalid directive** (unknown letter, `%` at end of string, e.g. `%k`, `%l`, `%P`, `%s`, `%-d`, `%i`, `%f`, `%q`, `%N`, `%L`, `%+`): UCRT raises EINVAL. Rainmeter then logs `Time: "Format=<fmt>" invalid` and the **whole string is empty**, so the value is 0 (MeasureTime.cpp:109-114). Output longer than 4095 chars: also empty (no log). • `#` flag (after `%`): see the table. `E` and `O` modifiers (C99: `%Ec %EC %Ex %EX %Ey %EY %Od %Oe %OH %OI %Om %OM %OS %Ou %OU %OV %Ow %OW %Oy`) are accepted and ignored. • Ground truth: Wine's `dlls/ucrtbase/tests/misc.c` `test_strftime` table. Its rows are verified against native ucrtbase on Windows, e.g. `%c`→"Thu Jan  1 00:00:00 1970", `%#c`→"Thursday, January 01, 1970 00:00:00", `%#x`→"Thursday, January 01, 1970", `%#D`→"1/1/70", `%#r`→"12:0:0 AM", `%e`→" 1". The behavior matches MS's documented UCRT behavior.
**Code table** for local time **2024-03-05 07:08:09, Tuesday** (tm_yday=64, tm_wday=2), C locale:

| Code | Meaning | Output | `%#…` output |
|---|---|---|---|
| `%a` | abbreviated weekday | `Tue` | `Tue` (# ignored) |
| `%A` | full weekday | `Tuesday` | `Tuesday` |
| `%b` / `%h` | abbreviated month | `Mar` | `Mar` |
| `%B` | full month | `March` | `March` |
| `%c` | C locale: `%a %b %e %T %Y`. Other locales: short date + `' '` + time | `Tue Mar  5 07:08:09 2024` (two spaces) | `Tuesday, March 05, 2024 07:08:09` (long date + `' '` + time) |
| `%C` | century 00-99 | `20` | `20` |
| `%d` | day 01-31 | `05` | `5` |
| `%D` | `%m/%d/%y` | `03/05/24` | `3/5/24` |
| `%e` | day, space-padded | ` 5` | `5` |
| `%F` | `%Y-%m-%d` | `2024-03-05` | `2024-3-5` |
| `%g` | ISO-8601 week-based year, 2 digits | `24` | `24` |
| `%G` | ISO-8601 week-based year | `2024` | `2024` |
| `%H` | hour 00-23 | `07` | `7` |
| `%I` | hour 01-12 | `07` | `7` |
| `%j` | day of year 001-366 | `065` | `65` |
| `%m` | month 01-12 | `03` | `3` |
| `%M` | minute 00-59 | `08` | `8` |
| `%n` | newline | `\n` | `\n` |
| `%p` | AM/PM (locale) | `AM` | `AM` |
| `%r` | C locale: `%I:%M:%S %p`. Other locales: the locale **time** format (=`%X`) | `07:08:09 AM` | `7:8:9 AM` |
| `%R` | `%H:%M` | `07:08` | `7:8` |
| `%S` | second 00-59 | `09` | `9` |
| `%t` | tab | `\t` | `\t` |
| `%T` | `%H:%M:%S` | `07:08:09` | `7:8:9` |
| `%u` | ISO weekday 1-7 (Mon=1, Sun=7) | `2` | `2` |
| `%U` | week of year, Sunday first, 00-53 | `09` | `9` |
| `%V` | ISO week 01-53 | `10` | `10` |
| `%w` | weekday 0-6 (Sun=0) | `2` | `2` |
| `%W` | week of year, Monday first, 00-53 | `10` | `10` |
| `%x` | short date. C locale `MM/dd/yy` | `03/05/24` | `Tuesday, March 05, 2024` (long date; C locale `dddd, MMMM dd, yyyy`, note **zero-padded dd**) |
| `%X` | time. C locale `HH:mm:ss` | `07:08:09` | `07:08:09` (# ignored) |
| `%y` | year 00-99 | `24` | `24` (2005 → `05` / `5`) |
| `%Y` | year | `2024` | `2024` |
| `%z` | UTC offset `±hhmm`, **standard time** (tm_isdst=0 means DST is never added) | e.g. `-0800` (US Pacific, even in July), `+0100` (Berlin, even in summer), `+0000` | same |
| `%Z` | Windows time-zone **standard** name (`_tzname[0]` from `TIME_ZONE_INFORMATION.StandardName`) | e.g. `Pacific Standard Time`, `W. Europe Standard Time` | same |
| `%%` | literal % | `%` | `%` |

Formulas (UCRT/Wine `strftime_impl`): • `%U = yday/7 + (wday <= yday%7)`. • `%W`: same with `wd = (wday+6)%7`. • `%V`: `d = yday − (wday?wday:7) + 4`. If `d < 0` → it belongs to the previous ISO year (`d += 365+leap(y−1)`). If `d ≥ 365+leap(y)` → next year (`d -= 365+leap(y)`). Week = `d/7+1`. • `#` = no zero/space padding for d D e F H I j m M r R S T U V W y Y (and C). • C-locale names: `Sun Mon Tue Wed Thu Fri Sat` / `Sunday … Saturday` / `Jan … Dec` / `January … December` / `AM PM`. • Linux `%Z`: map the IANA zone to the Windows standard name via CLDR `windowsZones.xml` (e.g. `Europe/Berlin` → `W. Europe Standard Time`). Fall back to the tz abbreviation.
**FormatLocale** (non-C locale). UCRT fills names from `GetLocaleInfoEx`: `LOCALE_SDAYNAMEn`, `SABBREVDAYNAMEn`, `SMONTHNAMEn` (**nominative**), `SABBREVMONTHNAMEn`, `S1159/S2359`, and date pictures `LOCALE_SSHORTDATE`, `SLONGDATE`, `STIMEFORMAT`. `%c` = shortdate + " " + time, `%#c` = longdate + " " + time, `%x`/`%#x`/`%X`/`%r` use those pictures. • Picture letters: `d`, `dd`, `ddd` (abbr day), `dddd` (day), `M`, `MM`, `MMM`, `MMMM`, `yy`, `yyyy`, `h`, `hh`, `H`, `HH`, `m`, `mm`, `s`, `ss`, `t` (first char of AM/PM), `tt`, and `'literal'`. • Accepted names: BCP-47 (`fr-FR`, `de-DE`, `ja-JP`), CRT names (`French`, `German_Germany`), `local`. • Linux mapping: `xx-YY` → `xx_YY.UTF-8` via `newlocale`/`nl_langinfo`. **Use `ALTMON_n`/`_NL_ABALTMON_n` (nominative), not `MON_n`**: glibc ≥2.27 `%B`/MON is genitive for ru, pl, cs, uk, el, ca …, while Windows `%B` is nominative. Example fr-FR: `%A %d %B` → `mardi 05 mars`, `%c` → `05/03/2024 07:08:09`.
**TimeStampFormat parsing** (4.5: `std::get_time` with `TimeStampLocale`; HEAD: Common/DateTimeParser.cpp). • Supported directives: `%a %A %b %B %h %c %x %X %C %d %e %D %F %g %G %H %I %j %m %M %n %t %p %r %R %S %T %u %U %V %w %W %y %Y %z %Z %%`. • Missing fields default to 1900-01-01 00:00:00. `%y` 00-68 → 20xx, 69-99 → 19xx (DateTimeParser.cpp:878). Prefix match: trailing input is ignored. Whitespace in the format matches any run of whitespace. • Result: a local wall-clock fixed timestamp. HEAD converts to UTC only if `%z`/`%Z` is present. • Failure: log `Invalid TimeStampFormat: …` (HEAD has more specific messages), TimeStamp = 0 (1601-01-01). • HEAD only: `TimeStampFormat=locale-date|locale-time` → `%x`/`%X` in the user locale. • Example: `TimeStamp=2024-03-05 07:08:09`, `TimeStampFormat=%Y-%m-%d %H:%M:%S` → value 13354096089.
**Linux implementation**: `clock_gettime(CLOCK_REALTIME)` + `localtime_r` (tm_gmtoff) for local; tzdata for DST transitions; a custom UCRT-compatible formatter (above); a strptime-like parser for TimeStampFormat. Keep all arithmetic in "local FILETIME" (i64 100 ns units since 1601) exactly like Rainmeter.

### 3.2 CPU (MeasureCPU.cpp)
- `Processor` (default 0): 0 = all CPUs (`GetSystemTimes`), N = logical CPU N (1-based, `NtQuerySystemInformation(SystemProcessorPerformanceInformation)[N-1]`). `<0` or `>count` → warning `CPU: Processor=%i is not valid`, falls back to 0 (MeasureCPU.cpp:61-66).
- `MaxValue` default **100** (MeasureCPU.cpp:50). MinValue 0.
- Value = `100 − 100·Δidle/Δ(kernel+user)`, clamped 0..100 (MeasureCPU.cpp:162-172). Windows kernel time includes idle. The previous sample starts at 0, so **the first update returns the average load since boot**, not 0. Changing `Processor` resets the history.
- No string value.
- **Linux**: `/proc/stat`. Use the `cpu` line for 0 and `cpuN-1` for N. `idle = idle + iowait`, `total = user+nice+system+idle+iowait+irq+softirq+steal` (exclude guest/guest_nice, which are already in user/nice). Value = `100·(1 − Δidle/Δtotal)`, clamped. First sample uses deltas from zero (since boot). If `Δtotal == 0`, Windows gives NaN→clamped. Return the previous value (or 0).

### 3.3 PhysicalMemory / Memory / SwapMemory (Measure*Memory.cpp)
All three take `Total` (default 0). With `Total=1` the value = MaxValue. **They ignore any `MaxValue=` in the skin**: ReadOptions restores the computed max (MeasureMemory.cpp:38-43). Values are in bytes. No string value.

| Measure | Windows value (`GlobalMemoryStatusEx`) | Windows MaxValue | Linux (`/proc/meminfo`) |
|---|---|---|---|
| `PhysicalMemory` | `TotalPhys − AvailPhys` = RAM in use | `TotalPhys`, fixed at construction (MeasurePhysicalMemory.cpp:13) | used = `MemTotal − MemAvailable`, max = `MemTotal` |
| `SwapMemory` (class "VirtualMemory") | `TotalPageFile − AvailPageFile` = **commit charge** (not swap usage!) | `TotalPageFile` = **commit limit** (RAM + pagefile), refreshed every update (MeasureVirtualMemory.cpp:25) | used = `(MemTotal−MemAvailable) + (SwapTotal−SwapFree)`, max = `MemTotal + SwapTotal` (bounded analog of commit charge/limit; `Committed_AS` is an alternative but can exceed the max) |
| `Memory` | commit charge + RAM in use (MeasureMemory.cpp:33) | commit limit + TotalPhys (MeasureMemory.cpp:25) | `Swap.used + Phys.used`, max = `Swap.max + Phys.max` |

(Values ×1024 from meminfo kB. Physical `Total=1` uses the value captured at load.)

### 3.4 FreeDiskSpace (MeasureDiskSpace.cpp)
| Option | Default | Meaning |
|---|---|---|
| `Drive` | `C:\` | a trailing `\` is appended if missing (MeasureDiskSpace.cpp:148-161). Empty → warning `FreeDiskSpace: Drive= empty`, value 0, max 0 |
| `Total` | 0 | 1 → value = total size instead of free |
| `Type` | 0 | 1 → value/string = drive type |
| `Label` | 0 | 1 → string value = volume label |
| `IgnoreRemovable` | 1 | removable drives give 0 / "" (avoids spinning up or "insert disk" prompts) |
| `DiskQuota` | 1 | 1 → free = bytes available **to the user**, total = total quota-limited bytes. 0 → free = total free bytes (MeasureDiskSpace.cpp:87-94) |
- Value (bytes): free, or total if `Total=1`. CD-ROM, missing drive, or (removable & IgnoreRemovable) → 0 and MaxValue 0. **MaxValue = total bytes**, set at load and updated whenever the total changes. A skin `MaxValue=` is ignored. Bars therefore show *free* space: skins use `InvertMeasure=1` to show used space (Invert = `Max − v + Min` = used bytes).
- `Type=1`: value/string ∈ {0 `Error`, 1 `Removed` (no root dir / unknown), 3 `Removable`, 4 `Fixed`, 5 `Network`, 6 `CDRom`, 7 `Ram`} (MeasureDiskSpace.cpp:9-18, 40-75). Note value 2 is unused. Quirk: on first load MaxValue = total bytes, on DynamicVariables re-read it becomes 7.
- String value exists only when `Type=1` or `Label=1` (MeasureDiskSpace.cpp GetStringValue). Otherwise `[M]` is the number.
- **Linux**: `statvfs(mountpoint)`. Free = `f_bavail·f_frsize` (DiskQuota=1) or `f_bfree·f_frsize` (DiskQuota=0). Total = `f_blocks·f_frsize`.
- Drive mapping: `C:`/`C:\` (any case) → `/`. Other letters come from a user-configurable table (e.g. `D:` → `/home` or a mount from `/proc/self/mountinfo`). Unmapped → behave like a removed drive (0, Type=1 "Removed"). Also accept an absolute POSIX path or mountpoint directly.
- Type from `/proc/self/mountinfo` fstype: nfs/cifs/smb3/sshfs/fuse.sshfs → Network; tmpfs/ramfs → Ram; iso9660/udf → CDRom; block device with `/sys/block/<dev>/removable=1` or under `/media`/`/run/media` → Removable; else Fixed.
- Label: reverse lookup in `/dev/disk/by-label/` (unescape `\x20`), "" if none.

### 3.5 Uptime (MeasureUptime.cpp)
| Option | Default | Meaning |
|---|---|---|
| `Format` | `%4!i!d %3!i!:%2!02i!` | Win32 `FormatMessage` template (MeasureUptime.cpp:23) |
| `AddDaysToHours` | 1 | read **only if** Format contains no `%4` substring. Otherwise forced to 0 (MeasureUptime.cpp:24-32) |
| `SecondsValue` | (unset) | if present: value = `abs((int64)SecondsValue)` instead of the system uptime. Lets you format any duration (with DynamicVariables) (MeasureUptime.cpp:34-41) |
- Value = whole seconds since boot (`GetTickCount64()/1000`, which **includes sleep/hibernate**). MaxValue 1 (default).
- String: the arguments are an array of 4 integers `[%1]=s%60`, `[%2]=(s/60)%60`, `[%3]=s/3600` (or `%24` if AddDaysToHours=0), `[%4]=s/86400` (MeasureUptime.cpp:63-71), then Substitute.
- **FormatMessage emulation** (FORMAT_MESSAGE_FROM_STRING | ARGUMENT_ARRAY). Scan left to right: ▸ `%N` (N = 1-2 digits, 1..99) optionally followed by `!spec!`. `spec` is a printf conversion without `%` (flags `-+ 0#`, width, `.prec`, optional length `l`/`h`/`I64`/`ll`, conversion `d i u x X o c`). Apply it to argument N as an integer. **No `!spec!` means `!s!`**: Windows dereferences the integer as a string pointer, crashes, and Rainmeter catches it → logs `Uptime: "Format=…" invalid` and the string is **empty** (MeasureUptime.cpp:75-82). N > 4: reads past the array (undefined), so treat as invalid as well. ▸ Escapes: `%%`→`%`, `%n`→`\r\n`, `%r`→`\r`, `%t`→tab, `%0`→stop output here, `%.`→`.`, `%!`→`!`, `% `→space. Any other `%<char>` → `<char>` (Wine behavior, best effort). ▸ `*` width/precision consumes extra arguments. Not needed: treat as invalid. ▸ Examples for s = 93784 (1d 2h 3m 4s): default → `1d 2:03`. `%3!i!:%2!02i!:%1!02i!` → `26:03:04`. Same with `AddDaysToHours=0` → `2:03:04`. `%4!i! days, %3!i! hours` → `1 days, 2 hours`.
- **Linux**: `/proc/uptime` first field (or `clock_gettime(CLOCK_BOOTTIME)`), floor.

### 3.6 Calc (MeasureCalc.cpp)
| Option | Default | Meaning |
|---|---|---|
| `Formula` | "" | MathParser expression, **no outer parentheses required** (empty → 0) |
| `LowBound` / `HighBound` | 0 / 100 | inclusive integer range of `Random` |
| `UpdateRandom` | 0 | 0: each `Random` token is replaced by a number **once**, when options are (re)read. 1: a new number on every evaluation |
| `UniqueRandom` | 0 | draw without replacement from a shuffled `[Low..High]` deck, which refills when empty. Only honored if the range ≤ 65535 (MeasureCalc.cpp:66-67) |
- Evaluation every update: `m_Value = Parse(Formula)` (MeasureCalc.cpp:31-46). On a parse error the value keeps its previous value and the error is logged once (`Calc: <msg>`) until it succeeds again. A formula that fails the syntax `Check` at read time is logged and **cleared** (value then 0).
- Identifier resolution order (MeasureCalc.cpp:143-176):
  1. **measure name** (case-insensitive, any type) → that measure's `GetValue()` (after Invert; the value from this tick if the measure is above in the file, else last tick's).
  2. `Counter` → skin update counter (starts 0, incremented at the start of every skin update, so 1 on the first update; Skin.cpp:3446).
  3. `Random` → `GetRandom()`.
  4. (HEAD) `$var$` dollar variables. ▸ Quirk: (2) and (3) use `_wcsnicmp(str, "counter", len)`, so **any case-insensitive prefix** matches (`c`, `co`, `r`, `ran`…) unless a measure has that name.
- `[M]` inside a Formula is textual substitution at read time, frozen unless DynamicVariables=1. Bare names are live.
- Random (MeasureCalc.cpp:185-216): `Low ≥ High` → always returns LowBound. `Low > High` also logs `"LowBound" (%i) must be less then or equal to "HighBound" (%i)`. Else `uniform_int_distribution(Low, High)` on an mt19937 seeded with `time()`. `FormulaReplace` only replaces whole-word `random` (delimiters per MathParser). ▸ Quirk: with `DynamicVariables=1` and UpdateRandom=0, the stored formula (with the number substituted) differs from the freshly read one, so a **new** random number is drawn on every re-read (MeasureCalc.cpp:84-97).
- Initial value before the first update: 0. MaxValue default 1 (set MaxValue for bars). No string value. `[Calc]` → `%.5f` trimmed ("0.33333", "42").
- Operators/functions are shared with the skin formula parser (Common/MathParser.cpp:127-152): `atan2 atan cos sin tan abs exp ln log sqrt frac trunc floor ceil round(x[,dec]) asin acos rad deg sgn neg min max clamp`, constants `e pi`, `+ - * / % ^ ~ & | && || == != <> < > <= >= ?:`, numeric literals incl. `0x`, `0o`, `0b`. See the formula note for precedence.

### 3.7 String (MeasureString.cpp)
- `String` (default ""). String value = `String` after Substitute. Numeric value = leading-number parse of the **raw** (unsubstituted) string: 4.5 `_wtof` (C locale, `"12abc"`→12, `"abc"`→0, `" 3.5"`→3.5); HEAD `StringToNumber` honoring NumberConversionFormat (MeasureString.cpp:43).
- The initial string is published at load unless the measure starts Disabled/Paused (then ""), so meters can use it before the first update (MeasureString.cpp:24-32). The numeric value is 0 until the first update.
- Usual use: `String=#Var#` or `String=[OtherMeasure]` with DynamicVariables.

### 3.8 NetIn / NetOut / NetTotal (MeasureNet.cpp)
| Option | Default | Meaning |
|---|---|---|
| `Interface` | `BEST` | non-numeric: `Best` → interface of the default route (`GetBestInterface(INADDR_ANY)`). Any other text → first interface whose **Description**, then **Alias**, equals it case-insensitively (preferring connected ones). Not found → 0. Numeric N: 0 = **sum of all** interfaces except loopback and filter interfaces; N>0 = Windows ifIndex N (MeasureNet.cpp:239-247, Common/NetworkUtil.cpp:38-103) |
| `Cumulative` | 0 | 1 → value = total bytes since the statistics reset date, persisted in `Rainmeter.data [Statistics]` (`Since`, `Count`, `In<i>`, `Out<i>`) |
| `UseBits` | 0 | 1 → values ×8 |
| `MaxValue` / per-measure `NetInSpeed`, `NetOutSpeed`, `NetTotalSpeed` / global `[Rainmeter] NetInSpeed`, `NetOutSpeed` (Rainmeter.cpp:1721-1722, default 0) | — | precedence: the measure's `MaxValue`, then the measure's `Net…Speed`, then the global (NetTotal: in+out). The value is taken as **bits/s** and **divided by 8 unless UseBits=1, even when it came from `MaxValue=`** (MeasureNet.cpp:224-268). 0 → auto-max (§2.3) |
- **Value = bytes transferred since the previous update of this measure** (octet counter delta, MeasureNet.cpp:164-200). It is per update, not per second: with `Update=1000` it equals bytes/s, with `Update=500` it is half that, and UpdateDivider multiplies it. The first update returns 0. A counter going backwards gives 0 for that tick. NetTotal = in+out.
- No string value. Formatting via meter AutoScale (1024 → "1.5 M").
- **Linux**: `/proc/net/dev` (or `/sys/class/net/<if>/statistics/{rx,tx}_bytes`). Loopback is `lo` (type 772). Treat virtual "filter" duplicates analogously by skipping `lo` only, optionally docker/veth. `Best` = the iface of the lowest-metric `00000000` route in `/proc/net/route` (IPv6 fallback `/proc/net/ipv6_route`). Numeric N>0 → Linux ifindex (`/sys/class/net/*/ifindex`). Unmatched Windows description strings → fall back to 0/all (same as Windows "not found").
- Cumulative: keep per-interface u64 accumulators (increment only on positive deltas, MeasureNet.cpp:219-254) persisted in Snowmeter's data file. `!ResetStats` clears them.

### 3.9 Registry (MeasureRegistry.cpp) — stub on Linux
Options: `RegHKey` (`HKEY_CURRENT_USER` default; `HKEY_LOCAL_MACHINE`, `HKEY_CLASSES_ROOT`, `HKEY_CURRENT_CONFIG`, `HKEY_PERFORMANCE_DATA`, `HKEY_DYN_DATA`), `RegKey`, `RegValue` (empty = default value), `OutputType` (`Value`|`SubKeyList`|`ValueList`), `OutputDelimiter` (default `\n`).
- Windows semantics: DWORD/QWORD → number. SZ/EXPAND_SZ → string, and number = leading-number parse. MULTI_SZ → strings joined by OutputDelimiter, number from the first one. BINARY → uppercase hex string. Missing key/value → value 0 and **no string value** (`GetStringValue` returns null when the string is empty, MeasureRegistry.cpp:230-234), so `[M]` = `"0"`. MaxValue: 0 means auto-max (MeasureRegistry.cpp:217-221).
- **Stub**: value 0, no string value (so `[Reg]` → "0" and String meters show "0", exactly like a missing key on Windows). Optional: a small virtual registry for keys skins commonly read: ▸ `HKCU\Control Panel\International` (`sShortDate`, `sTimeFormat`, `iMeasure`…) ▸ `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` (`ProductName`, `CurrentBuild`) ▸ `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize` (`AppsUseLightTheme`, `SystemUsesLightTheme`) ▸ `HKCU\SOFTWARE\Microsoft\Windows\DWM` (`ColorizationColor`, `AccentColor`) ▸ `HKCU\Control Panel\Desktop` (`WallPaper`)

### 3.10 Script (Lua) (MeasureScript.cpp, LuaScript.cpp, LuaBinding*.cpp)
**Engine**: Lua **5.1** semantics (4.5: Lua 5.1.4 with mods; HEAD: LuaJIT). Use `mlua` with `lua51` or `luajit`.
- Rainmeter-specific library changes (4.5: Library/lua/LuaModifications.txt): base `print` and `collectgarbage` removed (print is re-registered, see below); `io.popen` and the std streams removed; the `package` library not opened (**no `require`**); `os.exit` removed; `os.setlocale` is a no-op returning nil. `dofile` is replaced (resolves relative to the CWD, honors BOM encodings, runs in the caller's environment) (LuaBindingGlobal.cpp:53-100). `tolua.cast(x)` returns x (compatibility).
- Each Script measure has its own Lua state in HEAD (LuaScript.cpp:44-51). In 4.5 each also gets `luaL_newstate` per script (4.5: lua/LuaScript.cpp:35). The script chunk runs with a private environment table whose `__index` is `_G` (LuaScript.cpp:76-85). So plain globals are per script, and `_G.x` writes the real globals (per state).
- Script file encoding: UTF-16LE BOM (4.5), or UTF-16LE/UTF-8 BOM (HEAD), → "Unicode mode": strings exchanged with Rainmeter are UTF-8. No BOM → ANSI mode: strings are converted with the system ANSI code page (Windows-1252 on Western systems) (LuaScript.cpp:39-41; 4.5 lua/LuaScript.cpp:56). Snowmeter: no BOM → decode Lua→Rainmeter strings as Windows-1252 (configurable), and BOM → UTF-8.
**Options**: `ScriptFile` (made absolute relative to the skin folder; changing it with DynamicVariables reloads the script). Any other option is readable from Lua via `SELF:GetOption`. Legacy: a global `PROPERTIES` table is filled from same-named options (ANSI scripts only) (MeasureScript.cpp:113-146).
- Load errors → `Script: File not valid` (MeasureScript.cpp:173) plus the Lua error. The measure then has value 0.
**Calls** • At load: the chunk runs during ReadOptions, then `Initialize()` runs at measure-initialize time, after meters exist (MeasureScript.cpp:30-41). • Each update: `Update()` if defined (MeasureScript.cpp:50-66). Up to **2 return values** are inspected (LuaScript.cpp:222-273): ▸ number → value ▸ string → string value. If no number was returned before it, value = `strtod(string)`. ▸ So `return 5` → value 5, no string. `return "abc"` → string "abc", value 0. `return "12 px"` → string "12 px", value 12. `return 3, "x"` → value 3, string "x". `nil` or no return → **previous value and string are kept, but the measure becomes "no string"**: `m_ValueType = NIL`, so `GetStringValue` is null and `[M]` shows the number. ▸ Deprecated `GetStringValue()` Lua function: called if Update returned nil (ANSI scripts only). • Errors in `Update` are logged `Script Update() failed: …` every update. • `!CommandMeasure ScriptMeasure "lua code"` → runs the string as a chunk in the script environment (MeasureScript.cpp:178-188). • `[ScriptMeasure:Func(arg1, 'str', (1+2))]` inline section variable → calls a global function (LuaScript.cpp:332-450). ▸ Arguments: quoted `'…'`/`"…"` → string. `true`/`false`/`nil` → those values. `(…)` → evaluated formula. Anything else → leading number (else 0). ▸ Return: number → `%.5f` trimmed. boolean → `1`/`0`. string → as is. nil or other types → error logged, the variable is left unexpanded. ▸ Before the measure is initialized it returns `"0"`. ▸ HEAD: `[ScriptMeasure:name]` (no parens) → Lua variable.
**Lua API**
- `print(...)` → Rainmeter log, **Debug** level. Arguments go through `tostring`, separated by 4 spaces (LuaBindingGlobal.cpp:22-46).
- `SKIN` (Skin object, metatable `MeterWindow`): ▸ `SKIN:Bang('!Bang arg …')` → the whole string is executed as an action. `SKIN:Bang('!Bang', 'arg1', 'arg2')` → bang with args. `#vars#` are replaced in all parts (LuaBindingSkin.cpp:13-42). ▸ `GetMeter(name)` / `GetMeasure(name)` → object or nil. ▸ `GetVariable(name[, default])` → string, `default`, or nil. ▸ `ReplaceVariables(str)` → `#var#` **and** `[Measure]` replaced. ▸ `ParseFormula(str)` → number. The string must be wrapped in `( )`, otherwise `tonumber(str)` is used (`"5"`→5). ▸ `MakePathAbsolute(path)` → relative to the skin folder. ▸ `MoveWindow(x, y)`, `FadeWindow(from, to)`, `GetX()`, `GetY()`, `GetW()`, `GetH()`. ▸ HEAD-only extras (harmless to add): `SetOption(section, opt, val)` and table form, `SetOptionGroup(group, …)`, `SetVariable(name, val)`, `ReadTextFile(path)` → text or nil,err, `WriteTextFile(path, text[, 'ANSI'|'UTF-8'|'UTF-16'])`.
- `SELF` (Measure object of this script), and any measure from `SKIN:GetMeasure`: ▸ `GetName()` ▸ `GetOption(key[, default=""])` (string, variables and measures substituted) ▸ `GetNumberOption(key[, default=0])` (formula ok) ▸ `GetValue()` (inverted), `GetRelativeValue()`, `GetValueRange()`, `GetMinValue()`, `GetMaxValue()` ▸ `GetStringValue([autoScale[, scale[, decimals[, percentual]]]])`: string value, or the number formatted with those args. **With no args the defaults are AutoScale off, scale 1, decimals 0**, so `CPU:GetStringValue()` → `"37"` (rounded) (LuaBindingMeasure.cpp:107-117). ▸ `Disable()`, `Enable()` ▸ HEAD: `SetOption`.
- Meter objects (`SKIN:GetMeter`): ▸ `GetName()`, `GetOption(key[, default])` (honors MeterStyle) ▸ `GetX([absolute])`, `GetY([absolute])`, `GetW()`, `GetH()`, `SetX(n)`, `SetY(n)`, `SetW(n)`, `SetH(n)` ▸ `Hide()`, `Show()`, `SetText(str)` (String/TextEdit meters only) ▸ HEAD: `SetOption`.
- Global lua libs available: `string`, `table`, `math`, `os` (date/time/clock/getenv/remove/rename/tmpname), `io` (file ops). The `os.date` format follows the same MSVC strftime rules as §3.1, so reuse that formatter.

### 3.11 WebParser (4.5: Library/MeasureWebParser.cpp; HEAD: MeasureWebParser.cpp)
| Option | Default | Meaning |
|---|---|---|
| `Url` | "" | http(s)/ftp/`file://` URL. Read **without** old-style `[Measure]` substitution (new-style `[&M]`/`[#V]` are expanded) (MeasureWebParser.cpp:303-311). **Any `[` in the Url means the measure never fetches by itself.** `Url=[Parent]` makes it a **child** |
| `RegExp` | "" | PCRE (no flags, UTF-16 in 4.5) applied to the decoded page. Skins use `(?siU)` |
| `StringIndex` | 0 | capture group used for this measure's result (0 = whole match). Negative → 0 |
| `StringIndex2` | 0 | for a child **with its own RegExp**: the group of the child's match to keep |
| `UpdateRate` | 600 | refetch every N **measure updates** (counter of UpdateValue calls; affected by UpdateDivider). 600 × 1 s = 10 min |
| `FinishAction` | "" | runs (delayed to the main thread) after a successful parse (non-download), or after a download completes |
| `OnRegExpErrorAction` | "" | regex compile error or no match |
| `OnConnectErrorAction` | "" | fetch failed |
| `OnDownloadErrorAction` | "" | download failed |
| `ErrorString` | "" | result string on regex no-match (for the measure and all its children) |
| `LogSubstringErrors` | 1 | log `Not enough substrings` when StringIndex ≥ captures |
| `CodePage` | 0 = UTF-8 | code page used to decode the bytes. 1200 = UTF-16LE; others e.g. 1252 (MeasureWebParser.cpp:375-379, 593) |
| `DecodeCharacterReference` | 0 | 1 = numeric + named entities, 2 = numeric only, 3 = named only. Applied to the result. Numeric refs ≥ U+FFFE (astral, e.g. `&#128512;`) are **not** decoded (Common/CharacterEntityReference.cpp:302-383) |
| `DecodeCodePoints` | 0 | 1 → also decode `\uXXXX` / `\UXXXXXXXX` (BMP < U+FFFE only) |
| `Header`, `Header2`, `Header3`… | — | extra request headers, contiguous numbering |
| `UserAgent` | global or `Rainmeter WebParser plugin` | per measure. Global: `[WebParser] UserAgent=` in Rainmeter.data |
| `ProxyServer` | `/auto` | `/auto` (system), `/none`, or `host:port`. Global `[WebParser] ProxyServer` |
| `Download` | 0 | 1 → download the resource and the string value = local file path |
| `DownloadFile` | "" | file name inside the skin's `DownloadFile\` folder (`#CURRENTPATH#DownloadFile\`). Empty → temp cache file deleted when the measure is destroyed |
| `Debug` | 0 | 1 = log fetch + capture groups (when Rainmeter debug mode is on). 2 = also dump raw bytes to `Debug2File` (default `WebParserDump.txt`, relative to the skin) |
| `Flags` | "" | WinINet flags, `|`-separated: `ForceReload Resync NoCookies Hyperlink TempFile NoAuth NoCacheWrite PragmaNoCache Secure IgnoreCertName IgnoreCertDate IgnoreHTTPRedirect IgnoreHTTPSRedirect`. Map ForceReload/PragmaNoCache to no-cache. `ForceReload=1` is deprecated |
| HEAD only | | `JsonPointer=/a/0/b` (RFC 6901, alternative to RegExp; children resolve against the parent node; `JsonPointer=1` = parse only), `NumberConversionFormat` |

**Semantics** (4.5: MeasureWebParser.cpp:452-746):
- Fetch is asynchronous. `UpdateValue` sets `value = wcstod(resultString)` from the **current** result (0 if none), and starts a fetch when the update counter is 0 and no fetch is running. So the value/string become available on a later update, after the FinishAction.
- Parse: decode the bytes, run the regex once (first match only). Results: ▸ Match → result = group `StringIndex` (decoded entities), else "" + warning. ▸ No match → log `RegExp matching error (-1)`, result = `ErrorString`, children too, OnRegExpErrorAction. ▸ Invalid regex → `RegExp error at offset …`, OnRegExpErrorAction. ▸ **Empty `RegExp`** matches the empty string, so the result is "".
- Children: every WebParser measure **in the same skin** whose Url contains `[ParentName]` (case-insensitive) is resolved immediately inside the parent's parse: ▸ Child **without** RegExp → child result = its Url with `[ParentName]` replaced by parent group `child.StringIndex`, then decoded. If the child has `Download=1` it then downloads that URL. This is how image URLs are scraped: `Url=[MeasureParent]`, `StringIndex=3`, `Download=1`. ▸ Child **with** RegExp → the child regex runs on parent group `child.StringIndex`, and its result is the child's group `StringIndex2`. Recursive: grandchildren hang off the child. The child's own FinishAction/error actions fire. ▸ Child StringIndex ≥ parent captures → child result "" (+warning).
- The parent's FinishAction runs after all children are set.
- String value = result (or the downloaded file path if `Download=1`), then this measure's Substitute. Children receive the parent's **raw** capture (parent Substitute not applied).
- Value = `wcstod(result)` (4.5) / `StringToNumber` (HEAD).
- `!CommandMeasure M "Update"` → abort running tasks and refetch on the next update. `"Reset"` → clear the results of the measure and its children. HEAD also has bangs `!WebParser:Refetch`, `!WebParser:ClearResults`.
- Redirects follow WinINet defaults (http↔https allowed unless the Ignore flags are set). Cookies use the WinINet store.
- **Linux implementation**: `reqwest`/`ureq` on a worker thread, with gzip/deflate/br and redirects. Map `file://` to direct reads (Windows `file://C:/…` and `file:///#CURRENTPATH#…` paths need path translation). Decode with `encoding_rs` by CodePage (0/65001 UTF-8, 1200 UTF-16LE, 1201 UTF-16BE, 125x…). Use **PCRE2** (`pcre2` crate, UTF mode) for regex compatibility. Run FinishAction on the GTK main loop.

### 3.12 Plugin (MeasurePlugin.cpp)
- `Plugin=` path → DLL looked up in `<Rainmeter>\Plugins\`, then `%APPDATA%\Rainmeter\Plugins\`. A missing extension gets `.dll` appended (MeasurePlugin.cpp:93-135).
- DLL missing / cannot load: log **Error** `Plugin: Unable to load "Foo.dll" (error 126)` (4.5 text; HEAD `Plugin: Unable to find DLL for "Foo.dll" (error 126)`), then return. **The measure still exists** with value 0 forever, no string value (`GetString` is null), so `[M]` → `"0"` and a String meter shows "0" (with its NumOfDecimals). MaxValue = `MaxValue=` option or 1. `!CommandMeasure` logs `!CommandMeasure: Not supported` (Measure.cpp:873-876). `[M:Func()]` → error `Cannot find function` (no-op). IfMatch sees "".
- Loaded DLL (Windows only): `Initialize`/`Reload`/`Update`/`GetString`/`ExecuteBang`/`Finalize`, plus custom exported functions callable via `[M:Func(a,b)]`. MaxValue: from Reload's `*maxValue` if no MaxValue= option; 0 → auto-max (MeasurePlugin.cpp:190-202).
- **Snowmeter**: we cannot run Windows DLLs. Behave exactly as "DLL missing" (log once per measure), unless the plugin name maps to a native reimplementation (§1.2, §4). Optionally allow native Linux plugins later via the same C API (RmReadString etc.) using `.so`.

### 3.13 Loop (MeasureLoop.cpp; internal in 4.5 too)
- Options: `StartValue` (1), `EndValue` (100), `Increment` (1, may be negative), `LoopCount` (0 = infinite). Min/Max are set automatically to the start/end range (MeasureLoop.cpp:95-107), and any `MinValue`/`MaxValue` is overwritten.
- Sequence: value = Start at load. The first update keeps Start (skip-first). Each later update adds Increment. On reaching/passing End the value becomes End exactly. The **next** update after End wraps to Start. Example Start=1, End=5 → `1,2,3,4,5,1,2,3,4,5,…`. Increment=2, End=6 → `1,3,5,6,1,3,…`. With `LoopCount=1` it stops at 5.
- `InvertMeasure=1` changes the wrap logic (holds End then jumps to Start) and the reported value is inverted.
- Any change to Start/End/Increment/LoopCount/Invert on re-read resets the sequence.
- `!CommandMeasure M "Reset"` (HEAD also `!Loop:Reset`).
- Linux: pure logic.

## 4. Other built-ins + plugin inventory
Installed `Plugins\` DLLs (4.5.27): ActionTimer, AdvancedCPU, AudioLevel, CoreTemp, FileView, FolderInfo, InputText, iTunesPlugin, PerfMon, PingPlugin, PowerPlugin, QuotePlugin, ResMon, RunCommand, SpeedFanPlugin, UsageMonitor, Win7AudioPlugin, WindowMessagePlugin. Internal to `Rainmeter.dll` in 4.5 (no DLL, `Plugin=X` is redirected): WebParser, NowPlaying, SysInfo, Process, RecycleManager, WifiStatus, MediaKey. Anything else is third-party (Chameleon, HotKey, WebNowPlaying, MagickMeter, FrostedGlass…) and gets the placeholder behavior of §3.12.

| Name (`Plugin=` DLL) | Purpose / key options | Linux |
|---|---|---|
| ActionTimer (dll) | `ActionList1..N = action \| Wait ms \| Repeat X,ms,n` lists. `!CommandMeasure M "Execute N"`/`"Stop N"` | native (timers) |
| AdvancedCPU (dll) | `CPUInclude`/`CPUExclude` (process names `;`-sep), `TopProcess` 0 = summed %, 1 = top process (name as string) | native, `/proc/[pid]/stat` |
| AudioLevel (dll) | loopback analysis: `Port=Output\|Input`, `Type=RMS\|Peak\|FFT\|Band`, `Channel`, `FFTSize`, `Bands`, `FreqMin/Max`, attack/decay, `Sensitivity`, children via `Parent=` | native, PipeWire/Pulse monitor + FFT |
| Audio (Win7AudioPlugin) | default output volume (value 0-100) + device name (string). Cmds `ToggleMute`, `Mute`, `Unmute`, `SetVolume N`, `ChangeVolume ±N`, `SetOutputIndex N`, `ToggleNext/Previous` | native, libpulse/wpctl |
| CoreTemp (dll) | reads the Core Temp app: `CoreTempType=Temperature\|MaxTemperature\|TjMax\|Load\|CpuSpeed\|…`, `CoreTempIndex` | partial, `/sys/class/hwmon` coretemp/k10temp |
| FileView (dll) | folder browser: `Path`, `Count`, `Recursive`, `Sort*`, `Show*`, child `Type=FileName\|FileSize\|Icon…`, `Index` | native, std::fs + GIO icons |
| FolderInfo (dll) | `Folder`, `InfoType=FolderSize\|FileCount\|FolderCount` (+`Str`), `RegExpFilter`, `IncludeSubFolders/HiddenFiles/SystemFiles` | native |
| InputText (dll) | popup edit box. `!CommandMeasure M "ExecuteBatch 1-3"` runs `Command1..N` with `$UserInput$`. `DefaultValue X Y W H FontFace FontSize FontColor SolidColor StringStyle StringAlign InputLimit InputNumber Password FocusDismiss TopMost`, `OnDismissAction` | native, GTK popup |
| iTunes (iTunesPlugin) | iTunes COM | placeholder (or MPRIS) |
| MediaKey (internal) | `!CommandMeasure` `NextTrack PrevTrack Stop PlayPause VolumeMute VolumeDown VolumeUp` | native, MPRIS + Pulse |
| Mouse (HEAD) | `MouseMoveAction`, `*MouseDragAction`, `$MouseX$/$MouseY$`, `RelativeToSkin` | partial (X11 pointer; Wayland limited) |
| NowPlaying (internal) | `PlayerName=AIMP\|CAD\|foobar2000\|iTunes\|MediaMonkey\|Winamp\|WMP\|WLM\|Spotify` or `[Parent]`. `PlayerType=ARTIST TITLE ALBUM COVER DURATION POSITION PROGRESS RATING STATE STATUS VOLUME SHUFFLE REPEAT LYRICS FILE NUMBER YEAR GENRE`. `DisableLeadingZero`, `TrackChangeAction`, `PlayerPath`. DURATION/POSITION string `m:ss` | native, MPRIS for every PlayerName (pick the active player) |
| PerfMon (dll) | Windows perf counters `PerfMonObject/Counter/Instance`, `PerfMonDifference` | placeholder (0); map a few counters |
| Ping (PingPlugin) | `DestAddress`, `UpdateRate` (32), `Timeout` (30000), `TimeoutValue` (30000), `FinishAction`. Value = RTT ms | native, ICMP DGRAM socket or `ping` |
| Power (PowerPlugin) | `PowerState=ACLINE\|STATUS\|STATUS2\|LIFETIME\|PERCENT\|MHZ\|HZ`, `Format` (LIFETIME, default `%H:%M`) | native, `/sys/class/power_supply`, cpufreq |
| Process (internal) | `ProcessName=foo.exe` → 1 if running, else −1 | native, `/proc/*/comm` (strip `.exe`, case-insensitive) |
| Quote (QuotePlugin) | random line of `PathName` (file, `Separator`) or random file in a folder (`FileFilter`, `Subfolders`) | native |
| RecycleManager (internal) | `RecycleType=Count\|Size`. Cmds `EmptyBin`, `EmptyBinSilent`, `OpenBin` | native, XDG Trash |
| ResMon (dll) | `ResCountType=GDI\|USER\|Handle\|Window`, `ProcessName` | placeholder (Handle ≈ `/proc/pid/fd` count) |
| RunCommand (dll) | `Program` (default `cmd /U /C`), `Parameter`, `StartInFolder`, `OutputFile`, `OutputType`, `State`, `Timeout`, `FinishAction`. Cmds `Run`, `Close`, `Kill`. String = stdout | partial: `/bin/sh -c` (cmd-isms won't port; optionally via wine) |
| SpeedFan (SpeedFanPlugin) | SpeedFan shared memory `SpeedFanType/Number/Scale` | placeholder (or hwmon) |
| SysInfo (internal) | `SysInfoType=COMPUTER_NAME USER_NAME OS_VERSION OS_BITS SCREEN_WIDTH/HEIGHT WORK_AREA_* VIRTUAL_SCREEN_* NUM_MONITORS IP_ADDRESS MAC_ADDRESS GATEWAY_ADDRESS NET_MASK DNS_SERVER HOST_NAME DOMAIN_NAME ADAPTER_* INTERNET_CONNECTIVITY LAN_CONNECTIVITY TIMEZONE_* IDLE_TIME USER_LOGONTIME LAST_SLEEP/WAKE_TIME`, `SysInfoData` (adapter `BEST`/index, monitor index) | native, mostly (getifaddrs, GDK monitors, XScreenSaver idle) |
| UsageMonitor (dll) | perf-counter based `Alias=CPU\|Memory\|IO\|GPU\|VRAM\|Network…`, `Index`, `Name`, `Blacklist` (`_Total\|Idle`), `Percent` | partial, `/proc` per-process (GPU vendor-specific) |
| WifiStatus (internal) | `WifiInfoType=SSID\|QUALITY\|ENCRYPTION\|AUTH\|LIST\|PHY\|TXRATE\|RXRATE`, `WifiIntfID`, `WifiListStyle`, `WifiListLimit` | native, NetworkManager D-Bus / nl80211 |
| WindowMessage (WindowMessagePlugin) | `SendMessage` to a window (`WindowName/WindowClass`, `WindowMessage=msg wp lp`) | placeholder (0) |
| DragDrop (HEAD), Loop, Calc… | see above | native |
Placeholder = log once at load (`<Name>: not supported on Linux`), value 0, no string (`[M]`→"0"), commands ignored.

---

## 5. Implementation checklist
1. `Measure=` factory (case-insensitive, §1.1) + `Measure=Plugin` redirect table (HEAD list incl. `*Plugin` DLL names, §1.2); unknown type → log, no section.
2. Base `Measure` struct: raw value, min/max, invert, average ring, auto-max with **median-of-3**, string value `Option<String>`, IfActions, OnChange snapshot, Substitute list, divider counter, disabled/paused.
3. Update loop in INI order: divider → UpdateValue → average → auto-max → (DynVars reread conditions) → IfAbove/Below/Equal → IfCondition → IfMatch → OnUpdateAction → OnChangeAction; measures before meters; forced first update; `UpdateDivider<0` = forced only.
4. Substitute parser (4.5 quoting rules) + plain & PCRE2 regex application (NOTEMPTY, `\N` backrefs, resume after replacement, invalid regex → plain).
5. `GetFormattedValue` (percentual / autoscale tables with " k/M/G/T" and trailing space / `%.5f` trim) and meter decimal defaults; Substitute on formatted numbers.
6. `[M]`, `[M:dec]`, `[M:%]`, `[M:/n]`, `[M:MaxValue]`, `[M:EscapeRegExp]`, `[M:EncodeUrl]`, `[M:TimeStamp]`, `[Script:Func()]`.
7. IfCondition via shared MathParser with bare measure names → `GetValue()`; true only on exactly 1.0, false on 0.0; latches; modes.
8. Time: local-FILETIME model, `TimeZone`/`DaylightSavingTime`/`TimeStamp`/DST keywords, **custom UCRT strftime** (C-locale names, `#` flag, `%c`=`%a %b %e %T %Y`, invalid → empty + log), `locale-date/time`, `FormatLocale` via glibc nominative month names, TimeStampFormat parser, `wcstod` value.
9. CPU (/proc/stat, first sample since boot), memory trio (meminfo mapping §3.3), FreeDiskSpace (statvfs + drive-letter map + Type/Label), Uptime (FormatMessage emulator), Net (per-update byte deltas, Best iface, bits/8 max rule, cumulative persisted).
10. Calc with Random/UniqueRandom/UpdateRandom/Counter (prefix quirk) semantics; String; Loop.
11. Script: Lua 5.1 (mlua lua51/luajit), per-script env table, stripped libs, `print`→debug log, Update 2-return rule, `!CommandMeasure` chunk, inline `[S:F()]`, full SKIN/SELF/Meter API (§3.10), ANSI vs BOM string conversion.
12. WebParser: async fetch, UpdateRate counter, PCRE2 regex, StringIndex/StringIndex2, children by `[Parent]` in Url, ErrorString, Download/DownloadFile, CodePage, DecodeCharacterReference, actions on main thread, `Update`/`Reset` commands.
13. Plugin: placeholder semantics identical to "DLL missing" (value 0, `[M]`="0", error logged once).
14. Registry stub (0 / no string) + optional virtual keys.
15. Native ports in priority order: NowPlaying(MPRIS), SysInfo, Power, Audio, MediaKey, Process, FolderInfo, FileView, Ping, ActionTimer, InputText, RecycleManager, WifiStatus, AdvancedCPU, RunCommand, AudioLevel, Quote, UsageMonitor; then placeholders.

## 6. Test vectors
**Time** (`TimeStamp=13354096089` = 2024-03-05 07:08:09, Tue; C locale). `Format` → string / value:
- `` (empty) → `07:08:09` / 13354096089. `[M:TimeStamp]` → `13354096089`.
- `%A` → `Tuesday` / 0. `%a, %d %b %Y` → `Tue, 05 Mar 2024`. `%B %#d` → `March 5`.
- `%#d` → `5` / 5. `%d.%m.%Y` → `05.03.2024` / 5.03 (wcstod stops at the 2nd dot). `%Y%m%d` → `20240305` / 20240305.
- `%H:%M` → `07:08` / 7. `%#H:%M` → `7:08`. `%I:%M %p` → `07:08 AM`. `%#I` → `7`.
- `%j`/`%#j` → `065`/`65`. `%U %W %V %G %g %u %w` → `09 10 10 2024 24 2 2`. `%#U` → `9`.
- `%c` → `Tue Mar  5 07:08:09 2024`. `%#c` → `Tuesday, March 05, 2024 07:08:09`. `%x` → `03/05/24`. `%#x` → `Tuesday, March 05, 2024`. `%X`/`%#X` → `07:08:09`.
- `%D`/`%#D` → `03/05/24`/`3/5/24`. `%F`/`%#F` → `2024-03-05`/`2024-3-5`. `%T`/`%#T` → `07:08:09`/`7:8:9`. `%R`/`%#R` → `07:08`/`7:8`. `%r`/`%#r` → `07:08:09 AM`/`7:8:9 AM`. `%e`/`%#e` → ` 5`/`5` (value 5). `%C` → `20`. `%%` → `%`.
- `%k`, `%P`, `%s`, `%-d`, `abc%` → `` (empty) / 0 + log `Time: "Format=…" invalid`.
- 2024-12-30 (Mon): `%G %V %U %W %j` → `2025 01 52 53 365`. 2021-01-01 (Fri): `%G %V %U %W` → `2020 53 00 00`.
- `TimeStamp=2024-03-05 07:08:09`, `TimeStampFormat=%Y-%m-%d %H:%M:%S` → value 13354096089. US zone `TimeStamp=DSTStart2024` → 13354509600 (2024-03-10 02:00:00).
- 12:00 → `%I %p` = `12 PM`. 00:30 → `%I %p` = `12 AM`.
**Substitute** (plain unless noted):
- `"a":"b","b":"c"` on `ab` → `cc` (sequential). `"aa":"a"` on `aaaa` → `aa` (no re-scan).
- `"":"N/A"` on `` → `N/A`; on `x` → `x`. `"a,b":"c:d"` on `a,b` → `c:d`. `"first":'second'` on `first` → `second`.
- `'"':''` on `say "hi"` → `say hi`. `Substitute='a':'b'` → logged invalid, no-op. `Substitute=a:b` → invalid.
- AutoScale=1, value 1536, `" k":"KB"` → `1.5KB`.
- Regex: `"(\d+)":"<\1>"` on `a12b` → `a<12>b`. `"^(.*?)\s*-\s*(.*)$":"\2 by \1"` on `Artist - Title` → `Title by Artist`. `"a*":"x"` on `baaac` → `bxc` (NOTEMPTY). `"":"Empty"` on `` → `Empty`. `"(a)|(b)":"[\2]"` on `a` → `[\2]`. `"[":"x"` on `a[b[` → `axbx` (invalid regex → plain). `"(?i)TUE":"Di"` on `Tue` → `Di`.
**IfCondition** (`IfCondition=MeasureCPU > 50`, `IfTrueAction=T`, `IfFalseAction=F`): CPU 10,60,70,40,30 → F,T,–,F,– (mode 0); F,T,T,F,F (`IfConditionMode=1`). `IfCondition=MeasureCPU` (37) → nothing. `IfCondition2` with no actions → chain ends; `IfCondition3` ignored. `IfEqualValue=5`, values 5.9, 5.2, 6, 5 → fire, –, (reset), fire. `IfAboveValue=50`, values 60,70,40,60 → fire,–,–,fire. IfMatch `^$` on a CPU measure → always matches.
**Uptime** (s = 93784): default → `1d 2:03`. `%3!i!:%2!02i!:%1!02i!` → `26:03:04` (AddDaysToHours=0 → `2:03:04`). `%4!i! days, %3!i! hours` → `1 days, 2 hours`. `SecondsValue=3661`, `%3!i!:%2!02i!:%1!02i!` → `1:01:01`. `SecondsValue=-90` → value 90, default → `0d 0:01`. `Format=%1` → `` + log. `%4!i!d%n%3!i!h` → `1d\r\n2h`. `100%%` → `100%`.
**Calc**: `LowBound=1 HighBound=6 Formula=Random` → integer in [1,6], same every update until refresh/re-read; `UpdateRandom=1` → new each update. `Low=High=5` → 5. `Low=7 High=3` → 7 + error. `UniqueRandom=1 Low=1 High=3` → every aligned triple is a permutation of {1,2,3}. `Formula=Counter % 2` → 1,0,1,0… `Formula=1/3` → `[Calc]` = `0.33333`, `[Calc:2]` = `0.33`. `Formula=` (empty) → 0. `Formula=MeasureLoop*2` where Loop above in file → uses this tick's Loop value.
**Formatting / misc**: value 2.5, NumOfDecimals=0 → `2`. 1536 AutoScale=1 (no NumOfDecimals) → `1.5 k`; AutoScale=1k, 500 → `0.5 k`; AutoScale=2, 1500000 → `1.5 M`; AutoScale=1, 500 → `500.0 ` (trailing space). Percentual, value 37 of 0..100 → `37`. `[M]` of 37.5 → `37.5`; of 100 → `100`. Net counter 1000,1500,1400,1600 → values 0,500,0,200. Loop Start=1 End=5 → 1,2,3,4,5,1,2. Missing plugin → `[M]` = `0`. Script `return "12 px"` → string `12 px`, value 12.