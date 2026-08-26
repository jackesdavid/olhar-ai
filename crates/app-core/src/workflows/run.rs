//! Starting a run.
//!
//! One function, and it exists to keep a promise: nothing outside this module needs to know which
//! engine executes a graph. `sentinel-flow` asks for a graph to be run and is handed back how many
//! nodes ran; it does not name a scheduler, construct an `Entry`, or convert a payload. That is
//! what makes swapping the scheduler a change in one file rather than in every caller.

use super::domain::Graph;
use super::host::SentinelHost;
use super::node::PortValues;

/// Where a run begins.
pub enum Start {
    /// The whole graph, from every node with no incoming exec edge. A manual run, a cron fire.
    Whole,
    /// One node, carrying a payload — how an event delivery arrives and how a timer comes back.
    /// Only what that node reaches runs; everything upstream of it stays as it was.
    At(u32, PortValues),
}

/// Run a graph to completion and report how many nodes ran.
///
/// The host carries everything the world-facing half needs — the tenant, the database, the
/// observer watching, this run's variables. Errors come back as a string because that is what the
/// run record stores.
pub fn run(graph: &Graph, host: &SentinelHost, start: Start) -> Result<usize, String> {
    // The instance this run belongs to. Per-camera graphs keep one set of remembered values per
    // camera, so one camera's window cannot surface in another's.
    let payload = match &start {
        Start::Whole => PortValues::new(),
        Start::At(_, p) => p.clone(),
    };
    let instance = graph.meta.instance_scope.key_for(&payload);

    let entry = match start {
        Start::Whole => ggraph_core::Entry::default(),
        Start::At(at, payload) => ggraph_core::Entry {
            at: vec![at],
            // A closing window is the one run that resumes rather than starts. Its trailing edge
            // fires from a timer with no payload of its own, so the only way it can ever see the
            // `camera` or `clip_id` the arming run had is to read back what that run produced.
            //
            // Only then. Seeding a fresh run from an earlier one's leftovers would make stale
            // values look live and revive branches the scheduler deliberately left dead (#114).
            restore: payload.contains_key(super::domain::WINDOW_STOP_MARKER),
            payload: super::compat::to_engine_values(&payload),
        },
    };

    let outputs = ggraph_core::run(
        graph,
        super::registry::registry(),
        host,
        &entry,
        &ggraph_core::RunOptions::default(),
    )
    .map_err(|e| e.to_string())?;

    remember(graph, host, &instance, &outputs);
    Ok(outputs.len())
}

/// Write down what each node produced, so a later run of the SAME instance can read it.
///
/// This is not a checkpoint — nothing here is cleared when the run ends, and that is the point.
/// It is the last known value of each node, and two things depend on it: a `DelayTrigger`'s
/// trailing edge, which fires from a timer with no payload of its own, and the editor's live
/// value view (#94 phase 2), which shows what each node last saw.
///
/// Only nodes that RAN and produced something are written. That is what makes a re-trigger
/// correct: a re-trigger lights nothing, so `StartRecording` does not run, so the arming run's
/// `clip_id` survives untouched while the camera refreshes. Write everything instead and a
/// closing window would overwrite the very values it had just restored.
fn remember(graph: &Graph, host: &SentinelHost, instance: &str, outputs: &ggraph_core::Outputs) {
    use ggraph_core::host::Host;
    for (nid, produced) in outputs {
        if produced.is_empty() {
            continue;
        }
        let json = ggraph_core::codec::encode_ports(produced, host.io());
        host.state().set(
            &ggraph_core::exec::values_key(graph.id, *nid, instance),
            &json,
        );
    }
}
