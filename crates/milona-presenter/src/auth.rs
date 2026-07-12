//! API-key authentication middleware.
//!
//! Concrete implementation of ROADMAP.md Phase 0.5 "No unauthenticated
//! route except a liveness health check": every route except `/healthz` is
//! wrapped by [`require_api_key`], which validates the `x-api-key` header
//! against a configured [`ApiKeyDirectory`], constructs a `TenantContext`
//! from the matched key, and rejects the request with 401 *before* it
//! reaches any handler if the key is missing or unknown.
//!
//! OAuth2/OIDC (per ROADMAP.md) is a documented future upgrade for human
//! users; this is the service-to-service API key path.

use crate::state::{ApiKeyRecord, AppState};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use milona_core::tenant::TenantContext;
use serde_json::json;
use std::collections::HashMap;

pub const API_KEY_HEADER: &str = "x-api-key";

/// Maps API key strings to the tenant identity they authenticate as.
#[derive(Debug, Default)]
pub struct ApiKeyDirectory {
    keys: HashMap<String, ApiKeyRecord>,
}

impl ApiKeyDirectory {
    pub fn new(keys: HashMap<String, ApiKeyRecord>) -> Self {
        Self { keys }
    }

    pub fn validate(&self, key: &str) -> Option<TenantContext> {
        self.keys
            .get(key)
            .map(|rec| TenantContext::new(rec.tenant_id, rec.role, rec.subject.clone()))
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

fn unauthorized(reason: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized", "reason": reason })),
    )
        .into_response()
}

/// Extracts the `x-api-key` header, validates it against `state.api_keys`,
/// and inserts the resulting `TenantContext` into request extensions for
/// downstream handlers to use. Returns 401 immediately on any failure so no
/// route handler runs for an unauthenticated request.
pub async fn require_api_key(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let headers: &HeaderMap = request.headers();
    let key = match headers.get(API_KEY_HEADER).and_then(|v| v.to_str().ok()) {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => return unauthorized("missing x-api-key header"),
    };

    match state.api_keys.validate(&key) {
        Some(ctx) => {
            request.extensions_mut().insert(ctx);
            next.run(request).await
        }
        None => unauthorized("invalid api key"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use milona_core::tenant::{Role, TenantId};

    #[test]
    fn validates_known_key_and_rejects_unknown() {
        let tenant = TenantId::new(uuid::Uuid::new_v4());
        let mut keys = HashMap::new();
        keys.insert(
            "secret-key".to_string(),
            ApiKeyRecord {
                tenant_id: tenant,
                role: Role::Member,
                subject: "user-1".to_string(),
            },
        );
        let directory = ApiKeyDirectory::new(keys);

        let ctx = directory.validate("secret-key").expect("should validate");
        assert_eq!(ctx.tenant_id, tenant);
        assert!(directory.validate("wrong-key").is_none());
    }
}
