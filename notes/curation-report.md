# Curation report (condensed) + controller rulings — 2026-10-12

Exact refs (tags, SHAs, asset URLs): `refs.txt` in this folder. Copied license files: `licenses/<repo>/`. Montages: `png/`. Staged downloads were deleted (re-download from refs.txt when needed).

## Verdicts (32 tested)
good 11 · partial 16 · broken 4 · not-applicable 1

| Skin | Repo @ ref | License (actual) | Verdict | Key notes |
|---|---|---|---|---|
| illustro | rainmeter/rainmeter @6b2950c `Build/Skins/illustro` (+ `Build/Layouts/illustro default`) | GPL-2.0 | good | Google config uses TextEdit/!TextEdit (fork) |
| STRS (Sam Toki) | SamToki/Sam-Toki-Rainmeter-Skin v6.04 @829316a | GPL-3.0 | good | 56/62 full; fonts Sarasa UI SC (OFL, very large), Font Awesome |
| DMeloper's Block HUD | d-meloper/dmelopers-block-hud v1.4.3 @f6a2eec | MIT (original content) + THIRD_PARTY_NOTICES | good | remove Mouse.dll; Galmuri OFL; FolderInfo/FileView/Mouse/RunCommand for some parts |
| SilverAzide Gadgets | SilverAzide/Gadgets v7.10.0 @bc23874 | CC BY-NC-SA 4.0 | good | group bangs needed for config sub-skins; All CPU Meter 0x0 |
| Wisp | FiiZzioN/Rainmeter.Wisp @e25b89c | CC BY-NC-SA 3.0 | good | drop GOTHIC.TTF (proprietary Century Gothic) |
| Enigma | kaelri/enigma @596bee0 (`Skins/Enigma`) | CC BY-NC-SA 3.0 (README) | partial | Calendar needs Lua 5.1 implicit `arg`; TimeStampFormat warning |
| ModernGadgets | raiguard/ModernGadgets v1.8.2 @c269b74 | skin CC BY-NC-SA 3.0, source MIT | partial | Calendar needs `arg`; many plugins (RunCommand, Process, Mouse, ActionTimer, UsageMonitor, CoreTemp); fonts IBM Plex/Weather Icons/FA Free |
| SystemFetch | Meti0X7CB/SystemFetch 2.5 @faa4f59 | GPL-3.0 | partial | FrostedGlass everywhere; CPU/GPU 0 |
| SysDash | marcopixel/SysDash 1.1.0 @3094972 | MIT | partial | InlineSetting formulas; Yahoo weather dead; fonts Uni Sans/UniNeue (no license) |
| Cleartext | redsaph/cleartext 6.0 @839a774 | CC0 | partial | needs WebNowPlaying/GPMDP; settings.lua uses `dofile` |
| rainmeter-todo | alperenozlu/rainmeter-todo 1.0.2 @dfdd38e | GPL-3.0 | partial | interaction untested |
| Monterey | creewick/MontereyRainmeter v1.0.2 @7103362 | CC BY-NC-SA 4.0, bundles Apple SF Pro (not covered) | partial | analog clock hands missing; FrostedGlass/Chameleon |
| Monstercat / Lano Visualizer | marcopixel (MIT) | MIT | partial | blank without AudioLevel |
| ModularClocks / Players / Visualizer | Jax-Core (MIT, need JaxCore MPL-2.0) | MIT | good / partial / broken | 27–33 fonts of unclear license |
| Blue Archive | Xenon257R v1.2.6 @008fb71 | MIT code, game art = Nexon IP | good | |
| Needy Streamer Overload | lezzthanthree v.5.2.1 | MIT code, game art IP | good | 207 MB |
| Persona 5 Calendar | Mive82 v1.1 @2c31c89 | WTFPL code, Atlus art IP | good | nested-ternary Calc warnings |
| NThing UI | Runixe786 v1.1-publicbeta @00317c2 | none | good | |
| BigSur | fediaFedia v1.4.5 @84c80e4 | none | good | EasyBlur sidebar blank |
| Mondtholomew | reisir v1.3.2 @8c2d843 | none | partial | clock excellent |
| Sonder | mpurses v22.1.0 @d52b699 | none | partial | date left-clipped; inline script call in bang |
| Notch | khanhas v1.4 @b6256e2 | none | partial | |
| catppuccin | modkavartini v1.4.1 @cb88052 | unclear (no LICENSE) | partial | |
| MD3 Windows | Runixe786 v0.5-PublicBeta @f7ecbb0 | none | partial | MeterialYou colours |
| DetailedSystemMonitor | Lssg97 v3.5 | CC BY-NC-SA 3.0 CN | partial | Chinese only |
| WinLock | Runixe786 | MIT | not-applicable | lock screen |
| SquarePlayer, ModularVisualizer, Fountain of Colors, Expressive Windows | — | — | broken | |

