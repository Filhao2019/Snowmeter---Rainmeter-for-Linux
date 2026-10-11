# .rmskin packages, Rainmeter.ini, layouts and fonts

Research notes for Snowmeter's importer. Everything here was read from the Rainmeter C++ source
in `rainmeter-src/` (paths below are relative to it) and checked against the Windows install in
`~/.wine/drive_c/Program Files/Rainmeter/` where possible.

**Versions.** The source tree is a development build (`Version.h` holds placeholder `0.0.0`; it
needs Windows 10 1903+, `Library/Rainmeter.cpp:206-210`). The Wine install is **Rainmeter
4.5.27.3934 (64-bit)** (version resource of `Rainmeter.dll`). The skin installer lives in
`Rainmeter.dll` (`SkinInstaller.exe` only calls `SkinInstallerMain`,
`SkinInstaller/SkinInstallerLauncher.cpp:41-49`). The 4.5.27 DLL contains the same key strings
(`RMSKIN.ini`, `Rainstaller.cfg`, `RMSKIN.bmp`, `MergeSkins`, `VariableFiles`, `MinimumRainmeter`,
`MinimumWindows`, `LoadType`, `ArchivePlugins`, `@Vault`, `64bit`, `Wallpaper.bmp`), so the
package format below applies to both. Where the dev source adds newer settings (for example
`Zoom`), this is noted.

There is no `Rainmeter.ini` under `~/.wine/drive_c/users/bernardo/AppData/Roaming/` because
Rainmeter was never started in that prefix. The default settings file is the layout
`Defaults/Layouts/illustro default/Rainmeter.ini`, which is identical to `sample-skins/Layouts/...`
(checked with `diff -r`).

---

## 1. Binary format of a `.rmskin`

A `.rmskin` is an ordinary ZIP archive (written with minizip, Deflate, UTF-8 names) followed by a
**16-byte footer**.

### 1.1 Footer structure

```c
// Library/DialogInstall.h:71-81
enum PackageFlag { Backup = 0x0001 };
struct PackageFooter
{
    __int64 size;   // offset 0, 8 bytes, little-endian signed
    BYTE    flags;  // offset 8, 1 byte
    char    key[7]; // offset 9, 7 bytes: "RMSKIN\0"
};                  // sizeof == 16 (8+1+7, no padding on x86 or x64 MSVC)
```

Byte layout at the end of the file (`L` = total file length in bytes):

| Absolute offset | Length | Field | Value written by the Skin Packager |
|---|---|---|---|
| `L-16` .. `L-9` | 8 | `size`, int64 **little-endian** | `L - 16`, the length of the ZIP data before the footer |
| `L-8` | 1 | `flags` | `0x00` |
| `L-7` .. `L-1` | 7 | `key` | `52 4D 53 4B 49 4E 00` (`"RMSKIN"` plus a NUL) |

The writer is `Library/DialogPackage.cpp:671-679`. It closes the ZIP, seeks to the end, and
writes `{ _ftelli64(file), 0, "RMSKIN" }`, so `size` is the ZIP length and `flags` is always 0.

Worked example (built in memory with Python, a 270-byte ZIP):
`... <zip bytes> 0e 01 00 00 00 00 00 00 | 00 | 52 4d 53 4b 49 4e 00` (`0x10e` = 270, total 286).
Python's `zipfile` reads that file with the footer still attached. The Rust `zip` crate
(9.0.1 in `~/.cargo/registry`) also tolerates data after the EOCD: it searches backwards and only
requires the comment to end before EOF (`zip-9.0.1/src/format/find_central_directory.rs:35-56`).
Snowmeter should still hand the ZIP reader a reader limited to the first `size` bytes.

### 1.2 How the installer validates it (`DialogInstall::ReadPackage`, `Library/DialogInstall.cpp:352-570`)

1. **Extension decides the path** (`:355-389`, case-insensitive):
   * `.rmskin`: read the last 16 bytes (`fseek(-16, SEEK_END)`, `fileSize = ftell` = `L-16`).
     * If `strcmp(footer.key, "RMSKIN") == 0`, the package is **New format**. `key[6]` must be
       `0x00`, because `strcmp` is used.
       * If `footer.size != L-16`, it is **rejected** (`:375-378`). Appending, truncating or
         re-zipping invalidates the package.
       * If `flags != 0`, `m_BackupPackage = !(flags & Backup)` (`:380-383`). So `flags == 0`
         gives a normal package, `flags == 1` also gives a normal package, and any non-zero value
         with bit 0 clear gives a "backup package" (see §3.4). The packager always writes 0.
     * If there is no matching key, the format stays **Old** (the constructor default,
       `:39`).
   * `.zip`: always **Old** format, and no footer check is made.
   * Any other extension: rejected (`:386-389`).
2. The archive is opened with minizip `unzOpen2_64` (`:391-397`), which finds the central
   directory from the end, so the 16-byte footer does not get in its way.
3. **First pass: find the options file** (`:424-466`).
   * New format: `RMSKIN.ini` must be at the **archive root**. Entries inside any folder are
     skipped (`:441-445`). The name match is case-insensitive.
   * Old format: `Rainstaller.cfg` at **any depth**. Its folder becomes `m_PackageRoot`, and
     every component path is taken relative to that folder (`:471`, `:480-484`).
   * The file is extracted to a temp file and parsed (`ReadOptions`). If it is not found, or the
     options are invalid, the package is rejected.
4. **Second pass: collect components** (`:468-560`, see §3). The package must contain at least
   one skin, layout, addon, font or plugin, otherwise it is rejected (`:568-569`).
5. Error text for a rejected package (`:97-102`, `Language/en.ini:282-283`):
   *"Invalid package: <file>. The Skin Packager tool must be used to create valid .rmskin
   packages."* Version errors use their own messages (§2).

### 1.3 A plain ZIP without the footer

* Named `*.zip`, or named `*.rmskin` but missing the footer: Rainmeter handles it as an **old
  (Rainstaller) package**. It looks for `Rainstaller.cfg` with a `[Rainstaller]` section, at any
  depth. A ZIP that only has `RMSKIN.ini` is therefore **rejected as invalid**, even when its
  contents are otherwise perfect. Renaming a `.zip` to `.rmskin` does not work in real Rainmeter.
* A `.rmskin` whose footer `size` does not match the file is rejected outright. It is **not**
  retried as the old format.
* Snowmeter should be more lenient (§7): accept a footerless `RMSKIN.ini` package with a warning.

### 1.4 Old "Rainstaller" format (for completeness)

The same code path is used with different names (`DialogInstall.cpp:450, 495, 522-533, 552-557,
575-630, 791-828`):

