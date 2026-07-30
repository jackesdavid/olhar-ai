//! Async data access for tenancy + RBAC (own pool, runs on this service's runtime).
use sqlx::PgPool;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub email: String,
    pub password_hash: Option<String>,
    pub is_master: bool,
    /// Consecutive failed sign-ins; cleared on success. Drives [`lockout_until`].
    #[sqlx(default)]
    pub failed_logins: i32,
    /// RFC3339; while in the future, sign-in is refused without checking the password.
    #[sqlx(default)]
    pub locked_until: Option<String>,
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct TenantRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub status: String,
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct RoleRow {
    pub id: Uuid,
    pub name: String,
    pub is_system: bool,
}

/// A tenant member for the user-management screen: identity + their assigned role ids
/// (so the UI can pre-check the role toggles).
#[derive(serde::Serialize)]
pub struct UserListRow {
    pub id: Uuid,
    pub email: String,
    pub is_master: bool,
    pub role_ids: Vec<Uuid>,
}

// ---- users ------------------------------------------------------------------

pub async fn find_user_by_email(pool: &PgPool, email: &str) -> sqlx::Result<Option<UserRow>> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, tenant_id, email, password_hash, is_master, failed_logins, locked_until \
         FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<UserRow>> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, tenant_id, email, password_hash, is_master, failed_logins, locked_until \
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn create_user(
    pool: &PgPool,
    tenant_id: Option<Uuid>,
    email: &str,
    password_hash: Option<&str>,
    is_master: bool,
) -> sqlx::Result<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO users (tenant_id, email, password_hash, is_master) VALUES ($1,$2,$3,$4) RETURNING id",
    )
    .bind(tenant_id)
    .bind(email)
    .bind(password_hash)
    .bind(is_master)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// The user's effective permissions = union across their roles.
pub async fn effective_perms(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT rp.permission_key
           FROM user_roles ur
           JOIN role_permissions rp ON rp.role_id = ur.role_id
          WHERE ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(k,)| k).collect())
}

