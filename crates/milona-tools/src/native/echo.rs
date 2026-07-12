//! `echo` — a diagnostic tool that returns the `message` argument verbatim.
//!
//! Useful for exercising the tool-invocation path end to end (registry
//! lookup, GenAI loop tool-use wiring, MCP round-trip once unblocked)
//! without depending on any other capability.

use async_trait::async_trait;
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{Tool, ToolInvocation, ToolResult};

pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echoes back the string passed in the `message` argument. Useful for testing tool wiring."
    }

    async fn invoke(
        &self,
        _ctx: &TenantContext,
        invocation: ToolInvocation,
    ) -> Result<ToolResult, CoreError> {
        let message = invocation
            .arguments
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CoreError::InvalidInput("echo requires a string `message` argument".to_string())
            })?;

        Ok(ToolResult {
            content: message.to_string(),
        })
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

    #[tokio::test]
    async fn echoes_the_message_argument_back() {
        let tool = EchoTool;
        let result = tool
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"message": "hello world"}),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.content, "hello world");
    }

    #[tokio::test]
    async fn rejects_missing_message_argument() {
        let tool = EchoTool;
        let err = tool
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "echo".to_string(),
                    arguments: serde_json::json!({}),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    #[test]
    fn name_and_description_are_stable() {
        let tool = EchoTool;
        assert_eq!(tool.name(), "echo");
        assert!(!tool.description().is_empty());
    }
}
