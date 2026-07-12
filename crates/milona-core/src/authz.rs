use crate::tenant::{Role, TenantContext, TenantId};

/// A knowledge scope a caller may be granted or denied access to. Enforced
/// in `milona-knowledge` (Phase 3), not just at the HTTP edge, so internal
/// callers (tools, MCP) cannot bypass it either. This is a minimal
/// hand-rolled policy check per ROADMAP.md Phase 0.5; swap for `casbin-rs`
/// if policy complexity grows beyond role + tenant-match.
pub trait AuthzPolicy: Send + Sync {
    fn can_read(&self, ctx: &TenantContext, resource_tenant: TenantId) -> bool;
    fn can_write(&self, ctx: &TenantContext, resource_tenant: TenantId) -> bool;
}

/// Default policy: a caller may only ever act within their own tenant.
/// `ReadOnly` may read but not write; `Service` may both, since internal
/// jobs (ingestion, tools) run as `Service` within a single tenant.
#[derive(Debug, Default, Clone, Copy)]
pub struct SameTenantPolicy;

impl AuthzPolicy for SameTenantPolicy {
    fn can_read(&self, ctx: &TenantContext, resource_tenant: TenantId) -> bool {
        ctx.tenant_id == resource_tenant
    }

    fn can_write(&self, ctx: &TenantContext, resource_tenant: TenantId) -> bool {
        ctx.tenant_id == resource_tenant && !matches!(ctx.role, Role::ReadOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn denies_cross_tenant_read() {
        let policy = SameTenantPolicy;
        let tenant_a = TenantId::new(Uuid::new_v4());
        let tenant_b = TenantId::new(Uuid::new_v4());
        let ctx = TenantContext::new(tenant_a, Role::Member, "user-1");

        assert!(policy.can_read(&ctx, tenant_a));
        assert!(!policy.can_read(&ctx, tenant_b));
    }

    #[test]
    fn read_only_role_cannot_write() {
        let policy = SameTenantPolicy;
        let tenant = TenantId::new(Uuid::new_v4());
        let ctx = TenantContext::new(tenant, Role::ReadOnly, "user-2");

        assert!(policy.can_read(&ctx, tenant));
        assert!(!policy.can_write(&ctx, tenant));
    }

    #[test]
    fn service_role_can_write_within_tenant() {
        let policy = SameTenantPolicy;
        let tenant = TenantId::new(Uuid::new_v4());
        let ctx = TenantContext::service(tenant);

        assert!(policy.can_write(&ctx, tenant));
    }
}
