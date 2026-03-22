//! Workspace switcher renderer — event-driven via `gdk_window_add_filter`.
//!
//! Reads `_NET_NUMBER_OF_DESKTOPS`, `_NET_CURRENT_DESKTOP`, and
//! `_NET_DESKTOP_NAMES` from the root window using `gdk::property_get()` calls.
//! Renders as a row of workspace buttons; clicking a button switches to that
//! workspace via `wmctrl -s` (falls back to `xdotool set-desktop` when `wmctrl`
//! is absent).
//!
//! # Update model
//!
//! On `x86_64` Linux, a `gdk_window_add_filter` callback is registered on the
//! root GDK window. It watches for `PropertyNotify` X11 events on the three
//! EWMH properties above and sets an `Arc<AtomicBool>` dirty flag when any
//! change is detected. A 100 ms `glib::timeout_add_local` dispatcher in
//! `main.rs` calls `refresh_if_dirty()`, which only reads X11 data and repaints
//! buttons when the flag is set. Steady-state overhead: one atomic load per
//! 100 ms.
//!
//! On non-`x86_64` targets, no filter is installed and `refresh_if_dirty()`
//! always calls `refresh()` (unconditional 100 ms poll fallback).
//!
//! # Architecture note
//!
//! This renderer does **not** use a `parapet_core` `WorkspacesWidget` or the
//! `Poller`. `impl Drop for WorkspacesWidget` calls `ffi::uninstall()` to
//! remove the event filter before the widget is freed. Teardown is safe during
//! hot-reload because the glib `SourceId` is always cancelled before the
//! `Rc<WorkspacesWidget>` is dropped.

use std::cell::RefCell;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gdk::prelude::*;
use gtk::prelude::*;

// ── Cached state ──────────────────────────────────────────────────────────────

/// Workspace state snapshot used to skip full button rebuilds on every tick.
#[derive(Default, PartialEq)]
struct WorkspaceState {
    count: usize,
    current: usize,
    names: Vec<String>,
}

// ── Event-filter state ────────────────────────────────────────────────────────

/// Shared state between the GDK event filter callback and the GTK main thread.
///
/// `Box::into_raw`-allocated in `WorkspacesWidget::new()` and passed as the
/// `data` pointer to `gdk_window_add_filter`. Reclaimed via `Box::from_raw` in
/// `ffi::uninstall()`. The allocation must outlive the filter registration —
/// guaranteed by `impl Drop for WorkspacesWidget`.
struct WorkspacesShared {
    /// Raw X11 Atom IDs for the three watched EWMH properties.
    ///
    /// On GDK3's X11 backend, `GdkAtom` stores X11 atom IDs as pointer values
    /// via `GUINT_TO_POINTER`; casting to `usize` recovers the numeric atom ID.
    watched_atoms: [usize; 3],
    /// Set to `true` by the filter callback when a watched property changes.
    /// Cleared by `refresh_if_dirty()` on the GTK main thread.
    dirty: Arc<AtomicBool>,
}

// ── Widget ────────────────────────────────────────────────────────────────────

/// GTK3 renderer for the workspace switcher widget.
pub struct WorkspacesWidget {
    container: gtk::Box,
    last_state: RefCell<WorkspaceState>,
    /// Dirty flag shared with the GDK event filter; `true` means workspace state
    /// may have changed and `refresh()` should be called.
    dirty: Arc<AtomicBool>,
    /// Raw pointer to the `Box<WorkspacesShared>` passed to `gdk_window_add_filter`.
    ///
    /// `None` on non-`x86_64` targets (filter not installed; 100 ms poll used
    /// unconditionally). `Some` on `x86_64`: reclaimed in `Drop` via `ffi::uninstall`.
    filter_data: Option<*mut WorkspacesShared>,
}

