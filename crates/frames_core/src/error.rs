//! Error types for `frames_core`.
//!
//! All fallible public functions in this crate return [`FramesError`]. The
//! [`ConfigError`] type covers config-specific failures and is wrapped by
//! [`FramesError::Config`]. [`FramesError::Http`] covers network fetch failures
//! (e.g. the weather widget) and [`FramesError::DBus`] covers D-Bus IPC
//! failures (e.g. the MPRIS media widget).

use std::path::PathBuf;

/// Top-level error type for `frames_core`.
///
/// All fallible public functions in this crate return this type. Use the
/// sub-variants to distinguish between config, system information, battery
/// read, and widget-not-found failures.
#[derive(Debug, thiserror::Error)]
pub enum FramesError {
    /// A configuration error. Wraps [`ConfigError`].
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    /// A system information error. Contains a human-readable description.
    #[error("sysinfo error: {0}")]
    SysInfo(String),

    /// A battery read error from `/sys/class/power_supply/`. Wraps [`std::io::Error`].
    #[error("battery read error: {0}")]
    Battery(#[from] std::io::Error),

    /// A requested widget was not found in the active widget registry.
    #[error("widget not found: {name}")]
    WidgetNotFound {
        /// The name of the missing widget as used in config.
        name: String,
    },

    /// An HTTP error occurred while fetching widget data (e.g. the weather API).
    ///
    /// Contains a human-readable description of the failure.
    #[error("http error: {0}")]
    Http(String),

    /// A D-Bus error occurred while querying a widget data source (e.g. MPRIS).
    ///
    /// Contains a human-readable description of the failure.
    #[error("dbus error: {0}")]
    DBus(String),
}

/// Configuration-specific error variants, wrapped by [`FramesError::Config`].
///
/// Used by [`crate::config::FramesConfig::load`] and related functions to
/// distinguish between missing files, parse failures, and validation errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The config file was not found at the expected path.
    #[error("config file not found at {path}")]
    NotFound {
        /// The path that was searched.
        path: PathBuf,
    },

    /// The TOML source could not be parsed.
    #[error("config parse error: {0}")]
    Parse(#[from] toml::de::Error),

    /// A field was present but failed validation rules.
    #[error("config validation error in field '{field}': {reason}")]
    Validation {
        /// The TOML field path that failed validation (e.g. `"bar.height"`).
        field: String,
        /// Human-readable explanation of the violated rule.
        reason: String,
    },

    /// An I/O error occurred while reading the config file.
    #[error("io error reading config: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn config_error_display_not_found() {
        let path = PathBuf::from("/home/user/.config/frames/config.toml");
        let err = ConfigError::NotFound { path: path.clone() };
        let msg = err.to_string();
        assert!(
            msg.contains("/home/user/.config/frames/config.toml"),
            "display must include the path; got: {msg}"
        );
    }

    #[test]
    fn config_error_display_validation() {
        let err = ConfigError::Validation {
            field: "bar.height".to_string(),
            reason: "must be greater than 0".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("bar.height"), "display must include field name; got: {msg}");
        assert!(
            msg.contains("must be greater than 0"),
            "display must include reason; got: {msg}"
        );
    }

    #[test]
    fn frames_error_from_config_error() {
        fn propagate() -> Result<(), FramesError> {
            let e = ConfigError::NotFound {
                path: PathBuf::from("/tmp/test"),
            };
            Err(e)?
        }
        let result = propagate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FramesError::Config(_)));
    }

    #[test]
    fn frames_error_from_io_error() {
        fn propagate() -> Result<(), FramesError> {
            let e = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
            Err(e)?
        }
        let result = propagate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FramesError::Battery(_)));
    }

    #[test]
    fn frames_error_sysinfo_display() {
        let err = FramesError::SysInfo("cpu data unavailable".to_string());
        assert!(err.to_string().contains("cpu data unavailable"));
    }

    #[test]
    fn frames_error_http_display() {
        let err = FramesError::Http("connection refused on port 443".to_string());
        let msg = err.to_string();
        assert!(
            msg.contains("connection refused on port 443"),
            "display must include the message; got: {msg}"
        );
    }

    #[test]
    fn frames_error_dbus_display() {
        let err = FramesError::DBus("org.freedesktop.DBus.Error.ServiceUnknown".to_string());
        let msg = err.to_string();
        assert!(
            msg.contains("org.freedesktop.DBus.Error.ServiceUnknown"),
            "display must include the message; got: {msg}"
        );
    }
}
