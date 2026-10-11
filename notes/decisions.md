# Decisions log

Decisions made while working unattended. Newest at the bottom.

1. **Rust installed with rustup** (`--profile default`, no sudo) into `~/.cargo`. `. "$HOME/.cargo/env"` was appended to `~/.bashrc` and `~/.profile` (what rustup does by default).
2. **Git identity** set locally in the repo only (`user.name=Bernardo`, `user.email` = account email) because no global git identity existed.
3. **Screenshots > 2MB**: `.gitignore` cannot filter by size, so a local `pre-commit` hook rejects any staged file over 2MB and anything that looks like a GitHub token. Screenshots are also saved downscaled.
4. **Desktop detected**: `x11 X-Cinnamon` (Linux Mint 22.3). Primary backend is X11 with a `_NET_WM_WINDOW_TYPE_DESKTOP`-style below-all window. Wayland/layer-shell support is behind an optional cargo feature since `gtk4-layer-shell` dev files are not installed here.
5. **GitHub repo name**: `snowmeter` was free on the account, so it is used (private).
