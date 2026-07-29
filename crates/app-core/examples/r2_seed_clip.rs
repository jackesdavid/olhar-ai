//! Seed one real clip into R2 exactly as the edge uploader would, so cloud playback (#64
//! Phase 2) can be exercised before an edge box runs the 0.5.36 uploader.
//!
//!   set -a; source .env; set +a
//!   cargo run -p app-core --example r2_seed_clip -- <mp4> <retention_days> <tenant_uuid> <clip_id> <started_at_rfc3339>
//!
//! Prints the object key on success. Pair it with an `UPDATE recordings SET cloud_key=…,
//! upload_state='uploaded'` so the row matches the object.

use std::io::Read;

struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, r: &log::Record) {
        eprintln!("{}: {}", r.level(), r.args());
    }
    fn flush(&self) {}
}
static LOG: StderrLog = StderrLog;

fn main() {
    let _ = log::set_logger(&LOG);
    log::set_max_level(log::LevelFilter::Warn);

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        eprintln!("usage: r2_seed_clip <mp4> <retention_days> <tenant_uuid> <clip_id> <started_at_rfc3339>");
        std::process::exit(2);
    }
    let (path, days, tenant, clip_id, started_at) =
        (&args[1], &args[2], &args[3], &args[4], &args[5]);

    use app_core::clip_store as cs;
    if !cs::enabled() {
        eprintln!("R2 not configured");
        std::process::exit(1);
    }
    let tenant = uuid::Uuid::parse_str(tenant).expect("tenant uuid");
    let days: i32 = days.parse().expect("retention_days");

    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .expect("open mp4")
        .read_to_end(&mut bytes)
        .expect("read mp4");

    let key = cs::object_key(days, tenant, started_at, clip_id);
    let put_url = cs::presign_put(&key, 300).expect("presign put");

    // Same as the edge: explicit Content-Length so ureq streams instead of chunking (R2 rejects
    // chunked on a presigned PUT).
    let status = ureq::put(&put_url)
        .set("Content-Type", "video/mp4")
        .set("Content-Length", &bytes.len().to_string())
        .send_bytes(&bytes)
        .map(|r| r.status())
        .unwrap_or_else(|e| panic!("upload failed: {e}"));

    println!("uploaded HTTP {status}, {} bytes", bytes.len());
    println!("key {key}");
}
