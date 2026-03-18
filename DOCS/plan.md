# Plan: Temperature, Disk, Script, and Active Window Widgets

**Status:** Draft
**Date:** 2026-03-18
**Standards consulted:** RULE_OF_LAW.md, WIDGET_API.md, ARCHITECTURE.md,
CODING_STANDARDS.md, CONFIG_MODEL.md, BAR_DESIGN.md, PLATFORM_COMPAT.md, TESTING_GUIDE.md

---

## Overview

Implement four new widgets identified as Priority-1 in
`DOCS/research/widget-candidates.md` and ratified by the design note in that same file:

1. **`temperature`** — CPU hardware sensor temperature via `sysinfo::Components`.
2. **`disk`** — Filesystem usage for a configured mount point via `sysinfo::Disks`.
3. **`script`** — Arbitrary shell command output; the extensibility escape hatch.
4. **`active_window`** — Currently focused window title via X11 EWMH; bar-only
   self-poll, no `frames_core` component.

`temperature`, `disk`, and `script` follow the standard eight-step WIDGET_API.md §5
checklist. `active_window` follows the established workspaces/launcher bar-only pattern:
no `WidgetData` variant, no Poller registration, `build_widget` returns
`(None, Some(source_id))`.

No new external crate dependencies are required for any of these four widgets.

---

## Affected Crates & Modules

### `frames_core`

| File | Change |
|------|--------|
| `src/widget.rs` | Add `WidgetData::Temperature`, `::Disk`, `::Script` variants; bump `WIDGET_API_VERSION` `"1.4.0"` → `"1.7.0"` (three minor bumps, one per new variant) |
| `src/widgets/mod.rs` | Add `pub mod temperature; pub mod disk; pub mod script;` |
| `src/widgets/temperature.rs` | **New** — `TemperatureWidget` implementing `Widget` |
| `src/widgets/disk.rs` | **New** — `DiskWidget` implementing `Widget` |
| `src/widgets/script.rs` | **New** — `ScriptWidget` implementing `Widget` |
| `src/config.rs` | Add `component`, `mount`, `command`, `max_length` fields to `WidgetConfig` |
| `tests/config_roundtrip.rs` | Add round-trip tests for the three new widget types |

### `frames_bar`

| File | Change |
|------|--------|
| `src/widgets/mod.rs` | Add `pub mod temperature; pub mod disk; pub mod script; pub mod active_window;` |
| `src/widgets/temperature.rs` | **New** — GTK3 label renderer consuming `WidgetData::Temperature` |
| `src/widgets/disk.rs` | **New** — GTK3 label renderer consuming `WidgetData::Disk` |
| `src/widgets/script.rs` | **New** — GTK3 label renderer consuming `WidgetData::Script` |
| `src/widgets/active_window.rs` | **New** — bar-only self-polling X11 EWMH renderer (no Poller, no core widget) |
| `src/main.rs` | Add `TemperatureRenderer`, `DiskRenderer`, `ScriptRenderer` structs + `impl RendererDispatch`; add `"temperature"`, `"disk"`, `"script"`, `"active_window"` arms to `build_widget()` |

### New modules requiring ARCHITECTURE.md §4.1 / §5 update
- `frames_core::widgets::temperature`
- `frames_core::widgets::disk`
- `frames_core::widgets::script`
- `frames_bar::widgets::temperature`
- `frames_bar::widgets::disk`
- `frames_bar::widgets::script`
- `frames_bar::widgets::active_window`

### New external dependencies
None. `sysinfo ~0.30` (`Components`, `Disks`) and `std::process::Command` are already
available. `gdk::property_get` (EWMH reads) is already used by `workspaces.rs`.

---

## New Types & Signatures

### `WidgetData::Temperature` (new variant in `frames_core/src/widget.rs`)

```rust
/// CPU or system sensor temperature from the hardware monitoring subsystem.
///
/// Temperature is always stored in °C regardless of display preference —
/// unit conversion is a rendering concern (see TempUnit in the weather widget).
/// `label` is the raw hardware sensor label (e.g. `"Package id 0"`, `"Tctl"`)
/// sourced from `sysinfo::Component::label()`. `critical` is the hardware
/// shutdown threshold from `sysinfo::Component::critical()`, if available.
Temperature {
    /// Current temperature in degrees Celsius.
    celsius: f32,
    /// Hardware sensor label as reported by sysinfo.
    label: String,
    /// Hardware shutdown/throttle threshold in °C, if the sensor reports one.
    critical: Option<f32>,
},
```

