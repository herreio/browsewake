use thiserror::Error;

#[derive(Error, Debug)]
pub enum BrowseWakeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid mozlz4 file: {0}")]
    MozLz4(String),

    #[error("LZ4 decompression error: {0}")]
    Lz4(String),

    #[error("SNSS parse error: {0}")]
    Snss(String),

    #[cfg(target_os = "macos")]
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("No profile found for {0}")]
    NoProfile(String),

    #[error("Browser not supported on this platform: {0}")]
    Unsupported(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, BrowseWakeError>;
