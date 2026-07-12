//! Phase 1 — Ingestion pipeline. Implements
//! `milona_core::traits::{DocumentSource, Chunker, Embedder}`.
//!
//! - [`sources`]: local text file, local PDF, and basic web fetch
//!   `DocumentSource` implementations.
//! - [`chunker::RecursiveChunker`]: recursive/size-aware chunking via
//!   `text-splitter`.
//! - [`embedder::MockEmbedder`]: a **placeholder** deterministic hash-based
//!   embedder — see its doc comment for why `fastembed-rs` could not be
//!   wired in this sandbox (rustc MSRV conflict, same class of issue as
//!   `milona-adapter`'s `genai` substitution).
//!
//! All ingested text carries `TrustLabel::UntrustedIngestedContent`
//! (`milona-core`) once chunked — nothing produced here should ever be
//! placed in an LLM system/instruction role by callers (Phase 3).

pub mod chunker;
pub mod embedder;
mod error;
pub mod sources;

pub use chunker::RecursiveChunker;
pub use embedder::MockEmbedder;
pub use error::IngestError;
pub use sources::{PdfFileSource, TextFileSource, WebSource};
