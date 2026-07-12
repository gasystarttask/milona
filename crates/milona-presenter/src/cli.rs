//! `clap`-based CLI. Reuses `AppState::answer_question` directly — the same
//! core application handler the axum `/v1/query` route calls — so the API
//! and CLI presenters share logic rather than duplicating it, per
//! ROADMAP.md "same handlers reused by both".

use crate::auth::ApiKeyDirectory;
use crate::state::AppState;
use clap::{Parser, Subcommand};
use milona_core::tenant::{Role, TenantContext, TenantId};
use std::collections::HashMap;

#[derive(Parser, Debug)]
#[command(name = "milona", about = "Milona GenAI application presenter")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the axum HTTP API server.
    Serve {
        #[arg(long, default_value = "0.0.0.0:8080")]
        addr: String,
    },
    /// Run a single question directly against the GenAI application loop,
    /// without starting the HTTP server.
    Query {
        /// The question to ask.
        question: String,
        /// Tenant UUID to run the query as. A fresh random tenant is used
        /// if omitted, since the in-process demo wiring has no persistent
        /// tenant directory.
        #[arg(long)]
        tenant: Option<uuid::Uuid>,
        #[arg(long, default_value = "cli-user")]
        subject: String,
    },
}

/// Runs the `milona query` subcommand: builds a default in-process
/// `AppState` (same wiring `serve` uses) and calls the shared
/// `answer_question` handler directly.
pub async fn run_query_command(
    question: &str,
    tenant: Option<uuid::Uuid>,
    subject: &str,
) -> anyhow::Result<String> {
    let state = AppState::new_default(HashMap::new());
    let tenant_id = TenantId::new(tenant.unwrap_or_else(uuid::Uuid::new_v4));
    let ctx = TenantContext::new(tenant_id, Role::Member, subject.to_string());

    let response = state.answer_question(&ctx, question).await?;
    Ok(response.answer)
}

/// Builds the `AppState` used by `milona serve`, loading API keys from the
/// `MILONA_API_KEYS` environment variable (format:
/// `key1:tenant-uuid:role:subject,key2:tenant-uuid:role:subject`) if set,
/// falling back to a single generated development key logged at startup so
/// the server is still usable out of the box in local/dev environments
/// without silently shipping unauthenticated.
///
/// Storage backend selection is delegated to
/// `AppState::new_from_env`: the safe in-memory default unless
/// `MILONA_STORAGE_BACKEND=mongo` is set (see that function's doc comment).
pub async fn build_serve_state() -> anyhow::Result<(AppState, Option<String>)> {
    match std::env::var("MILONA_API_KEYS") {
        Ok(raw) if !raw.trim().is_empty() => {
            let keys = parse_api_keys(&raw);
            Ok((AppState::new_from_env(keys).await?, None))
        }
        _ => {
            let dev_key = uuid::Uuid::new_v4().to_string();
            let tenant_id = TenantId::new(uuid::Uuid::new_v4());
            let mut keys = HashMap::new();
            keys.insert(
                dev_key.clone(),
                crate::state::ApiKeyRecord {
                    tenant_id,
                    role: Role::Admin,
                    subject: "dev".to_string(),
                },
            );
            Ok((AppState::new_from_env(keys).await?, Some(dev_key)))
        }
    }
}

fn parse_api_keys(raw: &str) -> HashMap<String, crate::state::ApiKeyRecord> {
    let mut keys = HashMap::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let parts: Vec<&str> = entry.splitn(4, ':').collect();
        if parts.len() != 4 {
            tracing::warn!(entry, "skipping malformed MILONA_API_KEYS entry");
            continue;
        }
        let (key, tenant_str, role_str, subject) = (parts[0], parts[1], parts[2], parts[3]);
        let Ok(tenant_uuid) = uuid::Uuid::parse_str(tenant_str) else {
            tracing::warn!(
                entry,
                "skipping MILONA_API_KEYS entry with invalid tenant uuid"
            );
            continue;
        };
        let role = match role_str.to_ascii_lowercase().as_str() {
            "admin" => Role::Admin,
            "member" => Role::Member,
            "readonly" | "read_only" => Role::ReadOnly,
            "service" => Role::Service,
            _ => {
                tracing::warn!(entry, "skipping MILONA_API_KEYS entry with unknown role");
                continue;
            }
        };
        keys.insert(
            key.to_string(),
            crate::state::ApiKeyRecord {
                tenant_id: TenantId::new(tenant_uuid),
                role,
                subject: subject.to_string(),
            },
        );
    }
    keys
}

/// Used by tests/composition to build an `ApiKeyDirectory` directly.
pub fn directory_from_keys(keys: HashMap<String, crate::state::ApiKeyRecord>) -> ApiKeyDirectory {
    ApiKeyDirectory::new(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_api_keys_env_value() {
        let tenant = uuid::Uuid::new_v4();
        let raw = format!("abc123:{tenant}:member:user-1");
        let keys = parse_api_keys(&raw);
        assert_eq!(keys.len(), 1);
        let rec = &keys["abc123"];
        assert_eq!(rec.tenant_id, TenantId::new(tenant));
        assert_eq!(rec.subject, "user-1");
    }

    #[test]
    fn skips_malformed_entries() {
        let keys = parse_api_keys("not-well-formed,,also:bad");
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn query_command_returns_an_answer() {
        let answer = run_query_command("hello", None, "tester").await.unwrap();
        assert!(!answer.is_empty());
    }
}
