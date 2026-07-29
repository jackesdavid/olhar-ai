//! HMAC-SHA256 request signing for edge → cloud traffic.
//!
//! The appliance authenticates with a static bearer (`sbx_…`) that is minted once and rotated only
//! at re-enrollment. That token proves *who* is calling and nothing else: it does not bind the
//! request to its body, it never expires, and a captured request replays forever. So a `/config/face`
//! push could be replayed or its payload rewritten by anyone who ever saw one — and those pushes
//! enqueue directly into the workflow event queue.
//!
//! Signing closes that: every request carries a MAC over its method, path, query, timestamp and
//! body, keyed on a per-box secret. What it deliberately does **not** fix is a stolen `edge.env` —
//! an attacker holding the bearer can fetch a signing key and sign perfectly well. This bounds
//! forgery and replay, not credential theft.
//!
//! ### Why the timestamp is Unix seconds and not RFC3339
//!
//! The house rule is RFC3339 text everywhere, because sqlx is built without the `timestamptz`
//! decoder. But [`crate::timeutil::now_rfc3339`] emits **local offset** — an appliance in São Paulo
//! and a container in UTC produce strings that are not comparable, and skew is exactly what this
//! check measures. Epoch seconds are unambiguous by construction. This is a deliberate, documented
//! exception, and it applies only to the value inside the MAC; stored columns stay RFC3339.

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Scheme tag, and the first line of every canonical string.
///
/// Domain-separates this MAC from every other use of the same key material, so a v1 signature can
/// never be replayed as a future v2 with different semantics.
pub const SCHEME: &str = "SEN1";

/// How far a request's timestamp may sit from ours, in seconds.
///
/// Five minutes is generous for NTP-synced hosts and deliberately so: appliances live on customer
/// LANs, some behind firewalls that block NTP, and a too-tight window turns a clock problem into a
/// fleet outage. Replay inside the window is accepted — see the module note on nonces.
pub const MAX_SKEW_SECS: i64 = 300;

/// Wire prefix on the signature header value, mirroring Stripe's `v1=` so a second algorithm can
/// coexist later without renaming the header.
const SIG_PREFIX: &str = "v1=";

/// Everything that enters the MAC.
///
/// Borrowed throughout: the only allocation is the body hash, so signing a 1.5 MB base64 frame
/// costs one pass over the bytes rather than a copy.
pub struct Canonical<'a> {
    /// Uppercase HTTP method.
    pub method: &'a str,
    /// Request path, exactly as it goes on the wire — percent-encoding intact, **not** normalized.
    pub path: &'a str,
    /// Raw query string without `?`, or `""`.
    pub query: &'a str,
    /// Unix epoch seconds.
    pub timestamp: i64,
    /// Per-request random value. Carried and signed, but not yet checked for reuse.
    pub nonce: &'a str,
    /// Which key signed this, so the verifier picks one key instead of trying every live one.
    pub key_id: &'a str,
    /// Raw request body; empty for GET.
    pub body: &'a [u8],
}

impl Canonical<'_> {
    /// The exact bytes that get MAC'd.
    ///
    /// Path and query are **not** normalized. There is one client implementation, so the honest
    /// contract is "sign the literal bytes you send, verify the literal bytes you received";
    /// canonicalization rules (SigV4-style) exist to reconcile many clients and are where interop
    /// bugs live. Do not add them here without a second implementation to reconcile with.
    ///
    /// The identity of the caller is *not* in this string. It is bound indirectly: `key_id` selects
    /// a DB row that carries the box and tenant, so the client never asserts who it is — which
    /// preserves the invariant the bearer path already has.
    pub fn string_to_sign(&self) -> String {
        format!(
            "{SCHEME}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.method,
            self.path,
            self.query,
            self.timestamp,
            self.nonce,
            self.key_id,
            sha256_hex(self.body),
        )
    }
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Lowercase-hex HMAC-SHA256.
pub fn hmac_sha256_hex(secret: &[u8], msg: &[u8]) -> String {
    hex::encode(mac(secret, msg).finalize().into_bytes())
}