## Controller rulings
1. **Bundle in the repo (`Kind=folder`) only when** the license allows redistribution **and** every bundled font/image/binary is clearly redistributable (OFL/Apache/MIT/CC0/public domain or explicitly covered), **and** the skin is ≤ ~15 MB. Windows `.dll`/`.exe` are removed (they cannot run; only for non-ND licenses). Add `LICENSE` (upstream text) and `SOURCE.md` (author, repo@ref, license, what was removed) in the skin root.
2. **Otherwise `Kind=remote`**, pinned to the author's release asset or commit archive — the user downloads from the author. Applies to: no-license skins, skins with unclear/proprietary fonts or third-party game/Apple art, and large skins.
3. **Not offered (yet):** WinLock (not applicable), the 4 broken ones, audio visualizers (need AudioLevel), Jax-Core Modular* (need the JaxCore framework installer), Needy Streamer Overload (207 MB > size cap), DetailedSystemMonitor (Chinese-only, partial).
4. Lua: stock 4.5.27 embeds **Lua 5.1.5 with LUA_COMPAT_VARARG** (not LuaJIT; LuaJIT is only in the 5.0 dev fork). Library changes per `LuaModifications.txt`: base without collectgarbage (print → log), io without popen/std files, os without exit (setlocale no-op), no package lib. `dofile`/`loadfile` available.

## Snowmeter bugs found (ranked)
1. Lua implicit `arg` (needs Lua 5.1 compat) + `dofile` missing.
2. Group bangs missing: !RefreshGroup (49 cfgs), !DeactivateConfigGroup, !RedrawGroup, !UpdateGroup, !Show/Hide/ToggleFadeGroup, !ZPosGroup, !ShowGroup/!HideGroup.
3. Formulas inside InlineSetting values not evaluated (`Size | (68*#Scale#)`).
4. Missing plugins by impact: RunCommand 53, FolderInfo/FileView 42, ActionTimer 35, AudioLevel 27, Mouse 25, UsageMonitor 24, Process 23, WebNowPlaying 22, ConfigActive 16; SysInfo USER_LOGONTIME wrong ("155512d"); cosmetic plugins (EasyBlur 136, FrostedGlass 26, Chameleon 20, SysColor, MeterialYou 32) should be no-ops/fallbacks, not "unsupported".
5. Inline script call inside a bang argument not evaluated: `[&MeasureScript:FormatNumber([&M],3,'1k')]` (Sonder).
6. Calc nested/chained ternaries: `(#V#=2 ? ([x]<3 ? 1 : 0) : #V#)` → "Logical expression error" (Persona 5 Calendar).
7. TimeStampFormat `%Y-%m-%dT%H:%M:%S` reported invalid (Enigma); WebParser URLs with spaces/quotes rejected (Rainmeter percent-encodes).
8. Sonder Date string left-clipped; Monterey analog clock shows only the second hand.
9. IPC socket shared per user even with SNOWMETER_CONFIG_DIR (test commands reach the real instance).
10. Compat noise: template Item*.ini / Disabled or Hidden references counted as partial; dynamic-node skins render 0x0.
11. `!WriteKeyValue` file arguments with spaces split.
12. Folder import should detect a root containing `@Resources` one level down (Enigma `Skins/Enigma`).
