//! Integration tests for the axum router: auth gating, a valid-key happy
//! path, and rate limiting — all via `tower::ServiceExt::oneshot`, no real
//! network binding needed.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use milona_core::tenant::{Role, TenantId};
use milona_presenter::app::build_router;
use milona_presenter::rate_limit::RateLimiterConfig;
use milona_presenter::state::{ApiKeyRecord, AppState};
use std::collections::HashMap;
use tower::ServiceExt;

fn api_keys_with_one_valid_key(key: &str) -> (HashMap<String, ApiKeyRecord>, TenantId) {
    let tenant_id = TenantId::new(uuid::Uuid::new_v4());
    let mut keys = HashMap::new();
    keys.insert(
        key.to_string(),
        ApiKeyRecord {
            tenant_id,
            role: Role::Member,
            subject: "test-user".to_string(),
        },
    );
    (keys, tenant_id)
}

#[tokio::test]
async fn healthz_is_reachable_without_authentication() {
    let (keys, _tenant) = api_keys_with_one_valid_key("valid-key");
    let state = AppState::new_default(keys);
    let router = build_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn query_without_api_key_is_rejected_with_401() {
    let (keys, _tenant) = api_keys_with_one_valid_key("valid-key");
    let state = AppState::new_default(keys);
    let router = build_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/query")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"question":"hello?"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn query_with_wrong_api_key_is_rejected_with_401() {
    let (keys, _tenant) = api_keys_with_one_valid_key("valid-key");
    let state = AppState::new_default(keys);
    let router = build_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/query")
                .header("content-type", "application/json")
                .header("x-api-key", "totally-wrong-key")
                .body(Body::from(r#"{"question":"hello?"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn query_with_valid_api_key_succeeds() {
    let (keys, _tenant) = api_keys_with_one_valid_key("valid-key");
    let state = AppState::new_default(keys);
    let router = build_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/query")
                .header("content-type", "application/json")
                .header("x-api-key", "valid-key")
                .body(Body::from(r#"{"question":"What is Milona?"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(!json["answer"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn query_with_empty_question_is_rejected_with_400() {
    let (keys, _tenant) = api_keys_with_one_valid_key("valid-key");
    let state = AppState::new_default(keys);
    let router = build_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/query")
                .header("content-type", "application/json")
                .header("x-api-key", "valid-key")
                .body(Body::from(r#"{"question":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn requests_over_rate_limit_are_rejected_with_429() {
    let (keys, _tenant) = api_keys_with_one_valid_key("valid-key");
    // Quota of 1 request per minute so the second immediate request from the
    // same authenticated subject is rejected.
    let state = AppState::new_with_rate_limit(keys, RateLimiterConfig::new(1));
    let router = build_router(state);

    let make_request = || {
        Request::builder()
            .method("POST")
            .uri("/v1/query")
            .header("content-type", "application/json")
            .header("x-api-key", "valid-key")
            .body(Body::from(r#"{"question":"hello?"}"#))
            .unwrap()
    };

    let first = router.clone().oneshot(make_request()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = router.clone().oneshot(make_request()).await.unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn different_tenants_have_independent_rate_limit_quotas() {
    let tenant_a = TenantId::new(uuid::Uuid::new_v4());
    let tenant_b = TenantId::new(uuid::Uuid::new_v4());
    let mut keys = HashMap::new();
    keys.insert(
        "key-a".to_string(),
        ApiKeyRecord {
            tenant_id: tenant_a,
            role: Role::Member,
            subject: "user-a".to_string(),
        },
    );
    keys.insert(
        "key-b".to_string(),
        ApiKeyRecord {
            tenant_id: tenant_b,
            role: Role::Member,
            subject: "user-b".to_string(),
        },
    );
    let state = AppState::new_with_rate_limit(keys, RateLimiterConfig::new(1));
    let router = build_router(state);

    let make_request = |key: &'static str| {
        Request::builder()
            .method("POST")
            .uri("/v1/query")
            .header("content-type", "application/json")
            .header("x-api-key", key)
            .body(Body::from(r#"{"question":"hello?"}"#))
            .unwrap()
    };

    let a_first = router.clone().oneshot(make_request("key-a")).await.unwrap();
    assert_eq!(a_first.status(), StatusCode::OK);

    // Tenant B's first request succeeds even though tenant A just used its
    // quota, proving isolation between per-tenant rate limits.
    let b_first = router.clone().oneshot(make_request("key-b")).await.unwrap();
    assert_eq!(b_first.status(), StatusCode::OK);

    let a_second = router.clone().oneshot(make_request("key-a")).await.unwrap();
    assert_eq!(a_second.status(), StatusCode::TOO_MANY_REQUESTS);
}
