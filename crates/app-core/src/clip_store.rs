//! Durable clip storage — Cloudflare R2.
//!
//! The copy of a recording that survives the edge box being stolen, smashed, or wiped. That
//! is the whole point: today a clip lives only on the box's local ring, so the footage is
//! only as safe as the hardware being stolen.
//!
//! Deliberately separate from [`crate::media`], which is a **staging** bucket (minio, on the
//! VPS) holding notification attachments for a few hours. Different store, different lifetime,
//! different failure mode.
//!
//!   R2_ENDPOINT          — https://<account>.r2.cloudflarestorage.com
//!   R2_BUCKET            — e.g. sentinel-recordings
//!   R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY
//!   R2_REGION            — "auto" for R2
//!
//! **The edge never holds these credentials.** Control signs a short-lived URL and the edge
//! uploads straight to R2, so no video crosses the VPS (75 GB of disk, one shared NIC).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use s3::creds::Credentials;
use s3::{Bucket, Region};
use uuid::Uuid;

fn bucket() -> Option<&'static Bucket> {
    static B: OnceLock<Option<Box<Bucket>>> = OnceLock::new();
    B.get_or_init(build).as_deref()
}

fn build() -> Option<Box<Bucket>> {
    let endpoint = env("R2_ENDPOINT")?;
    let name = env("R2_BUCKET").unwrap_or_else(|| "sentinel-recordings".to_string());
    let key = env("R2_ACCESS_KEY_ID")?;
    let secret = env("R2_SECRET_ACCESS_KEY")?;
    let region = Region::Custom {
        // R2 has no regions; SigV4 still needs a name and expects "auto".
        region: env("R2_REGION").unwrap_or_else(|| "auto".to_string()),
        endpoint,
    };
    let creds = match Credentials::new(Some(&key), Some(&secret), None, None, None) {
        Ok(c) => c,
        Err(e) => {
            log::error!("clip_store: credentials init failed: {e}");
            return None;
        }
    };
    match Bucket::new(&name, region, creds) {
        Ok(b) => Some(b.with_path_style()),
        Err(e) => {
            log::error!("clip_store: bucket init failed: {e}");
            None
        }
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `rust-s3` returns `Ok(ResponseData)` for HTTP 4xx/5xx — a rejected request looks like a
/// successful one unless the status is checked. A silently-failed delete is the worst kind:
/// we would tell a customer their footage is gone while it sits in the bucket.
fn ok_status(what: &str, code: u16, body: &[u8]) -> Result<(), String> {
    if (200..300).contains(&code) {
        return Ok(());
    }
    let detail: String = String::from_utf8_lossy(body).chars().take(200).collect();
    Err(format!("clip_store: {what}: HTTP {code} {detail}"))
}

/// Whether durable clip storage is configured.
pub fn enabled() -> bool {
    bucket().is_some()
}

static ARMED: AtomicBool = AtomicBool::new(false);

/// Whether the bucket's retention rules are proven present, and clips may therefore be
/// uploaded.
///
/// Storing footage we cannot expire is worse than not storing it: the customer is the LGPD
/// controller and we would be holding their video indefinitely with no mechanism to stop.
/// So [`ensure_retention_lifecycle`] arms this on success and disarms it on failure, and
/// nothing hands out an upload URL while it is false.
pub fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// `rust-s3` marks presigning `async` for API symmetry even though it is pure signing. A
/// throwaway current-thread runtime keeps this callable from the sync edge/control paths and
/// avoids a shared runtime that several threads would contend on.
fn block_on<F: std::future::Future>(f: F) -> Result<F::Output, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("clip_store: runtime: {e}"))?;
    Ok(rt.block_on(f))
}

/// `{days}d/{tenant}/{yyyy}/{mm}/{clip_id}.mp4`.
///
/// The retention tier is the **key prefix** so one R2 lifecycle rule per tier expires objects
/// on its own — no purge job to run, forget, or get wrong. Changing a tenant's plan changes
/// the prefix for *new* clips only: existing ones keep the retention they were stored under,
/// so we never silently hold footage longer than we promised.
pub fn object_key(
    retention_days: i32,
    tenant_id: Uuid,
    started_at_rfc3339: &str,
    clip_id: &str,
) -> String {
    // RFC3339 begins `YYYY-MM-…`; fall back to `0000/00` rather than panic on a malformed one.
    let (year, month) = match started_at_rfc3339.len() >= 7 {
        true => (&started_at_rfc3339[0..4], &started_at_rfc3339[5..7]),
        false => ("0000", "00"),
    };
    format!("{retention_days}d/{tenant_id}/{year}/{month}/{clip_id}.mp4")
}

