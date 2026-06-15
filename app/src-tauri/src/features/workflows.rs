//! Workflows — Tauri adapter for the workflow slice.
//!
//! The Tauri app is a **client of the always-on `sentinel-flow` service** — that
//! service owns the engine, the trigger poller, and the run executor (out of
//! process, so the UI can restart freely). This adapter therefore:
//!   * keeps the **graph editor** `#[tauri::command]`s (CRUD on the in-memory
//!     `Workflows` store, persisted to the shared Postgres), and
//!   * **proxies execution** (run / run-state) to `sentinel-flow`, pokes it to
//!     reload after edits, and **bridges its SSE stream** back to the Tauri events
//!     the editor already listens for.
//!
//! `lib.rs` only calls the lifecycle entry points ([`manage_empty_store`],
//! [`hydrate`], [`start`]) and registers the commands.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use app_core::alerts::AlertLevel;
use app_core::db::Db;
use app_core::tenancy::DEFAULT_TENANT_ID;
use app_core::workflows::{
    self, ConcurrencyPolicy, Graph as WfGraph, NodeKind as WfNodeKind, Trigger, Viewport, Workflows,
};

use crate::db_ref;

/// The named-graph collection, managed state. All graph edits go through commands
/// so Rust stays authoritative for ids + typed-edge validation. The working set is
/// persisted to Postgres; `sentinel-flow` reads the same table.
pub type WfStore = Arc<Mutex<Workflows>>;

/// Live run activity per graph, polled by the editor/list: how many runs are in
/// flight and which node is currently executing. Deserialized from `sentinel-flow`'s
/// `GET /runs` (the service is authoritative for run state).
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct GraphActivity {
    running: u32,
    current: Option<u32>,
}

// ---- sentinel-flow client ---------------------------------------------------

/// The workflow engine service base URL (`FLOW_URL`, default `127.0.0.1:8787`).
/// Execution always goes through this service — the app no longer runs an embedded
/// engine.
fn flow_base() -> String {
    std::env::var("FLOW_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8787".to_string())
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
    let body = serde_json::json!({ "graph_id": id, "tenant_id": DEFAULT_TENANT_ID });
    match flow_agent().post(&format!("{base}/runs")).send_json(body) {
        Ok(_) => Ok("Run accepted.".into()),
        Err(ureq::Error::Status(404, _)) => Err("graph not found".into()),
        Err(e) => Err(format!("sentinel-flow unreachable: {e}")),
    }
}

/// Proxy live run-state for the desktop's single (default) tenant from the service
/// (`GET /runs?tenant=…`). Empty map on any error so the editor degrades gracefully
/// when the service is down.
fn proxy_run_states(base: &str) -> HashMap<u32, GraphActivity> {
    flow_agent()
        .get(&format!("{base}/runs?tenant={DEFAULT_TENANT_ID}"))
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
    let mut graphs = workflows::repo::load(db, DEFAULT_TENANT_ID);
    if graphs.is_empty() {
        let from_json = workflows::repo::import_legacy_json(&workflows_file(app));
        if !from_json.is_empty() {
            workflows::repo::save(db, DEFAULT_TENANT_ID, &from_json);
            log::info!("Imported {} workflow graph(s) from workflows.json into Postgres", from_json.len());
            graphs = workflows::repo::load(db, DEFAULT_TENANT_ID);
        }
    }
    log::info!("Workflows loaded from Postgres — {} graph(s)", graphs.len());
    *app.state::<WfStore>().lock().unwrap() = Workflows::from_graphs(graphs);
}

/// Start the workflow client: the engine, poller, and run executor live in the
/// always-on `sentinel-flow` service, so all this does is bridge that service's SSE
/// stream to the Tauri events the editor listens for. Execution is driven on demand
/// via the proxying commands ([`run_graph`], [`run_states`]).
pub fn start(app: &tauri::AppHandle) {
    let base = flow_base();
    log::info!("Workflows: client of sentinel-flow at {base}");
    spawn_sse_bridge(app, base);
}

// ---- persistence ------------------------------------------------------------

/// Persist the whole graph collection to Postgres (replace-all). Best-effort: if the
/// DB isn't connected yet (can't happen past the splash) the edit is in-memory only.
/// Also pokes `sentinel-flow` to re-read so a freshly-edited trigger fires without
/// restarting the service.
fn persist(app: &tauri::AppHandle, store: &WfStore) {
    let graphs = store.lock().unwrap().graphs.clone();
    if let Ok(db) = db_ref(app) {
        workflows::repo::save(&db, DEFAULT_TENANT_ID, &graphs);
    }
    poke_reload(&flow_base());
}

// ---- commands ---------------------------------------------------------------

/// All named graphs (with triggers, concurrency, nodes, typed edges).
#[tauri::command]
pub fn load_graphs(store: tauri::State<'_, WfStore>) -> Vec<WfGraph> {
    store.lock().unwrap().graphs.clone()
}

/// Live run state per graph id (`{ running, current }`) — the editor/list poll
/// this to show running/idle status and highlight the executing node. Served by
/// `sentinel-flow` (the authoritative engine); empty on any error.
#[tauri::command]
pub fn run_states() -> HashMap<u32, GraphActivity> {
    proxy_run_states(&flow_base())
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

/// Toggle a node's memoization. `true` (default) caches the node's output for the run;
/// `false` re-runs it every time it's reached (loop bodies / time-varying sources).
#[tauri::command]
pub fn set_node_memoize(
    app: tauri::AppHandle,
    store: tauri::State<'_, WfStore>,
    graph_id: u32,
    node_id: u32,
    memoize: bool,
) -> Result<(), String> {
    {
        let mut s = store.lock().unwrap();
        let g = s.graph_mut(graph_id).ok_or("graph not found")?;
        g.node_mut(node_id).ok_or("node not found")?.memoize = memoize;
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

/// Run-now: ask `sentinel-flow` to fire the graph immediately. The service applies
/// the graph's concurrency policy + global cap; progress streams back over the
/// bridged SSE events.
#[tauri::command]
pub fn run_graph(id: u32) -> Result<String, String> {
    proxy_run(&flow_base(), id)
}
