//! `frames_bar` — GTK3 status bar binary for the Cinnamon desktop.
//!
//! Startup sequence:
//! 1. Initialize tracing
//! 2. Parse `--theme <name>` CLI override
//! 3. Load `FramesConfig` from TOML (env `FRAMES_CONFIG` or default path)
//! 4. Initialize GTK3
//! 5. Create the `Bar` window
//! 6. Resolve theme source; detect dark/light variant; load and apply CSS
//! 7. Build widget renderers from config; register core widgets with `Poller`
//! 8. Wire glib timer to poll widgets and update renderers
//! 9. Register SIGTERM handler for clean shutdown
//! 10. Start config hot-reload watcher; start CSS file hot-reload watcher
//! 11. Enter `gtk::main()`

mod bar;
mod css;
mod widgets;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Context;
use gdk::prelude::*;
use glib::ControlFlow;
use gtk::prelude::*;

use frames_core::config::{BarSection, ConfigWatcher, WidgetConfig};
use frames_core::{FramesConfig, Poller, WidgetData};

#[allow(clippy::too_many_lines)] // GTK startup sequence is inherently sequential; extracting into subfunctions would obscure the ordered startup steps
fn main() -> anyhow::Result<()> {
    let t0 = Instant::now();

    // ── 1. Tracing ──────────────────────────────────────────────────────────
    tracing_subscriber::fmt::init();

    // ── 2. CLI args — --theme <name> ────────────────────────────────────────
    // `--theme <name>` overrides bar.theme and bar.css in config.
    let cli_theme: Option<String> = {
        let mut args = std::env::args().skip(1);
        let mut found = None;
        while let Some(arg) = args.next() {
            if arg == "--theme" {
                found = args.next();
                break;
            }
        }
        found
    };

    // ── 2. Config ───────────────────────────────────────────────────────────
    let config_path = std::env::var("FRAMES_CONFIG")
        .map_or_else(|_| FramesConfig::default_path(), std::path::PathBuf::from);

    let config = match FramesConfig::load(&config_path) {
        Ok(c) => {
            c.validate().context("config validation failed")?;
            c
        }
        Err(frames_core::ConfigError::NotFound { path }) => {
            anyhow::bail!(
                "config file not found: {}\n\
                 Create it or set FRAMES_CONFIG to point to your config.",
                path.display()
            );
        }
        Err(e) => return Err(e).context("failed to load config"),
    };
    tracing::debug!("config loaded ({:.1}ms)", t0.elapsed().as_secs_f64() * 1000.0);

    // ── 5. GTK init ─────────────────────────────────────────────────────────
    gtk::init().context("failed to initialize GTK")?;
    tracing::debug!("GTK initialised ({:.1}ms)", t0.elapsed().as_secs_f64() * 1000.0);

    // ── 6. Bar window ───────────────────────────────────────────────────────
    let bar_rc = Rc::new(bar::Bar::new(&config.bar));
    tracing::debug!("bar window created ({:.1}ms)", t0.elapsed().as_secs_f64() * 1000.0);

    // ── 7. CSS theme — resolve source, detect dark/light variant, load ──────
    //
    // Priority: --theme CLI > bar.theme config > bar.css raw path > built-in.
    // After GTK init so that resolve_theme_variant can read GtkSettings.
    let effective_theme_name: Option<String> = cli_theme
        .as_deref()
        .or(config.bar.theme.as_deref())
        .map(css::resolve_theme_variant);

    let theme_source: css::ThemeSource<'_> = match effective_theme_name.as_deref() {
        Some(name) => css::ThemeSource::Named(name),
        None => match config.bar.css.as_deref() {
            Some(path) => css::ThemeSource::Path(std::path::Path::new(path)),
            None => css::ThemeSource::Default,
        },
    };

    // Compute the active CSS file path for the hot-reload watcher (Steps 8–9).
    let active_css_path: Option<PathBuf> = match &theme_source {
        css::ThemeSource::Named(name) => {
            let p = css::resolve_theme_path(name);
            if p.as_os_str().is_empty() {
                None
            } else {
                Some(p)
            }
        }
        css::ThemeSource::Path(p) => Some(p.to_path_buf()),
        css::ThemeSource::Default => None,
    };

    let provider: Rc<RefCell<gtk::CssProvider>> =
        Rc::new(RefCell::new(css::load_theme(theme_source)));
    css::apply_provider(&provider.borrow());

    // ── 8. Widgets ──────────────────────────────────────────────────────────
    let poller: Rc<RefCell<Poller>> = Rc::new(RefCell::new(Poller::new()));
    let renderers: Rc<RefCell<Vec<Rc<dyn RendererDispatch>>>> = Rc::new(RefCell::new(Vec::new()));
    let self_poll_ids: Rc<RefCell<Vec<glib::SourceId>>> = Rc::new(RefCell::new(Vec::new()));

    build_all_widgets(
        &bar_rc,
        &mut poller.borrow_mut(),
        &mut renderers.borrow_mut(),
        &mut self_poll_ids.borrow_mut(),
        &config,
    );
    tracing::debug!("widgets built ({:.1}ms)", t0.elapsed().as_secs_f64() * 1000.0);

    // ── 9. Polling timer ────────────────────────────────────────────────────
    // Tick every 100 ms; each widget fires only when its own interval elapses.
    {
        let poller = Rc::clone(&poller);
        let renderers = Rc::clone(&renderers);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let results = poller.borrow_mut().poll(Instant::now());
            for (name, data) in results {
                for renderer in renderers.borrow().iter() {
                    renderer.dispatch(&name, &data);
                }
            }
            ControlFlow::Continue
        });
    }

    // ── 10. SIGTERM handler ─────────────────────────────────────────────────
    let (sig_tx, sig_rx) = mpsc::channel::<()>();
    ctrlc::set_handler(move || {
        let _ = sig_tx.send(());
    })
    .context("failed to install signal handler")?;

    glib::timeout_add_local(Duration::from_millis(100), move || {
        if sig_rx.try_recv().is_ok() {
            tracing::info!("received shutdown signal; exiting");
            gtk::main_quit();
            return ControlFlow::Break;
        }
        ControlFlow::Continue
    });

    // ── 11. Config hot-reload watcher + CSS file hot-reload watcher ─────────
    let config_watcher = match ConfigWatcher::new(&config_path) {
        Ok(w) => {
            tracing::info!(path = %config_path.display(), "config watcher started");
            Some(w)
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not start config watcher; hot-reload disabled");
            None
        }
    };

    // Optional watcher on the active CSS file (only when a file-based theme is used).
    let css_watcher: Option<ConfigWatcher> =
        active_css_path.as_deref().and_then(|p| match ConfigWatcher::new(p) {
            Ok(w) => {
                tracing::info!(path = %p.display(), "CSS theme watcher started");
                Some(w)
            }
            Err(e) => {
                tracing::warn!(error = %e, path = %p.display(),
                    "could not start CSS theme watcher; CSS hot-reload disabled");
                None
            }
        });

    if config_watcher.is_some() || css_watcher.is_some() {
        let bar = Rc::clone(&bar_rc);
        let poller = Rc::clone(&poller);
        let renderers = Rc::clone(&renderers);
        let self_poll_ids = Rc::clone(&self_poll_ids);
        let config_path = config_path.clone();
        let provider = Rc::clone(&provider);
        let mut last_reload = Instant::now();

        glib::timeout_add_local(Duration::from_millis(500), move || {
            // Config change — rebuild widget tree.
            if let Some(ref watcher) = config_watcher {
                if watcher.has_changed() && last_reload.elapsed() >= Duration::from_millis(500) {
                    match FramesConfig::load(&config_path) {
                        Ok(new_config) => {
                            // Cancel widget-owned self-poll timers before clearing the bar.
                            {
                                let mut ids = self_poll_ids.borrow_mut();
                                for id in ids.drain(..) {
                                    id.remove();
                                }
                            }

                            bar.clear_widgets();

                            let mut new_poller = Poller::new();
                            let mut new_renderers = Vec::new();
                            build_all_widgets(
                                &bar,
                                &mut new_poller,
                                &mut new_renderers,
                                &mut self_poll_ids.borrow_mut(),
                                &new_config,
                            );
                            *poller.borrow_mut() = new_poller;
                            *renderers.borrow_mut() = new_renderers;

                            bar.show();
                            last_reload = Instant::now();
                            tracing::info!("config reloaded successfully");
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "config reload failed; keeping current config");
                        }
                    }
                }
            }

            // CSS file change — reapply theme without restarting.
            if let (Some(ref css_w), Some(ref css_path)) = (&css_watcher, &active_css_path) {
                if css_w.has_changed() {
                    let new_provider = css::load_theme(css::ThemeSource::Path(css_path.as_path()));
                    css::remove_provider(&provider.borrow());
                    css::apply_provider(&new_provider);
                    *provider.borrow_mut() = new_provider;
                    tracing::info!(path = %css_path.display(), "CSS theme hot-reloaded");
                }
            }

            ControlFlow::Continue
        });
    }

    // ── 12. Bar show + main loop ─────────────────────────────────────────────
    bar_rc.show();
    tracing::debug!("bar ready, entering main loop ({:.1}ms total)", t0.elapsed().as_secs_f64() * 1000.0);
    gtk::main();
    Ok(())
}

