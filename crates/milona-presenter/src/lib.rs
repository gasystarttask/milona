//! Phase 4 — Presenter (API / CLI).
//!
//! Exposes the Phase 3 GenAI application loop (`milona-knowledge`,
//! `milona-adapter`) through an `axum` HTTP API and a `clap` CLI that share
//! the same core handler (`state::AppState::answer_question`) — "same
//! handlers reused by both" per ROADMAP.md Phase 4.
//!
//! ## Enterprise requirements implemented here (ROADMAP.md Phase 0.5)
//! - **AuthN**: [`auth::require_api_key`] middleware validates an API key
//!   and constructs a `TenantContext`; no route except `/healthz` is
//!   reachable without one.
//! - **Rate limiting**: [`rate_limit::rate_limit`] middleware caps requests
//!   per authenticated tenant/subject using the `governor` crate.
//! - **Tracing**: every request handler runs inside a `tracing` span
//!   carrying `tenant_id`, `route`, and logs latency.

pub mod app;
pub mod auth;
pub mod cli;
pub mod handlers;
pub mod otel;
pub mod rate_limit;
pub mod state;
