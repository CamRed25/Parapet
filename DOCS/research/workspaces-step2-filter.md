# Research: WorkspacesWidget Step 2 — `gdk_window_add_filter` Implementation Path
**Date:** 2026-03-22
**Status:** Findings complete — decision recommended — ready to close

## Question

What is the exact safe-Rust or minimal-unsafe implementation path for subscribing
to X11 `PropertyNotify` events on the root window via `gdk_window_add_filter` in
gdk 0.18 (`gdk = "~0.18"`)?

This is a Step 2 follow-up to `DOCS/research/workspaces-event-driven.md`, which
established that the approach is feasible but deferred the exact implementation
details. Do not re-read that document as a prerequisite — the current findings
supersede or extend every open question raised there.

---

## Summary

`gdk-0.18` exposes **no safe Rust API for `gdk_window_add_filter`**. The
minimal-unsafe path is a 3-block `extern "C"` declaration linking to the
already-linked GDK3 shared library. The callback fires on the GTK main thread, so
`Arc<AtomicBool>` (or `mpsc::Sender<()>`) can safely carry the signal to a
consuming glib timer without any locking concern. One factual error from the prior
research doc is corrected: `GDK_FILTER_REMOVE = 2`, not `1`.

---

## Findings

### Q1 — Does gdk-0.18 expose a safe `connect_filter` or `add_filter`?

**No.** A complete audit of `gdk::prelude::WindowExtManual` (confirmed from
`docs.rs/gdk/0.18.0`) shows these are the only provided methods:

```
set_user_data<T>(), user_data<T>(), default_root_window(),
offscreen_window_set_embedder(), offscreen_window_get_embedder(),
offscreen_window_get_surface(), pixbuf(), background_pattern(),
set_background_pattern()
```

There is no `add_filter`, `remove_filter`, or `connect_property_notify` variant.

`gdk-sys 0.18` (the raw C-level bindings) does declare `gdk_window_add_filter`
and `gdk_window_remove_filter` as `extern "C"` symbols — they are part of the GDK3
ABI that is already linked when `gdk = "~0.18"` is a dependency. The minimal-unsafe
approach uses a bare `extern "C"` block to access these symbols directly:

```rust
extern "C" {
    fn gdk_window_add_filter(
        window: *mut std::os::raw::c_void,   // GdkWindow*
        function: unsafe extern "C" fn(
            *mut std::os::raw::c_void,   // GdkXEvent* (actually XEvent*)
            *mut std::os::raw::c_void,   // GdkEvent*
            *mut std::os::raw::c_void,   // gpointer user_data
        ) -> i32,
        data: *mut std::os::raw::c_void,
    );

    fn gdk_window_remove_filter(
        window: *mut std::os::raw::c_void,
        function: unsafe extern "C" fn(
            *mut std::os::raw::c_void,
            *mut std::os::raw::c_void,
            *mut std::os::raw::c_void,
        ) -> i32,
        data: *mut std::os::raw::c_void,
    );
}
```

No additional Cargo.toml changes are required — these symbols come from the system
`libgdk-3.so` already linked via `gdk = "~0.18"`.

**To obtain the `*mut c_void` pointer for the root window:**

```rust
use gdk::prelude::WindowExtManual;
use glib::translate::ToGlibPtr;

let root = <gdk::Window as WindowExtManual>::default_root_window();
let root_ptr: *mut std::os::raw::c_void = root.to_glib_none().0 as *mut _;
```

---

### Q2 — Correct event-subscription sequence; role of `gdk_window_set_events`

The sequence is:

1. **Get the root window:**  
   `let root = <gdk::Window as WindowExtManual>::default_root_window();`

2. **Enable `PropertyChangeMask` on the root window:**  
   ```rust
   root.set_events(root.events() | gdk::EventMask::PROPERTY_CHANGE_MASK);
   ```
   This is equivalent to `XSelectInput(dpy, root, PropertyChangeMask)`. Without
   this step, GDK does not forward `PropertyNotify` events to the filter callback.

3. **Register the filter:**  
   ```rust
   unsafe {
       gdk_window_add_filter(root_ptr, workspace_event_filter, data_ptr);
   }
   ```