/// A short-lived URL the edge can `PUT` the clip to, without ever holding our credentials.
///
/// Blocking: drives a throwaway runtime, so it must not be called from inside an async task
/// (tokio panics). Wrap in `spawn_blocking` from an axum handler.
pub fn presign_put(key: &str, expiry_secs: u32) -> Result<String, String> {
    let b = bucket().ok_or("clip store not configured")?;
    block_on(b.presign_put(key, expiry_secs, None, None))?
        .map_err(|e| format!("clip_store: presign put: {e}"))
}

/// A short-lived URL the browser can stream the clip from. R2 charges no egress.
///
/// The bucket still needs a CORS policy exposing `Content-Range`/`Accept-Ranges`, or the
/// player will render the video but refuse to seek — presigning does not exempt a browser
/// from CORS.
pub fn presign_get(key: &str, expiry_secs: u32) -> Result<String, String> {
    let b = bucket().ok_or("clip store not configured")?;
    block_on(b.presign_get(key, expiry_secs, None))?
        .map_err(|e| format!("clip_store: presign get: {e}"))
}

/// Remove one object. Idempotent, like every S3 delete.
///
/// Retention expiry is the lifecycle's job; this is the **customer-initiated** deletion, which
/// must be immediate. R2 removes lifecycle-expired objects "usually within 24 hours" — fine for
/// a retention promise, not for a data-subject request.
///
/// Goes through a presigned URL rather than `delete_object`: the crate signs a bodyless DELETE
/// with an empty `content-length` and `content-type: text/plain`, which R2 rejects with
/// `SignatureDoesNotMatch` (and, per the note on [`ok_status`], reports as success). Presigned
/// DELETE signs the URL instead of the headers, and R2 accepts it.
pub fn delete(key: &str) -> Result<(), String> {
    let b = bucket().ok_or("clip store not configured")?;
    let url = block_on(b.presign_delete(key, 60))?
        .map_err(|e| format!("clip_store: presign delete: {e}"))?;
    match ureq::delete(&url).call() {
        Ok(_) => Ok(()),
        // Already gone is success — a purge must be safe to retry.
        Err(ureq::Error::Status(404, _)) => Ok(()),
        Err(ureq::Error::Status(code, r)) => {
            let detail: String = r
                .into_string()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect();
            Err(format!("clip_store: delete: HTTP {code} {detail}"))
        }
        Err(e) => Err(format!("clip_store: delete: {e}")),
    }
}

/// Delete every object belonging to a tenant, across all retention tiers (card #64, Phase 3).
///
/// The key is `{days}d/{tenant}/…`, so a tenant's footage is spread under one prefix per tier
/// it has ever used. We list each `{d}d/{tenant}/` prefix and delete what's there — paging
/// through continuation tokens so a tenant with thousands of clips is fully cleared, not just
/// the first page. Returns how many objects were removed.
///
/// This is **not** wired to subscription cancellation: cancelling isn't a deletion request, and
/// the per-tier lifecycle already expires a lapsed tenant's clips within their retention window.
/// This is the immediate, on-demand purge — a customer's "delete my data", or a tenant removed
/// for good — and it must reach the object store, not just the DB row.
///
/// Idempotent and safe to retry: a missing object counts as already-deleted.
pub fn purge_tenant(tenant_id: uuid::Uuid, tiers: &[i32]) -> Result<u64, String> {
    let b = bucket().ok_or("clip store not configured")?;
    let mut removed = 0u64;
    for d in tiers.iter().copied().filter(|d| *d > 0) {
        let prefix = format!("{d}d/{tenant_id}/");
        let mut token: Option<String> = None;
        loop {
            // No delimiter: recurse into `…/{yyyy}/{mm}/` so every clip is seen, not just the
            // year "folders". 1000 keys/page is R2's max.
            let (page, code) =
                block_on(b.list_page(prefix.clone(), None, token.clone(), None, Some(1000)))?
                    .map_err(|e| format!("clip_store: list {prefix}: {e}"))?;
            if !(200..300).contains(&code) {
                return Err(format!("clip_store: list {prefix}: HTTP {code}"));
            }
            for obj in &page.contents {
                delete(&obj.key)?;
                removed += 1;
            }
            match page.next_continuation_token {
                Some(t) => token = Some(t),
                None => break,
            }
        }
    }
    Ok(removed)
}

