# Research: Project Bootstrap — Getting Started
**Date:** 2026-03-17
**Status:** Findings complete — decision pending

## Question

What is the correct sequence of steps to bootstrap the Frames codebase from zero code (standards only) to a buildable, testable skeleton?

## Summary

The project has fully-defined standards and architecture but no code at all. The correct path is: scaffold the workspace files first, build `frames_core` module by module before touching GTK, then wire up `frames_bar` once the core library compiles cleanly. The clock widget is the best first widget — it has no sysfs dependencies and immediately proves the full data-flow pipeline.

---

## Findings

### Current State

The workspace contains:
- `standards/` — 11 authoritative `.md` files covering architecture, widget API, build, testing, platform, coding style, etc.
- `.claude/` and `.github/` — agent/copilot configuration

**Missing entirely:** `Cargo.toml`, `crates/`, `DOCS/`, `justfile`, `rustfmt.toml`, any `.rs` files.

Every architectural decision has already been made in standards. Nothing needs to be re-designed — only implemented.

---

### Phase 1 — Workspace Scaffold (do this first)

These files have no code dependencies. Create them before writing any Rust.

| File | Source | Notes |
|------|--------|-------|
| `Cargo.toml` | BUILD_GUIDE §2.1 | Workspace root; exact template in the standard |
| `crates/frames_core/Cargo.toml` | BUILD_GUIDE §2.2 | Template in the standard |
| `crates/frames_bar/Cargo.toml` | BUILD_GUIDE §2.2 | Template in the standard |
| `rustfmt.toml` | CODING_STANDARDS §1.3 | Exact content in the standard |
| `justfile` | BUILD_GUIDE §1.3 | Recipes: `check`, `check-headless`, `install-hooks` |
| `.cargo/config.toml` | (if needed) | Target-specific flags; defer unless CI requires it |

**Verification:** `cargo build --workspace` with empty `lib.rs`/`main.rs` stubs must exit 0.

---

### Phase 2 — `frames_core` Implementation Order

Build bottom-up. Each module depends only on modules above it in this list.

#### Step 1: `error.rs`
Defines `FramesError`. Everything else in the crate returns this type. Write it first so `?` is available everywhere.

```
FramesError variants to implement (ARCHITECTURE §4.5):
  Config(#[from] ConfigError)
  SysInfo(String)
  Battery(#[from] std::io::Error)
  WidgetNotFound { name: String }
```

Unit test: confirm each variant's `Display` output matches the `#[error(...)]` string.

#### Step 2: `widget.rs`
Defines the `Widget` trait and `WidgetData` enum. This is the contract between `frames_core` and `frames_bar` — nothing in either crate works without it.

Key decisions (already made in WIDGET_API.md):
- `WidgetData` is `#[non_exhaustive]` — add the attribute from day one
- `BatteryStatus` enum lives here too
- `WIDGET_API_VERSION` const must be present

Unit tests: a `MinimalWidget` stub that satisfies the contract (see TESTING_GUIDE §2.3 for the exact pattern).

#### Step 3: `config.rs`
Defines `FramesConfig`, `BarConfig`, `WidgetConfig` with serde/toml. The TOML config file path is `~/.config/frames/config.toml`.

Design (from ARCHITECTURE §4.4 and CONFIG_MODEL.md):
- `BarConfig` fields: `position`, `height`, `monitor`, CSS path
- `WidgetConfig` fields: `type`, `name`, `interval_ms`, optional per-widget fields
- `BarConfig` must implement `Default` — tests use defaults when optional fields absent

Unit tests (TESTING_GUIDE §2.1): valid TOML, defaults when optional fields absent, config round-trip serialize→deserialize.

#### Step 4: `widgets/clock.rs`
The simplest widget — uses `chrono`, no sysfs, no sysinfo. Implement first so the full Widget→WidgetData pipeline can be tested in isolation.

Returns `WidgetData::Clock { display: String }` formatted via a `format` config field (e.g. `"%H:%M:%S"`).

Unit test: `update()` returns a non-empty display string; format string is respected.

#### Step 5: `widgets/cpu.rs`, `widgets/memory.rs`, `widgets/network.rs`
These three all use `sysinfo`. Implement together — they share the `sysinfo::System` refresh lifecycle.

**Important:** `sysinfo::System` must be refreshed before reading CPU data. Per sysinfo docs, CPU usage requires two refresh calls separated by a delay to compute a delta. The first call after construction returns 0% — this is expected and the widget must handle it gracefully (return stale or zero data, not an error).

Unit test: `update()` returns `WidgetData::Cpu/Memory/Network` without panic. Exact values cannot be asserted (hardware-dependent) but the types and valid ranges can be.

#### Step 6: `widgets/battery.rs`
Reads `/sys/class/power_supply/`. Yields `WidgetData::Battery { charge_pct: Option<f32>, status: BatteryStatus }`.

`charge_pct` is `Option<f32>` — `None` when no battery is present (desktop machine). The widget must not fail on a desktop — it must return `WidgetData::Battery { charge_pct: None, status: BatteryStatus::Full }`.

Unit test: uses `tempfile` to create a fake `/sys/class/power_supply/` tree and asserts correct parsing.

#### Step 7: `widgets/workspaces.rs`
A stub returning a static `WidgetData::Workspaces { count: 1, active: 0, names: vec![] }`. Real X11 EWMH query lives in `frames_bar` — core only holds the data model. This stub satisfies the `Widget` trait contract so `frames_bar` can receive workspace data.

#### Step 8: `poll.rs`
`Poller` drives widget updates. Per ARCHITECTURE §4.3:
- Pure Rust struct — no GTK, no glib timers
- Called on the glib main thread from `frames_bar`
- `poll()` returns `Vec<(String, WidgetData)>` for widgets whose interval has elapsed

