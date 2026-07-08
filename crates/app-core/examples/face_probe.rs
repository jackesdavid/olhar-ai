//! Smoke-test the real face pipeline against the downloaded ONNX models.
//! Run: `cargo run -p app-core --example face_probe`
//! (Expects scrfd.onnx + arcface.onnx under the models dir set below.)

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
    log::set_max_level(log::LevelFilter::Info);

    // Point at the app-data models dir where the ONNX files were downloaded.
    let home = std::env::var("HOME").unwrap();
    let dir = format!("{home}/Library/Application Support/com.jackesdavid.sentinel/models");
    std::env::set_var("SENTINEL_MODELS_DIR", &dir);
    eprintln!("models dir: {dir}");

    // A synthetic frame (no real face) — validates that the models load, run, and the
    // SCRFD decode + ArcFace produce a 512-d unit vector without panicking. Real-face
    // accuracy needs a real photo; this catches load/shape/decode errors.
    let img = image::RgbImage::from_fn(320, 240, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .unwrap();

    // Face embedding now runs on the GPU sidecar and meters usage per tenant, so it
    // needs a DB handle + tenant. Connect via DATABASE_URL (skip cleanly if unset).
    let db = match std::env::var("DATABASE_URL").ok().and_then(|u| app_core::db::Db::connect(&u).ok()) {
        Some(db) => db,
        None => {
            eprintln!("set DATABASE_URL to run this probe (embed records ai_usage per tenant)");
            return;
        }
    };
    let tenant = app_core::tenancy::DEFAULT_TENANT_ID;

    // Twice: 1st pays the one-time model optimisation; 2nd is steady-state per-call.
    for pass in 1..=2 {
        let t = std::time::Instant::now();
        match app_core::agent::face::embed(&db, tenant, &bytes) {
            Ok(v) => {
                let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                eprintln!("[pass {pass}] embed OK: dims={}, norm={:.4}, TOTAL {:?}", v.len(), norm, t.elapsed());
            }
            Err(e) => eprintln!("[pass {pass}] embed FAILED: {e}"),
        }
    }
}
