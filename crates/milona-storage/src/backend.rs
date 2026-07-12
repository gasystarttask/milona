//! Backend capability flag for the Mongo-backed stores.
//!
//! ROADMAP.md Key Risk #1: DocumentDB does not support `$graphLookup`. The
//! `GraphStore` trait must fail fast (return `CoreError::Unsupported` from
//! `traverse()`, and report `false` from `supports_traversal()`) rather than
//! silently degrading when running against DocumentDB. Every Mongo-backed
//! store in this crate is constructed with an explicit [`MongoBackend`] so
//! the capability check doesn't depend on sniffing the server at runtime.

/// Which MongoDB-compatible deployment a store instance is talking to.
///
/// This is a construction-time flag, not detected automatically: the caller
/// (deployment config) is expected to know which backend it targets, per
/// ROADMAP.md Phase 2 ("Gate this behind a capability check so a DocumentDB
/// deployment fails fast").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MongoBackend {
    /// Real MongoDB or MongoDB Atlas. Supports `$vectorSearch` and
    /// `$graphLookup`.
    Atlas,
    /// Amazon DocumentDB. Supports neither Atlas Search's `$vectorSearch`
    /// nor `$graphLookup`'s recursive semantics in the way this crate's
    /// `GraphStore` traversal needs — graph traversal is unsupported in
    /// this mode.
    DocumentDb,
}

impl MongoBackend {
    /// Whether this backend can execute `$graphLookup`-based recursive
    /// graph traversal. `false` for DocumentDB (ROADMAP.md Key Risk #1).
    pub fn supports_graph_traversal(&self) -> bool {
        matches!(self, MongoBackend::Atlas)
    }

    /// Whether this backend supports the Atlas Search `$vectorSearch`
    /// aggregation stage. `false` for DocumentDB, which has its own
    /// (different) vector search API not implemented by this crate yet.
    pub fn supports_vector_search(&self) -> bool {
        matches!(self, MongoBackend::Atlas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_supports_graph_traversal_and_vector_search() {
        assert!(MongoBackend::Atlas.supports_graph_traversal());
        assert!(MongoBackend::Atlas.supports_vector_search());
    }

    #[test]
    fn document_db_supports_neither() {
        assert!(!MongoBackend::DocumentDb.supports_graph_traversal());
        assert!(!MongoBackend::DocumentDb.supports_vector_search());
    }
}
