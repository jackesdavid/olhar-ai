//! The workflow engine runtime: the trigger poller, the concurrency gate, and the
//! run executor. Ported from the Tauri shell's `features/workflows.rs`, but free of
//! Tauri — UI side-effects go to the SSE [`EventBus`] instead of `emit(...)`, and
//! the run context is built with `ToolContext::new_local` (Phase 0.6 ports).
//!
//! **Multi-tenant**: the poller iterates every tenant's enabled graphs, the engine is
//! keyed by `(tenant, graph_id)`, and each run's frame source is bound to that tenant's
//! edge — so one pod runs graphs of any tenant concurrently without conflict.
//!
//! Runs on dedicated std threads (blocking), exactly like the Tauri version; the
//! axum server runs on the tokio runtime alongside. `Db::block` bridges sqlx safely
//! from either.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use uuid::Uuid;

use app_core::agent::{ToolContext, ToolUiEvent};
use app_core::notification::{NotificationConfig, NotificationStore};
use app_core::workflows::{self, ConcurrencyPolicy, Fire, Graph as WfGraph, Trigger, WorkflowEngine};

use crate::events;
use crate::state::{self, AppState, WF_GLOBAL_CAP};

/// Spawn the 1 s trigger poller on a background thread. Across ALL tenants: fires due
/// Cron graphs and qualifying `OnNewAlert` graphs, each under its concurrency policy,
/// persisting the engine's per-tenant timing state (Phase 0.5) as it goes.
pub fn spawn_poller(state: Arc<AppState>) {
    // The last new-alert watermark we persisted, per tenant — so we only write on a
    // genuine advance.
    let mut persisted_max: HashMap<Uuid, u64> = HashMap::new();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = Instant::now();

        // Snapshot enabled graphs' firing properties (don't hold the lock during runs).
        let graphs: Vec<(Uuid, u32, Trigger, ConcurrencyPolicy)> = {
            let s = state.store.lock().unwrap();
            s.iter()
                .filter(|(_, g)| g.enabled)
                .map(|(t, g)| (*t, g.id, g.trigger.clone(), g.concurrency.clone()))
                .collect()
        };

        // Drain new-alert levels ONCE per tenant per tick (advances + persist on advance).
        // The alert log is shared state — reload per tenant so alerts created by any
        // service drive `OnNewAlert` here too.
        let mut tenant_seen = std::collections::HashSet::new();
        let tenants: Vec<Uuid> = graphs.iter().map(|(t, ..)| *t).filter(|t| tenant_seen.insert(*t)).collect();
        let mut levels_by_tenant: HashMap<Uuid, Vec<app_core::alerts::AlertLevel>> = HashMap::new();
        for tenant in tenants {
            let alerts = app_core::alerts::repo::load(&state.db, tenant);
            let levels = state.engine.lock().unwrap().drain_new_alert_levels(tenant, &alerts);
            levels_by_tenant.insert(tenant, levels);
            let max = state.engine.lock().unwrap().alert_seen_max(tenant);
            if max > persisted_max.get(&tenant).copied().unwrap_or(0) {
                workflows::repo::save_alert_seen_max(&state.db, tenant, max);
                persisted_max.insert(tenant, max);
            }
        }

        for (tenant, id, trigger, policy) in graphs {
            if state.engine.lock().unwrap().total_running() >= WF_GLOBAL_CAP {
                break; // global backstop
            }
            let due = match &trigger {
                Trigger::Manual => false,
                Trigger::Cron { every_secs } => {
                    state.engine.lock().unwrap().cron_due(tenant, id, *every_secs, now)
                }
                Trigger::OnNewAlert { min_level } => {
                    let levels = levels_by_tenant.get(&tenant).map(|v| v.as_slice()).unwrap_or(&[]);
                    WorkflowEngine::levels_qualify(levels, *min_level)
                }
            };
            if due {
                if matches!(trigger, Trigger::Cron { .. }) {
                    workflows::repo::save_cron_fired(&state.db, tenant, id, crate::now_epoch_ms());
                }
                fire_graph(&state, tenant, id, &policy);
            }
        }
    });
}

