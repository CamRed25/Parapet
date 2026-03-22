# Plan: `hover_delay_ms` for `LauncherWidget`

**Status:** Completed
**Date:** 2026-03-22
**Standards consulted:** RULE_OF_LAW.md, CODING_STANDARDS.md §1–4, CONFIG_MODEL.md §4.1 §4.2 §5, WIDGET_API.md §1 §3, BAR_DESIGN.md §5.4 §6, PLATFORM_COMPAT.md §2, TESTING_GUIDE.md §2 §3 §5, DECISIONS.md ADR-005 ADR-006

---

## Overview

Add a configurable `hover_delay_ms` field to `LauncherConfig` so users can tune (or disable) the delay before the launcher dropdown opens on cursor-enter. Currently the dropdown opens instantly on hover; this causes accidental opens when the cursor passes over the launcher button while moving to another part of the screen.

**Default:** 150 ms — perceptually instant on deliberate hover, eliminates most accidental opens without requiring user configuration. `0` restores the current instant-open behavior.

This is a self-contained `parapet_core` config field addition (`Option<u32>`) plus a `parapet_bar` signal-handler change. No `Widget` trait or `WidgetData` enum is involved. No WIDGET_API_VERSION bump is required.

---

## Affected Crates & Modules

| Crate | File | Nature of Change |
|-------|------|-----------------|
| `parapet_core` | `src/config.rs` | Add `hover_delay_ms: Option<u32>` to `LauncherConfig` |
| `parapet_bar` | `src/widgets/launcher.rs` | Add `open_timer` cell; defer open in `connect_enter_notify_event`; cancel open on leave |
| *(standards)* | `standards/CONFIG_MODEL.md` | Add §4.14 Launcher Widget Fields subsection |
| *(docs)* | `DOCS/futures.md` | Close the `hover_delay_ms` debt entry |

**No new external crate dependencies.** `glib::timeout_add_local_once` and `glib::SourceId` are already in use for the `close_timer` pattern. No `BUILD_GUIDE.md` update needed.

**No new `ARCHITECTURE.md` modules.** This is an addition to an existing widget renderer.

---

## New Types & Signatures

### `parapet_core::config::LauncherConfig` (modified)

New field added:
```rust
/// Milliseconds to wait after the cursor enters the launcher button before
/// the dropdown opens. Set to `0` to open immediately (previous behaviour).
/// Default: `150`.
///
/// A short delay prevents accidental opens when the cursor passes over the
/// button while moving to another part of the screen.
#[serde(default)]
pub hover_delay_ms: Option<u32>,
```

No new structs, enums, or traits. No `ParapetError` variants needed; this field cannot fail validation (any `u32` is a valid delay). No `validate()` rule needed — `Option<u32>` with a sane default cannot be invalid.

### `parapet_bar::widgets::launcher` (modified)

`wire_dropdown()` gains one parameter:
```rust
fn wire_dropdown(
    button: &gtk::Button,
    apps: &Rc<RefCell<Vec<gio::AppInfo>>>,
    corpus: &Rc<RefCell<Vec<AppSearchData>>>,
    pinned: &Rc<Vec<String>>,
    max_results: usize,
    popup_width: i32,
    popup_min_height: i32,
    hover_delay_ms: u64,   // ← new; 0 = open immediately
)
```

New private function (mirrors `cancel_close_timer`):
```rust
/// Cancel a pending hover-open timer started by the enter-notify handler.
///
/// Prevents the dropdown from opening if the cursor leaves the button before
/// the delay expires (accidental drive-by hover). Safe to call when no timer
/// is pending.
fn cancel_open_timer(timer: &Rc<RefCell<Option<glib::SourceId>>>) { ... }
```

---

## Implementation Steps

Each step must compile (`cargo build --workspace`) before the next begins.

### Step 1 — Add `hover_delay_ms` to `LauncherConfig` in `parapet_core/src/config.rs`

- Insert `hover_delay_ms: Option<u32>` field with `#[serde(default)]` and `///` doc comment into `LauncherConfig`
- Field order: after `pinned: Vec<String>` (last existing field)
- No `validate()` change needed — any `u32` (or absent) is valid

Doc comment (required per CODING_STANDARDS §3):
```rust
/// Milliseconds to wait after the cursor enters the launcher button before
/// the dropdown opens. Set to `0` to open immediately (previous behaviour).
/// Default: `150`.
#[serde(default)]
pub hover_delay_ms: Option<u32>,
```

**Documentation update:** This step adds a config field — CONFIG_MODEL.md §4 must be updated in the same commit (RULE_OF_LAW §4.2). Add a new **§4.14 Launcher Widget Fields** subsection after §4.13 Disk. Include a TOML example and the field table. (See Documentation Updates Required section below for the exact text.)

Build check: `cargo build --workspace`

---

### Step 2 — Add `open_timer` cell to `wire_dropdown()` in `launcher.rs`

- Add `hover_delay_ms: u64` as the last parameter of `wire_dropdown()`
- Inside `wire_dropdown()`, immediately after the existing `close_timer` declaration, add:
  ```rust
  let open_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
  ```
