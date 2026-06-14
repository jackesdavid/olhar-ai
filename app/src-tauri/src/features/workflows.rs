//! Workflows — Tauri adapter for the workflow slice.
//!
//! Owns the graph-editor `#[tauri::command]`s, the trigger/concurrency poller, and
//! the run executor — all the Tauri-specific glue. The domain (`Graph`, engine,
//! nodes) and the Postgres adapter (`workflows::repo`) live in `app_core`; this
//! reaches the shared DB and the run context through `crate::{db_ref,
//! build_run_context}`. `lib.rs` only calls the lifecycle entry points
//! ([`manage_empty_store`], [`hydrate`], [`start`]) and registers the commands.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use app_core::alerts::{AlertLevel, AlertStore};
use app_core::db::Db;
use app_core::workflows::{
    self, ConcurrencyPolicy, Fire, Graph as WfGraph, NodeKind as WfNodeKind, Trigger, Viewport,
    WorkflowEngine, Workflows,
};

use crate::{build_run_context, db_ref};

/// The named-graph collection, managed state. All graph edits go through commands
/// so Rust stays authoritative for ids + typed-edge validation.
pub type WfStore = Arc<Mutex<Workflows>>;
/// The trigger/concurrency engine (timers + in-flight counters + alert watermark).
pub type WfEngine = Arc<Mutex<WorkflowEngine>>;

/// Live run activity per graph, polled by the editor/list: how many runs are in
/// flight and which node is currently executing.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct GraphActivity {
    running: u32,
    current: Option<u32>,
}
pub type WfActivity = Arc<Mutex<HashMap<u32, GraphActivity>>>;

/// A safety backstop: never let more than this many workflow runs be in flight
/// across all graphs at once.
const WF_GLOBAL_CAP: u32 = 8;

// ---- client mode (Phase 1 cutover) ------------------------------------------

/// When `FLOW_URL` is set, the shell runs in **client mode**: the always-on
/// `sentinel-flow` service is authoritative, so the embedded engine + poller are NOT
/// started here (no double-firing). Runs + run-state proxy to the service, graph
/// edits poke it to reload, and its SSE stream is bridged back to the same Tauri
/// events the editor already listens for. Unset → embedded engine (today's
/// behavior). The cutover is a single env var — fully reversible.
fn flow_url() -> Option<String> {
    std::env::var("FLOW_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
}

/// A short-timeout agent for the proxy calls so a down service never hangs the UI.
fn flow_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(5))
        .build()
}

/// Proxy a run-now to the service (`POST /runs`). Maps its 202 to the editor's
/// expected message; surfaces connection errors as a friendly string.
fn proxy_run(base: &str, id: u32) -> Result<String, String> {
    match flow_agent().post(&format!("{base}/runs")).send_json(serde_json::json!({ "graph_id": id })) {
        Ok(_) => Ok("Run accepted.".into()),
        Err(ureq::Error::Status(404, _)) => Err("graph not found".into()),
        Err(e) => Err(format!("sentinel-flow unreachable: {e}")),
    }
}

/// Proxy live run-state from the service (`GET /runs`). Empty map on any error so
/// the editor degrades gracefully when the service is down.
fn proxy_run_states(base: &str) -> HashMap<u32, GraphActivity> {
    flow_agent()
        .get(&format!("{base}/runs"))
        .call()
        .ok()
        .and_then(|r| r.into_json::<HashMap<u32, GraphActivity>>().ok())
        .unwrap_or_default()
}

/// Poke the service to re-read graphs after a local edit (best-effort, fire-and-forget).
fn poke_reload(base: &str) {
    let _ = flow_agent().post(&format!("{base}/graphs/reload")).call();
}

