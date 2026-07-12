//! Crate-local error type for `milona-ingest`, converted into
//! [`milona_core::error::CoreError`] at trait boundaries (`DocumentSource`,
//! `Chunker`, `Embedder`).

use milona_core::error::CoreError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("io error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("pdf extraction failed for {origin}: {message}")]
    Pdf { origin: String, message: String },

    #[error("http fetch failed for {url}: {message}")]
    Http { url: String, message: String },

    #[error("invalid url: {0}")]
    InvalidUrl(String),

    #[error("chunking failed: {0}")]
    Chunking(String),

    #[error("embedding failed: {0}")]
    Embedding(String),
}

impl From<IngestError> for CoreError {
    fn from(err: IngestError) -> Self {
        match err {
            IngestError::InvalidUrl(msg) => CoreError::InvalidInput(msg),
            other => CoreError::Other(anyhow::anyhow!(other)),
        }
    }
}
