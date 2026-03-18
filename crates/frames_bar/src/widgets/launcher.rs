//! App launcher renderer — opens a search-driven application dropdown.
//!
//! Renders as a single `gtk::Button` labelled "Apps" in the bar. Clicking it
//! opens a standalone `gtk::Window` (popup-menu type) positioned below the
//! button, containing a `gtk::SearchEntry` and a `gtk::ListBox` of installed
//! applications filtered by the current query. Pressing Enter or clicking a
//! row launches the selected app via `gio::AppInfo::launch`.
//!
//! A dedicated window is used instead of `gtk::Popover` because the bar is a
//! dock window only 30 px tall — GTK3 on X11 clips popovers to the parent
//! window bounds, preventing them from opening fully.
//!
//! # Architecture note
//!
//! This renderer does **not** implement the [`frames_core::Widget`] trait or
//! use the [`frames_core::Poller`]. The application list is loaded once at
//! construction via [`gio::AppInfo::all()`] and cached for the lifetime of
//! the bar. Fuzzy filtering runs synchronously on the GTK main thread —
//! acceptable for lists of ~200–500 apps.
//!
//! This is the same architectural exception already established for
//! `WorkspacesWidget`. See `DOCS/futures.md` for the known limitation and
//! planned improvement (XDG data dir watching).

use std::rc::Rc;

use fuzzy_matcher::FuzzyMatcher;
use gdk::prelude::*;
use gio::prelude::AppInfoExt;
use gtk::prelude::*;

use frames_core::config::WidgetConfig;

/// GTK3 renderer for the app launcher widget.
///
/// Renders as a single `gtk::Button` in the bar. Clicking it opens a
/// standalone dropdown `gtk::Window` containing a `gtk::SearchEntry` and
/// a `gtk::ListBox` of installed applications filtered by the current query.
/// Selecting an entry launches the application via [`gio::AppInfo::launch`].
///
/// # Architecture note
///
/// This widget does **not** implement the `Widget` trait or use the `Poller`.
/// The application list is loaded once at construction via
/// [`gio::AppInfo::all()`] and cached for the lifetime of the bar.
///
/// A standalone window is used instead of `gtk::Popover` because the bar
/// is a dock window 30 px tall on X11 — GTK3 clips popovers to their parent
/// window bounds, preventing the popup from opening fully.
pub struct LauncherWidget {
    button: gtk::Button,
}

impl LauncherWidget {
    /// Create a new launcher renderer.
    ///
    /// Loads the installed application list via `gio::AppInfo::all()`, filters
    /// to visible apps (`should_show()`), and caches them in an `Rc<Vec<_>>`.
    /// Builds the `gtk::Popover` tree and wires all signal handlers.
    ///
    /// `config.max_results` caps the number of rows shown in the filtered list.
    /// Defaults to 10 if absent.
    ///
    /// # Errors
    ///
    /// Returns `anyhow::Error` if GTK widget construction fails (should not
    /// happen after `gtk::init()` succeeds, but the contract is consistent
    /// with other renderers).
    // clippy::unnecessary_wraps: consistent renderer contract — future display init may fail
    #[allow(clippy::unnecessary_wraps)]
    pub fn new(config: &WidgetConfig) -> anyhow::Result<Self> {
        let max_results = config.max_results.unwrap_or(10);

        // Load installed apps once; cache for signal handler lifetime.
        let apps: Rc<Vec<gio::AppInfo>> =
            Rc::new(gio::AppInfo::all().into_iter().filter(AppInfoExt::should_show).collect());

        if apps.is_empty() {
            tracing::warn!("launcher: no installed applications found via gio::AppInfo::all()");
        }

        // ── Button ────────────────────────────────────────────────────────
        let button = gtk::Button::with_label("Apps");
        button.set_relief(gtk::ReliefStyle::None);
        button.set_widget_name("launcher");
        button.style_context().add_class("widget");
        button.style_context().add_class("widget-launcher");

        // ── Dropdown window ───────────────────────────────────────────────
        // Use a standalone window rather than gtk::Popover. On X11/GTK3 a
        // Popover is rendered within its parent window's surface, so a 30 px
        // dock bar clips it to 30 px. A separate PopupMenu window gets its
        // own X11 window and can extend freely below the bar.
        let dropdown = gtk::Window::new(gtk::WindowType::Toplevel);
        dropdown.set_decorated(false);
        dropdown.set_resizable(false);
        dropdown.set_skip_taskbar_hint(true);
        dropdown.set_skip_pager_hint(true);
        dropdown.set_type_hint(gdk::WindowTypeHint::PopupMenu);
        dropdown.set_default_size(280, -1);
        dropdown.style_context().add_class("launcher-popover");

        // ── Inner layout ──────────────────────────────────────────────────
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);

        let search = gtk::SearchEntry::new();
        search.style_context().add_class("launcher-search");

        let scrolled = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scrolled.set_min_content_height(200);
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let list = gtk::ListBox::new();
        list.style_context().add_class("launcher-list");
        list.set_activate_on_single_click(false);

        scrolled.add(&list);
        vbox.pack_start(&search, false, false, 0);
        vbox.pack_start(&scrolled, true, true, 0);
        dropdown.add(&vbox);

