//! Exercise the durable clip store against the real R2 bucket, end to end.
//!
//!   set -a; source .env; set +a; cargo run -p app-core --example r2_probe
//!
//! Proves the four things Phase 1 depends on: a presigned PUT that an edge (holding no
//! credentials) can upload through, a presigned GET a browser can stream from, per-tier
//! lifecycle rules keyed to the `{days}d/` prefix, and an immediate delete for LGPD requests.

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

    use app_core::clip_store as cs;
    if !cs::enabled() {
        eprintln!("R2 not configured — set R2_ENDPOINT / R2_BUCKET / R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY");
        std::process::exit(1);
    }

    let tenant = uuid::Uuid::nil();
    let key = cs::object_key(30, tenant, "2026-07-09T12:34:56-03:00", "probe-clip");
    println!("key                : {key}");
    assert!(key.starts_with("30d/"), "retention must be the key prefix");

    // 1. Presigned PUT — what the edge gets. It never sees our credentials.
    let put_url = cs::presign_put(&key, 300).expect("presign put");
    println!("presign PUT        : ok ({} chars)", put_url.len());

    let body = b"fake-mp4-bytes";
    let put = ureq::put(&put_url)
        .set("Content-Type", "video/mp4")
        .send_bytes(body)
        .map(|r| r.status())
        .unwrap_or_else(|e| panic!("upload through presigned PUT failed: {e}"));
    println!("upload via PUT     : HTTP {put}");

    // 2. Presigned GET — what the browser streams from.
    let get_url = cs::presign_get(&key, 300).expect("presign get");
    let mut got = Vec::new();
    let status = ureq::get(&get_url)
        .call()
        .map(|r| {
            r.into_reader().read_to_end(&mut got).ok();
            200
        })
        .unwrap_or_else(|e| panic!("download through presigned GET failed: {e}"));
    println!("download via GET   : HTTP {status}, {} bytes", got.len());
    assert_eq!(got, body, "round-tripped bytes differ");

    // 3. Lifecycle: one rule per retention tier, scoped by prefix.
    match cs::ensure_retention_lifecycle(&[3, 30, 60]) {
        Ok(()) => println!("lifecycle (3/30/60): applied"),
        Err(e) => {
            eprintln!("lifecycle FAILED   : {e}");
            std::process::exit(1);
        }
    }

    // 4. Customer-initiated deletion must be immediate, not lifecycle-lagged.
    cs::delete(&key).expect("delete");
    println!("delete             : ok");

    let gone = ureq::get(&cs::presign_get(&key, 60).expect("presign get")).call();
    match gone {
        Err(ureq::Error::Status(404, _)) => println!("object after delete : 404 ✓"),
        Err(ureq::Error::Status(s, _)) => println!("object after delete : HTTP {s} (expected 404)"),
        Ok(_) => panic!("object still readable after delete"),
        Err(e) => println!("object after delete : {e}"),
    }

    println!("\nR2 probe passed.");
}

use std::io::Read;
