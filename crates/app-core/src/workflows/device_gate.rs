//! Device Gate (card #176) — the persistence half of "a human approves before the graph continues".
//!
//! One row per ENTRY into the node. The asking run is already over by the time anyone answers: a
//! run cannot block on a person, so the wait is a row in Postgres and the answer arrives as a
//! sub-run forced at that node. Same shape as `DelayTrigger`'s trailing edge, deliberately, so
//! there is one suspend mechanism in the engine rather than two.
//!
//! Two invariants live here and nowhere else:
//!
//! 1. **The raw token never touches the database.** Only `sha256(token)` hex is stored. A dump of
//!    `device_gate_requests` must not be enough to open a gate. The raw value exists in the push
//!    payload and in memory during the request that mints it.
//! 2. **A settled request can never be settled again.** Every settle is ONE conditional statement
//!    with `state = 'pending'` in the `WHERE`; the row count is the answer to "did I win". This is
//!    what makes a duplicate push delivery — which happens — land on a settled request instead of
//!    opening a second slider, and it is why the check-then-act shape used by password reset was
//!    deliberately not copied.
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// What a settled request resolved to. Five states onto three exec arms: the arm is what the graph
/// branches on, the state is what the audit trail has to be able to say. `Expired` and `Failed`
/// both take `failure` and are not the same event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateState {
    Pending,
    Approved,
    Denied,
    Expired,
    Failed,
}

impl GateState {
    pub fn as_str(self) -> &'static str {
        match self {
            GateState::Pending => "pending",
            GateState::Approved => "approved",
            GateState::Denied => "denied",
            GateState::Expired => "expired",
            GateState::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => GateState::Pending,
            "approved" => GateState::Approved,
            "denied" => GateState::Denied,
            "expired" => GateState::Expired,
            "failed" => GateState::Failed,
            _ => return None,
        })
    }

    /// Which exec arm this state fires. The whole point of three arms rather than two: a denial is
    /// an ANSWER — someone was there, read it, and said no — while `failure` means nobody decided.
    /// A graph that cannot tell them apart is a graph that eventually treats "nobody answered" as
    /// "go ahead", which is the incident this node exists to prevent.
    pub fn exec_arm(self) -> &'static str {
        match self {
            GateState::Approved => "positive",
            GateState::Denied => "negative",
            // Pending never reaches here in practice; if it somehow did, taking `failure` is the
            // safe direction — it is the arm that means "no human decided", which is true.
            GateState::Expired | GateState::Failed | GateState::Pending => "failure",
        }
    }
}

/// Which factor the phone actually used. Decision 02: a biometric is a property of the DEVICE, not
/// a requirement of the product, so a phone with none falls back to the passcode — and recording
/// WHICH is the only thing that keeps that fallback from being a silent downgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFactor {
    Biometric,
    Passcode,
}

impl AuthFactor {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthFactor::Biometric => "biometric",
            AuthFactor::Passcode => "passcode",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "biometric" => AuthFactor::Biometric,
            "passcode" => AuthFactor::Passcode,
            _ => return None,
        })
    }
}

/// Why a Recipient could not be turned into exactly one phone to ask.
pub enum ResolveError {
    /// The label matches no enabled device. The baseline lists "no registered device" as a
    /// `failure` case explicitly — a person was never asked.
    NoDevice,
    /// The label matches SEVERAL enabled devices. This cannot be honoured: decision 01 says only
    /// the addressed device may answer, and minting one request per phone would mean any of them
    /// could open the gate — the opposite of what was decided. Refusing is the honest outcome, and
    /// it takes `failure` because, again, nobody was asked.
    Ambiguous(usize),
}

/// Turn a Recipient label into the ONE device to ask. Recipients are matched by label
/// (`notification::push::enabled_tokens` does the same), but a gate needs the row id, not just the
/// token: the request is bound to a specific device for its whole life.
pub fn resolve_device_by_label(
    db: &Db,
    tenant_id: Uuid,
    label: &str,
) -> Result<(Uuid, String), ResolveError> {
    let pool = db.pool();
    let label = label.trim().to_string();
    let res: Result<Vec<(Uuid, String)>, sqlx::Error> = db.block(async move {
        sqlx::query_as(
            "SELECT id, device_token FROM push_devices
              WHERE tenant_id = $1 AND enabled AND label = $2
              ORDER BY created_at",
        )
        .bind(tenant_id)
        .bind(label)
        .fetch_all(&pool)
        .await
    });
    let rows = match res {
        Ok(r) => r,
        Err(e) => {
            log::error!("device_gate recipient lookup failed: {e}");
            return Err(ResolveError::NoDevice);
        }
    };
    match rows.len() {
        0 => Err(ResolveError::NoDevice),
        1 => Ok(rows.into_iter().next().expect("len checked")),
        n => Err(ResolveError::Ambiguous(n)),
    }
}

