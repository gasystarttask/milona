//! Native, in-process [`Tool`](milona_core::traits::Tool) implementations.
//!
//! Every tool here is tenant-context-aware: `invoke()` always receives a
//! `&TenantContext` (per ROADMAP.md Phase 0.5's tenant-isolation discipline),
//! even though these particular tools don't need to read from tenant-scoped
//! storage. Tools that do (future work) get the context for free from the
//! same call signature.

mod calculator;
mod current_time;
mod echo;

pub use calculator::CalculatorTool;
pub use current_time::CurrentTimeTool;
pub use echo::EchoTool;
