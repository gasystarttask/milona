use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(uuid::Uuid);

impl DocumentId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for DocumentId {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalized output of ingestion, before chunking. `source_kind` and
/// `origin` exist so a downstream trust-boundary check (Phase 0.5) can tell
/// ingested content apart from anything else in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawDocument {
    pub id: DocumentId,
    pub text: String,
    pub source_kind: SourceKind,
    pub origin: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    Text,
    Pdf,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkId(uuid::Uuid);

impl ChunkId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for ChunkId {
    fn default() -> Self {
        Self::new()
    }
}

/// A chunk of a document, always marked untrusted: nothing in this struct
/// may be treated as an instruction by the GenAI application loop (Phase 3),
/// only as retrievable data. See ROADMAP.md Phase 0.5, "Prompt-injection &
/// content trust boundary".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: ChunkId,
    pub document_id: DocumentId,
    pub text: String,
    pub sequence: usize,
    pub token_count: usize,
}

impl Chunk {
    pub fn trust_label(&self) -> TrustLabel {
        TrustLabel::UntrustedIngestedContent
    }
}

/// Explicit trust label attached to any text that originated from ingestion.
/// The GenAI application loop must render this content in a data role, never
/// concatenate it into a system/instruction role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLabel {
    UntrustedIngestedContent,
}
