//! Phase 2 — Storage layer. Implements `milona_core::traits::{VectorStore,
//! GraphStore}` over MongoDB.
//!
//! Two families of implementation live here:
//!
//! - [`mongo::MongoVectorStore`] / [`mongo::MongoGraphStore`]: the real
//!   MongoDB-backed stores (`$vectorSearch` for vectors, an adjacency-list
//!   `$graphLookup` for graph traversal), gated by a [`backend::MongoBackend`]
//!   capability flag so a DocumentDB deployment fails fast on graph
//!   traversal instead of silently degrading (ROADMAP.md Key Risk #1).
//! - [`memory::InMemoryVectorStore`] / [`memory::InMemoryGraphStore`]:
//!   `HashMap`-backed fakes implementing the same traits (and the same
//!   tenant-scoping discipline) for unit tests that must not require a live
//!   Mongo instance.
//!
//! Every method on every store here takes a `&TenantContext` and applies
//! `tenant_id` inside the query/aggregation stage itself, never as a
//! post-filter, per ROADMAP.md Phase 0.5.

pub mod backend;
pub mod memory;
pub mod mongo;

pub use backend::MongoBackend;
pub use memory::{InMemoryGraphStore, InMemoryVectorStore};
pub use mongo::{MongoGraphStore, MongoVectorStore};
