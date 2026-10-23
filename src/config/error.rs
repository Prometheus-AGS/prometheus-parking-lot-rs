//! Errors from loading and validating scheduler configuration.

use std::fmt;

use prometheus_parking_lot_core::SchedulerError;

/// Why a scheduler configuration was rejected.
///
/// Both variants name what is wrong: a [`Parse`](Self::Parse) error carries
/// `serde_json`'s message, which names an unknown or missing field (for
/// example ``unknown field `max_unit`, expected one of `max_units`, ...``) and
/// the line and column; an [`Invalid`](Self::Invalid) error carries the dotted
/// path of the offending field, such as `pools.inference.max_units`.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The input is not valid JSON for the schema: malformed JSON, an unknown
    /// or misspelled field, a missing required field, or a value of the wrong
    /// type.
    Parse(serde_json::Error),
    /// The input parsed, but a value is not acceptable.
    Invalid {
        /// Dotted path of the offending field, e.g. `pools.inference.max_units`.
        field: String,
        /// What is wrong with it.
        reason: String,
    },
}

impl ConfigError {
    /// An [`Invalid`](Self::Invalid) error for `field`.
    pub fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// The dotted path of the offending field, for an
    /// [`Invalid`](Self::Invalid) error.
    #[must_use]
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::Invalid { field, .. } => Some(field),
            Self::Parse(_) => None,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "scheduler config does not match the schema: {error}"),
            Self::Invalid { field, reason } => write!(f, "{field}: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Invalid { .. } => None,
        }
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(error: serde_json::Error) -> Self {
        Self::Parse(error)
    }
}

impl From<ConfigError> for SchedulerError {
    fn from(error: ConfigError) -> Self {
        Self::invalid_config(error.to_string())
    }
}
