//! Volume widget — audio output level data provider.
//!
//! Queries the default PulseAudio/PipeWire sink volume and mute state by
//! spawning `pactl`. Works headlessly — no display dependency.
//!
//! Falls back to the last known value on `pactl` failures so a brief audio
//! daemon restart does not blank the widget.

use std::process::Command;

use crate::error::FramesError;
use crate::widget::{Widget, WidgetData};

/// Provides audio output volume and mute state by querying `pactl`.
pub struct VolumeWidget {
    name: String,
    last_data: Option<WidgetData>,
}

impl VolumeWidget {
    /// Create a new `VolumeWidget` targeting the default audio sink.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            last_data: None,
        }
    }

    /// Query `pactl` for volume percentage and mute state.
    ///
    /// Returns `None` if `pactl` is absent or produces unrecognisable output.
    fn read_volume() -> Option<(f32, bool)> {
        let vol_out = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
            .ok()?;
        let vol_str = String::from_utf8_lossy(&vol_out.stdout);
        // Format: "Volume: front-left: 45875 / 70% / -8.66 dB, ..."
        // Split on '/' — first field is raw value, second is percentage.
        let vol_pct =
            vol_str.split('/').nth(1)?.trim().trim_end_matches('%').parse::<f32>().ok()?;

        let mute_out =
            Command::new("pactl").args(["get-sink-mute", "@DEFAULT_SINK@"]).output().ok()?;
        // Format: "Mute: yes" or "Mute: no"
        let muted = String::from_utf8_lossy(&mute_out.stdout).to_lowercase().contains("yes");

        Some((vol_pct, muted))
    }
}

impl Widget for VolumeWidget {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self) -> Result<WidgetData, FramesError> {
        let data = match Self::read_volume() {
            Some((volume_pct, muted)) => WidgetData::Volume { volume_pct, muted },
            None => self.last_data.clone().unwrap_or(WidgetData::Volume {
                volume_pct: 0.0,
                muted: false,
            }),
        };
        self.last_data = Some(data.clone());
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_widget_name_non_empty() {
        let w = VolumeWidget::new("vol");
        assert!(!w.name().is_empty());
    }

    #[test]
    fn volume_widget_satisfies_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VolumeWidget>();
    }

    #[test]
    fn volume_update_does_not_error() {
        // pactl may be absent in CI; widget must return Ok(…) regardless.
        let mut w = VolumeWidget::new("vol");
        assert!(w.update().is_ok());
    }
}
