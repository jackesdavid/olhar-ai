//! Shared service state + boot/hydration. The engine, store, activity map, and a
//! per-tenant edge cache live here behind `Arc<AppState>` so the HTTP layer and the
//! engine threads share one source of truth.
//!
//! This pod is **multi-tenant**: it loads EVERY tenant's graphs (`repo::load_all`) and
//! the poller fires each `(tenant, graph)` under the engine's `(tenant, graph_id)`
//! keying. Camera frames are grabbed per-run, bound to the run's tenant edge (resolved
//! via the box registry + a short-lived cache), so concurrent runs of different tenants
//! never collide on a pod-global cache.
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use uuid::Uuid;

use app_core::db::Db;
use app_core::workflows::{self, Graph, WorkflowEngine};

use crate::events::{self, EventBus};

/// The named-graph collection, each tagged with its tenant. Edits are owned by the
/// control plane and land in Postgres — this service reloads them (see
/// [`AppState::reload_graphs`]).
pub type WfStore = Arc<Mutex<Vec<(Uuid, Graph)>>>;
/// The trigger/concurrency engine (cron timers + in-flight counters + per-tenant alert
/// watermark), keyed by `(tenant, graph_id)`.
pub type WfEngine = Arc<Mutex<WorkflowEngine>>;

/// Live run activity per `(tenant, graph_id)`, polled by the UI: how many runs are in
/// flight, which node is currently executing, and (for the most recent live run) its id
/// + trigger source so the list can show how/why a graph is running.
#[derive(Default, Clone, Serialize)]
pub struct GraphActivity {
    pub running: u32,
    pub current: Option<u32>,
    pub run_id: Option<Uuid>,
    /// Trigger source of the current run: "manual" | "cron" | "event".
    pub trigger: Option<String>,
    /// The flow instance (pod) executing the current run.
    pub pod_name: Option<String>,
}
pub type Activity = Arc<Mutex<HashMap<(Uuid, u32), GraphActivity>>>;

/// Latest `ShowImage` frame bytes per `(tenant, graph_id, node_id)`, fetched out of band.
pub type NodeFrames = Arc<Mutex<HashMap<(Uuid, u32, u32), Vec<u8>>>>;

/// A safety backstop: never let more than this many workflow runs be in flight
/// across all tenants/graphs at once.
pub const WF_GLOBAL_CAP: u32 = 8;

/// Active pairing-poller stop flags, keyed by `tenant_id`.
/// Each entry carries the bot token that was active when the poller started —
/// a token change triggers a restart: the old flag is set, a fresh poller begins.
pub type PairingStops = Arc<Mutex<HashMap<Uuid, (String, Arc<AtomicBool>)>>>;

/// How long a resolved tenant→edge-base mapping is cached before re-resolving from the
/// box registry (mirrors the gateway's upstream cache TTL).
const EDGE_TTL: Duration = Duration::from_secs(30);

/// Everything the engine threads + HTTP handlers share.
pub struct AppState {
    pub db: Db,
    /// All tenants' graphs (`(tenant_id, Graph)`), reloaded from Postgres on demand.
    pub store: WfStore,
    pub engine: WfEngine,
    pub activity: Activity,
    pub events: EventBus,
    /// Latest `ShowImage` debug frame per `(tenant, graph, node)`, served out of band by
    /// `GET /runs/frame` (raw bytes never go on the SSE stream — see [`crate::events`]).
    /// One small entry per debug node, overwritten on each run; bounded by node count.
    pub node_frames: NodeFrames,
    /// Resolved tenant → edge go2rtc base (`https://<host>`) with a short TTL, so each
    /// run's `EdgeFrameSource` targets that tenant's own edge without a DB hit per frame.
    pub edge_cache: Arc<Mutex<HashMap<Uuid, (String, Instant)>>>,
    /// In-flight event handler counts, keyed `(tenant, graph_id, event_name)`. Tracks how
    /// many concurrent handler runs are active for each `(graph, event)` pair. A new event
    /// is dropped only when the count is at or above the graph's `max_concurrent` limit
    /// (or ≥1 for non-Concurrent graphs). Decremented when each handler sub-run finishes.
    pub event_runs: Arc<Mutex<std::collections::HashMap<(Uuid, u32, String), usize>>>,
    /// This flow instance's name, shown in the UI as the "pod" executing a run. Resolved
    /// once at boot from `FLOW_INSTANCE_NAME`, else `HOSTNAME`, else `"flow"`.
    pub pod_name: String,
    /// Running pairing pollers per tenant. Each entry holds the bot token that started
    /// the poller (for change detection) and the stop flag used to retire it.
    pub pairing_stops: PairingStops,
}

