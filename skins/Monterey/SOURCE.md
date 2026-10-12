# Monterey for Snowmeter

This folder is a Linux port of **Monterey for Rainmeter** by **creewick**
(<https://github.com/creewick/MontereyRainmeter>, release v1.0.2, commit
`7103362`), itself inspired by fediaFedia's *Big Sur for Rainmeter*.

- License: CC BY-NC-SA 4.0 (see `LICENSE.txt`). This adapted version is
  shared under the same license: credit the original author, no commercial
  use, and share changes under the same terms. It is not covered by
  Snowmeter's MIT license.
- Font: Inter (SIL Open Font License 1.1, see `@Resources/Fonts/Inter-OFL.txt`),
  reduced to Latin and Cyrillic characters, replaces Apple's SF Pro, which may
  not be redistributed.

## Changes from the original

- Files converted from UTF-16 to UTF-8.
- Widgets kept: Clock, Calendar, Weather, Music, Volume, Timer, each in Small
  (1x1), Medium (2x2), Wide (4x2) and Large (4x4). Removed: Monitor, Notes and
  Reminders widgets, and the Windows-only Sidebar (tray icon) and Unlock
  (Windows task) extras; the Settings lists were rebuilt to match.
- Removed the unused OpenWeatherMap API key and the debug copy of the weather
  parser (it downloaded the forecast twice and logged it on every update).
- Default theme is Auto. The Blur theme draws its own rounded corners and is a
  little more opaque, so it stays readable where the compositor cannot blur
  (real blur needs KDE Plasma/KWin).
- Double-click actions open `gnome-clocks`, `gnome-calendar` and
  `gnome-weather` instead of Windows apps (change `ClockApp`, `CalendarApp`,
  `WeatherApp` in `@Resources/Variables/Global.inc`).
- Music player list offers MPRIS players (Spotify, VLC, Firefox, Chromium,
  Chrome, Rhythmbox, Elisa, Strawberry, Audacious, mpv) instead of Windows ones.
- New languages: Português, Español, Français, Italiano (with English,
  Deutsch, Русский and Український from the original).
- Fixes: weather error code typo (`!SerVariable`), hourly forecast starting at
  midnight instead of the current hour, daily min/max failing before data
  arrives, analog clock hand colour (`IfNotMatchAction`), the Settings tabs
  being drawn under the window panels, the German language tick mark.
- Sidebar label reads "Monterey" instead of "Monterey Rainmeter".
- PNG images recompressed losslessly.