Unit test: mock widgets with known intervals, advance a fake clock, assert which widgets are called on each tick.

---

### Phase 3 — `frames_bar` Implementation Order

Start only after `frames_core` compiles cleanly and its tests pass with `--no-default-features`.

#### Step 1: `main.rs` stub
GTK init, config load, `gtk::main()`. No widgets rendered yet. Goal: a window appears.

#### Step 2: `bar.rs`
The `Bar` struct (BAR_DESIGN §2):
- `gtk::WindowType::Popup`
- `TypeHint::Dock`
- EWMH properties via GDK X11 after `realize`
- `_NET_WM_STRUT_PARTIAL` using the 12-value array from BAR_DESIGN §3

This is the hardest part of `frames_bar`. Key subtlety: strut must be set **after** `realize` fires, because the GDK window doesn't have an X11 ID until then. Connect to the `realize` signal:

```rust
window.connect_realize(move |w| {
    apply_strut(w.window().unwrap(), &bar_config, &monitor_geom);
});
```

#### Step 3: Widget renderers (one at a time)
Start with `widgets/clock.rs` — a `gtk::Label` updated by `Poller`. Then add CPU/memory/network/battery. Workspace buttons last (they require X11 EWMH queries).

Per ARCHITECTURE §5 rules: no business logic in renderers. A renderer receives `&WidgetData` and calls `label.set_text(...)` or `progressbar.set_value(...)` only.

#### Step 4: `css.rs`
Load a user CSS file (path from config) via `gtk::CssProvider`. Fall back to a built-in minimal stylesheet so the bar is visible even without a user theme.

---

### Option A: Clock-first vertical slice (Recommended)

Implement the minimum end-to-end pipeline before adding more widgets:

1. Scaffold workspace
2. `frames_core`: `error.rs` → `widget.rs` → `config.rs` → `clock.rs` → `poll.rs`
3. `frames_bar`: `main.rs` → `bar.rs` → `widgets/clock.rs` → `css.rs`
4. Verify: bar appears, clock ticks, struts reserve screen space
5. Then add remaining widgets one at a time

This proves the architecture before committing to more implementation work.

### Option B: Full `frames_core` first, then full `frames_bar`

Build all core widgets before starting GTK work. Safer for CI (headless tests pass first) but delays real visual feedback.

---

## Recommendation

**Option A — clock-first vertical slice.** Complete the end-to-end data flow with the simplest widget before expanding. This surfaces integration issues (GTK init, strut setup, Poller wiring) early when the surface area is small. Per RULE_OF_LAW §3.1, a working build is the first gate — a vertical slice satisfies that requirement faster.

Implementation sequence:
1. **Phase 1:** Workspace scaffold (`Cargo.toml`, `rustfmt.toml`, `justfile`)
2. **Phase 2 subset:** `error.rs` → `widget.rs` → `config.rs` → `clock.rs` → `poll.rs`
3. **Phase 3 subset:** `main.rs` stub → `bar.rs` → `widgets/clock.rs` → `css.rs`
4. Verify bar window appears with working clock and correct struts
5. Expand: add remaining `frames_core` widgets, then their `frames_bar` renderers

The `Planner` agent can generate a detailed step-by-step plan from here. Once planning is approved, code writing can begin.

---

## Standards Conflict / Proposed Update

None identified. All architectural decisions are already captured in standards. The BUILD_GUIDE has the exact `Cargo.toml` templates needed for Phase 1. ARCHITECTURE.md has the module structures. BAR_DESIGN.md has the full strut property setup.

One gap to note: `DOCS/DECISIONS.md` does not exist yet. The first architectural decisions to record there:
- GTK3 over GTK4 (already justified in PLATFORM_COMPAT §4)
- X11-only scope (already justified in PLATFORM_COMPAT §3)
- `sysinfo` as the system data crate (documented in ARCHITECTURE §3)

---

## Sources

- `standards/ARCHITECTURE.md` — crate graph, module structure, startup sequence, data flow
- `standards/WIDGET_API.md` — Widget trait contract, WidgetData enum, versioning policy
- `standards/BUILD_GUIDE.md` — workspace Cargo.toml templates, crate templates, build steps
- `standards/BAR_DESIGN.md` — GTK3 window properties, EWMH strut setup, positioning
- `standards/TESTING_GUIDE.md` — test structure, headless policy, widget contract tests
- `standards/RULE_OF_LAW.md` — standards hierarchy, build-must-pass, fix-don't-skip
- `standards/CODING_STANDARDS.md` — Rust edition, MSRV, clippy config, rustfmt config
- `standards/PLATFORM_COMPAT.md` — X11 requirements, GTK version floor

---

## Open Questions

1. **`justfile` content** — The standards reference `just check` and `just check-headless` but do not define the exact recipe contents. These need to be written before CI is set up.
2. **Monitor detection** — `bar.rs` needs to detect which monitor to display on. ARCHITECTURE §6.1 mentions `BarConfig.monitor` but BAR_DESIGN §4 (positioning) was only partially read. Verify multi-monitor positioning logic before implementing `Bar::new()`.
3. **Config default path** — Does `config.rs` create `~/.config/frames/config.toml` on first run, or fail and print a helpful error? RULE_OF_LAW §3.5 (silent failure not acceptable) implies a helpful error is correct. CONFIG_MODEL.md should be read before implementing `config.rs`.
4. **notify watcher** — Config hot-reload via the `notify` crate is listed in the dependency graph. Whether this is needed in the initial vertical slice or can be deferred should be decided before writing `config.rs`.