### `WidgetData::Disk` (new variant)

```rust
/// Filesystem usage statistics for one mount point.
///
/// `used_bytes = total_bytes - available_bytes`. The mount point string
/// comes from the config; the widget verifies the mount exists.
Disk {
    /// Bytes consumed by files on this filesystem.
    used_bytes: u64,
    /// Total filesystem capacity in bytes.
    total_bytes: u64,
    /// Mount point path (e.g. `"/"`, `"/home"`).
    mount_point: String,
},
```

### `WidgetData::Script` (new variant)

```rust
/// Output of a user-configured shell command, trimmed of surrounding whitespace.
///
/// An empty string means the command produced no output or exited non-zero.
/// The renderer decides whether to hide or show a placeholder in that case.
Script {
    /// Trimmed stdout of the configured command. Empty on non-zero exit or timeout.
    text: String,
},
```

### `TemperatureWidget` (new in `frames_core/src/widgets/temperature.rs`)

```rust
/// Polls `sysinfo::Components` for the configured hardware sensor temperature.
///
/// On construction, enumerates available components and selects the target
/// sensor (by `component` config field). Selection heuristic when `component`
/// is absent: first label matching `"Package id 0"` (case-insensitive), then
/// first matching `"CPU"`, then the first component overall. On machines with
/// no sensors (some VMs), returns a stale-zero `WidgetData::Temperature`.
pub struct TemperatureWidget { ... }

/// Create a new temperature widget targeting the given sensor label, or
/// auto-detecting the CPU package sensor when `component_label` is `None`.
pub fn new(name: &str, component_label: Option<String>) -> Self
```

Error type: `FramesError` (no new error variants needed; uses `SysInfo` variant on
read failures). Component absence is not an error — returns `celsius: 0.0, critical: None`.

### `DiskWidget` (new in `frames_core/src/widgets/disk.rs`)

```rust
/// Polls `sysinfo::Disks` for the usage of a configured mount point.
///
/// Skips disks where `total_space() == 0` (pseudo-filesystems). If the
/// configured mount is not found on a given poll cycle, returns the last
/// known value (stale cache per WIDGET_API §7.2).
pub struct DiskWidget { ... }

/// Create a new disk widget monitoring `mount_point` (e.g. `"/"`).
pub fn new(name: &str, mount_point: String) -> Self
```

### `ScriptWidget` (new in `frames_core/src/widgets/script.rs`)

```rust
/// Runs a shell command and captures its stdout as widget data.
///
/// The command is run as `sh -c <command>` with a 5-second wall-clock
/// timeout enforced via a background OS thread and `std::sync::mpsc`.
/// Non-zero exit or a timed-out command produces an empty `text` string.
/// A guard flag prevents overlapping spawns if a previous run has not
/// completed before the next poll cycle fires.
pub struct ScriptWidget { ... }

/// Create a new script widget running `command` on each poll cycle.
pub fn new(name: &str, command: String) -> Self
```

### `ActiveWindowWidget` (new in `frames_bar/src/widgets/active_window.rs`)

```rust
/// Bar-only renderer that displays the title of the currently focused X11 window.
///
/// Does not implement `frames_core::Widget` — there is no core data provider.
/// Manages its own glib timer instead of using the Poller. Reads
/// `_NET_ACTIVE_WINDOW` + `_NET_WM_NAME` from the root window via
/// `gdk::property_get` (ADR-004, PLATFORM_COMPAT §3.1).
///
/// `max_length` caps the displayed title length; long titles are truncated
/// with `…` using the char-boundary-safe helper from `media.rs`.
pub struct ActiveWindowWidget { label: gtk::Label }

pub fn new(config: &WidgetConfig) -> anyhow::Result<Self>
pub fn widget(&self) -> &gtk::Widget
pub fn refresh(&self)   // reads EWMH, updates label
```

### New `WidgetConfig` fields (in `frames_core/src/config.rs`)

