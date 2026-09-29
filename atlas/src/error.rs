//! What can go wrong, and how it gets said.
//!
//! One error type for the whole program, with the variants named after what
//! actually happened rather than after where it happened. `Platform` rather
//! than `WinApiError`, because the person reading it doesn't care which layer
//! failed — they care that something outside Atlas said no.
//!
//! Every variant carries enough to act on. An error that says "failed" and
//! nothing else forces whoever hits it to reproduce the problem to find out
//! what it was, which is the whole cost of the error all over again.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AtlasError {
    #[error("config: {0}")]
    Config(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml in {path}: {source}")]
    Yaml {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("unknown app '{0}' (not defined in config/apps.yaml)")]
    UnknownApp(String),
    #[error("unknown layout '{0}' (not defined in config/layouts.yaml)")]
    UnknownLayout(String),
    #[error("no monitor could be assigned to role '{0}'")]
    NoMonitorForRole(String),
    #[error("window for '{0}' never appeared after {1} attempts")]
    WindowNeverAppeared(String, u32),
    #[error("platform: {0}")]
    Platform(String),
    #[error("blocked: '{0}' requires explicit approval")]
    ApprovalRequired(String),
}

pub type Result<T> = std::result::Result<T, AtlasError>;
