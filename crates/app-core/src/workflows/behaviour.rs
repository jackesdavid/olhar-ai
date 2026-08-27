//! What a graph does, end to end.
//!
//! These ran against this product's own scheduler for as long as it existed. They are not about
//! that scheduler — they are about what a switch routes to, what a loop iterates, which arm of a
//! branch stays dead — so when the scheduler was replaced they were repointed rather than deleted,
//! and they now exercise the one that actually runs in production.
//!
//! Everything goes through the same door a real run does: `workflows::run`, the engine's
//! scheduler, this product's registry, the deferring observer. A test that reached past that would
//! be testing a path nobody uses.

#![cfg(test)]

use super::domain::{types, ConcurrencyPolicy, NodeExt};
use super::domain::{Graph, GraphExt, NodeKind};
use super::gate::{Fire, WorkflowEngine};
use super::golden::test_ctx;
use super::host::SentinelHost;
use super::node::parse_text_items;
use super::node::{PortValue, PortValues};
use super::steps::on_event::on_event_node;
use crate::agent::tools::{ToolContext, ToolUiEvent};
use ggraph_core::host::{Observer, UiEvent};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// An event bus that only remembers. Enough to prove a node found one and used it.
struct Recording(Arc<Mutex<Vec<String>>>);

impl crate::workflows::emit::EventEmitter for Recording {
    fn emit(&self, _graph: uuid::Uuid, event: &str, _run: uuid::Uuid, _payload: &PortValues) {
        self.0.lock().unwrap().push(event.to_string());
    }
}

/// Records what reached a person: which nodes lit, and what the Console said.
#[derive(Default)]
struct Recorder {
    nodes: Mutex<Vec<u32>>,
    lines: Mutex<Vec<(u32, String)>>,
    debug: bool,
}

impl Observer for Recorder {
    /// A declaration defect fails the test where it happens, not three screens later.
    ///
    /// The engine reports rather than asserts, because it cannot pick the severity for every
    /// consumer. Here the severity is obvious: a node whose declared ports disagree with what it
    /// does is a bug we want to hear about while writing it.
    fn defect(&self, node: u32, message: &str) {
        panic!("node {node}: {message}");
    }
    fn node_started(&self, node: u32) {
        self.nodes.lock().unwrap().push(node);
    }
    fn node_finished(&self, node: u32, summary: &str, _ms: u128) {
        // The debug gate lives in the product's observer, so a test that leaves debug off must see
        // no lines — exactly as a graph with debug off produces none.
        if self.debug {
            self.lines.lock().unwrap().push((node, summary.to_string()));
        }
    }
    fn ui(&self, _node: u32, _event: UiEvent) {}
}

/// A `Box<dyn Observer>` needs an owned value; the test needs to read the recording afterwards.
struct Shared(Arc<Recorder>);

impl Observer for Shared {
    fn defect(&self, node: u32, message: &str) {
        self.0.defect(node, message);
    }
    fn node_started(&self, node: u32) {
        self.0.node_started(node);
    }
    fn node_finished(&self, node: u32, summary: &str, ms: u128) {
        self.0.node_finished(node, summary, ms);
    }
    fn arm(&self, node: u32, port: &str) {
        self.0.arm(node, port);
    }
    fn ui(&self, node: u32, event: UiEvent) {
        self.0.ui(node, event);
    }
    fn run_finished(&self) {
        self.0.run_finished();
    }
}

/// Run a graph the way production does, and report which nodes lit and what was logged.
fn run_capture(g: &Graph, ctx: &ToolContext) -> (Vec<u32>, Vec<(u32, String)>) {
    run_from(g, ctx, None)
}

/// What every node produced, for a test that cares about values rather than order.
fn run_outputs(g: &Graph, ctx: &ToolContext) -> Result<HashMap<u32, PortValues>, String> {
    outputs_from(g, ctx, None)
}

/// The same, entering at one node with a payload.
fn outputs_from(
    g: &Graph,
    ctx: &ToolContext,
    entry: Option<(u32, PortValues)>,
) -> Result<HashMap<u32, PortValues>, String> {
    let host = SentinelHost::new(ctx.clone());
    let start = match entry {
        Some((at, payload)) => super::Start::At(at, payload),
        None => super::Start::Whole,
    };
    let outs = super::run_graph(g, &host, start)?;
    Ok(outs
        .iter()
        .map(|(id, ports)| (*id, super::compat::from_engine_values(ports)))
        .collect())
}

/// The same, entering at one node with a payload — an event delivery, a timer coming back.
fn run_from(
    g: &Graph,
    ctx: &ToolContext,
    entry: Option<(u32, PortValues)>,
) -> (Vec<u32>, Vec<(u32, String)>) {
    let rec = Arc::new(Recorder {
        debug: g.meta.debug,
        ..Default::default()
    });
    let watcher = super::observer::DeferringObserver::new(g, Shared(rec.clone()), |n| {
        crate::workflows::domain::AsKind::as_kind(&n.kind).is_some_and(|k| k.is_pure_source())
    });
    let host = SentinelHost::watching(ctx.clone(), Box::new(watcher));
    let start = match entry {
        Some((at, payload)) => super::Start::At(at, payload),
        None => super::Start::Whole,
    };
    super::run_graph(g, &host, start).expect("the graph runs");
    let nodes = rec.nodes.lock().unwrap().clone();
    let lines = rec.lines.lock().unwrap().clone();
    (nodes, lines)
}

use crate::agent::detector::{PersonDetection, PersonResult};
use crate::ports::Vision;

/// A context with no database behind it.
///
/// `Db::lazy` never connects, and every `repo::*` call folds to `None`, so these run anywhere —
/// which is what lets them run in CI on every merge request rather than only where Postgres is.
fn unused_test_ctx() -> ToolContext {
    let (tx, _rx) = std::sync::mpsc::channel();
    ToolContext::new_local(
        std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        crate::db::Db::lazy("postgres://sentinel:sentinel@localhost/sentinel_test"),
        crate::tenancy::DEFAULT_TENANT_ID,
        std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        None,
        tx,
    )
}

