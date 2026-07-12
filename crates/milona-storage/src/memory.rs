//! In-memory `VectorStore`/`GraphStore` implementations.
//!
//! These back Phase 2's unit tests without needing a live Mongo instance.
//! They implement the same tenant-isolation discipline required of the real
//! Mongo-backed stores in this crate: every stored record carries a
//! `tenant_id`, and every read filters by it inside the "query" itself
//! (`search`/`traverse`) rather than as a post-filter bolted on afterward —
//! mirroring what `$vectorSearch`/`$graphLookup` must do against Mongo.

use async_trait::async_trait;
use milona_core::document::{Chunk, ChunkId, DocumentId};
use milona_core::error::CoreError;
use milona_core::tenant::{TenantContext, TenantId};
use milona_core::traits::{GraphEdge, GraphStore, VectorMatch, VectorStore};
use std::collections::HashMap;
use std::sync::Mutex;

struct StoredVector {
    tenant_id: TenantId,
    document_id: DocumentId,
    embedding: Vec<f32>,
}

/// In-memory `VectorStore`. Cosine similarity over a `Vec` guarded by a
/// `Mutex` — fine for unit tests, not for production traffic.
#[derive(Default)]
pub struct InMemoryVectorStore {
    records: Mutex<HashMap<ChunkId, StoredVector>>,
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn upsert(
        &self,
        ctx: &TenantContext,
        chunk: &Chunk,
        embedding: &[f32],
    ) -> Result<(), CoreError> {
        let mut records = self.records.lock().unwrap();
        records.insert(
            chunk.id,
            StoredVector {
                tenant_id: ctx.tenant_id,
                document_id: chunk.document_id,
                embedding: embedding.to_vec(),
            },
        );
        Ok(())
    }

    async fn search(
        &self,
        ctx: &TenantContext,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorMatch>, CoreError> {
        let records = self.records.lock().unwrap();
        // Tenant filter is applied in the same scan that computes scores,
        // not as a post-filter over an already-scored/truncated set — this
        // mirrors the requirement that the real Mongo store's
        // `$vectorSearch` stage carries the tenant filter itself.
        let mut scored: Vec<VectorMatch> = records
            .iter()
            .filter(|(_, v)| v.tenant_id == ctx.tenant_id)
            .map(|(id, v)| VectorMatch {
                chunk_id: *id,
                score: cosine_similarity(query_embedding, &v.embedding),
            })
            .collect();
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        scored.truncate(top_k);
        Ok(scored)
    }

    async fn delete_document(
        &self,
        ctx: &TenantContext,
        document_id: DocumentId,
    ) -> Result<(), CoreError> {
        let mut records = self.records.lock().unwrap();
        records.retain(|_, v| !(v.tenant_id == ctx.tenant_id && v.document_id == document_id));
        Ok(())
    }
}

struct StoredEdge {
    tenant_id: TenantId,
    edge: GraphEdge,
}

/// In-memory `GraphStore` performing bounded-depth BFS traversal, tenant
/// filtered inside `traverse` itself (the in-process equivalent of a Mongo
/// `$graphLookup`'s `restrictSearchWithMatch`). Always reports
/// `supports_traversal() == true` since it has no DocumentDB-style
/// limitation — see [`crate::mongo::MongoGraphStore`] for the capability
/// gate that matters in production.
#[derive(Default)]
pub struct InMemoryGraphStore {
    edges: Mutex<Vec<StoredEdge>>,
}

#[async_trait]
impl GraphStore for InMemoryGraphStore {
    async fn add_edge(&self, ctx: &TenantContext, edge: GraphEdge) -> Result<(), CoreError> {
        let mut edges = self.edges.lock().unwrap();
        edges.push(StoredEdge {
            tenant_id: ctx.tenant_id,
            edge,
        });
        Ok(())
    }

    async fn traverse(
        &self,
        ctx: &TenantContext,
        start_node: &str,
        max_depth: usize,
    ) -> Result<Vec<GraphEdge>, CoreError> {
        let edges = self.edges.lock().unwrap();
        // Tenant filter applied in the same pass that walks the graph, not
        // as a post-filter over a cross-tenant traversal result.
        let tenant_edges: Vec<&GraphEdge> = edges
            .iter()
            .filter(|e| e.tenant_id == ctx.tenant_id)
            .map(|e| &e.edge)
            .collect();

        let mut visited = std::collections::HashSet::new();
        let mut frontier = vec![start_node.to_string()];
        let mut result = Vec::new();
        visited.insert(start_node.to_string());

        for _ in 0..max_depth {
            let mut next_frontier = Vec::new();
            for node in &frontier {
                for e in tenant_edges.iter().filter(|e| &e.from == node) {
                    result.push((*e).clone());
                    if visited.insert(e.to.clone()) {
                        next_frontier.push(e.to.clone());
                    }
                }
            }
            if next_frontier.is_empty() {
                break;
            }
            frontier = next_frontier;
        }

        Ok(result)
    }

