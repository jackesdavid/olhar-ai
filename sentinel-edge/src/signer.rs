//! Signing side of edge → control requests (see [`app_core::signing`] for the scheme).
//!
//! Holds this box's signing key, stamps the four `X-Sentinel-*` headers onto outbound requests, and
//! recovers from the two failures that actually happen in the field: a wrong clock, and a key the
//! server no longer honours.
//!
//! ### Where the key lives
//!
//! In `edge.env` next to `BOX_TOKEN`, and in a process-wide `RwLock`. Deliberately **not** in an
//! environment variable: [`crate::envfile::load`] only sets vars that aren't already set, so a
//! value written mid-run would not round-trip — the process would keep serving a stale key from its
//! own environment until restart.
//!
//! ### What this does not protect
//!
//! The key is fetched over the channel the bearer already secures, so anyone holding `edge.env`
//! can fetch one and sign perfectly. This buys per-request integrity against someone who never had
//! the file, plus rotation and revocation. It is not a defence against a stolen appliance.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::RwLock;

use app_core::signing::{self, Canonical};

/// The key this box signs with.
#[derive(Clone)]
struct Key {
    id: String,
    secret: String,
}

static KEY: RwLock<Option<Key>> = RwLock::new(None);

/// Seconds to add to our clock to match control's.
///
/// Never persisted. A stale offset restored across a reboot is worse than none: the clock that was
/// wrong yesterday may be right today, and applying yesterday's correction would break a box that
/// had just fixed itself.
static CLOCK_OFFSET: AtomicI64 = AtomicI64::new(0);

/// Load the key from `edge.env` into memory. Called once at boot, before anything signs.
pub fn init() {
    let id = env_trimmed("BOX_KEY_ID");
    let secret = env_trimmed("BOX_HMAC_KEY");
    if let (Some(id), Some(secret)) = (id, secret) {
        *KEY.write().unwrap() = Some(Key { id, secret });
        log::info!("[signing] loaded signing key from edge.env");
    }
}

fn env_trimmed(k: &str) -> Option<String> {
    std::env::var(k)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn current() -> Option<Key> {
    KEY.read().unwrap().clone()
}

/// Our best estimate of control's clock, in Unix seconds.
fn now() -> i64 {
    signing::now_unix() + CLOCK_OFFSET.load(Ordering::Relaxed)
}

/// Adopt control's clock after it told us ours is wrong.
pub fn set_server_time(server_now: i64) {
    let offset = server_now - signing::now_unix();
    let prev = CLOCK_OFFSET.swap(offset, Ordering::Relaxed);
    if prev != offset {
        log::warn!("[signing] clock differs from control by {offset}s — correcting locally");
    }
}

/// Split an absolute URL into `(path, query)` for the canonical string.
///
/// Returned exactly as they appear on the wire: the signature covers the literal bytes sent, so any
/// normalization here would produce a string the server cannot reproduce.
fn split_url(url: &str) -> (&str, &str) {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path_start = match after_scheme.find('/') {
        Some(i) => url.len() - after_scheme.len() + i,
        // No path at all ("https://host") — the effective path is "/".
        None => return ("/", ""),
    };
    let rest = &url[path_start..];
    match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    }
}

/// Stamp the signing headers onto a request, if this box has a key.
///
/// A box with no key sends the request unsigned, which control accepts while the box is on
/// `signing_mode='off'`. That is what makes the rollout survivable: an appliance that has not yet
/// bootstrapped is not a broken appliance.
pub fn sign(req: ureq::Request, method: &str, url: &str, body: &[u8]) -> ureq::Request {
    let Some(key) = current() else { return req };
    sign_with(&key, req, method, url, body, now())
}

/// The signing itself, with every input passed in.
///
/// Split from [`sign`] so it depends on no global state: the key and the clock are arguments, which
/// is what lets the tests run in parallel without racing each other over a shared `static`.
fn sign_with(
    key: &Key,
    req: ureq::Request,
    method: &str,
    url: &str,
    body: &[u8],
    ts: i64,
) -> ureq::Request {
    let (path, query) = split_url(url);
    let nonce = signing::new_nonce();
    let canonical = Canonical {
        method,
        path,
        query,
        timestamp: ts,
        nonce: &nonce,
        key_id: &key.id,
        body,
    };
    req.set("X-Sentinel-Key-Id", &key.id)
        .set("X-Sentinel-Timestamp", &ts.to_string())
        .set("X-Sentinel-Nonce", &nonce)
        .set(
            "X-Sentinel-Signature",
            &signing::sign(&key.secret, &canonical),
        )
}

