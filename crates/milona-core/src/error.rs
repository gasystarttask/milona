use thiserror::Error;

/// Shared error type for all trait boundaries in `milona-core`. Concrete
/// crates (`milona-storage`, `milona-ingest`, ...) define their own richer
/// error types and convert into this at the trait boundary.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("tenant isolation violation: {0}")]
    TenantIsolationViolation(String),

    #[error("unsupported capability: {0}")]
    Unsupported(String),

    #[error("upstream provider error: {0}")]
    Upstream(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
