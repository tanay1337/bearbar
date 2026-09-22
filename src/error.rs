use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing value after {0}")]
    MissingArgument(&'static str),
    #[error("unknown argument: {0}")]
    UnknownArgument(String),
    #[error("could not read configuration at {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid configuration at {path}: {source}")]
    ParseConfig {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("could not initialize the background runtime: {0}")]
    Runtime(std::io::Error),
    #[error("GTK could not open a display")]
    NoDisplay,
}

pub type Result<T> = std::result::Result<T, Error>;