- Add `cancel_open_timer()` private function at the bottom of the file immediately after `cancel_close_timer()`, following identical structure

The `cancel_open_timer` function must have a `///` doc comment covering purpose and parameters (CODING_STANDARDS §3).

Build check: `cargo build --workspace`

---

### Step 3 — Replace immediate open with delayed open in `connect_enter_notify_event`

Current hover-enter handler in `wire_dropdown()`:
```rust
button.connect_enter_notify_event(move |btn, ev| {
    if ev.detail() == gdk::NotifyType::Inferior { return ...; }
    cancel_close_timer(&close_timer);
    if !dropdown.is_visible() {
        open_dropdown(btn, ...);
    }
    glib::Propagation::Proceed
});
```

Replace with:
```rust
button.connect_enter_notify_event(move |btn, ev| {
    if ev.detail() == gdk::NotifyType::Inferior { return ...; }
    cancel_close_timer(&close_timer);
    cancel_open_timer(&open_timer);       // cancel any in-flight open timer
    if !dropdown.is_visible() {
        if hover_delay_ms == 0 {
            open_dropdown(btn, ...);       // zero delay: immediate (old path)
        } else {
            // Start pending open timer; will fire after hover_delay_ms if
            // cursor remains over the button.
            let btn = btn.clone();
            // (clone all Rc args — same pattern as existing click handler)
            let open_timer_clone = Rc::clone(&open_timer);
            let id = glib::timeout_add_local_once(
                Duration::from_millis(hover_delay_ms),
                move || {
                    open_dropdown(&btn, ...);
                    *open_timer_clone.borrow_mut() = None;
                },
            );
            *open_timer.borrow_mut() = Some(id);
        }
    }
    glib::Propagation::Proceed
});
```

**Correctness requirement (RULE_OF_LAW §3.1):** All `Rc` values captured in the delayed closure must be clones made *outside* the `timeout_add_local_once` call, exactly as the click handler does it. The closure captures: `btn` clone, `dropdown` clone, `list` clone, `search` clone, `apps` clone, `corpus` clone, `pinned` clone, `matcher` clone, `open_timer_clone`.

Build check: `cargo build --workspace`

---

### Step 4 — Cancel `open_timer` in `connect_leave_notify_event` on the button

Current leave handler:
```rust
button.connect_leave_notify_event(move |_, ev| {
    if ev.detail() == gdk::NotifyType::Inferior { return ...; }
    schedule_close_dropdown(&dropdown, &close_timer);
    glib::Propagation::Proceed
});
```

Add `cancel_open_timer(&open_timer);` **before** `schedule_close_dropdown(...)`:
```rust
button.connect_leave_notify_event(move |_, ev| {
    if ev.detail() == gdk::NotifyType::Inferior { return ...; }
    cancel_open_timer(&open_timer);                      // ← new
    schedule_close_dropdown(&dropdown, &close_timer);
    glib::Propagation::Proceed
});
```

This is the critical correctness step: prevents the dropdown from opening if the cursor leaves before the delay expires.

Build check: `cargo build --workspace`

---

### Step 5 — Thread `hover_delay_ms` from `LauncherWidget::new()` to `wire_dropdown()`

In `LauncherWidget::new()`, the `wire_dropdown()` call site must pass the delay:
```rust
wire_dropdown(
    &button,
    &apps,
    &corpus,
    &pinned,
    max_results,
    popup_width,
    popup_min_height,
    config.hover_delay_ms.unwrap_or(150) as u64,   // ← new, default 150 ms
);
```

The `unwrap_or(150)` default must live here, not inside `wire_dropdown`, so the default is co-located with the config field read-site.

Build check: `cargo build --workspace`

---

### Step 6 — Update `wire_dropdown()` `///` doc comment

Per CODING_STANDARDS §3, every `pub fn` (and significant private fn) requires a doc comment covering purpose, parameters, return value, and side effects. `wire_dropdown` has existing doc — update it to document the new `hover_delay_ms` parameter:

```rust
/// Wire all dropdown open/close behaviour onto `button`.
///
/// # Parameters
///
/// - `hover_delay_ms`: milliseconds to wait after cursor-enter before opening
///   the dropdown. `0` opens immediately. The leave-notify handler cancels the
///   pending timer, so drive-by hover never opens the dropdown.
```

Build check: `cargo build --workspace`

---

### Step 7 — Update `standards/CONFIG_MODEL.md`: add §4.14 Launcher Widget Fields

Per RULE_OF_LAW §4.2, config field additions require a `CONFIG_MODEL.md` update in the same commit.

Add a new **§4.14** subsection after §4.13 (Disk Widget Fields):

