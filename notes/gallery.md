# Skin gallery design

Goal (user request, 2026-10-12): ship many famous Rainmeter skins with Snowmeter so people have options, plus Mond. Constraint: skins belong to their authors, so a skin is **bundled in the repo only if its license allows redistribution**; everything else is offered as a **download-from-the-author** catalog entry.

## Kinds of gallery entries

| Kind | Where the files are | When it is used |
|---|---|---|
| `folder` | `skins/<Root>/` in the repo (unmodified upstream files + `LICENSE`/`SOURCE.md` added at the root folder) | MIT, GPL, MPL, CC0, WTFPL, CC BY, CC BY-SA, CC BY-NC-SA skins |
| `package` | `skins/packages/<Name>.rmskin`, byte-for-byte the author's release | licenses that forbid derivatives (CC BY-ND, CC BY-NC-ND — e.g. Mond). Adding files or patching would be a derivative, so the original package is kept intact and installed with the normal importer |
| `remote` | not in the repo; `url=` points at the author's own GitHub (release asset or archive at a pinned commit) | skins without any license (all rights reserved) — the user downloads them from the author at install time |

`skins/catalog.ini` (INI so the existing parser reads it; one section per entry):

```ini
[Mond]
Kind=package
Path=packages/Mond.rmskin
Author=ApexXx-SenSei
License=CC BY-NC-ND 3.0
Source=https://www.deviantart.com/apexxx-sensei/art/Mond-762455575
Description=Minimal clock, date, weather and music player
Status=good
; optional extras the user downloads from the official source at install time
FontUrl1=...
PostInstall=mond-weather-wttr      ; built-in fix-ups the user's copy gets (never redistributed)

[ModernGadgets]
Kind=folder
Path=ModernGadgets
...

[MD3-Windows]
Kind=remote
Url=https://github.com/Runixe786/MD3-Windows/archive/<commit>.zip
Subdir=...
```

## Behaviour

- `snowmeter gallery` — list entries: name, author, license, status (good / partial: what's missing), installed or not.
- `snowmeter install <name>` — `folder`: copy into the skins folder (no overwrite unless `--force`); `package`: run the `.rmskin` importer on the bundled file (layout packages: never deactivate the user's other skins — apply only the package's own config sections); `remote`: download to `~/.cache/snowmeter/gallery/`, verify size limits, import. Then print the compat report and how to load it.
- Built-in post-install fix-ups (applied to the user's local copy only): `mond-weather-wttr` (Yahoo → wttr.in, keeps `Weather.ini.yahoo-original`).
- First run copies the `folder` entries and the Snowmeter suite (as today) and activates only the Snowmeter widgets. Existing users get newly bundled `folder` entries once: `[Snowmeter] BundledInstalled=` remembers which roots were already offered, so a root the user deleted is not copied back.
- Tray: "Get skins" submenu listing gallery entries that are not installed; clicking installs (remote ones download in a background thread) and shows a notification/log line.
- `skins/README.md` — human-readable gallery table with credits, licenses and status.
