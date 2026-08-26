//! Holding back what a pure source reports until somebody reads it.
//!
//! A pure source — `GetVariable`, `GetCameraByName`, `GetRecipient`, `GetTuyaDevice` — activates
//! on **every** run and every epoch, whether or not the exec branch it feeds is ever reached. That
//! is load-bearing (a sub-run entering halfway still needs its sources seeded, #114), and it is
//! also why one lights up on the canvas and writes a Console line on a branch nobody took (#115,
//! #205).
//!
//! The engine cannot decide this at the moment the source runs: sources run producer-before-
//! consumer, so at that point it is not yet known whether the consumer will run at all. So the
//! decision is deferred, and this observer is where it lives — between the scheduler and whatever
//! actually paints the canvas and writes the log.
//!
//! **Two reports, two moments, and the difference is deliberate.**
//!
//! - The *ping* that lights the node is released the instant a node reading it starts — which is
//!   the moment it becomes true that the branch was taken. Any later and the highlight arrives
//!   after the thing it was supposed to precede.
//! - The *line* in the Console is released at the end of the run, in activation order. It has been
//!   emitted there since #115 and moving it would reorder the log of every existing graph.
//!
//! Both are the same rule — *report a source only if something read it* — carried out at two
//! different times. Collapsing them into one moment is worth doing, but as its own change, with
//! its own before-and-after: doing it inside the scheduler swap would make any complaint about
//! the Console impossible to attribute.

use ggraph_core::host::{Observer, UiEvent};
use ggraph_core::{Graph, GraphMeta, GraphNode, PortValues};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// Wraps an observer and holds back what pure sources report until a reader runs.
pub struct DeferringObserver<O: Observer> {
    inner: O,
    /// The nodes whose reports are held.
    deferred: HashSet<u32>,
    /// Reader → the deferred nodes it reads. Drives the eager release of pings.
    feeders: HashMap<u32, Vec<u32>>,
    /// Deferred node → the nodes that read it. Drives the end-of-run release of log lines.
    readers: HashMap<u32, Vec<u32>>,
    held: Mutex<Held>,
}

#[derive(Default)]
struct Held {
    /// Pings waiting for a reader. Re-armable: a source in a loop lights once per epoch, which is
    /// what makes a loop legible on the canvas.
    pings: Vec<u32>,
    /// Log lines waiting for the end of the run. Held once per run — a source in a loop writes one
    /// line, not one per iteration, which is what keeps the Console readable.
    lines: Vec<(u32, String, u128)>,
    /// Which sources have already been buffered, so the once-per-run rule survives re-activation.
    buffered: HashSet<u32>,
    /// Every node seen to start, held or not. This is the observer's own answer to "did it run?",
    /// which is the question the end-of-run release asks.
    ran: HashSet<u32>,
}

impl<O: Observer> DeferringObserver<O> {
    /// Build from the graph, deferring every node `is_deferred` accepts.
    ///
    /// The predicate is the caller's, not this module's: which kinds are pure sources is a fact
    /// about the product's catalog, and an observer that knew it could not be tested without one.
    pub fn new<M: GraphMeta>(
        graph: &Graph<M>,
        inner: O,
        is_deferred: impl Fn(&GraphNode) -> bool,
    ) -> Self {
        let deferred: HashSet<u32> = graph
            .nodes
            .iter()
            .filter(|n| is_deferred(n))
            .map(|n| n.id)
            .collect();

        let mut feeders: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut readers: HashMap<u32, Vec<u32>> = HashMap::new();
        for e in &graph.edges {
            if deferred.contains(&e.from) {
                feeders.entry(e.to).or_default().push(e.from);
                readers.entry(e.from).or_default().push(e.to);
            }
        }

        DeferringObserver {
            inner,
            deferred,
            feeders,
            readers,
            held: Mutex::new(Held::default()),
        }
    }

    /// The wrapped observer, for a caller that kept no other handle on it.
    pub fn inner(&self) -> &O {
        &self.inner
    }
}

impl<O: Observer> Observer for DeferringObserver<O> {
    fn node_started(&self, node: u32) {
        let release: Vec<u32> = {
            let mut h = self.held.lock().unwrap();
            h.ran.insert(node);

            if self.deferred.contains(&node) {
                if !h.pings.contains(&node) {
                    h.pings.push(node);
                }
                return;
            }

            // A node is starting, so every held source it reads has now been read. Release those
            // pings first, so the source lights just before the node that consumed it.
            let mut out = Vec::new();
            for f in self.feeders.get(&node).into_iter().flatten() {
                if let Some(i) = h.pings.iter().position(|p| p == f) {
                    h.pings.remove(i);
                    out.push(*f);
                }
            }
            out
        };

        for f in release {
            self.inner.node_started(f);
        }
        self.inner.node_started(node);
    }

    fn node_finished(&self, node: u32, summary: &str, elapsed_ms: u128) {
        if self.deferred.contains(&node) {
            let mut h = self.held.lock().unwrap();
            if h.buffered.insert(node) {
                h.lines.push((node, summary.to_string(), elapsed_ms));
            }
            return;
        }
        self.inner.node_finished(node, summary, elapsed_ms);
    }

    fn emitted(&self, event: &str, payload: &PortValues) {
        self.inner.emitted(event, payload);
    }

    fn ui(&self, node: u32, event: UiEvent) {
        self.inner.ui(node, event);
    }

