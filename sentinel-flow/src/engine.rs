//! The workflow engine runtime: the trigger poller, the concurrency gate, and the
//! run executor. Ported from the Tauri shell's `features/workflows.rs`, but free of
//! Tauri — UI side-effects go to the SSE [`EventBus`] instead of `emit(...)`, and
//! the run context is built with `ToolContext::new_local` (Phase 0.6 ports).
//!
//! Runs on dedicated std threads (blocking), exactly like the Tauri version; the
//! axum server runs on the tokio runtime alongside. `Db::block` bridges sqlx safely
//! from either.
use std::sync::Arc;
use std::time::{Duration, Instant};

use app_core::agent::{ToolContext, ToolUiEvent};
use app_core::notification::{NotificationConfig, NotificationStore};
use app_core::workflows::{self, ConcurrencyPolicy, Fire, Graph as WfGraph, Trigger, WorkflowEngine};

use crate::events;
use crate::state::{AppState, WF_GLOBAL_CAP};

/// Spawn the 1 s trigger poller on a background thread. Fires due Cron graphs and
/// qualifying `OnNewAlert` graphs, each under its concurrency policy, persisting the
/// engine's timing state (Phase 0.5) as it goes.
pub fn spawn_poller(state: Arc<AppState>) {
    // The last watermark we persisted — seeded from the engine's restored value so
    // we only write on a genuine advance.
    let mut persisted_max = state.engine.lock().unwrap().alert_seen_max();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = Instant::now();

        // The alert log is shared state — reload it so alerts created by any service
        // drive `OnNewAlert` here too.
        state.refresh_alerts();

        // Snapshot enabled graphs' firing properties (don't hold the lock during runs).
        let graphs: Vec<(u32, Trigger, ConcurrencyPolicy)> = {
            let s = state.store.lock().unwrap();
            s.graphs
                .iter()
                .filter(|g| g.enabled)
                .map(|g| (g.id, g.trigger.clone(), g.concurrency.clone()))
                .collect()
        };

        // Drain new-alert levels ONCE per tick (advances + we persist on advance).
        let levels = {
            let alerts = state.alerts.lock().unwrap().clone();
            state.engine.lock().unwrap().drain_new_alert_levels(&alerts)
        };
        {
            let max = state.engine.lock().unwrap().alert_seen_max();
            if max > persisted_max {
                workflows::repo::save_alert_seen_max(&state.db, max);
                persisted_max = max;
            }
        }

        for (id, trigger, policy) in graphs {
            if state.engine.lock().unwrap().total_running() >= WF_GLOBAL_CAP {
                break; // global backstop
            }
            let due = match &trigger {
                Trigger::Manual => false,
                Trigger::Cron { every_secs } => {
                    state.engine.lock().unwrap().cron_due(id, *every_secs, now)
                }
                Trigger::OnNewAlert { min_level } => {
                    WorkflowEngine::levels_qualify(&levels, *min_level)
                }
            };
            if due {
                if matches!(trigger, Trigger::Cron { .. }) {
                    workflows::repo::save_cron_fired(&state.db, id, crate::now_epoch_ms());
                }
                fire_graph(&state, id, &policy);
            }
        }
    });
}

/// Apply a graph's concurrency policy to a fire: spawn, queue, or skip.
pub fn fire_graph(state: &Arc<AppState>, graph_id: u32, policy: &ConcurrencyPolicy) {
    let decision = state.engine.lock().unwrap().decide(graph_id, policy);
    match decision {
        Fire::Spawn => {
            state.engine.lock().unwrap().on_spawn(graph_id);
            spawn_graph_run(state, graph_id);
        }
        Fire::Queue => state.engine.lock().unwrap().on_queue(graph_id),
        Fire::Skip => {}
    }
}