/// Uppercase-hex HMAC-SHA256 — Tuya's API requires uppercase.
pub fn hmac_sha256_hex_upper(secret: &[u8], msg: &[u8]) -> String {
    hex::encode_upper(mac(secret, msg).finalize().into_bytes())
}

/// Constant-time check of a hex-encoded HMAC-SHA256.
///
/// Uses `Mac::verify_slice`, which compares in constant time — comparing hex with `==` would leak
/// the correct prefix one byte at a time.
pub fn hmac_sha256_verify(secret: &[u8], msg: &[u8], expected_hex: &str) -> bool {
    match hex::decode(expected_hex) {
        Ok(raw) => mac(secret, msg).verify_slice(&raw).is_ok(),
        Err(_) => false,
    }
}

fn mac(secret: &[u8], msg: &[u8]) -> HmacSha256 {
    let mut m = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    m.update(msg);
    m
}

/// Sign a canonical request, producing the `X-Sentinel-Signature` value (`v1=<hex>`).
pub fn sign(secret: &str, c: &Canonical<'_>) -> String {
    format!(
        "{SIG_PREFIX}{}",
        hmac_sha256_hex(secret.as_bytes(), c.string_to_sign().as_bytes())
    )
}

/// A new 32-byte signing secret, base64url-nopad (43 chars).
///
/// Drawn from the OS CSPRNG rather than built from UUIDs the way box tokens are: a UUID is fine as
/// an opaque bearer, but a MAC key should be a full-entropy draw.
pub fn new_secret() -> String {
    use base64::Engine as _;
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// A new per-request nonce: 16 random bytes as lowercase hex.
pub fn new_nonce() -> String {
    let mut buf = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Why a signature was refused. Carries enough for the caller to answer usefully — notably
/// [`VerifyError::Skew`], which must tell the edge our clock so it can correct and retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// Signature header missing a recognized `v<n>=` prefix, or not hex.
    Malformed,
    /// Timestamp outside [`MAX_SKEW_SECS`]. `server_now` goes back on the wire.
    Skew { server_now: i64 },
    /// Well-formed, correct key selected, wrong MAC.
    BadSignature,
}

impl VerifyError {
    /// Short stable tag for the error body — the edge branches on this to decide whether to correct
    /// its clock or drop its key and re-bootstrap.
    pub fn code(&self) -> &'static str {
        match self {
            VerifyError::Malformed => "malformed_signature",
            VerifyError::Skew { .. } => "clock_skew",
            VerifyError::BadSignature => "bad_signature",
        }
    }
}

/// Verify a presented signature against `secret`.
///
/// Order matters: skew is checked **before** the MAC so a box with a wrong clock gets `clock_skew`
/// (which it can act on) rather than `bad_signature` (which would make it discard a perfectly good
/// key and re-bootstrap in a loop). Neither branch reveals anything an attacker doesn't have.
pub fn verify(
    secret: &str,
    c: &Canonical<'_>,
    presented: &str,
    now: i64,
) -> Result<(), VerifyError> {
    let Some(hex_sig) = presented.strip_prefix(SIG_PREFIX) else {
        return Err(VerifyError::Malformed);
    };
    if hex_sig.len() != 64 || !hex_sig.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(VerifyError::Malformed);
    }
    if (now - c.timestamp).abs() > MAX_SKEW_SECS {
        return Err(VerifyError::Skew { server_now: now });
    }
    if hmac_sha256_verify(secret.as_bytes(), c.string_to_sign().as_bytes(), hex_sig) {
        Ok(())
    } else {
        Err(VerifyError::BadSignature)
    }
}

