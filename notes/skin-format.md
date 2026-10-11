# Rainmeter skin format: behavioral spec for Snowmeter

Source of truth: the C++ tree at `rainmeter-src/` (paths below are relative to it). Every
`file:line` refers to that tree. This spec describes what the code does, including its quirks.
Where it differs from what skin authors might expect, that is called out.

**About the source tree.** This is a modernized Rainmeter (the startup check needs Windows 10 1903+,
`Library/Rainmeter.cpp:206`). It has features that stock 4.5 does not: `SyntaxVersion=2`,
`[$Skin…]`/`[$Display…]` variables, `@Inherit`, `Group=` selectors in bangs, `Type:Bang` section
bangs, `!Skin:` bangs, `Update=N | Invisible M`, TextEdit/Svg meters, and others. These are tagged
**(fork)**. Unmodified Windows skins will almost never use them. Implement them last, or not at all.

Terminology: *config* = a skin folder path relative to the Skins folder (`illustro\Clock`). *Skin* =
one loaded `.ini` of a config. *Section* = a measure or meter. *Option* = key.

---

## 1. INI reading

Reader: `Common/IniFile.{h,cpp}` (`DecodedText::Parse`), called twice per file by
`ConfigParser::ReadIniFile` (`Library/ConfigParser.cpp:1647-1816`).

### 1.1 Encoding detection (`Common/IniFile.cpp:21-104`)
Apply these checks in order to the raw bytes:
1. `FE FF` (UTF-16BE BOM): the file is **rejected**, giving "Unable to read file" (`IniFile.cpp:24`, `ConfigParser.cpp:1656-1660`).
2. `FF FE`: UTF-16LE. An odd byte count makes the file unreadable (`IniFile.cpp:27-30`). The BOM is skipped (`:62-64`).
3. `EF BB BF`: UTF-8 with BOM. Decoding is strict, and invalid UTF-8 after a BOM makes the file unreadable (`:31-34`, `:86-93`).
4. UTF-16LE without a BOM is detected by `IsTextUnicode(IS_TEXT_UNICODE_ASCII16 | IS_TEXT_UNICODE_CONTROLS)` (`:41-45`).
   That needs NUL bytes in UTF-16 positions, so ANSI or UTF-8 text never matches. A practical Rust
   equivalent: the byte length is even and ≥2, and either every code unit is ≤0x7F with a zero high
   byte, or the code units include CR/LF/TAB/SPACE (0x000D/0x000A/0x0009/0x0020).
5. Otherwise, try **strict UTF-8** (`MB_ERR_INVALID_CHARS`). If that succeeds the file is UTF-8 (`:86-92`).
6. Otherwise the file is ANSI and is decoded with the process codepage, with no errors raised (`:99-103`).
   For Snowmeter, use **Windows-1252** (undefined bytes map to C1 controls). Test vector: bytes
   `C3 A9 FF` are ANSI (`Common/IniFile_Test.cpp:320-326`).
- A BOM-only file is a valid empty file (`IniFile.cpp:106-111`).
- Rainmeter converts its own `Rainmeter.ini` and `Rainmeter.data` to UTF-16LE with a BOM at startup
  (`Library/Rainmeter.cpp:383-384, 2364-2396`). Snowmeter must *read* a UTF-16 Rainmeter.ini, but it
  can keep its own file in UTF-8.
- Writes (`!WriteKeyValue`, settings) keep the existing encoding and BOM. A new file is created as
  ANSI without a BOM (`IniFile.h:48-49`, `IniFile.cpp:297-357`). An ASCII-only file counts as valid
  UTF-8, so non-ASCII written into it is UTF-8 (`IniFile_Test.cpp:290-296`).

### 1.2 Lines, comments, sections, keys (`Common/IniFile.h:109-152`, `IniFile.cpp:683-721`)
- CR, LF and CRLF all end a line, and they can be mixed. `0x1A`, U+2028 and U+0085 are not line
  breaks (`IniFile.h:125-128`).
- **Trim** = remove every leading and trailing char ≤ U+0020. U+00A0 and U+3000 are kept (`IniFile.cpp:682-692`).
- A line is trimmed first. Empty lines and lines whose first char is `;` are dropped. `#` is **not** a
  comment, and a `;` later in the line is ordinary text (`IniFile.h:130-131`). Example:
  `One = 1 ; comment` → value `1 ; comment` (`IniFile_Test.cpp:136-142`).
- Section header: a trimmed line starting with `[`. The name runs to the first `]`, or to the end of
  the line if there is none, and is trimmed. Text after `]` is ignored (`IniFile.cpp:694-704`).
  `[]` opens a section with an empty name.
- Key line: split at the **first** `=`, then trim both the key and the value (`IniFile.cpp:706-715`).
  Lines without `=` are ignored. Key lines before the first header are ignored (`IniFile.h:143-145`).
  An empty key is ignored (`ConfigParser.cpp:1758`).
- **Quote stripping:** after trimming, if the value has length >1 and starts and ends with `"`, or
  starts and ends with `'`, remove exactly one outer pair (`IniFile.cpp:718-721`,
  `Common/StringUtil.cpp:64-78`). This is applied to every value.
  - `Text="  padded  "` → `  padded  ` (the inner whitespace is kept)
  - `Empty=""` → empty string (`IniFile_Test.cpp:78, 84`)
  - `"a" "b"` → `a" "b`
  - **Consequence:** `LeftMouseUpAction="C:\Program Files\x.exe"` loses its quotes, and §10.4 then splits it at the space.
- **No line continuation** exists.
- **No length limits on lines or values** (`IniFile.h:20-21`). Section and option **names** must be
  ≤ 255 UTF-16 code units. Longer names produce an invalid ID and are silently ignored
  (`Library/IniNameRegistry.cpp:34-56`, `ConfigParser.cpp:1693, 1760-1761`).
- An empty value (`Key=`) is *defined but empty*. `ReadString` returns `""`, and numeric, color and
  bool reads return the **default** (see §5). An empty value still **blocks inheritance** from
  MeterStyle (§6).

### 1.3 Case-insensitivity
- Section and option names are interned in a global registry after ASCII-only uppercasing (`a`-`z`
  → `A`-`Z`) (`IniNameRegistry.cpp:34-38`, `StringUtil.cpp:80-91`). So `Meter`, `METER` and `meter`
  are the same option, but `ä` and `Ä` are different names.
- The variable table and the section lookup table use `_wcsupr` (`Library/ConfigParser.h:275`,
  `ConfigParser.cpp:164, 195, 1336, 1342`). In the C locale that is effectively ASCII as well.
  **Implement ASCII case folding everywhere.**

### 1.4 Duplicates and order
- **Duplicate section in the same file:** only the first occurrence is read. *Every* key under a
  later header with the same name (case-insensitive) is ignored, including keys with new names
  (`ConfigParser.cpp:1695-1697, 1737`).
- **Duplicate key in one section block:** the first one wins (`ConfigParser.cpp:1763-1764`, `IniFile_Test.cpp:148-155`).
- **Across files** (main file plus `@Include`s): each accepted `key=value` is stored with
  `insert_or_assign` in parse order (`ConfigParser.cpp:1812`). **The value processed last wins.**
  "Processed" means a depth-first walk where an include is read at the point its `@Include` key
  appears (§2).
- `[Metadata]` values are not stored at all (`ConfigParser.cpp:1753, 1810`). Its `@Include` keys are
  still processed, because the `@Include` check runs before that return.
- The section *order* (which sets measure and meter creation order) is described in §2.3.

---

## 2. @Include

Code: `ConfigParser.cpp:1647-1816`.

### 2.1 Recognition and expansion
- Any key whose name starts with `@Include` (case-insensitive, ≥ 8 chars) is an include:
  `@Include`, `@Include2`, `@IncludeFoo`, `@include_x` (`:1766`). These keys work in **any section**,
  not just `[Variables]` or `[Rainmeter]`.
- Includes are processed during the second (key) pass, **in key order inside the file**. Numeric
  suffixes do not matter. Because duplicate keys are dropped first, each distinct `@IncludeX` name
  is used once per section block.