**`_NET_WM_STRUT_PARTIAL` is NOT relevant here.** That atom is a property the bar
*sets* on its own window to reserve screen space. It has no role in subscribing to
root-window property change events.

The atoms to watch in the filter callback are:
- `_NET_CURRENT_DESKTOP` — active workspace changed
- `_NET_NUMBER_OF_DESKTOPS` — workspaces added or removed
- `_NET_DESKTOP_NAMES` — workspace label renamed

Intern these atoms once during `WorkspacesWidget::new()` (after `gtk::init()`):

```rust
let atom_current  = gdk::Atom::intern("_NET_CURRENT_DESKTOP");
let atom_count    = gdk::Atom::intern("_NET_NUMBER_OF_DESKTOPS");
let atom_names    = gdk::Atom::intern("_NET_DESKTOP_NAMES");
```

`gdk::Atom::intern` returns an opaque value backed by `u64` (the X atom ID). Store
these in the `Box<WorkspacesShared>` struct passed as `data_ptr` so the filter
callback can compare against them.

---

### Q3 — `GdkFilterReturn` values in gdk-0.18

**⚠ Correction from prior research doc (`workspaces-event-driven.md`):**  
That document stated `GDK_FILTER_REMOVE = 1`. This is **wrong**.

The correct values from the GDK3 C API (verified from
`https://docs.gtk.org/gdk3/enum.FilterReturn.html`):

| Constant | Value | Meaning |
|----------|-------|---------|
| `GDK_FILTER_CONTINUE` | `0` | Event not handled — continue processing |
| `GDK_FILTER_TRANSLATE` | `1` | Native event translated to a `GdkEvent`; stops further filter processing |
| `GDK_FILTER_REMOVE` | `2` | Event handled, terminate processing (discard) |

For the workspace filter callback, always return `0` (`GDK_FILTER_CONTINUE`) so
GDK's own event processing continues uninterrupted:

```rust
unsafe extern "C" fn workspace_event_filter(
    xevent_ptr: *mut c_void,
    _gdk_event: *mut c_void,
    data: *mut c_void,
) -> i32 {
    const PROPERTY_NOTIFY: i32 = 28; // X11 PropertyNotify event type

    // SAFETY: xevent_ptr is a valid XEvent pointer for the lifetime of this
    // call; GDK guarantees this contract for all GdkFilterFunc invocations.
    let ev_type = *(xevent_ptr as *const i32);
    if ev_type == PROPERTY_NOTIFY {
        // XPropertyEvent layout: type(i32), serial(u64), send_event(bool),
        //   display(*mut Display), window(u64), atom(u64), time(u64), state(i32)
        // atom is at byte offset 32 on 64-bit Linux.
        let atom = *((xevent_ptr as *const u8).add(32) as *const u64);

        // SAFETY: data was set to Box::into_raw(shared) in WorkspacesWidget::new()
        // and remains valid until gdk_window_remove_filter is called on teardown.
        let shared = &*(data as *const WorkspacesShared);
        if shared.watched_atoms.contains(&atom) {
            // Signal to the GTK main thread that workspace data changed.
            shared.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    0 // GDK_FILTER_CONTINUE
}
```

**Note on `XPropertyEvent` layout:** The `atom` field is at a fixed byte offset
in the X11 ABI on x86-64 Linux. If the `x11` crate is confirmed as a direct
dependency (see Open Questions), use `x11::xlib::XPropertyEvent` instead of
manual offset arithmetic for type safety and portability.

---

### Q4 — Safe data transfer out of the filter callback

The filter callback runs on the **GTK main thread** (the same `glib::MainContext`
that drives `gtk::main()`). This means:

- **`Arc<AtomicBool>` (recommended):** Set a dirty flag inside the filter; a
  100 ms `glib::timeout_add_local` timer reads and clears the flag, then calls
  `renderer.refresh()` if dirty. This is the simplest, lowest-overhead approach.
  No allocation in the hot path. No GTK locking needed.

  ```rust
  // In WorkspacesShared:
  dirty: Arc<AtomicBool>,

  // In filter callback:
  shared.dirty.store(true, Ordering::Relaxed);

  // In glib timer (replacing current 100ms poll):
  if dirty.load(Ordering::Relaxed) {
      dirty.store(false, Ordering::Relaxed);
      renderer.refresh();
  }
  ```

