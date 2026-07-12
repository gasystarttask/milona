use crate::document::{Chunk, ChunkId, DocumentId, RawDocument};
use crate::error::CoreError;
use crate::tenant::TenantContext;
use async_trait::async_trait;

/// A source of raw documents (file, PDF, web, ...). Implemented in
/// `milona-ingest` (Phase 1).
#[async_trait]
pub trait DocumentSource: Send + Sync {
    async fn fetch(&self, ctx: &TenantContext, location: &str) -> Result<RawDocument, CoreError>;
}

/// Splits a `RawDocument` into `Chunk`s. Implemented in `milona-ingest`.
pub trait Chunker: Send + Sync {
    fn chunk(&self, document: &RawDocument) -> Result<Vec<Chunk>, CoreError>;
}

/// Produces a vector embedding for a piece of text. Implemented in
/// `milona-ingest` (fastembed-rs) or swapped for a hosted API later.
#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, CoreError>;
    fn dimensions(&self) -> usize;
}

/// A single vector search hit.
#[derive(Debug, Clone)]
pub struct VectorMatch {
    pub chunk_id: ChunkId,
    pub score: f32,
}

/// Vector storage/search, always tenant-scoped. Every implementation MUST
/// include `ctx.tenant_id` in the underlying query filter/aggregation stage
/// itself (not a post-filter) — see ROADMAP.md Phase 0.5, "Tenant data
/// isolation". Implemented in `milona-storage` (Phase 2).
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(
        &self,
        ctx: &TenantContext,
        chunk: &Chunk,
        embedding: &[f32],
    ) -> Result<(), CoreError>;

    async fn search(
        &self,
        ctx: &TenantContext,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorMatch>, CoreError>;

    async fn delete_document(
        &self,
        ctx: &TenantContext,
        document_id: DocumentId,
    ) -> Result<(), CoreError>;
}

/// A directed edge in the knowledge graph.
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
}

/// Graph storage/traversal, always tenant-scoped — every edge and every
/// traversal query must carry `tenant_id`. Implemented in `milona-storage`
/// (Phase 2) over MongoDB `$graphLookup`, with a capability check that fails
/// fast on DocumentDB (see ROADMAP.md Key Risk #1).
#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn add_edge(&self, ctx: &TenantContext, edge: GraphEdge) -> Result<(), CoreError>;

    async fn traverse(
        &self,
        ctx: &TenantContext,
        start_node: &str,
        max_depth: usize,
    ) -> Result<Vec<GraphEdge>, CoreError>;

    /// Returns Err if the backing store cannot support graph traversal
    /// (e.g. DocumentDB without `$graphLookup`) rather than silently
    /// degrading.
    fn supports_traversal(&self) -> bool;
}

/// A chat message role, kept separate from ingested content by construction:
/// `Data` is the only role ingested/retrieved text may be placed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    /// Retrieved/ingested content — never treated as an instruction.
    Data,
}

#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A swappable LLM provider adapter. Implemented in `milona-adapter` (Phase
/// 3) wrapping the `genai` crate; provider selection is a config value.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, messages: &[LlmMessage]) -> Result<LlmResponse, CoreError>;
    fn provider_name(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct ToolInvocation {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
}

/// A tool the GenAI application can invoke, whether native or MCP-backed.
/// Implemented in `milona-tools` (Phase 5).
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn invoke(
        &self,
        ctx: &TenantContext,
        invocation: ToolInvocation,
    ) -> Result<ToolResult, CoreError>;
}