/// Spawn one graph run on a background thread. The caller must have already reserved
/// a slot (`engine.on_spawn`). On finish it releases the slot and, if a queued fire
/// is waiting (`QueueSingle`), reserves + spawns the next one.
pub fn spawn_graph_run(state: &Arc<AppState>, graph_id: u32) {
    let graph: WfGraph = {
        let s = state.store.lock().unwrap();
        match s.graph(graph_id) {
            Some(g) => g.clone(),
            None => return,
        }
    };
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        state.activity.lock().unwrap().entry(graph_id).or_default().running += 1;
        events::publish(
            &state.events,
            "workflow-run-start",
            serde_json::json!({ "graph": graph_id, "name": graph.name }),
        );

        let (ctx, drain) = build_run_context(&state);
        let progress = Arc::clone(&state);
        let mut on_node = |nid: u32| {
            progress.activity.lock().unwrap().entry(graph_id).or_default().current = Some(nid);
            events::publish(&progress.events, "workflow-node", serde_json::json!({ "graph": graph_id, "node": nid }));
        };
        let log_state = Arc::clone(&state);
        let log_graph = graph.clone();
        let mut on_log = |nid: u32, msg: &str, ms: u128| {
            let label = log_graph.node(nid).map(|n| n.kind.label()).unwrap_or("node");
            events::publish(
                &log_state.events,
                "workflow-log",
                serde_json::json!({ "graph": graph_id, "node": nid, "label": label, "message": msg, "ms": ms as u64 }),
            );
        };
        let result = workflows::run_dag_with(&graph, &ctx, &mut on_node, &mut on_log);
        drop(ctx); // closes the UI channel → its drain thread ends
        let _ = drain.join();

        let (ok, summary) = match &result {
            Ok(outs) => (true, format!("ran {} node(s)", outs.len())),
            Err(e) => (false, e.clone()),
        };

        {
            let mut a = state.activity.lock().unwrap();
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
        events::publish(
            &state.events,
            "workflow-run",
            serde_json::json!({ "graph": graph_id, "name": graph.name, "ok": ok, "summary": summary }),
        );

        // Release the slot; drain a queued fire if the policy buffered one.
        let drain_queued = state.engine.lock().unwrap().on_finish(graph_id);
        if drain_queued {
            state.engine.lock().unwrap().on_spawn(graph_id);
            spawn_graph_run(&state, graph_id);
        }
    });
}

/// Build a [`ToolContext`] for one run, wired to the in-process ports, with stores
/// freshly loaded from Postgres (devices + beacons can change between runs). The
/// returned `JoinHandle` is the thread draining UI side-effects to the SSE bus; it
/// ends when the context (and its `Sender`) is dropped.
fn build_run_context(state: &Arc<AppState>) -> (ToolContext, std::thread::JoinHandle<()>) {
    let devices = Arc::new(std::sync::Mutex::new(app_core::devices::repo::load(&state.db)));
    let notif: NotificationStore =
        Arc::new(std::sync::Mutex::new(app_core::notification::repo::load_beacons(&state.db)));
    let notif_cfg = resolve_telegram_config(state);

    let (tx, rx) = std::sync::mpsc::channel::<ToolUiEvent>();
    let bus = state.events.clone();
    let drain = std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            events::publish_tool_event(&bus, ev);
        }
    });

    let ctx = ToolContext::new_local(
        devices,
        state.db.clone(),
        Arc::clone(&state.alerts),
        notif,
        notif_cfg,
        tx,
    );
    (ctx, drain)
}

/// Resolve the active Telegram config: the saved config (Postgres) if it has a
/// non-empty token, else the environment.
fn resolve_telegram_config(state: &Arc<AppState>) -> Option<NotificationConfig> {
    if let Some((bot_token, pairing_code)) = app_core::notification::repo::load_telegram(&state.db) {
        if !bot_token.trim().is_empty() {
            return Some(NotificationConfig { bot_token, pairing_code });
        }
    }
    NotificationConfig::from_env()
}
