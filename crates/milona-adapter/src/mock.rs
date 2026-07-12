//! `MockLlmProvider` — a clearly-labeled stand-in for a real `genai`-backed
//! provider (see the crate-level doc comment for why `genai` isn't wired in
//! this sandbox). Returns deterministic, canned responses so callers and
//! tests never depend on network access or an API key.

use async_trait::async_trait;
use milona_core::error::CoreError;
use milona_core::traits::{LlmMessage, LlmProvider, LlmResponse};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// Deterministic behavior a [`MockLlmProvider`] can be configured to exhibit,
/// so tests can simulate transient upstream failures without any real
/// network call.
#[derive(Debug, Clone)]
pub enum MockBehavior {
    /// Always succeed with a canned response.
    AlwaysSucceed,
    /// Fail the first `failures` calls with `CoreError::Upstream`, then
    /// succeed — simulates a transient provider outage that retry/backoff
    /// (see [`crate::retry`]) is expected to ride out.
    FailNTimesThenSucceed { failures: u32 },
    /// Always fail with `CoreError::Upstream` — simulates a hard outage.
    AlwaysFail,
}

/// A deterministic, canned-response `LlmProvider`. Explicitly NOT a real
/// model call — see the crate-level doc comment for the `genai` substitution
/// rationale.
pub struct MockLlmProvider {
    behavior: MockBehavior,
    call_count: AtomicU32,
    canned_response: Mutex<String>,
}

impl MockLlmProvider {
    pub fn new(canned_response: impl Into<String>) -> Self {
        Self {
            behavior: MockBehavior::AlwaysSucceed,
            call_count: AtomicU32::new(0),
            canned_response: Mutex::new(canned_response.into()),
        }
    }

    pub fn with_behavior(mut self, behavior: MockBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    pub fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl Default for MockLlmProvider {
    fn default() -> Self {
        Self::new("This is a canned response from MockLlmProvider (no real LLM was called).")
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn complete(&self, messages: &[LlmMessage]) -> Result<LlmResponse, CoreError> {
        let call_number = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;

        let should_fail = match self.behavior {
            MockBehavior::AlwaysSucceed => false,
            MockBehavior::AlwaysFail => true,
            MockBehavior::FailNTimesThenSucceed { failures } => call_number <= failures,
        };

        if should_fail {
            return Err(CoreError::Upstream(format!(
                "mock provider simulated transient failure (call #{call_number})"
            )));
        }

        let input_tokens: u32 = messages
            .iter()
            .map(|m| (m.content.len() / 4).max(1) as u32)
            .sum();

        Ok(LlmResponse {
            content: self.canned_response.lock().unwrap().clone(),
            input_tokens,
            output_tokens: 8,
        })
    }

    fn provider_name(&self) -> &str {
        "mock (genai substitution — see crate-level doc comment)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use milona_core::traits::MessageRole;

    #[tokio::test]
    async fn always_succeed_returns_canned_response() {
        let provider = MockLlmProvider::new("hello");
        let response = provider
            .complete(&[LlmMessage {
                role: MessageRole::User,
                content: "hi".into(),
            }])
            .await
            .unwrap();
        assert_eq!(response.content, "hello");
    }

    #[tokio::test]
    async fn fail_n_times_then_succeed_behaves_deterministically() {
        let provider = MockLlmProvider::new("ok")
            .with_behavior(MockBehavior::FailNTimesThenSucceed { failures: 2 });

        let msgs = [LlmMessage {
            role: MessageRole::User,
            content: "hi".into(),
        }];

        assert!(provider.complete(&msgs).await.is_err());
        assert!(provider.complete(&msgs).await.is_err());
        assert!(provider.complete(&msgs).await.is_ok());
        assert_eq!(provider.call_count(), 3);
    }
}
