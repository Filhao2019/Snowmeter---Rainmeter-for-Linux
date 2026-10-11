# Meters: behavior spec (from Rainmeter source)

Source of truth: `/home/bernardo/linux-rainmeter/rainmeter-src` (modern Rainmeter with D2D/DirectWrite, `IniNameRegistry`, `SyntaxVersion`, Svg/TextEdit meters). All `file:line` references are relative to that tree. Paths: `L/` = `Library/`, `G/` = `Common/Gfx/`, `C/` = `Common/`.

Conventions used below:
- "px" = logical pixel (DIP). Rainmeter draws in DIPs and lets the canvas DPI scale (`Canvas::SetDpiScale`, `G/Canvas.cpp:477`; `m_EffectiveScale = zoom * dpiScale`, `L/Skin.cpp:785`). At 100% scale 1 DIP = 1 px.
- Coordinates are y-down. Every D2D rotation with a positive angle is **clockwise on screen**.
- Colors are parsed to floats `component/255` (`C/ParseUtil.cpp:202-230`). Named D2D defaults used by meters: White=255,255,255; Black=0,0,0; **Green=0,128,0** (not 0,255,0); Red=255,0,0; Yellow=255,255,0; **Gray=128,128,128**; "transparent" = 0,0,0,0 (`G/Util/D2DUtil.h:10`).
- Integer options: `ReadInt` evaluates a formula only if the value starts with `(`, else `wcstol` (so `10.7` -> 10, `(10.7)` -> 10, truncation toward zero) (`L/ConfigParser.cpp:1386-1417`). Floats likewise with `wcstod` (`:1485`). Bools are `ReadInt(...) != 0` (`ConfigParser.h:238`).
- Enum options (`StringStyle`, `BarOrientation`, ...) are case-insensitive whole-string matches; an unknown value logs an error and yields the default (`L/ConfigParser.cpp:1315-1330`).
- An option counts as **defined** only if present and non-empty (after `@Inherit`/`MeterStyle` lookup); `W=` (empty) is "not defined" (`L/ConfigParser.cpp:1281-1286`).
- `StringParser::Consume` keyword matches are case-insensitive unless `MatchCase` (`C/StringParser.h:19-34`), so `Rectangle`, `rectangle`, `RECTANGLE` are all fine.

---------------------------------------------------------------------------------------------------

## 1. Base Meter (`L/Meter.cpp`, `L/Meter.h`, `L/Section.cpp`, `L/Skin.cpp`)

### 1.1 Creation, order, types
- Every section with a non-empty `Meter=` (and no `Measure=`) becomes a meter; `Measure=` wins if both are present (`L/Skin.cpp:2806-2888`). Meter types (case-insensitive): String, TextEdit, Image, Svg, Histogram, Bar, Bitmap, Line, RoundLine, Rotator, Button, Shape (`L/Meter.cpp:496-550`). Unknown type: error, section skipped.
- `m_Meters` is in section (file) order, with `@Include`d sections inserted right after the section holding the `@Include` (`L/ConfigParser.cpp:1670-1672`). **Drawing order = this order** (later meters on top) (`L/Skin.cpp:3196-3213`). Mouse dispatch walks it in reverse (topmost first).
- Load sequence (`L/Skin.cpp:2899-2940`): read `Container` of all meters -> compute relative chain -> read measure options -> per meter `ReadOptions` + `Initialize` -> measures `Initialize` -> one forced `Update` of every meter -> `ResizeWindow(true)`.