/// Bridge the service's SSE stream to Tauri events: subscribe to `{base}/events` and
/// re-emit each `{event, …}` line as the matching Tauri event the editor already
/// listens for (`workflow-run`/`-node`/`-log`/`-print`/`-image`, plus the
/// store-changed pokes). Reconnects on drop. Started once at setup in client mode.
fn spawn_sse_bridge(app: &tauri::AppHandle, base: String) {
    use std::io::BufRead;
    let app = app.clone();
    std::thread::spawn(move || {
        let url = format!("{base}/events");
        loop {
            // Default agent: NO read timeout — the SSE stream is long-lived (axum
            // sends keep-alive comment lines, which we skip).
            match ureq::get(&url).call() {
                Ok(resp) => {
                    log::info!("flow SSE connected: {url}");
                    let reader = std::io::BufReader::new(resp.into_reader());
                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        let Some(data) = line.strip_prefix("data:") else { continue };
                        let data = data.trim();
                        if data.is_empty() {
                            continue;
                        }
                        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data) {
                            let name = v.get("event").and_then(|e| e.as_str()).map(str::to_string);
                            if let (Some(name), Some(obj)) = (name, v.as_object_mut()) {
                                obj.remove("event");
                                let _ = app.emit(&name, v);
                            }
                        }
                    }
                    log::warn!("flow SSE stream ended; reconnecting in 2s…");
                }
                Err(e) => log::warn!("flow SSE connect failed ({e}); retrying in 2s…"),
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

/// The legacy `workflows.json` path (one-time import source on first DB boot only).
fn workflows_file(app: &tauri::AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("workflows.json")
}

// ---- lifecycle (called by lib.rs setup / try_connect_db) --------------------

/// Manage an empty graph store BEFORE the DB connect so [`hydrate`] can fill it.
pub fn manage_empty_store(app: &tauri::AppHandle) {
    let wf_store: WfStore = Arc::new(Mutex::new(Workflows::from_graphs(Vec::new())));
    app.manage(wf_store);
}

/// Load the graphs from Postgres into the managed store (rebuilding so
/// `next_graph_id` is recomputed), with a one-time non-destructive import from any
/// legacy `workflows.json` when the DB is empty.
pub fn hydrate(app: &tauri::AppHandle, db: &Db) {
    let mut graphs = workflows::repo::load(db);
    if graphs.is_empty() {
        let from_json = workflows::repo::import_legacy_json(&workflows_file(app));
        if !from_json.is_empty() {
            workflows::repo::save(db, &from_json);
            log::info!("Imported {} workflow graph(s) from workflows.json into Postgres", from_json.len());
            graphs = workflows::repo::load(db);
        }
    }
    log::info!("Workflows loaded from Postgres — {} graph(s)", graphs.len());
    *app.state::<WfStore>().lock().unwrap() = Workflows::from_graphs(graphs);
}

/// Unix epoch millis now (the wall-clock basis for persisted cron timers).
fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Start the workflow runtime. In **client mode** (`FLOW_URL` set) the embedded
/// engine + poller are NOT started — `sentinel-flow` is authoritative — but the
/// engine/activity state is still managed (so commands resolve) and the service's
/// SSE stream is bridged to Tauri events. Otherwise the **embedded** engine runs:
/// create the engine + activity map and spawn the trigger poller, restoring the
/// engine's timing state from Postgres (Phase 0.5) so a restart neither replays the
/// `OnNewAlert` backlog nor fires a long cron immediately:
///   * `alert_seen_max` — the persisted high-water mark, falling back to the current
///     alert-store max on first boot (no row yet), which is then persisted.
///   * cron timers — each persisted wall-clock fire time is converted into a
///     monotonic anchor so a mid-interval restart waits out the remainder.
pub fn start(app: &tauri::AppHandle) {
    if let Some(base) = flow_url() {
        // Client mode: sentinel-flow owns the engine. Manage placeholder state so the
        // run/run-state commands resolve, but never spawn the poller (no double-fire).
        app.manage::<WfEngine>(Arc::new(Mutex::new(WorkflowEngine::new(0))));
        app.manage::<WfActivity>(Arc::new(Mutex::new(HashMap::new())));
        log::info!("Workflows in CLIENT mode — engine at {base}; embedded poller disabled");
        spawn_sse_bridge(app, base);
        return;
    }

    let db = db_ref(app).ok();

    // Restore the alert high-water mark (persisted → current alert max fallback).
    let alert_max = db
        .as_ref()
        .and_then(workflows::repo::load_alert_seen_max)
        .unwrap_or_else(|| {
            app.state::<AlertStore>().lock().unwrap().iter().map(|a| a.id).max().unwrap_or(0)
        });
    let mut engine = WorkflowEngine::new(alert_max);

    // Restore cron timers: anchor each persisted wall-clock fire to "now" on the
    // monotonic clock so cron_due measures the real remaining interval.
    if let Some(db) = db.as_ref() {
        let now_ms = now_epoch_ms();
        let now_inst = Instant::now();
        for (graph_id, last_fired_ms) in workflows::repo::load_cron_state(db) {
            let elapsed = Duration::from_millis(now_ms.saturating_sub(last_fired_ms));
            // If the box was down longer than the monotonic clock can represent,
            // fall back to "now" (waits one full interval — safe, never a burst).
            let anchor = now_inst.checked_sub(elapsed).unwrap_or(now_inst);
            engine.seed_cron_last(graph_id, anchor);
        }
        // Ensure a row exists so subsequent boots restore (instead of falling back).
        workflows::repo::save_alert_seen_max(db, alert_max);
    }

    let wf_engine: WfEngine = Arc::new(Mutex::new(engine));
    app.manage(wf_engine);
    let wf_activity: WfActivity = Arc::new(Mutex::new(HashMap::new()));
    app.manage(wf_activity);
    spawn_workflow_poller(app);
}

// ---- persistence + run executor --------------------------------------------

/// Persist the whole graph collection to Postgres (replace-all). Best-effort: if the
/// DB isn't connected yet (can't happen past the splash) the edit is in-memory only.
/// In client mode, also poke `sentinel-flow` to re-read so a freshly-edited trigger
/// fires without restarting the service.
fn persist(app: &tauri::AppHandle, store: &WfStore) {
    let graphs = store.lock().unwrap().graphs.clone();
    if let Ok(db) = db_ref(app) {
        workflows::repo::save(&db, &graphs);
    }
    if let Some(base) = flow_url() {
        poke_reload(&base);
    }
}

/// Spawn one graph run on a background thread. The caller must have already
/// reserved a slot (`engine.on_spawn`). On finish it releases the slot and, if a
/// queued fire is waiting (`QueueSingle`), reserves + spawns the next one.
fn spawn_graph_run(app: &tauri::AppHandle, graph_id: u32) {
    let graph: WfGraph = {
        let store = app.state::<WfStore>();
        let s = store.lock().unwrap();
        match s.graph(graph_id) {
            Some(g) => g.clone(),
            None => return,
        }
    };
    let app = app.clone();
    std::thread::spawn(move || {
        // Mark this graph as running for the live editor/list.
        app.state::<WfActivity>().lock().unwrap().entry(graph_id).or_default().running += 1;

        let result = match build_run_context(&app) {
            None => Err("database not ready".to_string()),
            Some(ctx) => {
                let progress_app = app.clone();
                let mut on_node = |nid: u32| {
                    progress_app
                        .state::<WfActivity>()
                        .lock()
                        .unwrap()
                        .entry(graph_id)
                        .or_default()
                        .current = Some(nid);
                    // Per-node event so the editor highlights every node, even on a
                    // run faster than the poll interval.
                    let _ = progress_app
                        .emit("workflow-node", serde_json::json!({ "graph": graph_id, "node": nid }));
                };
                let log_app = app.clone();
                let log_graph = graph.clone();
                let mut on_log = |nid: u32, msg: &str, ms: u128| {
                    let label = log_graph.node(nid).map(|n| n.kind.label()).unwrap_or("node");
                    let _ = log_app.emit(
                        "workflow-log",
                        serde_json::json!({ "graph": graph_id, "node": nid, "label": label, "message": msg, "ms": ms }),
                    );
                };
                workflows::run_dag_with(&graph, &ctx, &mut on_node, &mut on_log)
                // ctx drops here → its drain thread ends.
            }
        };
        let (ok, summary) = match &result {
            Ok(outs) => (true, format!("ran {} node(s)", outs.len())),
            Err(e) => (false, e.clone()),
        };

        // Release activity: decrement running, clear the current node when idle.
        {
            let activity = app.state::<WfActivity>();
            let mut a = activity.lock().unwrap();
            let e = a.entry(graph_id).or_default();
            if e.running > 0 {
                e.running -= 1;
            }
            if e.running == 0 {
                e.current = None;
            }
        }
        if ok {
            log::info!("workflow '{}' ({graph_id}) ok: {summary}", graph.name);
        } else {
            log::warn!("workflow '{}' ({graph_id}) failed: {summary}", graph.name);
        }
        let _ = app.emit(
            "workflow-run",
            serde_json::json!({ "graph": graph_id, "name": graph.name, "ok": ok, "summary": summary }),
        );

        // Release the slot; drain a queued fire if the policy buffered one.
        let drain = app.state::<WfEngine>().lock().unwrap().on_finish(graph_id);
        if drain {
            app.state::<WfEngine>().lock().unwrap().on_spawn(graph_id);
            spawn_graph_run(&app, graph_id);
        }
    });
}

/// Apply a graph's concurrency policy to a fire: spawn, queue, or skip.
fn fire_graph(app: &tauri::AppHandle, graph_id: u32, policy: &ConcurrencyPolicy) {
    let decision = app.state::<WfEngine>().lock().unwrap().decide(graph_id, policy);
    match decision {
        Fire::Spawn => {
            app.state::<WfEngine>().lock().unwrap().on_spawn(graph_id);
            spawn_graph_run(app, graph_id);
        }
        Fire::Queue => app.state::<WfEngine>().lock().unwrap().on_queue(graph_id),
        Fire::Skip => {}
    }
}

/// The engine poller: ~1 s tick that fires due Cron graphs and qualifying
/// `OnNewAlert` graphs, each under its concurrency policy. Started at setup.
fn spawn_workflow_poller(app: &tauri::AppHandle) {
    let app = app.clone();
    // The last watermark we wrote to Postgres — seeded from the engine's restored
    // value so we only persist on a genuine advance (Phase 0.5).
    let mut persisted_max = app.state::<WfEngine>().lock().unwrap().alert_seen_max();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = Instant::now();

        // Snapshot the enabled graphs' firing properties (don't hold the lock during runs).
        let graphs: Vec<(u32, Trigger, ConcurrencyPolicy)> = {
            let s = app.state::<WfStore>();
            let s = s.lock().unwrap();
            s.graphs
                .iter()
                .filter(|g| g.enabled)
                .map(|g| (g.id, g.trigger.clone(), g.concurrency.clone()))
                .collect()
        };

        // Drain new-alert levels ONCE per tick (advances the high-water mark so
        // the backlog fires only once and workflow-made alerts can't loop).
        let levels = {
            let alerts = app.state::<AlertStore>().lock().unwrap().clone();
            app.state::<WfEngine>().lock().unwrap().drain_new_alert_levels(&alerts)
        };
        // Persist the watermark only when it advanced (Phase 0.5) so a restart
        // doesn't replay the backlog. Cheap: a write only when new alerts arrived.
        {
            let max = app.state::<WfEngine>().lock().unwrap().alert_seen_max();
            if max > persisted_max {
                if let Ok(db) = db_ref(&app) {
                    workflows::repo::save_alert_seen_max(&db, max);
                }
                persisted_max = max;
            }
        }

        for (id, trigger, policy) in graphs {
            if app.state::<WfEngine>().lock().unwrap().total_running() >= WF_GLOBAL_CAP {
                break; // global backstop
            }
            let due = match &trigger {
                Trigger::Manual => false,
                Trigger::Cron { every_secs } => {
                    app.state::<WfEngine>().lock().unwrap().cron_due(id, *every_secs, now)
                }
                Trigger::OnNewAlert { min_level } => {
                    WorkflowEngine::levels_qualify(&levels, *min_level)
                }
            };
            if due {
                // Persist the cron fire time (wall-clock) so a restart resumes the
                // timer rather than firing immediately. OnNewAlert fires ride the
                // watermark above, so only cron needs a per-fire write.
                if matches!(trigger, Trigger::Cron { .. }) {
                    if let Ok(db) = db_ref(&app) {
                        workflows::repo::save_cron_fired(&db, id, now_epoch_ms());
                    }
                }
                fire_graph(&app, id, &policy);
            }
        }
    });
}

// ---- commands ---------------------------------------------------------------

/// All named graphs (with triggers, concurrency, nodes, typed edges).
#[tauri::command]
pub fn load_graphs(store: tauri::State<'_, WfStore>) -> Vec<WfGraph> {
    store.lock().unwrap().graphs.clone()
}

/// Live run state per graph id (`{ running, current }`) — the editor/list poll
/// this to show running/idle status and highlight the executing node.
#[tauri::command]
pub fn run_states(activity: tauri::State<'_, WfActivity>) -> HashMap<u32, GraphActivity> {
    if let Some(base) = flow_url() {
        return proxy_run_states(&base); // client mode: live state from the service
    }
    activity.lock().unwrap().clone()
}

/// Static node metadata (kinds + typed ports + default config) for the palette
/// and the editor's client-side type hints. The backend stays authoritative on
/// `add_edge`; this just lets the UI render and pre-filter.
#[tauri::command]
pub fn node_catalog() -> serde_json::Value {
    let ports = |list: &[workflows::Port]| {
        list.iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "type": serde_json::to_value(p.ty).unwrap_or(serde_json::Value::Null),
                    "required": p.required,
                })
            })
            .collect::<Vec<_>>()
    };
    // Every node carries its control-flow ports up front, then its data ports.
    let with_exec = |exec: &[workflows::Port], list: &[workflows::Port]| {
        let mut v = ports(exec);
        v.extend(ports(list));
        v
    };
    let exec_in = [workflows::EXEC_IN];
    let kinds = WfNodeKind::ALL
        .iter()
        .map(|k| {
            serde_json::json!({
                "slug": k.slug(),
                "label": k.label(),
                "category": k.category(),
                "inputs": with_exec(&exec_in, k.input_ports()),
                "outputs": with_exec(k.exec_out_ports(), k.output_ports()),
                "default_config": k.default_config(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({ "kinds": kinds })
}

#[tauri::command]
pub fn create_graph(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, name: String) -> u32 {
    let name = if name.trim().is_empty() { "Untitled".to_string() } else { name.trim().to_string() };
    let id = store.lock().unwrap().create_graph(&name);
    persist(&app, &store);
    id
}

#[tauri::command]
pub fn rename_graph(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, id: u32, name: String) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(id).ok_or("graph not found")?;
        g.name = name.trim().to_string();
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn delete_graph(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, id: u32) {
    store.lock().unwrap().remove_graph(id);
    persist(&app, &store);
}

#[tauri::command]
pub fn set_graph_trigger(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    id: u32,
    kind: String,
    every_secs: Option<u64>,
    min_level: Option<String>,
) -> Result<(), String> {
    let trigger = match kind.as_str() {
        "manual" => Trigger::Manual,
        "cron" => Trigger::Cron { every_secs: every_secs.unwrap_or(60).max(1) },
        "on_new_alert" => Trigger::OnNewAlert {
            min_level: min_level.as_deref().and_then(AlertLevel::parse).unwrap_or(AlertLevel::Info),
        },
        other => return Err(format!("unknown trigger kind '{other}'")),
    };
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(id).ok_or("graph not found")?;
        g.trigger = trigger;
        // Changing the trigger always stops the graph — it must never start firing
        // automatically on a config change. The user explicitly presses play (Enable).
        g.enabled = false;
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn set_graph_concurrency(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    id: u32,
    kind: String,
    max: Option<u32>,
) -> Result<(), String> {
    let policy = match kind.as_str() {
        "skip" => ConcurrencyPolicy::SkipIfRunning,
        "queue" => ConcurrencyPolicy::QueueSingle { max_queued: max.unwrap_or(1).max(1) },
        "concurrent" => ConcurrencyPolicy::Concurrent { max_concurrent: max.unwrap_or(2).max(1) },
        other => return Err(format!("unknown concurrency kind '{other}'")),
    };
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(id).ok_or("graph not found")?.concurrency = policy;
    }
    persist(&app, &store);
    Ok(())
}

/// Persist the editor viewport (pan + zoom) for a graph so re-opening restores it.
#[tauri::command]
pub fn set_graph_viewport(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    id: u32,
    x: f64,
    y: f64,
    zoom: f64,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(id).ok_or("graph not found")?.viewport = Viewport { x, y, zoom };
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn set_graph_enabled(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, id: u32, enabled: bool) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(id).ok_or("graph not found")?.enabled = enabled;
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn set_graph_run_orphans(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    id: u32,
    run_orphans: bool,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(id).ok_or("graph not found")?.run_orphans = run_orphans;
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn set_graph_debug(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    id: u32,
    debug: bool,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(id).ok_or("graph not found")?.debug = debug;
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn add_node(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    kind: String,
    x: i32,
    y: i32,
) -> Result<u32, String> {
    let kind = WfNodeKind::from_slug(&kind).ok_or_else(|| format!("unknown node kind '{kind}'"))?;
    let id = {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        g.add_node(kind, x, y)
    };
    persist(&app, &store);
    Ok(id)
}

#[tauri::command]
pub fn remove_node(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, graph_id: u32, node_id: u32) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(graph_id).ok_or("graph not found")?.remove_node(node_id);
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn move_node(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    node_id: u32,
    x: i32,
    y: i32,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        let n = g.node_mut(node_id).ok_or("node not found")?;
        n.x = x;
        n.y = y;
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn set_node_config(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    node_id: u32,
    config: serde_json::Value,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        g.node_mut(node_id).ok_or("node not found")?.config = config;
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn add_edge(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    from: u32,
    from_port: String,
    to: u32,
    to_port: String,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        g.add_edge(from, &from_port, to, &to_port)?; // returns the typed-edge validation error
    }
    persist(&app, &store);
    Ok(())
}

#[tauri::command]
pub fn remove_edge(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, graph_id: u32, idx: usize) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        s.graph_mut(graph_id).ok_or("graph not found")?.remove_edge(idx);
    }
    persist(&app, &store);
    Ok(())
}

/// Run-now: fire the graph immediately, bypassing `enabled`/trigger but still
/// respecting its concurrency policy and the global cap.
#[tauri::command]
pub fn run_graph(app: tauri::AppHandle, store: tauri::State<'_, WfStore>, engine: tauri::State<'_, WfEngine>, id: u32) -> Result<String, String> {
    let policy = {
        let s = store.lock().unwrap();
        s.graph(id).ok_or("graph not found")?.concurrency.clone()
    };
    if let Some(base) = flow_url() {
        return proxy_run(&base, id); // client mode: the service runs it (its concurrency policy applies)
    }
    if engine.lock().unwrap().total_running() >= WF_GLOBAL_CAP {
        return Err("Too many workflows are running right now — try again shortly.".into());
    }
    let decision = engine.lock().unwrap().decide(id, &policy);
    match decision {
        Fire::Spawn => {
            engine.lock().unwrap().on_spawn(id);
            spawn_graph_run(&app, id);
            Ok("Run started.".into())
        }
        Fire::Queue => {
            engine.lock().unwrap().on_queue(id);
            Ok("Queued — a run is already in progress.".into())
        }
        Fire::Skip => Err("Skipped — a run is already in progress (concurrency: skip).".into()),
    }
}
