//! Shared service state + boot/hydration. The engine, store, activity map, and a
//! refreshable alert mirror live here behind `Arc<AppState>` so the HTTP layer and
//! the engine threads share one source of truth.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use app_core::alerts::AlertStore;
use app_core::db::Db;
use app_core::tenancy::DEFAULT_TENANT_ID;
use app_core::workflows::{self, WorkflowEngine, Workflows};

use crate::events::{self, EventBus};

/// The named-graph collection (the working set; edits are owned by the UI/Tauri side
/// and land in Postgres — this service reloads them, see [`AppState::reload_graphs`]).
pub type WfStore = Arc<Mutex<Workflows>>;
/// The trigger/concurrency engine (cron timers + in-flight counters + alert watermark).
pub type WfEngine = Arc<Mutex<WorkflowEngine>>;

/// Live run activity per graph id, polled by the UI: how many runs are in flight and
/// which node is currently executing. Mirrors the Tauri shell's `GraphActivity`.
#[derive(Default, Clone, Serialize)]
pub struct GraphActivity {
    pub running: u32,
    pub current: Option<u32>,
}
pub type Activity = Arc<Mutex<HashMap<u32, GraphActivity>>>;

/// A safety backstop: never let more than this many workflow runs be in flight
/// across all graphs at once (mirrors the Tauri shell).
pub const WF_GLOBAL_CAP: u32 = 8;

/// Everything the engine threads + HTTP handlers share.
pub struct AppState {
    pub db: Db,
    pub store: WfStore,
    pub engine: WfEngine,
    pub activity: Activity,
    /// An in-memory mirror of the alert log (the trigger source for `OnNewAlert` and
    /// the `GetLastAlert` node), refreshed from Postgres each poller tick so alerts
    /// created by ANY service are seen.
    pub alerts: AlertStore,
    pub events: EventBus,
}

impl AppState {
    /// Connect to Postgres (applies migrations), hydrate the graph store + alert
    /// mirror, and restore the engine's persisted timing state (Phase 0.5) so a
    /// restart neither replays the `OnNewAlert` backlog nor fires a long cron at once.
    pub fn boot(database_url: &str) -> Result<Arc<AppState>, String> {
        let db = Db::connect(database_url).map_err(|e| format!("connect Postgres: {e}"))?;
        log::info!("Postgres connected; migrations applied");

        let graphs = workflows::repo::load(&db, DEFAULT_TENANT_ID);
        log::info!("Loaded {} workflow graph(s) from Postgres", graphs.len());
        let store: WfStore = Arc::new(Mutex::new(Workflows::from_graphs(graphs)));

        let alerts = app_core::alerts::repo::load(&db, DEFAULT_TENANT_ID);
        log::info!("Loaded {} alert(s) from Postgres", alerts.len());
        let alerts: AlertStore = Arc::new(Mutex::new(alerts));

        let engine = restore_engine(&db, &alerts);

        Ok(Arc::new(AppState {
            db,
            store,
            engine: Arc::new(Mutex::new(engine)),
            activity: Arc::new(Mutex::new(HashMap::new())),
            alerts,
            events: events::channel(),
        }))
    }

    /// Reload the graph set from Postgres (the UI edits land there). Cheap; called on
    /// demand so a freshly-saved graph fires without restarting the service.
    pub fn reload_graphs(&self) {
        let graphs = workflows::repo::load(&self.db, DEFAULT_TENANT_ID);
        *self.store.lock().unwrap() = Workflows::from_graphs(graphs);
    }

    /// Refresh the in-memory alert mirror from Postgres (the trigger source of truth).
    pub fn refresh_alerts(&self) {
        let fresh = app_core::alerts::repo::load(&self.db, DEFAULT_TENANT_ID);
        *self.alerts.lock().unwrap() = fresh;
    }
}

/// Build the engine with its timing state restored from Postgres (Phase 0.5):
///   * `alert_seen_max` — persisted high-water mark, falling back to the current
///     alert max on first boot (then persisted), so the backlog doesn't fire.
///   * cron timers — each persisted wall-clock fire is anchored to "now" on the
///     monotonic clock so a mid-interval restart waits out the remainder.
fn restore_engine(db: &Db, alerts: &AlertStore) -> WorkflowEngine {
    let alert_max = workflows::repo::load_alert_seen_max(db, DEFAULT_TENANT_ID).unwrap_or_else(|| {
        alerts.lock().unwrap().iter().map(|a| a.id).max().unwrap_or(0)
    });
    let mut engine = WorkflowEngine::new(alert_max);

    let now_ms = crate::now_epoch_ms();
    let now_inst = Instant::now();
    for (graph_id, last_fired_ms) in workflows::repo::load_cron_state(db, DEFAULT_TENANT_ID) {
        let elapsed = Duration::from_millis(now_ms.saturating_sub(last_fired_ms));
        let anchor = now_inst.checked_sub(elapsed).unwrap_or(now_inst);
        engine.seed_cron_last(graph_id, anchor);
    }
    workflows::repo::save_alert_seen_max(db, DEFAULT_TENANT_ID, alert_max);
    engine
}