```rust
/// Temperature: hardware sensor label to monitor (e.g. `"Package id 0"`).
/// `None` → auto-detect CPU package sensor.
#[serde(default)]
pub component: Option<String>,

/// Disk: mount point to monitor (e.g. `"/"`). Default `"/"`.
#[serde(default)]
pub mount: Option<String>,

/// Script: shell command whose stdout is displayed. Required for `"script"` type.
#[serde(default)]
pub command: Option<String>,

/// Active window: maximum title length in Unicode scalar values before truncation.
#[serde(default)]
pub max_length: Option<usize>,
```

---

## Implementation Steps

### Step 1 — Add three `WidgetData` variants + version bump
**File:** `crates/frames_core/src/widget.rs`

Add `Temperature { celsius: f32, label: String, critical: Option<f32> }`,
`Disk { used_bytes: u64, total_bytes: u64, mount_point: String }`, and
`Script { text: String }` to the `WidgetData` enum. Each is a non-breaking
addition under `#[non_exhaustive]`. Bump `WIDGET_API_VERSION` from `"1.4.0"` to
`"1.7.0"` (three variants × one minor bump each per WIDGET_API §2). Add unit tests
for each new variant's `Clone` and `Debug` bounds in the inline `#[cfg(test)]` block.

Verify: `cargo build -p frames_core` compiles. All existing bar
`match data { ... }` arms that include `_ => {}` continue to compile due to
`#[non_exhaustive]`.

### Step 2 — `TemperatureWidget` core implementation
**File:** `crates/frames_core/src/widgets/temperature.rs` (new file)

Implement `TemperatureWidget`. Fields: `name: String`,
`components: sysinfo::Components`, `target_label: Option<String>`,
`last: Option<WidgetData>`. Construction calls
`Components::new_with_refreshed_list()` to enumerate sensors; stores all
component labels for the auto-detect heuristic. `update()` calls
`self.components.refresh()`, then finds the target component via the selection
heuristic (Package id 0 → CPU → first overall → empty-sensor fallback).

Auto-detect heuristic (private helper `fn select_component`):
```
1. If config.component is Some(label): find by case-insensitive exact match.
2. Else: first whose label.to_ascii_lowercase().contains("package id 0").
3. Else: first whose label.to_ascii_lowercase().contains("cpu").
4. Else: first component in the list.
5. Else (empty list): return Ok(WidgetData::Temperature { celsius: 0.0, label: "—".to_string(), critical: None })
```

On error: store stale `last` and return it (WIDGET_API §7.2). Return
`FramesError::SysInfo(...)` only if the list was non-empty on construction but
becomes empty on refresh (hardware removal — rare).

Add module-level `//!` doc comment and doc comments on all `pub` items.

Unit tests in `#[cfg(test)]`:
- `temperature_widget_name_returns_name`
- `temperature_widget_name_non_empty`
- `temperature_widget_satisfies_send_sync` (static assert via `fn assert_send_sync<T: Send + Sync>()`)
- `temperature_widget_empty_sensor_list_returns_zero` (construct with no real hardware → impossible to force; test the fallback path by calling `select_component` with an empty slice directly — make `select_component` `pub(crate)` for testability)

Verify: `cargo build -p frames_core`.

### Step 3 — `DiskWidget` core implementation
**File:** `crates/frames_core/src/widgets/disk.rs` (new file)

Implement `DiskWidget`. Fields: `name: String`, `mount_point: String`,
`disks: sysinfo::Disks`, `last: Option<WidgetData>`.

Construction calls `Disks::new_with_refreshed_list()`. `update()` calls
`self.disks.refresh_list()` then iterates to find the disk whose
`mount_point().to_str() == Some(&self.mount_point)`. Skip any disk where
`total_space() == 0`. On match, return:
```rust
Ok(WidgetData::Disk {
    used_bytes:  disk.total_space() - disk.available_space(),
    total_bytes: disk.total_space(),
    mount_point: self.mount_point.clone(),
})
```
On no match: return stale `last` if present, else a zeroed variant. This is
not an error — the mount may be temporarily unmounted.

Unit tests:
- `disk_widget_name_returns_name`
- `disk_widget_name_non_empty`
- `disk_widget_satisfies_send_sync`
- `disk_widget_default_mount_is_slash` — construct with `"/"`, verify `name()` is stable

Verify: `cargo build -p frames_core`.

### Step 4 — `ScriptWidget` core implementation
**File:** `crates/frames_core/src/widgets/script.rs` (new file)

Implement `ScriptWidget`. Fields: `name: String`, `command: String`,
`last: Option<WidgetData>`, `running: Arc<Mutex<bool>>`.

