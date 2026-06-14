//! Tenancy basics shared across the workspace. Every domain row is scoped to a
//! `tenant_id`; the repos take it explicitly and never derive it from client input.
//!
//! Single-tenant callers (the desktop shell, sentinel-flow today) pass
//! [`DEFAULT_TENANT_ID`] — the same well-known tenant the migration backfills existing
//! rows to. The cloud APIs pass the real tenant resolved from the JWT.
use uuid::Uuid;

/// The "Default" tenant (`11111111-1111-1111-1111-111111111111`) seeded by migration
/// 0011 and used by single-tenant callers + the data backfill.
pub const DEFAULT_TENANT_ID: Uuid = Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111);
