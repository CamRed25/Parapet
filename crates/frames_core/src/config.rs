//! TOML configuration model for Frames.
//!
//! [`FramesConfig`] is the top-level structure loaded from
//! `~/.config/frames/config.toml` (or the path in `FRAMES_CONFIG`).
//! All fields have sane defaults so the config file section is optional.
//!
//! See `CONFIG_MODEL.md` for the full field documentation and example configs.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

// ── Top-level ────────────────────────────────────────────────────────────────

/// Top-level configuration, loaded from `~/.config/frames/config.toml`.
///
/// The config file is not created automatically if absent — the bar exits with
/// a clear error message pointing to the expected path.
#[derive(Debug, Clone, Deserialize)]
pub struct FramesConfig {
    /// Bar window configuration (position, height, monitor, CSS).
    #[serde(default)]
    pub bar: BarConfig,

    /// Ordered list of widget definitions. Widgets render in config order
    /// within their respective section (left / center / right).
    #[serde(default)]
    pub widgets: Vec<WidgetConfig>,
}

impl FramesConfig {
    /// Load and validate the config from `path`.
    ///
    /// Returns [`ConfigError::NotFound`] if the file does not exist,
    /// [`ConfigError::Io`] on read failure, [`ConfigError::Parse`] on invalid
    /// TOML, and [`ConfigError::Validation`] if field values fail validation
    /// rules (see `CONFIG_MODEL §5`).
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] on any of the above failure modes.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::NotFound {
                path: path.to_path_buf(),
            });
        }
        let source = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&source)?;
        config.validate()?;
        Ok(config)
    }

    /// Return the XDG default config path: `~/.config/frames/config.toml`.
    ///
    /// Uses the `HOME` environment variable. Falls back to `/root` if `HOME`
    /// is unset (unusual in a normal desktop session).
    #[must_use]
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        PathBuf::from(home).join(".config").join("frames").join("config.toml")
    }

    /// Validate all fields according to `CONFIG_MODEL §5`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Validation`] describing the first violated rule.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.bar.height == 0 {
            return Err(ConfigError::Validation {
                field: "bar.height".to_string(),
                reason: "must be greater than 0".to_string(),
            });
        }
        for (i, w) in self.widgets.iter().enumerate() {
            // CPU: warn_threshold < crit_threshold
            if w.widget_type == "cpu" {
                let warn = w.warn_threshold.unwrap_or(80.0);
                let crit = w.crit_threshold.unwrap_or(95.0);
                if warn >= crit {
                    return Err(ConfigError::Validation {
                        field: format!("widgets[{i}].warn_threshold"),
                        reason: format!(
                            "CPU warn_threshold ({warn}) must be less than crit_threshold ({crit})"
                        ),
                    });
                }
            }
            // Battery: crit_threshold < warn_threshold
            if w.widget_type == "battery" {
                let warn = w.warn_threshold.unwrap_or(20.0);
                let crit = w.crit_threshold.unwrap_or(5.0);
                if crit >= warn {
                    return Err(ConfigError::Validation {
                        field: format!("widgets[{i}].crit_threshold"),
                        reason: format!(
                            "battery crit_threshold ({crit}) must be less than warn_threshold ({warn})"
                        ),
                    });
                }
            }
        }
        Ok(())
    }
}

// ── Bar config ────────────────────────────────────────────────────────────────

/// Global bar window configuration.
///
/// All fields are optional in the TOML source; `Default` provides sensible
/// values so `[bar]` can be omitted entirely.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BarConfig {
    /// Screen edge where the bar is anchored. Default: `Top`.
    #[serde(default)]
    pub position: BarPosition,

    /// Bar height in pixels. Must be > 0. Default: `30`.
    #[serde(default = "BarConfig::default_height")]
    pub height: u32,

    /// Which monitor to display on. Default: `Primary`.
    #[serde(default)]
    pub monitor: MonitorTarget,

    /// Path to a user CSS file. `~` is expanded by the loader. `None` uses
    /// the built-in default theme compiled into `frames_bar`.
    #[serde(default)]
    pub css: Option<String>,

    /// Named theme to load from `~/.config/frames/themes/<name>.css`.
    ///
    /// Takes precedence over `css` when both are set. `None` falls through to
    /// `css`; if that is also `None`, the built-in default theme is used.
    /// Use the `--theme` CLI flag to override this field at runtime.
    #[serde(default)]
    pub theme: Option<String>,

    /// Pixel gap between adjacent widgets. Default: `4`.
    #[serde(default = "BarConfig::default_widget_spacing")]
    pub widget_spacing: u32,
}