// ── Widget renderer dispatch ─────────────────────────────────────────────────

/// Object-safe dispatch trait so renderers of different types can live in a
/// `Vec<Rc<dyn RendererDispatch>>`.
trait RendererDispatch {
    /// Dispatch new widget data to this renderer if the widget name matches.
    fn dispatch(&self, name: &str, data: &WidgetData);
}
struct ClockRenderer {
    name: String,
    renderer: widgets::clock::ClockWidget,
}

impl RendererDispatch for ClockRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct CpuRenderer {
    name: String,
    renderer: widgets::cpu::CpuWidget,
}

impl RendererDispatch for CpuRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct MemoryRenderer {
    name: String,
    renderer: widgets::memory::MemoryWidget,
}

impl RendererDispatch for MemoryRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct NetworkRenderer {
    name: String,
    renderer: widgets::network::NetworkWidget,
}

impl RendererDispatch for NetworkRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct BatteryRenderer {
    name: String,
    renderer: widgets::battery::BatteryWidget,
}

impl RendererDispatch for BatteryRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct VolumeRenderer {
    name: String,
    renderer: widgets::volume::VolumeWidget,
}

impl RendererDispatch for VolumeRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct BrightnessRenderer {
    name: String,
    renderer: widgets::brightness::BrightnessWidget,
}