| Old | New equivalent |
|---|---|
| `Rainstaller.cfg`, section `[Rainstaller]` | `RMSKIN.ini`, section `[rmskin]` |
| `Rainstaller.bmp` | `RMSKIN.bmp` |
| `Merge` | `MergeSkins` |
| `KeepVar` | `VariableFiles` |
| `MinRainmeterVer` | `MinimumRainmeter` |
| `LaunchType` (`Load` means skins, anything else means layout) | `LoadType` |
| `LaunchCommand` (`|`-separated list of `Config\File.ini`) | `Load` (a single value) |
| `Themes\<name>\*.thm`, installed as `Layouts\<name>\*.ini` | `Layouts\<name>\*.ini` |
| `Addons\<name>\...` installed to `<Settings>\Addons\` | (none) |
| `Fonts\*.ttf` copied into every packaged skin's `@Resources\Fonts\` | (none; fonts ship inside the skin) |
| Backup package if `Name` matches `Backup-%d.%d.%d-%d.%d.rmskin` (`:581-587`) | footer `flags` |

---

## 2. `RMSKIN.ini` `[rmskin]` keys

`ReadOptions`, `Library/DialogInstall.cpp:572-643`. The file is read with Rainmeter's `IniFile`
reader, which accepts UTF-16LE (with or without a BOM) and UTF-8 (with or without a BOM), and
falls back to the ANSI code page (`Common/IniFile.cpp:21-104`). UTF-16BE is refused. Section and
key names are matched case-insensitively (`EqualsNoCase`, `Common/IniFile.cpp:197`), so `[RMSKIN]`
works too.

Error texts (`Language/en.ini:289-291`): *"Rainmeter %1 or higher is required to install this
package."*, *"Your version of Windows is not supported by this package."*, *"Unable to move
to: %1"*.

| Key | Meaning | Default when absent | Source |
|---|---|---|---|
| `Name` | Package name shown in the dialog. **Required**: an empty value makes the package invalid. | (invalid) | `:578-579` |
| `Author` | Shown in the dialog. | `""` | `:589` |
| `Version` | Shown in the dialog. Not compared with anything. | `""` | `:590` |
| `MergeSkins` | Integer. Non-zero means *merge* (overlay) instead of *backup and replace*. See §3.3. | `0` | `:592` |
| `VariableFiles` | `|`-separated list of files, relative to the Skins folder (e.g. `illustro\@Resources\Variables.inc`). Each entry is trimmed of spaces and tabs. The user's current `[Variables]` values are kept (§3.5). The packager only writes it when `MergeSkins` is off (`DialogPackage.cpp:583-594`). | `""` | `:594-595` |
| `MinimumRainmeter` | Dotted version. If it is greater than the `FileVersion` of `Rainmeter.dll` (e.g. `4.5.27.3934`), installation is refused with "*Rainmeter %1 or higher is required*". The packager defaults it to the current `major.minor.patch.revision` (`DialogPackage.cpp:99-104, 1746-1748`). | no check | `:597-607` |
| `MinimumWindows` | Dotted version compared with `major.minor.build` from `GetVersionEx` (`:1320-1328`). If it is greater, installation is refused. The dev-source packager no longer writes this key (`DialogPackage.cpp:572-597`), but older packages may have it. | no check | `:632-640` |
| `LoadType` | Case-insensitive `Skin` means activate a skin after install. **Any other non-empty value** (the packager writes `Layout`) means load a layout. If empty, nothing is loaded. | `""` | `:609-630` |
| `Load` | With `LoadType=Skin`: one `Config\Path\File.ini` relative to the Skins folder, e.g. `illustro\Clock\Clock.ini`. With a layout: the layout folder name, e.g. `illustro default`. | `""` | `:614-629` |

**Version comparison** (`CompareVersions`, `:1234-1262`): split on `.`, `_wtoi` each part
(non-numeric parts count as 0), and treat missing parts as 0. So `4.5` equals `4.5.0.0`.

**There are no `Plugins`, `Fonts`, `Layouts` or `Skins` keys.** Components are found only from
folder names in the archive (§3). The packager remembers its own choices (layouts, plugin paths,
header image, output folder) in `<Settings>\SkinPackager.ini` (`DialogPackage.cpp:41-46,
493-556`), and that file is not part of the package.

Comparison of `Name`, `Author` and `Version` with an installed copy is not done anywhere.

### `RMSKIN.bmp`

* Optional, at the **archive root** (`DialogInstall.cpp:493-504`). The name match is
  case-insensitive.
* It is loaded with `LoadImage(IMAGE_BITMAP, LR_LOADFROMFILE)`, so it must be a Windows **BMP**.
* The packager only accepts a 400x60 bitmap (`DialogPackage.cpp:2010-2026`).
* The installer scales it into a 266x37 dialog-unit header strip, and the dialog grows from 213
  to 250 DLUs when a header is present (`DialogInstall.cpp:111, 152-157, 198-221`).
* Snowmeter can show it in its install dialog with gdk-pixbuf's BMP loader. It is never
  installed to disk.

---

## 3. Archive layout and how each part is installed

Paths in the archive are decoded as **UTF-8 if general-purpose flag bit 11 is set, otherwise
with the ANSI code page** (Windows-1252 on Western systems). `/` is converted to `\`
(`DialogInstall.cpp:408-422`). The packager always writes `/` with the UTF-8 flag
(`DialogPackage.cpp:724-747`) and, unless `MinimumRainmeter >= 3.0.1`, refuses non-ASCII names
(`:599-600`). Top-level component names are matched case-insensitively. Rainmeter does **not**
reject `..` components; Snowmeter must.

### 3.1 What each path means

`<SkinPath>` is the user's Skins folder (default `%USERPROFILE%\Documents\Rainmeter\Skins\`,
`Rainmeter.cpp:459-479`, or the `SkinPath` key in Rainmeter.ini). `<Settings>` is
`%APPDATA%\Rainmeter\`, or the program folder for a portable install. The installer reads both
the same way Rainmeter does (`SkinInstaller.cpp:48-99`).

| Archive path | Recognised when | Installed to | Source |
|---|---|---|---|
| `RMSKIN.ini` | at the root | not installed (read from a temp file) | `:424-466` |
| `RMSKIN.bmp` | at the root | not installed | `:493-504` |
| `Skins\<Root>\...` | `<Root>` is not `Backup`, `@Backup` or `@Vault` (case-insensitive, `:1132-1142`). The entry needs at least `Skins\<Root>\<something>`. A file directly in `Skins\` is ignored. | `<SkinPath>\<Root>\...`, keeping the full sub-path | `:517-521`, `:760-764`, `:786-790` |
| `Layouts\<Name>\...*.ini` | extension `.ini`; `<Name>` is not `Backup` or `@Backup` | `<Settings>\Layouts\<Name>\...`, then the `[Rainmeter]` section of each extracted file is **emptied** (`CleanLayoutFile`, `:1112-1116`) | `:522-527`, `:791-808` |
| `Layouts\<Name>\Wallpaper.bmp` or any non-`.ini` file | not recognised | **ignored**, so a layout wallpaper is never installed from a package | same |
| `Plugins\64bit\<X>.dll` (64-bit Rainmeter) or `Plugins\32bit\<X>.dll` (32-bit) | the DLL sits directly in the arch folder (`32bit`/`64bit` compared case-insensitively), extension `.dll`, and `<X>` is not a built-in plugin (list below) | `<Settings>\Plugins\<X>.dll` | `:534-544`, `:772-784` |
| `Plugins\<anyFolder>\<X>.dll` | `ArchivePlugins` in `Rainmeter.data [SkinInstaller]` is on (default `1`, `:46-48`). This runs for **every** DLL one level under `Plugins`, built-in names and unchecked items included. | copied to `<SkinPath>\@Vault\Plugins\<X without .dll>\<FileVersion>\<anyFolder>\<X>.dll`. The `<FileVersion>\` level is left out if the DLL has no version resource. | `:750-758`, `:987-1052` |
| `Fonts\*.ttf` | **old format only** | each packaged skin's `@Resources\Fonts\` | `:552-557`, `:812-828` |
| `Addons\<Name>\...` | **old format only** | `<Settings>\Addons\...` | `:528-533`, `:765-771` |
| anything else (README, `Skins\file.ini`, top-level folders, `Fonts\` in new format) | | **ignored** | |

Plugins that the installer ignores because the engine has them built in
(`IsIgnoredPlugin`, `:1167-1199`): `ActionTimer`, `AdvancedCPU`, `AudioLevel`, `CoreTemp`,
`FileView`, `FolderInfo`, `InputText`, `iTunesPlugin`, `MediaKey`, `NowPlaying`, `PerfMon`,
`PingPlugin`, `PowerPlugin`, `Process`, `QuotePlugin`, `RecycleManager`, `ResMon`, `RunCommand`,
`SpeedFanPlugin`, `SysInfo`, `UsageMonitor`, `WebParser`, `WifiStatus`, `Win7AudioPlugin`,
`WindowMessagePlugin` (all `.dll`).

The dev engine also rewrites `Measure=Plugin` / `Plugin=<name or path>` to the built-in measure
when the file name without extension matches `GetOldDefaultPlugins()`
(`Skin.cpp:2809-2846`, `Rainmeter.cpp:2029-2063`). The list also includes `Drag&Drop` and
`Mouse`, and some names are renamed: `iTunesPlugin`→`iTunes`, `PingPlugin`→`Ping`,
`PowerPlugin`→`Power`, `QuotePlugin`→`Quote`, `SpeedFanPlugin`→`SpeedFan`,
`Win7AudioPlugin`→`Audio`, `WindowMessagePlugin`→`WindowMessage`, `Drag&Drop`→`DragDrop`.
Rainmeter 4.5.27 still ships these as real DLLs in `Program Files\Rainmeter\Plugins\` (18 DLLs
there). Snowmeter should use the same name-to-native mapping.

What the packager writes (`DialogPackage.cpp:565-693`):
* `RMSKIN.bmp` (optional) and `RMSKIN.ini`.
* **exactly one** `Skins\<Root>\` tree (`m_SkinFolder` is a single pair). Hidden files are
  skipped (`:821-825`) and empty folders become directory entries (`:858-864`).
* `Layouts\<name>\Rainmeter.ini` for each chosen layout. It only packs `Rainmeter.ini`.
* `Plugins\32bit\<X>.dll` **and** `Plugins\64bit\<X>.dll` for each plugin. Both are required
  (`:1243`).

The installer itself accepts several roots and several layouts.

### 3.2 Install sequence

`InstallThread`, `DialogInstall.cpp:907-947`:

1. The user can untick components, and can untick "Apply included layout" / "Load included
   skins" (ticked by default, `:1545-1558`). Unticked items are removed from the sets, and an
   unticked load box clears the load action (`:860-896`). A plugin whose installed copy has the
   same or a newer version resource starts **unticked** (`IsPluginNewer`, `:1508-1522`,
   `:1330-1397`).
2. Rainmeter is closed (`CloseRainmeterIfActive`, `SkinInstaller.cpp:189-212`, waits up to 5 s).
3. `InstallPackage()` (`:645-847`):
   1. Backup or delete existing roots (§3.3).
   2. A single pass over the archive extracts every recognised entry with
      `CreateFile(CREATE_ALWAYS)`, which **overwrites** files (`:308-350`). Missing directories
      are created recursively.
   3. `KeepVariables()` runs if `!MergeSkins && BackupSkins` (`:841-844`, §3.5).
4. Rainmeter is relaunched with the load action (`LaunchRainmeter`, `:1054-1110`, §3.6). If
   installation failed, the load action is cleared (`:938-939`), but Rainmeter is still
   relaunched.

### 3.3 Overwrite, backup (`@Backup`) and `MergeSkins`

The backup phase is `DialogInstall.cpp:647-692`. `m_BackupSkins` defaults to `true` (`:41`) and
can only be turned off in the "Advanced" menu. That menu item is greyed out when the package has
no skins, has `MergeSkins`, or is a backup package (`:234-241`, `:271-273`).

| Situation | What happens to an existing `<SkinPath>\<Root>` |
|---|---|
| Default: `MergeSkins=0` and "Backup skins" ticked | The old `<SkinPath>\@Backup\<Root>` is deleted (to the Recycle Bin, `FOF_ALLOWUNDO`). Then `<SkinPath>\<Root>` is **moved** to `<SkinPath>\@Backup\<Root>` (`System::CopyFiles(from, to, true)` is `FO_MOVE`, `System.cpp:934-963`), and the package is extracted into a now-empty `<Root>`. The result is a **clean replace**. Files the user added only exist in `@Backup`. If the move fails, the install fails with "Unable to move to ...". |
| `MergeSkins=1` | No backup and no delete. Package files overwrite same-named files, and all other existing files stay. **`VariableFiles` is not applied**, so packaged `.inc` files overwrite the user's values. |
| `MergeSkins=0`, "Backup skins" unticked | Same as merge: an in-place overlay, and no `KeepVariables`. |
| Backup package (footer flags, or old-format name pattern) | The current `<Root>` is **deleted** (to the Recycle Bin) and the package is extracted. `KeepVariables` still runs (`m_BackupSkins` is still true), but only against whatever stale `@Backup` copy exists. |

`@Backup`, `Backup` and `@Vault` at the top of the Skins folder are never shown as skins
(`SkinRegistry.cpp:9-13, 250-251`). At startup an old `Skins\Backup` is renamed to `@Backup`
(`Rainmeter.cpp:983-991`), and `@Vault\Plugins\` is created (`:994-1000`).

Layouts and plugins are always overwritten in place, with no backup.

### 3.4 Roots in the archive vs on disk

`m_PackageSkins` is a `std::set<std::wstring>`, so it is **case-sensitive**. NTFS is not, so
`Skins\Foo\` and `Skins\foo\` from the same archive land in one folder on Windows. On Linux they
would not, so Snowmeter must canonicalise (§7).

### 3.5 `VariableFiles` (keeping the user's variable values)

`KeepVariables`, `DialogInstall.cpp:949-985`. For each file `F` in the list:

1. `from = <SkinPath>\@Backup\F` (the user's old copy, just moved there) and
   `to = <SkinPath>\F` (the newly extracted copy). **Both must exist.**
2. Read `[Variables]` from both. If either is empty, skip the file.
3. For every key present in **both** files, write the **old value** into the new file. Keys that
   only exist in the old file are dropped. New keys keep the package's default. The key match
   uses `StringMap`, which is **case-sensitive** (`Common/Map.h:22`).
4. Only `[Variables]` is touched. All other sections and the order and comments of the new file
   are kept (it is an in-place key write through `IniFile::Writer`).

This only runs in the default mode (`!MergeSkins && BackupSkins`), because it reads from
`@Backup`.

### 3.6 Load action after install

`LaunchRainmeter`, `DialogInstall.cpp:1054-1110`:

* **Layout** (`LoadType` other than `Skin`): Rainmeter is started as
  `Rainmeter.exe !LoadLayout "<Load>"`. Rainmeter accepts only that bang on its command line when
  it is not already running (`Rainmeter.cpp:101-124`) and calls `LoadLayout()` during startup
  (`:580-590`). See §4.4.
* **Skin** (`LoadType=Skin`): Rainmeter is started normally, the installer waits for input-idle
  plus 500 ms, then runs it again with `[!ActivateConfig "<Load up to the last \>" "<after the
  last \>"]`. For example `illustro\Clock\Clock.ini` becomes
  `!ActivateConfig "illustro\Clock" "Clock.ini"`. If `Load` has no `\`, nothing is activated.
  `!ActivateConfig` with a file name goes to `Rainmeter::ActivateSkin(folder, file)`
  (`Rainmeter.cpp:1190-1262`). That writes `Active=<index>` for the config, and on first
  activation (no section in Rainmeter.ini yet) copies the skin's `Default*` options into
  Rainmeter.ini (§4.3).
* New-format packages can only name **one** skin. Old format allowed a `|` list.

---

## 4. The `Rainmeter.ini` settings file

### 4.1 Location, companions and encoding

* Rainmeter uses `<ProgramDir>\Rainmeter.ini` if it exists (portable). Otherwise it uses
  `%APPDATA%\Rainmeter\Rainmeter.ini`, or a path given on the command line
  (`Rainmeter.cpp:225-264`).
* Companion files sit next to it with the same base name: `Rainmeter.data` (favourites,
  `[SkinInstaller] ArchivePlugins`, update state), `Rainmeter.log` and `Rainmeter.stats`
  (`:346-361`).
* On first run it is created by copying `Defaults\Layouts\illustro default\Rainmeter.ini`
  (`:368-374, 947-952`).
* Rainmeter.ini and every layout being loaded are **rewritten as UTF-16LE with a BOM** if they
  are not already (`CheckSettingsFileEncoding`, `:2364-2394`, called at `:383` and `:1933`). The
  sample layout is UTF-16LE with a BOM and CRLF line endings. Readers must accept UTF-16LE, UTF-8
  and ANSI (`Common/IniFile.cpp:21-104`).
* Duplicate sections and duplicate keys: **the first one wins** (`ConfigParser.cpp:1695, 1763`).
  Section and key names are case-insensitive.

### 4.2 `[Rainmeter]` keys that matter

Most are read in `ReadGeneralSettings`, `Rainmeter.cpp:1691-1838`.

| Key | Meaning (default) |
|---|---|
| `SkinPath` | Skins folder (default `Documents\Rainmeter\Skins\`; `:459-479`). Environment variables are expanded. |
| `ConfigEditor` | Editor used for "Edit skin" (default: the `.ini` association, or Notepad). |
| `Language` | Language id/name (`Language.cpp:154`). |
| `Logging`, `Debug` | 0/1. |
| `TrayIcon`, `TrayExecuteR/M/DR/DM` | Tray icon and click actions (`TrayIcon::ReadOptions`). |
| `DisableDragging` | 0/1: global drag lock. |
| `DisableVersionCheck`, `DisableAutoUpdate`, `DisableRDP` | 0/1. |
| `SelectedColor` | Highlight colour for selected skins (default LightBlue). |
| `NetInSpeed`, `NetOutSpeed` | Global fallback max for NetIn/NetOut measures, in bits/s (default 0). |
| `DesktopWorkArea`, `DesktopWorkArea@N`, `DesktopWorkAreaType` | Override the work area: `L,T,R,B`. |
| `NormalStayDesktop` | 1 (default): "Normal" z-order skins stay on the desktop under Show Desktop. |
| `HardwareAcceleration` | 0/1 (`:281`). |
| `DefaultZoom` (10-500, default 100), `ForceDefaultZoom` | Dev build only. |

The sections `[Rainmeter]` and `[TrayMeasure]` are not treated as configs (`:1803-1807`). Every
other section whose name is a known config folder is (`:1809-1829`). Sections for configs that do
not exist are skipped but left in the file.

### 4.3 Per-config sections (`[illustro\Clock]`)

The section name is the **config path** (§5). It is read in `Skin::ReadOptions` (`Skin.cpp:2201-2331`)
with a `ConfigParser` limited to that section (`Skin.cpp:2548-2554`) and with `skin = nullptr`. So
`#VARIABLES#` from the skin are **not** available here, but built-in monitor variables and
formulas are.

| Key | Values / meaning | Default | Source |
|---|---|---|---|
| `Active` | `0` = not loaded. `N >= 1` = load the N-th `.ini` file of the config folder, in enumeration order (§5). A value above the file count is ignored. | 0 | `Rainmeter.cpp:1820-1825, 1156-1167, 1339-1345` |
| `LoadOrder` | Integer. Configs are activated in ascending order, and it sets the stacking order within the same z-position. Ties keep the order of sections in Rainmeter.ini (it is a multimap). | 0 | `:1827-1828, 1498-1552` |
| `WindowX`, `WindowY` | Position, see the format below. | `0` | `Skin.cpp:2244-2252` |
| `AnchorX`, `AnchorY` | The point of the skin placed at WindowX/Y, see below. | `0` | `:2254-2262` |
| `AlwaysOnTop` | `-2` on desktop, `-1` bottom, `0` normal, `1` top, `2` topmost (`Skin.h:34-38`). Out of range means 0. | 0 | `:2264-2266` |
| `OnHover` (older name `HideOnMouseOver`, read first and then overridden) | `0` none, `1` hide, `2` fade in, `3` fade out (`Skin.h:59-62`) | 0 | `:2268-2276` |
| `Draggable` | 0/1 | 1 | `:2278` |
| `SnapEdges` | 0/1 (snap to screen edges and other skins; Ctrl/Shift disables it while dragging, `:4724`) | 1 | `:2281` |
| `ClickThrough` | 0/1 | 0 | `:2284` |
| `KeepOnScreen` | 0/1 | 1 | `:2287` |
| `SavePosition` | 0/1. If 0, WindowX/Y are not written back after a drag. | 1 | `:2290, 2383-2387` |
| `StartHidden` | 0/1 | 0 | `:2293` |
| `AutoSelectScreen` | 0/1: rewrite `@N` to the monitor the skin was dropped on | 0 | `:2296, 2159-2182` |
| `AlphaValue` | 0-255, clamped | 255 | `:2306-2308` |
| `FadeDuration` | ms, minimum 0 | 250 | `:2311-2312` |
| `Zoom` | 10-500; when absent, `DefaultZoom` is used. **Dev build only.** | none | `:2299-2304` |
| `Group`, `DragGroup` | `|`-separated group names, added to the skin's own `Group` | `""` | `:2316-2321` |

**`WindowX` / `WindowY` format** (`SkinPosition.cpp:27-58, 100-134`):
* `[-]number` in logical (96-DPI) pixels, from the left/top of the reference monitor.
* `%` suffix: a percentage of the monitor width/height.
* `R` (on X) or `B` (on Y): measure from the right/bottom edge instead.
* `@N`: monitor number. `@0` is the whole virtual screen. Without `@`, the primary monitor is
  used. If only X or only Y has `@N`, the other one copies it (`:116-123`). A monitor that is
  inactive or does not exist is ignored.
* The modifiers count only if they come after the last `#`, so they are not mistaken for parts of
  variables.
* A **formula** in parentheses, e.g. `(#SCREENAREAWIDTH#-200)`, is evaluated first. Anything after
  the last `)` is kept as modifiers, e.g. `(#SCREENAREAWIDTH#-200)R@2`
  (`ConfigParser::ParseFormulaWithModifiers`, `ConfigParser.cpp:1539-1559`).
* Because the parser for this section has no skin, unqualified monitor variables such as
  `#SCREENAREAWIDTH#` resolve to the **primary monitor** (`ConfigParser.cpp` `GetMonitorVariable`,
  which only looks at the skin's monitor when `m_Skin` is set).
* Resolved position = `monitor.origin + offset (or extent - offset with R/B) - anchorPos`
  (`SkinPosition.cpp:100-105`).
* Rainmeter writes positions back as integers, or as `%.5f%` when a `%` was used, keeping the `R`,
  `B` and `@N` modifiers (`:60-98`). The authored value is kept if the skin was only clamped
  (`SkinPosition.h:13-15`).

**`AnchorX` / `AnchorY`** (`SkinPosition.cpp:8-19`): a number in skin pixels (multiplied by
zoom), `%` means a percentage of the skin's width/height, and `R`/`B` means from the right/bottom
of the skin. Example: `WindowX=50%`, `AnchorX=50%` centres the skin horizontally on the primary
monitor (the Welcome entry in the sample layout).

**Defaults supplied by the skin.** When a config is activated and Rainmeter.ini has **no section
for it** (`hasSettings`, `Rainmeter.cpp:1241-1242`, `Skin.cpp:72`), the skin's own `[Rainmeter]`
section is read for `DefaultWindowX`, `DefaultWindowY`, `DefaultAnchorX`, `DefaultAnchorY`,
`DefaultAlwaysOnTop`, `DefaultOnHover`/`DefaultHideOnMouseOver`, `DefaultDraggable`,
`DefaultSnapEdges`, `DefaultClickThrough`, `DefaultKeepOnScreen`, `DefaultSavePosition`,
`DefaultStartHidden`, `DefaultAutoSelectScreen`, `DefaultAlphaValue` and `DefaultFadeDuration`.
Each one that is defined is written into Rainmeter.ini without the `Default` prefix
(`Skin.cpp:2209-2232, 2541-2546`). Then the normal read from Rainmeter.ini follows.

Keys are written back when the user changes them through the context menu
(`Skin::WriteOptions`, `Skin.cpp:2333-2468`). Unloading a skin writes `Active=0`
(`Rainmeter.cpp:1281-1285`).

### 4.4 Layouts (`<Settings>\Layouts\<name>\Rainmeter.ini`)

* A layout is a folder `Layouts\<name>\` containing a **complete copy of Rainmeter.ini** and an
  optional `Wallpaper.bmp`. `Rainmeter::ScanForLayouts` lists every sub-folder
  (`Rainmeter.cpp:1599-1634`).
* Saving a layout from the Manage dialog copies Rainmeter.ini. The options are: drop sections with
  `Active=0`, copy the current wallpaper to `Wallpaper.bmp`, or write an **empty** file to make an
  "empty layout" (`DialogManage.cpp:1871-1925`).
* Very old versions used `Themes\<name>\Rainmeter.thm` and `RainThemes.bmp`. These are migrated
  automatically (`Rainmeter.cpp:1002-1046`, `SkinInstaller.cpp:109-157`).

**Applying a layout** (`Rainmeter::LoadLayout`, `Rainmeter.cpp:1919-1987`):

1. Fail if `Layouts\<name>\Rainmeter.ini` is missing. Normalise its encoding.
2. Unload all skins.
3. Unless `<name>` is `@Backup`, copy the current Rainmeter.ini to
   `Layouts\@Backup\Rainmeter.ini` (this is the "undo" layout).
4. **Replace Rainmeter.ini with the layout file** (a whole-file copy). Every per-config section,
   including `Active`, positions and so on, now comes from the layout. Configs that the layout does
   not mention end up with no section, so they are inactive.
5. Unless loading `@Backup`, restore these `[Rainmeter]` keys from the backed-up file:
   `SkinPath`, `ConfigEditor`, `Logging`, `DisableVersionCheck`, `DisableAutoUpdate`, `Language`,
   `NormalStayDesktop`, `SelectedColor`, `HardwareAcceleration`, `DefaultZoom` and
   `ForceDefaultZoom` are restored whenever the old value is non-empty. `TrayExecuteM/R/DM/DR` are
   restored only if the layout left them empty (`PreserveSetting`, `:1989-1997`,
   `Rainmeter.h:192`).
6. If `Wallpaper.bmp` exists, set it as the desktop wallpaper.
7. Re-read the settings and activate every config with `Active > 0`, in `LoadOrder` order.

Because the installer empties `[Rainmeter]` in packaged layouts (§3.1), a packaged layout can
never change the user's global settings.

Sample (`sample-skins/Layouts/illustro default/Rainmeter.ini`, decoded from UTF-16LE):

```ini
[Rainmeter]

[illustro\Clock]
Active=1
WindowX=(#SCREENAREAWIDTH#-200)
WindowY=0

[illustro\Disk]
Active=2
WindowX=(#SCREENAREAWIDTH#-200)
WindowY=176

[illustro\System]
Active=1
WindowX=(#SCREENAREAWIDTH#-200)
WindowY=68

[illustro\Welcome]
Active=1
WindowX=50.000000%
WindowY=50.000000%
AnchorX=50.000000%
AnchorY=50.000000%
```

`Active=2` under `[illustro\Disk]` selects the second file of that folder, `2 Disks.ini` (§5).
Rainmeter INI files have no inline comments: a `;` only starts a comment when it is the first
character after leading whitespace (`Common/IniFile.cpp:222-224`). The illustro skins are 210 px wide (the background width). `(#SCREENAREAWIDTH#-200)` puts them
10 px past the right edge of the primary monitor; `KeepOnScreen=1` (the default) pulls them back
inside. Their heights are Clock 68, System 108 and Network 108, which is why the Y values are 0,
68 and 176 (see `sample-skins.md`).

---

## 5. Config names, files and `Active=N`

`SkinRegistry::PopulateRecursive`, `Library/SkinRegistry.cpp:217-352`:

* The **config name** is the folder path relative to the Skins folder, joined with `\`, with no
  leading or trailing separator: `Skins\illustro\Clock\Clock.ini` is config `illustro\Clock`,
  file `Clock.ini`. The **root config** is the first component (`illustro`), and `#@#` is
  `<SkinPath>\<root>\@Resources\` for every config under that root (`Skin.cpp:2524`,
  `ConfigParser.cpp:218`).
* `.ini` files directly in the Skins folder (level 0) are ignored (`:257`).
* Folders that are never configs: `@Backup`, `Backup` and `@Vault` at level 0 (case-insensitive,
  `:9-13, 251`), and `@Resources` **directly under a root** (level 1 only, compared with
  case-**sensitive** `wcscmp`, `:252`). A nested `illustro\Sub\@Resources` *would* be scanned.
* A folder is listed as a config if it has at least one `.ini` file or a descendant that has one
  (`:308-349`). A root such as `illustro`, with only sub-configs, is still a (file-less) folder
  entry, so `[illustro]` can exist but cannot be `Active`.
* Only `*.ini` files count (case-insensitive extension). `.inc`, `.txt` and so on are not
  variants.
* Config lookups are case-insensitive (`FindFolderIndex`, `:37-79`), and so are file lookups
  (`FindIndexes`, `:87-104`).

**Order of files (what `Active=N` indexes).** `folder.files` is filled in the order
`FindFirstFileEx`/`FindNextFile` returns the files (`:227-304`), with **no explicit sort**. On
NTFS that is B-tree order: an ordinal comparison of names after **upper-casing** (NTFS `$UpCase`
table). `Active=N` is the 1-based index into that list. Snowmeter must sort the same way:

```
key(name) = name.encode_utf16().map(upcase_simple)  // compare code-unit by code-unit, shorter prefix first
```

Use upper-case, not lower-case. They differ for characters between `Z` and `a` (`[ \ ] ^ _ `` ` ``).
For example `AB.ini` < `A_B.ini` with upper-casing, but `A_B.ini` < `ab.ini` with lower-casing.
Other examples: `1 Disk.ini` < `2 Disks.ini` (so `Active=2` is "2 Disks"), and
`Clock 2.ini` < `Clock.ini` because a space (0x20) sorts before `.` (0x2E).

Indices drift when files are added or removed. On refresh Rainmeter re-derives `Active` from the
loaded file name and rewrites it (`Rainmeter.cpp:1866-1885, 1582-1594`). Snowmeter should store
the file name next to `Active` internally (for example in its own state file) and treat
`Active=N` as an import/export encoding.

Subfolders are walked after the files of the current folder, also in NTFS order. That order
determines the menu and the `baseID`s, not `Active`.

---

## 6. Fonts

Loading happens per skin, in `Skin::ReadSkin` (`Skin.cpp:2680-2756`):

1. **Automatic `@Resources\Fonts`.** If `<root>\@Resources\` exists, **every regular file**
   directly in `<root>\@Resources\Fonts\` (not recursive, no extension filter) is added to the
   skin's private font collection (`:2680-2719`). If a file cannot be referenced, the log shows
   "Unable to load font". Rainmeter 4.x does this too, so a skin needs no `LocalFont` for fonts in
   that folder.
2. **`LocalFont`, `LocalFont2`, `LocalFont3`, ...** from the skin's `[Rainmeter]` section, read
   in sequence until the **first missing number** (`:2721-2756`). Each value is variable-expanded
   (for example `#@#Fonts\X.ttf`) and resolved as:
   1. `<ProgramDir>\Fonts\<value>` (legacy program-folder fonts), then
   2. if not absolute, `<SkinPath>\<config>\<value>` (relative to the **config folder**, not the
      root; `Skin::MakePathAbsolute`, `Skin.cpp:6042-6058`).
3. **Scope.** The collection belongs to the skin (`m_FontCollection`). Fonts in one root's
   `@Resources\Fonts` are only visible to skins under that root, and `LocalFont` only to that one
   skin. Nothing is installed system-wide. The installer's "Install fonts to system" menu toggle
   (`m_SystemFonts`) is **dead code**: it is toggled (`DialogInstall.cpp:275-277`) but never read
   during installation.
4. **Lookup precedence** (`Common/Gfx/TextFormat.cpp:547-571`): if `FontFace` names a family that
   is **installed on the system**, the system font is used. Only otherwise is the skin collection
   searched, matching the **GDI (legacy, name ID 1) family name**
   (`FindDWriteFontInFontCollectionByGDIFamilyName`). That is why skins write
   `FontFace=Roboto Light` rather than `Roboto` with a weight.
5. **Supported types.** Files go through DirectWrite `CreateFontFileReference` and a custom
   collection (`Common/Gfx/FontCollection.cpp:50-69, 40-47`). In practice that means TrueType
   `.ttf`, OpenType `.otf` (CFF), and collections `.ttc` / `.otc`. Type 1, `.fon` and WOFF are not
   usable through this path. The **old** installer only copied `*.ttf` (`DialogInstall.cpp:554, 814`).

Snowmeter on Linux (Pango 1.52.1 and fontconfig 2.15.0 here; `pango_font_map_add_font_file`
needs Pango 1.56, so it is not available):
* Per skin (or per root): create an `FcConfig` from the default config, call
  `FcConfigAppFontAddFile`/`FcConfigAppFontAddDir` for `@Resources/Fonts/*.{ttf,otf,ttc,otc}`
  (case-insensitive extension) and for each `LocalFontN`, and give the skin its own
  `PangoCairoFontMap` with `pango_fc_font_map_set_config`. A simpler first version is
  process-wide `FcConfigAppFontAddFile(NULL, ...)` followed by
  `pango_fc_font_map_config_changed`, which accepts that fonts leak between skins.
* Keep the "system font wins" rule, and match on fontconfig `family` **and** `fullname`/`style` so
  that GDI-style names like "Roboto Light" resolve.
* Never copy fonts into `~/.local/share/fonts`. Rainmeter does not install fonts.

---

## 7. Recommended Snowmeter import algorithm

### 7.1 Paths on Linux (suggested)

| Rainmeter | Snowmeter |
|---|---|
| `%APPDATA%\Rainmeter\Rainmeter.ini` | `$XDG_CONFIG_HOME/snowmeter/Rainmeter.ini` |
| `<Settings>\Layouts\` | `$XDG_CONFIG_HOME/snowmeter/Layouts/` |
| `<Settings>\Plugins\` | `$XDG_DATA_HOME/snowmeter/Plugins/{64bit,32bit}/` (stored, not loaded) |
| `Documents\Rainmeter\Skins\` (`SkinPath`) | `SkinPath` if set, else `$XDG_DATA_HOME/snowmeter/Skins/` |
| `Skins\@Backup\`, `Skins\@Vault\` | the same names under SkinPath |
| Recycle Bin for deleted backups | freedesktop Trash (`gio trash`) |

### 7.2 Steps

1. **Classify the input.**
   * A directory: go to step 9 (bare folder import).
   * A file: read the last 16 bytes. If `bytes[9..16] == b"RMSKIN\0"` and
     `i64::from_le_bytes(bytes[0..8]) == len-16`, it is a **strict new-format** package; remember
     `flags = bytes[8]`. If the key matches but the size does not, **refuse** ("corrupt or modified
     package"), as Rainmeter does. Otherwise it is a **footerless** archive (`.rmskin` or `.zip`).
     Look at the content, not the extension.
2. **Open the ZIP** over the byte range `[0, size)` for strict packages, or the whole file
   otherwise. Reject encrypted entries and entries whose Unix mode is a symlink. Cap the total
   uncompressed size and the entry count (zip-bomb guard).
3. **Normalise entry names.** Decode as UTF-8 if GP bit 11 is set. Otherwise try strict UTF-8 and
   fall back to Windows-1252. Turn `\` into `/` and strip a leading `/` or `./`. **Reject** any
   entry with a `..` component, a drive letter (`C:`), or an absolute path. Ignore `__MACOSX/`.
4. **Find the options file.**
   * Strict: `RMSKIN.ini` at the root (case-insensitive). If it is missing, refuse, as Rainmeter
     does.
   * Footerless: (a) `RMSKIN.ini` at the root means a new-format layout, with a warning "not
     created by Skin Packager". (b) Otherwise, if a single top-level folder wraps
     `RMSKIN.ini`, strip that folder and warn. (c) Otherwise `Rainstaller.cfg` at any depth means
     the old format, with its folder as root and the old key names (§1.4). (d) Otherwise run the
     heuristic for a bare zipped skin: if there is a `Skins/` folder, use it; else, if the archive
     has one top folder containing `*.ini` files (in it or in sub-folders) or `@Resources/`, treat
     that folder as `Skins/<thatFolder>/`; else refuse.
5. **Parse the options** with an INI reader that accepts UTF-16LE/UTF-8/ANSI (the first duplicate
   wins, case-insensitive). `Name` is required. Read `Author`, `Version`, `MergeSkins` (int != 0),
   `VariableFiles` (split on `|`, trim spaces and tabs, convert `\` to `/`), `MinimumRainmeter`,
   `MinimumWindows`, `LoadType`, `Load`.
   * Compare `MinimumRainmeter` with Snowmeter's **emulated Rainmeter version** (a constant,
     e.g. `4.5.27.3934`, raised as features land) using the §2 comparison. If the package needs a
     newer version, show Rainmeter's message and offer "Install anyway".
   * Treat `MinimumWindows` as always satisfied (emulate e.g. `10.0.22631`), and log it.
   * Backup package: `flags != 0 && flags & 1 == 0`, or an old-format name matching
     `Backup-%d.%d.%d-%d.%d.rmskin`.
6. **Enumerate components** relative to the root (§3.1):
   * skins `Skins/<Root>/**` where `<Root>` is not `Backup`, `@Backup` or `@Vault`;
   * layouts `Layouts/<Name>/**/*.ini` where `<Name>` is not `Backup` or `@Backup`;
   * plugins `Plugins/{32bit,64bit}/<X>.dll`, minus the built-in list;
   * old-format extras: `Fonts/*.ttf`, `Addons/`, `Themes/*.thm`.
   * Group skin roots **case-insensitively**. If an existing folder in SkinPath matches a root
     case-insensitively, use the existing folder's spelling as the target. Refuse if nothing was
     found.
7. **Show the install dialog**: `RMSKIN.bmp` header, Name/Author/Version, and components with
   Add/Replace/"Backup and replace". Plugins get the action *"Windows plugin, stored but not
   loaded"*, or *"provided natively by Snowmeter"* when a native replacement exists for that DLL
   name. Include the "Apply included layout" / "Load included skins" checkbox, ticked by default,
   and an Advanced → "Backup skins" toggle (default on; disabled for merge and backup packages).
8. **Install** (stage everything into a temporary directory on the same filesystem first, then
   swap):
   1. Unload any **running** skins that belong to the affected roots. Rainmeter closes the whole
      app; Snowmeter only needs to unload those roots.
   2. Backup or delete each existing root:
      * default (not merge, backup on): trash the old `SkinPath/@Backup/<Root>`, then
        `rename(SkinPath/<Root>, SkinPath/@Backup/<Root>)`;
      * backup package: trash `SkinPath/<Root>`;
      * merge, or backup off: keep it.
   3. Extract the skin entries into `SkinPath/<Root>/...`, overwriting files. Canonicalise each
      path component case-insensitively against siblings that already exist (first spelling
      wins), the way NTFS behaves.
   4. Layouts: extract only the `*.ini` files into `Layouts/<Name>/...`, then empty the
      `[Rainmeter]` section of each one (keep the header line).
   5. Plugins: store the `Plugins/<arch>/<X>.dll` files under `$XDG_DATA_HOME/snowmeter/Plugins/<arch>/`
      for the record, and optionally archive them as `@Vault/Plugins/<X>/<ver>/<arch>/`. Never
      load them. Log a compatibility warning for each skin that uses them.
   6. Old format only: copy `Fonts/*.ttf` into each installed root's `@Resources/Fonts/`, install
      `Themes/<n>/*.thm` as `Layouts/<n>/*.ini` (cleaned), and ignore `Addons/` with a warning.
   7. VariableFiles, only if not merge and backup is on: for each file `F`, if both
      `@Backup/F` and `F` exist and both have a non-empty `[Variables]`, copy the old value of every
      key present in both. Key matching is case-sensitive. Rewrite the values in place, keeping the
      new file's order, comments and encoding.
9. **Bare folder import** (an unmodified skin folder dropped in):
   * If the folder has `Skins/` and/or `Layouts/` sub-folders (an extracted package), run step 6
     and the following steps against it.
   * Else, if the folder itself contains `*.ini` files (at any depth below it) or `@Resources/`,
     it is **one root**: copy it to `SkinPath/<folderName>/`, using the same backup rules as
     step 8.2 (default: backup).
   * Else, if it contains several sub-folders that each qualify, import each as a root.
10. **Rescan** the skin registry (§5), then apply the **load action**, if the box is ticked and
    installation succeeded:
    * `LoadType=Skin`: split `Load` at the last `\` into config and file. Activate that file and
      write `Active=<NTFS-order index>`. If Rainmeter.ini has no section for the config, apply the
      skin's `Default*` options first.
    * Layout: run `LoadLayout(Load)` (§4.4): back up to `Layouts/@Backup`, copy the layout over
      Rainmeter.ini, preserve the global keys, skip `Wallpaper.bmp` (or ask before applying it),
      reload, and activate in `LoadOrder` order.
    * Nothing to load and nothing running: for a bare-folder import, offer to activate the root's
      first `.ini` file (NTFS order). This is Snowmeter-only convenience behaviour.
11. **Report** what was installed, backed up, skipped and unsupported (plugins, `Addons`, unsafe
    paths).

### 7.3 Test fixtures to build

Build them with a small script (Python `zipfile` plus `struct.pack('<qB7s', len(zip), flags,
b'RMSKIN\x00')`). Keep each one tiny: one config with a one-line `.ini`.

| # | Fixture | Contents | Expected result |
|---|---|---|---|
| 1 | `basic.rmskin` | footer flags=0; `RMSKIN.ini` (`Name=Basic`, `Author=t`, `Version=1.0`, `MinimumRainmeter=4.5.0`); `Skins/Basic/Main/Main.ini` | Installs to `SkinPath/Basic/Main/Main.ini`. No load action. |
| 2 | `nofooter.zip` and `nofooter.rmskin` | the same bytes as #1 without the footer | Rainmeter: **invalid package** (no `Rainstaller.cfg`). Snowmeter: installs with a "not created by Skin Packager" warning. |
| 3 | `badsize.rmskin` | #1 with the footer `size` set to `len-17` | **Refused** as corrupt. Not retried as the old format. |
| 4 | `badkey.rmskin` | #1 with the key `RMSKIX\0` | Same as a footerless file (#2). |
| 5 | `nested-ini.rmskin` | footer; `Pkg/RMSKIN.ini`, `Pkg/Skins/...` | Rainmeter: invalid (`RMSKIN.ini` not at the root). Snowmeter strict: refuse. Snowmeter lenient (footerless copy): strip the wrapper folder and warn. |
| 6 | `noname.rmskin` | `RMSKIN.ini` without `Name` | Refused (invalid). |
| 7 | `minver.rmskin` | `MinimumRainmeter=99.0` | Rainmeter-required error, with an "Install anyway" override. |
| 8 | `minwin.rmskin` | `MinimumWindows=10.0.99999` | Installs (treated as satisfied), with a log entry. |
| 9 | `load-skin.rmskin` | `LoadType=Skin`, `Load=Basic\Main\B.ini`; files `A.ini`, `B.ini`, `b_c.ini` in `Basic\Main` | Activates `Basic\Main` with `B.ini` and writes `Active=2`. NTFS order is `A`, `B`, `B_C`, so `Active=2` is `B.ini`. |
| 10 | `load-skin-defaults.rmskin` | as #9, and `B.ini` has `[Rainmeter] DefaultWindowX=100 DefaultWindowY=50 DefaultAlwaysOnTop=1` | The new Rainmeter.ini section gets `WindowX=100`, `WindowY=50`, `AlwaysOnTop=1`. |
| 11 | `layout.rmskin` | `Skins/L/A/A.ini`, `Layouts/L Layout/Rainmeter.ini` (with `[Rainmeter] SkinPath=/evil` + `[L\A] Active=1 WindowX=10`), `Layouts/L Layout/Wallpaper.bmp`, `LoadType=Layout`, `Load=L Layout` | Layout installed with an **empty** `[Rainmeter]`. Wallpaper **not** installed. Rainmeter.ini is backed up to `Layouts/@Backup/`, replaced, and the user's `SkinPath` is preserved. `L\A` is active at x=10. |
| 12 | `plugins.rmskin` | `Plugins/32bit/Foo.dll`, `Plugins/64bit/Foo.dll`, `Plugins/64bit/WebParser.dll`, plus one skin | `Foo.dll` is stored, not loaded, and the user is warned. `WebParser.dll` is ignored (built in). |
| 13 | `fonts.rmskin` | `Skins/F/@Resources/Fonts/Test.ttf`, `Skins/F/@Resources/Fonts/readme.txt`, and `F/S/S.ini` using `FontFace=<family of Test.ttf>`; plus a top-level `Fonts/Other.ttf` | `Test.ttf` is auto-loaded for skins under root `F` only, and `readme.txt` is ignored. The top-level `Fonts/` is **ignored** (new format). |
| 14 | `localfont.rmskin` | `S.ini` with `LocalFont=#@#Extra\X.otf`, `LocalFont3=...` (no `LocalFont2`) | `X.otf` is loaded. `LocalFont3` is ignored because the sequence stops at the gap. |
| 15 | `vars-v1.rmskin`, then `vars-v2.rmskin` | v1: `Skins/V/@Resources/Vars.inc` with `[Variables] Color=red Size=10 Old=1`. The user edits it to `Color=blue`. v2: `Color=green Size=12 New=5`, `VariableFiles=V\@Resources\Vars.inc` | After v2: `Color=blue Size=10 New=5`, no `Old`. The previous tree is in `@Backup/V`. |
| 16 | `vars-merge.rmskin` | v2 with `MergeSkins=1` | No `@Backup`. `Vars.inc` is overwritten (`Color=green`). Files the user added under `V/` survive. |
| 17 | `replace.rmskin` | v2 without `VariableFiles`; the user has an extra file `V/mine.txt` | Default mode: `V/mine.txt` is only in `@Backup/V/`, and the second install moves the earlier `@Backup/V` to the Trash. |
| 18 | `backupflag.rmskin` | footer flags=`0x02` | Backup package: the existing root is trashed and replaced, and no `@Backup` is made. |
| 19 | `ignored-roots.rmskin` | `Skins/@Backup/x.ini`, `Skins/@Vault/y.ini`, `Skins/top.ini`, `README.txt`, `Skins/R/R.ini` | Only `R` is installed. |
| 20 | `zipslip.rmskin` | `Skins/R/../../evil.ini`, `/abs.ini`, a symlink entry | Unsafe entries are rejected (or the whole package refused). Nothing is written outside SkinPath. |
| 21 | `cp1252names.zip` | old-format `Rainstaller.cfg`, entry name bytes `0xE9` without the UTF-8 flag | The name decodes as `é` (Windows-1252). |
| 22 | `case.rmskin` | `Skins/Foo/a.ini` and `Skins/foo/Images/b.png`; an existing `SkinPath/FOO/` | Everything lands in the existing `FOO/` (case-insensitive canonicalisation), and `FOO` is backed up once. |
| 23 | `old-format.zip` | `MySkin/Rainstaller.cfg` (`[Rainstaller] Name=Old LaunchType=Load LaunchCommand=Old\A\A.ini`), `MySkin/Skins/Old/A/A.ini`, `MySkin/Fonts/f.ttf` | Root `MySkin/` is stripped, `Old` is installed, `f.ttf` is copied to `Old/@Resources/Fonts/`, and `Old\A` is activated with `A.ini`. |
| 24 | `utf16-options.rmskin` | `RMSKIN.ini` saved as UTF-16LE with a BOM | Parsed correctly. |
| 25 | folder `illustro/` (copy of `sample-skins/Skins/illustro`) | bare folder drop | Installed as root `illustro`. Configs `illustro\Clock`, `illustro\Disk` (`1 Disk.ini`, `2 Disks.ini`), `illustro\Google`, `illustro\Network`, `illustro\Recycle Bin`, `illustro\System`, `illustro\Welcome`. `@Resources` is not a config. |
| 26 | `sample-skins/Layouts/illustro default/Rainmeter.ini` | loaded as a layout | Clock, `2 Disks.ini`, System and Welcome are active; Welcome is centred; the other three are placed at the right edge (§4.4). |
