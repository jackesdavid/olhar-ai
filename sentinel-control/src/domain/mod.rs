//! Tenant-scoped domain APIs (Phase 3): JWT + action-guarded HTTP handlers that
//! reuse app-core's tenant-scoped repos/services. The tenant ALWAYS comes from the
//! JWT (`claims.tid`), never the client — that is the isolation boundary. The repos
//! are sync (`Db::block`), so handlers run them on `spawn_blocking`.
use axum::http::StatusCode;
use axum::Router;
use uuid::Uuid;

use crate::auth::Claims;
use crate::AppState;

mod admin_providers;
mod agent;
mod alerts;
mod beacons;
mod billing;
pub mod boxes;
mod devices;
mod dictionaries;
mod device_gate;
mod events;
mod faces;
mod jobs;
mod map;
mod persons;
mod push_devices;
pub(crate) mod recordings;
mod settings;
mod tuya;
mod workflows;

/// `(status, message)` error tuple — axum turns it into the HTTP response.
pub type ApiErr = (StatusCode, String);

/// Merge every domain sub-router. Each handler guards itself with `auth::require`.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(devices::router())
        .merge(dictionaries::router())
        .merge(faces::router())
        .merge(agent::router())
        .merge(alerts::router())
        .merge(billing::router())
        .merge(persons::router())
        .merge(beacons::router())
        .merge(push_devices::router())
        .merge(map::router())
        .merge(workflows::router())
        .merge(boxes::router())
        .merge(admin_providers::router())
        .merge(recordings::router())
        .merge(jobs::router())
        .merge(events::router())
        .merge(device_gate::router())
        .merge(settings::router())
        .merge(tuya::router())
}

/// The tenant this token operates in. The master's own (un-assumed) token carries no
/// tenant — it must `assume` one first (which mints a tenant-scoped token).
pub fn tenant_of(c: &Claims) -> Result<Uuid, ApiErr> {
    c.tenant().ok_or((
        StatusCode::BAD_REQUEST,
        "no tenant context — assume a tenant first".into(),
    ))
}

/// Run a blocking (sync repo) closure off the async runtime.
pub async fn blocking<F, T>(f: F) -> Result<T, ApiErr>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task panicked: {e}"),
        )
    })
}

/// A short helper for ad-hoc errors.
pub fn err(code: StatusCode, msg: impl Into<String>) -> ApiErr {
    (code, msg.into())
}

#[cfg(test)]
mod authz_guard {
    /// Authorization here is a **convention**, not a type: `UserClaims` proves only that someone is
    /// signed in, and `auth::require` is a separate call the author has to remember. A handler that
    /// takes `UserClaims` and never calls it is silently reachable by any logged-in user of the
    /// tenant — which is exactly how `/agent/*`, `/platform-logs`, and `/internal/live-start` ended
    /// up open. The compiler cannot see the mistake, so this test does.
    ///
    /// A lint, not a proof: it reads source text and can be fooled. The real fix is a per-action
    /// extractor so the signature *is* the guard; that touches every handler in the crate, and this
    /// holds the line until then.
    const SOURCES: &[(&str, &str)] = &[
        ("admin_providers.rs", include_str!("admin_providers.rs")),
        ("agent.rs", include_str!("agent.rs")),
        ("alerts.rs", include_str!("alerts.rs")),
        ("beacons.rs", include_str!("beacons.rs")),
        ("billing.rs", include_str!("billing.rs")),
        ("boxes.rs", include_str!("boxes.rs")),
        ("device_gate.rs", include_str!("device_gate.rs")),
        ("devices.rs", include_str!("devices.rs")),
        ("dictionaries.rs", include_str!("dictionaries.rs")),
        ("events.rs", include_str!("events.rs")),
        ("faces.rs", include_str!("faces.rs")),
        ("jobs.rs", include_str!("jobs.rs")),
        ("map.rs", include_str!("map.rs")),
        ("persons.rs", include_str!("persons.rs")),
        ("push_devices.rs", include_str!("push_devices.rs")),
        ("recordings.rs", include_str!("recordings.rs")),
        ("tuya.rs", include_str!("tuya.rs")),
        ("workflows.rs", include_str!("workflows.rs")),
    ];

    /// Handlers that authenticate but intentionally gate on nothing. Each needs a reason, because
    /// the next person to add one should have to justify it in a diff.
    const ALLOWED: &[&str] = &[
        // Browser error telemetry. Any signed-in user may report their own crash; there is no
        // action to gate on, and refusing telemetry to a narrow role just loses the crash report.
        "client_log",
        // Device Gate (#176). Both gate on OWNERSHIP of the addressed phone, not on a role: the
        // graph author picked a device, and a permission check would mean a Viewer holding that
        // phone cannot answer the question the graph asked them. See `device_gate.rs`.
        "answer",
        "pending",
    ];