impl RendererDispatch for BrightnessRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct WeatherRenderer {
    name: String,
    renderer: widgets::weather::WeatherWidget,
}

impl RendererDispatch for WeatherRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct MediaRenderer {
    name: String,
    renderer: widgets::media::MediaWidget,
}

impl RendererDispatch for MediaRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

struct DiskRenderer {
    name: String,
    renderer: widgets::disk::DiskWidget,
}

impl RendererDispatch for DiskRenderer {
    fn dispatch(&self, name: &str, data: &WidgetData) {
        if name == self.name {
            self.renderer.update(data);
        }
    }
}

// ── Widget factory ───────────────────────────────────────────────────────────

/// Return type for [`build_widget`]: `(renderer, self_poll_source_id)`.
///
/// `renderer` is `Some` for polled widgets. `source_id` is `Some` for
/// self-polling widgets (e.g. workspaces) whose glib timer must be
/// cancelled before a hot-reload.
type BuildWidgetResult = anyhow::Result<(Option<Rc<dyn RendererDispatch>>, Option<glib::SourceId>)>;

/// Build all widgets from `config`, register polled widgets with `poller`,
/// accumulate renderers into `renderers`, and store any widget-owned
/// `glib::SourceId`s (for cancellation on reload) into `self_poll_ids`.
///
/// Widget construction failures are logged as `WARN` and skipped; a single
/// broken widget config does not block the whole bar.
fn build_all_widgets(
    bar: &bar::Bar,
    poller: &mut Poller,
    renderers: &mut Vec<Rc<dyn RendererDispatch>>,
    self_poll_ids: &mut Vec<glib::SourceId>,
    config: &FramesConfig,
) {
    for widget_config in &config.widgets {
        match build_widget(bar, poller, widget_config) {
            Ok((maybe_renderer, maybe_id)) => {
                if let Some(renderer) = maybe_renderer {
                    renderers.push(renderer);
                }
                if let Some(id) = maybe_id {
                    self_poll_ids.push(id);
                }
            }
            Err(e) => {
                tracing::warn!(
                    widget = widget_config.widget_type,
                    error = %e,
                    "failed to create widget; skipping"
                );
            }
        }
    }
}

