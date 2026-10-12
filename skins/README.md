# Snowmeter skin gallery

Snowmeter runs unmodified Windows Rainmeter skins. This folder holds the skins that ship with it and the gallery catalog (`catalog.ini`).

## How to install

- `snowmeter gallery` lists every entry, its license, status and whether it is installed.
- `snowmeter install <name>` installs one (add `--force` to reinstall).
- In the tray icon, open **Get skins** and click an entry that is not installed yet.
- Load a config afterwards with `snowmeter load "<Root>\<Config>"`, or from the tray.

## Gallery

Status is how well the skin works in Snowmeter today: good means the main configs render and work, partial means some parts are missing (see notes). Statuses are refreshed as Snowmeter improves.

| Name | Author | License | Kind | Status | Notes |
|---|---|---|---|---|---|
| Snowmeter | Snowmeter contributors | MIT | bundled | good | Weekday, System and Disk widgets that ship with Snowmeter |
| Monterey | creewick (Linux port: Snowmeter) | CC BY-NC-SA 4.0 | bundled | good | Clock, Calendar, Weather, Music, Volume and Timer in Small/Medium/Wide/Large, 5 themes, 8 languages. Open `Monterey\Settings.ini` to start; right-click a widget for sizes. Changes from the original are listed in `Monterey/SOURCE.md`. |
| MondRemake | Snowmeter (inspired by Mond, ApexXx-SenSei) | MIT | bundled | good | Original remake: clock/date, music player (Spotify and any MPRIS player), Open-Meteo weather with 5-day forecast; settings from the right-click menu. Installing it downloads the Anurati font (personal use). |
| Mond | ApexXx-SenSei | CC BY-NC-ND 3.0 | original package (bundled) | good | Original package, unmodified (the license allows no derivatives). On install Snowmeter downloads the Anurati font (personal use, not redistributed here) and switches the dead Yahoo weather source to wttr.in in your local copy. |

## Licensing

Snowmeter and its own skins (`Snowmeter/`, `MondRemake/`) are MIT-licensed;
the fonts they bundle keep their own licenses (see each `@Resources/Fonts`).
Monterey is an adaptation of creewick's skin and stays CC BY-NC-SA 4.0
(non-commercial, share alike; see `Monterey/LICENSE.txt` and `SOURCE.md`).
Mond (CC BY-NC-ND 3.0) ships as the author's unmodified package in
`packages/`, and its font is downloaded from its source at install time
instead of being redistributed here.

## Credits

Thanks to creewick for Monterey for Rainmeter (and fediaFedia for Big Sur, which inspired it), and to ApexXx-SenSei for Mond.
