//! HTTP route handlers. Every handler here (except [`healthz`]) is only
//! ever reached after [`crate::auth::require_api_key`] and
//! [`crate::rate_limit::rate_limit`] middleware have run, per ROADMAP.md
//! Phase 0.5 — handlers themselves never re-derive a `TenantContext` from
//! raw request input, only from the validated extension inserted by auth.

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use milona_core::tenant::TenantContext;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Instant;

use crate::state::AppState;

/// Unauthenticated liveness health check — the sole exception to "no route
/// ships unauthenticated" per ROADMAP.md Phase 0.5.
pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    /// The caller's question. `tenant_id` is deliberately NOT accepted here
    /// as a body field for the HTTP path — it comes solely from the
    /// authenticated `TenantContext` (derived from the API key), so a
    /// caller cannot spoof another tenant by editing the request body.
    pub question: String,
}

#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub answer: String,
    pub retrieved_count: usize,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
    reason: String,
}

/// `POST /v1/query` — question/response endpoint calling directly into the
/// Phase 3 GenAI application loop via `AppState::answer_question`.
pub async fn query(
    State(state): State<AppState>,
    Extension(ctx): Extension<TenantContext>,
    Json(payload): Json<QueryRequest>,
) -> Response {
    let start = Instant::now();
    let span = tracing::info_span!(
        "http_request",
        route = "/v1/query",
        tenant_id = %ctx.tenant_id,
        subject = %ctx.subject,
    );
    let _enter = span.enter();

    if payload.question.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid_input".to_string(),
                reason: "question must not be empty".to_string(),
            }),
        )
            .into_response();
    }

    let result = state.answer_question(&ctx, &payload.question).await;
    let latency_ms = start.elapsed().as_millis();

    match result {
        Ok(response) => {
            tracing::info!(
                latency_ms,
                retrieved = response.retrieved.len(),
                "query handled"
            );
            (
                StatusCode::OK,
                Json(QueryResponse {
                    answer: response.answer,
                    retrieved_count: response.retrieved.len(),
                    input_tokens: response.input_tokens,
                    output_tokens: response.output_tokens,
                }),
            )
                .into_response()
        }
        Err(err) => {
            tracing::warn!(latency_ms, error = %err, "query failed");
            let status = match &err {
                milona_core::error::CoreError::Unauthorized(_) => StatusCode::FORBIDDEN,
                milona_core::error::CoreError::InvalidInput(_) => StatusCode::BAD_REQUEST,
                milona_core::error::CoreError::NotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status,
                Json(ErrorBody {
                    error: "query_failed".to_string(),
                    reason: err.to_string(),
                }),
            )
                .into_response()
        }
    }
}