/// A Vision stub whose `ask_yes_no` returns a fixed answer. Used to test `AskAi`
/// routing without hitting the Anthropic API.
struct StubVision(bool);
impl Vision for StubVision {
    fn detect_persons(&self, _: &[u8]) -> Result<PersonResult, String> {
        Ok(PersonResult {
            person: false,
            count: 0,
            confidence: 0.0,
        })
    }
    fn detect_persons_detailed(&self, _: &[u8]) -> Result<Vec<PersonDetection>, String> {
        Ok(vec![])
    }
    fn detect_objects_detailed(&self, _: &[u8], _: u32) -> Result<Vec<PersonDetection>, String> {
        Ok(vec![])
    }
    fn describe(&self, _: &[u8], _: &str) -> Result<String, String> {
        Ok("stub".into())
    }
    fn ask_yes_no(&self, _: &str, _: Option<&[u8]>) -> Result<bool, String> {
        Ok(self.0)
    }
    fn ask_text(&self, _: &str, _: Option<&[u8]>) -> Result<String, String> {
        Ok("stub".into())
    }
    fn classify(&self, _: &str, _: &[String]) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn extract(&self, _: &str, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
}

fn ctx_with_vision(answer: bool) -> ToolContext {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut ctx = test_ctx();
    ctx.ui_tx = tx;
    ctx.vision = std::sync::Arc::new(StubVision(answer));
    ctx
}

/// The shape that silently died: a pure source with no incoming edges, feeding a required
/// input, inside a sub-run.
///
/// An orphan falls back to `seed_entries` — false for every `OnEvent`/timer sub-path — so the
/// source never ran, the consumer's required input resolved to nothing, and the engine skipped
/// the whole branch as dead while the run reported `ok`. `GetVariable` was hand-exempted when
/// someone hit this; `GetCameraByName` wasn't, so it worked on cron/manual runs and quietly
/// did nothing on event-driven ones.
#[test]
fn a_pure_source_runs_on_a_subrun_and_keeps_its_consumer_alive() {
    let dev = crate::devices::Device {
        id: "cam-1".into(),
        ip: "10.0.0.9".into(),
        xaddr: String::new(),
        name: "Hall".into(),
        manufacturer: None,
        model: None,
        username: None,
        password: None,
        rtsp_url: None,
        rtsp_sub_url: None,
        stream_mode: None,

        ..Default::default()
    };
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = ToolContext::new_local(
        std::sync::Arc::new(std::sync::Mutex::new(vec![dev])),
        crate::db::Db::lazy("postgres://sentinel:sentinel@localhost/sentinel_test"),
        crate::tenancy::DEFAULT_TENANT_ID,
        std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        None,
        tx,
    );

    let mut g = Graph::new("g");
    let entry = g.add(NodeKind::OnEvent, 0, 0);
    g.node_mut(entry).unwrap().config["event"] = json!("motion");
    // The orphan: no incoming edges at all — its value comes purely from config.
    let src = g.add(NodeKind::GetCameraByName, 0, 0);
    g.node_mut(src).unwrap().config["camera"] = json!("Hall");
    // A consumer whose `camera` input is REQUIRED and wired to that orphan. `BreakCamera`
    // is PURE, so it has no exec pins and is reached by being READ — which is why a reader
    // hangs off it below. The earlier version of this test wired exec straight into it, a
    // wire the editor cannot draw (the pin does not exist in the catalog) and that only
    // worked because the old `add_edge` was more permissive than the palette.
    let sink = g.add(NodeKind::BreakCamera, 0, 0);
    g.wire(src, "camera", sink, "camera").unwrap();
    let reader = g.add(NodeKind::Print, 0, 0);
    g.wire(entry, "exec_out", reader, "exec_in").unwrap();
    g.wire(sink, "name", reader, "message").unwrap();

    let outs = outputs_from(&g, &ctx, Some((entry, PortValues::new()))).unwrap();

    assert!(
        outs.contains_key(&src),
        "the orphan source must run on a sub-run (it used to fall back to seed_entries=false)"
    );
    assert!(
        outs.contains_key(&sink),
        "its consumer must not be skipped as a dead branch"
    );
}

#[test]
fn if_node_runs_only_the_live_branch() {
    for (cond, want, skip) in [(true, "a", "b"), (false, "b", "a")] {
        let ctx = test_ctx();
        let mut g = Graph::new("g");
        let gate = g.add(NodeKind::If, 0, 0);
        g.node_mut(gate).unwrap().config["condition"] = json!(cond); // literal Bool
        let a = g.add(NodeKind::Format, 0, 0);
        let b = g.add(NodeKind::Format, 0, 0);
        g.wire(gate, "true", a, "exec_in").unwrap();
        g.wire(gate, "false", b, "exec_in").unwrap();
        let outs = run_outputs(&g, &ctx).unwrap();
        let chosen = if want == "a" { a } else { b };
        let skipped = if skip == "a" { a } else { b };
        assert!(outs.contains_key(&gate), "If itself runs");
        assert!(outs.contains_key(&chosen), "live branch runs (cond={cond})");
        assert!(
            !outs.contains_key(&skipped),
            "dead branch is skipped (cond={cond})"
        );
    }
}

#[test]
fn show_image_mirrors_its_input_frame_to_the_ui() {
    // A jpeg literal on the node's `image` port is read at run time; the engine
    // should then emit a ShowImage UI event tagged with that node's id.
    let path = std::env::temp_dir().join("wf_eng_show_image.jpg");
    let img = image::RgbImage::from_pixel(16, 12, image::Rgb([10, 200, 120]));
    image::DynamicImage::ImageRgb8(img)
        .save_with_format(&path, image::ImageFormat::Jpeg)
        .unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let mut ctx = test_ctx();
    ctx.ui_tx = tx;
    let mut g = Graph::new("g");
    let show = g.add(NodeKind::ShowImage, 0, 0);
    g.node_mut(show).unwrap().config = json!({ "label": "cam", "image": path.to_str().unwrap() });
    // run_orphans so the single, edgeless node participates in the run.
    g.meta.run_orphans = true;
    run_outputs(&g, &ctx).unwrap();

    match rx.try_recv() {
        Ok(ToolUiEvent::ShowImage {
            graph,
            node,
            label,
            jpeg,
        }) => {
            assert_eq!(graph, g.id);
            assert_eq!(node, show);
            assert_eq!(label, "cam");
            assert!(!jpeg.is_empty());
        }
        _ => panic!("expected a ShowImage UI event"),
    }
}

#[test]
fn exec_edge_without_if_still_runs_both() {
    // Regression: a plain exec edge (no branch) must not gate anything off.
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    let a = g.add(NodeKind::Format, 0, 0);
    let b = g.add(NodeKind::Format, 0, 0);
    g.wire(a, "exec_out", b, "exec_in").unwrap();
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(outs.contains_key(&a) && outs.contains_key(&b));
}

#[test]
fn a_back_edge_on_its_own_does_not_repeat() {
    // start → A → B, and B wires back to A.
    //
    // Under this product's previous scheduler that looped until the step budget stopped it.
    // The engine repeats only when a node ASKS to be re-entered — which is what ForEach does
    // and what an ordinary node has no way to say — so the back-edge fires its arm and the run
    // ends. Every graph in production was checked: none draws a loop this way, they all use
    // ForEach. Written down because a graph that COULD be drawn this way now behaves
    // differently, and quietly.
    let ctx = test_ctx();
    let mut g = Graph::new("loop");
    let start = g.add(NodeKind::Print, 0, 0);
    g.node_mut(start).unwrap().config =
        json!({ "message": "go", "level": "info", "on_screen": false });
    let a = g.add(NodeKind::Format, 0, 0);
    let b = g.add(NodeKind::Format, 0, 0);
    g.wire(start, "exec_out", a, "exec_in").unwrap();
    g.wire(a, "exec_out", b, "exec_in").unwrap();
    g.wire(b, "exec_out", a, "exec_in").unwrap();

    let outs = run_outputs(&g, &ctx).expect("it ends rather than spinning");
    assert!(
        outs.contains_key(&a) && outs.contains_key(&b),
        "both sides of the back-edge still run once: {outs:?}"
    );
}

#[test]
fn a_loop_with_no_way_in_runs_nothing() {
    // A → B → A and nothing else: every node has an incoming exec edge, so there is nowhere
    // for control to start and no run happens.
    //
    // This product's previous scheduler counted indegree over non-back edges, so it picked A
    // arbitrarily, spun ten thousand steps and failed on the budget. Nothing is the better
    // answer — starting at A rather than B was a coin toss — but it is a DIFFERENT answer, and
    // a graph shaped like this now completes quietly instead of reporting an error.
    let ctx = test_ctx();
    let mut g = Graph::new("closed loop");
    let a = g.add(NodeKind::Format, 0, 0);
    let b = g.add(NodeKind::Format, 0, 0);
    g.wire(a, "exec_out", b, "exec_in").unwrap();
    g.wire(b, "exec_out", a, "exec_in").unwrap();
    let outs = run_outputs(&g, &ctx).expect("it completes rather than failing");
    assert!(outs.is_empty(), "nothing ran: {outs:?}");
}

#[test]
fn add_edge_allows_exec_back_edge_but_rejects_data_cycle() {
    let mut g = Graph::new("g");
    let a = g.add(NodeKind::Format, 0, 0); // text out
    let b = g.add(NodeKind::Format, 0, 0);
    g.wire(a, "exec_out", b, "exec_in").unwrap();
    // exec back-edge → allowed (a loop).
    assert!(g.wire(b, "exec_out", a, "exec_in").is_ok());
    // data back-edge (b.text → a.p1) would create a data cycle → rejected.
    let err = g.wire(b, "text", a, "p1").unwrap_err();
    assert!(err.contains("data cycle"), "got: {err}");
}

#[test]
fn add_edge_accepts_exec_fan_in_but_rejects_duplicate_pins() {
    let mut g = Graph::new("g");
    let a = g.add(NodeKind::Format, 0, 0);
    let b = g.add(NodeKind::Format, 0, 0);
    let c = g.add(NodeKind::Format, 0, 0);
    g.wire(a, "exec_out", c, "exec_in").unwrap();
    // A second, different source converging on the same exec_in → allowed (fan-in).
    assert!(g.wire(b, "exec_out", c, "exec_in").is_ok());
    // The same pin pair again → rejected, so fan-in can't stack duplicate wires.
    let err = g.wire(a, "exec_out", c, "exec_in").unwrap_err();
    assert!(err.contains("already linked"), "got: {err}");
    assert_eq!(g.edges.len(), 2);
}

#[test]
fn for_each_iterates_in_order_then_fires_completed_once() {
    // ForEach("a,b,c") → loop_body → Format("{p1}", p1=item)
    //                  → completed → Format("done").
    let ctx = test_ctx();
    let mut g = Graph::new("loop");
    g.meta.debug = true;
    let fe = g.add(NodeKind::ForEach, 0, 0);
    g.node_mut(fe).unwrap().config["items"] = json!("a,b,c");
    let body = g.add(NodeKind::Format, 0, 0);
    g.node_mut(body).unwrap().config["template"] = json!("{p1}");
    let done = g.add(NodeKind::Format, 0, 0);
    g.node_mut(done).unwrap().config["template"] = json!("done");
    g.wire(fe, "loop_body", body, "exec_in").unwrap();
    g.wire(fe, "item", body, "p1").unwrap();
    g.wire(fe, "completed", done, "exec_in").unwrap();

    let (acts, logs) = run_capture(&g, &ctx);
    assert_eq!(
        acts.iter().filter(|&&x| x == body).count(),
        3,
        "body runs once per element"
    );
    assert_eq!(
        acts.iter().filter(|&&x| x == done).count(),
        1,
        "completed fires once"
    );
    assert_eq!(
        acts.iter().filter(|&&x| x == fe).count(),
        4,
        "fe: 3 elements + final"
    );
    let body_logs: Vec<String> = logs
        .iter()
        .filter(|(id, _)| *id == body)
        .map(|(_, m)| m.clone())
        .collect();
    assert_eq!(body_logs, vec!["→ a", "→ b", "→ c"], "item flows in order");
}

/// The real graph this bug was found in ("Face Test", tenant workflow 5ef37805). Shape copied
/// from production: `When Event` fans out to four `Set Variable`s, the numeric ones drive
/// `>=` → `And` → `Branch`, and the branch's true arm feeds a consumer whose input comes from a
/// `Get Variable` — a Get created early in the editor, so it carries a LOWER node id (19) than
/// the Set that fills it (27).
///
/// Before the fix the Get won the topological tie-break, read an unset variable, and the
/// consumer reported `missing required input` — while `bbox`, whose `zero_value` is a real
/// all-zero box rather than `None`, was silently filled with garbage instead of erroring.
/// Uses Text so the test needs no camera, sidecar or event payload; the ordering is the bug.
#[test]
fn face_test_graph_shape_get_before_set_reads_the_written_value() {
    let ctx = test_ctx();
    let mut g = Graph::new("Face Test");

    // Created FIRST, exactly as in the real graph → lowest ids, wins the tie-break.
    let get_image = g.add(NodeKind::GetVariable, 0, 0);
    g.node_mut(get_image).unwrap().config["variable"] = json!("image");
    g.node_mut(get_image).unwrap().config["type"] = json!("text");

    // The event's payload stand-in + the four Sets it fans out to.
    let entry = g.add(NodeKind::Format, 0, 0);
    g.node_mut(entry).unwrap().config["template"] = json!("FRAME");
    let set_image = g.add(NodeKind::SetVariable, 0, 0);
    g.node_mut(set_image).unwrap().config["variable"] = json!("image");
    g.node_mut(set_image).unwrap().config["type"] = json!("text");
    g.wire(entry, "exec_out", set_image, "exec_in").unwrap();
    g.wire(entry, "text", set_image, "value").unwrap();

    // face_width >= 20 AND face_confidence >= 60 → Branch, as in the real graph.
    let gte_w = g.add(NodeKind::NumGte, 0, 0);
    g.node_mut(gte_w).unwrap().config["a"] = json!(33);
    g.node_mut(gte_w).unwrap().config["b"] = json!(20);
    let gte_c = g.add(NodeKind::NumGte, 0, 0);
    g.node_mut(gte_c).unwrap().config["a"] = json!(89);
    g.node_mut(gte_c).unwrap().config["b"] = json!(60);
    let and = g.add(NodeKind::And, 0, 0);
    g.wire(gte_w, "result", and, "a").unwrap();
    g.wire(gte_c, "result", and, "b").unwrap();
    let branch = g.add(NodeKind::If, 0, 0);
    g.wire(and, "result", branch, "condition").unwrap();
    g.wire(set_image, "exec_out", branch, "exec_in").unwrap();

    // The gated consumer, fed from the Get — the node that reported the missing input.
    let consumer = g.add(NodeKind::Format, 0, 0);
    g.node_mut(consumer).unwrap().config["template"] = json!("{p1}");
    g.wire(branch, "true", consumer, "exec_in").unwrap();
    g.wire(get_image, "value", consumer, "p1").unwrap();

    assert!(
        get_image < set_image,
        "the Get must hold the lower id — that is the condition that triggered the bug"
    );

    let outs = run_outputs(&g, &ctx).expect("run");
    assert_eq!(
        outs.get(&consumer)
            .and_then(|m| m.get("text"))
            .and_then(PortValue::as_text),
        Some("FRAME"),
        "the gated consumer must see what the Set wrote, not an empty epoch-0 read"
    );
}

/// The same Set→Get contract, but with the Get created FIRST so it holds the lower node id.
/// `set_variable_then_get_variable_reads_the_value` builds Set first and so passes on the
/// topological tie-break (lowest id wins) rather than on the wiring — it would pass even if
/// ordering were broken. This is the case that actually bit: a real graph whose Get was node 19
/// and whose Set was node 27 read the variable before it was written.
#[test]
fn get_created_before_set_still_reads_the_written_value() {
    let ctx = test_ctx();
    let mut g = Graph::new("vars-id-order");
    // Get FIRST → lowest id → wins the tie-break → would run before Set without the fix.
    let get = g.add(NodeKind::GetVariable, 0, 0);
    g.node_mut(get).unwrap().config["variable"] = json!("greeting");
    g.node_mut(get).unwrap().config["type"] = json!("text");
    let src = g.add(NodeKind::Format, 0, 0);
    g.node_mut(src).unwrap().config["template"] = json!("hello");
    let set = g.add(NodeKind::SetVariable, 0, 0);
    g.node_mut(set).unwrap().config["variable"] = json!("greeting");
    g.node_mut(set).unwrap().config["type"] = json!("text");
    let sink = g.add(NodeKind::Format, 0, 0);
    g.node_mut(sink).unwrap().config["template"] = json!("{p1}");
    g.wire(src, "exec_out", set, "exec_in").unwrap();
    g.wire(src, "text", set, "value").unwrap();
    g.wire(set, "exec_out", sink, "exec_in").unwrap();
    g.wire(get, "value", sink, "p1").unwrap();

    let outs = run_outputs(&g, &ctx).expect("run");
    let got = outs
        .get(&sink)
        .and_then(|m| m.get("text"))
        .and_then(PortValue::as_text);
    assert_eq!(
        got,
        Some("hello"),
        "the Get must read what the Set wrote — ordering follows the wires, not node ids"
    );
}

#[test]
fn set_variable_then_get_variable_reads_the_value() {
    // Format("hello") --exec--> SetVariable(greeting)   [value <- Format.text]
    //                            SetVariable --exec--> Format("{p1}")  [p1 <- GetVariable.value]
    // The Get must read "hello" (the value Set wrote), not its epoch-0 zero default.
    let ctx = test_ctx();
    let mut g = Graph::new("vars");
    let src = g.add(NodeKind::Format, 0, 0);
    g.node_mut(src).unwrap().config["template"] = json!("hello");
    let set = g.add(NodeKind::SetVariable, 0, 0);
    g.node_mut(set).unwrap().config["variable"] = json!("greeting");
    g.node_mut(set).unwrap().config["type"] = json!("text");
    let get = g.add(NodeKind::GetVariable, 0, 0);
    g.node_mut(get).unwrap().config["variable"] = json!("greeting");
    g.node_mut(get).unwrap().config["type"] = json!("text");
    let sink = g.add(NodeKind::Format, 0, 0);
    g.node_mut(sink).unwrap().config["template"] = json!("{p1}");
    g.wire(src, "exec_out", set, "exec_in").unwrap();
    g.wire(src, "text", set, "value").unwrap();
    g.wire(set, "exec_out", sink, "exec_in").unwrap();
    g.wire(get, "value", sink, "p1").unwrap();

    let outs = run_outputs(&g, &ctx).expect("run");
    let got = outs
        .get(&sink)
        .and_then(|m| m.get("text"))
        .and_then(PortValue::as_text);
    assert_eq!(got, Some("hello"), "Get reads the value Set wrote");
}

#[test]
fn get_variable_logs_only_on_the_branch_that_ran() {
    // #115: a pure `GetVariable` activates every run, but its log line + canvas highlight
    // (both driven by `on_log`) must only appear when a node it feeds actually ran. Build a
    // two-branch graph: `If(false)` lights the `false` branch; each branch has its own `Get`
    // feeding its branch's `Format`. Only the `false`-branch Get should log.
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    g.meta.debug = true;

    let gate = g.add(NodeKind::If, 0, 0);
    g.node_mut(gate).unwrap().config["condition"] = json!(false); // → false branch runs

    // TRUE branch (does NOT run): Get feeds the true-branch Format.
    let get_a = g.add(NodeKind::GetVariable, 0, 0);
    g.node_mut(get_a).unwrap().config["variable"] = json!("a");
    g.node_mut(get_a).unwrap().config["type"] = json!("text");
    let fmt_a = g.add(NodeKind::Format, 0, 0);
    g.node_mut(fmt_a).unwrap().config["template"] = json!("{p1}");
    g.wire(gate, "true", fmt_a, "exec_in").unwrap();
    g.wire(get_a, "value", fmt_a, "p1").unwrap();

    // FALSE branch (runs): Get feeds the false-branch Format.
    let get_b = g.add(NodeKind::GetVariable, 0, 0);
    g.node_mut(get_b).unwrap().config["variable"] = json!("b");
    g.node_mut(get_b).unwrap().config["type"] = json!("text");
    let fmt_b = g.add(NodeKind::Format, 0, 0);
    g.node_mut(fmt_b).unwrap().config["template"] = json!("{p1}");
    g.wire(gate, "false", fmt_b, "exec_in").unwrap();
    g.wire(get_b, "value", fmt_b, "p1").unwrap();

    let (_, logs) = run_capture(&g, &ctx);
    let logged: HashSet<u32> = logs.iter().map(|(id, _)| *id).collect();
    // Both Gets ACTIVATE (pure sources), but only the one feeding the branch that ran logs.
    assert!(
        logged.contains(&get_b),
        "the Get feeding the branch that ran must log"
    );
    assert!(
        !logged.contains(&get_a),
        "the Get feeding the dead branch must NOT log/highlight (#115)"
    );
    // Sanity: the false branch's Format ran, the true branch's did not.
    assert!(logged.contains(&fmt_b) && !logged.contains(&fmt_a));
}

#[test]
fn get_variable_with_no_consumers_stays_quiet() {
    // A truly unused variable read (no outgoing edge) never lit a node → no log/highlight.
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    g.meta.debug = true;
    g.meta.run_orphans = true; // let the edgeless Get participate in the run
    let get = g.add(NodeKind::GetVariable, 0, 0);
    g.node_mut(get).unwrap().config["variable"] = json!("x");
    g.node_mut(get).unwrap().config["type"] = json!("text");
    let (_, logs) = run_capture(&g, &ctx);
    assert!(
        !logs.iter().any(|(id, _)| *id == get),
        "a Get with no consumers stays quiet"
    );
}

#[test]
fn get_variable_feeding_a_node_that_ran_still_logs() {
    // A normal single-branch graph is unaffected: the Get feeds a Format that runs → it logs.
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    g.meta.debug = true;
    let get = g.add(NodeKind::GetVariable, 0, 0);
    g.node_mut(get).unwrap().config["variable"] = json!("greeting");
    g.node_mut(get).unwrap().config["type"] = json!("text");
    let fmt = g.add(NodeKind::Format, 0, 0);
    g.node_mut(fmt).unwrap().config["template"] = json!("{p1}");
    g.wire(get, "value", fmt, "p1").unwrap();
    let (_, logs) = run_capture(&g, &ctx);
    assert!(
        logs.iter().any(|(id, _)| *id == get),
        "a Get feeding a node that ran still logs"
    );
}

#[test]
fn for_each_empty_list_only_fires_completed() {
    let ctx = test_ctx();
    let mut g = Graph::new("loop");
    let fe = g.add(NodeKind::ForEach, 0, 0); // items defaults to ""
    let body = g.add(NodeKind::Format, 0, 0);
    g.wire(fe, "loop_body", body, "exec_in").unwrap();
    let done = g.add(NodeKind::Format, 0, 0);
    g.wire(fe, "completed", done, "exec_in").unwrap();

    let (acts, _) = run_capture(&g, &ctx);
    assert_eq!(
        acts.iter().filter(|&&x| x == body).count(),
        0,
        "no body on empty list"
    );
    assert_eq!(
        acts.iter().filter(|&&x| x == done).count(),
        1,
        "completed fires once"
    );
}

#[test]
fn for_each_iterates_a_json_number_array() {
    // A JSON number array yields Num elements (not text). Verify the body runs once per
    // element (3×) — the element type is checked directly in `parse_text_items_typing`.
    let ctx = test_ctx();
    let mut g = Graph::new("loop");
    let fe = g.add(NodeKind::ForEach, 0, 0);
    g.node_mut(fe).unwrap().config["items"] = json!("[1, 2, 3]");
    let body = g.add(NodeKind::Print, 0, 0);
    g.wire(fe, "loop_body", body, "exec_in").unwrap();
    let (acts, _) = run_capture(&g, &ctx);
    assert_eq!(
        acts.iter().filter(|&&x| x == body).count(),
        3,
        "3 JSON elements iterate"
    );
}

#[test]
fn parse_text_items_typing() {
    // CSV → Text elements; JSON array → typed (string→Text, number→Num, bool→Bool).
    let csv = parse_text_items("a, b ,c");
    assert_eq!(csv.len(), 3);
    assert_eq!(csv[0].as_text(), Some("a"));
    let jsonv = parse_text_items(r#"["x", 7, true]"#);
    assert_eq!(jsonv[0].as_text(), Some("x"));
    assert_eq!(jsonv[1].as_num(), Some(7));
    assert_eq!(jsonv[2].as_bool(), Some(true));
    assert!(parse_text_items("  ").is_empty());
}

#[test]
fn emit_event_calls_the_sink_and_on_event_is_inert_on_a_normal_run() {
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    // EmitEvent("seen") (entry) → Print, so it's connected and runs on a normal trigger.
    let em = g.add(NodeKind::EmitEvent, 0, 0);
    g.node_mut(em).unwrap().config["event"] = json!("seen");
    let pr = g.add(NodeKind::Print, 0, 0);
    g.wire(em, "exec_out", pr, "exec_in").unwrap();
    // OnEvent("seen") → Format: must NOT run on a normal trigger.
    let on = g.add(NodeKind::OnEvent, 0, 0);
    g.node_mut(on).unwrap().config["event"] = json!("seen");
    let fmt = g.add(NodeKind::Format, 0, 0);
    g.wire(on, "exec_out", fmt, "exec_in").unwrap();

    // EmitEvent reaches the bus through the run's CONTEXT now, not through a sink handed to
    // the scheduler alongside the graph. So the test supplies one, which is also what proves
    // the seam works: the node found the bus the same way a real run does.
    let fired = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut ctx = ctx;
    ctx.emitter = Some(Arc::new(Recording(fired.clone())));

    let outs = run_outputs(&g, &ctx).unwrap();
    assert_eq!(
        *fired.lock().unwrap(),
        vec!["seen".to_string()],
        "EmitEvent fires its event"
    );
    assert!(outs.contains_key(&em), "EmitEvent ran");
    // The listener is VISITED and does nothing: it produces no values and fires no arm, so
    // everything below it stays dead. The previous scheduler left it out of the outputs map
    // entirely; the engine records the empty visit. Same behaviour, one more key — worth
    // knowing only because the run summary counts that map, so an inert listener adds one to
    // "ran N node(s)".
    assert!(
        outs.get(&on).is_none_or(|v| v.is_empty()),
        "OnEvent must produce nothing on a normal trigger: {:?}",
        outs.get(&on)
    );
    assert!(
        !outs.contains_key(&fmt),
        "the OnEvent handler is inert on a normal run"
    );

    // The handler runs only when its event fires (a sub-run seeded at the OnEvent node).
    let entry = on_event_node(&g, "seen", None, &[]).expect("OnEvent found by name");
    assert_eq!(entry, on);
    let outs2 = outputs_from(&g, &ctx, Some((entry, PortValues::new()))).unwrap();
    assert!(
        outs2.contains_key(&on) && outs2.contains_key(&fmt),
        "handler subtree runs on its event"
    );
    assert!(
        !outs2.contains_key(&em),
        "the sub-run runs ONLY the handler subtree"
    );
}

/// Two test cameras: "hall" (10.0.0.1) and "lobby" (10.0.0.2) — mirrors
/// `test_ctx_with_cameras`'s fixtures, just as a plain `Vec<Device>` (no `ToolContext` needed
/// for graph-static camera resolution).
fn two_test_devices() -> Vec<crate::devices::Device> {
    use crate::devices::Device;
    vec![
        Device {
            id: "urn:uuid:aaa".into(),
            ip: "10.0.0.1".into(),
            xaddr: "".into(),
            name: "hall".into(),
            manufacturer: None,
            model: None,
            username: None,
            password: None,
            rtsp_url: None,
            rtsp_sub_url: None,
            stream_mode: None,
            ..Default::default()
        },
        Device {
            id: "".into(),
            ip: "10.0.0.2".into(),
            xaddr: "".into(),
            name: "lobby".into(),
            manufacturer: None,
            model: None,
            username: None,
            password: None,
            rtsp_url: None,
            rtsp_sub_url: None,
            stream_mode: None,
            ..Default::default()
        },
    ]
}

#[test]
fn on_event_node_camera_gate_scoped_vs_unscoped() {
    let devices = two_test_devices();
    let mut g = Graph::new("g");
    // Scoped: OnEvent("object_entered") with camera="hall" (a plain config literal, no wire).
    let scoped = g.add(NodeKind::OnEvent, 0, 0);
    g.node_mut(scoped).unwrap().config = json!({ "event": "object_entered", "camera": "hall" });
    // Fires for hall's ip, not lobby's.
    assert_eq!(
        on_event_node(&g, "object_entered", Some("10.0.0.1"), &devices),
        Some(scoped)
    );
    assert_eq!(
        on_event_node(&g, "object_entered", Some("10.0.0.2"), &devices),
        None
    );

    // Unscoped (no camera set) matches any camera — today's behaviour, unchanged.
    let mut g2 = Graph::new("g2");
    let unscoped = g2.add(NodeKind::OnEvent, 0, 0);
    g2.node_mut(unscoped).unwrap().config = json!({ "event": "object_entered" });
    assert_eq!(
        on_event_node(&g2, "object_entered", Some("10.0.0.1"), &devices),
        Some(unscoped)
    );
    assert_eq!(
        on_event_node(&g2, "object_entered", Some("10.0.0.2"), &devices),
        Some(unscoped)
    );

    // An event with no camera concept (event_camera=None) never filters, even on a scoped node.
    assert_eq!(
        on_event_node(&g, "object_entered", None, &devices),
        Some(scoped)
    );
}

#[test]
fn on_event_node_camera_wired_from_get_camera_by_name() {
    let devices = two_test_devices();
    let mut g = Graph::new("g");
    let cam = g.add(NodeKind::GetCameraByName, 0, 0);
    g.node_mut(cam).unwrap().config = json!({ "camera": "lobby" });
    let on = g.add(NodeKind::OnEvent, 0, 0);
    g.node_mut(on).unwrap().config = json!({ "event": "object_exited" });
    g.wire(cam, "camera", on, "camera").unwrap();

    assert_eq!(
        on_event_node(&g, "object_exited", Some("10.0.0.2"), &devices),
        Some(on),
        "resolves the wired GetCameraByName, not a literal"
    );
    assert_eq!(
        on_event_node(&g, "object_exited", Some("10.0.0.1"), &devices),
        None
    );
}

#[test]
fn event_payload_makes_dynamic_ports_and_seeds_the_on_event_output() {
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    // An event "seen" carrying a Text field "msg" — baked onto both nodes' config.
    let schema = json!({ "event": "seen", "payload": [{ "name": "msg", "type": "text" }] });
    let em = g.add(NodeKind::EmitEvent, 0, 0);
    g.node_mut(em).unwrap().config = schema.clone();
    let on = g.add(NodeKind::OnEvent, 0, 0);
    g.node_mut(on).unwrap().config = schema;

    // EmitEvent gains a Text INPUT port "msg"; OnEvent a Text OUTPUT port "msg".
    assert_eq!(
        g.node(em).unwrap().input_port("msg").unwrap().ty,
        types::TEXT
    );
    assert_eq!(
        g.node(on).unwrap().output_port("msg").unwrap().ty,
        types::TEXT
    );

    // The dynamic ports wire by type: Format.text → EmitEvent.msg, OnEvent.msg → Format.p1.
    let src = g.add(NodeKind::Format, 0, 0);
    g.wire(src, "text", em, "msg")
        .expect("payload INPUT port wires by type");
    let sink = g.add(NodeKind::Format, 0, 0);
    g.wire(on, "exec_out", sink, "exec_in").unwrap();
    g.wire(on, "msg", sink, "p1")
        .expect("payload OUTPUT port wires by type");

    // Firing the handler with a payload seeds the OnEvent's "msg" output for downstream.
    let entry = on_event_node(&g, "seen", None, &[]).unwrap();
    let mut payload = PortValues::new();
    payload.insert("msg".into(), PortValue::Text("hello".into()));
    let outs = outputs_from(&g, &ctx, Some((entry, payload))).unwrap();
    assert_eq!(
        outs.get(&on)
            .and_then(|m| m.get("msg"))
            .and_then(PortValue::as_text),
        Some("hello"),
        "OnEvent exposes the delivered payload on its named output"
    );
}

#[test]
fn concurrency_policies_spawn_queue_skip() {
    let t = uuid::Uuid::nil();
    let mut e = WorkflowEngine::new();
    let p = ConcurrencyPolicy::SkipIfRunning;
    assert_eq!(e.decide(t, uuid::Uuid::from_u128(1), &p), Fire::Spawn);
    e.on_spawn(t, uuid::Uuid::from_u128(1));
    assert_eq!(e.decide(t, uuid::Uuid::from_u128(1), &p), Fire::Skip);
    assert!(!e.on_finish(t, uuid::Uuid::from_u128(1)));
    assert_eq!(e.decide(t, uuid::Uuid::from_u128(1), &p), Fire::Spawn);

    let mut q = WorkflowEngine::new();
    let qp = ConcurrencyPolicy::QueueSingle { max_queued: 1 };
    assert_eq!(q.decide(t, uuid::Uuid::from_u128(1), &qp), Fire::Spawn);
    q.on_spawn(t, uuid::Uuid::from_u128(1));
    assert_eq!(q.decide(t, uuid::Uuid::from_u128(1), &qp), Fire::Queue);
    q.on_queue(t, uuid::Uuid::from_u128(1));
    assert_eq!(q.decide(t, uuid::Uuid::from_u128(1), &qp), Fire::Skip);
    assert!(q.on_finish(t, uuid::Uuid::from_u128(1)));

    let mut c = WorkflowEngine::new();
    let cp = ConcurrencyPolicy::Concurrent { max_concurrent: 2 };
    assert_eq!(c.decide(t, uuid::Uuid::from_u128(2), &cp), Fire::Spawn);
    c.on_spawn(t, uuid::Uuid::from_u128(2));
    assert_eq!(c.decide(t, uuid::Uuid::from_u128(2), &cp), Fire::Spawn);
    c.on_spawn(t, uuid::Uuid::from_u128(2));
    assert_eq!(c.decide(t, uuid::Uuid::from_u128(2), &cp), Fire::Skip);
}

#[test]
fn same_graph_id_under_different_tenants_is_isolated() {
    // Graph id 1 of tenant A must not share run state with graph id 1 of tenant B. (This test
    // also covered the cron timer until `Trigger::Cron` was removed; concurrency is what is
    // left keyed by `(tenant, graph)`, and it is the half that would actually cross-fire.)
    let a = uuid::Uuid::from_u128(1);
    let b = uuid::Uuid::from_u128(2);
    let mut e = WorkflowEngine::new();
    let p = ConcurrencyPolicy::SkipIfRunning;
    e.on_spawn(a, uuid::Uuid::from_u128(1));
    assert_eq!(
        e.decide(a, uuid::Uuid::from_u128(1), &p),
        Fire::Skip,
        "A's graph 1 is running"
    );
    assert_eq!(
        e.decide(b, uuid::Uuid::from_u128(1), &p),
        Fire::Spawn,
        "B's graph 1 is independent"
    );
}

#[test]
fn ask_ai_routes_true_branch_on_yes_answer() {
    let ctx = ctx_with_vision(true);
    let mut g = Graph::new("g");
    let ai = g.add(NodeKind::AskAi, 0, 0);
    g.node_mut(ai).unwrap().config =
        json!({ "question": "Is someone there?", "timeout_secs": "65" });
    let yes = g.add(NodeKind::Format, 0, 0);
    let no = g.add(NodeKind::Format, 0, 0);
    g.wire(ai, "true", yes, "exec_in").unwrap();
    g.wire(ai, "false", no, "exec_in").unwrap();
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(outs.contains_key(&ai), "AI If ran");
    assert!(outs.contains_key(&yes), "'true' branch ran (answer = YES)");
    assert!(!outs.contains_key(&no), "'false' branch skipped");
}

#[test]
fn ask_ai_routes_false_branch_on_no_answer() {
    let ctx = ctx_with_vision(false);
    let mut g = Graph::new("g");
    let ai = g.add(NodeKind::AskAi, 0, 0);
    g.node_mut(ai).unwrap().config =
        json!({ "question": "Is someone there?", "timeout_secs": "65" });
    let yes = g.add(NodeKind::Format, 0, 0);
    let no = g.add(NodeKind::Format, 0, 0);
    g.wire(ai, "true", yes, "exec_in").unwrap();
    g.wire(ai, "false", no, "exec_in").unwrap();
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(!outs.contains_key(&yes), "'true' branch skipped");
    assert!(outs.contains_key(&no), "'false' branch ran (answer = NO)");
}

#[test]
fn ask_ai_errors_without_question() {
    let ctx = ctx_with_vision(false);
    let mut g = Graph::new("g");
    let ai = g.add(NodeKind::AskAi, 0, 0);
    // No question in config or wired
    g.node_mut(ai).unwrap().config = json!({ "question": "" });
    g.meta.run_orphans = true;
    let err = run_outputs(&g, &ctx).unwrap_err();
    assert!(err.contains("question is required"), "got: {err}");
}

#[test]
fn ask_ai_wires_true_false_exec_ports() {
    let mut g = Graph::new("g");
    let ai = g.add(NodeKind::AskAi, 0, 0);
    let t = g.add(NodeKind::Format, 0, 0);
    let f = g.add(NodeKind::Format, 0, 0);
    g.wire(ai, "true", t, "exec_in").expect("true port wires");
    g.wire(ai, "false", f, "exec_in").expect("false port wires");
    // image and question wire correctly
    let snap = g.add(NodeKind::CameraSnapshot, 0, 0);
    g.wire(snap, "image", ai, "image").expect("image wires");
}

#[test]
fn string_switch_routes_to_matching_arm() {
    // StringSwitch("red") → fires the "red" arm; "blue" and "default" are skipped.
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    let sw = g.add(NodeKind::StringSwitch, 0, 0);
    g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue", "green"], "value": "red" });
    let red = g.add(NodeKind::Format, 0, 0);
    let blue = g.add(NodeKind::Format, 0, 0);
    let def = g.add(NodeKind::Format, 0, 0);
    g.wire(sw, "red", red, "exec_in").unwrap();
    g.wire(sw, "blue", blue, "exec_in").unwrap();
    g.wire(sw, "default", def, "exec_in").unwrap();
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(outs.contains_key(&sw), "switch ran");
    assert!(outs.contains_key(&red), "matching arm 'red' ran");
    assert!(!outs.contains_key(&blue), "'blue' skipped");
    assert!(
        !outs.contains_key(&def),
        "'default' skipped when a match fires"
    );
}

#[test]
fn string_switch_fires_default_when_no_arm_matches() {
    // StringSwitch("purple") → no arm matches → "default" fires, named arms skip.
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    let sw = g.add(NodeKind::StringSwitch, 0, 0);
    g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue"], "value": "purple" });
    let red = g.add(NodeKind::Format, 0, 0);
    let def = g.add(NodeKind::Format, 0, 0);
    g.wire(sw, "red", red, "exec_in").unwrap();
    g.wire(sw, "default", def, "exec_in").unwrap();
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(!outs.contains_key(&red), "'red' skipped");
    assert!(outs.contains_key(&def), "'default' fires on no match");
}

#[test]
fn string_switch_value_from_upstream_node() {
    // Wired value: Format → "blue" → StringSwitch routes to blue arm.
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    let fmt = g.add(NodeKind::Format, 0, 0);
    g.node_mut(fmt).unwrap().config = json!({ "template": "blue", "p1": "", "p2": "", "p3": "" });
    let sw = g.add(NodeKind::StringSwitch, 0, 0);
    g.node_mut(sw).unwrap().config = json!({ "arms": ["red", "blue", "green"] });
    g.wire(fmt, "exec_out", sw, "exec_in").unwrap();
    g.wire(fmt, "text", sw, "value").unwrap();
    let blue = g.add(NodeKind::Format, 0, 0);
    let def = g.add(NodeKind::Format, 0, 0);
    g.wire(sw, "blue", blue, "exec_in").unwrap();
    g.wire(sw, "default", def, "exec_in").unwrap();
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(
        outs.contains_key(&blue),
        "'blue' arm fires (wired value = \"blue\")"
    );
    assert!(!outs.contains_key(&def), "'default' skipped");
}

#[test]
fn string_switch_missing_value_errors() {
    // No wired value and no config literal → run errors on missing required input.
    let ctx = test_ctx();
    let mut g = Graph::new("g");
    let sw = g.add(NodeKind::StringSwitch, 0, 0);
    // arms set but no value literal or wire
    g.node_mut(sw).unwrap().config = json!({ "arms": ["red"] });
    g.meta.run_orphans = true;
    let err = run_outputs(&g, &ctx).unwrap_err();
    assert!(err.contains("missing required input 'value'"), "got: {err}");
}

// ---- CameraSwitch routing -----------------------------------------------
// Helper: build a ToolContext pre-populated with two test cameras.
// cam_a: ONVIF id "urn:uuid:aaa", ip "10.0.0.1", name "hall"
// cam_b: no ONVIF id (legacy), ip "10.0.0.2", name "lobby"
fn test_ctx_with_cameras() -> ToolContext {
    use crate::devices::Device;
    let cam_a = Device {
        id: "urn:uuid:aaa".into(),
        ip: "10.0.0.1".into(),
        xaddr: "".into(),
        name: "hall".into(),
        manufacturer: None,
        model: None,
        username: None,
        password: None,
        rtsp_url: None,
        rtsp_sub_url: None,
        stream_mode: None,

        ..Default::default()
    };
    let cam_b = Device {
        id: "".into(),
        ip: "10.0.0.2".into(),
        xaddr: "".into(),
        name: "lobby".into(),
        manufacturer: None,
        model: None,
        username: None,
        password: None,
        rtsp_url: None,
        rtsp_sub_url: None,
        stream_mode: None,

        ..Default::default()
    };
    let (tx, _rx) = std::sync::mpsc::channel();
    ToolContext::new_local(
        std::sync::Arc::new(std::sync::Mutex::new(vec![cam_a, cam_b])),
        crate::db::Db::lazy("postgres://x@localhost/x"),
        crate::tenancy::DEFAULT_TENANT_ID,
        std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        None,
        tx,
    )
}

// Build a graph with a CameraSwitch node that gets the camera from a literal in config.
// The engine resolves `config["camera"]` as a camera entity (via resolve_camera_literal).
fn make_camera_switch_graph(
    camera_name: &str,
    arms: serde_json::Value,
) -> (Graph, u32, u32, u32, u32) {
    let mut g = Graph::new("g");
    g.meta.run_orphans = true; // run the switch even without an upstream exec trigger
    let sw = g.add(NodeKind::CameraSwitch, 0, 0);
    g.node_mut(sw).unwrap().config = serde_json::json!({ "arms": arms, "camera": camera_name });
    let arm1 = g.add(NodeKind::Format, 0, 0);
    let arm2 = g.add(NodeKind::Format, 0, 0);
    let def = g.add(NodeKind::Format, 0, 0);
    g.wire(sw, "arm_1", arm1, "exec_in").unwrap();
    g.wire(sw, "arm_2", arm2, "exec_in").unwrap();
    g.wire(sw, "default", def, "exec_in").unwrap();
    (g, sw, arm1, arm2, def)
}

#[test]
fn camera_switch_routes_by_camera_id() {
    // Camera "hall" (ONVIF id "urn:uuid:aaa"): arm_1 stores that id → arm_1 fires.
    let ctx = test_ctx_with_cameras();
    let (g, sw, arm1, arm2, def) =
        make_camera_switch_graph("hall", json!(["urn:uuid:aaa", "10.0.0.2"]));
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(outs.contains_key(&sw), "switch ran");
    assert!(outs.contains_key(&arm1), "arm_1 (hall by id) fires");
    assert!(!outs.contains_key(&arm2), "arm_2 skipped");
    assert!(!outs.contains_key(&def), "default skipped when match found");
}

#[test]
fn camera_switch_routes_by_camera_ip_legacy() {
    // Legacy camera "lobby" (no ONVIF id, ip "10.0.0.2"): arm_2 stores the IP → fires.
    let ctx = test_ctx_with_cameras();
    let (g, _sw, arm1, arm2, def) =
        make_camera_switch_graph("lobby", json!(["urn:uuid:aaa", "10.0.0.2"]));
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(!outs.contains_key(&arm1), "arm_1 skipped");
    assert!(outs.contains_key(&arm2), "arm_2 (lobby by ip) fires");
    assert!(!outs.contains_key(&def), "default skipped");
}

#[test]
fn camera_switch_fires_default_when_no_arm_matches() {
    // Camera ip/id not in any arm → default fires; named arm skips.
    let ctx = test_ctx_with_cameras();
    let mut g = Graph::new("g");
    g.meta.run_orphans = true;
    let sw = g.add(NodeKind::CameraSwitch, 0, 0);
    g.node_mut(sw).unwrap().config = json!({ "arms": ["10.0.0.99"], "camera": "hall" });
    let arm1 = g.add(NodeKind::Format, 0, 0);
    let def = g.add(NodeKind::Format, 0, 0);
    g.wire(sw, "arm_1", arm1, "exec_in").unwrap();
    g.wire(sw, "default", def, "exec_in").unwrap();
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(!outs.contains_key(&arm1), "arm_1 skipped (no match)");
    assert!(
        outs.contains_key(&def),
        "default fires when nothing matches"
    );
}

#[test]
fn camera_switch_empty_arm_string_never_matches() {
    // Empty string arms (old default config ["", ""]) must never match any camera —
    // the bug: `"" == cam.id` was true for legacy cameras with no ONVIF id.
    let ctx = test_ctx_with_cameras();
    let (g, _sw, arm1, _arm2, def) = make_camera_switch_graph(
        "lobby",
        json!(["", ""]), // lobby has id="" — must NOT false-match
    );
    let outs = run_outputs(&g, &ctx).unwrap();
    assert!(
        !outs.contains_key(&arm1),
        "empty arm must not false-match a legacy camera (id=\"\")"
    );
    assert!(outs.contains_key(&def), "default fires for unset arms");
}

// `seeded_cron_waits_out_remaining_interval` was here. It guarded the Phase 0.5 restore — a
// boot re-anchoring `cron_last` so a 30s interval that fired mid-window did not fire again
// immediately. Both the timer and the restore are gone with `Trigger::Cron`; a cron node's next
// fire is a stored timestamp, so there is no in-memory clock left to get wrong across a restart.
