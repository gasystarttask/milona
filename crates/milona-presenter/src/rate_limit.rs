//! Per-API-key/tenant rate limiting middleware, per ROADMAP.md Phase 0.5
//! "Cost control & resilience" — a single runaway API key/tenant must not be
//! able to exhaust shared capacity.
//!
//! Built on the `governor` crate's generic-cell-rate-algorithm limiter, keyed
//! per API key so each tenant gets an independent quota rather than sharing
//! one global bucket. Runs *after* the auth middleware (it needs the
//! validated `TenantContext`/key to key the limiter), so an unauthenticated
//! request is rejected by auth first and never consumes rate-limit quota.

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use milona_core::tenant::TenantContext;
use serde_json::json;
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::state::AppState;

/// Config for the per-key rate limiter: `requests_per_period` requests
/// allowed per `period`, with a burst of the same size (governor's default
/// "cell" burst == the quota's per-period count).
#[derive(Clone)]
pub struct RateLimiterConfig {
    pub requests_per_minute: u32,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        // Generous default so normal test/dev traffic isn't throttled;
        // override per-deployment via `RateLimiterConfig::new`.
        Self {
            requests_per_minute: 60,
        }
    }
}

impl RateLimiterConfig {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            requests_per_minute,
        }
    }

    fn quota(&self) -> Quota {
        let per_minute = NonZeroU32::new(self.requests_per_minute.max(1)).unwrap();
        Quota::per_minute(per_minute)
    }
}

/// Keyed rate limiter shared across requests, keyed by the authenticated
/// subject (tenant + subject string) so each caller's quota is independent.
pub type KeyedLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

pub fn build_limiter(config: &RateLimiterConfig) -> Arc<KeyedLimiter> {
    Arc::new(RateLimiter::keyed(config.quota()))
}

fn too_many_requests() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(
            json!({ "error": "rate_limited", "reason": "request quota exceeded, try again later" }),
        ),
    )
        .into_response()
}

/// Middleware layer applying the per-tenant/key rate limit. Must run after
/// [`crate::auth::require_api_key`] so `TenantContext` is already present in
/// request extensions. Uses the single shared `Arc<KeyedLimiter>` held in
/// `AppState` (built once at composition-root time) so quota is actually
/// tracked across requests rather than reset per call.
pub async fn rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let ctx = request.extensions().get::<TenantContext>().cloned();
    let Some(ctx) = ctx else {
        // Auth middleware should have already rejected this request; if it
        // somehow reached here without a TenantContext, fail closed.
        return too_many_requests();
    };

    let key = format!("{}:{}", ctx.tenant_id, ctx.subject);

    match state.limiter.check_key(&key) {
        Ok(_) => next.run(request).await,
        Err(_) => too_many_requests(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_allows_then_rejects_over_quota() {
        let config = RateLimiterConfig::new(2);
        let limiter = build_limiter(&config);
        let key = "tenant-a:user-1".to_string();

        assert!(limiter.check_key(&key).is_ok());
        assert!(limiter.check_key(&key).is_ok());
        // Third immediate call exceeds the 2-per-minute quota.
        assert!(limiter.check_key(&key).is_err());
    }

    #[test]
    fn limiter_tracks_keys_independently() {
        let config = RateLimiterConfig::new(1);
        let limiter = build_limiter(&config);

        assert!(limiter.check_key(&"tenant-a:user-1".to_string()).is_ok());
        // Different key has an independent quota.
        assert!(limiter.check_key(&"tenant-b:user-2".to_string()).is_ok());
        // Same key as first is now exhausted.
        assert!(limiter.check_key(&"tenant-a:user-1".to_string()).is_err());
    }
}
