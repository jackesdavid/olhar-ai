//! Per-tenant settings (#174 step 1).
//!
//! Preferences a customer chooses **once for their whole workspace**, as opposed to the per-box
//! knobs in `tenant_boxes.recording_config` / `tracking_config`. The distinction matters: a box owns
//! its disk quota and its cameras, but "capture faces from the HD stream" is a product decision that
//! should hold across every appliance the tenant installs — and expressed per box it silently
//! diverges the moment a second one is added.
//!
//! Resolution happens **server-side** in `store::recording_config_by_token`, which overlays the
//! tenant value onto the box's config. The edge is unchanged: it keeps reading
//! `face_capture_stream` from the same JSON. Pushing the decision into the appliance instead would
//! gate a UI preference on an edge release reaching every box in the field.
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};

use app_core::workflows::device_gate;

use super::{err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::{store, AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/settings", get(read).put(write))
}

/// Which stream faces are captured from. Stored as a string so the edge's existing
/// `face_capture_stream` comparison keeps working untouched.
const FACE_CAPTURE_STREAM: &str = "face_capture_stream";

#[derive(Deserialize)]
struct SettingsReq {
    /// `"preview"` | `"hd"`. Absent leaves the current choice alone — a PUT that omits a key is not
    /// a request to clear it, or a UI that only knows about one setting would wipe the others.
    #[serde(default)]
    face_capture_stream: Option<String>,
    /// How many unanswered Device Gate requests one phone may hold at once (#176). Absent leaves
    /// the current value; unset means [`app_core::workflows::device_gate::DEFAULT_MAX_PENDING`].
    #[serde(default)]
    device_gate_max_pending: Option<i64>,
}

/// The tenant's settings. `{}` when nothing has been chosen — absent means "not chosen", never a
/// default, so the caller can tell the difference.
async fn read(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "settings.read")?;
    let tid = tenant_of(&c)?;
    let settings = store::tenant_settings(&st.pool, tid)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(settings))
}

/// Set one or more settings. Merges, so writing one key cannot clobber another.
async fn write(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<SettingsReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "settings.write")?;
    let tid = tenant_of(&c)?;

    let mut patch = serde_json::Map::new();
    if let Some(raw) = b.face_capture_stream {
        let v = raw.trim().to_ascii_lowercase();
        // Validate rather than pass through: an unrecognised value would reach the edge, fail its
        // `eq_ignore_ascii_case("hd")` check, and silently mean "preview" — a setting that appears
        // saved and does nothing.
        if v != "preview" && v != "hd" {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "face_capture_stream must be 'preview' or 'hd'",
            ));
        }
        patch.insert(FACE_CAPTURE_STREAM.to_string(), json!(v));
    }
    if let Some(n) = b.device_gate_max_pending {
        // Rejected, not clamped, unlike the engine's read. Here a person typed it and can be told;
        // there a graph is mid-ask and silence would be worse than the nearest legal value.
        if !(1..=device_gate::MAX_PENDING_CEILING).contains(&n) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                format!(
                    "device_gate_max_pending must be between 1 and {}",
                    device_gate::MAX_PENDING_CEILING
                ),
            ));
        }
        patch.insert(device_gate::MAX_PENDING_KEY.to_string(), json!(n));
    }
    if patch.is_empty() {
        return Ok(StatusCode::NO_CONTENT); // nothing asked for; not an error
    }

    store::set_tenant_settings(&st.pool, tid, Value::Object(patch))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // The edge picks this up on its next `/config/recording` poll; there is nothing to push.
    Ok(StatusCode::NO_CONTENT)
}
