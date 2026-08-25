//! The kinds the scheduler drives, as behaviours the engine can run.
//!
//! Twelve node kinds have their real semantics inlined in `engine.rs` as `if kind == …` blocks,
//! because a node that loops, waits or ends a run needs to *cooperate* with the scheduler and
//! there was nowhere to put that. The engine's third node shape — [`NodeStep`] — is exactly that
//! place, and this module is where they move, one file each.
//!
//! Moving one does not change this product's scheduler: it keeps its inline block until the
//! swap. What it does is make the ENGINE's scheduler run that kind correctly, which the
//! differential test measures — each kind converted turns one more `KNOWN_DIFFERENCES` entry
//! into an agreement.

pub mod cooldown;
pub mod flip_flop;
pub mod for_each;