/// A freshly minted request: the row is in the database, and `token` is the ONLY copy of the raw
/// secret in existence. Hand it to the push payload and drop it — it cannot be recovered.
pub struct MintedRequest {
    pub id: Uuid,
    pub token: String,
    pub expires_at: String,
}

/// A pending request as the phone sees it. Deliberately does NOT carry the token: the phone already
/// has it from the push, and `GET /device-gate/pending` is reachable by any session that owns the
/// device — handing the secret back over that endpoint would make the token pointless.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PendingRequest {
    pub id: Uuid,
    pub prompt: String,
    pub flow_name: String,
    pub created_at: String,
    pub expires_at: String,
}

/// Where to resume, read back when a request settles.
#[derive(Debug, Clone)]
pub struct ResumeTarget {
    pub tenant_id: Uuid,
    pub graph_id: Uuid,
    pub node_id: u32,
    pub state: GateState,
    pub auth_factor: Option<AuthFactor>,
    pub answered_by_label: String,
}

/// 32 bytes of randomness, hex — 256 bits, so guessing is not a strategy. Same generator as the
/// password-reset token (`sentinel-control/src/password_reset.rs`), deliberately: this repo has
/// enough token patterns, and that is the newest and the closest fit.
fn new_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// The stored form. Note what is NOT here: any comparison helper. Tokens are looked up BY this
/// hash rather than compared byte-for-byte, so there is no secret-dependent branch to time. That
/// is how the "constant time" requirement is met — structurally, by the storage shape.
pub fn hash_token(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// Mint a request and return the raw token exactly once.
#[allow(clippy::too_many_arguments)]
pub fn mint(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
    asked_by_run: Uuid,
    push_device_id: Uuid,
    prompt: &str,
    flow_name: &str,
    ttl_secs: i64,
) -> Result<MintedRequest, String> {
    let token = new_token();
    let token_hash = hash_token(&token);
    let created_at = now_rfc3339();
    // Clamped so a mis-typed TTL cannot mint something absurd. The floor matters more than the
    // ceiling: decision 05 established that a push is best-effort, so a request that expires in
    // seconds would expire in a pocket and read as an unreliable node rather than a slow human.
    let ttl = ttl_secs.clamp(30, 24 * 3600);
    let expires_at = crate::timeutil::plus_secs_rfc3339(ttl);

    let pool = db.pool();
    let prompt = prompt.to_string();
    let flow_name = flow_name.to_string();
    let expires_for_row = expires_at.clone();
    let res: Result<Uuid, sqlx::Error> = db.block(async move {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO device_gate_requests
                (tenant_id, graph_id, node_id, asked_by_run, push_device_id, token_hash,
                 prompt, flow_name, state, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending', $9, $10)
             RETURNING id",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(asked_by_run)
        .bind(push_device_id)
        .bind(token_hash)
        .bind(prompt)
        .bind(flow_name)
        .bind(created_at)
        .bind(expires_for_row)
        .fetch_one(&pool)
        .await?;
        Ok(row.0)
    });

    match res {
        Ok(id) => Ok(MintedRequest {
            id,
            token,
            expires_at,
        }),
        Err(e) => {
            log::error!("device_gate mint failed: {e}");
            Err("could not create the approval request".into())
        }
    }
}

/// Settle a request with a human's answer. **The one statement that must stay atomic.**
///
/// Returns `Some(target)` only for the caller that WON. Zero rows means someone else already
/// settled it, the token was wrong, or it expired — all of which are "you did not win", and the
/// caller must resume nothing. Splitting this into a read and a write reintroduces the double-tap
/// race that ticket 04 exists to close.
pub fn settle_with_answer(
    db: &Db,
    id: Uuid,
    raw_token: &str,
    approved: bool,
    factor: AuthFactor,
    answered_by_user: Option<Uuid>,
) -> Option<ResumeTarget> {
    let token_hash = hash_token(raw_token);
    let state = if approved {
        GateState::Approved
    } else {
        GateState::Denied
    };
    let answered_at = now_rfc3339();
    let pool = db.pool();

    let res: Result<Option<(Uuid, Uuid, i64, String)>, sqlx::Error> = db.block(async move {
        sqlx::query_as(
            "UPDATE device_gate_requests
                SET state = $3, answered_at = $4, answered_by_user = $5, auth_factor = $6
              WHERE id = $1 AND token_hash = $2 AND state = 'pending'
              RETURNING tenant_id, graph_id, node_id,
                        COALESCE((SELECT label FROM push_devices d WHERE d.id = push_device_id), '')",
        )
        .bind(id)
        .bind(token_hash)
        .bind(state.as_str())
        .bind(answered_at)
        .bind(answered_by_user)
        .bind(factor.as_str())
        .fetch_optional(&pool)
        .await
    });

    match res {
        Ok(Some((tenant_id, graph_id, node_id, label))) => Some(ResumeTarget {
            tenant_id,
            graph_id,
            node_id: node_id as u32,
            state,
            auth_factor: Some(factor),
            answered_by_label: label,
        }),
        // Not an error: this is the duplicate-tap path, and it is the normal outcome of a push
        // delivered twice. The caller answers "already settled".
        Ok(None) => None,
        Err(e) => {
            log::error!("device_gate settle failed for {id}: {e}");
            None
        }
    }
}

/// Claim ONE request the engine still owes a resume, atomically, and say what arm it takes.
///
/// Two shapes, one statement, because they are the same job — "this request is finished and the
/// graph has not been told":
///
///  * **Still pending past its deadline** → becomes `expired`. Nobody decided.
///  * **Settled but never delivered** (`approved`/`denied`/`failed` with `resumed_at IS NULL`) →
///    delivered now. This is what makes an answer durable: control settles the row and nudges
///    flow, and if that nudge is lost — pod restart, network blip, wrong replica — the answer is
///    still here waiting rather than a gate someone approved that never opened.
///
/// `FOR UPDATE SKIP LOCKED` is what lets several flow pods sweep at once: each takes a different
/// row, and none resumes a request another already did.
pub fn claim_one_resumable(db: &Db) -> Option<ResumeTarget> {
    let now = now_rfc3339();
    let pool = db.pool();
    type Row = (Uuid, Uuid, i64, String, Option<String>, Option<String>);
    let res: Result<Option<Row>, sqlx::Error> = db.block(async move {
        sqlx::query_as(
            "UPDATE device_gate_requests r
                SET resumed_at = $1,
                    -- A pending row that reached its deadline settles in the SAME statement that
                    -- claims it. Two statements would leave a window where it is claimed but not
                    -- yet expired, and a crash in between would strand it forever.
                    state = CASE WHEN r.state = 'pending' THEN 'expired' ELSE r.state END,
                    answered_at = COALESCE(r.answered_at, $1)
              WHERE r.id = (
                    SELECT id FROM device_gate_requests
                     WHERE resumed_at IS NULL
                       AND (state <> 'pending' OR expires_at <= $1)
                     ORDER BY expires_at
                     LIMIT 1
                     FOR UPDATE SKIP LOCKED )
              -- `RETURNING` sees the row AFTER the update, so `r.state` is already the settled
              -- value — repeating the CASE here would be dead code. Verified against Postgres
              -- rather than assumed: a pending row claimed by this statement returns 'expired'.
              RETURNING r.tenant_id, r.graph_id, r.node_id, r.state, r.auth_factor,
                        (SELECT label FROM push_devices d WHERE d.id = r.push_device_id)",
        )
        .bind(now)
        .fetch_optional(&pool)
        .await
    });

    match res {
        Ok(Some((tenant_id, graph_id, node_id, state, factor, label))) => Some(ResumeTarget {
            tenant_id,
            graph_id,
            node_id: node_id as u32,
            // An unknown state string would be a bug, not a condition — but `failure` is the safe
            // direction to fall to, since it is the arm meaning "no human decided".
            state: GateState::from_str(&state).unwrap_or(GateState::Failed),
            auth_factor: factor.as_deref().and_then(AuthFactor::from_str),
            answered_by_label: label.unwrap_or_default(),
        }),
        Ok(None) => None,
        Err(e) => {
            log::error!("device_gate resume sweep failed: {e}");
            None
        }
    }
}

/// Mark a request failed without a human — the push could not be delivered at all. Takes the same
/// `failure` arm as a timeout, and is stored distinctly so the audit trail can tell them apart.
pub fn settle_failed(db: &Db, id: Uuid) -> bool {
    let now = now_rfc3339();
    let pool = db.pool();
    let res: Result<u64, sqlx::Error> = db.block(async move {
        let r = sqlx::query(
            "UPDATE device_gate_requests SET state = 'failed', answered_at = $2
              WHERE id = $1 AND state = 'pending'",
        )
        .bind(id)
        .bind(now)
        .execute(&pool)
        .await?;
        Ok(r.rows_affected())
    });
    matches!(res, Ok(n) if n > 0)
}

/// The phone's own list. Scoped to the CALLING DEVICE, per decision 01 — not the user's other
/// devices, because only the addressed device may answer.
///
/// This is the reliable path, not a fallback: decision 05 established that a push can be batched,
/// silenced or lost, so a request that exists only inside a notification is a request that quietly
/// expires. The app fetches this on launch and on foreground.
pub fn pending_for_device(db: &Db, push_device_id: Uuid) -> Vec<PendingRequest> {
    let now = now_rfc3339();
    let pool = db.pool();
    let res: Result<Vec<(Uuid, String, String, String, String)>, sqlx::Error> =
        db.block(async move {
            sqlx::query_as(
                "SELECT id, prompt, flow_name, created_at, expires_at
                   FROM device_gate_requests
                  WHERE push_device_id = $1 AND state = 'pending' AND expires_at > $2
                  ORDER BY created_at",
            )
            .bind(push_device_id)
            .bind(now)
            .fetch_all(&pool)
            .await
        });

    match res {
        Ok(rows) => rows
            .into_iter()
            .map(|(id, prompt, flow_name, created_at, expires_at)| PendingRequest {
                id,
                prompt,
                flow_name,
                created_at,
                expires_at,
            })
            .collect(),
        Err(e) => {
            log::error!("device_gate pending list failed: {e}");
            Vec::new()
        }
    }
}

/// The deadline of the pending request for a node, if any — what the canvas countdown reads.
/// Returns the RFC3339 expiry so the client formats and ticks it locally, exactly as the cron
/// node's countdown does: one source of truth for WHEN, and the browser only renders it.
pub fn pending_deadline_for_node(
    db: &Db,
    tenant_id: Uuid,
    graph_id: Uuid,
    node_id: u32,
) -> Option<String> {
    let now = now_rfc3339();
    let pool = db.pool();
    let res: Result<Option<(String,)>, sqlx::Error> = db.block(async move {
        sqlx::query_as(
            "SELECT expires_at FROM device_gate_requests
              WHERE tenant_id = $1 AND graph_id = $2 AND node_id = $3
                AND state = 'pending' AND expires_at > $4
              ORDER BY expires_at
              LIMIT 1",
        )
        .bind(tenant_id)
        .bind(graph_id)
        .bind(node_id as i64)
        .bind(now)
        .fetch_optional(&pool)
        .await
    });
    res.ok().flatten().map(|(s,)| s)
}

/// Who owns a request's device, and whether it is still pending — the authorization read for the
/// answer endpoint. Decision 01: the session's user must own the addressed device.
///
/// `Some((None, _))` means the device has no owner (`push_devices.user_id` is nullable so a device
/// can outlive a user row). Nobody can answer that request; it will expire down `failure`. That is
/// the correct outcome rather than widening who may answer, but it is worth being explicit that
/// the state is reachable.
pub fn owner_of_request(db: &Db, id: Uuid) -> Option<(Option<Uuid>, Uuid, bool)> {
    let pool = db.pool();
    let res: Result<Option<(Option<Uuid>, Uuid, String)>, sqlx::Error> = db.block(async move {
        sqlx::query_as(
            "SELECT d.user_id, r.tenant_id, r.state
               FROM device_gate_requests r
               JOIN push_devices d ON d.id = r.push_device_id
              WHERE r.id = $1",
        )
        .bind(id)
        .fetch_optional(&pool)
        .await
    });
    match res {
        Ok(Some((user_id, tenant_id, state))) => Some((user_id, tenant_id, state == "pending")),
        Ok(None) => None,
        Err(e) => {
            log::error!("device_gate owner lookup failed for {id}: {e}");
            None
        }
    }
}
