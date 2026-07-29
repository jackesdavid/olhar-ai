//! Smoke-test the native YOLOX detector against a real image file.
//! Usage: `cargo run --example detect -- <image.jpg>`
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: detect <image.jpg>");
    let bytes = std::fs::read(&path).expect("read image");

    let t0 = Instant::now();
    let dets = app_core::agent::detector::detect(&bytes).expect("detect");
    let elapsed = t0.elapsed();

    println!("{} detections in {:?}", dets.len(), elapsed);
    for d in &dets {
        let [x1, y1, x2, y2] = d.xyxy;
        println!(
            "  class={:>2} score={:.3}  box=[{:.0},{:.0},{:.0},{:.0}]",
            d.class, d.score, x1, y1, x2, y2
        );
    }

    let p = app_core::agent::detector::detect_person(&bytes).expect("detect_person");
    println!(
        "person={} count={} confidence={:.3}",
        p.person, p.count, p.confidence
    );

    // Warm latency: model + graph are now built, so this is steady-state per-frame cost.
    let t1 = Instant::now();
    let _ = app_core::agent::detector::detect(&bytes).expect("detect");
    println!("warm inference: {:?}", t1.elapsed());
}