/// The tenant's members with their assigned role ids.
pub async fn list_users(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<UserListRow>> {
    let users: Vec<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT id, email, is_master FROM users WHERE tenant_id = $1 ORDER BY email",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(users.len());
    for (id, email, is_master) in users {
        let roles: Vec<(Uuid,)> =
            sqlx::query_as("SELECT role_id FROM user_roles WHERE user_id = $1")
                .bind(id)
                .fetch_all(pool)
                .await?;
        out.push(UserListRow {
            id,
            email,
            is_master,
            role_ids: roles.into_iter().map(|(r,)| r).collect(),
        });
    }
    Ok(out)
}

pub async fn grant_role(pool: &PgPool, user_id: Uuid, role_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1,$2) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- boxes (edge appliances) ------------------------------------------------

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct BoxRow {
    pub id: Uuid,
    pub name: String,
    pub magicdns_host: Option<String>,
    pub version: Option<String>,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Edge health (reported on the heartbeat): serving | up | offline | disabled.
    pub tailscale_state: Option<String>,
    pub ffmpeg_ok: Option<bool>,
    /// Whether `gst-launch-1.0` is on PATH (card #138) — NULL until an updated edge reports it.
    pub gstreamer_ok: Option<bool>,
    pub health_at: Option<chrono::DateTime<chrono::Utc>>,
    pub recording_config: Option<serde_json::Value>,
    /// Reported platform, e.g. "macOS (aarch64)" / "Bazzite (x86_64)" (heartbeat).
    pub platform: Option<String>,
    /// Working GPU video-decode method ("vaapi"/"qsv"/"cuda"/"videotoolbox"/"v4l2m2m") or "none"
    /// (#120) — gates HD face capture. NULL until an updated edge reports it.
    pub hwaccel: Option<String>,
    /// Resource usage (percent 0..100), reported on the heartbeat via `X-Edge-Metrics`.
    /// `gpu_*` are null on boxes without an NVIDIA GPU. `metrics_at` = when last reported.
    pub cpu_pct: Option<i32>,
    pub mem_pct: Option<i32>,
    pub disk_pct: Option<i32>,
    pub gpu_pct: Option<i32>,
    pub gpu_mem_pct: Option<i32>,
    pub metrics_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// A camera the edge found via ONVIF WS-Discovery (surfaced in the cloud UI).
#[derive(sqlx::FromRow, serde::Serialize)]
pub struct DiscoveredRow {
    pub ip: String,
    pub name: String,
    pub xaddr: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub onvif_id: Option<String>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

/// Create a box and stamp its `platform` at creation. A phone light-edge (card #142) never sends the
/// heartbeat that would set `platform` on an appliance, so it must be recorded here or the box would
/// show blank/offline-unknown in the UI instead of as an on-demand phone.
pub async fn create_box_with_platform(
    pool: &PgPool,
    tenant_id: Uuid,
    name: &str,
    token_hash: &str,
    platform: &str,
) -> sqlx::Result<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO tenant_boxes (tenant_id, name, token_hash, platform) VALUES ($1,$2,$3,$4) RETURNING id",
    )
    .bind(tenant_id)
    .bind(name)
    .bind(token_hash)
    .bind(platform)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn create_box(
    pool: &PgPool,
    tenant_id: Uuid,
    name: &str,
    token_hash: &str,
) -> sqlx::Result<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO tenant_boxes (tenant_id, name, token_hash) VALUES ($1,$2,$3) RETURNING id",
    )
    .bind(tenant_id)
    .bind(name)
    .bind(token_hash)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn list_boxes(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<BoxRow>> {
    sqlx::query_as::<_, BoxRow>(
        "SELECT id, name, magicdns_host, version, last_seen, created_at, tailscale_state, ffmpeg_ok, gstreamer_ok, health_at, recording_config, platform, hwaccel, \
                cpu_pct, mem_pct, disk_pct, gpu_pct, gpu_mem_pct, metrics_at \
         FROM tenant_boxes WHERE tenant_id = $1 ORDER BY name",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}

pub async fn delete_box(pool: &PgPool, tenant_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM tenant_boxes WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Resolve a presented box token (already hashed) to its tenant, stamping last_seen and
/// (optionally) the reported tailnet host. Returns the tenant id, or None if unknown.
pub async fn touch_box_by_token(
    pool: &PgPool,
    token_hash: &str,
    host: Option<&str>,
    version: Option<&str>,
    tailscale: Option<&str>,
    ffmpeg: Option<bool>,
    gstreamer: Option<bool>,
    platform: Option<&str>,
    hwaccel: Option<&str>,
    metrics: Option<app_core::edge_metrics::EdgeMetrics>,
) -> sqlx::Result<Option<Uuid>> {
    // Resource metrics ride the same heartbeat as health; absent (pushes) → COALESCE keeps prior.
    let (cpu, mem, disk, gpu, gpu_mem) = match metrics {
        Some(m) => (
            Some(m.cpu as i32),
            Some(m.mem as i32),
            Some(m.disk as i32),
            m.gpu.map(|v| v as i32),
            m.gpu_mem.map(|v| v as i32),
        ),
        None => (None, None, None, None, None),
    };
    let row: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE tenant_boxes
            SET last_seen = now(),
                magicdns_host = COALESCE($2, magicdns_host),
                version = COALESCE($3, version),
                tailscale_state = COALESCE($4, tailscale_state),
                ffmpeg_ok = COALESCE($5, ffmpeg_ok),
                gstreamer_ok = COALESCE($6, gstreamer_ok),
                platform = COALESCE($7, platform),
                health_at = CASE WHEN $4 IS NOT NULL THEN now() ELSE health_at END,
                cpu_pct = COALESCE($8, cpu_pct),
                mem_pct = COALESCE($9, mem_pct),
                disk_pct = COALESCE($10, disk_pct),
                gpu_pct = COALESCE($11, gpu_pct),
                gpu_mem_pct = COALESCE($12, gpu_mem_pct),
                hwaccel = COALESCE($13, hwaccel),
                metrics_at = CASE WHEN $8 IS NOT NULL THEN now() ELSE metrics_at END
          WHERE token_hash = $1
        RETURNING tenant_id",
    )
    .bind(token_hash)
    .bind(host)
    .bind(version)
    .bind(tailscale)
    .bind(ffmpeg)
    .bind(gstreamer)
    .bind(platform)
    .bind(cpu)
    .bind(mem)
    .bind(disk)
    .bind(gpu)
    .bind(gpu_mem)
    .bind(hwaccel)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(t,)| t))
}

/// Like [`touch_box_by_token`] but also returns the box id — needed to tag pushed logs
/// to the right box. Returns `(tenant_id, box_id)`, or None for an unknown token.
pub async fn touch_box_identity(
    pool: &PgPool,
    token_hash: &str,
    host: Option<&str>,
    version: Option<&str>,
    tailscale: Option<&str>,
    ffmpeg: Option<bool>,
    gstreamer: Option<bool>,
    platform: Option<&str>,
    hwaccel: Option<&str>,
) -> sqlx::Result<Option<(Uuid, Uuid)>> {
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        "UPDATE tenant_boxes
            SET last_seen = now(),
                magicdns_host = COALESCE($2, magicdns_host),
                version = COALESCE($3, version),
                tailscale_state = COALESCE($4, tailscale_state),
                ffmpeg_ok = COALESCE($5, ffmpeg_ok),
                gstreamer_ok = COALESCE($6, gstreamer_ok),
                platform = COALESCE($7, platform),
                hwaccel = COALESCE($8, hwaccel),
                health_at = CASE WHEN $4 IS NOT NULL THEN now() ELSE health_at END
          WHERE token_hash = $1
        RETURNING tenant_id, id",
    )
    .bind(token_hash)
    .bind(host)
    .bind(version)
    .bind(tailscale)
    .bind(ffmpeg)
    .bind(gstreamer)
    .bind(platform)
    .bind(hwaccel)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// One log line pushed up by an edge box.
#[derive(serde::Deserialize)]
pub struct LogLineIn {
    pub ts: String,
    pub level: String,
    #[serde(default)]
    pub target: String,
    pub message: String,
    /// Structured fields of the event (card #146) — e.g. `{camera_ip, mode, fps}`. Empty for
    /// plaintext (`log::*`) lines. Control merges these into the Datadog record's searchable
    /// attributes and stamps the box identity (`edge_id`/`edge_name`/`tenant`) alongside.
    #[serde(default)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// Confirm a box id belongs to a tenant (guards the per-box log routes).
pub async fn box_belongs_to(pool: &PgPool, tenant_id: Uuid, box_id: Uuid) -> sqlx::Result<bool> {
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM tenant_boxes WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(box_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

/// Replace the tenant's discovered-camera set (the edge pushes its full current sweep).
pub async fn upsert_discovered(
    pool: &PgPool,
    tenant_id: Uuid,
    cams: &[app_core::onvif::DiscoveredDevice],
) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM discovered_cameras WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    for c in cams {
        sqlx::query(
            "INSERT INTO discovered_cameras (tenant_id, ip, name, xaddr, manufacturer, model, onvif_id)
             VALUES ($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (tenant_id, ip) DO UPDATE SET
               name = EXCLUDED.name, xaddr = EXCLUDED.xaddr, manufacturer = EXCLUDED.manufacturer,
               model = EXCLUDED.model, onvif_id = EXCLUDED.onvif_id, last_seen = now()",
        )
        .bind(tenant_id)
        .bind(&c.ip)
        .bind(&c.name)
        .bind(&c.xaddr)
        .bind(&c.manufacturer)
        .bind(&c.model)
        .bind(&c.id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

pub async fn list_discovered(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<DiscoveredRow>> {
    sqlx::query_as::<_, DiscoveredRow>(
        "SELECT ip, name, xaddr, manufacturer, model, onvif_id, last_seen
           FROM discovered_cameras WHERE tenant_id = $1 ORDER BY ip",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}

/// (ip, display name) per configured camera — the **user-given** name from `devices.name`, not the
/// raw ONVIF/model string in `discovered_cameras`. Everything user-facing (the recordings/playback
/// camera list) should read this. Falls back to the ip when a camera has no name yet.
pub async fn list_camera_names(
    pool: &PgPool,
    tenant_id: Uuid,
) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT ip, COALESCE(NULLIF(name, ''), ip) AS name FROM devices WHERE tenant_id = $1 ORDER BY name",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}

/// Mark a camera as configured (its credentials were relayed to the edge). The creds
/// themselves are never stored — only this flag.
pub async fn mark_configured(pool: &PgPool, tenant_id: Uuid, ip: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO configured_cameras (tenant_id, ip) VALUES ($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(tenant_id)
    .bind(ip)
    .execute(pool)
    .await
    .map(|_| ())
}

pub async fn unmark_configured(pool: &PgPool, tenant_id: Uuid, ip: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM configured_cameras WHERE tenant_id = $1 AND ip = $2")
        .bind(tenant_id)
        .bind(ip)
        .execute(pool)
        .await
        .map(|_| ())
}

pub async fn list_configured(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT ip FROM configured_cameras WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(ip,)| ip).collect())
}

/// Replace the tenant's "streaming" set — the (ip, codec) pairs the edge currently has a
/// resolved stream for (the edge pushes its full current set on each reload/rediscover).
/// `codec` is `None` when the edge hasn't probed it (older build, or ffmpeg/subprocess-gstreamer
/// backend). Best-effort.
pub async fn upsert_streaming(
    pool: &PgPool,
    tenant_id: Uuid,
    cams: &[(String, Option<String>)],
) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM streaming_cameras WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    for (ip, codec) in cams {
        sqlx::query(
            "INSERT INTO streaming_cameras (tenant_id, ip, codec) VALUES ($1,$2,$3) \
             ON CONFLICT (tenant_id, ip) DO UPDATE SET codec = EXCLUDED.codec",
        )
        .bind(tenant_id)
        .bind(ip)
        .bind(codec)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// (ip, codec) pairs the tenant's edge currently has a resolved stream for (the "online &
/// correct" set) — `codec` is `None` when the edge hasn't reported it.
pub async fn list_streaming(
    pool: &PgPool,
    tenant_id: Uuid,
) -> sqlx::Result<Vec<(String, Option<String>)>> {
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT ip, codec FROM streaming_cameras WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

/// The most-recently-seen box host for a tenant (the gateway's live upstream).
pub async fn box_host_for_tenant(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Option<String>> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT magicdns_host FROM tenant_boxes
          WHERE tenant_id = $1 AND magicdns_host IS NOT NULL
       ORDER BY last_seen DESC NULLS LAST LIMIT 1",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(h,)| h))
}

// ---- recording config (per box) --------------------------------------------

/// The recording_config jsonb for a specific box, resolved by its token hash.
pub async fn recording_config_by_token(
    pool: &PgPool,
    token_hash: &str,
) -> sqlx::Result<Option<serde_json::Value>> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT recording_config FROM tenant_boxes WHERE token_hash = $1")
            .bind(token_hash)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v))
}

pub async fn set_recording_config(
    pool: &PgPool,
    tenant_id: Uuid,
    box_id: Uuid,
    config: serde_json::Value,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE tenant_boxes SET recording_config = $1 WHERE tenant_id = $2 AND id = $3",
    )
    .bind(config)
    .bind(tenant_id)
    .bind(box_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

// ---- tracking config (per box, card: edge tracking P3+) --------------------

/// The tracking_config jsonb for a specific box, resolved by its token hash — mirrors
/// [`recording_config_by_token`].
pub async fn tracking_config_by_token(
    pool: &PgPool,
    token_hash: &str,
) -> sqlx::Result<Option<serde_json::Value>> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT tracking_config FROM tenant_boxes WHERE token_hash = $1")
            .bind(token_hash)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v))
}

/// The tracking_config jsonb for a specific box, resolved by its id — used by the graph-save
/// sync (`workflows::sync_tracking_config`) to read a box's current `enabled`/`detect_ms` before
/// overwriting only `trackers` with what the graphs now declare.
pub async fn tracking_config_by_id(
    pool: &PgPool,
    tenant_id: Uuid,
    box_id: Uuid,
) -> sqlx::Result<Option<serde_json::Value>> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT tracking_config FROM tenant_boxes WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(box_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v))
}

pub async fn set_tracking_config(
    pool: &PgPool,
    tenant_id: Uuid,
    box_id: Uuid,
    config: serde_json::Value,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE tenant_boxes SET tracking_config = $1 WHERE tenant_id = $2 AND id = $3",
    )
    .bind(config)
    .bind(tenant_id)
    .bind(box_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

// ---- persistent edge commands (survive control restarts) -------------------

/// Set a pending command flag for a box. Idempotent.
pub async fn set_box_command(
    pool: &PgPool,
    box_id: Uuid,
    field: &str,
    value: bool,
) -> sqlx::Result<()> {
    // field is a trusted internal string (never user input), so we construct the query
    // with the column name inline. The caller is responsible for passing a valid column.
    let sql = format!("UPDATE tenant_boxes SET {field} = $1 WHERE id = $2");
    sqlx::query(&sql)
        .bind(value)
        .bind(box_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Read and atomically clear both command flags for a box (called by the edge on each poll).
/// Returns (restart, update) reflecting what was set BEFORE clearing.
///
/// Postgres RETURNING gives post-update values (always false after we clear),
/// so we use SELECT FOR UPDATE + UPDATE inside a transaction to capture the old state.
pub async fn take_box_commands(pool: &PgPool, box_id: Uuid) -> sqlx::Result<(bool, bool)> {
    let mut tx = pool.begin().await?;
    let row: Option<(bool, bool)> = sqlx::query_as(
        "SELECT pending_restart, pending_update FROM tenant_boxes WHERE id = $1 FOR UPDATE",
    )
    .bind(box_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (restart, update) = row.unwrap_or((false, false));
    if restart || update {
        sqlx::query(
            "UPDATE tenant_boxes SET pending_restart = FALSE, pending_update = FALSE WHERE id = $1",
        )
        .bind(box_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok((restart, update))
}

// ---- recordings (clip metadata pushed by edge) -----------------------------

const FNAME_ADJ: &[&str] = &[
    "amber", "azure", "bold", "bright", "calm", "cedar", "clear", "cloud", "coral", "crisp",
    "dark", "dawn", "deep", "deft", "dense", "drift", "dusk", "early", "faint", "fast", "fern",
    "firm", "flash", "fleet", "frost", "gentle", "ghost", "glad", "glide", "gold", "grand", "grey",
    "gust", "hardy", "haze", "high", "hollow", "iron", "jade", "keen", "kind", "lake", "lean",
    "light", "lofty", "lone", "loud", "mist", "misty", "moss", "muted", "night", "noble", "north",
    "oak", "pale", "peak", "pine", "plain", "prime", "pure", "quick", "quiet", "rapid", "raw",
    "reef", "rich", "ridge", "ripple", "rise", "roam", "rocky", "rough", "sage", "sharp", "silver",
    "sleek", "slim", "slow", "smooth", "soft", "solid", "stark", "steady", "steel", "still",
    "stone", "storm", "strong", "swift", "tall", "thin", "tidal", "trim", "true", "vast", "veil",
    "warm", "wide", "wild",
];

const FNAME_NOUN: &[&str] = &[
    "arc", "ash", "bay", "beam", "birch", "blade", "blaze", "bloom", "boulder", "branch", "bridge",
    "brook", "canyon", "cedar", "cliff", "cloud", "comet", "cove", "creek", "crest", "dawn",
    "delta", "drift", "dune", "dusk", "echo", "ember", "falcon", "fern", "field", "fjord", "flame",
    "flare", "flash", "flint", "fog", "forest", "gale", "gate", "glacier", "glen", "grove", "gust",
    "harbor", "hawk", "haze", "hill", "hollow", "horizon", "isle", "jade", "lake", "ledge",
    "light", "loch", "mesa", "meteor", "mist", "moon", "moss", "mount", "nebula", "night", "north",
    "oak", "orbit", "pass", "peak", "pine", "plain", "pond", "pulse", "quarry", "rain", "range",
    "rapid", "reef", "ridge", "rift", "river", "rock", "root", "run", "sage", "salt", "sand",
    "shore", "sierra", "sky", "slate", "snow", "sol", "spark", "spire", "spring", "star", "stone",
    "storm", "stream", "summit", "sun", "surge", "tide", "timber", "torch", "trail", "vale",
    "vault", "wave", "wind",
];

pub fn generate_friendly_name(epoch: i64) -> String {
    let e = epoch.unsigned_abs() as usize;
    let na = FNAME_ADJ.len();
    let nn = FNAME_NOUN.len();
    format!(
        "{}-{}-{}",
        FNAME_ADJ[e % na],
        FNAME_ADJ[e.wrapping_mul(31) % na],
        FNAME_NOUN[e.wrapping_mul(97) % nn],
    )
}

/// Backfill friendly names for recordings that were created before the name generator
/// was deployed. Runs at control startup — cheap (one SELECT + N UPDATEs, once).
pub async fn backfill_friendly_names(pool: &PgPool) -> sqlx::Result<u64> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, started_at FROM recordings WHERE friendly_name = ''")
            .fetch_all(pool)
            .await?;
    let count = rows.len() as u64;
    for (id, started_at) in rows {
        let epoch = chrono::DateTime::parse_from_rfc3339(&started_at)
            .map(|t| t.timestamp())
            .unwrap_or(0);
        let name = generate_friendly_name(epoch);
        sqlx::query("UPDATE recordings SET friendly_name = $1 WHERE id = $2")
            .bind(&name)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if count > 0 {
        log::info!("backfill_friendly_names: updated {count} recording(s)");
    }
    Ok(count)
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct RecordingRow {
    pub id: i64,
    pub camera_ip: String,
    pub clip_id: String,
    pub friendly_name: String,
    pub duration_secs: i32,
    pub size_bytes: i64,
    pub started_at: String,
    pub created_at: String,
    /// Where this clip lives: `pending` (edge only), `uploading` (in transit), `uploaded`
    /// (durable in R2), `failed`. Drives the badge and whether cloud playback is offered.
    pub upload_state: String,
    // Enriched at query time — not DB columns.
    #[sqlx(default)]
    pub camera_name: Option<String>,
    /// True while a delete request for this clip is still working through the queue. Set in the
    /// list handler from `work_queue::open_recording_ids`, not stored.
    #[sqlx(default)]
    pub deleting: bool,
}

/// Persist clip metadata only — the video file stays on the edge and is proxied on download.
/// Retention days the tenant's current plan sells. `0` when it has no plan, a plan with no
/// cloud recording, or a subscription that no longer entitles them to one — in which case no
/// clip is uploaded.
///
/// The status filter matters: an expired trial or an unpaid tenant is already read-only, and
/// letting their edge keep filling the bucket would have us pay to store footage for someone
/// we've stopped serving. A lapsed trial still stored as `trialing` is excluded the same way
/// `subscriptions::current` resolves it — at read time, not by trusting the sweep.
pub async fn tenant_retention_days(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<i32> {
    let days: Option<i32> = sqlx::query_scalar(
        "SELECT p.retention_days
         FROM tenant_subscriptions s
         JOIN billing_plans p ON p.id = s.plan_id
         WHERE s.tenant_id = $1
           AND s.status IN ('trialing', 'active')
           AND NOT (s.status = 'trialing' AND s.ended_at IS NOT NULL
                    AND s.ended_at::timestamptz < now())
         ORDER BY s.started_at DESC
         LIMIT 1",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(days.unwrap_or(0))
}

/// Claim the clip for the cloud: record where it will live and that it is on its way.
pub async fn set_clip_uploading(pool: &PgPool, id: i64, cloud_key: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE recordings SET cloud_key = $2, upload_state = 'uploading' WHERE id = $1")
        .bind(id)
        .bind(cloud_key)
        .execute(pool)
        .await
        .map(|_| ())
}

/// The edge confirmed the object is in the bucket. Scoped by tenant: `clip_id` comes from a
/// box, and a box must not be able to mark another tenant's clip durable.
pub async fn mark_clip_uploaded(
    pool: &PgPool,
    tenant_id: Uuid,
    clip_id: &str,
) -> sqlx::Result<u64> {
    let now = app_core::timeutil::now_rfc3339();
    let res = sqlx::query(
        "UPDATE recordings
         SET upload_state = 'uploaded', cloud_uploaded_at = $3
         WHERE tenant_id = $1 AND clip_id = $2 AND cloud_key IS NOT NULL",
    )
    .bind(tenant_id)
    .bind(clip_id)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn insert_recording(
    pool: &PgPool,
    tenant_id: Uuid,
    camera_ip: &str,
    clip_id: &str,
    friendly_name: &str,
    duration_secs: i32,
    size_bytes: i64,
    started_at: &str,
) -> sqlx::Result<i64> {
    let effective_name = if friendly_name.is_empty() {
        let epoch = chrono::DateTime::parse_from_rfc3339(started_at)
            .map(|t| t.timestamp())
            .unwrap_or_else(|_| chrono::Utc::now().timestamp());
        generate_friendly_name(epoch)
    } else {
        friendly_name.to_string()
    };
    let now = app_core::timeutil::now_rfc3339();
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO recordings (tenant_id, camera_ip, clip_id, friendly_name, duration_secs, size_bytes, started_at, created_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
    )
    .bind(tenant_id)
    .bind(camera_ip)
    .bind(clip_id)
    .bind(&effective_name)
    .bind(duration_secs)
    .bind(size_bytes)
    .bind(started_at)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub struct RecordingPage {
    pub total: i64,
    pub items: Vec<RecordingRow>,
}

/// List recordings with optional filter, sort, and pagination.
/// All sort column names are validated against an allowlist before interpolation.
pub async fn list_recordings_filtered(
    pool: &PgPool,
    tenant_id: Uuid,
    camera_ip: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    sort: &str,
    dir: &str,
    limit: i64,
    offset: i64,
) -> sqlx::Result<RecordingPage> {
    let col = match sort {
        "duration_secs" => "r.duration_secs",
        "size_bytes" => "r.size_bytes",
        "friendly_name" => "r.friendly_name",
        "camera_ip" => "r.camera_ip",
        _ => "r.started_at",
    };
    let ord = if dir == "asc" { "ASC" } else { "DESC" };

    // Use a fixed-width filter that sets every optional to a sentinel when unused.
    // $2/$3/$4 are optional filters: NULL means "skip this filter".
    let sql = format!(
        "SELECT r.id, r.camera_ip, r.clip_id, r.friendly_name, r.duration_secs, r.size_bytes,
                r.started_at, r.created_at, r.upload_state,
                dc.name AS camera_name
           FROM recordings r
           LEFT JOIN devices dc
                  ON dc.tenant_id = r.tenant_id AND dc.ip = r.camera_ip
          WHERE r.tenant_id = $1
            AND ($2::text IS NULL OR r.camera_ip = $2)
            AND ($3::text IS NULL OR r.started_at::timestamptz >= $3::timestamptz)
            AND ($4::text IS NULL OR r.started_at::timestamptz <= $4::timestamptz)
          ORDER BY {col} {ord}
          LIMIT $5 OFFSET $6"
    );
    let count_sql = "SELECT COUNT(*) FROM recordings r
          WHERE r.tenant_id = $1
            AND ($2::text IS NULL OR r.camera_ip = $2)
            AND ($3::text IS NULL OR r.started_at::timestamptz >= $3::timestamptz)
            AND ($4::text IS NULL OR r.started_at::timestamptz <= $4::timestamptz)";

    let (total,): (i64,) = sqlx::query_as(count_sql)
        .bind(tenant_id)
        .bind(camera_ip)
        .bind(from)
        .bind(to)
        .fetch_one(pool)
        .await?;

    let items = sqlx::query_as::<_, RecordingRow>(&sql)
        .bind(tenant_id)
        .bind(camera_ip)
        .bind(from)
        .bind(to)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

    Ok(RecordingPage { total, items })
}

pub async fn get_recording(
    pool: &PgPool,
    tenant_id: Uuid,
    id: i64,
) -> sqlx::Result<Option<RecordingRow>> {
    sqlx::query_as::<_, RecordingRow>(
        "SELECT id, camera_ip, clip_id, friendly_name, duration_secs, size_bytes, started_at, created_at, upload_state, NULL::text AS camera_name
           FROM recordings WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id).bind(id)
    .fetch_optional(pool).await
}

/// What enqueuing a delete needs: the edge-local `clip_id` and the cloud object key (if any), so
/// each work-queue job can carry its own target in its payload. `cloud_key` is returned in **any**
/// upload state — a `failed` or mid-`uploading` clip can have left an object in the bucket, and
/// the delete must reach it too, not only `uploaded` ones.
pub async fn recording_for_delete(
    pool: &PgPool,
    tenant_id: Uuid,
    id: i64,
) -> sqlx::Result<Option<(String, Option<String>)>> {
    sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT clip_id, cloud_key FROM recordings WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// After the objects are gone from R2, detach the rows from the cloud: clear the key and drop
/// the state back to `pending` (edge-only). The row survives — the clip may still be on the box —
/// but nothing claims it's durable. Returns how many rows changed.
pub async fn clear_tenant_cloud_keys(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<u64> {
    let res = sqlx::query(
        "UPDATE recordings
         SET cloud_key = NULL, cloud_uploaded_at = NULL, upload_state = 'pending'
         WHERE tenant_id = $1 AND cloud_key IS NOT NULL",
    )
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// The R2 object key for a durable clip, scoped by tenant. `Some(None)` means the clip exists
/// but isn't in the cloud (still local / in transit / failed); `None` means no such clip for
/// this tenant. `cloud_key` never leaves the server — it's the input to a presigned URL, not
/// something the browser is handed.
pub async fn recording_cloud_key(
    pool: &PgPool,
    tenant_id: Uuid,
    id: i64,
) -> sqlx::Result<Option<Option<String>>> {
    let row: Option<(Option<String>, String)> = sqlx::query_as(
        "SELECT cloud_key, upload_state FROM recordings WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    // Only an 'uploaded' clip is durable and streamable; a key present mid-upload isn't there yet.
    Ok(row.map(|(key, state)| if state == "uploaded" { key } else { None }))
}

pub async fn delete_recording(pool: &PgPool, tenant_id: Uuid, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM recordings WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

// ---- tenants ----------------------------------------------------------------

pub async fn list_tenants(pool: &PgPool) -> sqlx::Result<Vec<TenantRow>> {
    sqlx::query_as::<_, TenantRow>("SELECT id, name, slug, status FROM tenants ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn find_tenant(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<TenantRow>> {
    sqlx::query_as::<_, TenantRow>("SELECT id, name, slug, status FROM tenants WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn tenant_slug_taken(pool: &PgPool, slug: &str) -> sqlx::Result<bool> {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM tenants WHERE slug = $1")
        .bind(slug)
        .fetch_one(pool)
        .await?;
    Ok(n > 0)
}

/// The tenant id for a slug, if it exists. Used by the template seeder to resolve the
/// stable `platform` tenant that owns the shared (global) workflow templates.
pub async fn tenant_id_by_slug(pool: &PgPool, slug: &str) -> sqlx::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM tenants WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id,)| id))
}

// ---- roles + permissions ----------------------------------------------------

pub async fn list_permissions(pool: &PgPool) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as("SELECT key, description FROM permissions ORDER BY key")
        .fetch_all(pool)
        .await
}

pub async fn upsert_permission(pool: &PgPool, key: &str, desc: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO permissions (key, description) VALUES ($1,$2)
         ON CONFLICT (key) DO UPDATE SET description = EXCLUDED.description",
    )
    .bind(key)
    .bind(desc)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_roles(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<RoleRow>> {
    sqlx::query_as::<_, RoleRow>(
        "SELECT id, name, is_system FROM roles WHERE tenant_id = $1 ORDER BY name",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}

pub async fn create_role(
    pool: &PgPool,
    tenant_id: Uuid,
    name: &str,
    is_system: bool,
) -> sqlx::Result<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO roles (tenant_id, name, is_system) VALUES ($1,$2,$3) RETURNING id",
    )
    .bind(tenant_id)
    .bind(name)
    .bind(is_system)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Give tenants that have no roles at all the standard system pair: `Admin` (every action)
/// and `Viewer` (read-only).
///
/// The `Default` tenant predates `provision_tenant_with_admin` — migration 0011 backfilled it
/// — so it never got roles. Anyone invited into such a tenant lands with zero permissions,
/// and the effect is invisible while you sign in as master (master bypasses RBAC).
///
/// Roles are created, never **assigned**: handing someone admin is not a decision a boot
/// sequence should make. Tenants that already have roles are untouched.
pub async fn ensure_tenant_roles(pool: &PgPool) -> sqlx::Result<usize> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT t.id FROM tenants t
         WHERE NOT EXISTS (SELECT 1 FROM roles r WHERE r.tenant_id = t.id)",
    )
    .fetch_all(pool)
    .await?;

    for (tenant_id,) in &rows {
        let admin = create_role(pool, *tenant_id, "Admin", true).await?;
        let admin_keys: Vec<String> = crate::rbac::all_perms()
            .into_iter()
            .map(String::from)
            .collect();
        set_role_permissions(pool, *tenant_id, admin, &admin_keys).await?;

        let viewer = create_role(pool, *tenant_id, "Viewer", true).await?;
        let viewer_keys: Vec<String> = crate::rbac::viewer_perms()
            .into_iter()
            .map(String::from)
            .collect();
        set_role_permissions(pool, *tenant_id, viewer, &viewer_keys).await?;
    }
    Ok(rows.len())
}

/// Keep the two **system** roles in step with the permission catalog: `Admin` gets every
/// action, `Viewer` gets every read-only one (`*.read`).
///
/// Role grants are seeded only at tenant creation, so a permission added later (e.g.
/// `billing.read`) reaches nobody — and the screen that would grant it sits behind
/// `roles.manage`. A system role has fixed semantics; customise access with a **custom** role,
/// which this never touches. Returns how many grants were added.
pub async fn sync_system_roles(
    pool: &PgPool,
    viewer_keys: &[&str],
    owner_only_keys: &[&str],
) -> sqlx::Result<u64> {
    // Admin gets every action EXCEPT the owner-only ones. Without the exclusion this runs on every
    // boot, so deleting such a grant by hand would be undone the next time control started.
    let excluded: Vec<String> = owner_only_keys.iter().map(|k| (*k).to_string()).collect();
    let admin = sqlx::query(
        "INSERT INTO role_permissions (role_id, permission_key)
         SELECT r.id, p.key
         FROM roles r CROSS JOIN permissions p
         WHERE r.is_system AND r.name = 'Admin'
           AND p.key <> ALL($1)
         ON CONFLICT DO NOTHING",
    )
    .bind(&excluded)
    .execute(pool)
    .await?;

    let keys: Vec<String> = viewer_keys.iter().map(|k| (*k).to_string()).collect();
    let viewer = sqlx::query(
        "INSERT INTO role_permissions (role_id, permission_key)
         SELECT r.id, k
         FROM roles r CROSS JOIN UNNEST($1::text[]) AS k
         WHERE r.is_system AND r.name = 'Viewer'
         ON CONFLICT DO NOTHING",
    )
    .bind(&keys)
    .execute(pool)
    .await?;

    Ok(admin.rows_affected() + viewer.rows_affected())
}

/// Whether `role_id` belongs to `tenant_id`./// Whether `role_id` belongs to `tenant_id`. Every role-permission read/write goes through
/// this: the role id is a client-supplied UUID, and `role_permissions` carries no tenant of
/// its own, so without the join a tenant admin could read or rewrite another tenant's roles.
pub async fn role_in_tenant(pool: &PgPool, tenant_id: Uuid, role_id: Uuid) -> sqlx::Result<bool> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM roles WHERE id = $1 AND tenant_id = $2")
            .bind(role_id)
            .bind(tenant_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

/// A role's granted permission keys. Tenant-scoped via a join on `roles`.
pub async fn role_perm_keys(
    pool: &PgPool,
    tenant_id: Uuid,
    role_id: Uuid,
) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT rp.permission_key FROM role_permissions rp
         JOIN roles r ON r.id = rp.role_id
         WHERE rp.role_id = $1 AND r.tenant_id = $2
         ORDER BY rp.permission_key",
    )
    .bind(role_id)
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(k,)| k).collect())
}

/// Replace a role's permission set (the matrix row). The `DELETE` is scoped by the same
/// tenant join, so a mismatched role id removes nothing instead of wiping a stranger's role.
pub async fn set_role_permissions(
    pool: &PgPool,
    tenant_id: Uuid,
    role_id: Uuid,
    keys: &[String],
) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM role_permissions rp
         USING roles r
         WHERE rp.role_id = r.id AND rp.role_id = $1 AND r.tenant_id = $2",
    )
    .bind(role_id)
    .bind(tenant_id)
    .execute(&mut *tx)
    .await?;
    for k in keys {
        sqlx::query(
            "INSERT INTO role_permissions (role_id, permission_key)
             SELECT $1, $2 FROM roles WHERE id = $1 AND tenant_id = $3
             ON CONFLICT DO NOTHING",
        )
        .bind(role_id)
        .bind(k)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

// ---- oauth ------------------------------------------------------------------

pub async fn find_oauth_user(
    pool: &PgPool,
    provider: &str,
    uid: &str,
) -> sqlx::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM oauth_identities WHERE provider = $1 AND provider_uid = $2",
    )
    .bind(provider)
    .bind(uid)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
}

pub async fn link_oauth(
    pool: &PgPool,
    provider: &str,
    uid: &str,
    user_id: Uuid,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO oauth_identities (provider, provider_uid, user_id) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(provider)
        .bind(uid)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- provisioning -----------------------------------------------------------

/// Create a tenant with its seeded Admin + Viewer roles (cloned from the templates),
/// create the first user, and make them the tenant Admin. Returns (tenant_id, user_id,
/// effective_perms). `password_hash` is None for social signups.
pub async fn provision_tenant_with_admin(
    pool: &PgPool,
    tenant_name: &str,
    slug: &str,
    admin_email: &str,
    password_hash: Option<&str>,
) -> sqlx::Result<(Uuid, Uuid, Vec<String>)> {
    let (tenant_id,): (Uuid,) =
        sqlx::query_as("INSERT INTO tenants (name, slug) VALUES ($1,$2) RETURNING id")
            .bind(tenant_name)
            .bind(slug)
            .fetch_one(pool)
            .await?;

    let admin_role = create_role(pool, tenant_id, "Admin", true).await?;
    let admin_keys: Vec<String> = crate::rbac::all_perms()
        .into_iter()
        .map(String::from)
        .collect();
    set_role_permissions(pool, tenant_id, admin_role, &admin_keys).await?;

    let viewer_role = create_role(pool, tenant_id, "Viewer", true).await?;
    let viewer_keys: Vec<String> = crate::rbac::viewer_perms()
        .into_iter()
        .map(String::from)
        .collect();
    set_role_permissions(pool, tenant_id, viewer_role, &viewer_keys).await?;

    let user_id = create_user(pool, Some(tenant_id), admin_email, password_hash, false).await?;
    grant_role(pool, user_id, admin_role).await?;

    // The account that creates a tenant owns it (migration 0081). Set here rather than in a
    // caller so every provisioning path gets it — an ownerless tenant is a support ticket.
    let _ = set_tenant_owner(pool, tenant_id, user_id).await;

    Ok((tenant_id, user_id, admin_keys))
}

// ---- box pairing (card #81) -------------------------------------------------
// A box learns who it belongs to by someone typing a short code, instead of by four secrets pasted
// onto a command line — which is what makes a double-click installer possible at all.
//
// Nothing secret is stored: the row holds only a HASH of the code. Redeeming mints a fresh box
// token and a fresh Tailscale key on the spot (see `domain::boxes`), so a dump of `box_pairings`
// yields nothing usable.

/// Mint a pairing code for a box, superseding any unclaimed one it already has (a user who clicks
/// "new code" expects the old one to stop working).
pub async fn create_pairing(
    pool: &PgPool,
    tenant_id: Uuid,
    box_id: Uuid,
    code_hash: &str,
    expires_at: &str,
) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM box_pairings WHERE box_id = $1 AND claimed_at IS NULL")
        .bind(box_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO box_pairings (code_hash, tenant_id, box_id, expires_at, created_at) \
         VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(code_hash)
    .bind(tenant_id)
    .bind(box_id)
    .bind(expires_at)
    .bind(app_core::timeutil::now_rfc3339())
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Spend a pairing code, atomically. Returns `(tenant_id, box_id)` for the winner and `None` for
/// everything else — wrong, expired, or already claimed. **One statement**: two racing redeems
/// cannot both succeed, because only one `UPDATE` can see `claimed_at IS NULL`.
pub async fn claim_pairing(pool: &PgPool, code_hash: &str) -> sqlx::Result<Option<(Uuid, Uuid)>> {
    let now = app_core::timeutil::now_rfc3339();
    sqlx::query_as::<_, (Uuid, Uuid)>(
        "UPDATE box_pairings SET claimed_at = $1 \
         WHERE code_hash = $2 AND claimed_at IS NULL AND expires_at > $1 \
         RETURNING tenant_id, box_id",
    )
    .bind(&now)
    .bind(code_hash)
    .fetch_optional(pool)
    .await
}

/// Point a box at a new token. The plaintext never lands in the DB — only its hash, exactly as at
/// box creation.
pub async fn rotate_box_token(pool: &PgPool, box_id: Uuid, token_hash: &str) -> sqlx::Result<bool> {
    let n = sqlx::query("UPDATE tenant_boxes SET token_hash = $1 WHERE id = $2")
        .bind(token_hash)
        .bind(box_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// A box's display name — the Tailscale key description wants it.
pub async fn box_name(pool: &PgPool, box_id: Uuid) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT name FROM tenant_boxes WHERE id = $1")
        .bind(box_id)
        .fetch_optional(pool)
        .await
}

/// The two signals the install view needs to show pairing progress (card #85): whether the box's
/// most recent pairing code was **claimed** (redeemed by the appliance), and whether the box is
/// **online** (has checked in *since that code was minted* — an old `last_seen` from a previous
/// install must not count as this install succeeding). One row per box, or `None` if the box isn't
/// this tenant's.
///
/// `created_at` is RFC3339 text (0055); casting it to `timestamptz` normalizes its offset to UTC so
/// the comparison against `last_seen` (a `now()` timestamptz) is correct.
pub async fn pairing_status(
    pool: &PgPool,
    tenant_id: Uuid,
    box_id: Uuid,
) -> sqlx::Result<Option<(bool, bool)>> {
    sqlx::query_as::<_, (bool, bool)>(
        "SELECT \
           (lp.claimed_at IS NOT NULL) AS claimed, \
           (b.last_seen IS NOT NULL AND (lp.created_at IS NULL OR b.last_seen > lp.created_at::timestamptz)) AS online \
         FROM tenant_boxes b \
         LEFT JOIN LATERAL ( \
           SELECT claimed_at, created_at FROM box_pairings p \
           WHERE p.box_id = b.id ORDER BY p.created_at DESC LIMIT 1 \
         ) lp ON true \
         WHERE b.id = $1 AND b.tenant_id = $2",
    )
    .bind(box_id)
    .bind(tenant_id)
    .fetch_optional(pool)
    .await
}

// ---- login throttling -------------------------------------------------------

/// Record a failed sign-in and lock the account once the attempts justify it.
///
/// A failure here is logged rather than discarded. If this UPDATE does not land, the attempt is
/// never counted, the lockout never engages, and the throttle is simply absent — fail-open with no
/// trace, which is the exact shape of bug the lockout was added to remove. `log::error!` because it
/// means a security control is silently off, not that one request went wrong.
///
/// Still returns `()`: refusing the sign-in is the caller's job and happens regardless, and a
/// bookkeeping failure must not turn a wrong password into a 500 — that would answer "was this
/// password right?" differently depending on whether the write succeeded.
pub async fn note_failed_login(pool: &PgPool, user_id: Uuid, lock_until: Option<String>) {
    let r = sqlx::query(
        "UPDATE users SET failed_logins = failed_logins + 1, locked_until = COALESCE($2, locked_until) WHERE id = $1",
    )
    .bind(user_id)
    .bind(lock_until)
    .execute(pool)
    .await;
    match r {
        Err(e) => log::error!("note_failed_login: attempt for user {user_id} NOT counted: {e}"),
        // No row updated means the id did not match. The caller just read this user, so this is a
        // deleted-mid-request race rather than a bug — but it still means the attempt went
        // uncounted, which is the thing worth knowing.
        Ok(res) if res.rows_affected() == 0 => {
            log::warn!("note_failed_login: user {user_id} vanished — attempt not counted")
        }
        Ok(_) => {}
    }
}

/// Clear the counter after a successful sign-in.
pub async fn clear_failed_logins(pool: &PgPool, user_id: Uuid) {
    let _ = sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
}

/// The user's session revocation cutoff (unix seconds), or `None` when nothing is revoked.
///
/// Errors are propagated, NOT swallowed: `introspect` fails closed on a lookup failure, because a
/// database blip must never silently un-revoke a session someone deliberately killed.
pub async fn sessions_valid_from(pool: &PgPool, user_id: Uuid) -> sqlx::Result<SessionGate> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT sessions_valid_from FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(match row {
        None => SessionGate::NoSuchUser,
        Some((cutoff,)) => SessionGate::Cutoff(cutoff),
    })
}

/// What the revocation lookup found.
///
/// The two cases MUST stay distinct. Collapsing them to `Option<i64>` — as this first did — makes a
/// DELETED user indistinguishable from a user with no revocation cutoff, so a removed member's token
/// reads as perfectly valid until it expires. Removing someone has to end their access now, not in
/// up to 24 hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionGate {
    /// No such user — the account was deleted. Every token naming it is dead.
    NoSuchUser,
    /// The user exists; `Some(t)` is their revocation cutoff, `None` means nothing revoked.
    Cutoff(Option<i64>),
}

// ---- tenant owner + password reset (migration 0081) -------------------------

/// The tenant's owner, or `None` for an ownerless tenant.
pub async fn tenant_owner(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Option<Uuid>> {
    let row: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT owner_user_id FROM tenants WHERE id = $1")
            .bind(tenant_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|r| r.0))
}

/// Record the tenant's owner. Used at provisioning; the master console can reassign later.
pub async fn set_tenant_owner(pool: &PgPool, tenant_id: Uuid, user_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE tenants SET owner_user_id = $2 WHERE id = $1")
        .bind(tenant_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set a user's password, clear any lockout, and **revoke their existing sessions** — in ONE
/// statement so the three cannot diverge.
///
/// Revocation is the part that is easy to leave out and dangerous to omit: a JWT already issued
/// stays valid for its full 24h, so without bumping `sessions_valid_from` (migration 0080) a
/// password reset would not evict whoever prompted the reset. Resetting the password of an account
/// you believe is compromised has to end that account's live sessions, or it accomplishes nothing.
///
/// Clearing `failed_logins`/`locked_until` too: an owner resetting a password for a locked-out user
/// means to restore access, and leaving the lockout in place would silently defeat that.
///
/// Scoped by tenant in the WHERE clause, not just by the caller's earlier check — a cross-tenant id
/// must not be writable even if a future caller forgets to look.
pub async fn set_password_and_revoke(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    password_hash: &str,
    now_unix: i64,
) -> sqlx::Result<u64> {
    let r = sqlx::query(
        "UPDATE users
            SET password_hash = $3,
                failed_logins = 0,
                locked_until = NULL,
                sessions_valid_from = $4
          WHERE id = $2 AND tenant_id = $1 AND is_master = false",
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(password_hash)
    .bind(now_unix)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

/// Delete a tenant member outright.
///
/// Safe as a hard delete because every FK into `users` is declared for it: `user_roles`,
/// `user_credentials` (passkeys) and `oauth_identities` CASCADE, `push_devices` SET NULLs. Crucially
/// `events.user_id` carries NO foreign key, so the audit trail SURVIVES the deletion — the record of
/// what this person did stays readable after they are gone, which is the whole reason not to reach
/// for a cascade there.
///
/// Guarded in SQL as well as by the caller: tenant-scoped, never a master, and never the tenant's
/// own owner. The owner clause is belt-and-braces behind the handler check — an owner deleted here
/// would blank `tenants.owner_user_id` (ON DELETE SET NULL) and leave the tenant unrecoverable.
pub async fn remove_tenant_user(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<u64> {
    let r = sqlx::query(
        "DELETE FROM users u
          WHERE u.id = $2
            AND u.tenant_id = $1
            AND u.is_master = false
            AND NOT EXISTS (SELECT 1 FROM tenants t WHERE t.id = $1 AND t.owner_user_id = u.id)",
    )
    .bind(tenant_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}