impl WorkspacesWidget {
    /// Create a new workspace renderer.
    ///
    /// Returns the widget ready for bar placement. Call [`refresh`] immediately
    /// after construction (handled by `main.rs`) to populate the initial state.
    ///
    /// # Errors
    ///
    /// Consistent with other renderer constructors; does not currently fail.
    // clippy::unnecessary_wraps: consistent renderer contract — future display init may fail
    #[allow(clippy::unnecessary_wraps)]
    pub fn new() -> anyhow::Result<Self> {
        debug_assert!(gtk::is_initialized(), "WorkspacesWidget::new() called before gtk::init()");
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        container.set_widget_name("workspaces");
        container.style_context().add_class("widget");
        container.style_context().add_class("widget-workspaces");

        let dirty = Arc::new(AtomicBool::new(false));

        #[cfg(target_arch = "x86_64")]
        let filter_data = {
            let watched_atoms = [
                atom_to_x11_id(gdk::Atom::intern("_NET_CURRENT_DESKTOP")),
                atom_to_x11_id(gdk::Atom::intern("_NET_NUMBER_OF_DESKTOPS")),
                atom_to_x11_id(gdk::Atom::intern("_NET_DESKTOP_NAMES")),
            ];
            Some(ffi::install(watched_atoms, Arc::clone(&dirty)))
        };

        // On non-x86_64 targets the event filter is not installed.
        #[cfg(not(target_arch = "x86_64"))]
        let filter_data: Option<*mut WorkspacesShared> = None;

        Ok(Self {
            container,
            last_state: RefCell::new(WorkspaceState::default()),
            dirty,
            filter_data,
        })
    }

    /// Return a reference to the root GTK widget.
    pub fn widget(&self) -> &gtk::Widget {
        self.container.upcast_ref()
    }

    /// Refresh workspace buttons from live X11 EWMH data.
    ///
    /// Reads the current state via `gdk::property_get()` on the root window and
    /// rebuilds the button row only when count, current index, or names have
    /// changed since the last call. The 100 ms glib timer in `main.rs` drives
    /// this method.
    pub fn refresh(&self) {
        let (count, current, names) = read_workspaces();
        let new_state = WorkspaceState {
            count,
            current,
            names,
        };

        // Skip rebuild when state has not changed.
        if *self.last_state.borrow() == new_state {
            return;
        }

        // Clear existing buttons.
        for child in self.container.children() {
            self.container.remove(&child);
        }

        for idx in 0..new_state.count {
            // Strip the redundant "Workspace N" prefix Cinnamon uses by default
            // so buttons show just "1", "2", "3" … unless the user set custom names.
            let raw = new_state.names.get(idx).map_or("", String::as_str);
            let label =
                if raw.eq_ignore_ascii_case(&format!("workspace {}", idx + 1)) || raw.is_empty() {
                    (idx + 1).to_string()
                } else {
                    raw.to_string()
                };

            let btn = gtk::Button::with_label(&label);
            btn.set_relief(gtk::ReliefStyle::None);
            btn.style_context().add_class("workspace");
            if idx == new_state.current {
                btn.style_context().add_class("active");
            }
            btn.connect_clicked(move |_| {
                switch_desktop(idx);
            });
            self.container.add(&btn);
        }

        self.container.show_all();
        *self.last_state.borrow_mut() = new_state;
    }

    /// Call `refresh()` only when the dirty flag has been set by the event filter.
    ///
    /// Atomically swaps the dirty flag to `false`. When it was `true`, calls
    /// `refresh()` to re-query EWMH properties and repaint workspace buttons.
    /// On non-`x86_64` targets no filter is installed; `refresh()` is always
    /// called (equivalent to the previous polling behaviour).
    pub fn refresh_if_dirty(&self) {
        // On x86_64: check the dirty flag set by the gdk_window_add_filter callback.
        // On other targets: no filter is installed — always poll.
        #[cfg(target_arch = "x86_64")]
        let should_refresh = self.dirty.swap(false, Ordering::Relaxed);
        #[cfg(not(target_arch = "x86_64"))]
        let should_refresh = true;
        if should_refresh {
            self.refresh();
        }
    }
}

impl Drop for WorkspacesWidget {
    fn drop(&mut self) {
        // Teardown ordering: the glib SourceId for this widget is always
        // cancelled before the Rc<WorkspacesWidget> is dropped in main.rs
        // (hot-reload path: cancel SourceIds → clear_widgets → drop old renderers).
        // At Drop time gtk::main() is still running, so the GDK display is alive
        // and gdk_window_remove_filter is safe to call.
        #[cfg(target_arch = "x86_64")]
        if let Some(data_ptr) = self.filter_data.take() {
            ffi::uninstall(data_ptr);
        }
    }
}

// ── X11 helpers ───────────────────────────────────────────────────────────────

