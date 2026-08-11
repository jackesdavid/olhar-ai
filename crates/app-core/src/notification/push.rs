//! Mobile push — the Recipient delivery channel that runs PARALLEL to Telegram (card #139).
//!
//! Phase 1 sends through **Expo's push service** (one HTTP endpoint fronting both APNs and FCM),
//! per the plan's open decision #1: no APNs certificate juggling, no per-platform branch. Tokens
//! are the `ExponentPushToken[…]` strings the app reports after `expo-notifications` grants
//! permission; they live in `push_devices` (migration `0070`), keyed per tenant.
//!
//! Deliberately NOT wired through the Telegram-shaped [`crate::ports::Notifier`] trait — the plan
//! calls for a parallel dispatch path so the existing Telegram flow stays untouched, and a trait
//! unification only if it later pays off.
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// Expo's send endpoint. An access token is optional unless the project turns on enhanced
/// security, so Phase 1 needs no operator-level credential to deliver.
const EXPO_SEND_URL: &str = "https://exp.host/--/api/v2/push/send";

/// One registered phone. `token` is write-mostly (only dispatch needs it), so the API layer
/// returns the rest and leaves the token out of list responses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PushDevice {
    pub id: Uuid,
    pub label: String,
    pub platform: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_seen: Option<String>,
}

/// Register (or refresh) a device token for a tenant. Re-registering the same token — app
/// reinstall, token rotation — updates the existing row instead of fanning out duplicate pushes
/// to one phone (`push_devices_tenant_token_uniq`). Returns the row id.
pub fn register(
    db: &Db,
    tenant_id: Uuid,
    user_id: Option<Uuid>,
    device_token: &str,
    platform: &str,
    label: &str,
) -> Result<Uuid, String> {
    let pool = db.pool();
    let (token, platform, label) = (
        device_token.to_string(),
        platform.to_string(),
        label.to_string(),
    );
    let now = now_rfc3339();
    db.block(async move {
        sqlx::query_as::<_, (Uuid,)>(
            "INSERT INTO push_devices (tenant_id, user_id, device_token, platform, label, created_at, last_seen)
             VALUES ($1,$2,$3,$4,$5,$6,$6)
             ON CONFLICT (tenant_id, device_token) DO UPDATE
               SET platform = EXCLUDED.platform,
                   label     = EXCLUDED.label,
                   user_id   = EXCLUDED.user_id,
                   enabled   = true,
                   last_seen = EXCLUDED.last_seen
             RETURNING id",
        )
        .bind(tenant_id)
        .bind(user_id)
        .bind(&token)
        .bind(&platform)
        .bind(&label)
        .bind(&now)
        .fetch_one(&pool)
        .await
        .map(|(id,)| id)
        .map_err(|e| format!("push::register: {e}"))
    })
}

/// The tenant's registered devices (no tokens), newest first.
pub fn list(db: &Db, tenant_id: Uuid) -> Vec<PushDevice> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, (Uuid, String, String, bool, String, Option<String>)>(
            "SELECT id, label, platform, enabled, created_at, last_seen
               FROM push_devices WHERE tenant_id = $1 ORDER BY created_at DESC",
        )
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(
                    |(id, label, platform, enabled, created_at, last_seen)| PushDevice {
                        id,
                        label,
                        platform,
                        enabled,
                        created_at,
                        last_seen,
                    },
                )
                .collect()
        })
        .unwrap_or_else(|e| {
            log::error!("push::list query failed: {e}");
            Vec::new()
        })
    })
}