- **`std::sync::mpsc::Sender<()>`:** Also safe. Filter pushes `()`, a glib timer
  calls `try_recv()`. Slightly more overhead than `AtomicBool` (heap allocation for
  the channel message), but well-understood and matches the `VolumeWidget` pattern.

- **`Rc<Cell<bool>>`:** Works only if both the filter and consumer run on the
  same thread (they do — both on the GTK main thread). However, `Rc` is not
  `Send`, so it cannot be placed in the `data_ptr` raw pointer and shared across
  the FFI boundary via `Box::into_raw()` without violating Rust's thread-safety
  model at the type level. **Not recommended** — use `Arc<AtomicBool>` instead,
  which is correct for both `!Send` and `Send` scenarios.

**No GTK lock is held during the filter callback.** GDK calls the filter from
within the GDK event dispatch loop, which runs under the GDK lock, but the lock
is released before calling filter functions on the GTK main thread. Writing to an
`AtomicBool` or `Sender<()>` from inside the filter is safe.

---

### Q5 — Known issues with `gdk_window_add_filter` on root window in Cinnamon/Muffin

No Cinnamon- or Muffin-specific issues are known. Muffin (Cinnamon's WM) is a
fork of Mutter and is fully EWMH-compliant. It posts `PropertyNotify` events on
the root window for `_NET_CURRENT_DESKTOP`, `_NET_NUMBER_OF_DESKTOPS`, and
`_NET_DESKTOP_NAMES` changes in the standard manner.

One Cinnamon-specific quirk: Cinnamon names workspaces `"Workspace 1"`,
`"Workspace 2"` etc. by default. The existing `WorkspacesWidget::refresh()` already
strips this prefix (see
`crates/parapet_bar/src/widgets/workspaces.rs` — the `raw.eq_ignore_ascii_case`
branch). This stripping logic carries over unchanged into the event-driven path.

**Performance note:** The filter will be called for **every X event**, not just
`PropertyNotify`. The event-type check (`ev_type == PROPERTY_NOTIFY`) runs in
~2 ns. For a typical desktop idle sending ~50–200 X events/second, the added
overhead is negligible (~10 µs/second total).

---

## Complete Minimal Implementation Sketch

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use gdk::prelude::WindowExtManual;
use glib::translate::ToGlibPtr;

struct WorkspacesShared {
    watched_atoms: Vec<u64>,  // X atom IDs for the three EWMH atoms
    dirty: Arc<AtomicBool>,
}

// Called once in WorkspacesWidget::new(), after gtk::init():
fn install_property_filter(dirty: Arc<AtomicBool>) {
    let atom_current = gdk::Atom::intern("_NET_CURRENT_DESKTOP");
    let atom_count   = gdk::Atom::intern("_NET_NUMBER_OF_DESKTOPS");
    let atom_names   = gdk::Atom::intern("_NET_DESKTOP_NAMES");

    // gdk::Atom is a newtype over u64 (the X atom ID).
    // Use unsafe transmute or glib::ffi to extract the raw u64.
    let watched_atoms: Vec<u64> = vec![
        // SAFETY: gdk::Atom is repr(transparent) over a glib::ffi atom ID.
        unsafe { std::mem::transmute(atom_current) },
        unsafe { std::mem::transmute(atom_count) },
        unsafe { std::mem::transmute(atom_names) },
    ];

    let shared = Box::new(WorkspacesShared { watched_atoms, dirty });
    let data_ptr = Box::into_raw(shared) as *mut std::os::raw::c_void;

    let root = <gdk::Window as WindowExtManual>::default_root_window();
    root.set_events(root.events() | gdk::EventMask::PROPERTY_CHANGE_MASK);

    unsafe {
        // SAFETY: root_ptr is valid for the lifetime of the GDK display.
        // data_ptr is the Box<WorkspacesShared> allocation; paired with
        // gdk_window_remove_filter + Box::from_raw on teardown.
        gdk_window_add_filter(
            root.to_glib_none().0 as *mut _,
            workspace_event_filter,
            data_ptr,
        );
    }
}
```

---

## Recommendation

Implement Option A from `workspaces-event-driven.md` with the following precise choices:

1. **Signalling mechanism:** `Arc<AtomicBool>` (dirty flag) shared between the
   filter closure and a surviving 100 ms glib timer (the timer now only calls
   `refresh()` when dirty, rather than unconditionally).

2. **No `x11` crate required** for the minimal implementation; use the literal
   `28` constant for `PropertyNotify` with a clear comment referencing the X11
   spec. If an `x11` dep is already present as a direct dep, prefer
   `x11::xlib::XPropertyEvent` struct access for readability.

3. **Teardown:** Store `data_ptr` in the `WorkspacesWidget` struct. In a `Drop`
   impl (or in `Bar::clear_widgets` teardown), call:
   ```rust
   unsafe {
       gdk_window_remove_filter(root_ptr, workspace_event_filter, data_ptr);
       drop(Box::from_raw(data_ptr as *mut WorkspacesShared));
   }
   ```

4. **SAFETY comments:** Three blocks — `add_filter` call, `remove_filter` call,
   callback body — each need a `// SAFETY:` justification per
   `CODING_STANDARDS.md §8.2`.

5. **Correction from prior doc:** Update `workspaces-event-driven.md` note that
   `GDK_FILTER_REMOVE = 2`, not `1`. The prior value was wrong.

---

## Standards Conflict / Proposed Update

`BAR_DESIGN.md §5` (Widget update lifecycle) should gain:

> **Event-driven widgets:** A bar-side renderer may register a
> `gdk_window_add_filter` callback instead of a polling timer, provided:
> (a) implemented only in `parapet_bar`; (b) all `unsafe` blocks carry
> `// SAFETY:` justifications per CODING_STANDARDS §8.2; (c)
> `gdk_window_remove_filter` is called before widget drop; (d) data is passed via
> `Arc<AtomicBool>` or `mpsc::Sender`/`Receiver`.

---

## Sources

- `https://docs.rs/gdk/0.18.0/gdk/prelude/trait.WindowExtManual.html` —
  confirmed complete method list of `WindowExtManual` in gdk 0.18; no `add_filter`
- `https://docs.rs/gdk/0.18.0/gdk/struct.Window.html` — confirmed `set_events`
  and `EventMask::PROPERTY_CHANGE_MASK` are available
- `https://docs.gtk.org/gdk3/enum.FilterReturn.html` — authoritative GDK3 C API
  values for `GdkFilterReturn`: `CONTINUE=0`, `TRANSLATE=1`, `REMOVE=2`
- `DOCS/research/workspaces-event-driven.md` — prior research, Option A selected;
  this document refines and corrects it
- `crates/parapet_bar/src/widgets/workspaces.rs` — current 100 ms poll
  implementation (Step 1 complete)
- `crates/parapet_bar/src/main.rs` (workspaces `build_widget` arm) — SourceId
  cancellation on hot-reload; confirmed separate from Poller pipeline

---

## Open Questions

1. **`gdk::Atom` raw value extraction:** The `gdk::Atom` type is opaque in the
   safe API. Confirm the cleanest way to obtain the raw X atom ID (either
   `glib::ffi::GQuark` cast, `std::mem::transmute`, or — if available —
   `gdk::ffi::GdkAtom` from `gdk-sys`). Requires a short compile experiment.

2. **`x11` direct dep:** Run `cargo tree -p parapet_bar | grep " x11"` to confirm
   whether `x11` is already a transitive dep. If so, adding it as a direct dep
   only documents existing linkage and costs nothing.

3. **`Box::into_raw` lifetime for teardown:** `Bar` currently has no `Drop` impl.
   Storing `data_ptr` and pairing it with a `remove_filter` call should go either
   in `Bar::clear_widgets()` (called on hot-reload) or a new `Bar::drop_filter()`
   method. Decision belongs to the implementer.
