-- Multi-tenant SaaS foundation (Phase 1): tenants, users, social-login identities,
-- and configurable RBAC (roles are tenant-owned; authorization is action-based).
--
-- Authorization rule (enforced in code, documented here): guards check PERMISSIONS
-- (actions), never role names. A user's effective permissions = the union of their
-- roles' permissions. The `users.is_master` flag is the SaaS-admin bypass — it grants
-- every action EXCEPT `live.view` (tenant live feeds are private even from the master).

CREATE EXTENSION IF NOT EXISTS citext;

-- A client account. All domain data is scoped to a tenant (Phase 2 adds tenant_id
-- to the domain tables). Master users have no tenant.
CREATE TABLE IF NOT EXISTS tenants (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name       text NOT NULL,
    slug       text UNIQUE NOT NULL,
    status     text NOT NULL DEFAULT 'active',
    created_at timestamptz NOT NULL DEFAULT now()
);

-- A person who logs in. `tenant_id` NULL + `is_master` = the SaaS administrator.
-- `password_hash` NULL for social-only accounts (Google/Apple).
CREATE TABLE IF NOT EXISTS users (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     uuid REFERENCES tenants(id) ON DELETE CASCADE,
    email         citext UNIQUE NOT NULL,
    password_hash text,
    is_master     boolean NOT NULL DEFAULT false,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS users_tenant_idx ON users(tenant_id);

-- Linked social identities (Google/Apple). One provider account → one user.
CREATE TABLE IF NOT EXISTS oauth_identities (
    provider     text NOT NULL,
    provider_uid text NOT NULL,
    user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, provider_uid)
);

-- The catalog of actions the app gates on. Seeded by the control plane; the
-- permission-matrix UI lists these as the columns.
CREATE TABLE IF NOT EXISTS permissions (
    key         text PRIMARY KEY,
    description text NOT NULL DEFAULT ''
);

-- Roles are TENANT-OWNED data. System templates (Admin/Viewer) are cloned into each
-- tenant at creation and flagged is_system (protected from deletion) — no shared rows.
CREATE TABLE IF NOT EXISTS roles (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id  uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name       text NOT NULL,
    is_system  boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, name)
);
CREATE INDEX IF NOT EXISTS roles_tenant_idx ON roles(tenant_id);

-- The matrix: which actions a role grants.
CREATE TABLE IF NOT EXISTS role_permissions (
    role_id        uuid NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_key text NOT NULL REFERENCES permissions(key) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_key)
);

-- Role assignment. Effective permissions = union across a user's roles.
CREATE TABLE IF NOT EXISTS user_roles (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id uuid NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);
