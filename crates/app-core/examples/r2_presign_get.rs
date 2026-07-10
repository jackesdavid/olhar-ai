//! Print a presigned GET URL for a key — to verify a seeded clip streams (and CORS is right).
//!   set -a; source .env; set +a
//!   cargo run -p app-core --example r2_presign_get -- <object_key>
fn main() {
    let key = std::env::args().nth(1).expect("usage: r2_presign_get <object_key>");
    match app_core::clip_store::presign_get(&key, 600) {
        Ok(url) => println!("{url}"),
        Err(e) => { eprintln!("{e}"); std::process::exit(1); }
    }
}