`update()` logic:
1. Lock `running`; if already `true` (previous spawn still in flight), return
   `last` immediately — no overlapping spawns.
2. Set `*running = true` and release the lock.
3. Spawn `Command::new("sh").arg("-c").arg(&self.command)`, capturing stdout.
4. Move `child.wait_with_output()` onto a dedicated OS thread:
   ```rust
   let (tx, rx) = std::sync::mpsc::channel();
   std::thread::spawn(move || { let _ = tx.send(child.wait_with_output()); });
   ```
5. `rx.recv_timeout(Duration::from_secs(5))`:
   - `Ok(Ok(output))` if exit status success and stdout non-empty → trim stdout
     → `WidgetData::Script { text }`.
   - `Ok(Ok(output))` with non-zero exit or empty stdout → `WidgetData::Script { text: String::new() }`.
   - `Err(RecvTimeoutError::Timeout)` → kill child (best-effort), return
     `WidgetData::Script { text: String::new() }`.
6. Set `*running = false`.
7. Store result in `self.last`; return it.

On `Command::new("sh")` spawn failure (extremely rare: `sh` not found):
return `FramesError::SysInfo(...)` — no `sh` is an unrecoverable misconfiguration.

Security note: `command` comes from user-controlled `config.toml`, the same
trust model as `on_click` / `on_scroll_*`. No sanitisation required beyond what
the shell itself provides. This is consistent with the existing `spawn_shell()`
in `main.rs`.

Unit tests:
- `script_widget_name_returns_name`
- `script_widget_name_non_empty`
- `script_widget_satisfies_send_sync`
- `script_widget_echo_returns_text` — `command = "echo hello"`, call `update()`,
  assert `text == "hello"` (requires a shell; skip in `--no-default-features` CI
  if needed, but `sh` is universally present on Linux. Mark with a comment.)
- `script_widget_nonzero_exit_returns_empty` — `command = "false"`, assert
  `text.is_empty()`

Verify: `cargo build -p frames_core`.

### Step 5 — Register new modules in `frames_core/src/widgets/mod.rs`
**File:** `crates/frames_core/src/widgets/mod.rs`

Add (in alphabetical order alongside existing entries):
```rust
pub mod disk;
pub mod script;
pub mod temperature;
```

Verify: `cargo build -p frames_core`.

### Step 6 — Add `WidgetConfig` fields for new widgets
**File:** `crates/frames_core/src/config.rs`

Add four new optional fields to `WidgetConfig`, each with `#[serde(default)]`
and a doc comment naming the widget(s) that use it, following the exact pattern
of the existing `latitude`/`longitude`/`units` fields:

```rust
/// Temperature: hardware sensor label to target (e.g. `"Package id 0"`).
/// Auto-detects the CPU package sensor when absent.
#[serde(default)]
pub component: Option<String>,

/// Disk: filesystem mount point to monitor. Default `"/"`.
#[serde(default)]
pub mount: Option<String>,

/// Script: shell command whose stdout is rendered as the widget label.
/// Required when `type = "script"`. Executed as `sh -c <command>`.
#[serde(default)]
pub command: Option<String>,

/// Active window: maximum title length (Unicode scalar values) before
/// truncation with `…`. Default 60.
#[serde(default)]
pub max_length: Option<usize>,
```

Add unit tests in the existing `#[cfg(test)]` block of `config.rs` confirming
each field deserialises correctly from TOML and defaults to `None` when absent.

Verify: `cargo build -p frames_core`.

### Step 7 — Config round-trip tests
**File:** `crates/frames_core/tests/config_roundtrip.rs`

Add tests (follow existing pattern of `weather_widget_config_parses_latitude_longitude_units`):
- `temperature_widget_config_parses_component`  
- `temperature_widget_config_defaults_when_component_absent`
- `disk_widget_config_parses_mount`  
- `disk_widget_config_defaults_when_mount_absent`
- `script_widget_config_parses_command`  
- `active_window_widget_config_parses_max_length`

Update the existing struct-literal tests that exhaustively list all `WidgetConfig`
fields (if any) to include the four new fields, each set to `None`.

Verify: `cargo test -p frames_core --no-default-features` — all tests pass.

### Step 8 — `frames_bar` temperature renderer
**File:** `crates/frames_bar/src/widgets/temperature.rs` (new file)

