//! Integration tests against a real MongoDB replica set.
//!
//! These require a live Mongo instance and are `#[ignore]`d by default so
//! `cargo test -p milona-storage` (the required non-Docker unit-test run)
//! never depends on Docker. To run them:
//!
//!   1. `docker compose up -d mongodb` from the repo root (starts the
//!      single-node replica set defined in `docker-compose.yml`; the
//!      `mongodb-init` helper service / healthcheck bring it to PRIMARY
//!      within a few seconds).
//!   2. `MONGODB_URI='mongodb://localhost:27017/?directConnection=true&replicaSet=rs0' \
//!      cargo test -p milona-storage --test mongo_integration -- --ignored --test-threads=1`
//!
//! `$vectorSearch` additionally requires an Atlas Search vector index on
//! the collection under test, which a plain community-edition `mongod`/
//! replica-set container (as started by `docker-compose.yml`) does **not**
//! provide (Atlas Search is an Atlas-hosted feature, not shipped in
//! self-hosted `mongo:7`). The vector-search test is therefore further
//! gated and will report a clear error rather than a silent pass/fail if
//! run against a non-Atlas deployment — see the comment on
//! `vector_search_is_tenant_scoped_against_atlas` below. The graph
//! ($graphLookup + $documents) tests work against plain self-hosted Mongo
//! 7+ and are expected to pass against the compose stack.

use futures_util::TryStreamExt;
use milona_core::document::{Chunk, ChunkId, DocumentId};
use milona_core::tenant::{Role, TenantContext, TenantId};
use milona_core::traits::{GraphEdge, GraphStore, VectorStore};
use milona_storage::backend::MongoBackend;
use milona_storage::mongo::{MongoGraphStore, MongoVectorStore};
use mongodb::Client;
use uuid::Uuid;

async fn test_client() -> Client {
    let uri = std::env::var("MONGODB_URI").unwrap_or_else(|_| {
        "mongodb://localhost:27017/?directConnection=true&replicaSet=rs0".to_string()
    });
    Client::with_uri_str(&uri)
        .await
        .expect("failed to connect to MongoDB — is `docker compose up -d mongodb` running?")
}

#[tokio::test]
#[ignore = "requires a live MongoDB replica set; see module docs for docker compose instructions"]
async fn graph_traversal_is_tenant_scoped_against_real_mongo() {
    let client = test_client().await;
    let db = client.database("milona_storage_it");
    let collection_name = format!("edges_{}", Uuid::new_v4().simple());
    let collection = db.collection(&collection_name);

    let store = MongoGraphStore::new(collection, MongoBackend::Atlas);

    let tenant_a = TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "a");
    let tenant_b = TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "b");

    store
        .add_edge(
            &tenant_a,
            GraphEdge {
                from: "n1".into(),
                to: "n2".into(),
                relation: "rel".into(),
            },
        )
        .await
        .unwrap();

    let result_b = store.traverse(&tenant_b, "n1", 3).await.unwrap();
    assert!(
        result_b.is_empty(),
        "tenant B must never see tenant A's edges"
    );

    let result_a = store.traverse(&tenant_a, "n1", 3).await.unwrap();
    assert_eq!(result_a.len(), 1);
    assert_eq!(result_a[0].to, "n2");

    // Cleanup.
    db.collection::<mongodb::bson::Document>(&collection_name)
        .drop()
        .await
        .ok();
}

#[tokio::test]
#[ignore = "requires a live MongoDB replica set; see module docs for docker compose instructions"]
async fn document_db_backend_rejects_graph_traversal_even_against_real_mongo() {
    // Even talking to a real, fully-$graphLookup-capable MongoDB, a store
    // constructed with MongoBackend::DocumentDb must refuse to traverse —
    // the capability flag is a deployment-time promise, not a runtime probe.
    let client = test_client().await;
    let db = client.database("milona_storage_it");
    let collection_name = format!("edges_{}", Uuid::new_v4().simple());
    let collection = db.collection(&collection_name);

    let store = MongoGraphStore::new(collection, MongoBackend::DocumentDb);
    let ctx = TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "a");

    let err = store.traverse(&ctx, "n1", 3).await.unwrap_err();
    assert!(matches!(err, milona_core::error::CoreError::Unsupported(_)));
    assert!(!store.supports_traversal());
}

#[tokio::test]
#[ignore = "requires a live MongoDB Atlas cluster with a $vectorSearch index named \
            `milona_vector_index` on the `embedding` field — a self-hosted \
            docker-compose mongod does not provide Atlas Search. See module docs."]
async fn vector_search_is_tenant_scoped_against_atlas() {
    let client = test_client().await;
    let db = client.database("milona_storage_it");
    let collection_name = format!("vectors_{}", Uuid::new_v4().simple());
    let collection = db.collection(&collection_name);

    let store = MongoVectorStore::with_default_index(collection.clone(), MongoBackend::Atlas);

    let tenant_a = TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "a");
    let tenant_b = TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "b");

    let chunk = Chunk {
        id: ChunkId::new(),
        document_id: DocumentId::new(),
        text: "hello".into(),
        sequence: 0,
        token_count: 1,
    };
    store.upsert(&tenant_a, &chunk, &[1.0, 0.0]).await.unwrap();

    // Give Atlas Search's near-real-time indexing a moment to catch up.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let hits_b = store.search(&tenant_b, &[1.0, 0.0], 10).await.unwrap();
    assert!(
        hits_b.is_empty(),
        "tenant B must never see tenant A's vectors"
    );

    let hits_a = store.search(&tenant_a, &[1.0, 0.0], 10).await.unwrap();
    assert_eq!(hits_a.len(), 1);

    // Cleanup.
    let _ = collection.drop().await;
}

#[tokio::test]
#[ignore = "requires a live MongoDB replica set; see module docs for docker compose instructions"]
async fn vector_store_upsert_and_delete_document_round_trip_via_find() {
    // Exercises upsert/delete_document directly against the collection
    // (bypassing $vectorSearch, which needs an Atlas Search index) so the
    // basic CRUD path is validated even without Atlas.
    let client = test_client().await;
    let db = client.database("milona_storage_it");
    let collection_name = format!("vectors_crud_{}", Uuid::new_v4().simple());
    let collection = db.collection(&collection_name);

    let store = MongoVectorStore::with_default_index(collection.clone(), MongoBackend::Atlas);
    let ctx = TenantContext::new(TenantId::new(Uuid::new_v4()), Role::Member, "a");
    let doc_id = DocumentId::new();
    let chunk = Chunk {
        id: ChunkId::new(),
        document_id: doc_id,
        text: "hello".into(),
        sequence: 0,
        token_count: 1,
    };
    store.upsert(&ctx, &chunk, &[1.0, 0.0, 0.0]).await.unwrap();

    let raw: Vec<mongodb::bson::Document> = collection
        .clone_with_type::<mongodb::bson::Document>()
        .find(mongodb::bson::doc! {})
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(raw.len(), 1);

    store.delete_document(&ctx, doc_id).await.unwrap();
    let raw_after: Vec<mongodb::bson::Document> = collection
        .clone_with_type::<mongodb::bson::Document>()
        .find(mongodb::bson::doc! {})
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    assert!(raw_after.is_empty());

    let _ = collection.drop().await;
}
