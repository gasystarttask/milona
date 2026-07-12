//! MongoDB-backed `VectorStore`/`GraphStore` implementations (Phase 2).

pub mod graph;
pub mod vector;

pub use graph::{EdgeDocument, MongoGraphStore};
pub use vector::{MongoVectorStore, VectorDocument, DEFAULT_VECTOR_INDEX_NAME};

use milona_core::error::CoreError;

/// `ChunkId`/`DocumentId` (milona-core) are UUID newtypes whose inner field
/// is private to that crate, so the only public way to reconstruct one from
/// a UUID string round-tripped out of Mongo is through their (derived,
/// serde-transparent) `Deserialize` impl — a tuple struct with one field
/// serializes/deserializes as that field directly. This crate deliberately
/// does not modify milona-core to add a `from_uuid` constructor; this
/// helper documents why a serde round-trip is used instead.
pub(crate) fn deserialize_uuid_newtype<T: serde::de::DeserializeOwned>(
    uuid_str: &str,
) -> Result<T, CoreError> {
    serde_json::from_value(serde_json::Value::String(uuid_str.to_string()))
        .map_err(|e| CoreError::Other(anyhow::anyhow!("invalid id `{uuid_str}`: {e}")))
}

/// The other direction of [`deserialize_uuid_newtype`]: `ChunkId` exposes no
/// public accessor for its inner UUID (unlike `DocumentId::as_uuid`), so its
/// (derived, serde-transparent) `Serialize` impl is the only public way to
/// get the UUID string back out.
pub(crate) fn serialize_uuid_newtype<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => unreachable!(
            "milona-core UUID newtypes are expected to serialize as a plain string, got {other:?}"
        ),
    }
}