/// Apply a graph's concurrency policy to a fire: spawn, queue, or skip.
pub fn fire_graph(state: &Arc<AppState>, tenant: Uuid, graph_id: u32, policy: &ConcurrencyPolicy) {
    let decision = state.engine.lock().unwrap().decide(tenant, graph_id, policy);
    match decision {
        Fire::Spawn => {
            state.engine.lock().unwrap().on_spawn(tenant, graph_id);
            spawn_graph_run(state, tenant, graph_id);
        }
        Fire::Queue => state.engine.lock().unwrap().on_queue(tenant, graph_id),
        Fire::Skip => {}
    }
}

/// Spawn one graph run on a background thread. The caller must have already reserved
/// a slot (`engine.on_spawn`). On finish it releases the slot and, if a queued fire
/// is waiting (`QueueSingle`), reserves + spawns the next one.
pub fn spawn_graph_run(state: &Arc<AppState>, tenant: Uuid, graph_id: u32) {
    let graph: WfGraph = match state::find_graph(&state.store, tenant, graph_id) {
        Some(g) => g,
        None => return,
    };
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        state.activity.lock().unwrap().entry((tenant, graph_id)).or_default().running += 1;
        events::publish(
            &state.events,
            "workflow-run-start",
            serde_json::json!({ "tenant": tenant, "graph": graph_id, "name": graph.name }),
        );

        let (ctx, drain) = build_run_context(&state, tenant);
        let progress = Arc::clone(&state);
        let mut on_node = |nid: u32| {
            progress.activity.lock().unwrap().entry((tenant, graph_id)).or_default().current = Some(nid);
            events::publish(&progress.events, "workflow-node", serde_json::json!({ "tenant": tenant, "graph": graph_id, "node": nid }));
        };
        let log_state = Arc::clone(&state);
        let log_graph = graph.clone();
        let mut on_log = |nid: u32, msg: &str, ms: u128| {
            let label = log_graph.node(nid).map(|n| n.kind.label()).unwrap_or("node");
            events::publish(
                &log_state.events,
                "workflow-log",
                serde_json::json!({ "tenant": tenant, "graph": graph_id, "node": nid, "label": label, "message": msg, "ms": ms as u64 }),
            );
        };
        let emit_state = Arc::clone(&state);
        let mut emit = |ev: &str| fire_event(&emit_state, tenant, graph_id, ev.to_string());
        let result = workflows::run_dag_eventful(&graph, &ctx, &mut on_node, &mut on_log, &mut emit);
        drop(ctx); // closes the UI channel → its drain thread ends
        let _ = drain.join();

        let (ok, summary) = match &result {
            Ok(outs) => (true, format!("ran {} node(s)", outs.len())),
            Err(e) => (false, e.clone()),
        };

        {
            let mut a = state.activity.lock().unwrap();
            let e = a.entry((tenant, graph_id)).or_default();
            if e.running > 0 {
                e.running -= 1;
            }
            if e.running == 0 {
                e.current = None;
            }
        }
        if ok {
            log::info!("workflow '{}' ({tenant}/{graph_id}) ok: {summary}", graph.name);
        } else {
            log::warn!("workflow '{}' ({tenant}/{graph_id}) failed: {summary}", graph.name);
        }
        events::publish(
            &state.events,
            "workflow-run",
            serde_json::json!({ "tenant": tenant, "graph": graph_id, "name": graph.name, "ok": ok, "summary": summary }),
        );

        // Release the slot; drain a queued fire if the policy buffered one.
        let drain_queued = state.engine.lock().unwrap().on_finish(tenant, graph_id);
        if drain_queued {
            state.engine.lock().unwrap().on_spawn(tenant, graph_id);
            spawn_graph_run(&state, tenant, graph_id);
        }
    });
}