    /// Split a file into `(name, text)` for every `async fn`, where `text` spans the signature and
    /// the body.
    ///
    /// The span is found by brace depth, not by indentation: a multi-line signature ends with
    /// `) -> Result<…> {` at column 0, so any "next item starts unindented" rule cuts the handler
    /// off before its first statement — and every guard would then look missing. Braces inside
    /// string literals could fool this, but `format!("{e}")` and friends are balanced, and the
    /// self-test below pins the shape.
    fn handlers(src: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut cur: Option<(String, String)> = None;
        let mut depth = 0i32;
        let mut opened = false;
        for line in src.lines() {
            if cur.is_none() {
                if let Some(rest) = line.trim_start().strip_prefix("async fn ") {
                    let name = rest
                        .split(|c: char| c == '(' || c == '<' || c.is_whitespace())
                        .next()
                        .unwrap_or("")
                        .to_string();
                    // Keep the signature line: for a single-line signature it is the only place
                    // `UserClaims(` appears, so dropping it made every handler look guarded.
                    cur = Some((name, String::new()));
                    depth = 0;
                    opened = false;
                } else {
                    continue;
                }
            }
            if let Some((_, body)) = cur.as_mut() {
                body.push_str(line);
                body.push('\n');
            }
            // Count braces in code only. A comment showing a JSON shape — `{ "inputs": { … } }` —
            // is balanced but flips `opened`, which would end the span before the first statement.
            let code = strip_comment(line);
            depth += code.matches('{').count() as i32;
            depth -= code.matches('}').count() as i32;
            opened |= code.contains('{');
            if opened && depth <= 0 {
                out.extend(cur.take());
            }
        }
        out.extend(cur);
        out
    }

    /// Drop a trailing `//` comment, leaving `https://…` alone.
    fn strip_comment(line: &str) -> &str {
        let b = line.as_bytes();
        for i in 0..b.len().saturating_sub(1) {
            if b[i] == b'/' && b[i + 1] == b'/' && (i == 0 || b[i - 1] != b':') {
                return &line[..i];
            }
        }
        line
    }

    /// The three shapes that count as authorizing a handler. `!c.master` is as real a guard as
    /// `auth::require`; the catalog just has no action for "platform operator".
    fn is_guarded(body: &str) -> bool {
        body.contains("auth::require")
            || body.contains("require_master")
            || body.contains("!c.master")
    }

    fn unguarded(file: &str, src: &str) -> Vec<String> {
        handlers(src)
            .into_iter()
            .filter(|(name, body)| {
                body.contains("UserClaims(")
                    && !is_guarded(body)
                    && !ALLOWED.contains(&name.as_str())
            })
            .map(|(name, _)| format!("{file}::{name}"))
            .collect()
    }

    #[test]
    fn every_authenticated_handler_checks_a_permission() {
        let found: Vec<String> = SOURCES
            .iter()
            .flat_map(|(file, src)| unguarded(file, src))
            .collect();
        assert!(
            found.is_empty(),
            "these handlers take UserClaims but never authorize it — add auth::require(&c, \"…\") \
             or, if the route is genuinely open to any signed-in user, add it to ALLOWED with a \
             reason:\n  {}",
            found.join("\n  ")
        );
    }

    /// A lint that cannot fail is worse than no lint, because it reads as coverage. Prove it fires.
    #[test]
    fn the_guard_actually_detects_an_unguarded_handler() {
        let src = "async fn peek(UserClaims(c): UserClaims) -> Result<(), ApiErr> {\n    \
                   let tid = tenant_of(&c)?;\n    Ok(())\n}\n";
        assert_eq!(unguarded("x.rs", src), vec!["x.rs::peek".to_string()]);

        let guarded = "async fn peek(UserClaims(c): UserClaims) -> Result<(), ApiErr> {\n    \
                       auth::require(&c, \"devices.read\")?;\n    Ok(())\n}\n";
        assert!(unguarded("x.rs", guarded).is_empty());

        // Master-only handlers guard with a plain `if`, not the action catalog.
        let master = "async fn peek(UserClaims(c): UserClaims) -> Result<(), ApiErr> {\n    \
                      if !c.master {\n        return Err(err(FORBIDDEN, \"master only\"));\n    \
                      }\n    Ok(())\n}\n";
        assert!(unguarded("x.rs", master).is_empty());
    }

    /// Both traps that made the first version of this lint report handlers that were fine: a
    /// multi-line signature (whose `) -> … {` sits at column 0) and braces inside a comment.
    #[test]
    fn the_guard_spans_real_handler_shapes() {
        let src = "async fn peek(\n    UserClaims(c): UserClaims,\n    \
                   // shape: { \"inputs\": { \"a\": \"b\" } }\n    \
                   body: Option<Json<Value>>,\n) -> Result<(), ApiErr> {\n    \
                   auth::require(&c, \"workflows.run\")?;\n    Ok(())\n}\n";
        assert!(
            unguarded("x.rs", src).is_empty(),
            "the guard is after the signature and after a brace-carrying comment; both must be \
             inside the span or every such handler is falsely reported"
        );
    }
}