        // Hide instead of destroy when the window manager asks to close.
        dropdown.connect_delete_event(|win, _| {
            win.hide();
            glib::Propagation::Stop
        });

        // ── Signal: Escape closes the dropdown ────────────────────────────
        dropdown.connect_key_press_event(|win, event| {
            if event.keyval() == gdk::keys::constants::Escape {
                win.hide();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });

        // ── Signal: button clicked → toggle dropdown ──────────────────────
        {
            let dropdown = dropdown.clone();
            let search = search.clone();
            let list = list.clone();
            let apps = Rc::clone(&apps);
            button.connect_clicked(move |btn| {
                if dropdown.is_visible() {
                    dropdown.hide();
                    return;
                }

                rebuild_list(&list, &apps, "", max_results);
                search.set_text("");

                // Position the dropdown below the button on screen.
                if let Some(gdk_win) = btn.window() {
                    let (wx, wy, _) = gdk_win.origin();
                    let alloc = btn.allocation();
                    dropdown.move_(wx + alloc.x(), wy + alloc.y() + alloc.height());
                }

                dropdown.show_all();
                search.grab_focus();
            });
        }

        // ── Signal: search changed → filter list ──────────────────────────
        {
            let list = list.clone();
            let apps = Rc::clone(&apps);
            search.connect_changed(move |entry| {
                let query = entry.text().to_string();
                rebuild_list(&list, &apps, &query, max_results);
            });
        }

        // ── Signal: search activate (Enter) → launch top result ───────────
        {
            let list = list.clone();
            let dropdown = dropdown.clone();
            search.connect_activate(move |_| {
                if let Some(row) = list.row_at_index(0) {
                    if let Some(app) = get_row_app(&row) {
                        launch_app(&app);
                        dropdown.hide();
                    }
                }
            });
        }

        // ── Signal: row activated → launch selected app ───────────────────
        {
            let dropdown = dropdown.clone();
            list.connect_row_activated(move |_, row| {
                if let Some(app) = get_row_app(row) {
                    launch_app(&app);
                    dropdown.hide();
                }
            });
        }

        Ok(Self { button })
    }

    /// Return a reference to the root GTK widget (the `gtk::Button`) for bar placement.
    pub fn widget(&self) -> &gtk::Widget {
        self.button.upcast_ref()
    }
}

/// Rebuild the `gtk::ListBox` contents from `apps` filtered by `query`.
///
/// Removes all existing rows, scores and sorts via fuzzy matching, then inserts
/// up to `max_results` new rows. Each row stores its `AppInfo` via `GObject`
/// `set_data` so signal handlers can retrieve and launch it.
fn rebuild_list(list: &gtk::ListBox, apps: &[gio::AppInfo], query: &str, max_results: u32) {
    // Remove all existing rows.
    for child in list.children() {
        list.remove(&child);
    }

    // Score and sort with fuzzy-matcher.
    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
    let mut scored: Vec<(i64, &gio::AppInfo)> = apps
        .iter()
        .filter_map(|app| {
            let name = app.name().to_string();
            if query.is_empty() {
                Some((0, app))
            } else {
                matcher.fuzzy_match(&name, query).map(|score| (score, app))
            }
        })
        .collect();

    if !query.is_empty() {
        scored.sort_by(|a, b| b.0.cmp(&a.0));
    }

    // Build rows, capped at max_results.
    for (_, app) in scored.into_iter().take(max_results as usize) {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row_box.style_context().add_class("launcher-row");

        // Icon (16×16 SmallToolbar size; omitted silently if absent).
        if let Some(icon) = app.icon() {
            let img = gtk::Image::from_gicon(&icon, gtk::IconSize::SmallToolbar);
            row_box.pack_start(&img, false, false, 0);
        }

        let label = gtk::Label::new(Some(app.name().as_ref()));
        label.set_halign(gtk::Align::Start);
        row_box.pack_start(&label, true, true, 0);

        let row = gtk::ListBoxRow::new();
        row.add(&row_box);

        // SAFETY: The AppInfo clone is owned exclusively by this ListBoxRow via
        // GObject qdata. It is freed by GObject when the row is finalized, which
        // occurs before the Rc<Vec<AppInfo>> owning the original is dropped.
        unsafe {
            row.set_data("app-info", app.clone());
        }

        list.add(&row);
    }
    list.show_all();
}

/// Retrieve the cached `AppInfo` from a `ListBoxRow`.
///
/// Returns `None` if no app data is present (defensive; should not occur for
/// rows constructed by [`rebuild_list`]).
fn get_row_app(row: &gtk::ListBoxRow) -> Option<gio::AppInfo> {
    // SAFETY: The AppInfo stored by `rebuild_list` matches the type requested
    // here. The row is alive during signal dispatch, so the pointer is valid.
    unsafe { row.data::<gio::AppInfo>("app-info").map(|ptr| ptr.as_ref().clone()) }
}

/// Launch an application via GIO, logging a warning on failure.
///
/// Non-fatal — a failed launch does not crash the bar.
fn launch_app(app: &gio::AppInfo) {
    if let Err(e) = app.launch(&[], None::<&gio::AppLaunchContext>) {
        tracing::warn!(app = %app.name(), error = %e, "failed to launch application");
    }
}