impl BarConfig {
    fn default_height() -> u32 {
        30
    }

    fn default_widget_spacing() -> u32 {
        4
    }
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            position: BarPosition::default(),
            height: Self::default_height(),
            monitor: MonitorTarget::default(),
            css: None,
            theme: None,
            widget_spacing: Self::default_widget_spacing(),
        }
    }
}

/// Screen edge where the bar is anchored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    /// Anchor the bar to the top of the screen.
    #[default]
    Top,
    /// Anchor the bar to the bottom of the screen.
    Bottom,
}

/// Which monitor to display the bar on.
///
/// Serializes as `"primary"` (string) or an integer index. Custom serde
/// impl required because `#[serde(untagged)]` cannot round-trip unit variants
/// in TOML.
#[derive(Debug, Clone, Default)]
pub enum MonitorTarget {
    /// Display on the primary monitor as reported by GDK.
    #[default]
    Primary,
    /// Use a specific 0-based GDK monitor index.
    Index(usize),
}

impl Serialize for MonitorTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            MonitorTarget::Primary => serializer.serialize_str("primary"),
            // clippy::cast_possible_truncation: monitor index fits in u64 on any real machine
            #[allow(clippy::cast_possible_truncation)]
            MonitorTarget::Index(i) => serializer.serialize_u64(*i as u64),
        }
    }
}

impl<'de> Deserialize<'de> for MonitorTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MonitorVisitor;

        impl serde::de::Visitor<'_> for MonitorVisitor {
            type Value = MonitorTarget;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, r#"\"primary\" or an integer monitor index"#)
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                if v == "primary" {
                    Ok(MonitorTarget::Primary)
                } else {
                    Err(E::custom(format!("unknown monitor target: {v}")))
                }
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                usize::try_from(v).map(MonitorTarget::Index).map_err(E::custom)
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                usize::try_from(v).map(MonitorTarget::Index).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(MonitorVisitor)
    }
}

/// Bar section (column) that a widget belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BarSection {
    /// Left-aligned section (packed with `expand=false`).
    Left,
    /// Centre section (expands to fill remaining space).
    Center,
    /// Right-aligned section (packed with `expand=false`).
    Right,
}

// ── Widget config ─────────────────────────────────────────────────────────────

/// Configuration for a single widget entry in `[[widgets]]`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WidgetConfig {
    /// Widget type identifier. Must match one of the known types in
    /// `CONFIG_MODEL §4.2` (e.g. `"clock"`, `"cpu"`, `"battery"`).
    #[serde(rename = "type")]
    pub widget_type: String,

    /// Bar section to place this widget in.
    pub position: BarSection,

    /// Polling interval in milliseconds. `None` uses the per-widget default
    /// defined in `CONFIG_MODEL §4.2`.
    #[serde(default)]
    pub interval: Option<u64>,

    /// Static text label displayed before the widget value.
    #[serde(default)]
    pub label: Option<String>,

    // Per-widget optional fields ──────────────────────────────────────────────
    /// Clock: `chrono::format` pattern string (e.g. `"%H:%M:%S"`).
    /// Memory: display mode — `"used"`, `"free"`, or `"percent"`.
    /// Separator: glyph text rendered as the divider (default `"|"`).
    #[serde(default)]
    pub format: Option<String>,

    /// Clock: `"local"` or an IANA timezone name (e.g. `"America/New_York"`).
    #[serde(default)]
    pub timezone: Option<String>,

    /// Network: interface name (e.g. `"eth0"`) or `"auto"`.
    #[serde(default)]
    pub interface: Option<String>,

    /// Network: whether to prefix the display with the interface name.
    #[serde(default)]
    pub show_interface: Option<bool>,

    /// Memory: whether to include swap usage in the display.
    #[serde(default)]
    pub show_swap: Option<bool>,

    /// Battery: whether to show a Unicode battery icon.
    #[serde(default)]
    pub show_icon: Option<bool>,

    /// Workspaces: whether to display workspace names or just numbers.
    #[serde(default)]
    pub show_names: Option<bool>,

    /// CPU / battery: percentage above (CPU) or below (battery) which the
    /// `.warning` CSS class is applied.
    #[serde(default)]
    pub warn_threshold: Option<f32>,

    /// CPU / battery: percentage above (CPU) or below (battery) which the
    /// `.critical` CSS class is applied.
    #[serde(default)]
    pub crit_threshold: Option<f32>,

    /// Launcher: maximum number of search results shown in the popup list.
    /// Defaults to 10 when absent.
    #[serde(default)]
    pub max_results: Option<u32>,

    /// Weather: WGS-84 latitude in decimal degrees (negative = south).
    /// Required for the `"weather"` widget type.
    #[serde(default)]
    pub latitude: Option<f64>,

    /// Weather: WGS-84 longitude in decimal degrees (negative = west).
    /// Required for the `"weather"` widget type.
    #[serde(default)]
    pub longitude: Option<f64>,

    /// Weather: temperature unit — `"celsius"` (default) or `"fahrenheit"`.
    #[serde(default)]
    pub units: Option<String>,

    /// Disk: filesystem mount point to monitor (e.g. `"/"` or `"/home"`).
    /// Defaults to `"/"` when absent.
    #[serde(default)]
    pub mount: Option<String>,

    // Per-widget action bindings ────────────────────────────────────────────────
    /// Shell command to execute when the widget is left-clicked.
    ///
    /// Spawned via `sh -c <command>`. Example: `"pavucontrol"`.
    #[serde(default)]
    pub on_click: Option<String>,

    /// Shell command to execute on scroll-wheel up.
    ///
    /// Spawned via `sh -c <command>`. Example: `"pactl set-sink-volume @DEFAULT_SINK@ +5%"`.
    #[serde(default)]
    pub on_scroll_up: Option<String>,

    /// Shell command to execute on scroll-wheel down.
    ///
    /// Spawned via `sh -c <command>`. Example: `"pactl set-sink-volume @DEFAULT_SINK@ -5%"`.
    #[serde(default)]
    pub on_scroll_down: Option<String>,

    /// Extra CSS class name applied to this widget's root GTK container.
    ///
    /// Allows theme authors to target individual widget instances in CSS
    /// without modifying widget source code. For example,
    /// `extra_class = "my-clock"` can be styled with
    /// `.my-clock { color: #ff0; }` in the user theme.
    #[serde(default)]
    pub extra_class: Option<String>,
}

