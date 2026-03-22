# Changelog

All notable changes to Parapet are documented here.
This project adheres to [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- `parapet_core/config` — added `hover_delay_ms: Option<u32>` to `LauncherConfig`; controls milliseconds before the launcher dropdown opens on hover; `0` restores immediate open; defaults to 150 ms
- `parapet_bar/launcher` — `open_timer: Rc<RefCell<Option<glib::SourceId>>>` cell and `cancel_open_timer()` helper mirror the existing close-timer pattern; pending open is cancelled on leave-before-delay so drive-by cursor moves never open the dropdown
- `parapet_bar/launcher` — `wire_dropdown()` gains a full `///` doc comment covering purpose, all parameters, and the open/close timer lifecycle
- `parapet_bar/workspaces` — `gdk_window_add_filter` display-level event filter installed on `x86_64` during `new()`; `_NET_CURRENT_DESKTOP` and `_NET_NUMBER_OF_DESKTOPS` property changes set an `Arc<AtomicBool>` dirty flag instead of triggering an immediate refresh
- `parapet_bar/workspaces` — `WorkspacesShared` data struct (watched atoms + dirty flag) heap-allocated and passed as a raw pointer to the GDK filter; reclaimed in `Drop` via `ffi::uninstall()`
- `parapet_bar/workspaces` — `refresh_if_dirty()` replaces the unconditional per-tick `refresh()` call; on `x86_64` the glib timer is a dirty-flag dispatcher; on non-`x86_64` the flag is always true (unconditional 100 ms poll fallback)
- `parapet_core/tests` — four new headless tests for `hover_delay_ms`: `launcher_hover_delay_defaults_to_none`, `launcher_hover_delay_explicit_zero`, `launcher_hover_delay_explicit_value`, `launcher_hover_delay_round_trips`
- `standards/CONFIG_MODEL.md` — added §4.14 Launcher Widget Fields subsection documenting all `[widgets.launcher]` TOML fields including `hover_delay_ms`
- `parapet_bar/launcher` — hover-open: launcher dropdown now opens and closes on button enter/leave with a 250 ms close-timer so the cursor can move from button into the dropdown without it disappearing
- `parapet_bar/launcher` — `open_dropdown()` helper centralises dropdown positioning, list rebuild, and CSS class management; `position_dropdown_below()` uses `translate_coordinates` for accurate positioning inside nested no-window containers
- `parapet_bar/launcher` — `schedule_close_dropdown()` and `cancel_close_timer()` helpers manage the 250 ms deferred-close `SourceId`
- `parapet_bar/launcher` — `.launcher-open` CSS class added to the button while the dropdown is visible; removed via `connect_hide` so all close paths (click, Escape, hover-out, programmatic) are covered
- `parapet_bar/themes/default.css` — `.widget-launcher.launcher-open` rule colours the button with `@color-accent` when the dropdown is open
- `parapet_bar/workspaces` — `parse_property_cardinal()` private helper: parses a single 32-bit CARDINAL X11 property from raw GDK bytes (native byte order)
- `parapet_bar/workspaces` — `parse_property_names()` private helper: parses a null-separated UTF-8 string list from raw GDK property bytes
- `parapet_bar/workspaces` — nine unit tests for `parse_property_cardinal` and `parse_property_names` (headless, pure byte-array; run under `--no-default-features`)

### Changed
- `parapet_bar/workspaces` — replaced `xprop -root` subprocess in `read_workspaces()` with three inline `gdk::property_get()` calls on the default root window; `_NET_DESKTOP_NAMES` uses `gdk::ATOM_NONE` (AnyPropertyType) per ADR-004 to handle STRING/UTF8_STRING storage ambiguity in Cinnamon/Muffin
- `parapet_bar/workspaces` — default poll interval changed from 500 ms to 100 ms (`unwrap_or(100)` in `main.rs` workspaces timer arm)
- `parapet_bar/launcher` — `set_activate_on_single_click(true)` replaces double-click activation on the result list
- `parapet_bar/launcher` — `SkimMatcherV2` matcher created once and shared across all filter operations instead of being re-created per keystroke
- `standards/ARCHITECTURE.md` — module table and startup flow updated: removed phantom `config.rs` from `parapet_bar`; startup sequence expanded to 11 numbered steps
- `standards/ARCHITECTURE.md` — `WidgetData::Cpu` updated to include `temp_celsius: Option<f32>`; `Battery` charge updated to `Option<f32>`; `Disk` updated to include `all_disks: Vec<DiskEntry>`
- `standards/BAR_DESIGN.md` — `WindowType` corrected from `Popup` to `Toplevel`
- `standards/BUILD_GUIDE.md` — dependency tables updated: `gtk` feature flag `v3_22 → v3_24`; added `gio`, `fuzzy-matcher`, `libc`, `ctrlc`; added `ureq`, `zbus`, `schemars`, `serde_json` to `parapet_core` table; `tempfile` moved to workspace
- `standards/CODING_STANDARDS.md` — added guidance on permitted uses of `unreachable!()` (invariant-guarded branches only, with `// INVARIANT:` comment) and `let _ = result` (signal-handler sends and background-thread shutdown paths only)
- `standards/RULE_OF_LAW.md` — added entries for `AGENT_GUIDE.md` (§11) and `SECURITY.md` (§12, §13)

### Removed
- `parapet_bar/workspaces` — `parse_xprop_cardinal()` and `parse_xprop_utf8_list()` archived to `doa/workspaces_xprop_helpers_2026-03-22.rs`

---

## [0.1.0-alpha] - 2026-03-18

### Added

#### Core (`parapet_core`)
- `widgets::clock` — Date/time data provider with configurable format string
- `widgets::cpu` — CPU usage (aggregate and per-core) with optional temperature via `sysinfo`
- `widgets::memory` — RAM and swap usage via `sysinfo`
- `widgets::network` — Network interface rx/tx stats via `sysinfo`
- `widgets::battery` — Battery charge and status via `/sys/class/power_supply/`
- `widgets::disk` — Filesystem usage via `sysinfo::Disks`
- `widgets::volume` — Audio volume and mute state via `pactl subscribe` background thread + mpsc channel (event-driven; no per-poll subprocesses)
- `widgets::brightness` — Screen backlight level via `/sys/class/backlight/`
- `widgets::weather` — Current conditions (temperature, WMO weather code, wind speed, humidity) from Open-Meteo free API via `ureq`; no API key required
- `widgets::media` — MPRIS2 playback info via `zbus` D-Bus session bus; single `GetAll` call per poll
- `widgets::workspaces` — Workspace list placeholder; X11 EWMH query resolved in `parapet_bar`
- `poll` — `Poller` for interval-based widget data refresh
- `config` — `ParapetConfig` with typed `WidgetKind` enum (`#[serde(tag = "type")]`), per-widget config structs with `#[serde(deny_unknown_fields)]`, JSON Schema via `schemars`; hot-reload via `ConfigWatcher`
- `error` — `ParapetError` (`thiserror`)

#### Bar (`parapet_bar`)
- GTK3 bar window — `gtk::WindowType::Toplevel` + `_NET_WM_WINDOW_TYPE_DOCK` + `_NET_WM_STRUT_PARTIAL` reserves screen edge
- Widget renderers for all 11 widget types; left/center/right section layout via `gtk::Box`
- CSS theme loading via `gtk::CssProvider`; named theme support via `--theme <name>` CLI flag; dark/light variant auto-detection via `GtkSettings`
- Theme hot-reload — active CSS file watched; provider swapped without restarting the bar
- Config hot-reload — `ConfigWatcher` on `~/.config/parapet/config.toml`; widget tree rebuilt on change
- CSS hot-reload follows `bar.theme` config changes — swaps both the CSS provider and the file watcher in place
- Per-widget `on_click`, `on_scroll_up`, `on_scroll_down` action strings; executed async via `sh -c`
- Per-widget `extra_class` CSS class injection
- Workspace click-to-switch via `_NET_CURRENT_DESKTOP` root-window message
- App launcher widget (`LauncherWidget`) with fuzzy search via `fuzzy-matcher`; live refresh via `gio::AppInfoMonitor`
- Separator widget with configurable glyph
- Startup timing instrumentation via `tracing::debug!` checkpoints (silent by default; `RUST_LOG=parapet_bar=debug`)
- SIGTERM / Ctrl-C handler via `ctrlc`; clean `gtk::main_quit()` on signal
- `themes/default.css` with six `@define-color` colour tokens (`color-pill`, `color-fg`, `color-fg-dim`, `color-accent`, `color-warning`, `color-urgent`)

#### Documentation & Tooling
- `standards/` — 13 standards documents covering architecture, coding style, widget API, bar design, config model, build, testing, platform compatibility, UI, release, agent guide, and security
- `DOCS/theme-spec.md` — community theme authoring specification
- `DOCS/research/` — research spikes for volume event-driven design, workspaces, config overhaul, theming, and more
- `justfile` — `check`, `check-headless`, `install-hooks` recipes
- Pre-commit hook enforcing `cargo clippy -D warnings` + `cargo fmt --check`