    fn run_finished(&self) {
        let lines = {
            let mut h = self.held.lock().unwrap();
            let ran = std::mem::take(&mut h.ran);
            let lines = std::mem::take(&mut h.lines);
            h.pings.clear();
            h.buffered.clear();
            lines
                .into_iter()
                .filter(|(n, _, _)| {
                    self.readers
                        .get(n)
                        .is_some_and(|rs| rs.iter().any(|r| ran.contains(r)))
                })
                .collect::<Vec<_>>()
        };

        // A source whose readers all sat out — or that has no readers at all — is simply dropped.
        for (n, summary, ms) in lines {
            self.inner.node_finished(n, &summary, ms);
        }
        self.inner.run_finished();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::domain::{GraphExt, NodeKind};
    use crate::workflows::host::SentinelMeta;

    /// Records what actually reached the far side, in order. Order is half the contract: a
    /// highlight that arrives after the node it was meant to precede is still the wrong picture.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Observer for Recorder {
        fn node_started(&self, node: u32) {
            self.0.lock().unwrap().push(format!("start:{node}"));
        }
        fn node_finished(&self, node: u32, _summary: &str, _ms: u128) {
            self.0.lock().unwrap().push(format!("line:{node}"));
        }
    }

    impl Recorder {
        fn seen(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    fn deferring(g: &Graph<SentinelMeta>) -> DeferringObserver<Recorder> {
        DeferringObserver::new(g, Recorder::default(), |n| {
            crate::workflows::domain::AsKind::as_kind(&n.kind).is_some_and(|k| k.is_pure_source())
        })
    }

    /// One node's whole visit, the way the scheduler reports it.
    fn visit(o: &DeferringObserver<Recorder>, node: u32) {
        o.node_started(node);
        o.node_finished(node, "summary", 1);
    }

    #[test]
    fn a_source_feeding_a_dead_branch_is_never_reported() {
        // #115 and #205, as one shape: `If` lights one branch, each branch has its own Get feeding
        // its own Format. Both Gets ACTIVATE — that is load-bearing — but only one was read.
        let mut g: Graph<SentinelMeta> = Graph::new("g");
        let gate = g.add(NodeKind::If, 0, 0);
        let get_dead = g.add(NodeKind::GetVariable, 0, 0);
        let fmt_dead = g.add(NodeKind::Format, 0, 0);
        let get_live = g.add(NodeKind::GetVariable, 0, 0);
        let fmt_live = g.add(NodeKind::Format, 0, 0);
        g.wire(gate, "true", fmt_dead, "exec_in").unwrap();
        g.wire(get_dead, "value", fmt_dead, "p1").unwrap();
        g.wire(gate, "false", fmt_live, "exec_in").unwrap();
        g.wire(get_live, "value", fmt_live, "p1").unwrap();

        let o = deferring(&g);
        visit(&o, gate);
        visit(&o, get_dead); // activates, is never read
        visit(&o, get_live);
        visit(&o, fmt_live); // the false branch is the one that ran
        o.run_finished();

        let seen = o.inner().seen();
        assert!(
            !seen.iter().any(|s| s.ends_with(&format!(":{get_dead}"))),
            "the source feeding the dead branch must neither light nor log, got {seen:?}"
        );
        assert!(
            seen.contains(&format!("start:{get_live}"))
                && seen.contains(&format!("line:{get_live}")),
            "the source that was read must do both, got {seen:?}"
        );
    }

    #[test]
    fn the_ping_lands_just_before_the_node_that_read_it() {
        // The highlight exists to say "this value came from here, now". Released after its reader
        // it says it about the wrong moment.
        let mut g: Graph<SentinelMeta> = Graph::new("g");
        let get = g.add(NodeKind::GetVariable, 0, 0);
        let fmt = g.add(NodeKind::Format, 0, 0);
        g.wire(get, "value", fmt, "p1").unwrap();

        let o = deferring(&g);
        visit(&o, get);
        visit(&o, fmt);
        o.run_finished();

        assert_eq!(
            o.inner().seen(),
            vec![
                format!("start:{get}"),
                format!("start:{fmt}"),
                format!("line:{fmt}"),
                format!("line:{get}"),
            ],
            "the ping precedes its reader; the Console line waits for the end of the run"
        );
    }

    #[test]
    fn a_source_nobody_reads_stays_quiet() {
        let mut g: Graph<SentinelMeta> = Graph::new("g");
        let get = g.add(NodeKind::GetVariable, 0, 0);

        let o = deferring(&g);
        visit(&o, get);
        o.run_finished();

        assert!(
            o.inner().seen().is_empty(),
            "a read nobody consumed is not worth a line or a highlight"
        );
    }

    #[test]
    fn a_source_in_a_loop_lights_every_pass_but_writes_one_line() {
        // The two rules differ on purpose. Re-lighting each pass is what makes a loop legible on
        // the canvas; one line per pass is what makes the Console useless.
        let mut g: Graph<SentinelMeta> = Graph::new("g");
        let get = g.add(NodeKind::GetVariable, 0, 0);
        let fmt = g.add(NodeKind::Format, 0, 0);
        g.wire(get, "value", fmt, "p1").unwrap();

        let o = deferring(&g);
        for _ in 0..3 {
            visit(&o, get);
            visit(&o, fmt);
        }
        o.run_finished();

        let seen = o.inner().seen();
        assert_eq!(
            seen.iter()
                .filter(|s| **s == format!("start:{get}"))
                .count(),
            3,
            "one highlight per pass"
        );
        assert_eq!(
            seen.iter().filter(|s| **s == format!("line:{get}")).count(),
            1,
            "one Console line for the whole run"
        );
    }
}