/// One expiry rule per retention tier, keyed to the `{days}d/` prefix.
///
/// Idempotent: `PutBucketLifecycleConfiguration` replaces the whole configuration — which is
/// also why nobody should hand-add rules in the dashboard, as this would erase them.
///
/// Applied from code, not from the deploy config: the VPS compose is hand-maintained and never
/// receives the repo's copy, so a rule declared there would simply never exist in production.
///
/// Signed by hand rather than through `rust-s3`: its `PutBucketLifecycle` command omits
/// `x-amz-content-sha256`, which R2 requires, and the crate reports the resulting 400 as
/// success. A lifecycle that silently never applied means we keep customers' footage forever.
pub fn ensure_retention_lifecycle(tiers: &[i32]) -> Result<(), String> {
    let days: Vec<i32> = tiers.iter().copied().filter(|d| *d > 0).collect();
    if days.is_empty() {
        return Ok(());
    }
    let mut xml = String::from("<LifecycleConfiguration>");
    for d in &days {
        xml.push_str(&format!(
            "<Rule><ID>retain-{d}d</ID><Status>Enabled</Status>\
             <Filter><Prefix>{d}d/</Prefix></Filter>\
             <Expiration><Days>{d}</Days></Expiration></Rule>"
        ));
    }
    xml.push_str("</LifecycleConfiguration>");

    let (code, body) = signed_request("PUT", "lifecycle", xml.as_bytes())?;
    ok_status("put lifecycle", code, body.as_bytes())?;

    // Read it back. "Applied" has to mean applied — see the note above.
    let (code, got) = signed_request("GET", "lifecycle", b"")?;
    ok_status("get lifecycle", code, got.as_bytes())?;
    for d in &days {
        if !got.contains(&format!("retain-{d}d")) {
            ARMED.store(false, Ordering::Relaxed);
            return Err(format!(
                "clip_store: lifecycle rule 'retain-{d}d' did not apply"
            ));
        }
    }
    ARMED.store(true, Ordering::Relaxed);
    Ok(())
}

/// A SigV4-signed request against the bucket itself (subresources like `?lifecycle`).
///
/// Returns `(status, body)` — the caller decides what a non-2xx means.
fn signed_request(method: &str, subresource: &str, body: &[u8]) -> Result<(u16, String), String> {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    type H = Hmac<Sha256>;

    let endpoint = env("R2_ENDPOINT").ok_or("clip store not configured")?;
    let bucket_name = env("R2_BUCKET").unwrap_or_else(|| "sentinel-recordings".to_string());
    let access = env("R2_ACCESS_KEY_ID").ok_or("clip store not configured")?;
    let secret = env("R2_SECRET_ACCESS_KEY").ok_or("clip store not configured")?;
    let region = env("R2_REGION").unwrap_or_else(|| "auto".to_string());

    let host = endpoint
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let datestamp = now.format("%Y%m%d").to_string();
    let payload_hash = hex::encode(Sha256::digest(body));

    // Path-style: the bucket is the first path segment.
    let canonical_uri = format!("/{bucket_name}");
    // A valueless subresource still canonicalises as `key=`.
    let canonical_query = format!("{subresource}=");
    let canonical_headers =
        format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";

    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!("{datestamp}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let mac = |key: &[u8], msg: &[u8]| -> Vec<u8> {
        let mut m = <H as Mac>::new_from_slice(key).expect("hmac key");
        m.update(msg);
        m.finalize().into_bytes().to_vec()
    };
    let k_date = mac(format!("AWS4{secret}").as_bytes(), datestamp.as_bytes());
    let k_region = mac(&k_date, region.as_bytes());
    let k_service = mac(&k_region, b"s3");
    let k_signing = mac(&k_service, b"aws4_request");
    let signature = hex::encode(mac(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );
    let url = format!(
        "{}/{bucket_name}?{subresource}",
        endpoint.trim_end_matches('/')
    );

    let req = ureq::request(method, &url)
        .set("Authorization", &authorization)
        .set("x-amz-content-sha256", &payload_hash)
        .set("x-amz-date", &amz_date);

    let resp = match method {
        "PUT" => req.send_bytes(body),
        _ => req.call(),
    };
    match resp {
        Ok(r) => Ok((r.status(), r.into_string().unwrap_or_default())),
        Err(ureq::Error::Status(c, r)) => Ok((c, r.into_string().unwrap_or_default())),
        Err(e) => Err(format!("clip_store: {method} ?{subresource}: {e}")),
    }
}

/// The lifecycle configuration the bucket currently holds — for operators and probes.
pub fn lifecycle_xml() -> Result<String, String> {
    let (code, body) = signed_request("GET", "lifecycle", b"")?;
    ok_status("get lifecycle", code, body.as_bytes())?;
    Ok(body)
}
