# Mond Remake

A light remake of the famous **Mond** Rainmeter skin by **ApexXx-SenSei**
(<https://www.deviantart.com/apexxx-sensei/art/Mond-762455575>), written from
scratch for Snowmeter. No files from the original are used (its license does
not allow derivatives); the original package is still offered separately as
`Mond` in the gallery.

## Widgets

| Config | What it shows |
|---|---|
| `MondRemake\Clock` | big spaced day name, date and time (12 or 24 hour) |
| `MondRemake\Player` | artist, title, progress (click to seek), previous / play-pause / next for Spotify or any MPRIS player |
| `MondRemake\Weather` | current temperature and conditions, feels-like, today's high/low and a 5-day forecast from Open-Meteo (no API key) |

## Settings

Right-click any of the widgets:

- **Edit settings** opens `@Resources/Settings.inc`: language (en, pt, es, fr, de, it),
  12/24-hour clock, units, location, music player, colours and fonts.
- **24-hour / 12-hour clock** and **Celsius / Fahrenheit** switch at once.
- On the weather widget, **Set location...** searches a city by name and
  **Update weather** downloads the forecast now.
- **Size** (from Snowmeter's menu, Ctrl+right-click if the skin menu shows) or
  Ctrl+mouse wheel resizes a widget.

## Resources

The clock updates once a second, the player once a second while it is open,
and the weather widget wakes every 5 seconds and downloads the forecast every
30 minutes. Nothing runs in a loop; the three widgets together stay well under
1% CPU.

## Fonts

Syncopate (Apache 2.0) and Quicksand (SIL OFL 1.1) ship in `@Resources/Fonts`.
`snowmeter install MondRemake` also downloads **Anurati**, the font of the
original Mond (free for personal use, not redistributed here); when it is
installed the day name uses it automatically.

## License

MIT, like Snowmeter. Fonts keep their own licenses (see `@Resources/Fonts`).
