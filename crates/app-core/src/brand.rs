//! The product's name **as customers see it** (card #76).
//!
//! The codebase, the services, the repo and the crates stay "Sentinel" — renaming them buys
//! nothing and breaks every deploy path. Only the COPY is Olhar AI: anything a customer can read.
//!
//! One constant, because the alternative was proven not to work. The rename left the name spelled
//! out in string literals across the app, and they drifted one at a time: a `402` body, a Telegram
//! test message, and eighteen strings in the web app were still saying "Sentinel" months later —
//! the billing banner among them, which is the sentence a customer reads at the worst moment.
//!
//! If you are writing a sentence a customer will read, use this. If you are naming a service, a
//! container, a crate or a log line, do not.
pub const PRODUCT: &str = "Olhar AI";
