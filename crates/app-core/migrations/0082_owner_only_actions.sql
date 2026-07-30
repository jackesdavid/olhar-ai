-- Revoke the owner-only actions from the system Admin role.
--
-- `users.password_reset` and `users.remove` were added to the permission catalog in the previous
-- deploy, and `sync_system_roles` does what it does for every new action: on boot it grants the
-- whole catalog to every tenant's Admin role, so a new permission always reaches somebody. For
-- these two that default is wrong — resetting a password is an account-takeover primitive and
-- removing a user is irreversible, so they belong to the tenant OWNER and should be delegated
-- deliberately rather than by a restart.
--
-- This is the one-time cleanup of the grants that boot already created. The DURABLE half is in
-- code: `sync_system_roles` now excludes `rbac::owner_only_perms()` from the Admin grant. Without
-- that exclusion this DELETE would be undone the next time control started, which is exactly the
-- kind of fix that looks applied and silently is not.
--
-- Scoped to the SYSTEM Admin role only. A custom role that an owner deliberately granted these to
-- is a decision, not an accident, and is left alone.
--
-- The owner is unaffected either way: ownership is checked directly (`handlers::may_reset_passwords`),
-- not through a role grant, so a tenant can never lose its own recovery path to this.
DELETE FROM role_permissions rp
 USING roles r
 WHERE rp.role_id = r.id
   AND r.is_system
   AND r.name = 'Admin'
   AND rp.permission_key IN ('users.password_reset', 'users.remove');
