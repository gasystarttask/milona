//! `calculator` — a minimal four-function arithmetic tool.
//!
//! Arguments: `{"op": "add"|"sub"|"mul"|"div", "a": <number>, "b": <number>}`.
//! Deliberately simple (Phase 5's stated goal is "at least 2 simple native
//! tools"); no expression parsing, just direct binary ops on two operands.

use async_trait::async_trait;
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{Tool, ToolInvocation, ToolResult};

pub struct CalculatorTool;

fn arg_number(invocation: &ToolInvocation, key: &str) -> Result<f64, CoreError> {
    invocation
        .arguments
        .get(key)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| {
            CoreError::InvalidInput(format!("calculator requires a numeric `{key}` argument"))
        })
}

fn arg_op(invocation: &ToolInvocation) -> Result<&str, CoreError> {
    invocation
        .arguments
        .get("op")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CoreError::InvalidInput(
                "calculator requires a string `op` argument (one of: add, sub, mul, div)"
                    .to_string(),
            )
        })
}

/// Formats a result without a trailing `.0` for whole numbers, so `2 + 3`
/// reads as `"5"` rather than `"5.0"`.
fn format_result(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

#[async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Performs basic arithmetic. Arguments: op (\"add\"|\"sub\"|\"mul\"|\"div\"), a (number), b (number)."
    }

    async fn invoke(
        &self,
        _ctx: &TenantContext,
        invocation: ToolInvocation,
    ) -> Result<ToolResult, CoreError> {
        let op = arg_op(&invocation)?;
        let a = arg_number(&invocation, "a")?;
        let b = arg_number(&invocation, "b")?;

        let result = match op {
            "add" => a + b,
            "sub" => a - b,
            "mul" => a * b,
            "div" => {
                if b == 0.0 {
                    return Err(CoreError::InvalidInput(
                        "calculator division by zero".to_string(),
                    ));
                }
                a / b
            }
            other => {
                return Err(CoreError::InvalidInput(format!(
                    "calculator unsupported op '{other}', expected one of: add, sub, mul, div"
                )))
            }
        };

        Ok(ToolResult {
            content: format_result(result),
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

    async fn calc(op: &str, a: f64, b: f64) -> Result<ToolResult, CoreError> {
        CalculatorTool
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "calculator".to_string(),
                    arguments: serde_json::json!({"op": op, "a": a, "b": b}),
                },
            )
            .await
    }

    #[tokio::test]
    async fn adds_two_numbers() {
        assert_eq!(calc("add", 2.0, 3.0).await.unwrap().content, "5");
    }

    #[tokio::test]
    async fn subtracts_two_numbers() {
        assert_eq!(calc("sub", 10.0, 4.0).await.unwrap().content, "6");
    }

    #[tokio::test]
    async fn multiplies_two_numbers() {
        assert_eq!(calc("mul", 6.0, 7.0).await.unwrap().content, "42");
    }

    #[tokio::test]
    async fn divides_two_numbers() {
        assert_eq!(calc("div", 9.0, 2.0).await.unwrap().content, "4.5");
    }

    #[tokio::test]
    async fn rejects_division_by_zero() {
        let err = calc("div", 1.0, 0.0).await.unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn rejects_unknown_operator() {
        let err = calc("pow", 2.0, 3.0).await.unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn rejects_missing_operands() {
        let err = CalculatorTool
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "calculator".to_string(),
                    arguments: serde_json::json!({"op": "add", "a": 1.0}),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    #[test]
    fn name_and_description_are_stable() {
        let tool = CalculatorTool;
        assert_eq!(tool.name(), "calculator");
        assert!(!tool.description().is_empty());
    }
}
