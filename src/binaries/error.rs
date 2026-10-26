use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BinaryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SHA-256 verification failed for '{binary}'. Expected: {expected}, calculated: {actual}")]
    ChecksumMismatch {
        binary: String,
        expected: String,
        actual: String,
    },

    #[error("Checksum entry not found for asset '{asset_name}' in manifest from {url}")]
    ChecksumNotFound {
        asset_name: String,
        url: String,
    },

    #[error("Archive extraction failed: {0}")]
    ArchiveExtraction(String),

    #[error("Unsupported platform: OS={os}, Arch={arch}")]
    PlatformNotSupported {
        os: String,
        arch: String,
    },

    #[error("Binary smoke test failed for '{name}' at {path:?}: {details}")]
    ExecutionFailed {
        name: String,
        path: PathBuf,
        details: String,
    },

    #[error("Binary '{0}' not found on system")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}
