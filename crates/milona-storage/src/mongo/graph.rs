//! `GraphStore` as an adjacency-list model over MongoDB, using
//! `$graphLookup` for traversal.
//!
//! ROADMAP.md Key Risk #1: `$graphLookup` is not available on DocumentDB.
//! [`MongoGraphStore::traverse`] checks [`MongoBackend::supports_graph_traversal`]
//! before issuing the aggregation and returns `CoreError::Unsupported` if
//! it's `false`, so a DocumentDB deployment fails fast and loudly instead of
//! silently returning an empty/wrong traversal result.
//!
//! Every edge document carries `tenant_id`, and the `$graphLookup` stage's
//! `restrictSearchWithMatch` includes `tenant_id` — per ROADMAP.md Phase
//! 0.5 ("every edge document and every `$graphLookup` stage carries
//! `tenant_id`") — so a traversal literally cannot walk into another
//! tenant's edges, not just filter them out afterward.

use crate::backend::MongoBackend;
use async_trait::async_trait;
use futures_util::TryStreamExt;
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{GraphEdge, GraphStore};
use mongodb::bson::doc;
use mongodb::{Collection, Database};
use serde::{Deserialize, Serialize};

/// On-disk shape of a single directed adjacency-list edge document.
#[derive(Debug, Serialize, Deserialize)]
pub struct EdgeDocument {
    tenant_id: String,
    from: String,
    to: String,
    relation: String,
}

/// `GraphStore` implementation storing one document per directed edge
/// (adjacency-list model) in a single MongoDB collection.
pub struct MongoGraphStore {
    collection: Collection<EdgeDocument>,
    // `$graphLookup` needs a seed document to start `startWith` from; the
    // idiomatic way to seed one without an extra round-trip is the
    // `$documents` stage, which the server only accepts on a *database*-
    // level aggregate ({aggregate: 1}), not a collection-level one — hence
    // this handle is kept alongside `collection` rather than deriving it
    // per call.
    database: Database,
    backend: MongoBackend,
}

impl MongoGraphStore {
    pub fn new(collection: Collection<EdgeDocument>, backend: MongoBackend) -> Self {
        let database = collection.client().database(&collection.namespace().db);
        Self {
            collection,
            database,
            backend,
        }
    }

    pub fn backend(&self) -> MongoBackend {
        self.backend
    }
}

#[async_trait]
impl GraphStore for MongoGraphStore {
    async fn add_edge(&self, ctx: &TenantContext, edge: GraphEdge) -> Result<(), CoreError> {
        let doc = EdgeDocument {
            tenant_id: ctx.tenant_id.to_string(),
            from: edge.from,
            to: edge.to,
            relation: edge.relation,
        };

        self.collection
            .insert_one(doc)
            .await
            .map_err(|e| CoreError::Other(anyhow::anyhow!("mongo insert_one failed: {e}")))?;

        Ok(())
    }

    async fn traverse(
        &self,
        ctx: &TenantContext,
        start_node: &str,
        max_depth: usize,
    ) -> Result<Vec<GraphEdge>, CoreError> {
        if !self.backend.supports_graph_traversal() {
            // Fail fast per ROADMAP.md Key Risk #1 rather than silently
            // returning an empty/incomplete result on DocumentDB, which
            // does not implement $graphLookup.
            return Err(CoreError::Unsupported(
                "$graphLookup traversal is not supported on the configured MongoBackend \
                 (DocumentDB does not implement $graphLookup)"
                    .to_string(),
            ));
        }

        // Seed the graph lookup from a synthetic single-document match so
        // $graphLookup can walk `from` -> `to` starting at `start_node`,
        // restricted to this tenant's edges at every hop via
        // `restrictSearchWithMatch`.
        let pipeline = vec![
            doc! {
                "$documents": [ { "_seed": start_node } ]
            },
            doc! {
                "$graphLookup": {
                    "from": self.collection.name(),
                    "startWith": "$_seed",
                    "connectFromField": "to",
                    "connectToField": "from",
                    "as": "path",
                    "maxDepth": (max_depth.saturating_sub(1)) as i64,
                    "restrictSearchWithMatch": {
                        "tenant_id": ctx.tenant_id.to_string(),
                    },
                }
            },
            doc! {
                "$unwind": "$path"
            },
            doc! {
                "$replaceRoot": { "newRoot": "$path" }
            },
        ];

        // Run at the database level (not `self.collection.aggregate`)
        // because the leading `$documents` stage is only accepted on a
        // database-level `{aggregate: 1}` command — the server rejects it
        // on a collection-scoped aggregate (verified against a live Mongo
        // 7 replica set; see `tests/mongo_integration.rs`).
        let mut cursor = self
            .database
            .aggregate(pipeline)
            .await
            .map_err(|e| CoreError::Other(anyhow::anyhow!("mongo $graphLookup failed: {e}")))?;

        let mut edges = Vec::new();
        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| CoreError::Other(anyhow::anyhow!("mongo cursor error: {e}")))?
        {
            let from = doc
                .get_str("from")
                .map_err(|e| CoreError::Other(anyhow::anyhow!("missing `from`: {e}")))?
                .to_string();
            let to = doc
                .get_str("to")
                .map_err(|e| CoreError::Other(anyhow::anyhow!("missing `to`: {e}")))?
                .to_string();
            let relation = doc
                .get_str("relation")
                .map_err(|e| CoreError::Other(anyhow::anyhow!("missing `relation`: {e}")))?
                .to_string();
            edges.push(GraphEdge { from, to, relation });
        }

        Ok(edges)
    }

    fn supports_traversal(&self) -> bool {
        self.backend.supports_graph_traversal()
    }
}