/// Fire an in-graph event: (re)run the matching `OnEvent` handler subtree as a decoupled,
/// concurrent sub-run — UNLESS that `(tenant, graph, event)` handler is already running, in
/// which case the fire is dropped (coalesced). This is the async "skip if the path is still
/// running" behavior `EmitEvent` provides. Nested `EmitEvent`s inside a handler recurse here.
pub fn fire_event(state: &Arc<AppState>, tenant: Uuid, graph_id: u32, event: String) {
    // Resolve the subscriber; no `OnEvent` for this name → nothing to do.
    let Some(graph) = state::find_graph(&state.store, tenant, graph_id) else { return };
    let entry = match workflows::on_event_node(&graph, &event) {
        Some(entry) => entry,
        None => return,
    };
    // Guard: one handler run per (tenant, graph, event) at a time — coalesce-drop.
    let key = (tenant, graph_id, event.clone());
    {
        let mut guard = state.event_runs.lock().unwrap();
        if !guard.insert(key.clone()) {
            log::debug!("event '{event}' ({tenant}/{graph_id}) dropped — handler still running");
            return;
        }
    }

    let state = Arc::clone(state);
    std::thread::spawn(move || {
        events::publish(&state.events, "workflow-event", serde_json::json!({ "tenant": tenant, "graph": graph_id, "event": event }));
        let (ctx, drain) = build_run_context(&state, tenant);
        let prog = Arc::clone(&state);
        let mut on_node = |nid: u32| {
            events::publish(&prog.events, "workflow-node", serde_json::json!({ "tenant": tenant, "graph": graph_id, "node": nid }));
        };
        let log_state = Arc::clone(&state);
        let log_graph = graph.clone();
        let mut on_log = |nid: u32, msg: &str, ms: u128| {
            let label = log_graph.node(nid).map(|n| n.kind.label()).unwrap_or("node");
            events::publish(
                &log_state.events,
                "workflow-log",
                serde_json::json!({ "tenant": tenant, "graph": graph_id, "node": nid, "label": label, "message": msg, "ms": ms as u64 }),
            );
        };
        let emit_state = Arc::clone(&state);
        let mut emit = |ev: &str| fire_event(&emit_state, tenant, graph_id, ev.to_string());
        let result = workflows::run_subpath_eventful(&graph, &ctx, entry, &mut on_node, &mut on_log, &mut emit);
        drop(ctx);
        let _ = drain.join();
        if let Err(e) = &result {
            log::warn!("event handler '{event}' ({tenant}/{graph_id}) failed: {e}");
        }
        state.event_runs.lock().unwrap().remove(&key);
    });
}

/// Build a [`ToolContext`] for one run of `tenant`, wired to the in-process ports, with
/// stores freshly loaded from Postgres (devices + beacons + alerts can change between
/// runs). The frame source is bound to THIS tenant's edge go2rtc (warm via the run's
/// keep-alive agent), so concurrent runs of different tenants never collide. The returned
/// `JoinHandle` is the thread draining UI side-effects to the SSE bus; it ends when the
/// context (and its `Sender`) is dropped.
fn build_run_context(state: &Arc<AppState>, tenant: Uuid) -> (ToolContext, std::thread::JoinHandle<()>) {
    let devices = Arc::new(std::sync::Mutex::new(app_core::devices::repo::load(&state.db, tenant)));
    let alerts = Arc::new(std::sync::Mutex::new(app_core::alerts::repo::load(&state.db, tenant)));
    let notif: NotificationStore =
        Arc::new(std::sync::Mutex::new(app_core::notification::repo::load_beacons(&state.db, tenant)));
    let notif_cfg = resolve_telegram_config(state, tenant);

    let (tx, rx) = std::sync::mpsc::channel::<ToolUiEvent>();
    let bus = state.events.clone();
    let drain = std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            events::publish_tool_event(&bus, tenant, ev);
        }
    });

    let mut ctx = ToolContext::new_local(
        devices,
        state.db.clone(),
        tenant,
        alerts,
        notif,
        notif_cfg,
        tx,
    );
    // Bind the frame source to THIS tenant's edge go2rtc. The per-run keep-alive agent
    // keeps the connection warm across every `GetFrame`/`Snapshot` in the run, so a
    // `memoize=false` node in a loop re-grabs a fresh frame without re-handshaking.
    ctx.frames = Arc::new(app_core::ports::EdgeFrameSource::new(state.edge_base(tenant)));
    (ctx, drain)
}

/// Resolve the active Telegram config for `tenant`: the saved config (Postgres) if it has
/// a non-empty token, else the environment.
fn resolve_telegram_config(state: &Arc<AppState>, tenant: Uuid) -> Option<NotificationConfig> {
    if let Some((bot_token, pairing_code)) = app_core::notification::repo::load_telegram(&state.db, tenant) {
        if !bot_token.trim().is_empty() {
            return Some(NotificationConfig { bot_token, pairing_code });
        }
    }
    NotificationConfig::from_env()
}