/// Build one widget from config, register with the poller, add to the bar.
///
/// Returns a pair `(renderer, source_id)`. `renderer` is `Some` for polled
/// widgets. `source_id` is `Some` for self-polling widgets (e.g. workspaces)
/// and holds the glib timer [`glib::SourceId`] for later cancellation on
/// hot-reload. Returns `Err` if widget construction fails.
///
/// # Errors
///
/// Returns an error if the renderer or core widget construction fails.
#[allow(clippy::too_many_lines)] // match arms are the canonical widget registry; refactoring into subfunctions would obscure the pattern
fn build_widget(bar: &bar::Bar, poller: &mut Poller, config: &WidgetConfig) -> BuildWidgetResult {
    let section = config.position.clone();
    let name = config.label.clone().unwrap_or_else(|| config.widget_type.clone());

    match config.widget_type.as_str() {
        "clock" => {
            let interval = config.interval.unwrap_or(1000);
            let format = config.format.clone().unwrap_or_else(|| "%H:%M:%S".to_string());

            let core_widget = frames_core::widgets::clock::ClockWidget::new(&name, &format);
            poller.register(Box::new(core_widget), interval);

            let renderer = widgets::clock::ClockWidget::new(config)
                .context("clock renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);

            Ok((Some(Rc::new(ClockRenderer { name, renderer })), None))
        }
        "cpu" => {
            let interval = config.interval.unwrap_or(2000);

            let core_widget = frames_core::widgets::cpu::CpuWidget::new(&name)?;
            poller.register(Box::new(core_widget), interval);

            let renderer =
                widgets::cpu::CpuWidget::new(config).context("cpu renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);

            Ok((Some(Rc::new(CpuRenderer { name, renderer })), None))
        }
        "memory" => {
            let interval = config.interval.unwrap_or(5000);

            let core_widget = frames_core::widgets::memory::MemoryWidget::new(&name)?;
            poller.register(Box::new(core_widget), interval);

            let renderer = widgets::memory::MemoryWidget::new(config)
                .context("memory renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);

            Ok((Some(Rc::new(MemoryRenderer { name, renderer })), None))
        }
        "network" => {
            let interval = config.interval.unwrap_or(2000);

            let interface = config.interface.clone().unwrap_or_else(|| "eth0".to_string());
            let core_widget = frames_core::widgets::network::NetworkWidget::new(&name, &interface)?;
            poller.register(Box::new(core_widget), interval);

            let renderer = widgets::network::NetworkWidget::new(config)
                .context("network renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);

            Ok((Some(Rc::new(NetworkRenderer { name, renderer })), None))
        }
        "battery" => {
            let interval = config.interval.unwrap_or(30_000);

            let core_widget = frames_core::widgets::battery::BatteryWidget::new(&name);
            poller.register(Box::new(core_widget), interval);

            let renderer = widgets::battery::BatteryWidget::new(config)
                .context("battery renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);

            Ok((Some(Rc::new(BatteryRenderer { name, renderer })), None))
        }
        "volume" => {
            let interval = config.interval.unwrap_or(2000);

            let core_widget = frames_core::widgets::volume::VolumeWidget::new(&name);
            poller.register(Box::new(core_widget), interval);

            let renderer = widgets::volume::VolumeWidget::new(config)
                .context("volume renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);

            Ok((Some(Rc::new(VolumeRenderer { name, renderer })), None))
        }
        "brightness" => {
            let interval = config.interval.unwrap_or(5000);

            let core_widget = frames_core::widgets::brightness::BrightnessWidget::new(&name);
            poller.register(Box::new(core_widget), interval);

            let renderer = widgets::brightness::BrightnessWidget::new(config)
                .context("brightness renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);

            Ok((Some(Rc::new(BrightnessRenderer { name, renderer })), None))
        }
        "workspaces" => {
            // WorkspacesWidget is self-polling; it manages its own glib timer.
            let renderer = Rc::new(
                widgets::workspaces::WorkspacesWidget::new()
                    .context("workspaces renderer construction failed")?,
            );
            add_to_bar(bar, renderer.widget(), config, &section);

            // Initial fill.
            renderer.refresh();

            // Self-polling timer: refresh workspace buttons every 500 ms.
            // The SourceId is returned so it can be cancelled on hot-reload.
            let renderer_clone = Rc::clone(&renderer);
            let interval_ms = config.interval.unwrap_or(500);
            let source_id =
                glib::timeout_add_local(Duration::from_millis(interval_ms), move || {
                    renderer_clone.refresh();
                    ControlFlow::Continue
                });

            // Workspaces does not participate in the Poller dispatch loop.
            Ok((None, Some(source_id)))
        }
        "launcher" => {
            // LauncherWidget is self-contained; it manages its own GTK signals.
            let renderer = Rc::new(
                widgets::launcher::LauncherWidget::new(config)
                    .context("launcher renderer construction failed")?,
            );
            add_to_bar(bar, renderer.widget(), config, &section);
            // Launcher does not participate in the Poller dispatch loop.
            Ok((None, None))
        }
        "separator" => {
            // Thin visual divider between widgets. Renders a configurable glyph
            // (default "|") styled by .widget-separator in the theme. The glyph
            // is set via the `format` field in config. Does not poll or dispatch.
            let glyph = config.format.as_deref().unwrap_or("|");
            let label = gtk::Label::new(Some(glyph));
            label.set_widget_name("separator");
            label.style_context().add_class("widget-separator");
            if let Some(cls) = &config.extra_class {
                label.style_context().add_class(cls.as_str());
            }
            bar.add_widget(label.upcast_ref(), &section);
            Ok((None, None))
        }
        "weather" => {
            let interval = config.interval.unwrap_or(1_800_000);
            let latitude = config.latitude.unwrap_or(0.0);
            let longitude = config.longitude.unwrap_or(0.0);
            let unit = match config.units.as_deref().unwrap_or("celsius") {
                "fahrenheit" => frames_core::widget::TempUnit::Fahrenheit,
                _ => frames_core::widget::TempUnit::Celsius,
            };
            let core_widget = frames_core::widgets::weather::WeatherWidget::new(
                &name, latitude, longitude, unit,
            );
            poller.register(Box::new(core_widget), interval);
            let renderer = widgets::weather::WeatherWidget::new(config)
                .context("weather renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);
            Ok((Some(Rc::new(WeatherRenderer { name, renderer })), None))
        }
        "media" => {
            let interval = config.interval.unwrap_or(2000);
            let core_widget = frames_core::widgets::media::MediaWidget::new(&name);
            poller.register(Box::new(core_widget), interval);
            let renderer = widgets::media::MediaWidget::new(config)
                .context("media renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);
            Ok((Some(Rc::new(MediaRenderer { name, renderer })), None))
        }
        "disk" => {
            let interval = config.interval.unwrap_or(30_000);
            let mount = config.mount.clone().unwrap_or_else(|| "/".to_string());
            let core_widget = frames_core::widgets::disk::DiskWidget::new(&name, &mount)?;
            poller.register(Box::new(core_widget), interval);
            let renderer = widgets::disk::DiskWidget::new(config)
                .context("disk renderer construction failed")?;
            add_to_bar(bar, renderer.widget(), config, &section);
            Ok((Some(Rc::new(DiskRenderer { name, renderer })), None))
        }
        other => {
            tracing::warn!(widget_type = other, "unknown widget type in config; skipping");
            Ok((None, None))
        }
    }
}

/// Add a widget to the bar, wrapping in an `EventBox` when click or scroll
/// actions are configured in `config`.
///
/// When any of `on_click`, `on_scroll_up`, or `on_scroll_down` is set, the
/// raw widget is placed inside an `EventBox` with appropriate signal handlers
/// before being added to the bar section. Otherwise the widget is added
/// directly.
///
/// If `config.extra_class` is set, the CSS class is applied to whatever
/// container is added to the section box (the `EventBox` when wrapping, the
/// widget itself when not).
fn add_to_bar(bar: &bar::Bar, widget: &gtk::Widget, config: &WidgetConfig, section: &BarSection) {
    let has_actions = config.on_click.is_some()
        || config.on_scroll_up.is_some()
        || config.on_scroll_down.is_some();

    if !has_actions {
        if let Some(cls) = &config.extra_class {
            widget.style_context().add_class(cls.as_str());
        }
        bar.add_widget(widget, section);
        return;
    }

    let event_box = gtk::EventBox::new();
    event_box.add(widget);

    if let Some(cls) = &config.extra_class {
        event_box.style_context().add_class(cls.as_str());
    }

    if let Some(cmd) = config.on_click.clone() {
        event_box.connect_button_press_event(move |_, event| {
            if event.button() == 1 {
                spawn_shell(&cmd);
            }
            glib::Propagation::Proceed
        });
    }

    if config.on_scroll_up.is_some() || config.on_scroll_down.is_some() {
        event_box.add_events(gdk::EventMask::SCROLL_MASK);
        let up_cmd = config.on_scroll_up.clone();
        let down_cmd = config.on_scroll_down.clone();
        event_box.connect_scroll_event(move |_, event| {
            match event.direction() {
                gdk::ScrollDirection::Up => {
                    if let Some(cmd) = &up_cmd {
                        spawn_shell(cmd);
                    }
                }
                gdk::ScrollDirection::Down => {
                    if let Some(cmd) = &down_cmd {
                        spawn_shell(cmd);
                    }
                }
                _ => {}
            }
            glib::Propagation::Proceed
        });
    }

    bar.add_widget(event_box.upcast_ref(), section);
}

/// Spawn a shell command asynchronously via `sh -c`.
///
/// Failures (e.g. `sh` not on `PATH`) are logged as `WARN` and ignored so a
/// misconfigured action does not crash or hang the bar.
fn spawn_shell(cmd: &str) {
    if let Err(e) = std::process::Command::new("sh").args(["-c", cmd]).spawn() {
        tracing::warn!(command = cmd, error = %e, "widget action command failed to spawn");
    }
}
