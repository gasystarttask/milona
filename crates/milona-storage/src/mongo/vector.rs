//! `VectorStore` over MongoDB Atlas `$vectorSearch`.
//!
//! Every document stored carries a `tenant_id` field, and every `search`
//! query includes `tenant_id` as a `filter` clause *inside* the
//! `$vectorSearch` aggregation stage itself — not a `$match` stage appended
//! after it — per ROADMAP.md Phase 0.5 ("every `$vectorSearch` query
//! includes a `tenant_id` filter in the same aggregation stage"). Atlas
//! Search evaluates `filter` as part of the ANN search itself, so this is
//! both correct isolation and the fast path (no full scan-then-discard).

use crate::backend::MongoBackend;
use crate::mongo::{deserialize_uuid_newtype, serialize_uuid_newtype};
use async_trait::async_trait;
use futures_util::TryStreamExt;
use milona_core::document::{Chunk, ChunkId, DocumentId};
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{VectorMatch, VectorStore};
use mongodb::bson::{doc, Bson};
use mongodb::Collection;
use serde::{Deserialize, Serialize};

/// On-disk shape of a vector record. `tenant_id` is mandatory on every
/// document so a query that forgets to filter by it would still be scoped
/// by the aggregation `filter` clause built in [`MongoVectorStore::search`]
/// — the schema alone doesn't enforce isolation, the query construction
/// does, but keeping the field on every document is what makes that
/// `filter` possible in the first place.
#[derive(Debug, Serialize, Deserialize)]
pub struct VectorDocument {
    #[serde(rename = "_id")]
    chunk_id: String,
    tenant_id: String,
    document_id: String,
    text: String,
    sequence: i64,
    token_count: i64,
    embedding: Vec<f32>,
}

/// Name of the Atlas Search vector index expected to exist on the
/// `embedding` field of the backing collection. Index creation is an
/// out-of-band, one-time Atlas admin operation (via `search_index` API or
/// the Atlas UI/Terraform) — this crate does not attempt to create it
/// automatically since `$vectorSearch` index management is
/// deployment/infra concern, not part of the `VectorStore` trait's
/// per-request contract.
pub const DEFAULT_VECTOR_INDEX_NAME: &str = "milona_vector_index";

/// `VectorStore` implementation backed by a single MongoDB collection and
/// an Atlas Search vector index.
pub struct MongoVectorStore {
    collection: Collection<VectorDocument>,
    backend: MongoBackend,
    index_name: String,
}

impl MongoVectorStore {
    /// `collection` should already have an Atlas Search vector index named
    /// `index_name` on its `embedding` field (see
    /// [`DEFAULT_VECTOR_INDEX_NAME`]).
    pub fn new(
        collection: Collection<VectorDocument>,
        backend: MongoBackend,
        index_name: impl Into<String>,
    ) -> Self {
        Self {
            collection,
            backend,
            index_name: index_name.into(),
        }
    }

    /// Convenience constructor using [`DEFAULT_VECTOR_INDEX_NAME`].
    pub fn with_default_index(
        collection: Collection<VectorDocument>,
        backend: MongoBackend,
    ) -> Self {
        Self::new(collection, backend, DEFAULT_VECTOR_INDEX_NAME)
    }

    pub fn backend(&self) -> MongoBackend {
        self.backend
    }
}

#[async_trait]
impl VectorStore for MongoVectorStore {
    async fn upsert(
        &self,
        ctx: &TenantContext,
        chunk: &Chunk,
        embedding: &[f32],
    ) -> Result<(), CoreError> {
        let doc = VectorDocument {
            chunk_id: serialize_uuid_newtype(&chunk.id),
            tenant_id: ctx.tenant_id.to_string(),
            document_id: chunk.document_id.as_uuid().to_string(),
            text: chunk.text.clone(),
            sequence: chunk.sequence as i64,
            token_count: chunk.token_count as i64,
            embedding: embedding.to_vec(),
        };

        // Filter on both _id AND tenant_id: an upsert must never let one
        // tenant overwrite a chunk id that happens to collide with
        // another's (chunk ids are UUIDv4 so this is theoretical, but the
        // filter is what makes the tenant scoping structural rather than
        // convention-based).
        let filter = doc! {
            "_id": &doc.chunk_id,
            "tenant_id": ctx.tenant_id.to_string(),
        };

        self.collection
            .replace_one(filter, &doc)
            .upsert(true)
            .await
            .map_err(|e| CoreError::Other(anyhow::anyhow!("mongo upsert failed: {e}")))?;

        Ok(())
    }

    async fn search(
        &self,
        ctx: &TenantContext,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorMatch>, CoreError> {
        if !self.backend.supports_vector_search() {
            return Err(CoreError::Unsupported(
                "$vectorSearch is not supported on the configured MongoBackend (DocumentDB)"
                    .to_string(),
            ));
        }

        let limit = top_k.max(1) as i64;
        // `numCandidates` is Atlas's ANN recall/latency knob; a common
        // default is 10-20x the requested limit, capped to a sane floor.
        let num_candidates = (limit * 20).max(100);

        // The tenant_id filter lives INSIDE the $vectorSearch stage's
        // `filter` field — evaluated as part of the ANN search itself, not
        // a $match appended afterward. This is the literal mechanism
        // ROADMAP.md Phase 0.5 requires ("not a post-filter").
        let pipeline = vec![
            doc! {
                "$vectorSearch": {
                    "index": &self.index_name,
                    "path": "embedding",
                    "queryVector": query_embedding.iter().map(|f| Bson::Double(*f as f64)).collect::<Vec<_>>(),
                    "numCandidates": num_candidates,
                    "limit": limit,
                    "filter": {
                        "tenant_id": ctx.tenant_id.to_string(),
                    },
                }
            },
            doc! {
                "$project": {
                    "_id": 1,
                    "score": { "$meta": "vectorSearchScore" },
                }
            },
        ];

        let mut cursor = self
            .collection
            .clone_with_type::<mongodb::bson::Document>()
            .aggregate(pipeline)
            .await
            .map_err(|e| CoreError::Other(anyhow::anyhow!("mongo $vectorSearch failed: {e}")))?;

        let mut matches = Vec::new();
        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| CoreError::Other(anyhow::anyhow!("mongo cursor error: {e}")))?
        {
            let id_str = doc
                .get_str("_id")
                .map_err(|e| CoreError::Other(anyhow::anyhow!("missing _id in result: {e}")))?;
            let score = doc.get_f64("score").unwrap_or(0.0) as f32;
            let chunk_id: ChunkId = deserialize_uuid_newtype(id_str)?;
            matches.push(VectorMatch { chunk_id, score });
        }

        Ok(matches)
    }

    async fn delete_document(
        &self,
        ctx: &TenantContext,
        document_id: DocumentId,
    ) -> Result<(), CoreError> {
        // tenant_id is part of the delete filter itself, not applied after
        // fetching candidate documents.
        let filter = doc! {
            "tenant_id": ctx.tenant_id.to_string(),
            "document_id": document_id.as_uuid().to_string(),
        };

        self.collection
            .delete_many(filter)
            .await
            .map_err(|e| CoreError::Other(anyhow::anyhow!("mongo delete_many failed: {e}")))?;

        Ok(())
    }
}
