//! Retry/backoff wrapper around any `LlmProvider`, per ROADMAP.md Phase 0.5
//! "Cost control & resilience for LLM calls": a provider error must not
//! propagate immediately.
//!
//! A hand-rolled exponential-backoff loop, as explicitly permitted by the
//! task brief, rather than pulling in `tower::retry` — keeps this crate's
//! dependency footprint minimal while providing the same guarantee.

use async_trait::async_trait;
use milona_core::error::CoreError;
use milona_core::traits::{LlmMessage, LlmProvider, LlmResponse};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    /// Multiplier applied to the backoff after each failed attempt.
    pub backoff_multiplier: f64,
    pub max_backoff: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(50),
            backoff_multiplier: 2.0,
            max_backoff: Duration::from_secs(5),
        }
    }
}

impl RetryConfig {
    /// A config with zero sleep, useful for fast unit tests that still want
    /// to exercise the retry-count logic.
    pub fn fast_for_tests(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            initial_backoff: Duration::from_millis(0),
            backoff_multiplier: 1.0,
            max_backoff: Duration::from_millis(0),
        }
    }
}

/// Wraps an inner `LlmProvider` with exponential-backoff retry. A provider
/// error (`CoreError::Upstream` and friends) does not propagate on the first
/// failure — the call is retried up to `config.max_attempts` times with
/// increasing backoff before the last error is surfaced.
pub struct RetryingLlmProvider<L: LlmProvider> {
    inner: L,
    config: RetryConfig,
}

impl<L: LlmProvider> RetryingLlmProvider<L> {
    pub fn new(inner: L, config: RetryConfig) -> Self {
        Self { inner, config }
    }
}

#[async_trait]
impl<L: LlmProvider> LlmProvider for RetryingLlmProvider<L> {
    async fn complete(&self, messages: &[LlmMessage]) -> Result<LlmResponse, CoreError> {
        let mut backoff = self.config.initial_backoff;
        let mut last_err = None;

        for attempt in 1..=self.config.max_attempts {
            match self.inner.complete(messages).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    tracing::warn!(
                        attempt,
                        max_attempts = self.config.max_attempts,
                        error = %err,
                        "LlmProvider call failed, will retry with backoff"
                    );
                    last_err = Some(err);
                    if attempt < self.config.max_attempts {
                        if !backoff.is_zero() {
                            tokio::time::sleep(backoff).await;
                        }
                        let next_millis =
                            (backoff.as_secs_f64() * self.config.backoff_multiplier).max(0.0);
                        backoff = Duration::from_secs_f64(next_millis).min(self.config.max_backoff);
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            CoreError::Upstream("retry loop exited with no recorded error".to_string())
        }))
    }

    fn provider_name(&self) -> &str {
        self.inner.provider_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{MockBehavior, MockLlmProvider};
    use milona_core::traits::MessageRole;

    #[tokio::test]
    async fn retries_on_transient_failure_and_eventually_succeeds() {
        let mock = MockLlmProvider::new("recovered")
            .with_behavior(MockBehavior::FailNTimesThenSucceed { failures: 2 });
        let provider = RetryingLlmProvider::new(mock, RetryConfig::fast_for_tests(5));

        let response = provider
            .complete(&[LlmMessage {
                role: MessageRole::User,
                content: "hi".into(),
            }])
            .await
            .unwrap();

        assert_eq!(response.content, "recovered");
    }

    #[tokio::test]
    async fn gives_up_after_max_attempts_and_surfaces_the_error() {
        let mock = MockLlmProvider::new("unused").with_behavior(MockBehavior::AlwaysFail);
        let provider = RetryingLlmProvider::new(mock, RetryConfig::fast_for_tests(3));

        let err = provider
            .complete(&[LlmMessage {
                role: MessageRole::User,
                content: "hi".into(),
            }])
            .await
            .unwrap_err();

        assert!(matches!(err, CoreError::Upstream(_)));
    }

    #[tokio::test]
    async fn does_not_retry_beyond_configured_attempts() {
        let mock = MockLlmProvider::new("unused")
            .with_behavior(MockBehavior::FailNTimesThenSucceed { failures: 10 });
        let provider = RetryingLlmProvider::new(mock, RetryConfig::fast_for_tests(3));

        let err = provider
            .complete(&[LlmMessage {
                role: MessageRole::User,
                content: "hi".into(),
            }])
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Upstream(_)));
    }
}