Pattern: identical to `weather.rs` and `brightness.rs`. `GtkLabel`, CSS classes
`.widget` and `.widget-temperature`.

`update()`:
```rust
if let WidgetData::Temperature { celsius, critical, .. } = data {
    let text = format!("🌡 {celsius:.0}°C");
    self.label.set_text(&text);

    let ctx = self.label.style_context();
    ctx.remove_class("warning");
    ctx.remove_class("urgent");
    if let Some(crit) = critical {
        if celsius >= &(crit - 10.0) { ctx.add_class("urgent"); }
        else if *celsius >= 80.0 { ctx.add_class("warning"); }
    } else if *celsius >= 80.0 {
        ctx.add_class("warning");
    }
}
```

Module-level `//!` comment. Doc comments on `new`, `widget`, `update`.
`#[allow(clippy::unnecessary_wraps)]` with explanatory comment on `new`.

Verify: `cargo build --workspace`.

### Step 9 — `frames_bar` disk renderer
**File:** `crates/frames_bar/src/widgets/disk.rs` (new file)

`update()` renders `format` config field with three modes:
- `"percent"` (default) → `"💾 45%"`
- `"used_gb"` → `"💾 120G / 512G"`
- `"free_gb"` → `"💾 392G free"`

Percent computation:
```rust
let pct = if total_bytes > 0 {
    // clippy::cast_precision_loss: display only; values < 2^53, precision acceptable
    #[allow(clippy::cast_precision_loss)]
    (used_bytes as f64 / total_bytes as f64 * 100.0) as u32
} else { 0 };
```

CSS state: adds `.warning` when disk > 80% full, `.urgent` when > 95% full.

Verify: `cargo build --workspace`.

### Step 10 — `frames_bar` script renderer
**File:** `crates/frames_bar/src/widgets/script.rs` (new file)

Simple renderer: `update()` sets `label.set_text(&text)`. Adds `.error` CSS
class when `text.is_empty()` and removes it otherwise. Renderer is infallible.

Verify: `cargo build --workspace`.

### Step 11 — `frames_bar` active window renderer
**File:** `crates/frames_bar/src/widgets/active_window.rs` (new file)

This renderer manages its own data collection (bar-only pattern, no `frames_core`
counterpart). Key design constraints per PLATFORM_COMPAT §3.1 and ADR-004:

- Uses `gdk::property_get()` for all X11 property reads.
- `gdk::Window::default_root_window()` for root window access (infallible).

`refresh()` implementation:
```rust
pub fn refresh(&self) {
    let root = gdk::Window::default_root_window();
    let display = root.display();

    // Read _NET_ACTIVE_WINDOW → u32 XID
    let active_xid: Option<u32> = gdk::property_get(
        &root,
        &gdk::Atom::intern("_NET_ACTIVE_WINDOW"),
        &gdk::Atom::intern("WINDOW"),
        0, 1, false,
    ).ok().and_then(|(_, data)| {
        // data is Vec<u8>; XID is 4 bytes little-endian on X11
        data.windows(4).next().map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    });

    let title: Option<String> = active_xid.and_then(|xid| {
        // Wrap XID as a GdkWindow
        let win = unsafe { gdk::Window::foreign_new_for_display(&display, xid as u64) }?;

        // Try _NET_WM_NAME (UTF-8) first
        gdk::property_get(
            &win,
            &gdk::Atom::intern("_NET_WM_NAME"),
            &gdk::Atom::intern("UTF8_STRING"),
            0, 256, false,
        ).ok()
        .and_then(|(_, data)| String::from_utf8(data).ok())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            // Fall back to WM_NAME (Latin-1)
            gdk::property_get(
                &win,
                &gdk::Atom::intern("WM_NAME"),
                &gdk::Atom::intern("STRING"),
                0, 256, false,
            ).ok()
            .and_then(|(_, data)| String::from_utf8_lossy(&data)
                .trim_end_matches('\0').to_string().into())
        })
    });

    let text = match title {
        Some(t) if !t.is_empty() => truncate(&t, self.max_length),
        _ => String::new(),
    };
    self.label.set_text(&text);
}
```

`truncate` is a private helper identical to the one in `media.rs` (copy, do not
import across widget modules — each renderer is self-contained per BAR_DESIGN §5.3).

`new()` does not schedule the timer — `build_widget` does, consistent with the
workspaces pattern.

