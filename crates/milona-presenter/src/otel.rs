//! OpenTelemetry tracing wiring point — Phase 6 hardening (ROADMAP.md).
//!
//! ROADMAP.md flags `tracing-opentelemetry` as still-Beta upstream (Key Risk #4) and
//! recommends adding OTel export "once the team is ready to absorb Beta-API churn" rather
//! than forcing it now. This module is therefore a **seam, not a full integration**: it is
//! entirely gated behind the `otel` Cargo feature (off by default, so the default build has
//! zero additional dependencies and zero behavior change) and gives a single, obvious place
//! to plug in a real `OtlpExporter`/collector when the team decides to take on that churn.
//!
//! ## Current state (this seam)
//! - Feature-flagged (`--features otel`) so it compiles nothing into default builds.
//! - [`init_tracing`] is a drop-in alternative to the plain `tracing_subscriber::fmt()`
//!   setup in `main.rs`: with the `otel` feature disabled it behaves identically (just
//!   `fmt` + `EnvFilter`); with it enabled, it additionally layers an OTel exporter.
//! - No collector endpoint is wired up yet — see the `TODO(otel)` markers below for exactly
//!   what real integration requires.
//!
//! ## TODO(otel): turning this into a real integration
//! 1. Add `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`, and
//!    `tracing-opentelemetry` to `[dependencies]` (only under the `otel` feature) once a
//!    target OTel version is chosen and pinned — expect breaking changes between minor
//!    versions while the Rust OTel tracing API is Beta.
//! 2. Build a real `opentelemetry_sdk::trace::TracerProvider` pointed at an OTLP endpoint
//!    (read from `OTEL_EXPORTER_OTLP_ENDPOINT`, following the OTel env-var convention so no
//!    Milona-specific config surface is needed) instead of the no-op tracer below.
//! 3. Register a shutdown hook (`TracerProvider::shutdown`) on process exit so buffered
//!    spans are flushed — easy to forget and silently lose the tail of every trace.
//! 4. Wire per-tenant `tenant_id` as a span attribute/OTel resource attribute (it's
//!    already present in `tracing` spans per `ROADMAP.md` Phase 0.5) so traces can be
//!    filtered per tenant in the collector UI, matching the existing `tracing` discipline.
//! 5. Add a `docker-compose.yml` profile (see repo root) for a local collector
//!    (`otel/opentelemetry-collector` image) once this graduates past the seam stage, so
//!    `docker compose --profile otel up` gives a full local trace pipeline.

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

/// Initialize global tracing for the presenter binary.
///
/// Without the `otel` feature: equivalent to `tracing_subscriber::fmt().with_env_filter(...)`
/// (this is intentionally the *only* behavior difference-free path — enabling this module
/// must never change default-build output).
///
/// With the `otel` feature: additionally layers an OTel tracing bridge. Currently a no-op
/// placeholder (see `otel_layer` below) — flip on real export by following the TODOs above.
pub fn init_tracing() {
    let env_filter = EnvFilter::from_default_env();
    let fmt_layer = tracing_subscriber::fmt::layer();

    let subscriber = Registry::default().with(env_filter).with(fmt_layer);

    #[cfg(feature = "otel")]
    {
        subscriber.with(otel_layer()).init();
    }

    #[cfg(not(feature = "otel"))]
    {
        subscriber.init();
    }
}

/// Placeholder OTel layer. Returns a layer that does nothing (`tracing_subscriber::layer::Identity`)
/// so `--features otel` builds today without requiring a live collector or any new
/// dependency — this is the explicit "seam" the ROADMAP.md Phase 6 note asks for, not a
/// working exporter. Replace the body per the `TODO(otel)` list on this module once the
/// team is ready to add `opentelemetry`/`tracing-opentelemetry` as real dependencies.
#[cfg(feature = "otel")]
fn otel_layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber,
{
    // TODO(otel): replace `Identity` with `tracing_opentelemetry::layer().with_tracer(tracer)`
    // once opentelemetry-otlp is wired up per the module-level TODO list.
    tracing_subscriber::layer::Identity::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards against `init_tracing` accidentally requiring a collector / panicking with
    /// the `otel` feature off — the common case, and the one every default `cargo test`
    /// run in CI exercises.
    #[test]
    fn init_tracing_without_otel_feature_does_not_panic() {
        // `tracing_subscriber` only allows one global default per process; run this in a
        // thread-local subscriber scope instead of calling the process-global `init()` so
        // the test is hermetic and repeatable regardless of test execution order.
        let env_filter = EnvFilter::new("info");
        let subscriber = Registry::default()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer());
        let _guard = tracing::subscriber::set_default(subscriber);

        tracing::info!("otel seam smoke test");
    }
}
