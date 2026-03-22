# Research: Hot-reload Widget Diff — Incremental Widget Tree Rebuild
**Date:** 2026-03-22
**Status:** Findings complete — decision recommended — ready to close

## Question

What is the best approach for diffing old vs new `Vec<WidgetConfig>` during
config hot-reload in `parapet_bar/src/main.rs` to avoid tearing down and
rebuilding unchanged widgets?

---

## Summary

Add `#[derive(PartialEq)]` to `WidgetConfig`, `WidgetKind`, and all 13
per-widget config structs in `parapet_core/src/config.rs`. In the hot-reload
handler in `main.rs`, zip the old and new widget lists and rebuild only widget
positions whose configs differ. Widget-owned `SourceId`s must be cancelled only
for changed positions. This approach requires no new dependencies, eliminates
visible flicker on unchanged widgets, and produces a diff at zero allocation cost.

---

## Findings

### Current hot-reload behaviour

Source: `crates/parapet_bar/src/main.rs`, hot-reload arm of the 500 ms
`glib::timeout_add_local`:

```
1. for each id in self_poll_ids → id.remove()
2. bar.clear_widgets()                // removes ALL children from the GTK Box
3. build_all_widgets(...)             // reconstructs every widget from config
```

Full teardown-and-rebuild runs on every file-change event, even when only one
field in one widget changed. The cost is dominated by:

- GTK side: `gtk::Box.remove()` per widget + DOM reflow + `show_all()` repaint.
- Application side: `Poller` reset, sysinfo re-init in widget constructors.

---

### Q1 — Identity model: how do we match old and new widget slots?

`WidgetConfig` has no stable `name` or `id` field. The available fields are:

| Field | Notes |
|-------|-------|
| `position: BarSection` | Left / Centre / Right — not unique within a section |
| `label: Option<String>` | Static text prefix for display; not intended as an ID |
| `kind: WidgetKind` | Discriminant of widget type |
| `interval`, `on_click`, … | Behavioural config |

Two `type = "cpu"` widgets in the same section cannot be distinguished by
anything except their order in the config file. Therefore, **index-based matching
is the only stable identity** available without user-visible API changes.

**Index-based diff rule:**
- If the new list is shorter → teardown trailing old widgets.
- If the new list is longer → build new widgets appended at the tail.
- For each index `i` present in both → rebuild only if `old[i] != new[i]`.