pub fn delete(db: &Db, tenant_id: Uuid, id: Uuid) -> bool {
    let pool = db.pool();
    db.block(async move {
        sqlx::query("DELETE FROM push_devices WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(id)
            .execute(&pool)
            .await
            .map(|r| r.rows_affected() > 0)
            .unwrap_or(false)
    })
}

/// Every enabled device token for a tenant — the dispatch fan-out set. When `label` is given,
/// only devices with that label (addressing one phone, mirroring how a Telegram send can target
/// one beacon); otherwise every enabled device.
pub fn enabled_tokens(db: &Db, tenant_id: Uuid, label: Option<&str>) -> Vec<String> {
    let pool = db.pool();
    let label = label.map(str::to_string);
    db.block(async move {
        let rows = match &label {
            Some(l) => {
                sqlx::query_as::<_, (String,)>(
                    "SELECT device_token FROM push_devices
                      WHERE tenant_id = $1 AND enabled AND label = $2",
                )
                .bind(tenant_id)
                .bind(l)
                .fetch_all(&pool)
                .await
            }
            None => {
                sqlx::query_as::<_, (String,)>(
                    "SELECT device_token FROM push_devices WHERE tenant_id = $1 AND enabled",
                )
                .bind(tenant_id)
                .fetch_all(&pool)
                .await
            }
        };
        rows.map(|r| r.into_iter().map(|(t,)| t).collect())
            .unwrap_or_else(|e| {
                log::error!("push::enabled_tokens query failed: {e}");
                Vec::new()
            })
    })
}

/// The Android channel a Device Gate request arrives on. Created app-side at MAX importance with
/// its own sound; naming it here keeps the two halves from drifting apart silently.
pub const GATE_CHANNEL_ID: &str = "device-gate";

/// Deliver an urgent, machine-readable notification — used by the Device Gate (#176).
///
/// Two things this does that [`send`] does not, and both matter:
///
/// 1. **`data` carries the payload, `body` carries only what a person should read.** The request's
///    token travels in `data`, which the app reads programmatically and the OS never renders.
///    Putting it in `body` would print a gate-opening secret on the lock screen.
/// 2. **It asks for the most interruptive level each platform actually allows.** iOS
///    `timeSensitive` — NOT `critical`, which needs an Apple entitlement restricted to health and
///    public-safety apps and is not obtainable for this one (map decision 05). Android rides a
///    MAX-importance channel. Neither is guaranteed to pierce Do Not Disturb, which is exactly why
///    the app also fetches pending requests on launch rather than trusting the push.
pub fn send_data(
    tokens: &[String],
    title: &str,
    message: &str,
    data: serde_json::Value,
) -> Result<usize, String> {
    if tokens.is_empty() {
        return Err("no registered push devices".into());
    }
    let body: Vec<serde_json::Value> = tokens
        .iter()
        .map(|t| {
            serde_json::json!({
                "to": t,
                "title": title,
                "body": message,
                "sound": "default",
                "data": data,
                "priority": "high",
                "interruptionLevel": "timeSensitive",
                "channelId": GATE_CHANNEL_ID,
            })
        })
        .collect();
    post_expo(body, tokens.len())
}

/// Deliver `message` to every token. Expo accepts a batch in one POST; a per-message error comes
/// back in the response body rather than as an HTTP status, so we surface the first one. Returns
/// how many messages Expo accepted.
pub fn send(tokens: &[String], title: &str, message: &str) -> Result<usize, String> {
    if tokens.is_empty() {
        return Err("no registered push devices".into());
    }
    let body: Vec<serde_json::Value> = tokens
        .iter()
        .map(
            |t| serde_json::json!({ "to": t, "title": title, "body": message, "sound": "default" }),
        )
        .collect();
    post_expo(body, tokens.len())
}

/// The shared POST + per-message error surfacing. Expo returns a 200 with per-item statuses, so a
/// rejected message is invisible to anyone checking only the HTTP code.
fn post_expo(body: Vec<serde_json::Value>, sent: usize) -> Result<usize, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build();
    let resp = agent
        .post(EXPO_SEND_URL)
        .set("accept", "application/json")
        .send_json(serde_json::json!(body))
        .map_err(|e| format!("expo push failed: {e}"))?;
    let parsed: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("expo push: bad response: {e}"))?;
    // { "data": [ { "status": "ok" | "error", "message": … }, … ] }
    let items = parsed
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    if let Some(err) = items
        .iter()
        .find(|i| i.get("status").and_then(|s| s.as_str()) == Some("error"))
    {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        return Err(format!("expo push rejected: {msg}"));
    }
    Ok(items.len().max(sent))
}