/// Current Unix epoch seconds. Not RFC3339 — see the module note.
pub fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "cnJmM2hLc0hZbGpXTndBTmhoV0VfTmY0dFRvUjBBMFE";
    const KEY_ID: &str = "0f5a2b3c-4d5e-4f60-8a1b-2c3d4e5f6071";
    const NONCE: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
    const TS: i64 = 1_800_000_000;

    fn canonical<'a>(body: &'a [u8]) -> Canonical<'a> {
        Canonical {
            method: "POST",
            path: "/config/face",
            query: "",
            timestamp: TS,
            nonce: NONCE,
            key_id: KEY_ID,
            body,
        }
    }

    /// The canonical string IS the contract between two independently-built binaries: the edge
    /// signs it, control rebuilds it from a parsed HTTP request. Any change — a reordered field, a
    /// separator, a stray trailing newline — silently 401s the entire fleet, and the failure shows
    /// up in production rather than here. So pin the exact bytes, with the newlines visible.
    #[test]
    fn canonical_string_is_exactly_this() {
        let c = canonical(b"{\"a\":1}");
        let body_hash = sha256_hex(b"{\"a\":1}");
        assert_eq!(
            c.string_to_sign(),
            format!("SEN1\nPOST\n/config/face\n\n{TS}\n{NONCE}\n{KEY_ID}\n{body_hash}")
        );
        // No trailing newline, and nothing locale-dependent in the timestamp.
        assert!(!c.string_to_sign().ends_with('\n'));
        assert!(!c.string_to_sign().contains('\r'));
    }

    /// Golden vector: freezes the whole pipeline, not just the format. Trustworthy because
    /// `hmac_primitives_match_rfc4231` independently pins the primitive against published vectors —
    /// this value is only as good as that one.
    #[test]
    fn golden_signature() {
        assert_eq!(
            sign(SECRET, &canonical(b"{\"a\":1}")),
            "v1=28deb3f5a65901a820e72bc6c8dc864c9f6e71f8c12d80c02c2abe28adfef4b5"
        );
    }

    #[test]
    fn sign_then_verify_round_trips() {
        let c = canonical(b"hello");
        assert_eq!(verify(SECRET, &c, &sign(SECRET, &c), TS), Ok(()));
    }

    /// An empty body must hash, not be skipped — otherwise a GET's signature would be independent
    /// of whether a body was present, and a body could be added to a signed GET.
    #[test]
    fn empty_body_hashes_to_the_sha256_of_nothing() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let c = canonical(b"");
        assert_eq!(verify(SECRET, &c, &sign(SECRET, &c), TS), Ok(()));
    }

    /// Body tampering is the attack this exists to stop: `/config/face` and `/config/motion` push
    /// straight into the workflow event queue, so a rewritten payload is injected work.
    #[test]
    fn a_single_flipped_body_byte_invalidates() {
        let sig = sign(SECRET, &canonical(b"{\"face_width\":30}"));
        let tampered = canonical(b"{\"face_width\":31}");
        assert_eq!(
            verify(SECRET, &tampered, &sig, TS),
            Err(VerifyError::BadSignature)
        );
    }

    /// Without the path in the string, a signed body replays at any other endpoint that accepts the
    /// same shape.
    #[test]
    fn a_signature_does_not_travel_to_another_endpoint() {
        let sig = sign(SECRET, &canonical(b"{}"));
        let mut moved = canonical(b"{}");
        moved.path = "/config/motion";
        assert_eq!(
            verify(SECRET, &moved, &sig, TS),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn method_and_query_are_bound() {
        let sig = sign(SECRET, &canonical(b""));

        let mut m = canonical(b"");
        m.method = "GET";
        assert_eq!(verify(SECRET, &m, &sig, TS), Err(VerifyError::BadSignature));

        // The long-poll's `?wait=N` rides the query field, so it can't be lengthened in flight.
        let mut q = canonical(b"");
        q.query = "wait=60";
        assert_eq!(verify(SECRET, &q, &sig, TS), Err(VerifyError::BadSignature));
    }

    /// The skew boundary, from both directions, including a clock that runs fast.
    #[test]
    fn timestamps_expire_outside_the_window_only() {
        let c = canonical(b"x");
        let sig = sign(SECRET, &c);
        assert_eq!(verify(SECRET, &c, &sig, TS + MAX_SKEW_SECS - 1), Ok(()));
        assert_eq!(verify(SECRET, &c, &sig, TS - MAX_SKEW_SECS + 1), Ok(()));
        assert_eq!(
            verify(SECRET, &c, &sig, TS + MAX_SKEW_SECS + 1),
            Err(VerifyError::Skew {
                server_now: TS + MAX_SKEW_SECS + 1
            })
        );
        assert_eq!(
            verify(SECRET, &c, &sig, TS - MAX_SKEW_SECS - 1),
            Err(VerifyError::Skew {
                server_now: TS - MAX_SKEW_SECS - 1
            })
        );
    }

    /// A wrong clock must be distinguishable from a wrong key, or the edge's recovery does the
    /// wrong thing: `clock_skew` means "correct and retry", `bad_signature` means "drop the key".
    /// Swapping them would make a skewed box wipe a valid key on every request.
    #[test]
    fn skew_is_reported_before_a_bad_mac() {
        let c = canonical(b"x");
        assert_eq!(
            verify(SECRET, &c, &sign("some-other-key", &c), TS + 10_000),
            Err(VerifyError::Skew {
                server_now: TS + 10_000
            })
        );
    }

    /// Box A signing with its own key while presenting box B's `key_id` must fail — this is what
    /// makes a per-box key worth more than a per-tenant one.
    #[test]
    fn another_boxs_key_does_not_verify() {
        let c = canonical(b"payload");
        let signed_by_a = sign("box-a-secret", &c);
        assert_eq!(
            verify("box-b-secret", &c, &signed_by_a, TS),
            Err(VerifyError::BadSignature)
        );
    }

    /// Junk in the header is `Malformed`, never a MAC comparison against attacker-chosen length.
    #[test]
    fn malformed_signatures_are_rejected_before_any_comparison() {
        let c = canonical(b"x");
        for bad in [
            "",
            "deadbeef", // no prefix
            "v1=",      // empty
            "v1=zz23f9cbb0b2ddf67bd23ff8a2d9e2c0d6ff6d0e4a5ab1d6e07d70a20dd8c2e", // non-hex
            "v1=b2b23f9cbb0b2ddf67bd23ff8a2d9e2c0d6ff6d0e4a5ab1d6e07d70a20dd8c", // short
            "v2=b2b23f9cbb0b2ddf67bd23ff8a2d9e2c0d6ff6d0e4a5ab1d6e07d70a20dd8c2e", // future scheme
        ] {
            assert_eq!(
                verify(SECRET, &c, bad, TS),
                Err(VerifyError::Malformed),
                "{bad:?} should be malformed"
            );
        }
        // Right shape, all zeros — must reach the MAC and fail there, not pass.
        assert_eq!(
            verify(SECRET, &c, &format!("v1={}", "0".repeat(64)), TS),
            Err(VerifyError::BadSignature)
        );
    }

    /// Secrets and nonces must be full-entropy and distinct per call.
    #[test]
    fn generated_material_is_random_and_the_right_size() {
        use base64::Engine as _;
        let a = new_secret();
        assert_eq!(a.len(), 43, "32 bytes base64url-nopad");
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&a)
                .unwrap()
                .len(),
            32
        );
        assert_ne!(a, new_secret());

        let n = new_nonce();
        assert_eq!(n.len(), 32);
        assert!(n.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(n, new_nonce());
    }

    /// The shared primitives, against published vectors, so a future refactor of `mac()` can't
    /// quietly change what every caller computes.
    #[test]
    fn hmac_primitives_match_rfc4231() {
        // RFC 4231 test case 1.
        let key = [0x0b; 20];
        assert_eq!(
            hmac_sha256_hex(&key, b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hmac_sha256_hex_upper(&key, b"Hi There"),
            "B0344C61D8DB38535CA8AFCEAF0BF12B881DC200C9833DA726E9376C2E32CFF7"
        );
        assert!(hmac_sha256_verify(
            &key,
            b"Hi There",
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        ));
        assert!(!hmac_sha256_verify(&key, b"Hi There", "not hex"));
    }
}