// ── Config watcher ────────────────────────────────────────────────────────────

/// File watcher for the Frames config file.
///
/// Spawns a `notify` background watcher on construction. On any file-system
/// event for the watched path, an internal channel is signalled. Poll
/// [`ConfigWatcher::has_changed`] from the GTK main thread to check without
/// blocking.
///
/// Drop the `ConfigWatcher` to stop watching. The background watcher thread
/// is joined on drop automatically by the `notify` crate.
///
/// # Errors
///
/// [`ConfigWatcher::new`] returns [`ConfigError::Io`] if the `notify` watcher
/// cannot be initialised or the path cannot be registered.
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<()>,
}

impl ConfigWatcher {
    /// Start watching `path` for modifications.
    ///
    /// Returns a `ConfigWatcher` that will signal on any `Modify`, `Create`,
    /// or `Remove` event for the given path. Does not require the file to
    /// exist at construction time.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] if the underlying watcher cannot be created
    /// or the path cannot be registered.
    pub fn new(path: &Path) -> Result<Self, ConfigError> {
        let (tx, rx) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
                Ok(event) => {
                    use notify::EventKind;
                    if matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                    ) {
                        // Ignore send errors — receiver may have been dropped during shutdown.
                        let _ = tx.send(());
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "notify watcher error");
                }
            })
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        watcher
            .watch(path, RecursiveMode::NonRecursive)
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// Returns `true` if at least one file-system event has arrived since the
    /// last call that returned `true`.
    ///
    /// Drains all pending events so the next call starts clean.
    /// Never blocks.
    #[must_use]
    pub fn has_changed(&self) -> bool {
        let mut changed = false;
        // Drain all pending events — only the fact of change matters, not the count.
        while self.rx.try_recv().is_ok() {
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_config_default_height_is_thirty() {
        assert_eq!(BarConfig::default().height, 30);
    }

    #[test]
    fn bar_config_default_position_is_top() {
        assert_eq!(BarConfig::default().position, BarPosition::Top);
    }

    #[test]
    fn bar_config_default_widget_spacing_is_four() {
        assert_eq!(BarConfig::default().widget_spacing, 4);
    }

    #[test]
    fn config_parses_valid_toml() {
        let toml = r#"
[bar]
position = "top"
height = 28

[[widgets]]
type = "clock"
position = "center"
format = "%H:%M"
"#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        assert_eq!(config.bar.height, 28);
        assert_eq!(config.bar.position, BarPosition::Top);
        assert_eq!(config.widgets.len(), 1);
        assert_eq!(config.widgets[0].widget_type, "clock");
        assert_eq!(config.widgets[0].format.as_deref(), Some("%H:%M"));
    }

    #[test]
    fn config_uses_defaults_when_bar_section_absent() {
        let toml = "[[widgets]]\ntype = \"clock\"\nposition = \"center\"\n";
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        assert_eq!(config.bar.height, BarConfig::default().height);
        assert_eq!(config.bar.position, BarPosition::Top);
    }

    #[test]
    fn config_validation_rejects_zero_height() {
        let toml = "[bar]\nheight = 0\nposition = \"top\"\n";
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        let result = config.validate();
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("bar.height"), "error should mention field; got: {msg}");
    }

    #[test]
    fn config_validation_rejects_inverted_cpu_thresholds() {
        let toml = r#"
[[widgets]]
type = "cpu"
position = "right"
warn_threshold = 95.0
crit_threshold = 80.0
"#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        let result = config.validate();
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("warn_threshold"),
            "error should mention threshold field; got: {msg}"
        );
    }

    #[test]
    fn config_validation_accepts_valid_thresholds() {
        let toml = r#"
[bar]
height = 30
position = "top"

[[widgets]]
type = "cpu"
position = "right"
warn_threshold = 80.0
crit_threshold = 95.0
"#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn launcher_max_results_field_round_trips() {
        let toml = r#"
            [[widgets]]
            type = "launcher"
            position = "center"
            max_results = 15
        "#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        assert_eq!(config.widgets[0].max_results, Some(15));
    }

    #[test]
    fn config_watcher_new_on_existing_file_succeeds() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[bar]\n").expect("write");
        let watcher = ConfigWatcher::new(&path);
        assert!(watcher.is_ok(), "watcher creation should succeed");
    }

    #[test]
    fn config_watcher_has_changed_returns_true_after_write() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[bar]\n").expect("initial write");
        let watcher = ConfigWatcher::new(&path).expect("watcher");

        // Give the watcher thread time to register before the write.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&path, "[bar]\nheight = 28\n").expect("modify");
        std::thread::sleep(std::time::Duration::from_millis(100));

        assert!(watcher.has_changed(), "should detect file modification");
    }

    #[test]
    fn widget_config_parses_on_click_and_scroll_actions() {
        let toml = r#"
            [[widgets]]
            type = "volume"
            position = "right"
            on_click = "pavucontrol"
            on_scroll_up = "pactl set-sink-volume @DEFAULT_SINK@ +5%"
            on_scroll_down = "pactl set-sink-volume @DEFAULT_SINK@ -5%"
        "#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        let w = &config.widgets[0];
        assert_eq!(w.on_click.as_deref(), Some("pavucontrol"));
        assert_eq!(w.on_scroll_up.as_deref(), Some("pactl set-sink-volume @DEFAULT_SINK@ +5%"));
        assert_eq!(w.on_scroll_down.as_deref(), Some("pactl set-sink-volume @DEFAULT_SINK@ -5%"));
    }

    #[test]
    fn widget_config_on_click_defaults_to_none() {
        let toml = r#"
            [[widgets]]
            type = "clock"
            position = "center"
        "#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        let w = &config.widgets[0];
        assert!(w.on_click.is_none());
        assert!(w.on_scroll_up.is_none());
        assert!(w.on_scroll_down.is_none());
    }

    #[test]
    fn extra_class_round_trip() {
        let toml = r#"
            [[widgets]]
            type = "clock"
            position = "center"
            extra_class = "my-clock"
        "#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        assert_eq!(config.widgets[0].extra_class.as_deref(), Some("my-clock"));
    }

    #[test]
    fn extra_class_absent_is_none() {
        let toml = r#"
            [[widgets]]
            type = "clock"
            position = "center"
        "#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        assert!(config.widgets[0].extra_class.is_none());
    }

    #[test]
    fn bar_theme_field_round_trip() {
        let toml = r#"
            [bar]
            theme = "dark"
        "#;
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        assert_eq!(config.bar.theme.as_deref(), Some("dark"));
    }

    #[test]
    fn bar_theme_field_absent_is_none() {
        let toml = "[[widgets]]\ntype = \"clock\"\nposition = \"center\"\n";
        let config: FramesConfig = toml::from_str(toml).expect("valid toml");
        assert!(config.bar.theme.is_none());
    }
}
