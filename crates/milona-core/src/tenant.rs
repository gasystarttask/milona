use serde::{Deserialize, Serialize};
use std::fmt;

/// Opaque tenant identifier. Every storage/knowledge/tool call carries one so
/// isolation is enforced at the type level, not by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(uuid::Uuid);

impl TenantId {
    pub fn new(id: uuid::Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A caller's role within a tenant, used by the AuthZ policy layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Member,
    ReadOnly,
    /// Internal service-to-service caller (tools, MCP, background jobs).
    Service,
}

/// Carried through every request so storage and knowledge queries can be
/// scoped without each call site re-deriving it. Constructing one is the
/// single point where authentication/authorization must already have
/// happened (see `milona-presenter`'s auth middleware) — nothing downstream
/// should trust a `TenantContext` it did not receive from that boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantContext {
    pub tenant_id: TenantId,
    pub role: Role,
    /// Subject identifier (user id or service account) for audit logging.
    pub subject: String,
}

impl TenantContext {
    pub fn new(tenant_id: TenantId, role: Role, subject: impl Into<String>) -> Self {
        Self {
            tenant_id,
            role,
            subject: subject.into(),
        }
    }

    /// Internal service contexts (tools, MCP, ingestion jobs) run as `Service`
    /// role and must still carry a real `tenant_id` — there is no
    /// tenant-less path through the system.
    pub fn service(tenant_id: TenantId) -> Self {
        Self::new(tenant_id, Role::Service, "system")
    }
}
