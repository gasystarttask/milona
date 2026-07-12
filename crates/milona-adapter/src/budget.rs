//! Per-tenant token budget enforcement, per ROADMAP.md Phase 0.5 "Cost
//! control & resilience for LLM calls": a single runaway tenant must not be
//! able to exhaust the shared LLM budget.
//!
//! `milona_core::traits::LlmProvider::complete` intentionally has no
//! `TenantContext` parameter (it's a low-level, provider-agnostic
//! completion call). Tenant-scoped budget enforcement therefore lives on a
//! dedicated method, [`BudgetedLlmProvider::complete_for_tenant`], which
//! callers (e.g. the `genai_loop` in `milona-knowledge`) are expected to use
//! instead of calling a bare `LlmProvider::complete` directly whenever a
//! `TenantContext` is available. `BudgetedLlmProvider` also implements the
//! plain `LlmProvider` trait itself (unmetered passthrough) so it remains a
//! drop-in `LlmProvider` where no tenant is in scope (e.g. wrapped further
//! by `RetryingLlmProvider`).

use milona_core::error::CoreError;
use milona_core::tenant::TenantId;
use milona_core::traits::{LlmMessage, LlmProvider, LlmResponse};
use std::collections::HashMap;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BudgetError {
    #[error("tenant {tenant_id} exceeded its token budget: used {used}, limit {limit}")]
    Exceeded {
        tenant_id: TenantId,
        used: u64,
        limit: u64,
    },
    #[error(transparent)]
    Core(#[from] CoreError),
}

impl From<BudgetError> for CoreError {
    fn from(err: BudgetError) -> Self {
        match err {
            BudgetError::Exceeded {
                tenant_id,
                used,
                limit,
            } => CoreError::Unauthorized(format!(
                "tenant {tenant_id} exceeded its token budget (used {used}, limit {limit})"
            )),
            BudgetError::Core(inner) => inner,
        }
    }
}

/// Configured token budget for a tenant, tracked with a simple in-memory
/// counter (per ROADMAP.md Phase 3: "in-memory counter is fine for now").
#[derive(Debug, Clone, Copy)]
pub struct TokenBudget {
    pub max_tokens: u64,
}

impl TokenBudget {
    pub fn new(max_tokens: u64) -> Self {
        Self { max_tokens }
    }
}

/// Wraps an inner `LlmProvider` with per-tenant token-budget accounting.
/// Tracks `input_tokens + output_tokens` from each successful call against a
/// configured per-tenant ceiling; once a tenant's cumulative usage would
/// exceed its budget, further calls are rejected before the inner provider
/// is invoked at all.
pub struct BudgetedLlmProvider<L: LlmProvider> {
    inner: L,
    default_budget: TokenBudget,
    per_tenant_budget: HashMap<TenantId, TokenBudget>,
    usage: Mutex<HashMap<TenantId, u64>>,
}

impl<L: LlmProvider> BudgetedLlmProvider<L> {
    pub fn new(inner: L, default_budget: TokenBudget) -> Self {
        Self {
            inner,
            default_budget,
            per_tenant_budget: HashMap::new(),
            usage: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_tenant_budget(mut self, tenant_id: TenantId, budget: TokenBudget) -> Self {
        self.per_tenant_budget.insert(tenant_id, budget);
        self
    }

    fn budget_for(&self, tenant_id: TenantId) -> TokenBudget {
        self.per_tenant_budget
            .get(&tenant_id)
            .copied()
            .unwrap_or(self.default_budget)
    }

    pub fn usage_for(&self, tenant_id: TenantId) -> u64 {
        *self.usage.lock().unwrap().get(&tenant_id).unwrap_or(&0)
    }

    /// Tenant-scoped completion call: checks the tenant's budget before
    /// invoking the inner provider, and records actual usage from the
    /// response afterward. Returns `BudgetError::Exceeded` (convertible to
    /// `CoreError::Unauthorized`) once the tenant has no budget left,
    /// without ever reaching the inner provider.
    pub async fn complete_for_tenant(
        &self,
        tenant_id: TenantId,
        messages: &[LlmMessage],
    ) -> Result<LlmResponse, BudgetError> {
        let budget = self.budget_for(tenant_id);

        {
            let usage = self.usage.lock().unwrap();
            let used = *usage.get(&tenant_id).unwrap_or(&0);
            if used >= budget.max_tokens {
                return Err(BudgetError::Exceeded {
                    tenant_id,
                    used,
                    limit: budget.max_tokens,
                });
            }
        }

        let response = self.inner.complete(messages).await?;
        let spent = u64::from(response.input_tokens) + u64::from(response.output_tokens);

        let mut usage = self.usage.lock().unwrap();
        let entry = usage.entry(tenant_id).or_insert(0);
        *entry += spent;

        Ok(response)
    }
}

#[async_trait::async_trait]
impl<L: LlmProvider> LlmProvider for BudgetedLlmProvider<L> {
    /// Unmetered passthrough — see the module doc comment. Callers that
    /// have a `TenantContext` in scope should prefer
    /// [`BudgetedLlmProvider::complete_for_tenant`].
    async fn complete(&self, messages: &[LlmMessage]) -> Result<LlmResponse, CoreError> {
        self.inner.complete(messages).await
    }

    fn provider_name(&self) -> &str {
        self.inner.provider_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockLlmProvider;
    use milona_core::traits::MessageRole;
    use uuid::Uuid;

    #[tokio::test]
    async fn allows_calls_within_budget() {
        let mock = MockLlmProvider::new("ok");
        let provider = BudgetedLlmProvider::new(mock, TokenBudget::new(1000));
        let tenant = TenantId::new(Uuid::new_v4());

        let response = provider
            .complete_for_tenant(
                tenant,
                &[LlmMessage {
                    role: MessageRole::User,
                    content: "hi".into(),
                }],
            )
            .await
            .unwrap();
        assert_eq!(response.content, "ok");
        assert!(provider.usage_for(tenant) > 0);
    }

    #[tokio::test]
    async fn rejects_calls_once_tenant_exceeds_budget() {
        let mock = MockLlmProvider::new("ok");
        // Budget of 1 token: the first successful call's usage (>= 1 token
        // accounted) will already meet/exceed it, so the second call must be
        // rejected without reaching the inner provider.
        let provider = BudgetedLlmProvider::new(mock, TokenBudget::new(1));
        let tenant = TenantId::new(Uuid::new_v4());
        let msgs = [LlmMessage {
            role: MessageRole::User,
            content: "hi".into(),
        }];

        provider.complete_for_tenant(tenant, &msgs).await.unwrap();

        let err = provider
            .complete_for_tenant(tenant, &msgs)
            .await
            .unwrap_err();
        assert!(matches!(err, BudgetError::Exceeded { .. }));

        let core_err: CoreError = err.into();
        assert!(matches!(core_err, CoreError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn tenants_have_independent_budgets() {
        let mock = MockLlmProvider::new("ok");
        let provider = BudgetedLlmProvider::new(mock, TokenBudget::new(1));
        let tenant_a = TenantId::new(Uuid::new_v4());
        let tenant_b = TenantId::new(Uuid::new_v4());
        let msgs = [LlmMessage {
            role: MessageRole::User,
            content: "hi".into(),
        }];

        provider.complete_for_tenant(tenant_a, &msgs).await.unwrap();
        // Tenant A is now over budget...
        assert!(provider.complete_for_tenant(tenant_a, &msgs).await.is_err());
        // ...but tenant B is unaffected.
        assert!(provider.complete_for_tenant(tenant_b, &msgs).await.is_ok());
    }
}