Note on `gdk::Window::foreign_new_for_display`: this exists in gtk-rs 0.18 GDK
bindings but must be verified at compile time (Step 11 build check). If the binding
name differs, the alternative is unsafe FFI via `gdk_x11_window_foreign_new_for_display`
wrapped appropriately. Flag in the plan if this requires deviation.

Verify: `cargo build --workspace`.

### Step 12 — Register new bar widget modules
**File:** `crates/frames_bar/src/widgets/mod.rs`

Add (alphabetical insertion):
```rust
pub mod active_window;
pub mod disk;
pub mod script;
pub mod temperature;
```

Verify: `cargo build --workspace`.

### Step 13 — Wire new widgets into `build_widget` in `main.rs`
**File:** `crates/frames_bar/src/main.rs`

Add three `RendererDispatch` structs (following the exact pattern of `WeatherRenderer`):

```rust
struct TemperatureRenderer { name: String, renderer: widgets::temperature::TemperatureWidget }
struct DiskRenderer         { name: String, renderer: widgets::disk::DiskWidget }
struct ScriptRenderer       { name: String, renderer: widgets::script::ScriptWidget }
```

Each implements `RendererDispatch` with the standard one-line `dispatch` body.

Add four arms to `build_widget` (inside the `match config.widget_type.as_str()` block,
before the `other =>` fallback):

**`"temperature"` arm** (polled, standard):
```rust
"temperature" => {
    let interval = config.interval.unwrap_or(3000);
    let component_label = config.component.clone();
    let core_widget = frames_core::widgets::temperature::TemperatureWidget::new(
        &name, component_label,
    );
    poller.register(Box::new(core_widget), interval);
    let renderer = widgets::temperature::TemperatureWidget::new(config)
        .context("temperature renderer construction failed")?;
    add_to_bar(bar, renderer.widget(), config, &section);
    Ok((Some(Rc::new(TemperatureRenderer { name, renderer })), None))
}
```

**`"disk"` arm** (polled, standard):
```rust
"disk" => {
    let interval = config.interval.unwrap_or(30_000);
    let mount = config.mount.clone().unwrap_or_else(|| "/".to_string());
    let core_widget = frames_core::widgets::disk::DiskWidget::new(&name, mount);
    poller.register(Box::new(core_widget), interval);
    let renderer = widgets::disk::DiskWidget::new(config)
        .context("disk renderer construction failed")?;
    add_to_bar(bar, renderer.widget(), config, &section);
    Ok((Some(Rc::new(DiskRenderer { name, renderer })), None))
}
```

**`"script"` arm** (polled, standard):
```rust
"script" => {
    let interval = config.interval.unwrap_or(10_000);
    let command = config.command.clone().unwrap_or_default();
    let core_widget = frames_core::widgets::script::ScriptWidget::new(&name, command);
    poller.register(Box::new(core_widget), interval);
    let renderer = widgets::script::ScriptWidget::new(config)
        .context("script renderer construction failed")?;
    add_to_bar(bar, renderer.widget(), config, &section);
    Ok((Some(Rc::new(ScriptRenderer { name, renderer })), None))
}
```

**`"active_window"` arm** (bar-only self-poll, returns `(None, Some(source_id))`):
```rust
"active_window" => {
    use std::rc::Rc;
    let renderer = Rc::new(
        widgets::active_window::ActiveWindowWidget::new(config)
            .context("active_window renderer construction failed")?,
    );
    add_to_bar(bar, renderer.widget(), config, &section);

    // Initial fill before first timer tick.
    renderer.refresh();

    let renderer_clone = Rc::clone(&renderer);
    let interval_ms = config.interval.unwrap_or(200);
    let source_id =
        glib::timeout_add_local(Duration::from_millis(interval_ms), move || {
            renderer_clone.refresh();
            ControlFlow::Continue
        });

    // Does not participate in the Poller dispatch loop.
    Ok((None, Some(source_id)))
}
```

Verify: `cargo build --workspace`.

### Step 14 — Final tests pass
**Command:** `cargo test --workspace --no-default-features`

All 101+ pre-existing tests plus new tests from Steps 2–4 and Step 7 must pass.
The `script_widget_echo_returns_text` test runs with `sh` available (always true
on Fedora/Cinnamon) — no `--no-default-features` incompatibility.

