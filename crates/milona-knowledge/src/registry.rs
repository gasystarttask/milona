//! Minimal tool registry over `milona_core::traits::Tool`.
//!
//! `milona-tools` (Phase 5) currently exposes only the `Tool` trait itself
//! (its `lib.rs` is still a stub with no concrete tools or registry), so
//! this crate provides the small lookup-by-name registry the GenAI loop
//! needs to resolve a `ToolInvocation` to a `Tool` impl. It is intentionally
//! trivial: swap for whatever richer registry `milona-tools` grows later
//! (e.g. MCP-discovered tool registration) without changing the loop's call
//! site, since it only depends on `milona_core::traits::Tool`.

use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{Tool, ToolInvocation, ToolResult};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Look up and invoke a tool by name, tenant-scoped like every other
    /// call in the system.
    pub async fn invoke(
        &self,
        ctx: &TenantContext,
        invocation: ToolInvocation,
    ) -> Result<ToolResult, CoreError> {
        let tool = self.get(&invocation.name).ok_or_else(|| {
            CoreError::NotFound(format!(
                "no tool registered with name '{}'",
                invocation.name
            ))
        })?;
        tool.invoke(ctx, invocation).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use milona_core::tenant::TenantId;
    use uuid::Uuid;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echoes its input arguments back as a string"
        }
        async fn invoke(
            &self,
            _ctx: &TenantContext,
            invocation: ToolInvocation,
        ) -> Result<ToolResult, CoreError> {
            Ok(ToolResult {
                content: invocation.arguments.to_string(),
            })
        }
    }

    #[tokio::test]
    async fn registry_resolves_and_invokes_by_name() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool));

        let ctx = TenantContext::service(TenantId::new(Uuid::new_v4()));
        let result = registry
            .invoke(
                &ctx,
                ToolInvocation {
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"x": 1}),
                },
            )
            .await
            .unwrap();

        assert_eq!(result.content, "{\"x\":1}");
    }

    #[tokio::test]
    async fn registry_errors_on_unknown_tool() {
        let registry = ToolRegistry::new();
        let ctx = TenantContext::service(TenantId::new(Uuid::new_v4()));
        let err = registry
            .invoke(
                &ctx,
                ToolInvocation {
                    name: "missing".to_string(),
                    arguments: serde_json::json!({}),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }
}
