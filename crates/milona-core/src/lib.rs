pub mod authz;
pub mod document;
pub mod error;
pub mod tenant;
pub mod traits;

pub use document::{Chunk, ChunkId, DocumentId, RawDocument, SourceKind, TrustLabel};
pub use error::CoreError;
pub use tenant::{Role, TenantContext, TenantId};
pub use traits::{
    Chunker, DocumentSource, Embedder, GraphEdge, GraphStore, LlmMessage, LlmProvider, LlmResponse,
    MessageRole, Tool, ToolInvocation, ToolResult, VectorMatch, VectorStore,
};