Verify: all tests green.

### Step 15 — Clippy clean
**Command:** `cargo clippy --workspace -- -D warnings`

Zero warnings required. Common issues to pre-empt:
- `clippy::cast_precision_loss` on `used_bytes as f64` in disk renderer — suppress
  with explanatory comment per CODING_STANDARDS §1.4.
- `clippy::too_many_lines` on `build_widget` — already suppressed by the existing
  `#[allow]` comment; verify the count doesn't exceed a hard limit that would cause
  a new warning.
- `clippy::missing_panics_doc` on any `pub fn` that calls `.expect()` — ensure doc
  comments explain why the panic is unreachable.

### Step 16 — Standards documentation update
**Files:** `standards/WIDGET_API.md`, `standards/ARCHITECTURE.md`,
`standards/CONFIG_MODEL.md`, `DOCS/futures.md`

Per RULE_OF_LAW §4.2, all of the following must be updated in the same commit as
(or immediately after) Step 13:

| Document | Section | Change |
|----------|---------|--------|
| `WIDGET_API.md` §2 | Version string | Update to `"1.7.0"` |
| `WIDGET_API.md` §4 | WidgetData enum spec | Add `Temperature`, `Disk`, `Script` variant docs |
| `WIDGET_API.md` Changelog | New section | Add `### 1.7.0 (2026-03-18)` entry listing all three variants |
| `ARCHITECTURE.md` §4.1 | Module table | Add `temperature.rs`, `disk.rs`, `script.rs` rows |
| `ARCHITECTURE.md` §5 | Bar renderer table | Add corresponding renderer rows |
| `ARCHITECTURE.md` §4.2 | `WidgetData` enum block | Add the three new variants to the code block |
| `CONFIG_MODEL.md` §4.2 | Widget types table | Add rows for `"temperature"`, `"disk"`, `"script"`, `"active_window"` |
| `CONFIG_MODEL.md` §4.x | New sub-sections | Add §4.13–§4.16 documenting each widget's config fields |
| `DOCS/futures.md` | Completed section | Add completion entry for all four widgets with date |

*Note on `active_window` and WIDGET_API.md:* `active_window` adds no `WidgetData`
variant and no `Widget` implementor, so it does **not** trigger a `WIDGET_API_VERSION`
bump. It is a bar-only renderer. Record it in ARCHITECTURE.md §5 only.

### Step 17 — Final full CI
**Commands (in order):**
```bash
cargo test --workspace --no-default-features   # headless baseline
cargo clippy --workspace -- -D warnings        # zero warnings
cargo build --workspace                        # full workspace build
```

All three must pass before the plan is marked **Completed**.

---

## Widget API Impact

Three new `WidgetData` variants are added: `Temperature`, `Disk`, `Script`.

- All three are **non-breaking** additions under `#[non_exhaustive]`: existing bar
  `match data { ... }` arms with `_ => {}` continue to compile without modification.
- `WIDGET_API_VERSION` bumps: `"1.4.0"` → `"1.5.0"` → `"1.6.0"` → `"1.7.0"`
  (one minor bump per variant per WIDGET_API §2 policy; applied in Step 1).
- `WIDGET_API.md` update required in same commit as Step 16.

`active_window` adds **no `WidgetData` variant** and does **not** trigger a version bump.

---

## Error Handling Plan

No new `FramesError` variants are needed. Mapping:

| Widget | Error condition | Handling |
|--------|-----------------|---------|
| `TemperatureWidget` | Empty sensor list at construction | Returns `celsius: 0.0` — not an error |
| `TemperatureWidget` | Sensor disappears between polls | Returns stale `last` per WIDGET_API §7.2 |
| `DiskWidget` | Mount not found | Returns stale `last` or zeroed variant — not an error |
| `DiskWidget` | `total_space() == 0` | Skips the disk silently |
| `ScriptWidget` | Spawn failure (`sh` not found) | `FramesError::SysInfo(...)` — unrecoverable |
| `ScriptWidget` | Non-zero exit / empty output | `WidgetData::Script { text: String::new() }` |
| `ScriptWidget` | 5-second timeout | Kill child (best-effort), return empty text |
| `ScriptWidget` | Overlapping spawn | Return `last` immediately — no new spawn |
| `ActiveWindowWidget` | `_NET_ACTIVE_WINDOW` absent | Empty label — EWMH absent but PLATFORM_COMPAT §3.2 requires EWMH compliance |

