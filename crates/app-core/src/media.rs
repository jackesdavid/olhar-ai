//! S3-compatible object store (minio) for notification media — snapshot JPEGs and
//! recording mp4s that ride a notification. Env-configured; when unset, [`enabled`] is false
//! and callers fall back (text-only). Blocking API (called from the sync enqueue/consumer paths).
//!
//!   S3_ENDPOINT  — e.g. http://sentinel-minio:9000
//!   S3_BUCKET    — default "sentinel-media"
//!   S3_KEY / S3_SECRET — access credentials
//!   S3_REGION    — default "us-east-1"
use std::sync::OnceLock;

use s3::creds::Credentials;
use s3::serde_types::{BucketLifecycleConfiguration, Expiration, LifecycleRule};
use s3::{Bucket, Region};

fn bucket() -> Option<&'static Bucket> {
    static B: OnceLock<Option<Box<Bucket>>> = OnceLock::new();
    B.get_or_init(build).as_deref()
}

fn build() -> Option<Box<Bucket>> {
    let endpoint = std::env::var("S3_ENDPOINT").ok().filter(|s| !s.trim().is_empty())?;
    let name = std::env::var("S3_BUCKET").unwrap_or_else(|_| "sentinel-media".to_string());
    let key = std::env::var("S3_KEY").ok().filter(|s| !s.trim().is_empty())?;
    let secret = std::env::var("S3_SECRET").ok().filter(|s| !s.trim().is_empty())?;
    let region = Region::Custom {
        region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
        endpoint,
    };
    let creds = match Credentials::new(Some(&key), Some(&secret), None, None, None) {
        Ok(c) => c,
        Err(e) => {
            log::error!("media: credentials init failed: {e}");
            return None;
        }
    };
    match Bucket::new(&name, region, creds) {
        Ok(b) => Some(b.with_path_style()),
        Err(e) => {
            log::error!("media: bucket init failed: {e}");
            None
        }
    }
}

/// Whether an object store is configured (media handoff available).
pub fn enabled() -> bool {
    bucket().is_some()
}

/// Store bytes; returns the object key (`uuid.ext`).
pub fn put(bytes: &[u8], content_type: &str, ext: &str) -> Result<String, String> {
    let b = bucket().ok_or("object store not configured")?;
    let key = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    b.put_object_with_content_type_blocking(&key, bytes, content_type)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

/// Fetch bytes by key.
pub fn get(key: &str) -> Result<Vec<u8>, String> {
    let b = bucket().ok_or("object store not configured")?;
    let resp = b.get_object_blocking(key).map_err(|e| e.to_string())?;
    Ok(resp.bytes().to_vec())
}

/// Remove an object.
///
/// This store is a **staging area between features**, not durable storage: media is uploaded,
/// handed to a recipient once, and then has no reason to exist. Nothing reclaimed it before,
/// so the bucket grew without bound on the same disk as Postgres. Deleting a key that is
/// already gone is a success (S3 delete is idempotent) — a purge must be safe to retry.
pub fn delete(key: &str) -> Result<(), String> {
    let b = bucket().ok_or("object store not configured")?;
    b.delete_object_blocking(key).map_err(|e| e.to_string())?;
    Ok(())
}

/// Expire every object after `days`, as a backstop for orphans.
///
/// The prompt reclaim happens in `sentinel-notification` the moment a notification goes
/// terminal. This catches what that can't see: an object staged by `put` whose enqueue then
/// failed, so no row ever pointed at it. Set from code rather than the compose file — the
/// VPS compose is hand-maintained and never receives the repo's copy, so a rule declared
/// there would simply never exist in production.
///
/// Idempotent: `PutBucketLifecycleConfiguration` replaces the whole configuration.
pub fn ensure_lifecycle(days: u32) -> Result<(), String> {
    let b = bucket().ok_or("object store not configured")?;
    let rule = LifecycleRule::builder("Enabled")
        .id("expire-staging")
        .expiration(Expiration::new(None, Some(days), None))
        .build();
    let config = BucketLifecycleConfiguration::new(vec![rule]);
    // `put_bucket_lifecycle` has no blocking variant; this runs once, off the hot path.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("media: lifecycle runtime: {e}"))?;
    rt.block_on(b.put_bucket_lifecycle(config))
        .map(|_| ())
        .map_err(|e| format!("media: put lifecycle: {e}"))
}
