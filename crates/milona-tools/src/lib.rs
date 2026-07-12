//! Phase 5 — Tools & MCP. Implements `milona_core::traits::Tool`, native and
//! MCP-backed.
//!
//! - [`native`]: concrete, in-process `Tool` implementations (`echo`,
//!   `current_time`, `calculator`).
//! - [`registry`]: [`registry::ToolRegistry`], the name -> tool lookup/
//!   dispatch table shared by native and (eventually) MCP-discovered tools.
//! - [`mcp`]: MCP server/client integration. **Currently stubbed** — see
//!   that module's doc comment for why `rmcp` cannot build in this sandbox
//!   and what's needed to unblock it.

pub mod mcp;
pub mod native;
pub mod registry;

pub use native::{CalculatorTool, CurrentTimeTool, EchoTool};
pub use registry::ToolRegistry;