    fn supports_traversal(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use milona_core::tenant::Role;
    use uuid::Uuid;

    fn ctx(tenant: TenantId, subject: &str) -> TenantContext {
        TenantContext::new(tenant, Role::Member, subject)
    }

    #[tokio::test]
    async fn vector_store_cross_tenant_query_returns_zero_results() {
        let store = InMemoryVectorStore::default();
        let tenant_a = TenantId::new(Uuid::new_v4());
        let tenant_b = TenantId::new(Uuid::new_v4());
        let ctx_a = ctx(tenant_a, "a");
        let ctx_b = ctx(tenant_b, "b");

        let chunk = Chunk {
            id: ChunkId::new(),
            document_id: DocumentId::new(),
            text: "hello".into(),
            sequence: 0,
            token_count: 1,
        };
        store.upsert(&ctx_a, &chunk, &[1.0, 0.0]).await.unwrap();

        // Tenant B's query must never see tenant A's vector, regardless of
        // cosine similarity.
        let hits_b = store.search(&ctx_b, &[1.0, 0.0], 10).await.unwrap();
        assert!(hits_b.is_empty());

        let hits_a = store.search(&ctx_a, &[1.0, 0.0], 10).await.unwrap();
        assert_eq!(hits_a.len(), 1);
        assert_eq!(hits_a[0].chunk_id, chunk.id);
    }

    #[tokio::test]
    async fn vector_store_delete_document_is_tenant_scoped() {
        let store = InMemoryVectorStore::default();
        let tenant_a = TenantId::new(Uuid::new_v4());
        let tenant_b = TenantId::new(Uuid::new_v4());
        let ctx_a = ctx(tenant_a, "a");
        let ctx_b = ctx(tenant_b, "b");

        let doc_id = DocumentId::new();
        let chunk = Chunk {
            id: ChunkId::new(),
            document_id: doc_id,
            text: "hello".into(),
            sequence: 0,
            token_count: 1,
        };
        store.upsert(&ctx_a, &chunk, &[1.0, 0.0]).await.unwrap();

        // Tenant B deleting "the same" document id must not affect tenant A's data.
        store.delete_document(&ctx_b, doc_id).await.unwrap();
        let hits_a = store.search(&ctx_a, &[1.0, 0.0], 10).await.unwrap();
        assert_eq!(hits_a.len(), 1);

        store.delete_document(&ctx_a, doc_id).await.unwrap();
        let hits_a = store.search(&ctx_a, &[1.0, 0.0], 10).await.unwrap();
        assert!(hits_a.is_empty());
    }

    #[tokio::test]
    async fn graph_store_cross_tenant_traversal_returns_zero_results() {
        let store = InMemoryGraphStore::default();
        let tenant_a = TenantId::new(Uuid::new_v4());
        let tenant_b = TenantId::new(Uuid::new_v4());
        let ctx_a = ctx(tenant_a, "a");
        let ctx_b = ctx(tenant_b, "b");

        store
            .add_edge(
                &ctx_a,
                GraphEdge {
                    from: "n1".into(),
                    to: "n2".into(),
                    relation: "rel".into(),
                },
            )
            .await
            .unwrap();

        let result_b = store.traverse(&ctx_b, "n1", 3).await.unwrap();
        assert!(result_b.is_empty());

        let result_a = store.traverse(&ctx_a, "n1", 3).await.unwrap();
        assert_eq!(result_a.len(), 1);
    }

    #[tokio::test]
    async fn graph_store_bounded_depth_traversal() {
        let store = InMemoryGraphStore::default();
        let tenant = TenantId::new(Uuid::new_v4());
        let ctx = ctx(tenant, "a");

        for (from, to) in [("n1", "n2"), ("n2", "n3"), ("n3", "n4")] {
            store
                .add_edge(
                    &ctx,
                    GraphEdge {
                        from: from.into(),
                        to: to.into(),
                        relation: "rel".into(),
                    },
                )
                .await
                .unwrap();
        }

        let depth_1 = store.traverse(&ctx, "n1", 1).await.unwrap();
        assert_eq!(depth_1.len(), 1);

        let depth_3 = store.traverse(&ctx, "n1", 3).await.unwrap();
        assert_eq!(depth_3.len(), 3);
    }

    #[test]
    fn in_memory_graph_store_always_supports_traversal() {
        let store = InMemoryGraphStore::default();
        assert!(store.supports_traversal());
    }
}
