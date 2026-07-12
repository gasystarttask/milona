//! Router assembly: wires the health check (unauthenticated), the
//! authenticated/rate-limited API surface, and structured request tracing.

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::auth::require_api_key;
use crate::handlers::{healthz, query};
use crate::rate_limit::rate_limit;
use crate::state::AppState;

/// Build the full axum `Router`. `/healthz` is mounted outside the
/// authenticated sub-router so it is reachable with no API key at all, per
/// ROADMAP.md Phase 0.5 "No unauthenticated route except a liveness health
/// check". Every other route is nested under middleware that runs, in
/// order: (1) API-key auth, constructing a `TenantContext`; (2) per-tenant
/// rate limiting keyed off that context. Middleware added via
/// `axum::middleware::from_fn_with_state` runs in the order layers are
/// applied to the router, and since `Router::layer` wraps outside-in for
/// requests already added, we apply rate-limit's `.layer` first so auth
/// executes before it.
pub fn build_router(state: AppState) -> Router {
    let authenticated_routes = Router::new()
        .route("/v1/query", post(query))
        .layer(middleware::from_fn_with_state(state.clone(), rate_limit))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));

    Router::new()
        .route("/healthz", get(healthz))
        .merge(authenticated_routes)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