### 1.2 Option inheritance (`MeterStyle`)
- `@Inherit=` (if present) else `MeterStyle=` is a `|`-separated list of section names (`L/ConfigParser.cpp:83-96`). A missing option is looked up in that list **from last to first**; the first style that has it wins, so **later styles override earlier ones** (`L/ConfigParser.cpp:1212-1226`). Not recursive (a style's own `MeterStyle` is not followed). The meter's own value always wins.

### 1.3 X / Y and relative positioning
Parsing (`L/Meter.cpp:285-339`): read the string (default `"0"`). If the **last character** is `r` -> `RELATIVE_TL`, `R` -> `RELATIVE_BR` (case-sensitive), suffix removed; else absolute. The rest goes through `ParseInt` (leading whitespace skipped, `(`-formula evaluated and truncated to int, otherwise integer prefix: `"10.9"` -> 10) (`C/ParseUtil.cpp:18-39,132-138`). Empty -> 0 absolute.

Relative meter (`Skin::UpdateRelativeMeters`, `L/Skin.cpp:3318-3357`):
- For a non-contained meter: the **previous non-contained meter in file order, hidden or not**. Contained meters are skipped by the outer chain.
- For a contained meter: the previous meter contained in the **same** container, or the container itself for the first one.
- The first meter has no relative meter -> `r`/`R` behave as absolute.

Position (`L/Meter.cpp:71-115`), with `prev = m_RelativeMeter`:
```
r : X = prev.GetX(abs=true) + x
R : X = prev.GetX(abs=true) + prev.GetW() + x
absolute: X = (container ? container.GetX(true) : 0) + x
(same for Y with GetY/GetH)
```
- `GetW()/GetH()` return **0 for a hidden meter** (`L/Meter.h:39-40`), but its X/Y are still computed normally. So `R` after a hidden meter = hidden meter's X + 0 + x.
- `GetX(abs=true)` is the meter's **anchor**; only String/TextEdit distinguish it from the drawn rect left (see 2.6). Consequence: `X=0R` after a right-aligned string at X=100, W=50 gives 150, not 100.
- The relative case does not add the container offset again (prev already includes it).
- First contained meter of a container is **forced** to `RELATIVE_TL` on both axes, relative to the container (`L/Meter.cpp:475-484`). So contained meters' plain X/Y are offsets from the container anchor.
- `!MoveMeter`/`SetX` make the coordinate absolute (`L/Meter.cpp:117-135`).

### 1.4 W / H, Padding, auto sizing
- `Padding=l,t,r,b` is parsed with `ParseRect`, which stores `{l, t, l+r, t+b}`; a value without a comma parses as all zeros (`C/ParseUtil.cpp:44,232-239`). Hence `GetWidthPadding() = l+r`, `GetHeightPadding() = t+b` (`L/Meter.h:44-45`). Integers only.
- `m_W/m_H` always **include** padding. For fixed-size meters (everything except String/TextEdit, see `IsFixedSize`), `m_W = W + l + r`, `m_H = H + t + b`; missing W/H keeps the previous content size (initially 0) (`L/Meter.cpp:341-371`).
- Content rect (`GetMeterRectPadding`, `L/Meter.cpp:168-176`): `(X + l, Y + t, X + m_W - r, Y + m_H - b)`.
- Auto-size per type (details in each section):
  | Type | W/H when not given | Do W/H options win? |
  |---|---|---|
  | String | measured text (+padding) | yes, per axis |
  | Image | image size, or aspect-derived from the one given | yes |
  | Bar | from BarImage if loaded | **no**: BarImage size always overrides (`L/MeterBar.cpp:35-41`) |
  | Button | one frame of ButtonImage | **no**, always frame size (`L/MeterButton.cpp:45-71`) |
  | Bitmap | one frame (x digits if BitmapExtend) | **no** (`L/MeterBitmap.cpp:38-73`) |
  | Histogram | PrimaryImage size if given | **no** when PrimaryImage loads (`L/MeterHistogram.cpp:93-110`) |
  | Shape | union of shape bounds | only if **both** W and H given (`L/MeterShape.cpp:182-201`) |
  | Line, RoundLine, Rotator | 0 | yes (no auto size) |

### 1.5 Hidden, UpdateDivider, DynamicVariables
- `Hidden` (bool, formula allowed) (`L/Meter.cpp:373-375`). Hidden meters: not drawn (`Meter::Draw` returns false, `L/Meter.cpp:780`), ignored by mouse actions and tooltips, width/height 0 in relative chains and in window sizing (but their X/Y still count, see 1.11).
- `UpdateDivider` default = `[Rainmeter] DefaultUpdateDivider` (default 1) (`L/Section.cpp:32-38`). Counter starts equal to the divider, so the first update fires; then fires every N skin updates (`L/Section.cpp:53-60`). Negative divider: never updated except on refresh/force (`L/Skin.cpp:3402-3412`).
- `DynamicVariables=1`: options are re-read (`ReadOptions` incl. MeterStyle) right before an update that will fire (`(counter+1) >= divider`) (`L/Skin.cpp:3405-3409`). Image/Bar/Button/Bitmap/Rotator reload their image when options are re-read (`if (m_Initialized) Initialize()`).
- Skin redraw happens when any meter's `Update()` fired (base `Meter::Update` returns true whenever its divider fires, even if nothing changed) (`L/Skin.cpp:3474-3502`).

### 1.6 Background: SolidColor, SolidColor2, GradientAngle, BevelType (`L/Meter.cpp:778-846`)
Drawn first, inside `Meter::Draw`, over the **full** meter rect `(X, Y, X+m_W, Y+m_H)` (padding included), using the meter's AntiAlias mode.
- `SolidColor` default 0,0,0,0. `SolidColor2` default = SolidColor. `GradientAngle` (float degrees) default 0.
- Skipped if both alphas are 0. Equal colors -> solid fill; else linear gradient (gamma 2.2 = ordinary sRGB-space interpolation, clamp) from `start = EdgePoint(angle+180)` (SolidColor) to `end = EdgePoint(angle)` (SolidColor2) (`G/Canvas.cpp:1058-1094`). With `EdgePoint(θ)` from `G/Util/D2DUtil.cpp:23-64`, which is equivalent to
  ```
  c = rect center; L = (w*|cos θ| + h*|sin θ|) / 2
  EdgePoint(θ) = c + L * (cos θ, sin θ)          (θ in degrees, y down)
  ```
  So angle 0: SolidColor at left edge, SolidColor2 at right; angle 90: SolidColor top, SolidColor2 bottom.
- `BevelType` 0 none, 1 up, 2 down (down swaps colors). `BevelColor` default white (light), `BevelColor2` default black (dark). Drawn **outside** the meter: rect `(X-2, Y-2, X+m_W+2, Y+m_H+2)` with eight 1px lines (`DrawBevel`, `L/Meter.cpp:826-846`); `o = AntiAlias ? 0 : 0.5`, `l=left+o, t=top+o, r=right, b=bottom`: light: (l,t)-(l,b), (l,t)-(r,t), (l+1,t)-(l+1,b-1), (l,t+1)-(r-1,t+1); dark: (l,b)-(r,b), (r,t)-(r,b), (l+1,b-1)-(r-1,b-1), (r-1,t+1)-(r-1,b-1).

### 1.7 AntiAlias
`AntiAlias` (default 0) sets the canvas geometry AA mode before the meter draws (`L/Meter.cpp:782`): 0 = aliased (D2D pixel-center coverage, like `CAIRO_ANTIALIAS_NONE`), 1 = per-primitive AA. For String it also selects text AA: 0 = **aliased (bi-level) glyphs**, 1 = grayscale AA (`G/Canvas.cpp:517-522`). Shape meters force AA on regardless (`L/MeterShape.cpp:231`).

### 1.8 MeasureName / MeasureName2..N
- `MeasureName` binds measure 1 (`L/Meter.cpp:562-580`). Bar/Line/Histogram require it (error logged if invalid); String, Image, Shape, RoundLine, Button, Rotator(optional) treat it as optional.
- String, Image, Shape, Line read `MeasureName2`, `MeasureName3`, ... until the first missing/invalid one (`L/Meter.cpp:583-613`). Histogram reads only `MeasureName2` (fallback `SecondaryMeasureName`).
- `%N` replacement (`ReplaceMeasures`, `L/Meter.cpp:616-647`): only if the text contains `%`; iterates N = count..1 (so `%10` is replaced before `%1`), replacing every occurrence of `%N` with the measure's string value if it has one, else its formatted number (2.2). Default formatting args (used by tooltips): AutoScale 1024, scale 1, 0 decimals, not percentual (`L/Meter.h:142`).

### 1.9 TransformationMatrix
`TransformationMatrix=m11;m12;m21;m22;dx;dy` (exactly 6 `;`-separated floats, formulas allowed; any other count logs an error and leaves identity) (`L/Meter.cpp:412-438`). D2D row-vector convention: `x' = x*m11 + y*m21 + dx`, `y' = x*m12 + y*m22 + dy` (Cairo: `cairo_matrix_init(xx=m11, yx=m12, xy=m21, yy=m22, x0=dx, y0=dy)`). It is set as the world transform for the whole `Draw` of that meter, background included (`L/Skin.cpp:3200-3212`), in skin coordinates. It does **not** affect hit testing (except Shape, 3.7), relative positioning or window size.

### 1.10 Container (`L/Meter.cpp:206-244,443-485`, `L/Skin.cpp:3261-3316`)
- `Container=MeterName` (case-insensitive; self-reference rejected). Contained meters are drawn **only** through the container; the container meter itself is **not drawn visibly**: it is rendered to a texture used as an **alpha mask**.
- Per redraw, for each container (when visited in file order; skipped if W<=0 or H<=0):
  1. Content texture (size W x H) cleared; every contained meter drawn into it in file order with world transform `item.TransformationMatrix * translate(-container.X, -container.Y)`.
  2. Mask texture cleared; container drawn into it with its own matrix and the same translation.
  3. Composite at the container rect: `out = content * mask.alpha` (FillOpacityMask; content pixels outside the container rect are clipped).
- Contained meters are excluded from window sizing and from the outer relative chain. Hit tests of contained meters first require the point to hit the container (`HitTestContainer`, `L/Meter.h:60`).
- Nested containers do not render their own contents (a contained container is drawn as a plain meter).

### 1.11 Window size (`Skin::ResizeWindow`, `L/Skin.cpp:2945-3030`; options `L/Skin.cpp:2579-2623`)
```
w = BackgroundMargins.left; h = BackgroundMargins.top        (ReadRECT: l,t,r,b literal)
for each meter m that is not contained (hidden ones included):
    w = max(w, m.GetX() + m.GetW());  h = max(h, m.GetY() + m.GetH())
w += BackgroundMargins.right; h += BackgroundMargins.bottom
w = max(w, bgImage.w); h = max(h, bgImage.h)                 (cached background size)
if BackgroundMode == 0 and Background image loaded: w,h = image size exactly (meters do NOT enlarge it)
if SkinWidth > 0: w = SkinWidth;  if SkinHeight > 0: h = SkinHeight   (ComputePositionFromOptions, :2142)
```
- `GetX()` is the drawn rect left (String alignment applied); hidden meters contribute `X+0`, i.e. a hidden meter at X=500 still makes the window 500 wide. Meters at negative coordinates are clipped, never shift the window. Drawing outside `X..X+W` (Rotator, RoundLine with W=0, String Angle/effects, Shape negative coords, bevels) is not counted.
- Window W or H of 0 -> nothing drawn (`L/Skin.cpp:3098`).
- `BackgroundMode`: 0 image (default; becomes 1 if `Background` is empty), 1 none/transparent ("copy"), 2 solid, 3 scaled image (9-slice by `BackgroundMargins`), 4 tiled (`L/Skin.h:48-55`). Skin-level `SolidColor` default **Gray 128,128,128**, `SolidColor2`, `GradientAngle`, `BevelType` work like 1.6 on the full window (BackgroundMode=2 only) (`L/Skin.cpp:3163-3193`).
- **When the size is recomputed**: at load/refresh; when `!ShowMeter/!HideMeter/!ToggleMeter/!MoveMeter/ !UpdateMeter` run; when a meter's X, Y or Hidden changes during (dynamic) `ReadOptions` (`L/Meter.cpp:377-380`); and, only if `DynamicWindowSize=1`, on every redraw and whenever a String re-measures to a new size (`L/Skin.h:178-179`, `L/Skin.cpp:3490-3494`). With the default `DynamicWindowSize=0`, a String that grows later is **clipped** by the window until one of the other triggers happens.

### 1.12 Mouse actions and hit testing
- Per-meter options (`L/Mouse.cpp:109-132`): `Left/Middle/Right/X1/X2` x `MouseUpAction`, `MouseDownAction`, `MouseDoubleClickAction`; `MouseScrollUp/Down/Left/RightAction`; `MouseOverAction`, `MouseLeaveAction`; `MouseActionCursor` (default = skin's, skin default 1), `MouseActionCursorName`.
- Click/scroll dispatch (`Skin::DoAction`, `L/Skin.cpp:5327-5376`): meters in **reverse order**, skip hidden; the first meter that **has that action** and whose `HitTest` passes gets it; a meter without that action does not block meters below. If none, the skin-level action runs if the skin hit test passes.
- Mouse over/leave (`L/Skin.cpp:5519-5656`): **every** non-hidden meter under the cursor gets its `MouseOverAction` (not just the topmost); leave fires when the cursor leaves its hit area.
- Default `HitTest` = rectangle `[GetX(), GetX()+m_W) x [GetY(), GetY()+m_H)` in skin coords (aligned rect for strings, padding included, TransformationMatrix ignored) (`L/Meter.cpp:195-204`). Overrides: Shape (geometry), Button (opaque pixel of current frame), Bitmap with BitmapExtend.
- **OS-level rule (Windows, not in this source):** the skin is a per-pixel-alpha layered window (`UpdateLayeredWindow ... ULW_ALPHA`, `L/Skin.cpp:3226-3245`); Windows delivers no mouse input on pixels whose alpha is 0. So every action (meter or skin) only works where the rendered window pixel has alpha > 0. That is why skins use `SolidColor=0,0,0,1` for click areas. Snowmeter must set the input region to `alpha > 0` pixels. The skin-level `HitTest` also reads the rendered pixel (`G/Canvas.cpp:387-400`).

### 1.13 ToolTip* (`L/Meter.cpp:392-408,649-776`)
`ToolTipText` (empty = no tooltip), `ToolTipTitle`, `ToolTipIcon` (`INFO|WARNING|ERROR|QUESTION|SHIELD` or an .ico path), `ToolTipWidth` (default 1000), `ToolTipType` (1 = balloon), `ToolTipHidden` (default `[Rainmeter] ToolTipHidden`, default 0). Text and title get `%N` replaced with the 1.8 defaults. Tooltip area = meter rect intersected with the container rect; inactive when hidden.

---------------------------------------------------------------------------------------------------

## 2. String meter (`L/MeterString.cpp`, `L/MeterStringBase.cpp`, `G/TextFormat.cpp`, `G/Canvas.cpp`)

### 2.1 Building the text (`MeterString::Update`, `L/MeterString.cpp:66-134`)
```
s = Prefix
if a measure is bound:
    if Text empty: s += value(measure1)
    else:          s += Text with %1..%N replaced (1.8) using the meter's formatting
else:              s += Text                         (literal; "%1" stays as is)
s += Postfix
apply StringCase to all of s
for each U+00A0 or U+205F: insert U+200B after it;  delete every '\r'
if TrailingSpaces=1: append U+200B
```
- An invalid `MeasureName` behaves as "no measure" (binding is optional).
- `value(m)` = the measure's string value if it has one (string-type measures; substitutions applied by the measure), else the formatted number (2.2) (`L/Measure.cpp:561-566`).
- `#CRLF#` expands to `"\n"` (LF only) (`L/ConfigParser.cpp:242`). Lines break on `\n`.
- Re-measuring only happens when the text or a layout-relevant option changed (`:123-129`).

### 2.2 Number formatting (`L/Measure.cpp:575-639`, `L/MeterString.cpp:38-70`)
Options: `Percentual` (0), `NumOfDecimals` (-1), `AutoScale` (0), `Scale` (string, default "1").
- `AutoScale` parse: `n = _wtoi(str)`; `n == 0` -> off; else if the string contains `k` or `K` anywhere: `n == 2 ? 1000K : 1024K`, else `n == 2 ? 1000 : 1024`. (So `3` means 1024.)
- `Scale`: `NoDecimals = Scale string has no '.'`; `scale = ParseDouble(str, 1)`.
- `decimals = NumOfDecimals != -1 ? NumOfDecimals : (NoDecimals && (Percentual || AutoScale==off)) ? 0 : 1`.
- Formatting priority:
  1. `Percentual=1`: `100 * relative(m)` with `decimals` (relative = clamp(v, min, max) - min over max - min, or 1.0 if max==min) (`L/Measure.cpp:530-547`).
  2. AutoScale on: thresholds T=b^4, G=b^3, M=b^2, k=b with b=1024 (modes 1,1k) or 1000 (2,2k); first of `v >= T` -> `v/T " T"`, `v >= G` -> `" G"`, `v >= M` -> `" M"`, then `" k"` if `v >= k` **or the mode is a k-mode**, else `v` with suffix `" "` (a single trailing space!). `Scale` is ignored.
  3. Else `v / scale` with `decimals`.
- Number printing: `fmt "{:.{}f}"` (round-to-nearest on the exact binary value, ties-to-even, always '.' decimal separator, no thousands separator) (`L/Measure.cpp:86-94`). Rust `format!("{:.*}", d, v)` matches.
- Measure `Invert` is applied inside `GetValue()`.

### 2.3 Font options
- `FontFace` default `Arial` (empty -> Arial) (`L/MeterStringBase.cpp:483-487`). Resolution (`G/TextFormat.cpp:512-615`): GDI family-name semantics (e.g. `Segoe UI Semibold` is a valid family); **system fonts are tried first**, then the skin's private collection (all files in `@Resources\Fonts\`, non-recursive, plus `[Rainmeter] LocalFont`, `LocalFont2..N`, each tried first under the program `Fonts\` folder then relative to the skin) (`L/Skin.cpp:2680-2756`). Unknown family: DirectWrite falls back to a system font; then the GDI-compat metrics below are 0 (`m_ExtraHeight = m_LineGap = 0`, because `ResolveFont` fails, `G/TextFormat.cpp:593-594,343-344`).
- `FontSize` default 10 (float; negative -> 10). **em size in px = FontSize * 4/3** (points at 96 DPI) (`G/TextFormat.cpp:524`). Pango: `pango_font_description_set_absolute_size(fd, FontSize*4/3*PANGO_SCALE)`.
- `StringStyle`: `Normal|Bold|Italic|BoldItalic` (default Normal) -> weight 700 / italic.
- `FontWeight`: 1..999 accepted (`L/MeterStringBase.cpp:535-546`), applied to the whole layout and overriding StringStyle's weight (`G/TextFormat.cpp:434-440`).
- `FontColor` default **0,0,0,255** (black). `FontEffectColor` default 0,0,0,255.
- `StringCase`: `None|Upper|Lower|Proper` via `LCMapString` UPPERCASE/LOWERCASE/TITLECASE (locale aware; Proper = first letter of each word upper-cased, rest lower-cased per Windows title case; verify) (`C/StringUtil.cpp:93-106`).

### 2.4 StringAlign (`L/MeterStringBase.cpp:495-529`)
Parse: consume `LEFT|RIGHT|CENTER|JUSTIFY` as a prefix (case-insensitive), then the **rest** must be exactly `BOTTOM` or `CENTER` for vertical, anything else (incl. `TOP` or nothing) = top. Unrecognised horizontal prefix leaves horizontal unchanged (initially Left) and sets vertical top. Values: `Left, Center, Right, LeftTop, CenterTop, RightTop, LeftCenter, CenterCenter, RightCenter, LeftBottom, CenterBottom, RightBottom` (+ `Justify*`, which wraps lines and justifies; starts at the left edge).

### 2.5 Measuring (W/H) (`L/MeterStringBase.cpp:687-834`, `G/TextFormat.cpp:632-706`)
DirectWrite model used by Rainmeter (Snowmeter must reproduce it, Pango's own line metrics differ):
- Font metrics in px: `a` ascent, `d` descent, `g` lineGap, from DirectWrite `DWRITE_FONT_METRICS` scaled by `em/unitsPerEm`. (DirectWrite behavior, not in this tree; verify: for fonts without the OS/2 USE_TYPO_METRICS bit, `a = usWinAscent`, `d = usWinDescent`, `g = max(0, hhea.lineGap - ((usWinAscent+usWinDescent) - (hhea.ascender - hhea.descender)))`; with the bit, typo ascender/descender/lineGap. Arial/Liberation Sans: 1854/434/67 of 2048.)
- Every line is `a + d + g` tall with the gap **above** the ascent (Rainmeter shifts drawing up by `g`, 2.6).
- `textW` = widest line **excluding trailing whitespace** (DWRITE_TEXT_METRICS.width), unhinted natural advances incl. kerning; `textH = lines * (a+d+g)`. If `textW == 0` (empty or whitespace-only text) `textH := 0` (`G/TextFormat.cpp:697-702`). A single trailing `\n` (string length > 2) is ignored for measuring (`:636-641`). `hasNL` = original string contains `\n`.
- `AccurateText=0` (default; GDI+ emulation) -- note `[Rainmeter] SyntaxVersion=2` forces AccurateText=1 (`L/Skin.cpp:2556`):
  ```
  e  = em/8 - g                                  (m_ExtraHeight, G/TextFormat.cpp:603-606)
  layout gets extra letter spacing em/144 before AND after every character (=> +em/72 per char)
  W  = textW(with spacing) + em/3
  H  = textH + e + (hasNL ? g : 0)               (single line: a + d + em/8)
  ```
- `AccurateText=1`:
  ```
  W = ceil(textW);  H = ceil(textH) - (hasNL ? 0 : g)
  ```
- Meter size (`UpdateAutoSize`): `m_W = (int)W + (l+r)` if W not given; `m_H = (int)H + (t+b)` if H not given (truncation). If the format failed to initialize: W=H=1.

### 2.6 Placement and drawing (`L/MeterStringBase.cpp:380-420,836-880`, `G/Canvas.cpp:531-585,790-833`)
- Drawn rect left/top from the anchor (X,Y), integer math (`m_W/2` truncates):
  ```
  left = X            (Left/Justify)   X - m_W/2  (Center)   X - m_W   (Right)
  top  = Y            (Top)            Y - m_H/2  (Center)   Y - m_H   (Bottom)
  ```
- Text box `B` = padded rect `(left+l, top+t, left+m_W-r, top+m_H-b)`. A layout of width `B.w` and height `B.h` (+2 if clipping, `G/TextFormat.cpp:377-385`) is created; lines are aligned inside it (horizontal: leading/center/trailing ignoring trailing whitespace; vertical "paragraph" alignment near/center/far of the whole block of `lines*(a+d+g)`), then the layout origin is offset:
  ```
  originX = B.left + (AccurateText ? 0 : {Left,Justify: +em/6, Right: -em/6, Center: 0})
  originY = B.top - g - {Top: 0, Center: e/2, Bottom: e}         (in both AccurateText modes)
  baseline of line i (block top T) = T + i*(a+d+g) + g + a
  ```
- The GDI letter spacing (em/144 each side) is applied to the **drawn** layout only when `AccurateText=0 AND AntiAlias=1` (`G/Canvas.cpp:575-580`), but it is always included in the measurement when AccurateText=0. With AntiAlias=0 the drawn text is therefore narrower than W suggests.
- Drawing order: StringEffect copies first (FontEffectColor, inline colors not applied), then the text with FontColor and inline formatting:
  - `StringEffect=Shadow`: one copy at (+1,+1).
  - `StringEffect=Border`: copies at (0,+1), (+1,0), (0,-1), (-1,0) (`L/MeterStringBase.cpp:848-876`).
- Text AA: AntiAlias=0 -> bi-level glyphs; 1 -> grayscale (never ClearType).
- Suggested Cairo/Pango setup (not from source): 96 DPI resolution, `HINT_METRICS_OFF`, `pango_context_set_round_glyph_positions(FALSE)`, hint style none/slight, AA none/gray per AntiAlias; compute `a,d,g` from the font tables yourself and position lines manually (or with absolute line-height) instead of trusting Pango's line height.

### 2.7 ClipString, ClipStringW, ClipStringH, wrapping
- `ClipString` 0 (default), 1, 2; other values log an error and keep the previous mode (`L/MeterStringBase.cpp:461-481`). `ClipStringW/H` (default -1) are read only for mode 2.
- `ShouldClip = mode==1 || (mode==2 && (needsClipping || (W and H both given)))` (`:653-657`). When clipping: DirectWrite trimming with an **ellipsis, character granularity**, word wrapping ON, and the drawing is clipped (aliased) to the text box `B` (`G/TextFormat.cpp:708-729`, `G/Canvas.cpp:814-824`). When not clipping: no wrapping, no ellipsis, no clip (text can overflow the meter).
- If only one line fits and both first lines are trimmed, wrapping is turned off so the single line shows as much text as possible + "..." (`G/TextFormat.cpp:454-469`).
- Mode 0/1 measure the text unwrapped (so mode 1 with only W given gives a one-line height and an ellipsized line).
- Mode 2 auto sizing (`L/MeterStringBase.cpp:736-833`), with `R` = unwrapped measured rect:
  - W given: wrap at `B.w`; height = wrapped height **+ 1**; then capped by H (if given) or by ClipStringH; `needsClipping=true`.
  - only H given: if ClipStringW == -1: height = given H, width = unwrapped, needsClipping if text taller; else width = min(textW, ClipStringW) (needsClipping if larger), wrapped height + 1, then height = H.
  - neither: if ClipStringW == -1: unwrapped; if ClipStringH set and text taller, height = ClipStringH and needsClipping; else width = min(textW, ClipStringW), wrapped at that width, height = wrapped + 1, capped at ClipStringH.
  - "wrapped" height = `lines*(a+d+g)` (+GDI extras as in 2.5) measured with max width w, then +1.

### 2.8 Angle (`L/MeterString.cpp:136-161`)
`Angle` in **radians** (float). Rotation (degrees = Angle*180/3.14159265, clockwise positive) about `(cx, B.top)` where `cx = B.left | (B.left+B.right)/2 | B.right` for Left/Center/Right; vertical alignment does not move the pivot. Applied before the meter's TransformationMatrix. Window size ignores rotation.

### 2.9 InlineSetting / InlinePattern (`L/MeterStringBase.cpp:20-345,556-583`, `G/TextFormat.cpp`)
- Keys: `InlineSetting`, `InlineSetting2`, ... read until the first empty **or unparsable** one. Pattern: `InlinePattern`, `InlinePattern2`, ... default `.*`.
- Pattern = PCRE (pcre16, no flags: case-sensitive, `.` excludes `\n`) matched against the **final** string (after case, ZWSP insertion), UTF-16 offsets (`L/Pcre.h:28`). The literal pattern `.*` is special-cased to the whole string (crosses newlines; nothing on empty text). Global matching with `PCRE_NOTEMPTY`. If the match has capture groups, **each capture** (not the whole match) is a range; else the whole match. Rust: needs PCRE semantics (lookaround) -> `fancy-regex` or `pcre2`; convert UTF-16 offsets to UTF-8 for Pango attributes.
- Settings are applied in index order; later ones override earlier ones on overlapping ranges.
- Syntax: name, then `|`-separated args; empty args are skipped; non-numeric numbers read as 0:
  | Setting | Args | Effect |
  |---|---|---|
  | `None` | - | no-op |
  | `Face` | name | family (private collection if it has it, else system) |
  | `Size` | points | size `pts*4/3` px (<=0 -> 1e-6) |
  | `Color` | color | fill color of the range |
  | `Weight` | 1..999 (clamped) | font weight |
  | `Italic` / `Oblique` | - | style |
  | `Underline` / `Strikethrough` | - | decoration |
  | `Stretch` | 1..9 (clamped) | font stretch |
  | `Case` | Upper / Lower / Proper / Sentence | case of the range (Sentence: lower, then upper first letter after `.?!`) |
  | `CharacterSpacing` | leading, trailing, advanceWidth (`\|`-separated) | points (x4/3); `*`/missing = keep; leading required |
  | `Typography` | 4-char OpenType tag, param (default 1) | font feature |
  | `Shadow` | x, y, blur, color (`\|`-separated) | Gaussian shadow (stddev = blur) of the range drawn under the text at offset |
  | `GradientColor` / `GradientColor1` | angle, then stops `color;pos` (`\|`-separated) | linear gradient per hit-test rect of the range; stop 0 at `EdgePoint(angle)`, stop 1 at `EdgePoint(angle+180)` (reverse of SolidColor); `1` suffix = gamma 1.0 (linear-light) interpolation; one stop -> a transparent stop added at 1 (pos<0.5) or 0 |
- **There is no `Stroke` inline setting in this source.**

---------------------------------------------------------------------------------------------------

## 3. Image meter (`L/MeterImage.cpp`, `L/GeneralImage.cpp`, `G/Util/EffectStream.cpp`)

### 3.1 Choosing the file
- Static case (no measure bound and `DynamicVariables=0`): `ImageName` loaded once at Initialize (`L/MeterImage.cpp:26-44`).
- With a measure: if ImageName is empty -> name = measure 1 value; else ImageName with `%N` replaced; **if no `%N` was replaced, the name is measure 1's value and ImageName is ignored** (`:143-157`). Numbers are formatted with AutoScale off, scale 1, 0 decimals (`"%.0f"`).
- Each update (measure or DynamicVariables) the file's size+mtime are rechecked and the image reloaded if changed (`L/GeneralImage.cpp:357-361`).
- Path: `ImagePath` (default: legacy `Path` option when SyntaxVersion < 2) + name; relative paths are relative to the **skin's own folder** (folder of the .ini) (`L/Skin.cpp:6042-6058`).
- **Extension defaulting**: if there is no `.` after the last `\` of the full path, `.png` is appended (`L/GeneralImage.cpp:350-355`). `ImageName=foo` -> `foo.png`. Only `\` is considered a separator.
- `SystemImage:index,list` loads a shell icon (Windows only).
- Formats: whatever WIC decodes, frame 0 only, converted to premultiplied BGRA (PNG, JPEG, BMP, GIF, TIFF, ICO, JPEG-XR, DDS, + installed codecs like WebP/HEIF) (`G/Util/BitmapLoader.cpp:11-124,299-322`). Image pixel size = logical size (no DPI metadata scaling).

### 3.2 Size (`L/MeterImage.cpp:53-96`)
`iw, ih` = processed image size (after crop/rotate), or MaskImage size if a mask is loaded.
```
W and H given  : as given (+padding)
only W given   : m_H = Tile ? ih : m_W * ih / iw  (+ t+b)   -- integer division; m_W already includes padding
only H given   : m_W = Tile ? iw : m_H * iw / ih  (+ l+r)
neither        : m_W = iw + l+r; m_H = ih + t+b
image failed   : undefined axes = 0
```

### 3.3 Drawing (`L/MeterImage.cpp:177-330`)
Let `B` = padded rect, `dw,dh` its size. All bitmap scaling uses high-quality cubic interpolation (`G/Canvas.cpp:900`; Cairo: `CAIRO_FILTER_BEST`/GOOD).
1. `MaskImageName` loaded: the image is center-cropped to B's aspect ratio and drawn through the mask (mask scaled to B, image alpha multiplied by mask alpha). Mask has its own `Mask*` options.
2. Else if `dw==iw && dh==ih` and ScaleMargins all 0: 1:1 copy.
3. Else if `Tile=1`: tile from B's top-left, partial tiles cropped (`G/Canvas.cpp:904-928`).
4. Else if `PreserveAspectRatio` 1 (Fit) or 2 (Crop) **and both W and H given**:
   - Fit: scale to fit, centered (letterbox); Crop: center crop of the source to B's ratio, fill B.
   - If not both given: plain stretch (the meter already has the image's ratio). `PreserveAspectRatio`: 0 stretch (default), 1 fit, 2 crop, any other value = fit (`L/AspectRatioMode.h:12-24`). Ignored when Tile=1.
5. Else (stretch): 9-slice with `ScaleMargins=l,t,r,b` (literal ints): corners unscaled, edges stretched along one axis, center stretched; destination edges snapped to device pixels. All-zero margins = plain stretch to B.

### 3.4 Image processing options (`L/GeneralImage.cpp:170-283,444-520`)
Order (fixed): EXIF orientation -> crop -> Greyscale -> color matrix (tint) -> flip -> rotate.
- `UseExifOrientation=1`: orientations 2..8 = flipH, rot180, flipV, flipH+rot270, rot90, flipH+rot90, rot270 (`G/Util/EffectStream.cpp:96-108`).
- `ImageCrop=x,y,w,h[,origin]` (ints, formulas ok; fewer than 4 values = error, ignored). Origin 1 TL (default), 2 TR, 3 BR, 4 BL, 5 center; other origins -> 1 with an error. Crop rect = `(ox+x, oy+y, ox+x+w, oy+y+h)` where `(ox,oy)` = (0,0), (iw,0), (iw,ih), (0,ih), (iw/2,ih/2) of the **original** (pre-EXIF) bitmap size. Areas outside the image become transparent. Negative w or h -> crop ignored.
- `Greyscale=1`: `Y = 0.299R + 0.587G + 0.114B` into R,G,B; alpha kept.
- Color matrix (5x4, row-vector): `out = [R G B A 1] x M`, on **un-premultiplied** values in 0..1, result clamped. Defaults: identity with diagonal `(tint.r, tint.g, tint.b, alpha/255)` where `ImageTint` default 255,255,255,255 and `ImageAlpha` default `tint.a*255`, clamped 0..255. So **ImageTint multiplies each channel** (255,0,0 keeps only red). `ColorMatrix1..5` each `v1;v2;v3;v4;v5` (exactly 5 values, 5th ignored) replace row 1..5 (row 1 = input R contributions to out R,G,B,A ... row 5 = constant offsets); a given ColorMatrixN overrides the tint/alpha value of that row.
- `ImageFlip`: `None|Horizontal|Vertical|Both` about the image center.
- `ImageRotate` (degrees, clockwise): rotated about the center; new size `(|w cos|+|h sin|, |w sin|+|h cos|)` truncated to int; bilinear resampling, soft (AA) edges except multiples of 90 (`G/Util/EffectStream.cpp:37-67`). The meter auto-size uses the rotated size.
- Button disables ImageCrop and ImageRotate (other options still apply).
- The same option set (prefixed) exists for other images: `MaskImageCrop`, `MaskGreyscale`, ... (`L/MeterImage.cpp:10`), Histogram `Primary*/Secondary*/Both*` (`L/MeterHistogram.cpp:9-11`). Bar, Rotator, Button, Bitmap use the unprefixed names.

---------------------------------------------------------------------------------------------------

## 4. Bar (`L/MeterBar.cpp`)
Options: `MeasureName` (required), `BarColor` (default **0,128,0,255**), `BarImage` (+ image options of 3.4), `BarBorder` (int px, default 0), `BarOrientation` `Vertical` (default) | `Horizontal`, `Flip` (0).
- Value = measure relative value `clamp((v-min)/(max-min), 0..1)` (1.0 if max==min) (`:89-97`).
- BarImage loaded -> `m_W/m_H = image size + padding`, **W/H options ignored** (`:35-41`). Without image, W/H come from the options.
- Geometry with `B` = padded rect (w, h), `bw = BarBorder`, along the bar axis `size = clamp(len_axis - 2*bw) * value` as float, clamped to `[0, len_axis - 2*bw]` (`:110-115,170-173`).
- **Color bar** (no image): fills a rect of length `size` anchored at the bottom (vertical) / left (horizontal), or top / right with `Flip=1`. BarBorder only shortens the maximum length; the fill still touches the edge (`:154-166,212-224`). Fractional sizes are rasterized with the meter's AntiAlias mode.
- **Image bar**: draws 1:1 slices of the image: the visible part of the bar is the image region at the same position (the image is revealed, not scaled). With BarBorder > 0 the two border strips (first and last `bw` px of the image along the axis) are drawn as caps next to the visible region: e.g. vertical unflipped: top cap src `(0,0,w,bw)` at `y = bottom - size - 2bw`, bottom cap src `(0,h-bw,w,bw)` at `y = bottom - bw`, body src `(0, h-size-bw, w, size)` at `y = bottom - size - bw` (`:117-152`, horizontal `:175-210`).
- SolidColor/bevel (1.6) are drawn first, under the bar, over the whole meter.

## 5. Line (`L/MeterLine.cpp`)
Options: `MeasureName` (required), `MeasureName2..N`; `LineCount` (default 1); `LineColor`, `LineColor2..N` (default white); `Scale`, `Scale2..N` (float, default 1); `LineWidth` (default 1, min 1); `AutoScale` (0); `HorizontalLines` (0); `HorizontalLineColor` (default black; legacy `HorizontalColor`); `GraphStart` `Right` (default) | `Left`; `GraphOrientation` `Vertical` (default) | `Horizontal`; `Flip` (0); `LineJoin` `Miter|Bevel|Round|MiterOrBevel` (default Miter); `MiterLimit` (default 10); `TransformStroke` `Normal|Fixed` (`:84-172`).
- History: one ring buffer per line, length = `m_W` (vertical) or `m_H` (horizontal) **including padding** (`:29-71`). Each fired update writes `GetValue()` of measure i (i < LineCount) at `pos` and advances `pos` (`:186-206`), so after an update `pos` points at the **oldest** sample. Buffers reset when LineCount, size or orientation change.
- Range (`:213-263`): without AutoScale `max = max over bound measures of MaxValue`, `min = min of MinValue`. With AutoScale: `m = max over all lines and samples of value*ScaleN` (starting at 0), `max = 2; while max < m: max *= 2`; min as before. `range = max - min`.
- Vertical orientation (default), `B` = padded rect, `dw = (int)B.w`, `dh = (int)B.h`, `Hf = dh - 1`, `off = 0.55`:
  ```
  yv(v) = range == 0 ? Hf : range < 0 ? 0 : clamp((v - min) * ScaleN * Hf / range + off, off, Hf + off)
  y(v)  = B.top + (Flip ? yv : Hf - yv + 1)
  sample k (k = 0 oldest .. dw-1 newest) = buf[(pos + k) % dw]
  GraphStart=Right: polyline (B.left + k, y(sample k)), k = 0..dw-1   -> newest at x = B.left + dw - 1
  GraphStart=Left : points x = B.right, B.right-1 (both oldest), then sample k at x = B.right - k - 1 ...
                    newest at x = B.left (see :438-451 for the exact point list)
  ```
  Each line is one open path (fill transparent), stroked with LineColorN, width LineWidth, LineJoin, MiterLimit, flat caps, transform type per TransformStroke, using the meter's AntiAlias mode. Lines are drawn in index order (line 1 at the bottom).
- Horizontal orientation mirrors this on the other axis (`:318-387`): rows are time (oldest at top, or at the bottom with Flip), x from the value with `GraphStart=Left` growing from the left, default growing from the right: `x = B.left + (Left ? xv : Wf - xv + 1)`, `Wf = dw - 1`, `range<0 -> xv = 1`.
- `HorizontalLines=1` (`:270-297`): `maxLines = dh/4`; `p = 2; while p < maxLines: p *= 2`; `n = ((int)max % p) + 1`; for j in 0..n-1: `Y = B.bottom - ((j+1)*dh/(n+1)) - 0.5` (integer division inside), line from `(B.left, Y)` to `(B.right - 1, Y)`, width 1, HorizontalLineColor. Drawn before the graph lines.
- `Draw` does nothing if the buffer size <= 0 (no W).

## 6. RoundLine (`L/MeterRoundLine.cpp`)
Options: `MeasureName` (optional; none -> value 1.0), `LineWidth` (1), `LineLength` (20), `LineStart` (**-1**), `StartAngle` (0, radians), `RotationAngle` (6.2832, radians), `ValueRemainder` (uint, 0; legacy typo `ValueReminder`), `LineColor` (**black**), `Solid` (0), `ControlAngle` (1), `ControlStart` (0), `ControlLength` (0), `StartShift` (0), `LengthShift` (0) (`:41-59`).
- Value (`:61-86`): `ValueRemainder > 0 ? ((int64)v % R) / R : relative value`.
- Geometry (`:89-174`), all floats, `X,Y` = meter position, **no padding applied**:
  ```
  cx = X + m_W/2.0 ; cy = Y + m_H/2.0          (W/H default 0 -> center is X,Y)
  rS = LineStart  + (ControlStart  ? StartShift  * value : 0)
  rL = LineLength + (ControlLength ? LengthShift * value : 0)
  P(r, θ) = (cx + r cos θ, cy + r sin θ)       (θ radians; 0 = east/right, positive = clockwise on screen)
  ```
- Not solid: `θ = (ControlAngle ? RotationAngle*value : RotationAngle) + StartAngle`; a line `P(rS,θ)` -> `P(rL,θ)` stroked with LineColor, width LineWidth, flat caps, meter AntiAlias mode (LineWidth 0 draws nothing). Default LineStart=-1 means the line starts 1 px behind the center.
- Solid: filled annular sector, no stroke:
  ```
  θ  = clamp(RotationAngle * (ControlAngle ? value : 1), -2π, 2π) + StartAngle
  sweepDeg = degrees(clamp(RotationAngle * value, -2π, 2π))        (uses value even if ControlAngle=0)
  θ0 = StartAngle + 0.001
  path: P(rS,θ0) -> line P(rL,θ0) -> arc radius rL to P(rL,θ) -> line P(rS,θ) -> arc radius rS to P(rS,θ0), close
  outer arc direction: clockwise if sweepDeg > 0 or sweepDeg < -359.935, else counter-clockwise;
  inner arc: the opposite; large-arc flag = |sweepDeg| >= 180
  ```
  Arcs are SVG-style endpoint arcs: negative radii clamp to 0 (= straight segment) and too-small radii are scaled up (`G/Shape.cpp:153-162`). Fill mode alternate, meter AntiAlias mode.

## 7. Shape (`L/MeterShape.cpp`, `G/Shape.cpp`, `G/Canvas.cpp:1104-1140`)

### 7.1 Options and parsing
- `Shape`, `Shape2`, `Shape3`, ... read until the first empty one; a shape that fails to parse stops the list (`:151-174`). Each value is `Definition | Modifier | Modifier ...`; parts split on `|` and values on `,` ignoring delimiters inside parentheses (formulas allowed); empty parts skipped (`:14-33`).
- Coordinates are floats relative to the meter's padded top-left; shapes are drawn at `translate((int)(X + l), (int)(Y + t))` (`:229-240`), **always anti-aliased**.

### 7.2 Definitions
| Definition | Syntax | Notes |
|---|---|---|
| Rectangle | `Rectangle x, y, w, h [, rx [, ry]]` | `ry` defaults to `rx`; radii -> rounded rect |
| Ellipse | `Ellipse cx, cy, rx [, ry]` | center + radii |
| Line | `Line x1, y1, x2, y2` | open path |
| Arc | `Arc x1, y1, x2, y2 [, rx [, ry [, angle [, sweep [, size [, closed]]]]]]` | defaults: `rx = ry = dist/2`, angle 0 (ellipse x-rotation, degrees); `sweep` 0 = **clockwise** (default), nonzero = ccw; `size` 0 = small (default), nonzero = large; `closed` 0 = open (default); `*` keeps a default (`:336-373`) |
| Curve | `Curve x1, y1, x2, y2, cx1, cy1 [, closed]` (quadratic) or `Curve x1, y1, x2, y2, cx1, cy1, cx2, cy2 [, closed]` (cubic) | 7 values = quadratic + closed flag (`:374-417`) |
| Path | `Path OptionName` (even-odd fill) / `Path1 OptionName` (**nonzero/winding** fill) | definition in the option `OptionName` (`:418-441`) |
| Combine | `Combine ShapeN`, then parts `Union ShapeM`, `Xor ShapeK`, `Intersect ...`, `Exclude ...`, transforms (all `\|`-separated) | see 7.5 |

Path definition (`:962-1094`): `startX, startY | segment | segment ...`, segments: `LineTo x, y`; `ArcTo x, y [, rx [, ry [, angle [, sweep [, size]]]]]` (same defaults/flags as Arc, radius default = half distance from the current point); `CurveTo x, y, cx1, cy1 [, cx2, cy2]` (end point **first**, then control point(s); 2 control points = cubic); `SetNoStroke 0|1` and `SetRoundJoin 0|1` (apply to following segments: unstroked segments / forced round join); `ClosePath 0|1` (1 = closed; default open). Needs a start point and at least one segment; an unknown segment invalidates the shape. **There is no `SetWindingMode` keyword in this source; winding is chosen by `Path1`.**

### 7.3 Defaults
- Fill: **white** (255,255,255,255). Open Arc/Curve/Path get a transparent fill unless a `Fill` modifier follows (it is set before modifiers are parsed). Line has no area.
- Stroke: **black, width 1** with the default stroke style (flat caps, miter join, limit 10, solid) and NORMAL transform (scales with transforms) (`G/Canvas.cpp:1119-1137`, `G/Shape.cpp:683-696`).
- As soon as any stroke modifier is used the shape gets custom stroke data (default black, width 1) and its stroke style is created with `D2D1_STROKE_TRANSFORM_TYPE_FIXED` (`G/Shape.h:75`, `L/MeterShape.cpp:750`): **stroke width is then not scaled by Scale/Skew modifiers or the TransformationMatrix**.
- `StrokeWidth 0` disables the stroke entirely (until a later non-zero StrokeWidth).

### 7.4 Modifiers (`:556-861`)
- `Fill Color c`; `Fill LinearGradient Opt` / `LinearGradient1 Opt`; `Fill RadialGradient Opt` / `RadialGradient1 Opt` (the `1` variants interpolate in linear gamma 1.0; default gamma 2.2 = plain sRGB).
- `Stroke Color c` / `Stroke LinearGradient[1] Opt` / `Stroke RadialGradient[1] Opt`.
- `StrokeWidth w` (negative -> 0 with warning).
- `StrokeStartCap`, `StrokeEndCap`, `StrokeDashCap`: `Flat|Square|Round|Triangle` (invalid -> Flat). D2D has separate caps for start, end and dashes; Triangle = isoceles cap of height w/2 (needs manual emulation in Cairo).
- `StrokeLineJoin Miter|Bevel|Round|MiterOrBevel [, limit]` (limit default 10, negative -> 10). D2D's miter limit is relative to **half** the stroke width (Cairo's to the full width -> use `limit/2`; verify); D2D `Miter` clips an over-limit miter instead of beveling, `MiterOrBevel` bevels (= Cairo MITER).
- `StrokeDashes d1, d2, ...`: custom dash pattern **in units of the stroke width** (multiply by width for Cairo). `StrokeDashOffset o` (negative -> 0), also in stroke widths.
- `Offset dx, dy`; `Rotate deg [, ax, ay]`; `Scale sx, sy [, ax, ay]`; `Skew degX, degY [, ax, ay]`; `TransformOrder Rotate, Scale, Skew, Offset` (any order, no duplicates; missing ones appended in the default order Rotate, Scale, Skew, Offset).
- `Extend OptA, OptB, ...`: parses the modifiers stored in those options at this position (not recursive).
- Modifiers are applied left to right; later ones override earlier ones.

### 7.5 Transforms, gradients, combine (`G/Shape.cpp`)
- Shape matrix (`:202-250`): `bounds` = untransformed geometry widened by the stroke width (default style); `P0 = (bounds.left, bounds.top)`; default anchor for all three = `P0 + (bounds.w/2, bounds.h/2)` (bounds center); an explicit anchor is `P0 + (ax, ay)` (**relative to the shape's own bounds top-left**, not to the meter). `M = Π over TransformOrder of {Rotation(deg, anchor), Scale(sx,sy, anchor), Skew(degX, degY, anchor) [x' = x + y tan(degX), y' = y + x tan(degY) around anchor], Translation(dx,dy)}`, applied in that order to points. Rendering transform = `M * translate(X+l, Y+t) * TransformationMatrix`.
- Linear gradient (`:561-577`): over the shape's **untransformed** bounds (stroke-widened union fill), stop position 0 at `EdgePoint(angle)` and 1 at `EdgePoint(angle+180)` (EdgePoint from 1.6). Note: this is the reverse of SolidColor: angle 0 puts the first stop on the **right**. Brush lives in shape space (so it rotates with the shape). Gradient option: `angle | color ; pos | color ; pos ...` (angle normalized to [0,360)).
- Radial gradient (`:579-612`): option `cx, cy [, ox, oy [, rx, ry]] | color ; pos | ...`. Center = bounds center + (cx, cy); gradient origin = center + (ox, oy) (default 0,0); radii default to half the bounds size. D2D radial = ellipse (rx, ry) around center with focal point at origin (Cairo: scale the pattern matrix by rx/ry; circle0 = (origin, 0), circle1 = (center, r)).
- Stops: `color;position` both required, else that stop is zero-initialized (transparent black at 0). Exactly one stop -> a transparent stop is added at 1.0 (if pos < 0.5) else at 0.0. Extend mode clamp.
- Combine (`L/MeterShape.cpp:455-554`, processed after all shapes): the combined shape copies the parent's fill/stroke, bakes the parent's transform into its geometry, then for each `Mode ShapeN` combines with that shape's transformed geometry (D2D CombineWithGeometry: Union/Xor/Intersect/Exclude); transform modifiers inside the Combine option apply to the result. Every referenced shape (parent included) is marked combined and **not drawn by itself** and excluded from bounds. Self reference/invalid index = error. Cairo has no boolean path ops: flatten and use a polygon clipper (e.g. `i_overlay`/`geo`).

### 7.6 Bounds and hit testing
- Auto size (only recomputed when options are read): if W or H is missing, **both** `m_W = ceil(max right) + l+r`, `m_H = ceil(max bottom) + t+b` over non-combined shapes, where per shape the bounds = union of stroke-widened bounds (width, caps, joins, transformed by M) and fill bounds (transformed by M) (`G/Shape.cpp:252-276`). Left/top extents are ignored (negative coords don't enlarge).
- Hit test (`L/MeterShape.cpp:244-263`, `G/Shape.cpp:283-302`): point `(x - X, y - Y)` (padding **not** subtracted) is inside if it is in any non-combined shape's stroke or fill area (fill counts even when transparent) using matrix `TransformationMatrix * M`.

## 8. Rotator (`L/MeterRotator.cpp`)
Options: `ImageName` (+3.4 options), `OffsetX`, `OffsetY` (float, 0), `StartAngle` (0, radians), `RotationAngle` (2π, radians), `ValueRemainder` (int; legacy `ValueReminder`), `MeasureName` (optional).
- Value: `ValueRemainder > 0 ? ((int64)v % R)/R : relative value`; with no measure the value stays 0.
- No auto size: W/H come from the options (default 0) (`:49-76`).
- Drawing (`:99-135`): image drawn unscaled with transform
  ```
  cx = B.left + m_W/2.0 ; cy = B.top + m_H/2.0     (B = padded rect; m_W includes padding)
  deg = degrees(RotationAngle * value + StartAngle)
  p_screen = Rotate(deg) * (p_image - (OffsetX, OffsetY)) + (cx, cy)      [then TransformationMatrix]
  ```
  i.e. image pixel (OffsetX, OffsetY) sits on the meter center; positive angle = clockwise; cubic interpolation. Drawing is not clipped to the meter.

## 9. Button (`L/MeterButton.cpp`)
Options: `ButtonImage` (+ image options except ImageCrop/ImageRotate), `ButtonCommand` (action string), `MeasureName` (optional, unused for drawing).
- Frames (`:29-78`): 3 frames. If image `h > w` they are stacked **vertically** (frame h = h/3, integer); otherwise (including square images) **horizontally** (frame w = w/3). Frame 0 = normal, 1 = pressed, 2 = hover. Meter size = frame size + padding (W/H options ignored). Drawn to `(B.left, B.top, B.left + m_W, B.top + m_H)` (padding stretches the frame).
- State machine (`:197-272`, driven by `Skin::HandleButtons`, `L/Skin.cpp:3841-3913`, and focus from `DoMoveAction`, `L/Skin.cpp:5547-5564`): only the topmost button under the cursor has focus. `HitTest2` = mouse over && inside rect && (SolidColor/SolidColor2 non-transparent || no image || pixel of the current frame has alpha > 0).
  - move without click: hit -> HOVER, leave -> NORMAL.
  - left down with focus and hit -> DOWN, clicked.
  - while clicked: moving off -> NORMAL (button released elsewhere clears clicked), back on -> DOWN.
  - left up: if DOWN and clicked and focused and `HitTest2` at the release point -> **execute ButtonCommand**; state -> NORMAL.
- Normal meter mouse actions (LeftMouseUpAction etc.) also work on a Button.

## 10. Histogram and Bitmap (concise)

### 10.1 Histogram (`L/MeterHistogram.cpp`)
Options: `MeasureName` (required), `MeasureName2` (or legacy `SecondaryMeasureName`); `PrimaryColor` (0,128,0), `SecondaryColor` (255,0,0), `BothColor` (255,255,0); `PrimaryImage`, `SecondaryImage`, `BothImage` (+ `Primary*`, `Secondary*`, `Both*` image options; with a second measure all three images are required or none is used); `AutoScale`, `Flip`, `GraphStart` (Right|Left), `GraphOrientation` (Vertical|Horizontal).
- PrimaryImage sets the meter size (W/H options ignored). Buffer length = m_W (vertical) / m_H.
- Per update: store `GetValue()`, advance pos (oldest at pos). Max/min = measure Max/MinValue; with AutoScale: primary max = smallest power of two >= max sample (min 2); secondary max continues from the primary's running maximum (so it is >= the primary's) (`:272-317`).
- Vertical (`:458-520`): column `i` (0..dw-1) shows sample `buf[(i + pos) % dw]` at `x = B.left + i` (GraphStart=Right: oldest left, newest right) or at `x = B.left + dw-1-i` (GraphStart=Left: newest at left). Bar height `hp = clamp((int)(dh * (v-min)/range), 0, dh)` (range < 0 -> 0, range == 0 -> full). Bars are 1 px wide rects from the bottom (top with Flip). Two measures: the overlap `min(hp, hs)` drawn in BothColor/BothImage, the remainder of the taller one in its own color. With images the same pixel region of the image is copied (1:1).
- Horizontal: rows instead of columns; Flip reverses time (newest on top), GraphStart=Left anchors bars at the left, default at the right.

### 10.2 Bitmap (`L/MeterBitmap.cpp`)
Options: `BitmapImage` (+ image options except crop/rotate), `BitmapFrames` (1), `BitmapZeroFrame` (0), `BitmapExtend` (0), `BitmapDigits` (0), `BitmapSeparation` (0), `BitmapAlign` `Left|Center|Right`, `BitmapTransitionFrames` (0), `MeasureName`.
- Frames are stacked vertically if image `h > w`, else horizontally; frame size = dimension / BitmapFrames (integer). Meter size = one frame (+ digits*frame + (digits-1)*separation along the stacking axis when Extend), + padding; W/H options ignored.
- Not extended: value = relative value; `realFrames = Frames / (TransitionFrames+1)`; `frame = (int)(v * realFrames) * (TransitionFrames+1)`, or with ZeroFrame `v > 0 ? (int)(v * (realFrames-1)) * (T+1) : 0`. No clamp: at exactly v = 1.0 the source rect is past the image and **nothing is drawn** (`:391-449`; verify on Windows).
- Extended: value = `max(0, (int64)GetValue())`, drawn as digits in base `realFrames` (base 2 if Frames == 1), least significant first, each digit `w` wide plus `BitmapSeparation`; digit count = BitmapDigits or as many as needed. Left align: digits occupy `[X, X + n*(w+sep) - sep)`; Center: shifted left by `n*(w+sep)/2`; Right: digits end at `X - sep` (they extend to the left of X) (`:250-389`).
- Transitions animate between frames over `(T+1) * TransitionUpdate` ms (`[Rainmeter] TransitionUpdate`, default 100).

---------------------------------------------------------------------------------------------------

## 11. Not covered here
`Meter=Svg` and `Meter=TextEdit` exist in this tree (`L/MeterSvg.cpp`, `L/MeterTextEdit.cpp`); TextEdit shares 2.3-2.7 via `MeterStringBase`.

---------------------------------------------------------------------------------------------------

## Implementation checklist (ordered by impact on common skins)
1. Option reading: `MeterStyle`/`@Inherit` (last style wins), defined = present and non-empty, int truncation, `(`-formulas, case-insensitive enums with default fallback, D2D named-color defaults (Green 0,128,0; Gray 128,128,128).
2. Meter list in file order (incl. `@Include` placement); draw in order, dispatch mouse in reverse.
3. X/Y parsing with `r`/`R` suffix; relative chain = previous non-contained meter (hidden included, W/H = 0 when hidden); String anchor vs aligned rect (`GetX(true)`); container offsets and forced-TL first item.
4. Padding (`l,t,r,b` -> W += l+r, H += t+b; content rect inset) and per-type auto size (table 1.4), incl. "image overrides W/H" types.
5. Window size (1.11): max over non-contained meters of X+W (hidden count with W=0), BackgroundMargins, BackgroundMode 0/2/3/4, SkinWidth/Height; recompute only on the listed triggers unless DynamicWindowSize=1.
6. String text pipeline (2.1-2.2): prefix/text/postfix, `%N` (N descending), AutoScale/Scale/Percentual/decimals, ties-to-even formatting, StringCase, NBSP/ZWSP/CR handling.
7. Text metrics model (2.5-2.6): em = FontSize*4/3, DWrite-style a/d/g from font tables, gap above ascent, trailing-whitespace-free width, AccurateText 0/1 formulas, `(int)` truncation, GDI x offset em/6, y offset -g - {0, e/2, e}, effects (Shadow/Border), aliased text when AntiAlias=0.
8. StringAlign parsing and anchor math; Angle (radians, pivot rule).
9. SolidColor/SolidColor2/GradientAngle background (EdgePoint math) and BevelType.
10. Image meter: `.png` defaulting, path base = skin folder, measure-driven names, size rules (integer division), stretch/Tile/Fit/Crop/ScaleMargins, cubic scaling.
11. Image processing chain: crop (origins), Greyscale, tint/ColorMatrix on unpremultiplied pixels, flip, rotate (bounding-box size).
12. Mouse: per-meter actions with rect hit test, mouse over for all meters under cursor, input region = window pixels with alpha > 0.
13. Shape meter: definitions, modifiers, transforms with bounds-relative anchors, FIXED stroke transform once stroke is customised, dash units = stroke width, miter-limit conversion, gradients (reversed angle), auto bounds incl. stroke, always AA.
14. Bar, RoundLine, Rotator, Line (ring buffer, 0.55 offset, AutoScale power of two), Button state machine/frames.
15. ClipString 1/2 (ellipsis, wrapping, +1 wrapped height, ClipStringW/H), inline settings (PCRE ranges, captures), Container masking, TransformationMatrix.
16. Histogram, Bitmap (incl. Extend digits and transitions), tooltips, UpdateDivider/DynamicVariables re-read timing.

## Test vectors (unit-testable; "A" = previous meter)
Positioning
- A: `X=10 Y=20 W=30 H=40`; B: `X=5R Y=0r W=8 H=8` -> B at (45, 20). C after B: `X=0r Y=10R` -> (45, 38).
- A hidden (`X=10 W=30`); B `X=5R` -> B.X = 15. `X=(10*2)R` = relative BR offset 20; `X=10.9` -> 10; `X=-5r` -> prev.X - 5.
- Container K `X=100 Y=100 W=50 H=50`; I1 `Container=K X=5 Y=5` -> (105,105); I2 `Container=K X=0R Y=0r W=10` -> relative to I1; next non-contained N `X=0R` -> 150 (relative to K, I1/I2 skipped).
- String `X=100 StringAlign=Right`, m_W=50 -> rect left 50; next meter `X=0R` -> 150. Center with m_W=51 -> left 75 (51/2 = 25). `StringAlign=RightBottom`, m_H=16, Y=40 -> rect top 24.
- `StringAlign` parse: `centertop` -> (Center, Top); `RightCenter` -> (Right, Center); `LeftMiddle` -> (Left, Top); `Bottom` -> (horizontal unchanged = Left, Top).
- `Padding=2,3,4,5` -> width padding 6, height padding 8, content rect (X+2, Y+3, X+m_W-4, Y+m_H-5); `Padding=5` -> all 0.
- Window: meters (X=10,W=30) and hidden (X=500,W=40) -> window width 500; with `BackgroundMargins=0,0,10,0` -> 510.
- `MeterStyle=S1|S2`, both set `FontSize` -> S2's value; meter's own value beats both.

Number formatting (String, NumOfDecimals unset unless stated)
- AutoScale=1: 1536 -> `1.5 k`; 1000 -> `1000.0 ` (trailing space); 1048576 -> `1.0 M`; -2048 -> `-2048.0 `.
- AutoScale=2: 1000 -> `1.0 k`; 1500000 -> `1.5 M`. AutoScale=1k: 512 -> `0.5 k`. AutoScale=3 behaves as 1.
- AutoScale=1, NumOfDecimals=0: 2560 -> `2 k` (2.5 ties-to-even); 3584 -> `4 k` (3.5).
- AutoScale=0: value 50, `Scale=2` -> `25`; `Scale=2.0` -> `25.0`; `NumOfDecimals=2`, 3.14159 -> `3.14`.
- Percentual=1, Min 0 Max 200, value 50 -> `25`; value 300 -> `100`; Min==Max -> `100`.
- `Text=%1 / %2`, measures "a","b" -> `a / b`; `Text=%10` with 10 measures uses measure 10 (not "%1"+"0").
- Invalid MeasureName + `Text=%1` -> literal `%1`.

Text metrics, Arial or Liberation Sans (upem 2048, win 1854/434, hhea gap 67 -> a,d,g), FontSize=10: em = 13.3333, a = 12.0703, d = 2.8255, g = 0.4362, e = em/8 - g = 1.2305
- "Hello" (advances 4667 units, textW = 30.3841): AccurateText=1 -> W=31, H=15 (ceil(15.3320)-0.4362 = 15.56); AccurateText=0 -> W=35 (30.3841 + 5*em/72 + em/3 = 35.754), H=16 (a+d+em/8 = 16.5625).
- "100%" (5238 units, 34.1016): Accurate W=35 H=15; GDI W=39 H=16. FontSize=8 "Hello": Accurate 25x12, GDI 28x13. FontSize=12 "Hello": Accurate 37x18, GDI 42x19.
- "A\nB" height: Accurate ceil(2*15.3320) = 31; GDI 2*15.3320 + em/8 = 32.33 -> 32.
- "" or "   " -> textW 0, so no GDI extras and H forced to 0 -> meter 0x0 (+padding), both modes.
- Baseline, StringAlign=LeftTop, Y=0, either mode: originY = -g, baseline y = a = 12.0703. GDI LeftTop X=0: first glyph origin x = em/6 = 2.2222 (+em/144 when AntiAlias=1).

Geometry
- SolidColor gradient on (0,0)-(100,50): angle 0 -> start (0,25) end (100,25); angle 90 -> start (50,0) end (50,50); angle 45 -> start (12.5,-12.5) end (87.5,62.5).
- Shape `Rectangle 0,0,100,50` (default stroke 1) -> W=101 H=51; `| StrokeWidth 0` -> 100x50; `| StrokeWidth 4` -> 102x52. `Ellipse 50,50,50` -> 101x101. Only `W=200` given -> still auto 101x51.
- Shape `Rectangle 10,10,100,50 | Rotate 45`: anchor = widened bounds (9.5,9.5)-(110.5,60.5) center = (60,35); `Rotate 45,0,0` pivots at (9.5,9.5).
- Shape linear gradient angle 0 on `Rectangle 0,0,100,50 | StrokeWidth 0` (bounds (0,0)-(100,50)): stop 0 at (100,25), stop 1 at (0,25).
- RoundLine `X=0 Y=0 W=100 H=100 LineLength=50 LineStart=0`, value 0.25, defaults otherwise: θ = 1.5708 -> end (50,100) (points down). `StartAngle=-1.5708` -> end (100,50). Default LineStart=-1 -> start (50,49) for θ = π/2. No W/H -> center = (X,Y).
- Rotator image 100x20, `OffsetX=10 OffsetY=10 W=200 H=200 X=0 Y=0`, value 0.25: deg = 90; image (10,10) -> (100,100); image (110,10) -> (100,200).
- Bar vertical `H=100`, value 0.37, no border -> fill (X, Y+63, W, 37); `BarBorder=5` (color) -> size 33.3 drawn from the bottom edge.
- Line `H=50` (no padding), min 0, max 100: v=0 -> y = top + 49.45; v=100 -> y = top + 0.45; newest sample at x = left + W - 1 (GraphStart=Right). AutoScale with max sample 5 -> max 8; with max 1.5 -> 2.
- Button image 30x90 -> 3 frames 30x30 stacked vertically; 90x30 -> horizontal 30x30; 30x30 -> horizontal frames 10x30.
- Bitmap `BitmapFrames=10`, relative value 0.5 -> frame 5; 0.99 -> frame 9; 1.0 -> frame 10 (nothing drawn). ZeroFrame=1: 0 -> 0, 0.5 -> 4.

Image
- `ImageName=foo` -> `<skin folder>\foo.png`; `foo.jpg` unchanged; `dir.v2\foo` -> `dir.v2\foo.png`; `#@#a.b/foo` -> no `.png` (dot after last `\`).
- MeasureName bound, `ImageName=bg` (no `%`) -> file = measure value; `ImageName=img%1` with value 3 -> `img3.png`.
- Image 200x100, `W=50` -> H 25; with `Padding=5,5,5,5` -> m_W 60, m_H = 60*100/200 + 10 = 40.
- `ImageCrop=-50,0,50,50,2` on 200x100 -> source rect (150,0)-(200,50). `ImageCrop=-10,-10,50,50` -> 50x50 with transparent 10px top/left band.
- `ImageTint=255,128,0`, pixel (200,100,50,255) -> (200, 50.2, 0, 255); `ImageAlpha=128` -> alpha 128/255 (straight). Greyscale on (255,0,0) -> (76.2,76.2,76.2).
- `ImageRotate=90` on 200x100 -> 100x200; `ImageRotate=45` on 100x100 -> 141x141.
