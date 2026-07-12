//! MCP server/client integration — **STUBBED**, blocked on a sandbox MSRV
//! conflict. Read this whole module comment before touching anything here.
//!
//! # What Phase 5 asked for
//!
//! ROADMAP.md Phase 5 calls for integrating the official `rmcp` SDK: a
//! minimal MCP server exposing this crate's native tools, and a minimal MCP
//! client wrapper to invoke tools on an external MCP server, pinned to a
//! version that fixes RUSTSEC-2026-0189 (HIGH, DNS-rebinding in the
//! Streamable HTTP server transport — fixed in `rmcp` >=1.4.0).
//!
//! # Why it's stubbed instead
//!
//! This workspace pins `rust-version = "1.87"` (see the root `Cargo.toml`,
//! and the same constraint already documented in `milona-adapter`'s
//! `retry.rs` for `genai`/`darling`). Every published `rmcp` release that
//! contains the RUSTSEC-2026-0189 fix — every 1.x from 1.4.0 through 1.8.0,
//! and every 2.x through the current 2.2.0, as of 2026-07-12 — has:
//!
//! - `rmcp-macros` hard-depending on `darling = "^0.23"`, which itself
//!   requires rustc >=1.88 (identical failure mode to the `genai` MSRV
//!   conflict), even with `default-features = false, features = [...]`
//!   dropping the `macros` feature so it isn't pulled in transitively; *and*
//! - independent of that, `rmcp`'s own crate is `edition = "2024"` and its
//!   source (`src/model/elicitation_schema.rs`, in the core `model` module
//!   that every feature set pulls in — not gated behind any optional
//!   feature) uses let-chains (`if let Some(x) = y && cond`), a syntax this
//!   sandbox's pinned rustc 1.87 rejects with `error[E0658]`.
//!
//! Both were reproduced directly in this sandbox: `cargo build -p
//! milona-tools` with `rmcp = "=2.2.0"` and then with `rmcp = "=1.4.0"`
//! (`default-features = false`, only `client`/`transport-io`/
//! `transport-child-process` features — i.e. no `macros`, no `server`)
//! still fails to compile `rmcp` itself on the let-chain, before
//! `milona-tools`'s own code is ever reached. There is no version in
//! `rmcp`'s release history that both contains the RUSTSEC fix and avoids
//! this — the let-chain usage predates 1.4.0. No dependency added by this
//! module to `Cargo.toml`; the experiment was reverted so as not to leave a
//! broken build for other crates in this workspace.
//!
//! # What's here instead
//!
//! Trait-shaped placeholders (`McpServer`, `McpClient`) that describe the
//! intended surface so the rest of the codebase (the GenAI tool-use loop,
//! `milona-knowledge`) can code against a stable shape and swap in the real
//! `rmcp`-backed implementation later with no call-site changes. Every
//! method returns `CoreError::Unsupported` — nothing here silently
//! pretends to talk MCP.
//!
//! # Unblocking this later
//!
//! Re-run the experiment once either (a) this workspace's `rust-version` is
//! raised to >=1.88, or (b) a future `rmcp` release backports the fix to a
//! rustc-1.87-compatible edition/dependency set. Then:
//! 1. Add `rmcp = "<patched-version>"` to this crate's `[dependencies]`
//!    (>=1.4.0, checked against the RUSTSEC advisory database at
//!    integration time in case a newer advisory supersedes this one).
//! 2. Replace `McpServer`/`McpClient` below with real wrappers over
//!    `rmcp::ServerHandler`/`rmcp::service::ServiceExt` (server side) and
//!    `rmcp::service::serve_client` + a transport (client side).
//! 3. Keep every tool invocation tenant-scoped: an MCP-discovered tool must
//!    still go through `ToolRegistry::invoke` with a `TenantContext`, same
//!    as native tools — MCP is a transport for tool *discovery/execution*,
//!    not a bypass of tenant isolation.

use crate::native::{CalculatorTool, CurrentTimeTool, EchoTool};
use crate::registry::ToolRegistry;
use async_trait::async_trait;
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{Tool, ToolInvocation, ToolResult};
use std::sync::Arc;

/// Placeholder for an MCP server exposing Milona's native tools. Real
/// implementation would wrap `rmcp::ServerHandler` and serve over stdio or
/// (once the RUSTSEC-fixed Streamable HTTP transport is buildable here)
/// HTTP.
pub struct McpServer {
    registry: ToolRegistry,
}

impl McpServer {
    /// Builds a server-side registry pre-populated with this crate's native
    /// tools, mirroring what an `rmcp`-backed server would expose.
    pub fn with_native_tools() -> Self {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool));
        registry.register(Arc::new(CurrentTimeTool));
        registry.register(Arc::new(CalculatorTool));
        Self { registry }
    }

    /// Tool names this stub server would advertise over MCP's
    /// `tools/list`. Real implementation: return this from
    /// `ServerHandler::list_tools`.
    pub fn advertised_tool_names(&self) -> Vec<&str> {
        self.registry.tool_names()
    }

    /// Would serve `tools/call` for `name` over a real MCP transport. The
    /// registry lookup/invoke path already works (this delegates to it) —
    /// only the wire transport is missing.
    pub async fn call_tool(
        &self,
        ctx: &TenantContext,
        invocation: ToolInvocation,
    ) -> Result<ToolResult, CoreError> {
        self.registry.invoke(ctx, invocation).await
    }
}

/// Placeholder for an MCP client wrapper that would invoke tools exposed by
/// an *external* MCP server (a separate process/service, not this crate's
/// own tools). Implements the [`Tool`] trait itself so, once real, an
/// MCP-discovered remote tool can be registered into a [`ToolRegistry`]
/// exactly like a native one — same uniform invocation path.
pub struct McpClientTool {
    /// Name as it would be discovered from the remote server's `tools/list`.
    name: String,
    description: String,
}

impl McpClientTool {
    /// Constructs a client-side handle for a tool that *would* be
    /// discovered from a remote MCP server at `server_addr`. Stubbed: no
    /// connection is actually made.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

#[async_trait]
impl Tool for McpClientTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    async fn invoke(
        &self,
        _ctx: &TenantContext,
        _invocation: ToolInvocation,
    ) -> Result<ToolResult, CoreError> {
        Err(CoreError::Unsupported(format!(
            "MCP client transport is not available in this build (rmcp is blocked by an \
             MSRV conflict with this workspace's pinned rustc 1.87 — see the module doc \
             comment on milona_tools::mcp for details); cannot invoke remote tool '{}'",
            self.name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use milona_core::tenant::TenantId;
    use uuid::Uuid;

    fn ctx() -> TenantContext {
        TenantContext::service(TenantId::new(Uuid::new_v4()))
    }

    #[test]
    fn stub_server_advertises_the_native_tools() {
        let server = McpServer::with_native_tools();
        let mut names = server.advertised_tool_names();
        names.sort_unstable();
        assert_eq!(names, vec!["calculator", "current_time", "echo"]);
    }

    #[tokio::test]
    async fn stub_server_still_invokes_registered_tools_locally() {
        let server = McpServer::with_native_tools();
        let result = server
            .call_tool(
                &ctx(),
                ToolInvocation {
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"message": "via stub server"}),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.content, "via stub server");
    }

    #[tokio::test]
    async fn stub_client_tool_reports_unsupported_rather_than_pretending() {
        let tool = McpClientTool::new("remote_tool", "a tool on an external MCP server");
        let err = tool
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "remote_tool".to_string(),
                    arguments: serde_json::json!({}),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Unsupported(_)));
    }
}