No `.unwrap()` will be introduced. `Mutex::lock().expect("mutex poisoned")` is
acceptable per CODING_STANDARDS (one accepted exception; include invariant explanation
in comment).

---

## Test Plan

Per TESTING_GUIDE §3, every new function must have unit tests with at least three
cases: happy path, failure path, edge case.

| Test | Location | `--no-default-features` safe? |
|------|----------|-------------------------------|
| `temperature_widget_name_*` | `temperature.rs` `#[cfg(test)]` | ✓ |
| `temperature_widget_satisfies_send_sync` | same | ✓ |
| `temperature_empty_sensor_list_fallback` | same | ✓ |
| `disk_widget_name_*` | `disk.rs` `#[cfg(test)]` | ✓ |
| `disk_widget_satisfies_send_sync` | same | ✓ |
| `script_widget_name_*` | `script.rs` `#[cfg(test)]` | ✓ |
| `script_widget_satisfies_send_sync` | same | ✓ |
| `script_widget_echo_returns_text` | same | ✓ (requires `sh`; always present on Linux) |
| `script_widget_nonzero_exit_returns_empty` | same | ✓ |
| Config round-trip × 6 widgets | `tests/config_roundtrip.rs` | ✓ |
| Widget trait contract × 3 core types | `tests/widget_update.rs` | ✓ |

`active_window` requires GTK and a live X11 display. Per TESTING_GUIDE §5, any test
that touches GTK must perform a runtime display check and print `SKIP:` to stderr if
absent. No `frames_core` tests are added for `active_window`. Visual verification is
noted in the plan completion comment.

---

## Documentation Updates Required

Per RULE_OF_LAW §4.2:

| Code Change | Standard to Update |
|-------------|-------------------|
| New `WidgetData` variants (Temperature, Disk, Script) | `WIDGET_API.md` §2, §4, Changelog |
| New core modules (temperature, disk, script) | `ARCHITECTURE.md` §4.1, §4.2 |
| New bar renderers (temperature, disk, script, active_window) | `ARCHITECTURE.md` §5 |
| New widget type strings | `CONFIG_MODEL.md` §4.2, new §4.13–§4.16 |
| Completed items | `DOCS/futures.md` Completed section |

---

## futures.md Impact

### Entries this plan completes
- `DOCS/research/widget-candidates.md` Options A (temperature), B (disk), C (active_window), D (script) — all Priority-1 items

### New debt this plan creates
- `active_window` adds a second instance of the bar-only self-poll pattern alongside
  workspaces. The architectural inconsistency (bypassing the Poller pipeline) is noted
  in `futures.md` as a companion to the existing workspaces entry.
- `ScriptWidget` timeout implementation uses a detached OS thread per invocation.
  If `interval` < 5 s and the command always times out, threads accumulate. Document
  as a known limitation until a proper `wait_timeout` solution is available in stable
  Rust stdlib.

---

## Risks & Open Questions

| Risk | Severity | Mitigation |
|------|----------|------------|
| `gdk::Window::foreign_new_for_display` binding name differs in gtk-rs 0.18 | Medium | Verify at Step 11 compile; fall back to `unsafe { gdk_sys::gdk_x11_window_foreign_new_for_display(...) }` if necessary; flag deviation before implementing |
| `sysinfo::Components` empty on VMs / Docker (TESTING_GUIDE §5 note about `/sys/class/hwmon`) | Low | Handled: Step 2 returns `celsius: 0.0` on empty list; unit test covers this path |
| `DiskWidget::refresh_list()` performance — re-scanning all mounts per poll | Low | Default interval 30 s; cost is negligible at that frequency |
| `build_widget` `too_many_lines` clippy lint threshold exceeded after +4 arms | Low | The existing `#[allow(clippy::too_many_lines)]` suppression already covers this; verify it is still in place at Step 15 |
| `active_window` shows stale title during rapid window switching at 200 ms default interval | Negligible | Acceptable UX — documented default; user may reduce `interval` |
| `ScriptWidget` with `command = ""` (absent field defaulting to empty string) | Low | Empty command passed to `sh -c ""` exits 0 with no output → `text: ""` → empty label. Add guard in `update()`: if `self.command.is_empty()` return `Ok(WidgetData::Script { text: String::new() })` immediately without spawning |
