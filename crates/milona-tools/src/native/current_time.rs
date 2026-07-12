//! `current_time` — returns the current UTC time as an RFC 3339 string.
//!
//! Deliberately dependency-light: uses `std::time::SystemTime` rather than
//! pulling in a datetime crate, since RFC 3339 formatting of a Unix timestamp
//! is a handful of lines of integer math (accurate for any date the process
//! will realistically see; no leap-second handling, which RFC 3339/UTC
//! itself does not require civil consumers to model).

use async_trait::async_trait;
use milona_core::error::CoreError;
use milona_core::tenant::TenantContext;
use milona_core::traits::{Tool, ToolInvocation, ToolResult};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CurrentTimeTool;

#[async_trait]
impl Tool for CurrentTimeTool {
    fn name(&self) -> &str {
        "current_time"
    }

    fn description(&self) -> &str {
        "Returns the current UTC date and time in RFC 3339 format. Takes no arguments."
    }

    async fn invoke(
        &self,
        _ctx: &TenantContext,
        _invocation: ToolInvocation,
    ) -> Result<ToolResult, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::Other(anyhow::anyhow!("system clock before epoch: {e}")))?;
        Ok(ToolResult {
            content: format_rfc3339(now.as_secs()),
        })
    }
}

/// Formats a Unix timestamp (seconds since epoch, UTC) as RFC 3339:
/// `YYYY-MM-DDTHH:MM:SSZ`. Proleptic Gregorian calendar, no leap seconds.
fn format_rfc3339(unix_secs: u64) -> String {
    const SECS_PER_DAY: u64 = 86_400;
    let days_since_epoch = unix_secs / SECS_PER_DAY;
    let secs_of_day = unix_secs % SECS_PER_DAY;

    let (year, month, day) = civil_from_days(days_since_epoch as i64);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days` algorithm: converts a day count since
/// 1970-01-01 into a (year, month, day) civil (Gregorian) date. Public
/// domain algorithm, widely used (e.g. in `chrono`'s internals) precisely
/// because it avoids leap-year branching bugs.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
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
    async fn produces_a_well_formed_rfc3339_timestamp() {
        let tool = CurrentTimeTool;
        let result = tool
            .invoke(
                &ctx(),
                ToolInvocation {
                    name: "current_time".to_string(),
                    arguments: serde_json::json!({}),
                },
            )
            .await
            .unwrap();

        // e.g. "2026-07-12T14:03:22Z"
        assert_eq!(result.content.len(), 20);
        assert_eq!(result.content.as_bytes()[4], b'-');
        assert_eq!(result.content.as_bytes()[7], b'-');
        assert_eq!(result.content.as_bytes()[10], b'T');
        assert_eq!(result.content.as_bytes()[13], b':');
        assert_eq!(result.content.as_bytes()[16], b':');
        assert!(result.content.ends_with('Z'));
    }

    #[test]
    fn known_unix_epoch_values_format_correctly() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        // 2024-01-01T00:00:00Z
        assert_eq!(format_rfc3339(1_704_067_200), "2024-01-01T00:00:00Z");
        // 2000-02-29T00:00:00Z (leap day, sanity-checks the civil algorithm)
        assert_eq!(format_rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn name_and_description_are_stable() {
        let tool = CurrentTimeTool;
        assert_eq!(tool.name(), "current_time");
        assert!(!tool.description().is_empty());
    }
}
