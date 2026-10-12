# Decisions log

Decisions made while working unattended. Newest at the bottom.

1. **Rust installed with rustup** (`--profile default`, no sudo) into `~/.cargo`. `. "$HOME/.cargo/env"` was appended to `~/.bashrc` and `~/.profile` (what rustup does by default).
2. **Git identity** set locally in the repo only (`user.name=Bernardo`, `user.email` = account email) because no global git identity existed.
3. **Screenshots > 2MB**: `.gitignore` cannot filter by size, so a local `pre-commit` hook rejects any staged file over 2MB and anything that looks like a GitHub token. Screenshots are also saved downscaled.
4. **Desktop detected**: `x11 X-Cinnamon` (Linux Mint 22.3). Primary backend is X11 with a `_NET_WM_WINDOW_TYPE_DESKTOP`-style below-all window. Wayland/layer-shell support is behind an optional cargo feature since `gtk4-layer-shell` dev files are not installed here.
5. **GitHub repo name**: `snowmeter` was free on the account, so it is used (private).
6. **Token budget (user request mid-session: "you're wasting too many tokens")**: the process was slimmed down:
   - Research agents were told to wrap up with what they had already read instead of reading more.
   - `notes/plan.md` is a compact milestone plan with exact interfaces, file paths and test commands, not full code listings.
   - Execution uses ~7 large milestones, each implemented by one Sonnet subagent (TDD where required, builds/tests/commits/pushes itself). Code review runs once per two or three milestones plus a final whole-branch review, instead of an implementer + reviewer + fix-loop per small task.
   - The controller (main session) only dispatches, checks the test output, and does the final verification/report.
7. **Execution ruling**: the user explicitly said to work on `main` with no worktrees, so subagent-driven-development's worktree step is skipped.
8. **Which Rainmeter to match**: `rainmeter-src` turned out to be a development fork (adds `SyntaxVersion=2`, `@Inherit`, `[$Skin…]` variables, `!Skin:` bangs, TextEdit/SVG meters), while the installed `Rainmeter.dll` is stock **4.5.27.3934** (see `binary-evidence.md`). Skins in the wild target stock 4.5, so Snowmeter matches stock 4.5 behaviour; fork-only features are low priority and only added where harmless.
9. **skin-format.md length**: the research agent produced 1260 lines instead of the requested ≤450. Kept as is (condensing would cost more tokens than it saves); implementers read only the sections they need.
10. **Rulings made during execution** (from the SDD ledger):
    - Research agents wrapped up early to save tokens; `skin-format.md` kept at 1260 lines. Cost if wrong: some edge cases undocumented.
    - Milestone reviews instead of per-task reviews (checkpoint A after M1–M3, final review of M4–M7 on Sonnet, not the most capable model). Cost if wrong: subtle M1–M3 issues found later.
    - `OptionReader` (missing from M1) added in M2; section-variable selectors implemented on `Measure`.
    - The "background regression" was a pre-existing relative-root bug (`#@#` joined twice), fixed in `Environment::with_root`.
    - Skin size capped at 8192 px/side, image crops at 16384, Line counts clamped — protects against hostile/broken skins; a legitimately larger skin would be clipped.
    - InputText implemented natively (illustro's Google skin needs it); the entry is a separate normal window because desktop-type windows can't take keyboard focus.
    - Importer is more lenient than Rainmeter (accepts footer-less `.rmskin`, plain zips and folders) because the goal is "drop it in and it runs".
    - Live-desktop screenshot is a composite of the 10 captured widget windows on a neutral background (the user's maximized terminal covered the desktop and the WM ignored the minimize hint); the public IP shown by the Network skin was redacted.
    - `EmptyBin` now asks for confirmation (Windows behaviour); `EmptyBinSilent` doesn't.
11. **Mond (user request, 2026-10-12)**: imported the original DeviantArt `.rmskin` (mirror: github.com/rmskins/mond) into the real config; Mond files stay out of the repo (third-party, not ours to redistribute). Added NowPlaying (MPRIS) and the Image `Path` separator fix to Snowmeter. Installed the Anurati font (free for personal use, from the Fixed-Mond fork) into the skin's `@Resources/Fonts`. Repointed the dead Yahoo weather at wttr.in (keyless) instead of the fork's OpenWeatherMap version, which needs a personal API key. Mond's layout deactivated the bundled widgets (Rainmeter layout semantics); they were re-activated at the user's positions (backup: `~/.config/snowmeter/snowmeter.ini.before-mond`). Mond\Settings left unloaded.