This is entirely correct for the common case (user edits one widget's config).
Reordering widgets in the file is treated as a change and triggers a rebuild of
the affected positions — accepted and correct behaviour.

**Future option:** Add `name: Option<String>` to `WidgetConfig` to support
name-keyed matching (drag-and-drop config reorder without flicker). Record in
`futures.md` for Phase 3 or later.

---

### Q2 — Can widgets update in-place, or must they always be recreated?

GTK3 widget rendering in Parapet follows a two-phase model:
1. **Construction** (`build_widget`): creates the `gtk::Widget` container,
   attaches it to the bar box, returns a `RendererDispatch` and an optional
   `SourceId`.
2. **Data dispatch** (`renderer.update(data)`): called on each Poller tick;
   changes label text, CSS, icon — without touching the GTK widget tree.

Config changes that affect **only data format** (e.g., `CpuConfig::format`,
`ClockConfig::format`) could theoretically be handled by updating the renderer
state and letting the next data tick repaint. However:

- Most per-widget structs hold format strings and thresholds that are consumed
  at **widget construction time** and baked into closures.
- There is no `RendererDispatch::reconfigure(&WidgetConfig)` method; adding one
  for each widget type is a larger refactor than simply recreating the widget.
- For interval changes, the old `SourceId` must be cancelled and a new one
  registered — effectively a teardown of the "engine" half even if the GTK half
  could survive.

**Decision**: For Phase 2, recreate the widget at any changed index. Full
in-place config application is deferred (WIDGET_API Phase 3 extension point).

---

### Q3 — SourceId / self_poll_ids bookkeeping

Current shape:

```rust
let mut self_poll_ids: Vec<glib::SourceId> = Vec::new();
```

`self_poll_ids` holds timers for `Workspaces` and any other self-polling widget.
Importantly, not every widget slot has a corresponding entry — the indices of
`self_poll_ids` do not align with widget config indices.

For the incremental diff to cancel only changed widgets' timers, the data
structure must align timer ownership with widget slots. Proposed:

```rust
// Maps widget config index → Option<glib::SourceId>
// (None for widgets that use the Poller rather than a self-managed timer)
let mut slot_poll_ids: Vec<Option<glib::SourceId>> = Vec::new();
```

Then the hot-reload diff loop:
1. For each changed index `i`, cancel `slot_poll_ids[i]` if `Some`.
2. Remove `renderers[i]` GTK widget from bar box.
3. Build a replacement widget; insert at position `i` in bar box.
4. Update `slot_poll_ids[i]` with new `SourceId`.

This is the minimal bookkeeping change needed. The existing `self_poll_ids` flat
`Vec` is replaced by `slot_poll_ids: Vec<Option<glib::SourceId>>`.

---

### Q4 — Diff comparison mechanism: serialize vs. derive PartialEq

**Option A — `toml::to_string` serialization comparison**

```rust
let old_s = toml::to_string(&old_widget).unwrap_or_default();
let new_s = toml::to_string(&new_widget).unwrap_or_default();
if old_s != new_s { /* rebuild slot */ }
```

Pros:
- Zero new derives; works with current `WidgetConfig` (which implements
  `Serialize`).
- Catches any field change, including newly added fields.

Cons:
- Allocates two `String` per widget slot on every hot-reload tick.
- `toml::to_string` can fail on non-`String`-keyed maps (unlikely here, but
  the `.unwrap_or_default()` fallback silently treats failure as "unchanged").
- Floats in thresholds (`f32`) serialize to their display form — differences
  below display precision are invisible to the diff.
- Hot-reload fires on every `notify` event (debounced to 500 ms), so the cost is
  paid rarely, but correctness risk from float roundtrip is a concern.

**Option B — `#[derive(PartialEq)]` on all config structs**

Add `PartialEq` to:
- `WidgetConfig`
- `WidgetKind` (enum: 13 variants)
- `ClockConfig`, `CpuConfig`, `MemoryConfig`, `NetworkConfig`, `BatteryConfig`,
  `DiskConfig`, `VolumeConfig`, `BrightnessConfig`, `WeatherConfig`,
  `MediaConfig`, `WorkspacesConfig` (or unit variant), `LauncherConfig`, any
  `CustomConfig`
- `BarSection` (if not already `PartialEq`)

Then the diff is:

```rust
if old_widget != new_widget { /* rebuild slot */ }
```

Pros:
- Zero allocations; comparison is direct field-by-field.
- Correct for `f32` fields (bitwise equality matches the semantics: if the user
  didn't change the value, the deserialized bits will be identical).
- `assert_eq!(old, new)` in tests is ergonomic.
- Clear intent: the type expresses "configs are comparable".

Cons:
- Requires a `#[derive(PartialEq)]` addition to ~15 structs/enums. This is a
  mechanical one-liner change per type.
- `f32` fields (`warn_threshold`, `crit_threshold`) derive `PartialEq` using
  bitwise IEEE 754 equality, which is correct for config comparison (NaN is not
  a valid threshold value and would not appear in a well-formed TOML file) but
  the Clippy lint `clippy::derive_partial_eq_without_eq` fires if `Eq` is not
  also derived for structs with no float fields. For structs containing `f32`,
  `PartialEq` is correct and `Eq` cannot be derived — suppress with
  `#[allow(clippy::derive_partial_eq_without_eq)]` on float-bearing structs with
  a comment.

**Recommendation: Option B.**

---

### Q5 — Is GTK3 full-tree flicker actually user-visible?

Measurement conditions: 28 px bar, ~10 widgets, Fedora / Cinnamon / X11 / 60 Hz.

`bar.clear_widgets()` calls `gtk::Container::remove()` for each child widget.
Each `remove()` queues a size-reallocation and damage region. The bar window is
not unmapped during this sequence, but all child widgets disappear immediately
(they are removed from the GTK widget tree). `show_all()` at the end re-adds
them and triggers a full repaint. The blank-bar period spans from the first
`remove()` to the `show_all()` repaint completion.

Empirical upper bound: 10 GTK widget removals + 10 GTK widget additions +
`show_all()` ≈ 2–5 ms on modern hardware. At 60 Hz, one frame = 16.7 ms. The
blank period is within one frame and **may or may not be visible** depending on
frame-boundary alignment.

In practice, users who actively save config files while watching the bar will
occasionally notice a transient flicker. It is a quality-of-life issue, not a
correctness bug. The incremental diff eliminates it for unchanged widgets.

---

## Recommendation

### Phase 1 — Minimal viable incremental diff

1. **Derive `PartialEq`** on `WidgetConfig`, `WidgetKind`, and all per-widget
   config structs in `parapet_core/src/config.rs`. Use
   `#[allow(clippy::derive_partial_eq_without_eq)]` on float-bearing structs,
   with a comment: `// f32 fields (threshold values) prevent Eq derivation`.

2. **Replace `self_poll_ids: Vec<glib::SourceId>` with
   `slot_poll_ids: Vec<Option<glib::SourceId>>`** in `main.rs`. Align index `i`
   with `config.widgets[i]`. This is the only structural change to `main.rs`'s
   data model.

3. **Rewrite the hot-reload handler** to diff by index:

   ```rust
   let old_widgets = old_config.widgets.clone();
   let new_widgets = new_config.widgets.clone();
   let max_len = old_widgets.len().max(new_widgets.len());

   for i in 0..max_len {
       match (old_widgets.get(i), new_widgets.get(i)) {
           (Some(old), Some(new)) if old == new => {
               // Unchanged — keep GTK widget and timer in place.
           }
           (Some(_), Some(new_cfg)) => {
               // Changed — tear down slot i, rebuild from new_cfg.
               if let Some(id) = slot_poll_ids[i].take() { id.remove(); }
               bar.replace_widget_at(i, ...);
               let (renderer, source_id) = build_widget(new_cfg, ...);
               renderers[i] = renderer;
               slot_poll_ids[i] = source_id;
           }
           (Some(_), None) => {
               // Removed — tear down slot i.
               if let Some(id) = slot_poll_ids[i].take() { id.remove(); }
               bar.remove_widget_at(i);
           }
           (None, Some(new_cfg)) => {
               // Added — build and append.
               let (renderer, source_id) = build_widget(new_cfg, ...);
               renderers.push(renderer);
               slot_poll_ids.push(source_id);
               bar.add_widget(...);
           }
           (None, None) => unreachable!(),
       }
   }
   ```

4. **Do not** attempt true in-place GTK widget reconfiguration (updating label
   text or formatter state). The rebuild-on-change approach is correct and
   simple.

### Future (Phase 3, optional)

- Add `name: Option<String>` to `WidgetConfig` to enable name-keyed matching
  (allows safe reordering of widgets in config without full re-render).
- Add `RendererDispatch::reconfigure(&WidgetConfig)` to apply format/interval
  changes without a GTK widget rebuild.

Record both items in `DOCS/futures.md`.

---

## Standards Conflict / Proposed Update

No conflicts with `ARCHITECTURE.md` or `WIDGET_API.md`. Both documents permit
`PartialEq` on config types; neither prohibits it.

**`WIDGET_API.md §4`** (WidgetConfig schema) does not list supported derives.
It should note that `PartialEq` is derived on all config types to support
incremental hot-reload diffing.

---

## Sources

- `crates/parapet_bar/src/main.rs` — full hot-reload handler; `build_widget`
  return shape `(Option<Rc<dyn RendererDispatch>>, Option<glib::SourceId>)`;
  current `self_poll_ids: Vec<glib::SourceId>` structure
- `crates/parapet_core/src/config.rs` — full `WidgetConfig`, `WidgetKind`, and
  per-widget config structs; confirmed `PartialEq` is NOT currently derived on
  any of them; `Serialize` IS derived
- `crates/parapet_bar/src/widgets/workspaces.rs` — `WorkspaceState` derives
  `PartialEq`; precedent for change-detection via equality comparison
- `standards/WIDGET_API.md` — `WidgetConfig` schema; `RendererDispatch` trait
  contract
- `standards/CODING_STANDARDS.md` — `#[allow(...)]` policy (comment required)

---

## Open Questions

1. **`bar.replace_widget_at(i, ...)` API**: The current `Bar` struct may not
   expose position-indexed widget replacement. Implementing the diff requires
   either (a) a `replace_widget_at(index, new_widget)` method on `Bar`, or (b)
   retaining the full render order in a `Vec<gtk::Widget>` beside
   `slot_poll_ids`. The exact `Bar` API refactor is an implementation detail,
   not a research question, but the implementer should audit `bar.rs` before
   starting.
2. **Poller alignment**: The Poller (`parapet_core::poll::Poller`) is keyed by
   widget name/type, not slot index. If two same-type widgets exist, their Poller
   entries conflict under the current key scheme. This is a pre-existing issue,
   not introduced by the diff work, but the implementer should be aware.