/// Extract the X11 Atom ID from a `gdk::Atom`.
///
/// On GDK3's X11 backend, `GdkAtom` values are produced via
/// `GUINT_TO_POINTER(x11_atom_id)` (confirmed in `gdkproperty-x11.c`). Casting
/// the atom to `usize` recovers the numeric X11 Atom ID. Only valid on the GDK3
/// X11 backend; guarded by `#[cfg(target_arch = "x86_64")]` per ADR-002.
#[cfg(target_arch = "x86_64")]
fn atom_to_x11_id(atom: gdk::Atom) -> usize {
    // SAFETY: gdk::Atom wraps GdkAtom = *mut _GdkAtom. On GDK3's X11 backend
    // the pointer value is the X11 Atom ID (GUINT_TO_POINTER). Both are pointer-
    // sized on all targets, so transmute is sound — usize accepts any bit pattern
    // and GdkAtom holds a valid pointer-sized integer. x86_64: 8 == 8.
    unsafe { std::mem::transmute::<gdk::Atom, usize>(atom) }
}

/// Query the current workspace state from the X11 root window via `gdk::property_get()`.
///
/// Reads `_NET_NUMBER_OF_DESKTOPS`, `_NET_CURRENT_DESKTOP`, and
/// `_NET_DESKTOP_NAMES` via inline GDK property calls on the default root
/// window. Returns safe defaults (`count=1`, `current=0`, `names=[]`) when
/// any required property is absent or the display is unavailable.
fn read_workspaces() -> (usize, usize, Vec<String>) {
    let root = gdk::Window::default_root_window();
    let cardinal = gdk::Atom::intern("CARDINAL");
    let net_num = gdk::Atom::intern("_NET_NUMBER_OF_DESKTOPS");
    let net_cur = gdk::Atom::intern("_NET_CURRENT_DESKTOP");
    let net_names = gdk::Atom::intern("_NET_DESKTOP_NAMES");

    let count = if let Some((_, _, data)) = gdk::property_get(&root, &net_num, &cardinal, 0, 4, 0) {
        parse_property_cardinal(&data).unwrap_or(1)
    } else {
        tracing::warn!("_NET_NUMBER_OF_DESKTOPS read failed; workspace widget will show defaults");
        return (1, 0, Vec::new());
    };

    let current = if let Some((_, _, data)) = gdk::property_get(&root, &net_cur, &cardinal, 0, 4, 0)
    {
        parse_property_cardinal(&data).unwrap_or(0)
    } else {
        tracing::warn!("_NET_CURRENT_DESKTOP read failed; defaulting to workspace 0");
        0
    };

    // Use gdk::ATOM_NONE (X11 AnyPropertyType = atom 0) so the property is
    // returned regardless of whether Cinnamon/Muffin stored it as STRING or
    // UTF8_STRING. This is the type-filter fix described in ADR-004.
    let names = if let Some((_, _, data)) =
        gdk::property_get(&root, &net_names, &gdk::ATOM_NONE, 0, 4096, 0)
    {
        parse_property_names(&data)
    } else {
        tracing::warn!("_NET_DESKTOP_NAMES read failed; workspace name labels will be numeric");
        Vec::new()
    };

    // Clamp current to a valid index.
    let current = if current >= count { 0 } else { current };

    (count, current, names)
}

/// Parse a single CARDINAL X11 property value from raw GDK property bytes.
///
/// GDK returns 32-bit format properties as a byte slice in native byte order.
/// Returns `None` when `bytes` is shorter than 4 bytes.
fn parse_property_cardinal(bytes: &[u8]) -> Option<usize> {
    let arr: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    // 32-bit CARDINAL: GDK returns data in host byte order (native-endian u32).
    Some(u32::from_ne_bytes(arr) as usize)
}

