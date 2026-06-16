use thiserror::Error;

/// Top-level error type for chukei-core.
///
/// Variants map onto the CLI exit codes documented in the PRD §13.1:
/// Config → 2, Connectivity → 3, Auth → 4, Plugin → 5.
#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("upstream connectivity error: {0}")]
    Connectivity(String),

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("plugin '{plugin}' error: {message}")]
    Plugin { plugin: String, message: String },

    #[error("SQL parse error: {0}")]
    SqlParse(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("replay error: {0}")]
    Replay(String),

    #[error("evidence error: {0}")]
    Evidence(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

impl Error {
    /// CLI exit code per PRD §13.1.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Config(_) | Error::Yaml(_) => 2,
            Error::Connectivity(_) => 3,
            Error::Auth(_) => 4,
            Error::Plugin { .. } => 5,
            _ => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