impl AppState {
    /// Connect to Postgres (applies migrations), hydrate every tenant's graph store,
    /// and restore the engine's persisted cron timing (Phase 0.5) per tenant so a
    /// restart doesn't fire a long cron immediately.
    pub fn boot(database_url: &str) -> Result<Arc<AppState>, String> {
        let db = Db::connect(database_url).map_err(|e| format!("connect Postgres: {e}"))?;
        log::info!("Postgres connected; migrations applied");

        let pod_name = std::env::var("FLOW_INSTANCE_NAME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("HOSTNAME").ok().filter(|s| !s.trim().is_empty()))
            .unwrap_or_else(|| "flow".to_string());
        log::info!("flow instance (pod) name: {pod_name}");
        // Runs die with the process; reconcile this pod's rows left 'running' by a crash.
        workflows::repo::mark_orphaned_runs(&db, &pod_name);

        let graphs = workflows::repo::load_all(&db);
        let tenants = distinct_tenants(&graphs);
        log::info!(
            "Loaded {} workflow graph(s) across {} tenant(s) from Postgres",
            graphs.len(),
            tenants.len()
        );
        let store: WfStore = Arc::new(Mutex::new(graphs));

        let engine = restore_engine(&db, &tenants);

        Ok(Arc::new(AppState {
            db,
            store,
            engine: Arc::new(Mutex::new(engine)),
            activity: Arc::new(Mutex::new(HashMap::new())),
            events: events::channel(),
            node_frames: Arc::new(Mutex::new(HashMap::new())),
            edge_cache: Arc::new(Mutex::new(HashMap::new())),
            event_runs: Arc::new(Mutex::new(std::collections::HashMap::new())),
            pod_name,
            pairing_stops: Arc::new(Mutex::new(HashMap::new())),
        }))
    }

    /// Reload the graph set (all tenants) from Postgres (the control plane edits land
    /// there). Cheap; called on demand so a freshly-saved graph fires without a restart.
    pub fn reload_graphs(&self) {
        *self.store.lock().unwrap() = workflows::repo::load_all(&self.db);
    }

    /// This tenant's go2rtc base for frame grabs (`https://<edge-host>`), cached for
    /// [`EDGE_TTL`]. Resolves the tenant's most-recent box host from the registry; falls
    /// back to the configured `GO2RTC_URL` / localhost (see [`app_core::go2rtc::base`])
    /// when the tenant has no known box yet.
    pub fn edge_base(&self, tenant: Uuid) -> String {
        if let Some((base, at)) = self.edge_cache.lock().unwrap().get(&tenant) {
            if at.elapsed() < EDGE_TTL {
                return base.clone();
            }
        }
        let base = match app_core::tenancy::box_host_for_tenant(&self.db, tenant) {
            Some(host) => https_base(&host),
            None => app_core::go2rtc::base(),
        };
        self.edge_cache.lock().unwrap().insert(tenant, (base.clone(), Instant::now()));
        base
    }
}

/// Normalize a stored box host into a usable base URL with exactly one scheme. The edge may
/// report its host with a scheme (`https://sentinel-x.ts.net`) or bare — so prepend `https://`
/// only when absent (and keep an explicit `http://` for dev). Prevents `https://https://…`.
fn https_base(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.starts_with("http://") || h.starts_with("https://") {
        h.to_string()
    } else {
        format!("https://{h}")
    }
}

/// Find a tenant's graph by id in the multi-tenant store (cloned for use outside the lock).
pub fn find_graph(store: &WfStore, tenant: Uuid, graph_id: u32) -> Option<Graph> {
    store
        .lock()
        .unwrap()
        .iter()
        .find(|(t, g)| *t == tenant && g.id == graph_id)
        .map(|(_, g)| g.clone())
}

/// The distinct tenants present in a `(tenant, graph)` list, order-stable.
pub fn distinct_tenants(graphs: &[(Uuid, Graph)]) -> Vec<Uuid> {
    let mut seen = std::collections::HashSet::new();
    graphs.iter().map(|(t, _)| *t).filter(|t| seen.insert(*t)).collect()
}

/// Build the engine with each tenant's cron timing restored from Postgres (Phase 0.5):
/// each persisted wall-clock fire is anchored to "now" on the monotonic clock so a
/// mid-interval restart waits out the remainder instead of firing immediately.
fn restore_engine(db: &Db, tenants: &[Uuid]) -> WorkflowEngine {
    let mut engine = WorkflowEngine::new();
    let now_ms = crate::now_epoch_ms();
    let now_inst = Instant::now();
    for &tenant in tenants {
        for (graph_id, last_fired_ms) in workflows::repo::load_cron_state(db, tenant) {
            let elapsed = Duration::from_millis(now_ms.saturating_sub(last_fired_ms));
            let anchor = now_inst.checked_sub(elapsed).unwrap_or(now_inst);
            engine.seed_cron_last(tenant, graph_id, anchor);
        }
    }
    engine
}