/// Parse a null-separated UTF-8 string list from raw GDK property bytes.
///
/// EWMH `_NET_DESKTOP_NAMES` is stored as null-separated UTF-8 strings
/// concatenated into a single byte array. Empty segments (trailing `\0`) are
/// filtered out. Returns an empty `Vec` when `bytes` is empty.
fn parse_property_names(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&b| b == 0)
        .filter(|seg| !seg.is_empty())
        .map(|seg| String::from_utf8_lossy(seg).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_property_cardinal ───────────────────────────────────────────────

    #[test]
    fn cardinal_single_workspace() {
        // [1, 0, 0, 0] in little-endian native byte order = 1.
        assert_eq!(parse_property_cardinal(&[1, 0, 0, 0]), Some(1));
    }

    #[test]
    fn cardinal_ten_workspaces() {
        assert_eq!(parse_property_cardinal(&[10, 0, 0, 0]), Some(10));
    }

    #[test]
    fn cardinal_empty_slice_returns_none() {
        assert_eq!(parse_property_cardinal(&[]), None);
    }

    #[test]
    fn cardinal_truncated_three_bytes_returns_none() {
        assert_eq!(parse_property_cardinal(&[1, 0, 0]), None);
    }

    // ── parse_property_names ─────────────────────────────────────────────────

    #[test]
    fn names_three_workspaces() {
        assert_eq!(parse_property_names(b"Home\0Work\0Code\0"), vec!["Home", "Work", "Code"]);
    }

    #[test]
    fn names_utf8_preserved() {
        let input = "Heim\0B\u{00FC}ro\0".as_bytes();
        assert_eq!(parse_property_names(input), vec!["Heim", "B\u{00FC}ro"]);
    }

    #[test]
    fn names_empty_bytes_returns_empty_vec() {
        assert_eq!(parse_property_names(b""), Vec::<String>::new());
    }

    #[test]
    fn names_single_null_filters_out() {
        assert_eq!(parse_property_names(b"\0"), Vec::<String>::new());
    }

    #[test]
    fn names_consecutive_nulls_filtered() {
        // b"A\0\0B\0" → segments ["A", "", "B", ""] → filtered → ["A", "B"].
        assert_eq!(parse_property_names(b"A\0\0B\0"), vec!["A", "B"]);
    }
}

/// Switch to workspace `idx` by posting `_NET_CURRENT_DESKTOP`.
///
/// Tries `wmctrl -s N` first (sends the correct EWMH `ClientMessage`).
/// Falls back to `xdotool set-desktop N` when `wmctrl` is absent.
/// Logs an actionable warning if both tools are missing.
fn switch_desktop(idx: usize) {
    let n = idx.to_string();

    // Primary: wmctrl — sends _NET_CURRENT_DESKTOP ClientMessage.
    match Command::new("wmctrl").args(["-s", &n]).spawn() {
        Ok(_) => return,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!("wmctrl not found; trying xdotool as fallback");
        }
        Err(e) => tracing::warn!(error = %e, desktop = idx, "wmctrl spawn failed"),
    }

    // Fallback: xdotool.
    if let Err(e) = Command::new("xdotool").args(["set-desktop", &n]).spawn() {
        if e.kind() == std::io::ErrorKind::NotFound {
            tracing::warn!(
                desktop = idx,
                "neither wmctrl nor xdotool found; install one to enable workspace switching. \
                 Fedora: sudo dnf install wmctrl"
            );
        } else {
            tracing::warn!(error = %e, desktop = idx, "xdotool set-desktop failed");
        }
    }
}

// ── FFI safe wrappers ─────────────────────────────────────────────────────────

/// Safe wrappers around `gdk::ffi` event-filter FFI functions.
///
/// `gdk::ffi::gdk_window_add_filter` is not exposed in `gdk` 0.18's safe API.
/// The symbol is declared in `gdk-sys 0.18.2` and re-exported as `gdk::ffi`,
/// which is already linked via `gdk = "~0.18"` in `Cargo.toml`.
#[cfg(target_arch = "x86_64")]
mod ffi {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use gdk::prelude::*;

    use super::WorkspacesShared;

