# Missing system packages

Packages that were not installed and could not be installed without sudo, and how Snowmeter works around them.

| Package (Debian/Ubuntu/Mint name) | Needed for | Workaround |
|---|---|---|
| `libgtk4-layer-shell-dev` | Wayland background layer (`gtk4-layer-shell`) | We are on X11; layer-shell is an optional cargo feature (`--features wayland-layer-shell`), off by default. |
| `librsvg2-dev` | SVG images | Not required; Rainmeter skins rarely use SVG. Image meter falls back to gdk-pixbuf loaders. |
| `liblua5.4-dev` | Lua Script measures | `mlua` is built with the `vendored` feature (compiles Lua 5.4 from source), so no system Lua is needed. |
| `libssl-dev` | HTTPS for WebParser | HTTP client uses `rustls` (pure Rust TLS), so OpenSSL is not needed. |
| `libdbus-1-dev` | Tray icon | Tray uses `ksni`, which talks D-Bus through pure-Rust `zbus`, so libdbus is not needed. |
