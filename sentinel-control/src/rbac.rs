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
    ("roles.manage", "Manage roles and permissions"),
    ("recordings.read", "View recorded clips"),
    ("recordings.write", "Create or delete recordings"),
    ("billing.read", "View billing, plan, and invoices"),
];

/// All action keys (the Admin role gets these — including `live.view`).
pub fn all_perms() -> Vec<&'static str> {
    PERMISSIONS.iter().map(|(k, _)| *k).collect()
}

/// Read-only actions (the default Viewer role) — NO `live.view` (Admin-only by default).
pub fn viewer_perms() -> Vec<&'static str> {
    PERMISSIONS.iter().map(|(k, _)| *k).filter(|k| k.ends_with(".read")).collect()
}
