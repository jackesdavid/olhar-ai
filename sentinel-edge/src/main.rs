//! Thin binary entry — all the edge lives in the library crate (`lib.rs`) so it can ALSO be compiled
//! as a `staticlib` and embedded in the phone app (card #144). The appliance just runs it.
fn main() {
    sentinel_edge::run();
}
