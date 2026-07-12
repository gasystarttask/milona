//! Phase 3 — Knowledge façade combining vector search and graph traversal, AuthZ-checked.
//!
//! `Knowledge` is the single retrieval entry point the GenAI application loop
//! (see [`genai_loop`]) is required to go through. It enforces
//! [`milona_core::authz::AuthzPolicy`] itself — not just at the HTTP edge —
//! so that internal callers (tools, MCP, background jobs) cannot bypass
//! tenant isolation by calling a `VectorStore`/`GraphStore` directly.
//!
//! Everything retrieved through this facade is `Chunk`-derived ingested
//! content and is therefore always untrusted (see
//! `milona_core::document::TrustLabel`); the [`genai_loop`] module is the
//! concrete enforcement point that keeps it out of the LLM's system/control
//! channel.

pub mod fakes;
pub mod genai_loop;
pub mod registry;

use milona_core::authz::AuthzPolicy;
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{GraphEdge, GraphStore, VectorStore};
use std::sync::Arc;

/// A single retrieved item, combining a vector-search hit with any graph
/// edges reachable from it. `chunk_id` identifies content that is always
/// untrusted ingested content (see `milona_core::document::TrustLabel`) —
/// callers must not place its text in an LLM system/instruction role.
#[derive(Debug, Clone)]
pub struct RetrievedItem {
    pub chunk_id: milona_core::document::ChunkId,
    pub score: f32,
    /// Graph edges whose `from` node matches this chunk's id, if the graph
    /// store supports traversal. Empty when the store doesn't support it or
    /// no edges exist.
    pub related_edges: Vec<GraphEdge>,
}

/// Combines vector similarity search and graph traversal into a single
/// tenant-scoped, AuthZ-checked retrieval API.
pub struct Knowledge<V, G, P>
where
    V: VectorStore + ?Sized,
    G: GraphStore + ?Sized,
    P: AuthzPolicy + ?Sized,
{
    vector_store: Arc<V>,
    graph_store: Arc<G>,
    policy: Arc<P>,
    /// Max graph traversal depth performed per retrieved chunk.
    graph_traversal_depth: usize,
}

