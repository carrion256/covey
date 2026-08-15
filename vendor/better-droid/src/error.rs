use thiserror::Error;

/// Errors returned by Better Droid mission lint/compile.
#[derive(Debug, Error)]
pub enum BetterDroidError {
    #[error("invalid Better Droid source in {path}: {detail}")]
    InvalidSource { path: String, detail: String },
    #[error(
        "output path must stay inside the project and outside openspec mission artifacts: {path}"
    )]
    OutputPathEscape { path: String },
    #[error("failed filesystem operation for {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Result alias for Better Droid operations.
pub type Result<T> = std::result::Result<T, BetterDroidError>;