```markdown
### 4.14 Launcher Widget Fields

\`\`\`toml
[[widgets]]
type = "launcher"
position = "left"
button_label = "Apps"    # text shown on the bar button
max_results = 10         # maximum rows in the dropdown list
popup_width = 280        # dropdown window width in pixels
popup_min_height = 200   # minimum dropdown list height in pixels
hover_delay_ms = 150     # ms before dropdown opens on hover; 0 = immediate
pinned = ["firefox", "kitty", "code"]
\`\`\`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `button_label` | string | `"Apps"` | Text shown on the launcher button in the bar |
| `max_results` | integer | `10` | Maximum number of application rows shown in the dropdown |
| `popup_width` | integer (px) | `280` | Width of the dropdown popup window in pixels |
| `popup_min_height` | integer (px) | `200` | Minimum height of the scrollable application list in pixels |
| `hover_delay_ms` | integer (ms) | `150` | Milliseconds to wait after the cursor enters the launcher button before the dropdown opens. Set to `0` to open immediately |
| `pinned` | array of strings | `[]` | Desktop ID stems (without `.desktop` suffix) shown at the top of the list regardless of search query. E.g. `["firefox", "kitty"]` |
```

Build check: `cargo build --workspace`

---

### Step 8 — Update `DOCS/futures.md`: close the `hover_delay_ms` debt entry

The debt entry (2026-03-21) reads:
> `LauncherConfig.hover_delay_ms: Option<u32>` — configurable delay before the launcher dropdown opens on hover...

Strike through and append completion annotation following the established pattern for completed items (match existing `VolumeWidget` and other completed entries). Move the entry to the Completed section or append `~~...~~ **Completed — see Completed section.**` inline.

---

### Final — Validation

```bash
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace --no-default-features
```

All three must pass before the plan is marked Completed.

---

## Widget API Impact

No `Widget` trait methods changed. No `WidgetData` variants added or modified. **No WIDGET_API_VERSION bump required.**

---

## Error Handling Plan

- `hover_delay_ms: Option<u32>` — plain serde field, no error path. Missing = `None` = use default 150 ms in `main.rs`. Any `u32` is valid; no `validate()` rule needed.
- `glib::timeout_add_local_once` cannot fail (it registers on the main loop, which is always alive when `wire_dropdown` is called). No error to propagate.
- No `.unwrap()` introduced. `Rc::clone` and `borrow_mut()` are infallible in single-threaded GTK context.

---

## Test Plan

Per TESTING_GUIDE §2 §3:

### Unit tests in `parapet_core/src/config.rs` (headless, `--no-default-features`)

1. `launcher_hover_delay_defaults_to_none` — parse TOML with no `hover_delay_ms` field; assert `config.hover_delay_ms == None`
2. `launcher_hover_delay_explicit_zero` — parse TOML with `hover_delay_ms = 0`; assert `== Some(0)`
3. `launcher_hover_delay_explicit_value` — parse TOML with `hover_delay_ms = 250`; assert `== Some(250)`
4. `launcher_hover_delay_round_trips` — serialize a `LauncherConfig` with `hover_delay_ms = Some(100)` and deserialize; assert field preserved

All four run headlessly under `--no-default-features`.

### GTK display tests

`cancel_open_timer` is a private timer helper. GTK signal wiring is not testable headlessly (TESTING_GUIDE §5). No display tests are required for this change — the critical logic is the `cancel_open_timer` call in the leave handler, which is verified by the timer pattern unit tests and by manual visual verification.

**Visual verification checklist:**
- Bar running with default config (`hover_delay_ms` absent)
- Slow deliberate hover over launcher button → dropdown opens after ~150 ms
- Fast drive-by cursor over button → dropdown does **not** open
- `hover_delay_ms = 0` in config → dropdown opens instantly on cursor-enter (old behavior)
- Leave button before delay expires → no dropdown appears

---

## Documentation Updates Required

Per RULE_OF_LAW §4.2:

| Code Change | Standard to Update | Step # |
|-------------|-------------------|--------|
| New `LauncherConfig` field | `CONFIG_MODEL.md §4` — new §4.14 subsection | Step 7 |
| `futures.md` debt closure | `DOCS/futures.md` | Step 8 |

No `ARCHITECTURE.md`, `BUILD_GUIDE.md`, or `WIDGET_API.md` updates needed.

---

## futures.md Impact

**Closes:** `(2026-03-21) LauncherConfig.hover_delay_ms: Option<u32>` debt entry.

**New debt created:** None. This is a leaf feature with no follow-on work.

---

## Risks & Open Questions

| Risk | Mitigation |
|------|-----------|
| `open_timer` SourceId leak if `LauncherWidget` is dropped with a pending timer | `Rc` holding the `open_timer` cell is captured by the closure. When the GTK widget tree is destroyed and all closures released, the `Rc` drops to zero; `glib` removes orphaned one-shot sources automatically. Same lifecycle as `close_timer` — already proven safe. |
| Rapid enter→leave→enter sequences stacking multiple timers | `cancel_open_timer()` is called at the start of every enter-notify handler, so at most one pending timer exists at any time. |
| `Inferior` crossing events (GDK child-widget enters) firing the timer | Both enter handlers already guard `ev.detail() == NotifyType::Inferior`. The `open_timer` start is inside that guard. |
| Default 150 ms feels too slow for some users | Default is a starting point; `hover_delay_ms = 0` fully restores old behavior. No wrong default — user can adjust. |
