//! The permission catalog (actions the app gates on) and the system role templates
//! cloned into every tenant at creation. Roles are tenant-owned; these are just seed
//! material. `live.view` is the tenant-private action (default: Admin only).
pub const PERMISSIONS: &[(&str, &str)] = &[
    ("devices.read", "View cameras"),
    ("devices.write", "Add, edit, or remove cameras"),
    ("live.view", "Watch live camera feeds (tenant-private)"),
    ("alerts.read", "View alerts"),
    ("alerts.ack", "Acknowledge / deliver alerts"),
    ("alerts.write", "Create or delete alerts"),
    ("persons.read", "View people"),
    ("persons.write", "Manage people and face enrollment"),
    ("workflows.read", "View workflows"),
    ("workflows.write", "Edit workflows"),
    ("workflows.run", "Run workflows"),
    ("map.read", "View the floor map"),
    ("map.write", "Edit the floor map"),
    ("beacons.read", "View notification targets"),
    ("beacons.write", "Manage notification targets"),
    ("settings.read", "View settings"),
    ("settings.write", "Change settings"),
    ("users.manage", "Invite and manage users"),
    ("users.password_reset", "Set another user's password"),
    ("users.remove", "Remove a user from the tenant"),
    ("roles.manage", "Manage roles and permissions"),
    ("recordings.read", "View recorded clips"),
    ("recordings.write", "Create or delete recordings"),
    ("billing.read", "View billing, plan, and invoices"),
    ("billing.write", "Subscribe to a plan and manage payment"),
    ("tuya.read", "View and sync Tuya smart-home devices"),
    // Since #191 this also SENDS commands: the Integrations console writes a data point
    // straight to the device. On this account that means opening a gate from a browser, so the
    // description has to say so — a permission whose blurb understates it is how someone grants
    // "edit labels" and hands over the front door. A separate `tuya.control` is card #192.
    (
        "tuya.write",
        "Link a Tuya account, edit device labels, and send commands to devices",
    ),
    ("assistant.use", "Use the AI assistant (chat, voice)"),
];

/// All action keys (the Admin role gets these — including `live.view`).
pub fn all_perms() -> Vec<&'static str> {
    PERMISSIONS.iter().map(|(k, _)| *k).collect()
}

/// Read-only actions (the default Viewer role) — NO `live.view` (Admin-only by default).
///
/// `assistant.use` is granted despite not ending in `.read`: the assistant was available to every
/// authenticated user before it was gated, and it is a read-mostly surface, so making it Admin-only
/// would be a feature removal rather than a security gain. Gating it buys the ability for a tenant
/// to *revoke* it with a custom role, which is what was missing. Cost control belongs in a
/// per-tenant quota, not in a permission — an Admin can spend just as much.
/// Actions that are NEVER auto-granted to the system Admin role.
///
/// Everything else in [`PERMISSIONS`] is handed to every tenant's Admin on boot, so that a newly
/// added action reaches somebody. These two must not be: resetting a password is an account-takeover
/// primitive and removing a user is irreversible, so they belong to the tenant OWNER by default and
/// get delegated deliberately, not by a restart.
///
/// They stay in `PERMISSIONS` on purpose — an owner can still grant either to a role from the roles
/// screen. The owner always holds them regardless (see `handlers::may_reset_passwords`), so this
/// cannot lock a tenant out of its own recovery path.
pub fn owner_only_perms() -> &'static [&'static str] {
    &["users.password_reset", "users.remove"]
}

pub fn viewer_perms() -> Vec<&'static str> {
    PERMISSIONS
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| k.ends_with(".read") || *k == "assistant.use")
        .collect()
}
