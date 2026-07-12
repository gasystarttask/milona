//! Phase 3 — Model adapter. Implements `milona_core::traits::LlmProvider`,
//! with retry/backoff and per-tenant token budgets.
//!
//! ## Substitution note: `genai` is not wired in this build
//!
//! ROADMAP.md recommends wrapping the `genai` crate. In this sandbox,
//! `genai`'s transitive dependencies (`darling` 0.23, `serde_with` 3.21)
//! require `rustc >= 1.88`, while the workspace pins
//! `[workspace.package] rust-version = "1.87"` in the root `Cargo.toml` (not
//! modifiable by this crate) and the installed toolchain is exactly
//! `rustc 1.87.0`. `cargo build` against `genai = "0.7.0-beta.12"` fails
//! with `darling@0.23.0 requires rustc 1.88.0` before any Milona code is
//! even compiled.
//!
//! Given that constraint, this crate ships [`MockLlmProvider`] — clearly
//! labeled, deterministic, canned-response — as the concrete `LlmProvider`
//! behind the same trait `genai` would sit behind. Swapping in a real
//! `genai`-backed provider later only means adding a new module that
//! implements [`milona_core::traits::LlmProvider`] and constructing it
//! instead of `MockLlmProvider` at the composition root; nothing in
//! [`retry`] or [`budget`] depends on the concrete provider.

pub mod budget;
pub mod mock;
pub mod retry;

pub use budget::{BudgetError, BudgetedLlmProvider, TokenBudget};
pub use mock::MockLlmProvider;
pub use retry::{RetryConfig, RetryingLlmProvider};