- An empty value is ignored (`:1768`).
- Path expansion (`:1770-1777`):
  1. `ReadVariables()` runs, so every `[Variables]` key processed *so far* (in any file) is
     defined (`:1771`).
  2. `ReplaceVariables(path, isNewStyle=true)` expands `#Var#`, `[#Var]`, built-ins (`#@#`,
     `#CURRENTPATH#`, …), `[\xNN]`, `[%ENV]`, and `%ENV%` when SyntaxVersion<2. **Section variables
     are not expanded** (`:1772`).
  3. If the result is not absolute (it contains `:\` or `:/`, or starts with two separators;
     `Common/PathUtil.cpp:23-28`), it is resolved against **the folder of the file that contains the
     `@Include` line**, not the skin's folder (`:1773-1777`).
- Nesting is allowed up to depth 100. Beyond that, an "infinite include loop" message box appears
  and the file is skipped (`:1649-1653`). A missing or unreadable include logs `Unable to read
  file: <path>` and is skipped (`:1655-1660`).

### 2.2 Merging
Included keys go into the same section/key store. The last processed write wins (§1.4). Example:
```
; main.ini
[Variables]
A=1
@Include=vars.inc      ; vars.inc: [Variables] A=2  B=2
B=3
```
The result is `A=2` (the include came after `A=1`) and `B=3` (the key after the include wins). A
section that only appears in the include is a new section.

### 2.3 Section order (measure/meter creation order) (`:1670-1705, 1779-1806`)
Pass 1 of each file registers that file's distinct sections at the current *insert position*. A
section name already registered by an earlier file keeps its original position (`:1700-1703`).

When pass 2 reaches a section's **first** `@Include`:
- If that section is the **last section of the current file**, the insert position becomes the
  **end of the global list** (`:1783-1787`). This applies even when the current file is itself an
  include (quirk inherited from stock).
- Otherwise the insert position is directly after that section's name in the global list
  (`:1788-1799`).

Later `@Include`s in the same section reuse that position. Their sections land after the earlier
include's sections, because the list iterator still points at the following section (`:1802-1806`).
The position is saved and restored around each nested read.

Examples (test vectors):
- `main: [A] @Include=x.inc; [B]` with `x.inc: [X1] [X2]` → `A, X1, X2, B`.
- `main: [A] @Include=x @Include2=y; [B]`, x=`[X]`, y=`[Y]` → `A, X, Y, B`.
- `main: [A] @Include=x; [B]`, x=`[X1] [X2] @Include=y` (y=`[Y]`) → `A, X1, X2, B, Y`.
  `X2` is the last section of x, so `Y` goes to the global end.
- Same, but the `@Include=y` is in `[X1]` → `A, X1, Y, X2, B`.
- `main: [A]…[Z] @Include=x` (include in the last section) → x's sections are appended after `Z`.

---

## 3. Variables

### 3.1 `[Variables]` (`ConfigParser.cpp:127-169, 1812-1814`)
- Every key stored in `[Variables]` is appended to an ordered list, which keeps duplicates from
  includes (`:1814`). `ReadVariables()` then walks that list in order. For each key it reads the
  *current* stored value through the normal reader with `sectionVariables=false` and calls
  `SetVariable(name, value)` (`:152-160`).
- Values are expanded **at the time they are read**, in list order: `#X#`, `[#X]`, `[\NN]`,
  `[%ENV]`, and `%ENV%` when SyntaxVersion<2. In `[Variables]` the new-style pass runs first, then
  old-style (`:1262-1266, 1231`). Section variables (`[Measure]`) are **not** expanded and stay
  literal, so they resolve wherever the variable is used.
- `ReadVariables()` runs before every `@Include` and once at the end of `Initialize`
  (`:144, 1771`). The variable table is not cleared between those calls.
- **Forward references:**
  - Old-style `A=#B#` defined before `B` keeps the literal `#B#`, because inserted values are never
    rescanned (§3.3). The exception is when a `ReadVariables()` call happened after `B` was stored,
    that is, an `@Include` processed later. Then the final pass resolves it.
  - New-style `A=[#B]` keeps `[#B]` literally, but options resolve it late (§3.4). So **new-style
    forward references work** in ordinary options.
- Variable names are case-insensitive. The first spelling is remembered for display (`:162-169`).
- `#CURRENTSECTION#` inside `[Variables]` expands to `""` at read time (`:235`, no current section).
  To defer it, use `[#*CURRENTSECTION*]`, which is stored as `[#CURRENTSECTION]` (`:783-787`).
- `!SetVariable` writes only to the variable table, not to the raw `[Variables]` values. `!Refresh`
  restores the file values (`Library/Skin.cpp:2055-2070`, `ConfigParser.cpp:132-137`).

### 3.2 Lookup order (`ConfigParser.cpp:177-203`)
1. Built-ins (`GetBuiltInVariable`, `:215-247`)
2. `CURRENTCONFIGX/Y/WIDTH/HEIGHT/ZPOS` (`:471-484`)
3. Monitor variables (`:569-745`)
4. User variables

**Built-ins shadow user variables of the same name.** All matching is ASCII case-insensitive (StringParser default).

### 3.3 Old-style `#Var#` (`ConfigParser.cpp:759-829`)
Scan from the first special char. Find `#` at `start`, then the next `#` at `end`. The name is
the text between them.
- If the name is `*…*` (the name has ≥2 chars and both ends are `*`), delete the two `*` and continue
  after the result. This is the escape: `#*A*#` → `#A#`. It happens even if `A` is undefined, and
  it does not count as a replacement.
- Else, if the name is defined, replace `#name#` with the value and **continue after the inserted
  value**. Values are never rescanned.
- Else (unknown), leave it and continue the search **from the closing `#`**, which becomes a
  candidate opener. So `#Foo#A#` with A=abc → `#Fooabc`.
- Before old-style replacement, when SyntaxVersion<2, `%ENV%` is expanded over the whole string with
  `ExpandEnvironmentStrings`; `%APPDATA%` is special-cased (`:766`, `Common/PathUtil.cpp:111-164`).
  Unknown `%X%` is left as is. It only runs when two `%` exist with at least one char between them.
- Compatibility hack (`:777-799`): in non-new-style reads (actions) with a current section,
  `[#CURRENTSECTION]` (case-**sensitive**) is replaced by the section name during this scan.
- Fast path: values shorter than 3 chars are returned raw, and so are values that contain none of
  the special chars (`[%#` when SyntaxVersion<2, `[#` when ≥2; `[` only matters when section
  variables are on or in `[Variables]`) (`:747-752, 1288-1296`).

Test vectors (`Library/ConfigParser_Test.cpp:176-212`), with A=abc, BB=def, CCC=`#A#`:
`A`→`A` (false); `#A#`→`abc`; `#*A*#`→`#A#` (false);
`#A##a# #*CCC*# #BB##CCC#` → `abcabc #CCC# def#A#`.

### 3.4 New-style and section variables: `ExpandSectionVariables` (`ConfigParser.cpp:907-1110`)
Bracket forms are `[<key><body>]`, where key ∈ `&` (measure or section variable), `#` (variable),
`%` (environment variable), `$` (fork dollar or mouse variables), `\` (character reference) (`:27-44`).

Algorithm (innermost first, left to right):
```
end = start
while (end = find(']', end)) != npos:
  start = end
  loop: start = rfind('[', start-1)    ; stop when start == 0 has been tried
    if str[start+2]=='*' && str[end-1]=='*' && start+2 != end-1:   ; escape
        erase both '*' (except in DollarMouseOnly mode); break
    key = str[start+1]
    if start+1 == end: break             ; "[]"
    body = str[start+2 .. end)
    if body empty: break                 ; "[#]" stays literal
    if key not in {&,#,%,$,\}: continue  ; look further left for another '['
    try resolve (by key and mode); on success:
        if key is & or #: recursively expand value (depth+1, max depth 10)
        replace [..] with value; end = start + len(value) - 1; break
    (unresolved valid key: keep looking further left)
  end += 1
```
- **Recursion limit:** at depth 10 the value is inserted without expansion, and an error is logged
  if it still contains keyed brackets (`:909-910, 966-988`).
- **Modes:**
  - `AllKeys`: normal option reads and actions at execution.
  - `HashOnly`: `[Variables]` reads, `@Include`, `ReplaceVariables(…, true)`. Only `#`, `%`, `\`.
  - `DollarMouseOnly`: mouse action pre-pass. Only `$Mouse…` and `\`.
  - `\` character references are processed in **every** mode (`:994-998, 1068-1090`).
- `[#Name]`: same lookup as §3.2.
- `[%NAME]`: environment variable. A defined but empty variable gives `""` (`:1026-1037`).
- `[\NNN]` decimal or `[\xHH]` / `[\XHH]` hex. The value must parse fully and satisfy
  0 < ch < 0xFFFE, else it stays literal (`:1068-1090`).
- `[&Name]`: a measure name gives the measure's default string (§3.6). Otherwise it is a section
  variable with a selector (§3.6).
- `[$…]` (AllKeys): `[$Input]` / `[$Input:MatchN]` (TextEdit, fork), then `$Skin…` / `$Display…` (fork, `:487-567`).

Then **old-style section references** `[Name]` (`ReplaceMeasures`, `:836-900`). After the bracket
pass, scan from the first `[`. For `[…]` that contains no inner `[`:
- `[*…*]` → unescape to `[…]` and skip.
- Else, if a measure is named `…` (case-insensitive), replace it with the measure's default string
  (§3.6).
- Else `GetSectionVariable(…)` (selectors).
- Else skip.

Inserted values are not rescanned.

Nested test vectors (`ConfigParser_Test.cpp:214-472`), with Index=2, Var2=Second,
Name=`Var[#Index]`, Command=`return '[#Var[#Index]]'`. String measures: MeasureString2="Second",
MeasureIndex="2".

| input | function | output |
|---|---|---|
| `[#Var[#Index]]` | RV(new) | `Second` |
| `[#[#Name]]` | RV(new) | `Second` |
| `[#Command]` | RV(new) | `return 'Second'` |
| `[#*Var[#Index]*]` | RV(new) | `[#Var2]` (returns true) |
| `[#Var[&MeasureIndex]]` | RM | `Second` |
| `[&MeasureString[#Index]]` | RM | `Second` |
| `[&MeasureString[&MeasureIndex]]` | RM | `Second` |
| `[&MeasureScript:GetWelcome('[&MeasureString[#Index]]')]` | RM | `[&MeasureScript:GetWelcome('Second')]` (no script measure) |
| `[MeasureString2] [&MeasureString2] [#Var2]` | RM | `Second Second Second` |
| `[*MeasureString2*] [&MeasureString2]` | RM | `[MeasureString2] Second` |
| `[MeasureString[#Index]]` | RM | `Second` |
| `[MeasureString[MeasureIndex]]` | RM | `[MeasureString2]` (old-style is not rescanned) |
| `[M]` (measure M="Single") | RM | `Single` |
| `[#]` | RV(new) | `[#]` (false) |
| chain Chain1→`[#Chain2]`→`[#Chain3]`→Final | RV(new) | `Final` |
| Deep1→…→Deep13 chain | RV(new) | `[#Deep12]` (depth cap) |
| SelfRef=`[#SelfRef]` | RV(new) | `[#SelfRef]` (true) |
| `[\65][\x42][\X43]` | RV(new) | `ABC` |
| `[&MeasureScript:Escape('[\39]quoted[\39]')]` | RM | `[&MeasureScript:Escape(''quoted'')]` |
| `[\0][\x][\x110000][\NotANumber]` | RV(new) | unchanged (false) |
| `[\*65*]` | RV(new) | `[\65]` (false) |
| `(?:[A-Z])` ×1000 | RM | unchanged (false) |

(RV(new) = `ReplaceVariables(s, true)`; RM = `ReplaceMeasures(s)`.)

### 3.5 When substitution happens
Reading an option (`ReadStringInternal`, `ConfigParser.cpp:1274-1313`; `ProcessReadString`, `:1228-1240`):
1. The raw value comes from the section, else from inherited styles (§6).
2. `ReplaceVariables` replaces old-style `#…#` (plus `%ENV%`). In `[Variables]` it runs the
   new-style bracket pass first.
3. If `sectionVariables` (the default) is set, `ReplaceMeasures` runs the full bracket pass
   (`[#Var]`, `[&M]`, `[\x]`, `[%E]`, `[$…]`) and then old-style `[Measure]` / `[Meter:X]`.

Consequences:
- **Ordinary options are fully substituted every time they are read.** That is at load, and again
  only when the section re-reads: `DynamicVariables=1`, or once after `!SetOption` (§8). At load,
  `[Measure]` gives the measure's *initial* value (0 or ""), because options are read before the
  first update (§8.1). **This is why section variables "need DynamicVariables=1".**
- **Action options** (`*Action`, `OnRefreshAction`, `ContextAction`, …) use `ReadActionString` =
  `sectionVariables=false` (`ConfigParser.h:88`):
  - `#Var#` is frozen at read time (refresh, or each update when DynamicVariables=1).
  - `[#Var]`, `[Measure]`, `[&Measure]` and `[Meter:X]` are left intact and resolved **at execution
    time**, by the command handler (§10). No DynamicVariables is needed (`ConfigParser.cpp:990-993`).
- `Meter=` and `Measure=` are read with `sectionVariables=false` and without inheritance (`Skin.cpp:2805-2866`).

### 3.6 Section variables and selectors (`ConfigParser.cpp:263-469`)
Default string of `[Measure]` and `[&Measure]` = `GetStringOrFormattedValue(AUTOSCALE_OFF, 1.0, -1, false)`
(`:871, 1004`). That is the string value if the measure has one (String, Time, …). Otherwise it
is the number formatted `%.5f` with trailing zeros removed (`Library/Measure.cpp:561-603, 641-660`).
`Substitute` is applied to the formatted number too (`Measure.cpp:602`). Examples: 30 → `30`,
1/3 → `0.33333`, 2.5 → `2.5`.

Splitting: `section` = text before the **last `:` that precedes the first `(`**
(`StringParser::ConsumeUntilLast(':', '(')`, `Common/StringParser.cpp:319-332`). The selector is
everything after it. If there is no `:`, it is not a selector variable.

*Key selectors* (the selector starts with a letter):
- Meter: `X`, `Y`, `W`, `H`, `XW` (=X+W), `YH` (=Y+H), case-insensitive. Any other key on a meter
  is invalid (`:274-288`). Meter X/Y are absolute window coordinates after relative positioning.
- Measure: `MaxValue`, `MinValue`, `EscapeRegExp` (escapes `\^$|()[{.+*?`, `Common/StringUtil.cpp:127-135`),
  `EncodeUrl` (UTF-8 percent-encoding of everything except `0-9 A-Z a-z - . _ ~`,
  uppercase hex, `StringUtil.cpp:137-155`), `TimeStamp` (Time measures, Windows epoch seconds).
  Otherwise it is a Script/Plugin call `[&Script:Func(args)]`, and unknown selectors are invalid (`:312-362`).

*Numeric selectors* (the first selector char is not a letter):
- `[M:]` → value with 10 decimals and trailing zeros removed.
- `[M:N]` → N decimals (clamped 0..32), zeros kept.
- `[M:%]` → `100*relative` (§3.7), zeros removed. `[M:%,N]` → N decimals.
- `[M:/S]` → value/S (S an integer ≠0; `/0` is invalid). `[M:/S,N]` → N decimals.
- `[M:MaxValue:/S, N]`, `[M:MinValue:…]` → max/min scaled. `%` with Max/Min is invalid (`:365-380, 403-424`).
- A `,` present without `%` or `/` → invalid (`:425-428`).

Invalid selectors leave the text unchanged and return false.

Test vectors (`ConfigParser_Test.cpp:311-381`). Measure Min=10, Max=50, value 30:
`[Measure:%]`=`50`, `[Measure:%,1]`=`50.0`, `[Measure:]`=`30`, `[Measure:2]`=`30.00`,
`[Measure:/5]`=`6`, `[Measure:/5, 2]`=`6.00`, `[Measure:MaxValue:/5]`=`10`,
`[Measure:MaxValue:/5, 2]`=`10.00`. `[Measure:MaxValue:%]` and `[Measure:/0]` are invalid.
`[Measure:Colon:/5]` = `6` for a measure named `Measure:Colon`.

String measure value ` !*'();:@test&=+$,/?#[ing]`:
- `EncodeUrl` → `%20%21%2A%27%28%29%3B%3A%40test%26%3D%2B%24%2C%2F%3F%23%5Bing%5D`
- `EscapeRegExp` → ` !\*'\(\);:@test&=\+\$,/\?#\[ing]`
- `MaxValue`=`34`, `MinValue`=`12` (the options were set to 34/12).

Meter at (10,20) size 30×40: X=10, Y=20, W=30, H=40, XW=40, YH=60, `Invalid` → unchanged.
All of these work identically with `[Name:sel]` and `[&Name:sel]`.

### 3.7 Measure value semantics used above (`Measure.cpp:519-552`)
- `GetValue()` = `InvertMeasure ? Max - v + Min : v`.
- Relative value = `(clamp(GetValue(), Min, Max) - Min) / (Max - Min)`, or **1.0** if Max==Min.
- Defaults: MinValue 0, MaxValue 1 (`Measure.cpp:100-101`).

### 3.8 Built-in variables (exact values)
Trailing backslash conventions matter. Skins concatenate `#@#Images\x.png` and `#CURRENTPATH#file`.

| Variable | Value | Source |
|---|---|---|
| `#@#` | `<SkinsPath><RootConfig>\@Resources\` (trailing `\`) | `ConfigParser.cpp:219`, `Skin.cpp:2524, 6079-6095` |
| `#PROGRAMPATH#` | folder of Rainmeter.exe/dll, trailing `\` | `:223`, `Rainmeter.cpp:217-222` |
| `#PROGRAMDRIVE#` | volume of PROGRAMPATH, e.g. `C:` (no `\`) or `\\server\share` | `:243`, `PathUtil.cpp:86-109` |
| `#SETTINGSPATH#` | folder of Rainmeter.ini, trailing `\` | `:224`, `Rainmeter.cpp:264` |
| `#SKINSPATH#` | Skins folder, trailing `\` | `:225`, `Rainmeter.cpp:460-479` |
| `#PLUGINSPATH#` | `PROGRAMPATH + "Plugins\"` (stock Rainmeter used SETTINGSPATH; this tree uses the program folder) | `:226`, `Library/Rainmeter.h:104` |
| `#ADDONSPATH#` | `SETTINGSPATH + "Addons\"` | `:228`, `Rainmeter.h:106` |
| `#CURRENTPATH#` | folder of the skin's main .ini (not of an include), trailing `\` | `:227, 139` |
| `#ROOTCONFIGPATH#` | `SKINSPATH + RootConfig + "\"` | `:229`, `Skin.cpp:6079-6095` |
| `#CURRENTFILE#` | file name, e.g. `Clock.ini` | `:236` |
| `#CURRENTCONFIG#` | config path, e.g. `illustro\Clock` (no trailing `\`) | `:237` |
| `#ROOTCONFIG#` | first component, e.g. `illustro` | `:244`, `Skin.cpp:6068-6077` |
| `#CURRENTSECTION#` | name of the section being read or running the action, else `""` | `:235` |
| `#CONFIGEDITOR#` | editor path (Rainmeter.ini `ConfigEditor`, else the .ini association, else `Notepad`) | `:241`, `Rainmeter.cpp:1729-1737` |
| `#CRLF#` | **`"\n"` (LF only)** | `:242` |
| `#CURRENTCONFIGX#`/`Y#` | window position, logical/virtual-screen coords | `:477-478` |
| `#CURRENTCONFIGWIDTH#`/`HEIGHT#` | window W/H, else SkinWidth/SkinHeight | `:479-480`, `Skin.h:219-220` |
| `#CURRENTCONFIGZPOS#` (fork) | z-position int | `:481` |
| `#SCREENAREAX/Y/WIDTH/HEIGHT#` | screen rect of the skin's monitor (from the `@N` in WindowX/WindowY, else primary) | `:569-745` |
| `#WORKAREAX/Y/WIDTH/HEIGHT#` | work-area rect, same monitor rule | same |
| `#SCREENAREAX@N#`, `#WORKAREAWIDTH@N#`, … | monitor N (1-based). N≤0 or a missing monitor is unresolved; an inactive monitor gives the primary | `:648-656, 711-726` |
| `#PSCREENAREA*#`, `#PWORKAREA*#` | primary monitor; `@N` not allowed | `:593-609, 650` |
| `#VSCREENAREAX/Y/WIDTH/HEIGHT#` | bounding box of all monitors; `@N` not allowed | `:610-612, 673-676` |

Monitor index 0 (from `@0` in WindowX) means the virtual screen (`:705-708`).
Matching is case-insensitive. Each name must match *exactly* (`ConsumeRest`).

Fork-only `[$…]` variables (`:487-567`): `$SkinX/Y/W/H`, `$SkinPhysicalX/Y/W/H`,
`$SkinZoomedW/H`, `$SkinZPos`, `$SkinDpiFactor`, `$SkinZoomFactor`, `$SkinVisible`,
`$Display[N|DeviceN][Name|Number|DpiFactor|[WorkArea][Physical]X/Y/W/H]`,
`$DisplayCount`, `$DisplayVirtualScreen[Physical]X/Y/W/H`. Bare names starting with `$`
also work inside formulas (`Skin.cpp:6110-6130`).

### 3.9 Mouse variables (mouse actions only)
Code: `Library/Mouse.cpp:273-325`, `ConfigParser.cpp:1157-1205`.

When a mouse action fires, before the command is run:
- `[$MouseX]`, `[$MouseY]` = cursor position relative to the **meter's** X/Y if the action belongs
  to a meter, else relative to the skin.
- `[$MouseX:%]`, `[$MouseY:%]` = `int((pos - offset + 1) / W * 100)`, using the meter's W/H, or the
  skin's W/H for skin-level actions.
- Old-style `$MouseX$`, `$MouseY$`, `$MouseX:%$`, `$MouseY:%$` behave the same. The escape is `$*…*$`.

All are case-insensitive.

---

## 4. Formulas / MathParser

Code: `Common/MathParser.cpp` (a ccalc port, using an operator-precedence stack machine).

### 4.1 Where formulas are evaluated
- `ReadInt` / `ReadUInt` / `ReadUInt64` / `ReadFloat` / `ReadBool`: if the **first char** of the
  (substituted) value is `(`, the **whole value** is evaluated with `CheckedParse` (bracket balance
  first, then parse) (`ConfigParser.cpp:1386-1516`). So `(10)+5` gives 15, but `10+(5)` is read by
  `wcstol` and gives 10. On a formula error, log `Formula: <msg> in key "K" in [Section]` and return
  the **default**.
- Lists (colors, rects, `X`/`Y` after the `r`/`R` suffix, TransformationMatrix) use
  `ParseUtil::ParseInt/ParseDouble(string_view)`. These skip leading whitespace and evaluate the
  remainder if it starts with `(` (`Common/ParseUtil.cpp:18-39`).
- `!SetVariable`, `!WriteKeyValue` and the WindowX/WindowY/AnchorX/AnchorY options only evaluate a
  value that **starts with `(` and ends with `)`** (`ConfigParser.cpp:1519-1535`).
  - SetVariable and WriteKeyValue format the result as `%.5f` with trailing zeros removed (`Skin.cpp:2055-2070`, `CommandHandler.cpp:1018-1030`).
  - WindowX/WindowY use `ParseFormulaWithModifiers`: evaluate up to the **last `)`** and append the
    rest. `(#SCREENAREAWIDTH#/2)R@2` → `960R@2`, formatted `%lf` with zeros trimmed (`:1539-1559`).
- Calc measure `Formula=`: the whole string, no parentheses required (`Library/MeasureCalc.cpp:32-47`).
  On error it logs once and **keeps the previous value**. Unbalanced brackets at read time clear the
  formula, so the result is 0 (`:100-105`).
- Bang numeric arguments: `Skin::DoBang` uses `ParseInt` (a leading `(` means formula).

### 4.2 Lexing (`MathParser.cpp:667-835`)
- Separators: space, TAB, LF. **CR is a syntax error.**
- Symbols: `+ - * / % ^ ~ ( ) < > , ? : = & |`.
- Numbers start with a digit:
  - `0x…` hex via `wcstoll(,16)`. **Lowercase `x` only.**
  - `0b…` binary and `0o…` octal (lowercase).
  - Otherwise `wcstod`, which accepts exponents (`1e3`, `2.5E-2`). Leading zeros are decimal: `011` = 11.
  - `.5` is **not** a number. `.` is a name char, so it gives "".5" is unknown".
  - Uppercase `0X1F` falls to `wcstod`, which accepts C99 hex, so it gives 31. `0B11` and `0O12` do not work.
- Names start with any graphic char that is not a digit or symbol, and continue over letters,
  digits and "unknown" chars (`:694-705, 822-829`). `_ . # @ [ ] $ ! ' "` are all name chars.
  Spaces end a name, so measures with spaces in their names can't be referenced bare.
- **Unary minus** (`:674-684, 402-410`):
  - At the start, after `(`, or after `)`: it is an operator. In prefix position it becomes the
    function `neg` (priority 6).
  - After any other operator, `-` must start a numeric literal (`2*-3` OK). `2*-x`, `2*-(3)` and
    `1 - -pi` give **"Extra operation"**.
  - `--1` = 1. `-(-5+-5)` = 10.
- Unary `+` is only skipped right after the start, `(`, `,`, `?` or `:`. `++1` is an error.

### 4.3 Operators and precedence (`MathParser.cpp:161-190, 418-422`)
Higher numbers bind tighter. **All binary operators are left-associative**: an operator is pushed
after popping every stacked op with priority ≥ its own.

| prio | operators |
|---|---|
| 6 | functions, unary `-` (neg) |
| 5 | `**` (power), `<<`, `>>`, `&` (bit and), `\|` (bit or), `^` (**bitwise XOR, not power**), `~` (bit not, unary) |
| 4 | `*`, `/`, `%` (`fmod`) |
| 3 | `+`, `-` |
| 2 | `=` (equality), `<>`, `<`, `>`, `<=`, `>=`, `&&`, `\|\|`, `:` |
| 1 | `?` |

Consequences:
- Bitwise operators bind tighter than `*`: `2*3&1` = `2*(3&1)` = 2.
- `**` is left-associative and tighter than unary minus's operand: `2**3**2` = 64, `-2**2` = 4.
- `&&` and `||` share priority with comparisons: `1 < 2 && 2 > 1` = `((1<2)&&2)>1` = **0**.
- Ternary: `?` is the lowest. `a ? b : c` needs no parentheses around a comparison. The else-chain
  is **left-associative**: `1 ? 2 : 0 ? 4 : 5` = `(1?2:0)?4:5` = **4**. A ternary nested in the then
  branch without parentheses is an error. `0 ? 1 : 2 > 1` = `(0?1:2)>1` = 1.
- No short-circuiting. **Both** ternary branches and both `&&`/`||` operands are evaluated, so
  `0 ? 1/0 : 2` is an error.
- `=` is equality; `==` is an error. There is no `!` or `!=` (use `<>`).
- Bit operators cast to `i64` by truncation: `5.9 & 3` = 1. `~x` = `!(x as i64)`. Shifts are i64.
- `/` by 0 → error **"Division by 0"**. `%` by 0 → NaN (no error).
- `fmod` keeps the sign of the dividend: `-7 % 3` = -1.

### 4.4 Functions and constants (`MathParser.cpp:94-153, 923-1037`)
Function names are case-insensitive and are **resolved before measure names**: a measure named `pi`
cannot be referenced (`MathParser_Test.cpp:75-76`).

- Single argument: `atan cos sin tan abs exp ln`(natural) `log`(**base 10**) `sqrt frac`(modf
  fraction, sign kept) `trunc floor ceil asin acos rad deg sgn neg`. They take the next operand,
  and parentheses are optional: `sin 50`.
- Multiple arguments: `atan2(y,x)`, `min(a,b)`, `max(a,b)` (exactly 2), `clamp(x,lo,hi)` (exactly 3),
  `round(x)` / `round(x,n)`. Round is half away from zero. A negative n rounds to tens: `round(155,-1)`=160.
  A wrong argument count gives "Invalid function parameter count". A comma outside a multi-argument
  function is a syntax error.
- Constants: `e`, `pi`.
- No `pow`, `random` or `counter`. The last two exist only in Calc measures (§4.5).

### 4.5 Name resolution
- Skin formulas (option values) (`Skin.cpp:6110-6138`):
  1. `$name` → fork dollar variable, which must parse fully as a number.
  2. Else a measure named `name` (case-insensitive) → `measure.GetValue()`, its *current* numeric
     value (inverted if `InvertMeasure`).
  3. Else error `"name" is unknown`.
- Calc measures (`MeasureCalc.cpp:140-179`):
  1. A measure name.
  2. Else `Counter` → the skin's update counter.
  3. Else `Random` → random in [LowBound, HighBound], default 0..100.
  4. Else `$var`.

  **Quirk:** counter and random use `_wcsnicmp(name, "counter", len)`, a *prefix* match, so `c`,
  `cou` or `r` (when not measures) also match.

  With `UpdateRandom=0` (default), whole-word `Random` tokens are replaced by a number once, when
  the options are read (`:109-138`).
- Measures update in file order. A Calc that references a later measure sees that measure's
  previous-tick value.

### 4.6 Errors (`MathParser.cpp:237-244`)
- `Unmatched brackets`
- `Syntax error`
- `Internal error` (e.g. two operands with no operator, `2pi`; or the stack is exhausted: 96 ops / 64 values)
- `Extra operation`
- `Division by 0`
- `"<name>" is unknown`
- `Logical expression error`
- `Invalid function parameter count`

An empty formula = 0 (`:294-298`). Formula results go to int by truncation toward zero.

### 4.7 MathParser_Test vectors (`Common/MathParser_Test.cpp:19-136`)
```
5→5; "      5      "→5; ((((5))))→5; (5 + 5)→10; (-5 - 5)→-10; -(-5-5)→10; -(-5+-5)→10
1 && 1→1; 1 && 0→0; 1 || 1→1; 1 || -1→1; 1 < 2 && 2 > 1→0; (1 < 2) && (2 > 1)→1
sin(50)→sin(50); -sin((25 + 25) * 2)→-sin(100)
01→1; 011→11; 0b11→3; 0xA→10; 0o12→10
1 ? 2 : 3→2; 0 ? 2 : 3→3; 1 ? (2 + 3) : 3→5; 1 ? 2 : 0 ? 4 : 5→4; 1 ? 2 : 1 ? 4 : 5→4
1 ? 2 : (1 ? 4 : 5)→2; 0?1:(0?1:(…30 levels…:5))→5
trunc(1.5)→1; trunc(-1.5)→-1; round(1.555)→2; round(1.555, 2)→1.56
min(1, -2)→-2; max(3, -2)→3; clamp(6, -2, 3)→3
errors: "1 ? 0 ? 4 : 5 : 3", "++1", "((1)", "1 / 0", "1 &&", "a" (no resolver)
e→2.718…; pi→3.14159…; pi with a resolver mapping pi→40 → still 3.14159
resolver {a:10, bbb:20, ccc_:30}: "a + 5"→15; "bbb"→20; "(ccc_)"→30
MatchingClosingBracket mode: "(1 + min(2, 3))tail" → 3, rest "tail"
string_view bounds: view "123" of "123456" → 123; view "3.1" of "3.123456" → 3.1
```
Additional vectors derived from the code:
`2^3`→1; `2**3**2`→64; `-2**2`→4; `2*3&1`→2; `-7%3`→-1; `0?1/0:2`→error; `1==1`→error;
`.5`→error; `2*-x`→error; `round(155,-1)`→160; `log(100)`→2; `ln(e)`→1; `(5)-2`→3; `~0`→-1; `1<<4`→16.

---

## 5. Number, color and rect parsing

### 5.1 Scalars (`ConfigParser.cpp:1386-1516`)
- Missing key, or a value that is empty after substitution → **default**.
- Value starting with `(` → formula (§4.1). A formula error → default.
- Otherwise `wcstol(s, 10)` / `wcstoul` / `_wcstoui64` / `wcstod` on the start of the string.
  Trailing garbage is ignored. Overflow (ERANGE) → default. **A non-numeric string gives 0, not the
  default.**
  - `ReadInt` targets 32-bit `long` (Windows), so `2147483648` overflows → default.
  - `wcstoul("-1")` = 4294967295.
- `ReadBool` = `ReadInt(...) != 0` (`ConfigParser.h:238`). So `true` → **false (0)**. `2` and `-1` → true.
- Numbers use the C locale (`.` decimal separator; `Skin.cpp:307`).

| value | ReadInt(def 5) | ReadFloat(def 5) | ReadBool(def true) |
|---|---|---|---|
| *(missing)* | 5 | 5 | true |
| `` (empty) | 5 | 5 | true |
| `12px` | 12 | 12 | true |
| `12.7` | 12 | 12.7 | true |
| `abc` | 0 | 0 | false |
| `true` | 0 | 0 | false |
| `0x10` | 0 | 16 (UCRT `wcstod` accepts C99 hex) | false |
| `(1 + 2)` | 3 | 3 | true |
| `(2.9)` / `(-2.9)` | 2 / -2 | 2.9 / -2.9 | true |
| `(1 +` | 5 (error) | 5 | true |
| `(10)+5` | 15 | 15 | true |
| `1e3` | 1 | 1000 | true |
| `2147483648` | 5 | 2147483648 | true |

(`ConfigParser_Test.cpp:102-125`: `2`→2, `1.23`→1.23, `18446744073709551615` as UInt64,
`(1 + 2)` → 3 for Int, UInt and Float.)

### 5.2 Colors (`Common/ParseUtil.cpp:202-230`)
Default for each channel is 255. The result is `R/255, G/255, B/255, A/255` and is **not clamped**.
- If the value contains `,`: a component list (`ParseInt4`, `:41-62`). Up to 4 components split on
  `,` (commas inside parentheses are ignored). Each component goes through `ParseInt`: leading
  whitespace skipped, a formula if it starts with `(`, else `wcstol`, so `10.7` → 10. An empty or
  non-numeric component → **0**. A trailing whitespace-only component is not a component. Extra
  components are ignored.
- Else hex: skip an optional `0x` prefix (**case-sensitive**). Alpha is read only if the remaining
  length is ≥ 8 and char[6] is not whitespace. If the length is ≥ 6 (or alpha applies), read 3 or 4
  hex bytes (≤2 digits each, leading whitespace allowed), stopping at the first non-hex. Shorter
  strings → all 255.
- `ReadColor` returns the default when the value is missing or empty (`ConfigParser.cpp:1561-1566`).

Test vectors (`Common/ParseUtil_Test.cpp:12-71`, `ConfigParser_Test.cpp:127-138`):
```
FF0000→255,0,0,255   FF000080→255,0,0,128   0xFF0000→255,0,0,255   11223344→17,34,51,68
0XFF0000→0,255,255,255   "FF0000 80"→255,0,0,255   FF00→255,255,255,255
FF00ZZ→255,0,255,255   ZZ0000→255,255,255,255
10,20,30→10,20,30,255   10,20,30,40→…,40   " 10 , 20 , 30 "→10,20,30,255
"(1 + min(2, 3)),20,30"→3,20,30,255
10,,30→10,0,30,255   "10,20, "→10,20,255,255   "10,20,\r\n"→10,20,255,255   10,invalid,30→10,0,30,255
10,20,30,40,50→10,20,30,40   AABBCCDD = 170,187,204,221 (same as AABBCC + alpha DD)
```

### 5.3 Rects
- `ReadRECT` (`ParseRECT`, `ParseUtil.cpp:241-246`): 4 ints taken as-is (left, top, right, bottom).
  Used for **margins** (`BackgroundMargins`, `DragMargins`, `ScaleMargins`). Missing → default.
  **A present value without a comma** gives `{0,0,0,0}`, not the default (`ParseInt4` needs a comma).
- `ReadRect` (`ParseRect`, `ParseUtil.cpp:232-239`): `x,y,w,h` → `{x, y, x+w, y+h}`, components
  truncated to int. Used for `Padding` and `ImageCrop`-style options. `10,20,30,40` →
  `l10 t20 r40 b60` (`ParseUtil_Test.cpp:93-102`). `1,2,11,22` as RECT stays `1,2,11,22`
  (`ConfigParser_Test.cpp:140-147`).
- Components missing from a list keep 0 (rect) or 255 (color).

---

## 6. MeterStyle and @Inherit

Code: `ConfigParser.cpp:77-97, 1212-1226`; `Library/Section.cpp:22-26`.

- Meters are read with `allowMeterStyle=true` and measures with `false`.
- The reader first reads `@Inherit` **(fork)** from the section's own values only. If that is empty
  and the section is a meter, it reads `MeterStyle`. **`@Inherit` and `MeterStyle` do not combine**;
  a non-empty `@Inherit` wins.
- The value undergoes normal substitution, so `MeterStyle=#Style#` and `MeterStyle=[&M]` both work.
  It is split on `|` with whitespace trimmed and empty tokens dropped. Each name that exists as a
  section name becomes part of the chain. Unknown names are silently skipped.
- Lookup for an option:
  1. The section's own value. **An empty `Key=` counts as defined and blocks styles.**
  2. Otherwise the styles in **reverse list order**: `MeterStyle=A | B` → B checked first. **The last
     listed style wins.**
- **Styles are one level only.** A style's own `MeterStyle` is not followed. Any section can serve
  as a style, including another meter or a measure.
- Values from a style are substituted **in the context of the reading meter**, so
  `#CURRENTSECTION#` in a style is the meter's name.
- **Dynamic:** the chain is rebuilt on every `ReadOptions`. `!SetOption Meter MeterStyle X`
  followed by an update or `!UpdateMeter` switches styles. Measures support only `@Inherit`
  (`Section.cpp:22-26`, `Skin.cpp:2916`).
- **Options that cannot come from a style:** `Meter=` and `Measure=` (they are read without
  inheritance, `Skin.cpp:2805-2866`), `MeterStyle` and `@Inherit` themselves, and anything read
  through `GetOptionReader` (all `[Rainmeter]` options).
- `!SetOption StyleSection Key Value` targets a section that is not a measure or meter. It is a
  **no-op** (`Skin.cpp:2109-2133`; only `[Rainmeter]` `Context*` keys are special-cased).

Test vectors (`ConfigParser_Test.cpp:150-174` covers `@Inherit`):
- `[S1] FontColor=1,1,1` / `[S2] FontColor=2,2,2` / `[M] MeterStyle=S1|S2` → M.FontColor = `2,2,2`.
- Add `FontColor=9,9,9` to M → `9,9,9`. Add `FontColor=` (empty) to M → the default color, not S2's.
- `[S1] MeterStyle=S0` with `[S0] X=5`, and `[M] MeterStyle=S1` → M.X is not 5 (no chaining).
- `[Child] @Inherit=Parent`, `[Parent] Value=1` → `GetOptionReader` (no inheritance) gives 0, while
  the inheritable reader gives 1.

---

## 7. `[Rainmeter]`, `[Metadata]`, `[Variables]` sections

### 7.1 Skin load sequence (`Skin::ReadSkin`, `Library/Skin.cpp:2507-2943`)
1. The .ini must exist, else an "Unable to refresh" message and the skin is deactivated (`:2516-2522`, `:498-502`).
2. `#@#` = `RootPath + "@Resources\"` (`:2524`). Then the parser reads the file, its includes and
   its variables (`:2527`).
3. `SyntaxVersion` (default 1). A value >2 means "newer version required" and the load fails (`:2532-2539`).
   SyntaxVersion ≥2 disables `%ENV%` expansion and forces AccurateText (`ConfigParser.cpp:749, 766`; `Skin.cpp:2556`).
4. **First run only** (the config has no section in Rainmeter.ini): read `Default*` options (§11.2)
   from the skin's `[Rainmeter]`, and write the ones that are defined into Rainmeter.ini (`:2541-2546, 2201-2315`).
5. Read the per-config settings from Rainmeter.ini (§11) (`:2548-2554`).
6. Read the `[Rainmeter]` options in the table below, then load fonts.
7. Create the sections, in the order of §2.3, skipping `Rainmeter`, `Variables` and `Metadata`
   (case-insensitive) (`:2797-2890`):
   - If `Measure=` is non-empty, it is a measure. `Measure=Plugin` + `Plugin=…\Foo.dll` is mapped
     to a built-in if Foo is an old default plugin (`:2817-2847`).
   - Else, if `Meter=` is non-empty, it is a meter.
   - Else the section is ignored (a style or data section).
   - Unknown types log `Measure=X is not valid in [S]` and are skipped (`Measure.cpp:691-871`, `Meter.cpp:496-550`).
     Meter types: STRING, IMAGE, BAR, BITMAP, HISTOGRAM, LINE, ROUNDLINE, ROTATOR, BUTTON, SHAPE,
     plus TEXTEDIT and SVG (fork).
8. **No meters → error message, and the skin is not loaded** (`:2892-2897`).
9. Meter `Container=` options are read, then relative-position chains are built (`:2902-2908`).
10. All measures `ReadOptions` (file order) → all meters `ReadOptions` + `Initialize` → all measures
    `Initialize` (`:2913-2932`).
11. Every meter gets a forced `Update`, and the window is sized (`:2935-2940`). This happens before
    any measure has updated, so meters see initial values.

### 7.2 `[Rainmeter]` options read by the skin
All are read with `GetOptionReader` (no inheritance), at refresh only. The exception is
`ContextTitle*`/`ContextAction*`, which are read when the menu opens.

| Option | Type / default | Notes / source |
|---|---|---|
| `SyntaxVersion` | int, 1 | §7.1 |
| `AppVersion` | uint, 0 | encoded `major*1e6+minor*1e3+sub`; refuses to load if newer (`:2562-2577`) |
| `AccurateText` | bool, 0 | forced on for SyntaxVersion≥2 (`:2556`) |
| `SkinWidth`, `SkinHeight` | int, 0 | >0 fixes the window size (`:2579-2583`, `:2142-2143`) |
| `Group` | string | appended after the Rainmeter.ini `Group` with `\|` (`:2586-2592`) |
| `DragGroup` | string | (`:2594-2595`) |
| `BackgroundMargins` | RECT l,t,r,b, 0 | scaled/tiled background margins and window-size padding (`:2598`, `:2947-2962`) |
| `DragMargins` | RECT, 0 | §9.5 (`:2599`) |
| `BackgroundMode` | int, 0 | 0 image, 1 none/transparent ("copy"), 2 solid, 3 scaled image, 4 tiled image (`Skin.h` BGMODE, `:2601`) |
| `Background` | path | read only for modes 0/3/4. Made absolute (§12). **Empty → mode becomes 1** (`:2612-2623`) |
| Background image options | | `ImageCrop, Greyscale, ImageTint, ImageAlpha, ColorMatrix1-5, ImageFlip, ImageRotate, UseExifOrientation, ImagePath`, read from `[Rainmeter]` (`:2980-2985`, `Library/GeneralImage.h:13-28`) |
| `BevelType` | int, 0 | 0 none, 1 up, 2 down (`:2602`) |
| `BevelColor`, `BevelColor2` | color, white / black | (`:2603-2604`) |
| `SolidColor` | color, **128,128,128,255** (Gray) | used only for BackgroundMode=2 (`:2606`, `:3163-3193`) |
| `SolidColor2` | color, = SolidColor | gradient end (`:2607`) |
| `GradientAngle` | float, 0 | (`:2608`) |
| `DynamicWindowSize` | bool, 0 | re-check the window size after every update that redraws (`:2610`, `:3490-3494`) |
| `SelectedColor` | color, default from Rainmeter.ini `[Rainmeter] SelectedColor` (LightBlue 173,216,230) | selection overlay (`:2625-2626`, `Rainmeter.cpp:1727`) |
| `LeftMouseUpAction` … `MouseLeaveAction` | action | skin-level mouse actions, §9 (`:2628`) |
| `MouseActionCursor` | bool, 1 | (`Mouse.cpp:185-186`) |
| `MouseActionCursorName` | enum or `*.cur`, HAND | ARROW HAND TEXT HELP BUSY CROSS PEN NO SIZE_ALL SIZE_NESW SIZE_NS SIZE_NWSE SIZE_WE UPARROW WAIT. A name containing `.` is a file in `#@#Cursors\` (`Mouse.cpp:14-31, 188-234`) |
| `OnRefreshAction` | action | runs at the end of every (re)load, after the first update (`:552-555`) |
| `OnUpdateAction` | action | after every skin update cycle (`:3507-3510`) |
| `OnCloseAction` | action | when the skin is unloaded or the app quits, **not on refresh** (`:188-195`) |
| `OnFocusAction`, `OnUnfocusAction` | action | window gains/loses keyboard focus (`:5263-5287`) |
| `OnWakeAction` | action | resume from sleep, run delayed (`:5791-5796`) |
| `OnDisplayMetricsChange`, `OnVisibilityChange` | action | (fork) (`:2636-2637`) |
| `Update` | int/formula, **1000** ms | `-1` (any negative) = no timer: the skin updates only at load and on `!Update`. Fork syntax: `N \| Invisible M`. Invalid syntax logs `Invalid Update` (`:2482-2505`) |
| `TransitionUpdate` | int, 100 ms | redraw interval while a meter transition is active (`:2641`, `:3359-3372`) |
| `DefaultUpdateDivider` | int, 1 | default `UpdateDivider` for all measures and meters (`:2642`, `Section.cpp:32-38`) |
| `ToolTipHidden` | bool, 0 | default for meters' `ToolTipHidden` (`:2643`, `Meter.cpp:407`) |
| `UpdateMode` | | deprecated, warning only (`:2645-2648`) |
| `Blur`, `BlurRegion`, `BlurRegion2`… | bool / region | Windows Aero blur; Snowmeter can treat as a no-op (`:2650-2678`) |
| `LocalFont`, `LocalFont2`, … | path | tried first as `PROGRAMPATH\Fonts\<v>`, then as a skin-relative path (§12). The numbering stops at the first empty one (`:2721-2756`) |
| (implicit) | | every file in `#@#Fonts\` is loaded as a private font (`:2680-2719`) |
| `ContextTitle`, `ContextAction`, `ContextTitle2`/`ContextAction2` … | string / action | see below |

Context menu items (`Library/ContextMenu.cpp:716-790`, `Skin.cpp:4283-4303`):
- Items are read while `ContextTitleN` is non-empty and (`ContextActionN` is non-empty or the title
  consists only of `-`, which makes a separator). Reading stops at the first gap.
- At most 25 items (`Library/resource.h:453-454`). Titles are truncated to 30 chars plus `…`.
- With ≤3 items they are inlined in the skin menu under a grayed "Custom skin actions" header, and
  separators are dropped. With more than 3 they go in a submenu.
- The action is read (`ReadActionString`) when clicked and run with the skin as context (no section).
- `!SetOption Rainmeter ContextTitleN …` / `ContextActionN` modify these dynamically (`Skin.cpp:2118-2130`).
- `!SkinCustomMenu` opens just this menu.

Window size (`Skin.cpp:2945-3030`):
`W = max(BackgroundMargins.left, max over non-contained meters of X+W) + BackgroundMargins.right`,
and likewise for H. Hidden meters have W=H=0 (`Meter.h:39-40`). Then:
- `max` with the background image size.
- `BackgroundMode=0` with a loaded image: the size is **exactly** the image size.
- `SkinWidth`/`SkinHeight` > 0 override the result.

### 7.3 `[Metadata]`
Not stored by the parser (`ConfigParser.cpp:1810`). It is read directly by the Manage UI with
`IniFile::ReadSection` (`Library/DialogManage.cpp:914-965`):
- `Name`, `Author` (fallback: `[Rainmeter] Author`), `Version`, `License`
- `Information` (fallback: `Description` + blank line + `Instructions`). `|` becomes a line
  break, swallowing one adjacent space on each side.

### 7.4 `[Variables]`
See §3.1. It is never a measure or meter, and its keys are variables, not options.
`!SetOption Variables …` does nothing (it is not a section), so use `!SetVariable`.

---

## 8. Update loop

### 8.1 Load / refresh (`Skin::Refresh`, `Skin.cpp:472-556`)
1. The state becomes refreshing. Deferred Rainmeter.ini writes are flushed. Unless this is the
   first load, all measures and meters are destroyed (`Dispose(true)`; OnCloseAction is **not** run).
2. `ReadSkin()` (§7.1).
3. The window position is computed (§11.3), clamped if KeepOnScreen, and the window is placed.
4. `Update({refresh=true})`: the **first update**. Every measure and meter is *forced*: its counter
   is reset, so it updates regardless of UpdateDivider, including `-1` (`:520`).
5. Blur, z-order.
6. If `Update >= 0`, start the periodic timer (`:542-545`).
7. The state becomes running, then **`OnRefreshAction` runs** (`:552-555`).

The skin window fades in from 0 to `AlphaValue` over `FadeDuration` on first show (`:331-342`).
`!Refresh` is posted: it runs from the message loop, never inside an update (`:1121-1124, 5971-5975`).

### 8.2 One tick (`Skin::Update`, `Skin.cpp:3426-3511`)
The timer fires every `Update` ms. It is a Windows `SetTimer`: periodic, not drift-corrected, with
a minimum of about 10 ms.
1. (fork) Skip the tick if the window is occluded and `Invisible` is set.
2. `++UpdateCounter` (the skin-wide counter used by Calc's `Counter`).
3. **Measures, in file order** (§2.3). For each, `UpdateMeasure(m, force=refresh)`
   (`:3374-3391`, `Measure.cpp:440-517`):
   - If the divider ≥ 0 or forced:
     - `reread = DynamicVariables && (counter+1) >= divider`. When true, the measure's options are
       re-read first, with `@Inherit` but not MeterStyle.
     - If `Paused`: stop. The value is kept and no actions run.
     - If `Disabled`: value = 0, the IfAction state is updated, and it returns false (no actions).
     - Else, if the divider counter fires (§8.3): `UpdateValue()` → AverageSize smoothing → LogMax →
       re-read the If* conditions if `reread` → **IfActions** → return true.
   - When it returned true: **`OnUpdateAction`**, then **`OnChangeAction`** (if the value or string
     changed since the last check; the first check only records) (`:3461-3465`, `Measure.cpp:664-687`).
4. **Meters, in file order** (`:3474-3483`, `:3393-3424`):
   - If the divider ≥ 0 or forced: re-read the full options (with MeterStyle) when
     `DynamicVariables && (counter+1) >= divider`. Then `meter.Update()`, which returns true when the
     divider fires (`Meter.cpp:554-558`).
   - If it updated: run the meter's `OnUpdateAction`.
   - Tooltip refresh happens whether or not it updated.
5. Recompute the relative-position chain if containers changed (`:3485`).
6. **Redraw** if any meter updated, a resize is pending, or this is the refresh. DynamicWindowSize
   first requests a size check (`:3488-3502`).
7. Start or stop the transition timer (`TransitionUpdate`) (`:3505`).
8. Run the skin's `[Rainmeter] OnUpdateAction` (`:3507-3510`).

### 8.3 UpdateDivider (`Library/Section.cpp:30-60`)
- Initial state is counter=1, divider=1 (`Section.cpp:12-13`). When `ReadOptions` reads a divider
  different from the current one, it sets **counter = divider = new value** (`:34-38`). The default
  is `[Rainmeter] DefaultUpdateDivider`.
- `UpdateCounter()`: `++counter; if counter < divider return false; counter = 0; return true`.
- With divider N ≥ 1, the section updates on the first eligible tick and then every N ticks:
  ticks 1, N+1, 2N+1, …
- Divider 0 behaves like 1.
- **Divider ≤ -1: never updated by the timer.** It updates only when forced: the load/refresh
  update, `!UpdateMeasure`, or `!UpdateMeter`. A forced update resets the counter to the divider,
  and `++` then makes the comparison fire (`Skin.cpp:3378-3384`).
- A forced update resets the cycle, so the next timer update comes N ticks later.
- `DynamicVariables` re-reads happen only on ticks where the section actually updates.

### 8.4 Disabled / Paused / Hidden
- `Disabled=1`: the value is forced to 0 every tick and no actions run. `!DisableMeasure` /
  `!EnableMeasure` also write `Disabled=1/0` into the in-memory options, so a DynamicVariables
  re-read keeps the bang's effect (`Measure.cpp:200-214`).
- `Paused=1`: nothing happens and the value is frozen. The `Pause`/`Unpause` bangs write the option
  in the same way (`:216-230`).
- Meters have `Hidden=1`. Hidden meters still update, but are not drawn or hit-tested and have
  W=H=0. Show/Hide bangs write `Hidden` (`Meter.cpp:246-274, 778-780`).

### 8.5 Bang effects on the loop
- `!Update`: kill the timer, run **one normal tick immediately** (dividers are respected, so
  sections are *not* forced; `force` only bypasses the occlusion skip), then restart the timer
  (`Skin.cpp:1130-1137`; the force flag is not passed to sections, `:3461`).
- `!UpdateMeasure M|*|Group=G`: forced update of the measure(s) plus OnUpdateAction and
  OnChangeAction. **No meter update or redraw** (`:2003-2037`).
- `!UpdateMeter M|*`: forced update of the meter(s) plus OnUpdateAction and a size check. **No
  redraw** (`:1721-1765`).
- `!Redraw`: redraw immediately, applying any pending resize. Skipped while occluded (`:3032-3060`).
- `!SetOption S K V`:
  - Stores `V` as the raw option. An empty `V` deletes the option, so it falls back to the style or
    default.
  - Sets the section's DynamicVariables=true **until its next ReadOptions**, which resets it to the
    option value. The new value therefore takes effect at that section's next real update (`:2072-2134`).
  - Typical usage is `!SetOption … [!UpdateMeter …][!Redraw]`.
- `!SetVariable`: changes the variable table only. Options see the change when they are re-read
  (DynamicVariables, or after a SetOption). Actions see `[#Var]` immediately at execution.
- `!SetUpdate N` (fork): change the interval; ≤ -1 stops the timer (`:1139-1146`).
- Transition timer: while any meter reports an active transition (Button/Bitmap animations), redraw
  every `TransitionUpdate` ms (`:3571-3596`).

---

## 9. Mouse actions

### 9.1 Options (`Library/Mouse.cpp:109-132`)
`LeftMouseUpAction`, `LeftMouseDownAction`, `LeftMouseDoubleClickAction`, `MiddleMouse…` (same 3),
`RightMouse…` (same 3), `X1Mouse…` (same 3), `X2Mouse…` (same 3), `MouseScrollUpAction`,
`MouseScrollDownAction`, `MouseScrollLeftAction`, `MouseScrollRightAction`, `MouseOverAction`,
`MouseLeaveAction`.

They are valid in `[Rainmeter]` (skin level) and in any meter (styleable). They are read as
action strings (§3.5).
- An action whose key is absent never exists.
- Once read, an action keeps its *state* (enabled/disabled/cleared) across DynamicVariables
  re-reads (`Mouse.cpp:156-178`).
- `!DisableMouseAction`, `!EnableMouseAction`, `!ToggleMouseAction` and `!ClearMouseAction` take a
  `|`-separated list of option names, or `*` (`:327-368, 430-460`).
  - Disabled: kept for cursor purposes.
  - Cleared: treated as absent.

### 9.2 Dispatch (`Skin::DoAction`, `Skin.cpp:5327-5376`)
For a button or scroll event at a skin-local logical point:
1. Walk meters **from last to first** (topmost first), skipping hidden ones. The first meter that
   has an **enabled action of this exact type** and `HitTest`s the point runs it, with that meter
   as the current section.
   - Meters without that action type do **not** block meters underneath.
   - A meter whose action is disabled is skipped.
2. If no meter took it and the skin-level action exists and the point is inside the window, run
   the skin-level action (no current section).
3. Before running: mouse variables are expanded (§3.9) and `[\..]` character references are
   replaced (`Mouse.cpp:370-381`).

### 9.3 Event mapping (`Skin.cpp:4916-5323`)
- **Left down:** run `LeftMouseDownAction`. If none ran and `Draggable`, start a window drag.
  With **Ctrl held**, only drag; no actions run (`:4988-4996`).
- **Left up:**
  - `LeftMouseUpAction`.
  - If the press started a drag that **moved** the window, LeftMouseUpAction does **not** fire.
  - If the window did not move, a synthetic up is posted, so it fires (`:4527-4599`).
  - Ctrl+Alt+click toggles skin selection instead (`:5021-5051`).
- **Double-click:** run `…DoubleClickAction`. If none exists at that point, run `…DownAction` as a
  fallback (`:5124-5134, 5175`).
  - The OS sequence is down, up, dblclk, up, so without a DoubleClickAction the DownAction fires
    twice and the UpAction fires twice.
  - Applies to Left, Right, Middle, X1 and X2.
- **Right up:** `RightMouseUpAction`. If none ran, or Ctrl is held, the Rainmeter **context menu**
  opens (`:5185-5200`).
- The context menu also checks (in test mode) whether `RightMouseUp`, `RightMouseDown` or
  `RightMouseDoubleClick` *would* run at the point. **Defining any of the three suppresses the
  menu.** Ctrl+right-click always shows it (`:5289-5323`).
- **Middle, X1, X2:** plain down, up and double-click actions.
- **Wheel:** `MouseScrollUpAction` (delta>0) or `MouseScrollDownAction`, once per wheel message
  regardless of delta. Horizontal: `MouseScrollRightAction` (delta>0) or `MouseScrollLeftAction`
  (`:4033-4066`).
  - Windows only delivers wheel messages to the focused window. Rainmeter uses raw input so
    unfocused skins under the cursor get them (`:386-400, 5658+`). Snowmeter should deliver scroll
    to the skin under the pointer.
- **Selected skins** (Ctrl+Alt selection) run no actions.

### 9.4 Over / Leave (`Skin.cpp:3929-4031, 5519-5656`)
On every mouse move, run LEAVE processing repeatedly until it returns false, then OVER processing
likewise. Each call runs at most one action. If the action changed the move counter (reentrancy),
the loop stops.

- **Meters, topmost first.** A meter enters "over" only if it has a `MouseOverAction` or
  `MouseLeaveAction` (or is a Button). Then:
  - It gets `MouseOverAction` once on entering.
  - It gets `MouseLeaveAction` once when the point leaves its hit area, or when it becomes hidden
    (a hidden meter is not "hit").
  - **Several overlapping meters can be "over" at the same time.**
- **Skin level:**
  - The skin-level `MouseOverAction` runs the first time the pointer is over any meter or the window
    area. It runs before any meter-level over action of the same move.
  - The skin-level `MouseLeaveAction` runs when the point is outside the window.
  - When the pointer leaves the window (`WM_MOUSELEAVE`), all meters are force-left at
    (SHRT_MIN, SHRT_MIN) (`:4013-4031`).
- ClickThrough skins do not process over/leave unless a modifier key (Ctrl, Shift or Alt) is held (`:3998`).

### 9.5 Hit testing
- **Meter hit test** = axis-aligned rect `X ≤ x < X+W, Y ≤ y < Y+H`, intersected with its container
  (`Meter.cpp:195-204`). It **does not look at pixel alpha**. Overrides:
  - Shape meters test the actual geometry (`MeterShape.cpp:244-263`).
  - Bitmap with `Extend` (`MeterBitmap.cpp:87+`).
  - Button meters use their image's non-transparent pixels for the button behavior and cursor (`HitTest2`).
- **Window level:** skins are per-pixel-alpha layered windows. **Windows routes the mouse through
  pixels with alpha 0**, so they belong to whatever is below. The skin never sees those events,
  regardless of meters or actions. That is why skins use `SolidColor=0,0,0,1` (alpha 1, invisible
  but hittable) to make empty areas clickable. The in-app check `HitTestDevice` only tests bounds
  (`Skin.cpp:3827-3839`, `Common/Gfx/Canvas.cpp:387-400`).
  **Snowmeter must emulate this**: set the window's input region or shape from the rendered alpha
  channel (alpha > 0), recomputed after each redraw.
- **Drag area:** with `Draggable=1` (and dragging not globally disabled), points inside the window
  minus `DragMargins` are a caption (drag) area. If no button-down action consumed the press, a
  drag starts (`Skin.cpp:4657-4687`).
  - A negative `DragMargins.left`/`top` counts from the opposite edge.
  - `right`/`bottom` are subtracted from W/H; if that exceeds W/H, W/H is subtracted again.
- **ClickThrough=1:** the window ignores the mouse, except while Ctrl, Shift or Alt is held
  (`:3743-3750, 3957-3964`).

### 9.6 Cursor (`Skin.cpp:3841-3913`, `Mouse.cpp:185-262`)
Walk meters from topmost to bottom:
- The first non-hidden meter with `MouseActionCursor=1` (default inherited from `[Rainmeter]`, which
  defaults to 1) that has any button or scroll action and is hit sets the cursor:
  `MouseActionCursorName` (meter, else the skin's, default HAND).
- If the meter's button actions are all disabled or empty, the cursor is an arrow.
- Otherwise the cursor is an arrow. A TextEdit gives an I-beam (fork).

---

## 10. Bangs and CommandHandler (`Library/CommandHandler.cpp`)

### 10.1 Executing an action string (`ExecuteCommand`, `:316-511`)
Actions from a meter or measure run with that section as the current section
(`Library/Rainmeter.cpp:1672-1683`). Steps:

1. Strip leading whitespace.
2. **Starts with `!`** → a single bang.
   - `!Execute …` → continue from the first `[` (legacy).
   - A `!Rainmeter` prefix is stripped (`!RainmeterRefresh` = `!Refresh`).
   - Bang name = text up to the **first space** (ASCII case-insensitive, ≤63 chars).
   - Arguments = `ParseString(rest)` (§10.2). Then `ExecuteBang`.
3. **Starts with `[`** (multi mode) → bracket groups:
   - Scan, counting `[`/`]` depth. **Only `"""…"""` protects brackets; `"…"` does not.**
   - Text outside top-level groups is ignored.
   - For each top-level group:
     - (a) If the char after `[` is a variable key (`& # % $ \`) and `ReplaceMeasures(group)`
       changed it: rerun `ExecuteCommand(replaced + all trailing text)` and stop. The replacement is
       wrapped in `[]` unless it already starts with `[`. This lets a variable or measure hold a
       whole bang list.
     - (b) Else take the inner text with leading whitespace skipped.
     - (c) If the inner text is `!Delay <n>` (exactly one argument; `ParseUInt`, a formula is
       allowed): run **the rest of the string after this group** after n ms on a skin timer, and
       stop (`:410-419`, `Skin.cpp:1513-1519, 3650-3659`).
     - (d) Else `ExecuteCommand(inner, multi=false)`.
4. **Otherwise** (plain text):
   - `PLAY file`, `PLAYLOOP file` and `PLAYSTOP` are built-in sound commands. Quotes are stripped,
     section variables expanded, and the path made absolute (`:441-484`).
   - Otherwise, if the text starts with a variable key, or is exactly a measure name, wrap it in
     `[]`. If replacement then happens, rerun as multi (a measure whose string is `[!Bang…]` runs).
   - Otherwise apply `ReplaceMeasures` and **RunCommand** (`:486-510`).

**RunCommand** (`:513-579`):
- If the text starts with `"`: the file is between the quotes and the arguments follow the closing
  quote. Otherwise split at the **first space**.
- `ShellExecuteEx("open")` with the working directory = the file's folder and `%ENV%` expanded. A
  directory is opened in Explorer, and URLs open the browser. `RestartRainmeter.exe` → restart.
- Snowmeter mapping: URL or directory or non-executable → `xdg-open`. Executable → spawn with
  arguments. Translate `\` to `/` and resolve case-insensitively (§12).

### 10.2 Argument parsing (`ParseString`, `:581-677`)
Loop: skip spaces (only `' '`). Then:
- `"""` → the argument runs to the next `""" ` (triple quote + space), else to the **last** `"""`.
  It is taken verbatim, including inner `"` and trailing spaces.
- `"` → the argument runs to the next `"`, and any remaining `"` chars are removed.
- Otherwise → the argument runs to the next space, and **all `"` chars inside it are removed**.
- An unterminated quote → the rest of the string.

Each argument then gets `ReplaceMeasures` (section variables, `[#Var]`, `[\x]`; old `#Var#` was
already expanded at read time).

### 10.3 Bang dispatch (`ExecuteBang`, `:1414-1451`; table `:1185-1307`)
- **Skin bangs** (`DoBang`, `:78-153`) take `argCount` fixed arguments plus an optional config:
  - Exactly argCount, from a skin → that skin.
  - argCount+1 → the skin named by the extra argument (config path, case-insensitive, slashes
    trimmed). `*` or empty → all skins. An inactive config → warning; a nonexistent one → error.
  - Exactly argCount with no skin (command line) → all skins.
  - Fewer → `Incorrect number of arguments`. `!CommandMeasure "M Cmd"` (one argument) is split at
    the first space for backward compatibility.
- **Group bangs** (`DoGroupBang`, `:155-173`): argument[argCount] is the skin group. The bang
  applies to each skin in that group by LoadOrder.
- Measure/meter name arguments are case-insensitive. **(fork)** `Group=G` in a name argument
  targets section groups. A non-group bang on a missing name → `![Bang]: [Name] not found` error.
- Numeric arguments use `ParseInt` / `ParseDouble`: a leading `(` means formula, else
  `wcstol`/`wcstod`. Toggle bangs take 0/1, or **-1 = toggle**.
- Section-typed bangs `Type:Bang` and `!Skin:<bang> <config|Group=G> …` are **(fork)**.
  ParseBangNumber vectors (`Library/CommandHandler_Test.cpp:99-166`):
  - `" 12.5 "`→12.5; `" (2 + 3) * 2 "`→invalid; `" ((2 + 3) * 2) "`→10.
  - Relative: `+(2 + 3)`→5 rel; `-(2 + 3)`→-5 rel; `(-5)`→-5 abs; `-5` (non-relative ctx)→-5 abs.
  - Invalid: `"" " " + --1 +-1 12tail (2 +) "(2 + 3)tail" 1e9999 nan inf`.
  - Int: `-(7 / 2)`→-3; `2147483648`→invalid.

### 10.4 Bang table
M = must-have for Snowmeter v1. `[cfg]` = optional trailing config. Toggle values are 0/1/-1.

| Bang | Args | Effect / notes | M |
|---|---|---|---|
| `!Refresh` | [cfg] | posted refresh of the skin; `*` = all | M |
| `!RefreshApp` | – | refresh everything (rescan skins, reread settings) | M |
| `!RefreshGroup` | group | | |
| `!Redraw` / `!RedrawGroup` | [cfg] / group | §8.5 | M |
| `!Update` / `!UpdateGroup` | [cfg] / group | §8.5 | M |
| `!UpdateMeasure` / `!UpdateMeter` | name\|`*`\|Group=G [cfg] | §8.5 | M |
| `!UpdateMeasureGroup` / `!UpdateMeterGroup` | group [cfg] | section group | M |
| `!SetVariable` | var value [cfg] | `(…)` formula → `%.5f` with zeros trimmed (`Skin.cpp:2055-2070`) | M |
| `!SetVariableGroup` | var value group | | |
| `!SetOption` | section key value [cfg] | §8.5. (fork) the 2-argument form uses the current section | M |
| `!SetOptionGroup` | group key value [cfg] | all measures and meters in the section group | M |
| `!WriteKeyValue` | section key value [file] | default file = the skin's .ini. Relative → config folder. Must be under SkinsPath or SettingsPath and contain no `..\` or `../`. The file must exist and be writable. `(…)` formula → `%.5f` trimmed. Writing keeps the encoding (`CommandHandler.cpp:955-1041`). **It does not refresh** | M |
| `!ShowMeter` / `!HideMeter` / `!ToggleMeter` | name [cfg] | writes `Hidden`, size check. No redraw | M |
| `!ShowMeterGroup` / `!HideMeterGroup` / `!ToggleMeterGroup` | group [cfg] | | M |
| `!MoveMeter` | x y meter [cfg] | sets absolute X/Y (also writes the options) | |
| `!EnableMeasure` / `!DisableMeasure` / `!ToggleMeasure` | name [cfg] | §8.4 | M |
| `!PauseMeasure` / `!UnpauseMeasure` / `!TogglePauseMeasure` | name [cfg] | §8.4 | M |
| `…MeasureGroup` variants of the 6 above | group [cfg] | | M |
| `!CommandMeasure` / `!CommandMeasureGroup` | measure cmd [cfg] | measure-specific (Script/Plugin/ActionTimer…) | M |
| `!PluginBang` | "measure cmd" | legacy; `"` stripped, split at the first space | |
| `!ActivateConfig` | cfg [file.ini] | without a file: the next variant (inactive → the first .ini; last → wraps to the first). Already active with the same file → bring to front (`Rainmeter.cpp:1171-1262`) | M |
| `!DeactivateConfig` | [cfg\|`*`] | without args: the current skin | M |
| `!ToggleConfig` | cfg file | active (any variant) → deactivate, else activate | M |
| `!DeactivateConfigGroup` | group | | |
| `!Move` | x y [cfg] | window top-left = primary-monitor origin + (x,y). Anchor is ignored. Saves if SavePosition (`Skin.cpp:1354-1360`) | M |
| `!SetWindowPosition` | X Y [AX AY] [cfg] | WindowX/Y syntax (§11.3), with formulas (`:1362-1376`) | |
| `!SetAnchor` | AX AY [cfg] | | |
| `!ZPos` (`!ChangeZPos`) / `!ZPosGroup` | -2..2 [cfg] | persisted | M |
| `!Draggable` / `!ClickThrough` / `!SnapEdges` / `!KeepOnScreen` / `!AutoSelectScreen` (+`Group`) | 0/1/-1 [cfg] | persisted to Rainmeter.ini | M |
| `!SetTransparency` / `Group` | 0-255 [cfg] | clamped; **not** persisted (`Skin.cpp:1437-1445`) | M |
| `!FadeDuration` / `FadeDurationGroup` | ms [cfg] | applied and persisted at the next fade | |
| `!Show` / `!Hide` / `!Toggle` (+`Fade`, +`Group`) | [cfg] | window visibility (Fade variants animate) | |
| `!SetZoomFactor` | n\|Default [cfg] | (fork) 0.1-5.0 | |
| `!DisableMouseAction` / `!EnableMouseAction` / `!ToggleMouseAction` / `!ClearMouseAction` | meter\|`Rainmeter`\|`*` actions [cfg] | `actions` = `\|` list of option names or `*` (§9.1). `*Group` variants: (actions group); `*SkinGroup`: (actions skingroup) applies to `[Rainmeter]` | |
| `!Log` | msg [Notice\|Warning\|Error\|Debug] | an invalid type is an error | M |
| `!About` / `!Manage` / `!Debug` / `!SkinMenu` / `!TrayMenu` / `!SkinCustomMenu` | various | UI; Snowmeter may no-op or log | M (no-op) |
| `!EditSkin` / `!Skin:Edit` | [cfg [file]] | open in editor | |
| `!LoadLayout` | name | | |
| `!SetClip` | text | clipboard | |
| `!SetWallpaper` | file [style] | | |
| `!SendKey` | media key name | (fork) | |
| `!ResetStats`, `!Quit`, `!Restart`, `!LsBoxHook`(no-op), `!ShowBlur` / `!HideBlur` / `!ToggleBlur` / `!AddBlur` / `!RemoveBlur`, `!ForceEnableVisibleMode` | | Blur can be a no-op | |
| `!Delay` | ms | **only inside a multi-bang group**; delays the rest (§10.1) | M |

---

## 11. Settings in Rainmeter.ini

### 11.1 Configs and `Active` (`Library/SkinRegistry.cpp:217-352`; `Rainmeter.cpp:1156-1262, 1691-1838`)
- A config name is a folder path below the Skins folder joined with `\`: `illustro\Clock`.
- Rules:
  - Root-level files are ignored.
  - Root folders `@Backup`, `Backup` and `@Vault` are ignored (case-insensitive).
  - `@Resources` is ignored at level 1 only (case-*sensitive* comparison).
  - A folder appears if it has `.ini` files (case-insensitive extension) or descendants that do.
  - Config lookup is case-insensitive (`SkinRegistry.cpp:37-79`).
- The **file list order** is the order `FindFirstFile`/`FindNextFile` returns, unsorted. On NTFS that
  is ordinal order of uppercased names. **Snowmeter: sort by ASCII-uppercased name, ordinal.**
- `Active=N` is the 1-based index into that list. 0 or a missing value = inactive. Out of range is
  ignored (`Rainmeter.cpp:1820-1825`). It is rewritten on activation, and set to 0 (`fileIndex+1`
  with -1) on deactivation (`:1339-1345`).
- Startup activates the configs that have a Rainmeter.ini section with a valid Active. Order:
  **LoadOrder ascending, ties in Rainmeter.ini section order** (`:1156-1167, 1827-1828`).
- LoadOrder also orders refresh-all and group bangs. Its default is 0.

### 11.2 Per-config keys (`[illustro\Clock]`; `Skin.cpp:2201-2331, 2333-2468`)
These are read from the Rainmeter.ini section for the config. Variables expand (but there is no
`[Variables]`, and `#CURRENTPATH#` = the settings folder). On first run, the skin's
`[Rainmeter] Default<Key>` versions are copied in (only those it defines).

| Key | Default | Meaning |
|---|---|---|
| `Active` | 0 | §11.1 |
| `LoadOrder` | 0 | §11.1 |
| `WindowX`, `WindowY` | 0 | §11.3. Saved on drag if SavePosition |
| `AnchorX`, `AnchorY` | 0 | §11.3 |
| `AlwaysOnTop` | 0 | -2 on desktop, -1 bottom, 0 normal, 1 topmost, 2 stay topmost (re-asserted). Out of range → 0 |
| `Draggable` | 1 | |
| `ClickThrough` | 0 | |
| `KeepOnScreen` | 1 | clamp to the screen |
| `SnapEdges` | 1 | snap within 10 px of other skins and screen edges (`Skin.cpp:67`) |
| `SavePosition` | 1 | write WindowX/Y after moves |
| `StartHidden` | 0 | |
| `OnHover` (`HideOnMouseOver` deprecated) | 0 | 0 none, 1 hide, 2 fade in, 3 fade out |
| `AlphaValue` | 255 | clamped 0-255 |
| `FadeDuration` | 250 | ms, ≥0 |
| `AutoSelectScreen` | 0 | rewrite the `@N` after moves |
| `Group`, `DragGroup` | "" | prepended to the skin's `[Rainmeter] Group` |
| `Zoom` | (none) | (fork) 10-500 |

### 11.3 WindowX / WindowY / Anchor syntax (`Library/SkinPosition.cpp`)
- If the value is `(...)…`, the formula part is evaluated first (§4.1).
- Number = the leading `[-0-9.]*` prefix (`_wtof`).
- Modifiers count only if they appear after the last `#`:
  - `%` → percent of the monitor extent.
  - `R` (X) / `B` (Y) (uppercase, anywhere) → measured from the right or bottom edge.
  - `@N` → monitor N (1-based; 0 = virtual screen). Invalid or inactive → ignored. If only one axis
    has `@N`, the other axis copies it.
- `pos = monitorOrigin + (fromOpposite ? extent - off : off) - anchorPx`, where
  `off = percent ? extent*v/100 : v` (`:100-105`).
- Anchor: number prefix `[0-9.]*` (no minus), `%` of the window size, `R`/`B` from the opposite
  edge. All are scaled by zoom (`:8-19`).
- Default monitor = primary.
- Saved format (`:60-98`): integer, or `%.5f%` when percent, plus `R`/`B`, plus `@N`.
  - A never-moved skin keeps its authored text.
  - `!Move` rewrites it as plain numbers.

SkinPosition test vectors (`Library/SkinPosition_Test.cpp`):
- `10R` (origin 100, extent 1000) → 1090; `25%` (100, 800) → 300; `10B` (50, 600) → 640
- `-100` (10, 1000) → -90; `-100R` (50, 1000) → 1150
- Anchor `10` (w200): `100` → 90; anchor `10R` → anchorPos 190, pos -90
- Anchor `25%` → 50, pos 50; anchor `25%R` → 150, pos -50; anchor `10` at zoom 2 → 20, pos 80

---

## 12. Paths

- `MakePathAbsolute` (`Skin.cpp:6042-6058`): empty or absolute paths are kept. Absolute means it
  contains `:\` or `:/`, or starts with `\\` or `//` (`PathUtil.cpp:23-28`). Anything else becomes
  `SkinsPath + config + "\" + path`, the **folder of the main .ini** (= `#CURRENTPATH#`), even for
  options that came from an included file.
  - `\foo` (a single leading separator) is *not* absolute, so it becomes `…\Clock\\foo`.
  - `..\` is preserved and resolved by the OS.
  - `/` is accepted as a separator.
- Used by: images (prefixed by `ImagePath`; `.png` appended when there is no extension,
  `GeneralImage.cpp:346-355`), `Background`, `LocalFont` fallback, `ScriptFile`, `!WriteKeyValue`,
  `PLAY`, `!SetWallpaper`, and FolderInfo, RunCommand and other measure paths.
- `@Include` paths are relative to the **including file's** folder (§2.1).
- Typical authored forms: `#@#Images\bg.png`, `#CURRENTPATH#x.png`, `#ROOTCONFIGPATH#…`,
  `%APPDATA%\…`, `C:\…`.
- **Snowmeter requirements:**
  - Convert `\` to `/`.
  - Resolve each component **case-insensitively** against the file system (Windows paths are
    case-insensitive; cache directory listings).
  - Map drive-letter paths (`C:\Users\…`) through a configurable prefix map, or report them as
    unsupported.
  - Map `%APPDATA%`, `%USERPROFILE%` etc. to XDG equivalents for `%ENV%` expansion.
  - Keep built-in path variables in Windows form internally (trailing `\`). Normalize only at the
    point of file system access, so string concatenation and comparisons in skins behave the same.

---

## Implementation checklist

1. Decoder: UTF-16LE (BOM or heuristic), UTF-8 (BOM, or strict without BOM), Windows-1252 fallback. Reject UTF-16BE and odd-length UTF-16.
2. Lines on CR/LF/CRLF. Trim ≤U+0020. `;` comments only at line start. `[name]` up to the first `]`. Split at the first `=`. Strip one pair of `"…"`/`'…'`. Ignore names >255 chars.
3. ASCII case-insensitive names everywhere (sections, options, variables, bangs, measure lookups).
4. Within a file: first duplicate section wins (later blocks dropped entirely), first duplicate key wins. Across includes: last processed write wins.
5. `@Include*` keys in any section, in key order. Expand `#vars#`/`[#vars]`/built-ins (after ReadVariables). Resolve relative to the including file. Depth ≤100. Section ordering exactly per §2.3, including the last-section quirk.
6. Variable table: built-ins → CURRENTCONFIG* → monitor vars → user vars. `[Variables]` read in order with eager expansion.
7. Old-style `#X#` scanner with `#*X*#` escape, the unknown-closer-reuse rule and no rescan. `%ENV%` when SyntaxVersion<2.
8. Bracket expander (§3.4) with key types `& # % $ \`, escape `[?*…*]`, recursion depth 10. Then the old-style `[Name]` pass and the selector grammar (§3.6).
9. Option read pipeline: own value → styles (reverse) → variables → section variables (skipped for actions). Track flags: default used, defined, replaced.
10. Numeric reads with C `strtol`/`strtod` prefix semantics (32-bit long), formulas when the value starts with `(`, ReadBool = int≠0, colors/rects per §5.
11. MathParser exactly per §4: priority table, left-associativity, unary-minus lexing rule, `^`=XOR, `**`=power, both ternary branches evaluated, error strings, measure-name resolver, Calc extras.
12. Measure/meter creation order, the Measure=/Meter= rule, no meters → fail. Load sequence §7.1 and the refresh sequence §8.1 (OnRefreshAction last).
13. Tick algorithm §8.2, UpdateDivider counter semantics §8.3 (≤-1 = only forced), DynamicVariables re-read only on firing ticks. Redraw only when a meter updated.
14. Mouse dispatch: topmost meter having *that* action type and hit; skin fallback; double-click → down fallback; drag suppresses LeftMouseUp; the right-click menu suppressed by any RMB action; Ctrl overrides.
15. Over/leave state machine per meter and skin. Mouse variables at fire time.
16. Input region from alpha>0 (layered-window emulation). Drag via DragMargins. ClickThrough with the modifier-key exception.
17. Command executor: `!bang`, `[..][..]` groups with depth counting and `"""` protection, `!Delay`, variable-group pre-expansion, run-command via xdg-open/spawn. ParseString quoting rules.
18. Bang arg convention: fixed count, plus optional config / `*`, plus group variants. All M bangs in §10.4.
19. Rainmeter.ini: per-config keys §11.2, Active index over a sorted .ini list, LoadOrder. Deferred writes that preserve encoding (UTF-16LE input).
20. WindowX/Y/Anchor parser and formatter §11.3, multi-monitor `@N`, `#SCREENAREA*#` following the `@N`.
21. Path resolution §12 with case-insensitive lookup and `.png` defaulting.

---

## Test vectors

Variables (A=abc, BB=def, CCC=`#A#`, Index=2, Var2=Second; string measures MS2="Second", MI="2"):
```
"#A#"                      -> "abc"
"#a#"                      -> "abc"           (case-insensitive)
"#*A*#"                    -> "#A#"
"#Foo#A#"                  -> "#Fooabc"       (unknown closer reused)
"#A##a# #*CCC*# #BB##CCC#" -> "abcabc #CCC# def#A#"
"[#Var[#Index]]"           -> "Second"
"[#*Var[#Index]*]"         -> "[#Var2]"
"[MS2] [&MS2] [#Var2]"     -> "Second Second Second"
"[*MS2*]"                  -> "[MS2]"
"[MeasureString[MI]]"      -> "[MeasureString2]"   (old-style, no rescan)
"[\65][\x42][\X43]"        -> "ABC"
"[#]" / "[\0]"             -> unchanged
[Variables] A=#B# then B=1 (no later include) ; option "#A#" -> "#B#"
[Variables] A=[#B] then B=1                    ; option "#A#" -> "1"
"#CRLF#"                   -> "\n"
```
Section selectors (Min 10, Max 50, value 30): `[M:]`→30, `[M:2]`→30.00, `[M:%]`→50, `[M:%,1]`→50.0,
`[M:/5]`→6, `[M:MaxValue:/5, 2]`→10.00, `[M:/0]`→unchanged. Meter (10,20,30,40): `[Mt:XW]`→40, `[Mt:YH]`→60.

Formulas: see §4.7 (all MathParser_Test cases), plus:
`(10)+5`→15 via ReadInt; `10+(5)`→10 via ReadInt; `X=(5*2)R` → 10, relative-BR;
`!SetVariable V (1/3)` → `0.33333`; `!SetVariable V (2*3)` → `6`.

Include order (section lists):
```
main[A]{@Include=x}, main[B]; x:[X1][X2]                  -> A X1 X2 B
main[A]{@Include=x,@Include2=y}, [B]; x:[X]; y:[Y]       -> A X Y B
main[A]{@Include=x}, [B]; x:[X1][X2]{@Include=y}; y:[Y]  -> A X1 X2 B Y
main[A]{@Include=x}, [B]; x:[X1]{@Include=y}[X2]; y:[Y]  -> A X1 Y X2 B
[Variables] A=1 @Include=v(A=2,B=2) B=3                  -> A=2, B=3
[A] X=1 / [A] Y=2 (same file)                            -> A.X=1, A.Y undefined
```

MeterStyle: `S1.C=1; S2.C=2; M.MeterStyle=S1|S2` → 2. `M.C=9` → 9. `M.C=` (empty) → default.
A style's own MeterStyle is not followed.

Bang parsing (ParseString):
```
'Meter Text "Hello World"'      -> [Meter, Text, Hello World]
'M Text """a "b" c"""'          -> [M, Text, a "b" c]
'ab"cd'                         -> [abcd]
'a   b'                         -> [a, b]
'M Text ""'                     -> [M, Text, ""]
'"unterminated'                 -> [unterminated]
```
ExecuteCommand:
```
"[!SetVariable A 1][!Redraw]"   -> SetVariable(A,1); Redraw
"[!Log "a]b"]"                  -> Log("a"); remainder "b"]" ignored
"[!Log """a]b"""]"              -> Log("a]b")
"[!A] junk [!B]"                -> A; B
"[!Delay 500][!B][!C]"          -> after 500 ms: [!B][!C]
"!RainmeterRefresh"             -> Refresh
"[#Bangs]" with Bangs=[!A][!B]  -> A; B
value "C:\Program Files\x.exe" (quotes stripped by INI) -> run "C:\Program" with args "Files\x.exe"
'["C:\Program Files\x.exe"]'    -> run "C:\Program Files\x.exe"
```
INI: `Key = "  v  "` → `  v  `; `K=1 ; c` → `1 ; c`; `Empty=""` → empty; bytes `C3 A9 FF` → ANSI;
ReadBool(`true`) → false; ReadInt(`abc`, 5) → 0; ReadInt(``, 5) → 5; ReadRECT(`5`) → 0,0,0,0.
