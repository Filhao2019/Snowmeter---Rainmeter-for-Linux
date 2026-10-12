# Snowmeter skin gallery

Snowmeter runs unmodified Windows Rainmeter skins. This folder holds the skins that ship with it and the catalog (`catalog.ini`) of skins you can fetch from their authors.

## How to install

- `snowmeter gallery` lists every entry, its license, status and whether it is installed.
- `snowmeter install <name>` installs one (add `--force` to reinstall). Bundled skins are copied, downloads come straight from the author's GitHub.
- In the tray icon, open **Get skins** and click an entry that is not installed yet.
- Load a config afterwards with `snowmeter load "<Root>\<Config>"`, or from the tray.

## Gallery

Status is how well the skin works in Snowmeter today: good means the main configs render and work, partial means some parts are missing (see notes). Statuses are refreshed as Snowmeter improves.

| Name | Author | License | Kind | Status | Notes |
|---|---|---|---|---|---|
| Snowmeter | Snowmeter contributors | GPL-2.0-or-later | bundled | good | Weekday, System and Disk widgets that ship with Snowmeter |
| Mond | ApexXx-SenSei | CC BY-NC-ND 3.0 | original package (bundled) | good | Original package, unmodified (the license allows no derivatives). On install Snowmeter downloads the Anurati font (personal use, not redistributed here) and switches the dead Yahoo weather source to wttr.in in your local copy. |
| illustro | Rainmeter team | GPL-2.0 | bundled | good | The Google search config uses the TextEdit input of the Rainmeter fork. |
| SilverAzide-Gadgets | SilverAzide | CC BY-NC-SA 4.0 | bundled | good | The All CPU Meter renders empty. Windows-only plugins (Mouse, ConfigActive, CursorColor, ActiveNet) are not included. |
| Wisp | FiiZzioN | CC BY-NC-SA 3.0 | bundled | good | The proprietary Century Gothic font and the wallpapers were removed. text uses a fallback font. Needs CoreTemp/MSI Afterburner on Windows for hardware data. |
| STRS | SAM TOKI STUDIO | GPL-3.0 | download from author | good | Downloaded from the author (the Sarasa UI SC font alone is 13 MB). About 56 of 62 configs render fully. |
| DMelopers-Block-HUD | d-meloper | MIT (original content, see THIRD_PARTY_NOTICES) | download from author | good | Downloaded from the author (too large to bundle, Mouse.dll is Windows only). Parts that need FolderInfo, FileView, Mouse or RunCommand do not work yet. |
| Blue-Archive | Xenon257R | MIT (code), game artwork belongs to Nexon | download from author | good | Downloaded from the author because of the third-party game artwork. |
| Persona-5-Calendar | Mive82 | WTFPL (code), artwork belongs to Atlus | download from author | good | Downloaded from the author because of the third-party artwork. Some Calc formulas with nested ternaries log warnings. |
| NThing-UI | Runixe786 | none stated (all rights reserved) | download from author | good | Downloaded from the author. |
| BigSur | fediaFedia | none stated (all rights reserved) | download from author | good | Downloaded from the author. The blurred sidebar background (EasyBlur plugin) is blank. |
| Enigma | Kaelri | CC BY-NC-SA 3.0 | bundled | good | Sidebar, clock, calendar, system, network and music work; weather needs network; CoreTemp/WifiStatus parts show placeholders. |
| ModernGadgets | raiguard | CC BY-NC-SA 3.0 (skin), MIT (code) | bundled | good | CPU, RAM, network, disks, calendar and chronometer work; temperature/fan readings (CoreTemp, SpeedFan, HWiNFO) are placeholders. |
| SystemFetch | Meti0X7CB | GPL-3.0 | bundled | good | System, clock, calendar and hardware monitor work (the FrostedGlass blur is ignored); GPU readings are placeholders. |
| Cleartext | redsaph | CC0 1.0 | bundled | good | Now-playing widget for any MPRIS player (Spotify, browsers, VLC...) through Snowmeter's WebNowPlaying emulation. |
| rainmeter-todo | alperenozlu | GPL-3.0 | bundled | partial | Clicking and text input are untested. |
| SysDash | marcopixel | MIT (fonts Uni Sans and UniNeue have no license) | download from author | partial | Downloaded from the author because its fonts are unlicensed. InlineSetting formulas are not evaluated. Yahoo weather is dead. |
| Monterey | creewick | CC BY-NC-SA 4.0 (bundles Apple SF Pro, not covered) | download from author | partial | Downloaded from the author because of the bundled Apple font. The analog clock shows only the second hand. FrostedGlass and Chameleon plugins are missing. |
| Mondtholomew | reisir | none stated (all rights reserved) | download from author | partial | Downloaded from the author. The clock is excellent. other parts are incomplete. |
| Sonder | mpurses | none stated (all rights reserved) | download from author | partial | Downloaded from the author. The date string is clipped on the left. inline script calls inside bang arguments are not evaluated. |
| Notch | khanhas | none stated (all rights reserved) | download from author | partial | Downloaded from the author. |
| catppuccin | modkavartini | none stated (no LICENSE file) | download from author | partial | Downloaded from the author. |
| MD3-Windows | Runixe786 | none stated (all rights reserved) | download from author | partial | Downloaded from the author (commit archive v0.5-PublicBeta). Material You colours (MeterialYou plugin) are not supported. |

## Credits

Thanks to all skin authors: ApexXx-SenSei (Mond), the Rainmeter team (illustro), SAM TOKI STUDIO (STRS), SilverAzide (Gadgets), FiiZzioN (Wisp), d-meloper (Block HUD), Xenon257R (Blue Archive), Mive82 (Persona 5 Calendar), Runixe786 (NThing UI, MD3 Windows), fediaFedia (BigSur), Kaelri (Enigma), raiguard (ModernGadgets), Meti0X7CB (SystemFetch), redsaph (Cleartext), alperenozlu (rainmeter-todo), marcopixel (SysDash), creewick (Monterey), reisir (Mondtholomew), mpurses (Sonder), khanhas (Notch), modkavartini (catppuccin).

## Licensing

Snowmeter itself is GPL-2.0. The skins in this folder are not: every bundled skin keeps its own license, stored in its folder as `LICENSE`, with the exact upstream ref and the list of removed files in `SOURCE.md`. Skins under CC BY-NC-SA terms (Gadgets, Wisp, Enigma, ModernGadgets) stay non-commercial and share-alike, and Mond (CC BY-NC-ND) is shipped as the author's unmodified package. Windows-only plugins, executables and proprietary fonts were removed from bundled copies. Skins without any license (all rights reserved), or that carry third-party art or fonts, are not redistributed here: `snowmeter install` downloads them from their authors at the pinned release or commit.