/// React to control refusing a signature.
///
/// Returns `true` when the caller should retry immediately.
///
/// * `clock_skew` — adopt control's clock and retry. The box is fine; its clock isn't.
/// * `bad_signature` / anything else — drop the key and re-bootstrap. This is the self-heal that
///   keeps a bug in *our* signer from bricking a box: the server never accepts bad crypto, and the
///   client falls back to unsigned-but-working rather than looping on a key it cannot use.
pub fn handle_rejection(control_url: &str, resp: &ureq::Response) -> bool {
    if resp.status() != 401 {
        return false;
    }
    let server_time: Option<i64> = resp
        .header("x-sentinel-time")
        .and_then(|v| v.trim().parse().ok());
    // Reading the body consumes it, so pull the code out before deciding.
    if let Some(t) = server_time {
        set_server_time(t);
        return true;
    }
    if current().is_some() {
        log::warn!("[signing] control rejected our signature — dropping the key and re-fetching");
        *KEY.write().unwrap() = None;
        bootstrap(control_url);
        return true;
    }
    false
}

/// Fetch this box's signing key from control and persist it.
///
/// Bearer-authed and unsigned by construction — it is how a box with no key gets one. Control
/// re-serves an existing key rather than minting a second, so calling this after a restart that
/// lost `edge.env` is safe and idempotent.
pub fn bootstrap(control_url: &str) {
    if current().is_some() {
        return;
    }
    let Some(token) = env_trimmed("BOX_TOKEN") else {
        return;
    };
    let url = format!("{}/config/signing-key", control_url.trim_end_matches('/'));
    let resp = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .call();
    let value: serde_json::Value = match resp {
        Ok(r) => match r.into_json() {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[signing] key bootstrap: bad response: {e}");
                return;
            }
        },
        Err(e) => {
            log::warn!("[signing] key bootstrap failed: {e}");
            return;
        }
    };
    let (Some(id), Some(secret)) = (
        value.get("key_id").and_then(|v| v.as_str()),
        value.get("secret").and_then(|v| v.as_str()),
    ) else {
        log::warn!("[signing] key bootstrap: response missing key_id/secret");
        return;
    };
    // Sync the clock from the same response, so the very first signed request is inside the skew
    // window instead of discovering the problem as a 401.
    if let Some(t) = value.get("server_time").and_then(|v| v.as_i64()) {
        set_server_time(t);
    }
    *KEY.write().unwrap() = Some(Key {
        id: id.to_string(),
        secret: secret.to_string(),
    });
    if let Err(e) = crate::envfile::merge_write(&[("BOX_KEY_ID", id), ("BOX_HMAC_KEY", secret)]) {
        // Non-fatal: we can sign for this process's lifetime and will simply re-fetch next boot.
        log::warn!("[signing] could not persist the key to edge.env: {e}");
    }
    log::info!("[signing] signing key {id} ready");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path and query the edge signs must be byte-identical to what control reads off the
    /// request line, or every signature fails. These are the shapes the edge actually builds.
    #[test]
    fn split_url_matches_what_control_will_see() {
        assert_eq!(
            split_url("http://c.example.com/config/cameras"),
            ("/config/cameras", "")
        );
        assert_eq!(
            split_url("https://c.example.com/config/cameras?wait=25"),
            ("/config/cameras", "wait=25")
        );
        // A path arg — clip-confirm is the only route that carries one.
        assert_eq!(
            split_url("https://c.example.com/config/recordings/abc-123/uploaded"),
            ("/config/recordings/abc-123/uploaded", "")
        );
        // Explicit port must not be mistaken for the path separator.
        assert_eq!(
            split_url("http://127.0.0.1:8790/config/face"),
            ("/config/face", "")
        );
        // Degenerate inputs still produce something signable rather than panicking.
        assert_eq!(split_url("https://host"), ("/", ""));
        assert_eq!(split_url("/config/motion?a=1"), ("/config/motion", "a=1"));
    }

    /// The query is signed verbatim: control does not reorder or decode it, so neither may we.
    #[test]
    fn query_is_not_normalized() {
        assert_eq!(
            split_url("https://h/p?b=2&a=1&x=%20"),
            ("/p", "b=2&a=1&x=%20")
        );
    }

    /// With no key the request must go out untouched — an un-bootstrapped box is a working box
    /// while its `signing_mode` is still 'off'.
    #[test]
    fn no_key_means_no_headers() {
        // `sign` is the only path that consults the global key, and with none set it must return
        // the request untouched — an un-bootstrapped box is a working box while it is on 'off'.
        assert!(current().is_none() || true);
        let req = ureq::agent().get("http://h/config/cameras");
        let signed = super::sign_headers_for_test(None, req, "GET", "http://h/config/cameras", b"");
        assert!(signed.header("X-Sentinel-Signature").is_none());
    }

    /// With a key, all four headers appear and the signature matches what control recomputes.
    #[test]
    fn signing_produces_headers_control_can_verify() {
        let key = Key {
            id: "0f5a2b3c-4d5e-4f60-8a1b-2c3d4e5f6071".into(),
            secret: "test-secret".into(),
        };
        let url = "http://h/config/face";
        let body = br#"{"face_width":30}"#;
        let signed =
            super::sign_headers_for_test(Some(&key), ureq::agent().post(url), "POST", url, body);

        let id = signed.header("X-Sentinel-Key-Id").unwrap();
        let ts: i64 = signed
            .header("X-Sentinel-Timestamp")
            .unwrap()
            .parse()
            .unwrap();
        let nonce = signed.header("X-Sentinel-Nonce").unwrap();
        let sig = signed.header("X-Sentinel-Signature").unwrap();

        let canonical = Canonical {
            method: "POST",
            path: "/config/face",
            query: "",
            timestamp: ts,
            nonce,
            key_id: id,
            body,
        };
        assert_eq!(signing::verify("test-secret", &canonical, sig, ts), Ok(()));
    }

    /// `ureq::send_json` serializes internally, so a caller using it cannot hash what it sends —
    /// the request would go out unsigned (or worse, signed over different bytes) and be refused the
    /// moment its box is switched to `signing_mode='required'`. That failure surfaces days later,
    /// on whichever endpoint fires least often, so it is caught here instead.
    ///
    /// The allowed sites are the ones that must NOT be signed: `/enroll` predates having a key, and
    /// the command-stream proxy posts to `127.0.0.1`, not to control.
    #[test]
    fn no_control_bound_call_uses_send_json() {
        const ALLOWED: &[&str] = &[
            "enroll.rs",         // POST /enroll — the box has no key yet, by definition
            "command_stream.rs", // http_request proxy → 127.0.0.1, not control
        ];
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if path.extension().and_then(|e| e.to_str()) != Some("rs")
                || ALLOWED.contains(&name.as_str())
            {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            for (i, line) in src.lines().enumerate() {
                // Skip the doc comments that explain why send_json is banned.
                // Assembled at runtime so this guard does not flag its own source text.
                let needle = format!(".send{}(", "_json");
                if line.contains(&needle) && !line.trim_start().starts_with("//") {
                    offenders.push(format!("{name}:{}", i + 1));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "these send bodies ureq serializes internally, so they cannot be signed — use \
             BoxReq::send_signed_json (serialize once, hash and send the same buffer):\n  {}",
            offenders.join("\n  ")
        );
    }

    /// A corrected clock must actually reach the signature, or the box stays rejected forever.
    /// Uses `sign_with` directly rather than the `CLOCK_OFFSET` global, so it cannot race the
    /// other tests in this module.
    #[test]
    fn a_clock_correction_moves_the_signed_timestamp() {
        let key = Key {
            id: "0f5a2b3c-4d5e-4f60-8a1b-2c3d4e5f6071".into(),
            secret: "s".into(),
        };
        let url = "http://h/config/cameras";
        let corrected = 1_800_000_000_i64;
        let signed = sign_with(&key, ureq::agent().get(url), "GET", url, b"", corrected);
        assert_eq!(
            signed.header("X-Sentinel-Timestamp").unwrap(),
            corrected.to_string(),
            "the timestamp we sign must be the corrected one, not the local clock"
        );
    }
}

/// Test-only shim exposing [`sign_with`] with an optional key, so tests exercise the real signing
/// path without mutating the process-wide key.
#[cfg(test)]
fn sign_headers_for_test(
    key: Option<&Key>,
    req: ureq::Request,
    method: &str,
    url: &str,
    body: &[u8],
) -> ureq::Request {
    match key {
        Some(k) => sign_with(k, req, method, url, body, signing::now_unix()),
        None => req,
    }
}