impl<V, G, P> Knowledge<V, G, P>
where
    V: VectorStore + ?Sized,
    G: GraphStore + ?Sized,
    P: AuthzPolicy + ?Sized,
{
    pub fn new(vector_store: Arc<V>, graph_store: Arc<G>, policy: Arc<P>) -> Self {
        Self {
            vector_store,
            graph_store,
            policy,
            graph_traversal_depth: 2,
        }
    }

    pub fn with_graph_traversal_depth(mut self, depth: usize) -> Self {
        self.graph_traversal_depth = depth;
        self
    }

    /// Retrieve the top-k most similar chunks for `query_embedding`, each
    /// enriched with graph edges reachable from it, all scoped to
    /// `ctx.tenant_id`.
    ///
    /// Rejects the call outright (`CoreError::Unauthorized`) if `policy`
    /// denies `ctx` read access to its own tenant scope — this is the
    /// application-layer enforcement point required by ROADMAP.md Phase 0.5,
    /// independent of any HTTP-edge check.
    pub async fn retrieve(
        &self,
        ctx: &TenantContext,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<RetrievedItem>, CoreError> {
        if !self.policy.can_read(ctx, ctx.tenant_id) {
            return Err(CoreError::Unauthorized(format!(
                "tenant {} (role {:?}) is not permitted to read its own knowledge scope",
                ctx.tenant_id, ctx.role
            )));
        }

        let matches = self
            .vector_store
            .search(ctx, query_embedding, top_k)
            .await?;

        let mut items = Vec::with_capacity(matches.len());
        for m in matches {
            let related_edges = if self.graph_store.supports_traversal() {
                let start_node = chunk_id_as_node(&m.chunk_id);
                self.graph_store
                    .traverse(ctx, &start_node, self.graph_traversal_depth)
                    .await
                    .unwrap_or_else(|err| {
                        tracing::warn!(error = %err, "graph traversal failed, continuing with vector-only result");
                        Vec::new()
                    })
            } else {
                Vec::new()
            };

            items.push(RetrievedItem {
                chunk_id: m.chunk_id,
                score: m.score,
                related_edges,
            });
        }

        Ok(items)
    }
}

/// Canonical graph-node key for a chunk id. Kept as a free function (rather
/// than a method on the `milona-core` type) so graph node naming conventions
/// stay a `milona-knowledge` concern.
pub fn chunk_id_as_node(chunk_id: &milona_core::document::ChunkId) -> String {
    format!("{:?}", chunk_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fakes::{InMemoryGraphStore, InMemoryVectorStore};
    use milona_core::authz::SameTenantPolicy;
    use milona_core::document::{Chunk, ChunkId, DocumentId};
    use milona_core::tenant::{Role, TenantContext, TenantId};
    use uuid::Uuid;

    fn make_chunk(seq: usize) -> Chunk {
        Chunk {
            id: ChunkId::new(),
            document_id: DocumentId::new(),
            text: format!("chunk body {seq}"),
            sequence: seq,
            token_count: 3,
        }
    }

    #[tokio::test]
    async fn retrieval_respects_tenant_isolation() {
        let vector_store = Arc::new(InMemoryVectorStore::default());
        let graph_store = Arc::new(InMemoryGraphStore::default());
        let policy = Arc::new(SameTenantPolicy);

        let tenant_a = TenantId::new(Uuid::new_v4());
        let tenant_b = TenantId::new(Uuid::new_v4());
        let ctx_a = TenantContext::new(tenant_a, Role::Member, "user-a");
        let ctx_b = TenantContext::new(tenant_b, Role::Member, "user-b");

        let chunk_a = make_chunk(1);
        vector_store
            .upsert(&ctx_a, &chunk_a, &[1.0, 0.0, 0.0])
            .await
            .unwrap();

        let knowledge = Knowledge::new(vector_store.clone(), graph_store.clone(), policy);

        // Tenant A can see its own chunk.
        let result_a = knowledge
            .retrieve(&ctx_a, &[1.0, 0.0, 0.0], 10)
            .await
            .unwrap();
        assert_eq!(result_a.len(), 1);
        assert_eq!(result_a[0].chunk_id, chunk_a.id);

        // Tenant B, querying the same embedding, gets nothing — isolation is
        // enforced at the query/aggregation level inside the fake store, not
        // as a post-filter.
        let result_b = knowledge
            .retrieve(&ctx_b, &[1.0, 0.0, 0.0], 10)
            .await
            .unwrap();
        assert!(result_b.is_empty());
    }

    #[tokio::test]
    async fn unauthorized_ctx_is_rejected() {
        // A policy that always denies, to prove Knowledge enforces AuthzPolicy
        // itself rather than relying solely on the HTTP edge.
        struct DenyAll;
        impl AuthzPolicy for DenyAll {
            fn can_read(&self, _ctx: &TenantContext, _resource_tenant: TenantId) -> bool {
                false
            }
            fn can_write(&self, _ctx: &TenantContext, _resource_tenant: TenantId) -> bool {
                false
            }
        }

        let vector_store = Arc::new(InMemoryVectorStore::default());
        let graph_store = Arc::new(InMemoryGraphStore::default());
        let policy = Arc::new(DenyAll);
        let knowledge = Knowledge::new(vector_store, graph_store, policy);

        let tenant = TenantId::new(Uuid::new_v4());
        let ctx = TenantContext::new(tenant, Role::Member, "user-1");

        let err = knowledge
            .retrieve(&ctx, &[1.0, 0.0, 0.0], 5)
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn retrieval_includes_graph_edges_when_supported() {
        let vector_store = Arc::new(InMemoryVectorStore::default());
        let graph_store = Arc::new(InMemoryGraphStore::default());
        let policy = Arc::new(SameTenantPolicy);

        let tenant = TenantId::new(Uuid::new_v4());
        let ctx = TenantContext::service(tenant);

        let chunk = make_chunk(1);
        vector_store
            .upsert(&ctx, &chunk, &[1.0, 0.0])
            .await
            .unwrap();

        let node = chunk_id_as_node(&chunk.id);
        graph_store
            .add_edge(
                &ctx,
                GraphEdge {
                    from: node.clone(),
                    to: "related-node".to_string(),
                    relation: "mentions".to_string(),
                },
            )
            .await
            .unwrap();

        let knowledge = Knowledge::new(vector_store, graph_store, policy);
        let result = knowledge.retrieve(&ctx, &[1.0, 0.0], 10).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].related_edges.len(), 1);
        assert_eq!(result[0].related_edges[0].to, "related-node");
    }
}
