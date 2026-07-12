//! Tool registry: looks up and invokes registered [`Tool`]s by name.
//!
//! Registers both native Rust tools (this crate's `native` module) and, once
//! unblocked, MCP-discovered tools uniformly — a caller invokes by name and
//! does not need to know whether the tool is native or MCP-backed.

use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{Tool, ToolInvocation, ToolResult};
use std::collections::HashMap;
use std::sync::Arc;

/// Holds a set of [`Tool`] implementations keyed by name and dispatches
/// [`ToolInvocation`]s to the right one, tenant-scoped like every other call
/// in the system (every `invoke` takes a `&TenantContext`).
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// An empty registry with no tools.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool, keyed by its own `name()`. Registering a second tool
    /// under the same name replaces the first.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Look up a tool by name without invoking it.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Number of tools currently registered.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Names of all registered tools, in no particular order.
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.keys().map(String::as_str).collect()
    }

    /// Look up and invoke a tool by name. Returns `CoreError::NotFound` if no
    /// tool with that name is registered.
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
    use crate::native::{CalculatorTool, CurrentTimeTool, EchoTool};
    use milona_core::tenant::TenantId;
    use uuid::Uuid;

    fn ctx() -> TenantContext {
        TenantContext::service(TenantId::new(Uuid::new_v4()))
    }

    #[tokio::test]
    async fn registry_resolves_and_invokes_the_right_tool_by_name() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool));
        registry.register(Arc::new(CurrentTimeTool));
        registry.register(Arc::new(CalculatorTool));

        assert_eq!(registry.len(), 3);

        let result = registry
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"message": "hi"}),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.content, "hi");

        let result = registry
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "calculator".to_string(),
                    arguments: serde_json::json!({"op": "add", "a": 2.0, "b": 3.0}),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.content, "5");
    }

    #[tokio::test]
    async fn registry_errors_with_not_found_on_unknown_tool_name() {
        let registry = ToolRegistry::new();
        let err = registry
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "does-not-exist".to_string(),
                    arguments: serde_json::json!({}),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }

    #[test]
    fn get_returns_none_for_unregistered_name() {
        let registry = ToolRegistry::new();
        assert!(registry.get("nope").is_none());
        assert!(registry.is_empty());
    }
}
