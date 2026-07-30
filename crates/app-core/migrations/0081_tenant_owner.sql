-- Tenant owner: the one account that cannot be locked out of its own tenant.
--
-- Until now "who owns this tenant" was implicit — whoever happened to hold the Admin role. That is
-- fine until roles get edited: an Admin can remove `users.manage` from the Admin role, or drop the
-- last admin's role assignment, and the tenant is left with nobody who can fix it. An owner is a
-- property of the TENANT, not of a role grant, so it survives any role edit.
--
-- On `tenants` rather than a flag on `users`, because the invariant is "one owner per tenant" and a
-- column here makes that true by construction. A `users.is_owner` boolean would allow two owners,
-- or none, with nothing to stop either.
--
-- ON DELETE SET NULL, never CASCADE: deleting a user must not delete their tenant and everything in
-- it. A tenant with a NULL owner is a recoverable state (the master console can reassign); a tenant
-- deleted by a user removal is not.
ALTER TABLE tenants
  ADD COLUMN IF NOT EXISTS owner_user_id uuid REFERENCES users(id) ON DELETE SET NULL;

-- Backfill: the tenant's earliest non-master member. That is the account `provision_tenant_with_admin`
-- created at signup, so for every tenant that came through the normal path this picks exactly the
-- person who created it. Masters are excluded because a master belongs to no tenant and must never
-- become one's owner. Tenants with no members are left NULL rather than guessed at.
UPDATE tenants t
   SET owner_user_id = (
         SELECT u.id
           FROM users u
          WHERE u.tenant_id = t.id
            AND u.is_master = false
          ORDER BY u.created_at ASC
          LIMIT 1)
 WHERE t.owner_user_id IS NULL;

COMMENT ON COLUMN tenants.owner_user_id IS
  'The tenant owner: always holds users.password_reset regardless of role grants, and only the owner may reset the owner''s own password. NULL = ownerless (recoverable via the master console).';
