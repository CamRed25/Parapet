//! Config round-trip serialization tests.
//!
//! Verifies that each config struct can be serialized to TOML and deserialized
//! back to an equal value (per TESTING_GUIDE §6).

use frames_core::config::{BarConfig, BarPosition, BarSection, FramesConfig, WidgetConfig};

#[test]
fn bar_config_round_trips_through_toml() {
    let config = BarConfig {
        position: BarPosition::Top,
        height: 28,
        monitor: frames_core::config::MonitorTarget::Primary,
        css: None,
        theme: None,
        widget_spacing: 4,
    };

    let serialized = toml::to_string(&config).expect("serialize BarConfig");
    let deserialized: BarConfig = toml::from_str(&serialized).expect("deserialize BarConfig");

    assert_eq!(config.position, deserialized.position);
    assert_eq!(config.height, deserialized.height);
    assert_eq!(config.widget_spacing, deserialized.widget_spacing);
    assert_eq!(config.css, deserialized.css);
}

#[test]
fn bar_config_bottom_round_trips() {
    let config = BarConfig {
        position: BarPosition::Bottom,
        height: 32,
        monitor: frames_core::config::MonitorTarget::Index(1),
        css: Some("~/.config/frames/frames.css".to_string()),
        theme: None,
        widget_spacing: 6,
    };

    let serialized = toml::to_string(&config).expect("serialize");
    let deserialized: BarConfig = toml::from_str(&serialized).expect("deserialize");

    assert_eq!(config.position, deserialized.position);
    assert_eq!(config.height, deserialized.height);
    assert_eq!(config.css, deserialized.css);
}

#[test]
fn widget_config_clock_round_trips() {
    let config = WidgetConfig {
        widget_type: "clock".to_string(),
        position: BarSection::Center,
        interval: Some(1000),
        label: None,
        format: Some("%H:%M:%S".to_string()),
        timezone: Some("local".to_string()),
        interface: None,
        show_interface: None,
        show_swap: None,
        show_icon: None,
        show_names: None,
        warn_threshold: None,
        crit_threshold: None,
        max_results: None,
        latitude: None,
        longitude: None,
        units: None,
        mount: None,
        on_click: None,
        on_scroll_up: None,
        on_scroll_down: None,
        extra_class: None,
    };

    let serialized = toml::to_string(&config).expect("serialize WidgetConfig");
    let deserialized: WidgetConfig = toml::from_str(&serialized).expect("deserialize WidgetConfig");

    assert_eq!(config.widget_type, deserialized.widget_type);
    assert_eq!(config.format, deserialized.format);
    assert_eq!(config.interval, deserialized.interval);
}

#[test]
fn full_config_round_trips() {
    let toml_src = r#"
[bar]
position = "bottom"
height = 32
widget_spacing = 6

[[widgets]]
type = "workspaces"
position = "left"

[[widgets]]
type = "clock"
position = "center"
format = "%a %b %d  %H:%M"

[[widgets]]
type = "cpu"
position = "right"
interval = 2000
warn_threshold = 80.0
crit_threshold = 95.0

[[widgets]]
type = "battery"
position = "right"
warn_threshold = 20.0
crit_threshold = 5.0
"#;

    let config: FramesConfig = toml::from_str(toml_src).expect("parse full config");
    assert_eq!(config.bar.height, 32);
    assert_eq!(config.widgets.len(), 4);
    assert_eq!(config.widgets[2].widget_type, "cpu");
    assert_eq!(config.widgets[2].warn_threshold, Some(80.0));
}

#[test]
fn weather_widget_config_parses_latitude_longitude_units() {
    let toml_src = r#"
[[widgets]]
type = "weather"
position = "right"
latitude = 51.5085
longitude = -0.1257
units = "celsius"
interval = 1800000
"#;

    let config: FramesConfig = toml::from_str(toml_src).expect("parse weather config");
    assert_eq!(config.widgets.len(), 1);
    let w = &config.widgets[0];
    assert_eq!(w.widget_type, "weather");
    assert_eq!(w.latitude, Some(51.5085));
    assert_eq!(w.longitude, Some(-0.1257));
    assert_eq!(w.units.as_deref(), Some("celsius"));
    assert_eq!(w.interval, Some(1_800_000));
}

#[test]
fn weather_widget_config_fahrenheit() {
    let toml_src = r#"
[[widgets]]
type = "weather"
position = "right"
latitude = 40.71
longitude = -74.01
units = "fahrenheit"
"#;

    let config: FramesConfig = toml::from_str(toml_src).expect("parse weather fahrenheit");
    assert_eq!(config.widgets[0].units.as_deref(), Some("fahrenheit"));
    assert!((config.widgets[0].latitude.unwrap() - 40.71).abs() < 0.001);
}

#[test]
fn weather_widget_config_defaults_when_coords_absent() {
    // latitude and longitude are optional; build_widget defaults them to 0.0
    let toml_src = r#"
[[widgets]]
type = "weather"
position = "right"
"#;

    let config: FramesConfig = toml::from_str(toml_src).expect("parse weather no coords");
    let w = &config.widgets[0];
    assert_eq!(w.latitude, None);
    assert_eq!(w.longitude, None);
    assert_eq!(w.units, None);
}

#[test]
fn media_widget_config_parses_minimal() {
    let toml_src = r#"
[[widgets]]
type = "media"
position = "center"
"#;

    let config: FramesConfig = toml::from_str(toml_src).expect("parse media config");
    assert_eq!(config.widgets.len(), 1);
    let w = &config.widgets[0];
    assert_eq!(w.widget_type, "media");
    assert_eq!(w.interval, None);
}

#[test]
fn media_widget_config_with_interval_and_actions() {
    let toml_src = r#"
[[widgets]]
type = "media"
position = "center"
interval = 2000
on_click = "playerctl play-pause"
on_scroll_up = "playerctl next"
on_scroll_down = "playerctl previous"
"#;

    let config: FramesConfig = toml::from_str(toml_src).expect("parse media with actions");
    let w = &config.widgets[0];
    assert_eq!(w.interval, Some(2000));
    assert_eq!(w.on_click.as_deref(), Some("playerctl play-pause"));
    assert_eq!(w.on_scroll_up.as_deref(), Some("playerctl next"));
    assert_eq!(w.on_scroll_down.as_deref(), Some("playerctl previous"));
}

#[test]
fn disk_widget_config_parses_mount() {
    let toml_src = r#"
[[widgets]]
type = "disk"
position = "right"
mount = "/home"
format = "percent"
interval = 30000
"#;

    let config: FramesConfig = toml::from_str(toml_src).expect("parse disk config");
    assert_eq!(config.widgets.len(), 1);
    let w = &config.widgets[0];
    assert_eq!(w.widget_type, "disk");
    assert_eq!(w.mount.as_deref(), Some("/home"));
    assert_eq!(w.format.as_deref(), Some("percent"));
    assert_eq!(w.interval, Some(30000));
}

#[test]
fn disk_widget_config_defaults_when_mount_absent() {
    let toml_src = r#"
[[widgets]]
type = "disk"
position = "right"
"#;

    let config: FramesConfig = toml::from_str(toml_src).expect("parse disk no mount");
    let w = &config.widgets[0];
    assert_eq!(w.mount, None);
    assert_eq!(w.format, None);
}