    /// GDK event filter callback — invoked for every X event on the root window.
    ///
    /// Detects `PropertyNotify` events for the three watched EWMH atoms and sets
    /// the dirty flag. Always returns `GDK_FILTER_CONTINUE` so GDK's own event
    /// processing is unaffected.
    ///
    /// # Safety
    ///
    /// `xevent` must be a valid `*mut XEvent` for the duration of this call
    /// (guaranteed by GDK's `GdkFilterFunc` contract). `data` must be a valid
    /// `*mut WorkspacesShared` allocated via `Box::into_raw` and kept alive until
    /// `gdk_window_remove_filter` has been called (guaranteed by `impl Drop`).
    unsafe extern "C" fn workspace_event_filter(
        xevent: *mut gdk::ffi::GdkXEvent,
        _event: *mut gdk::ffi::GdkEvent,
        data: *mut std::ffi::c_void,
    ) -> gdk::ffi::GdkFilterReturn {
        // X11 Xlib.h: #define PropertyNotify 28
        const PROPERTY_NOTIFY: i32 = 28;
        // XPropertyEvent::atom field — x86_64 Linux LP64 ABI byte offsets:
        //   offset  0: int           type        (4 bytes)
        //   offset  4: [4 pad — align unsigned long to 8]
        //   offset  8: unsigned long serial      (8 bytes)
        //   offset 16: int           send_event  (4 bytes, Bool = int)
        //   offset 20: [4 pad — align pointer to 8]
        //   offset 24: Display*      display     (8 bytes)
        //   offset 32: Window        window      (8 bytes, XID = unsigned long)
        //   offset 40: Atom          atom        (8 bytes, unsigned long)  ← here
        const ATOM_OFFSET: usize = 40;

        // SAFETY: GDK guarantees xevent is a valid *mut XEvent; the first
        // field (int type) is at offset 0.
        let ev_type = *(xevent as *const i32);
        if ev_type == PROPERTY_NOTIFY {
            // SAFETY: ev_type == PropertyNotify guarantees XPropertyEvent layout.
            // atom field at byte offset 40 (x86_64 LP64 ABI; see layout above).
            // read_unaligned used because the field is not guaranteed to be
            // pointer-aligned from the raw XEvent *const u8 base.
            let atom =
                std::ptr::read_unaligned((xevent as *const u8).add(ATOM_OFFSET).cast::<usize>());

            // SAFETY: data is Box::into_raw(WorkspacesShared) from install().
            // It remains valid until uninstall() is called in Drop, which runs
            // after this callback is deregistered via gdk_window_remove_filter.
            let shared = &*(data as *const WorkspacesShared);
            if shared.watched_atoms.contains(&atom) {
                shared.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        gdk::ffi::GDK_FILTER_CONTINUE
    }

    /// Register the workspace `PropertyNotify` filter as a GDK display-level filter.
    ///
    /// Enables `PROPERTY_CHANGE_MASK` on the root window so the X server delivers
    /// `PropertyNotify` events for root-window property changes. Installs
    /// [`workspace_event_filter`] as a display-level filter (NULL window) so GDK
    /// routes every X event through it before window-specific dispatch. This is
    /// more reliable than a window-specific root filter because GDK's
    /// `_gdk_default_filters` list is always consulted for every X event.
    /// Returns the raw `*mut WorkspacesShared` pointer that must be passed to
    /// [`uninstall`] to tear down the filter and free memory.
    ///
    /// Must be called after `gtk::init()`. Does not fail.
    pub(super) fn install(
        watched_atoms: [usize; 3],
        dirty: Arc<AtomicBool>,
    ) -> *mut WorkspacesShared {
        // Select PROPERTY_CHANGE_MASK on the root window so the X server delivers
        // PropertyNotify events for root property changes to our client.
        let root = gdk::Window::default_root_window();
        root.set_events(root.events() | gdk::EventMask::PROPERTY_CHANGE_MASK);

        let shared = Box::new(WorkspacesShared {
            watched_atoms,
            dirty,
        });
        let data_ptr: *mut WorkspacesShared = Box::into_raw(shared);

        unsafe {
            // SAFETY: Passing NULL as the window registers a display-level
            // (global) filter in GDK's _gdk_default_filters list. This filter
            // is applied to every X event before window-specific dispatch, which
            // is the reliable path for catching root-window PropertyNotify events.
            // data_ptr is from Box::into_raw; reclaimed in uninstall().
            // workspace_event_filter has the correct GdkFilterFunc signature.
            gdk::ffi::gdk_window_add_filter(
                std::ptr::null_mut(),
                Some(workspace_event_filter),
                data_ptr.cast::<std::ffi::c_void>(),
            );
        }
        data_ptr
    }

    /// Remove the workspace filter and free the shared state.
    ///
    /// Must be called exactly once per successful [`install`] call, while the
    /// GDK display is still alive. After this call, no further
    /// `workspace_event_filter` invocations occur.
    pub(super) fn uninstall(data_ptr: *mut WorkspacesShared) {
        unsafe {
            // SAFETY: NULL matches the window passed to install() (display-level
            // filter). data_ptr was allocated by install via Box::into_raw and
            // has not been freed. After gdk_window_remove_filter, no further
            // callback invocations will occur, making Box::from_raw safe.
            gdk::ffi::gdk_window_remove_filter(
                std::ptr::null_mut(),
                Some(workspace_event_filter),
                data_ptr.cast::<std::ffi::c_void>(),
            );
            drop(Box::from_raw(data_ptr));
        }
    }
}
