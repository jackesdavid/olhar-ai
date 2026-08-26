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
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use app_core::agent::{ToolContext, ToolUiEvent};
use app_core::notification::{NotificationConfig, NotificationStore};
use app_core::workflows::{
    self, ConcurrencyPolicy, EventSource, Fire, Graph as WfGraph, PortValues, RunLogLine,
    RunStatus, TriggerSource,
};

use crate::events;
use crate::state::{self, Activity, AppState, GraphActivity, WF_GLOBAL_CAP};
use app_core::workflows::domain::AsKind;

/// Total in-flight runs across every tenant/graph — cron, manual **and** event handlers.
///
/// The global backstop reads this rather than `WorkflowEngine::runs`: that counter is only
/// bumped by `on_spawn`, which just `fire_graph` (cron/manual) calls, so it cannot see an
/// event storm at all. The activity map is the one place every run registers, via
/// `mark_run_started` / `mark_run_finished` on both spawn paths.
fn total_in_flight(activity: &Activity) -> u32 {
    activity.lock().unwrap().values().map(|a| a.running).sum()
}

// The 1 s trigger poller lived here. It is GONE with `Trigger::Cron` (migration `0095`): scheduling
// is a node property now, and every recurring fire arrives through `schedule_consumer` — a durable
// `work_jobs` row per cron node, which survives a pod restart instead of re-arming from an
// in-memory `Instant`. Nothing polls graphs on an interval any more.

/// Apply a graph's concurrency policy to a fire: spawn, queue, or skip. `source` records
/// what kicked it off (manual / cron) on the run record.
pub fn fire_graph(
    state: &Arc<AppState>,
    tenant: Uuid,
    graph_id: Uuid,
    policy: &ConcurrencyPolicy,
    source: TriggerSource,
    // Manual-run input overrides (#102), raw strings by variable name; empty for cron/event.
    inputs: std::collections::HashMap<String, String>,
) {
    let decision = state
        .engine
        .lock()
        .unwrap()
        .decide(tenant, graph_id, policy);
    match decision {
        Fire::Spawn => {
            state.engine.lock().unwrap().on_spawn(tenant, graph_id);
            spawn_graph_run(state, tenant, graph_id, source, inputs);
        }
        Fire::Queue => state.engine.lock().unwrap().on_queue(tenant, graph_id),
        Fire::Skip => {}
    }
}

/// Spawn one graph run on a background thread. The caller must have already reserved
/// a slot (`engine.on_spawn`). On finish it releases the slot and, if a queued fire
/// is waiting (`QueueSingle`), reserves + spawns the next one. `source` (manual/cron) is
/// recorded on the run.
pub fn spawn_graph_run(
    state: &Arc<AppState>,
    tenant: Uuid,
    graph_id: Uuid,
    source: TriggerSource,
    inputs: std::collections::HashMap<String, String>,
) {
    let graph: WfGraph = match state::find_graph(&state.store, tenant, graph_id) {
        Some(g) => g,
        None => return,
    };
    let state = Arc::clone(state);
    let run_id = Uuid::new_v4();
    // Register BEFORE the thread starts: the global backstop counts this map, so a burst
    // could otherwise spawn past the ceiling while the threads were still starting up.
    mark_run_started(&state, tenant, graph_id, run_id, source.as_str());
    std::thread::spawn(move || {
        // Card #146: the central "root place" for tenant context. Flow is multi-tenant in one pod,
        // so every log emitted during this run (engine + bridged `log::*` node sites) inherits
        // `tenant`/`graph`/`run` from this span — no log call passes tenant_id by hand.
        let _span = tracing::info_span!("run", tenant = %tenant, graph = %graph_id, run = %run_id)
            .entered();
        workflows::repo::insert_run(
            &state.db,
            tenant,
            run_id,
            graph_id,
            &graph.name,
            source,
            None,
            None,
            None,
            &state.pod_name,
        );
        app_core::platform_logs::insert_log_sync(
            &state.db,
            app_core::platform_logs::PlatformLog {
                tenant_id: tenant,
                module: "workflow",
                trace_id: Some(run_id),
                entity_id: Some(graph_id.to_string()),
                entity_type: Some("workflow_graph"),
                level: "info",
                message: format!("Run started: {}", graph.name),
                payload: Some(
                    serde_json::json!({ "trigger": source.as_str(), "graph_name": graph.name }),
                ),
            },
        );
        events::publish(
            &state.events,
            "workflow-run-start",
            serde_json::json!({ "tenant": tenant, "graph": graph_id, "name": graph.name, "run_id": run_id, "trigger": source.as_str() }),
        );

        let (ok, summary) = execute_run(
            &state,
            tenant,
            &graph,
            run_id,
            None,
            PortValues::new(),
            inputs.clone(),
        );

        mark_run_finished(&state, tenant, graph_id);
        if ok {
            log::info!(
                "workflow '{}' ({tenant}/{graph_id}) ok: {summary}",
                graph.name
            );
        } else {
            log::warn!(
                "workflow '{}' ({tenant}/{graph_id}) failed: {summary}",
                graph.name
            );
        }
        events::publish(
            &state.events,
            "workflow-run",
            serde_json::json!({ "tenant": tenant, "graph": graph_id, "name": graph.name, "ok": ok, "summary": summary, "run_id": run_id }),
        );

        // Release the slot; drain a queued fire if the policy buffered one.
        let drain_queued = state.engine.lock().unwrap().on_finish(tenant, graph_id);
        if drain_queued {
            state.engine.lock().unwrap().on_spawn(tenant, graph_id);
            spawn_graph_run(
                &state,
                tenant,
                graph_id,
                source,
                std::collections::HashMap::new(),
            ); // v1: queued drain uses defaults
        }
    });
}

/// Mark a run started in the live activity map: bump the in-flight count and record the
/// current run id + trigger source (shown in the graph list). Pushes the new snapshot to
/// the UI over SSE (`workflow-activity`) so the list badges update without polling.
fn mark_run_started(
    state: &Arc<AppState>,
    tenant: Uuid,
    graph_id: Uuid,
    run_id: Uuid,
    trigger: &str,
) {
    let snapshot = {
        let mut a = state.activity.lock().unwrap();
        let e = a.entry((tenant, graph_id)).or_default();
        e.running += 1;
        e.run_id = Some(run_id);
        e.trigger = Some(trigger.to_string());
        e.pod_name = Some(state.pod_name.clone());
        e.clone()
    };
    publish_activity(state, tenant, graph_id, &snapshot);
}

/// Mark a run finished in the live activity map: drop the in-flight count, clearing the
/// current node / run id / trigger once the last run of this graph completes. Pushes the
/// new snapshot to the UI over SSE (`workflow-activity`).
fn mark_run_finished(state: &Arc<AppState>, tenant: Uuid, graph_id: Uuid) {
    let snapshot = {
        let mut a = state.activity.lock().unwrap();
        let e = a.entry((tenant, graph_id)).or_default();
        if e.running > 0 {
            e.running -= 1;
        }
        if e.running == 0 {
            e.current = None;
            e.run_id = None;
            e.trigger = None;
            e.pod_name = None;
        }
        e.clone()
    };
    publish_activity(state, tenant, graph_id, &snapshot);
}

/// Push one graph's live activity snapshot to the UI over the SSE bus as `workflow-activity`
/// (`{ tenant, graph, running, current, run_id, trigger, pod_name }`). This replaces the old
/// 700 ms `GET /runs` poll: the UI seeds once on connect (REST) then updates from these deltas.
fn publish_activity(state: &Arc<AppState>, tenant: Uuid, graph_id: Uuid, snapshot: &GraphActivity) {
    let mut payload = serde_json::to_value(snapshot).unwrap_or_else(|_| serde_json::json!({}));
    if let serde_json::Value::Object(ref mut map) = payload {
        map.insert("tenant".into(), serde_json::json!(tenant));
        map.insert("graph".into(), serde_json::json!(graph_id));
    }
    events::publish(&state.events, "workflow-activity", payload);
}

/// The shared run body: builds the tenant run context, wires SSE progress + run-log
/// persistence (per-node lines only when the graph has debug on; the run-level summary
/// always), runs either the full DAG (`entry = None`) or an `OnEvent` subtree, finalizes
/// the run record, and returns `(ok, summary)`.
fn execute_run(
    state: &Arc<AppState>,
    tenant: Uuid,
    graph: &WfGraph,
    run_id: Uuid,
    entry: Option<u32>,
    entry_payload: PortValues,
    // Per-launch input values (#102), raw strings by variable name. Empty for cron and for
    // event/timer sub-runs (those get their data from `entry_payload`, a different channel). A
    // manual run carries what the user typed in the prompt. Kept as raw strings so they're parsed
    // HERE with this run's real context, rather than building a throwaway one in the HTTP handler.
    input_overrides: std::collections::HashMap<String, String>,
) -> (bool, String) {
    let (ctx, drain) = build_run_context(state, tenant, graph.id, run_id);
    // Seed this run's variables from the graph's declared defaults (card #5b) — a Get before any
    // Set then returns the author's default instead of a type-zero.
    app_core::workflows::node::seed_variable_defaults(&graph.meta.variables, &ctx);
    // Then apply this run's input overrides ON TOP, so a value entered at launch beats the stored
    // default while cron/event runs keep using the default (they pass none). Parsed against each
    // variable's declared type; unknown names / unparseable values are dropped (the variable then
    // keeps its default), so a stale prompt can't fail the run.
    if !input_overrides.is_empty() {
        for decl in &graph.meta.variables {
            if let Some(raw) = input_overrides.get(&decl.name) {
                if let Some(pv) =
                    app_core::workflows::node::parse_variable_value(decl.ty.clone(), raw, &ctx)
                {
                    ctx.variables.lock().unwrap().insert(decl.name.clone(), pv);
                }
            }
        }
    }
    let graph_id = graph.id;
    let debug = graph.meta.debug;
    let labels: std::collections::HashMap<u32, &'static str> = graph
        .nodes
        .iter()
        .map(|n| {
            (
                n.id,
                n.kind
                    .as_kind()
                    .map(|k| k.label())
                    .unwrap_or("unknown node"),
            )
        })
        .collect();
    // Owned, not `&'static str`: a kind's slug now comes from the stored document rather than
    // from an enum, and a document can name a kind this build does not have a constant for.
    let slugs: std::collections::HashMap<u32, String> = graph
        .nodes
        .iter()
        .map(|n| (n.id, n.kind.as_str().to_string()))
        .collect();
    let logs = Arc::new(Mutex::new(Vec::<RunLogLine>::new()));

    // Everything the run reports to a person goes through one observer.
    //
    // It used to be four closures handed to this product's own scheduler. As closures they could
    // only ever be handed to THAT scheduler; as an Observer they are handed to whichever one is
    // running, which is what lets the engine's scheduler drive a real Sentinel run.
    let watcher = FlowObserver {
        state: Arc::clone(state),
        tenant,
        graph_id,
        run_id,
        debug,
        labels,
        slugs,
        logs: Arc::clone(&logs),
        ui: workflows::host::UiObserver::new(ctx.ui_tx.clone()),
    };

    // Wrapped, because a pure source activates on every epoch whether or not the branch it feeds
    // was ever reached, and one that nobody read has no business writing a line or lighting up
    // (#115, #205). The rule lives in the wrapper so it holds under either scheduler.
    let watcher = app_core::workflows::observer::DeferringObserver::new(graph, watcher, |n| {
        n.kind.as_kind().is_some_and(|k| k.is_pure_source())
    });

    let start = match entry {
        Some(at) => workflows::Start::At(at, entry_payload),
        None => workflows::Start::Whole,
    };

    let host = workflows::host::SentinelHost::watching(ctx, Box::new(watcher));
    let result = workflows::run_graph(graph, &host, start).map(|outputs| outputs.len());

    drop(host); // closes the UI channel → its drain thread ends
    let _ = drain.join();

    let (ok, summary) = match &result {
        Ok(n) => (true, format!("ran {n} node(s)")),
        Err(e) => (false, e.clone()),
    };
    let buffered = logs.lock().unwrap().clone();
    workflows::repo::insert_run_logs(&state.db, run_id, &buffered);
    // Mirror run log lines to the unified platform_logs table so the /logs page can
    // show detailed execution traces, cross-linked by trace_id = run_id.
    if !buffered.is_empty() {
        let plogs: Vec<app_core::platform_logs::PlatformLog> = buffered
            .iter()
            .filter(|l| l.kind != "node") // "node" entries are UI-only (live node highlights); skip them
            .map(|l| app_core::platform_logs::PlatformLog {
                tenant_id: tenant,
                module: "workflow",
                trace_id: Some(run_id),
                entity_id: Some(graph.id.to_string()),
                entity_type: Some("workflow_graph"),
                level: if l.kind == "log" { "info" } else { "debug" },
                message: l.message.clone(),
                payload: Some(serde_json::json!({
                    "kind": l.kind,
                    "node_id": l.node_id,
                    "label": l.label,
                    "node_kind": l.node_kind,
                })),
            })
            .collect();
        app_core::platform_logs::insert_logs_sync(&state.db, plogs);
    }
    workflows::repo::finish_run(
        &state.db,
        run_id,
        if ok { RunStatus::Ok } else { RunStatus::Failed },
        &summary,
    );
    app_core::platform_logs::insert_log_sync(
        &state.db,
        app_core::platform_logs::PlatformLog {
            tenant_id: tenant,
            module: "workflow",
            trace_id: Some(run_id),
            entity_id: Some(graph.id.to_string()),
            entity_type: Some("workflow_graph"),
            level: if ok { "info" } else { "error" },
            message: format!("Run {}: {}", if ok { "ok" } else { "failed" }, summary),
            payload: Some(
                serde_json::json!({ "status": if ok { "ok" } else { "failed" }, "summary": summary }),
            ),
        },
    );
    (ok, summary)
}

/// Fire an event into the tenant bus. Every ENABLED graph in `tenant` with an `OnEvent`
/// for `event` spawns its handler subtree as a decoupled, concurrent sub-run — unless that
/// listening graph's `(tenant, graph, event)` handler is already running, in which case the
/// fire is dropped for that graph (coalesced) and logged as a skip. The emitter logs the
/// dispatch; each listener logs that it was triggered (by which graph) or skipped.
/// `emitting_run_id` is the emitter's run, so the dispatch line attaches to its history.
///
/// Returns the number of enabled listener graphs the event matched on THIS pod. A durable inbound
/// event (from the `event_consumer`) that returns 0 is a red flag — the graph likely exists but this
/// pod's in-memory store is stale (a graph edit/reload only reached another replica), so the event is
/// silently consumed with no run. The consumer logs that case to Datadog.
pub fn fire_event(
    state: &Arc<AppState>,
    tenant: Uuid,
    source: EventSource,
    event: String,
    emitting_run_id: Option<Uuid>,
    payload: PortValues,
) -> usize {
    // The firing event's own camera (if it has one — most system events don't), + this tenant's
    // devices, so `on_event_node` can gate delivery to only the `OnEvent` nodes actually scoped
    // to that camera (a plain config literal, or wired from `GetCameraByName`) instead of firing
    // every listener in the tenant regardless of which camera the event is about.
    let event_camera = payload
        .get("camera")
        .and_then(app_core::workflows::PortValue::as_text)
        .map(str::to_string);

    // Record every deliberate graph emit (the EmitEvent node) as a first-class occurrence on the
    // agnostic event stream (card #66, Phase 4). This makes EmitEvent double as the "push a custom
    // event" node without a new NodeKind — the fan-out to OnEvent listeners below is unchanged.
    // System-sourced fires (motion/tracking storms) are deliberately left out to keep the stream
    // meaningful; they already surface downstream as presence/alert events.
    if let EventSource::Graph { id, name } = &source {
        let mut ev = app_core::events::NewEvent::new(tenant, "workflow.custom", "workflow");
        ev.title = Some(event.clone());
        ev.message = Some(format!("'{name}' emitted '{event}'"));
        ev.camera = event_camera.clone();
        ev.data =
            Some(serde_json::json!({ "event": event, "graph": name, "graph_id": id.to_string() }));
        app_core::events::insert_event_sync(&state.db, ev);
    }

    let devices = app_core::devices::repo::load(&state.db, tenant);

    // Snapshot listeners (don't hold the store lock while spawning).
    let listeners: Vec<(Uuid, String, u32)> = {
        let s = state.store.lock().unwrap();
        s.iter()
            .filter(|(t, g)| *t == tenant && g.meta.enabled)
            .filter_map(|(_, g)| {
                workflows::on_event_node(g, &event, event_camera.as_deref(), &devices)
                    .map(|e| (g.id, g.name.clone(), e))
            })
            .collect()
    };
    let (emitter_id, emitter_name) = match &source {
        EventSource::Graph { id, name } => (Some(*id), name.clone()),
        EventSource::System { label } => (None, format!("system:{label}")),
    };

    // Dispatch log on the emitter (live + history).
    let names: Vec<String> = listeners.iter().map(|(_, n, _)| n.clone()).collect();
    events::publish(
        &state.events,
        "workflow-event",
        serde_json::json!({ "tenant": tenant, "graph": emitter_id, "event": event, "kind": "dispatch", "by_graph": emitter_name, "dispatched_to": names, "count": listeners.len() }),
    );
    if let Some(rid) = emitting_run_id {
        workflows::repo::insert_run_logs(
            &state.db,
            rid,
            &[RunLogLine {
                node_id: None,
                label: None,
                kind: "dispatch".into(),
                node_kind: None,
                message: format!("Dispatched '{}' to {} listener(s)", event, listeners.len()),
            }],
        );
    }

    let listener_count = listeners.len();

    for (listener_id, listener_name, entry) in listeners {
        // Global backstop: don't let an event storm exceed the pod's run ceiling.
        if total_in_flight(&state.activity) >= WF_GLOBAL_CAP {
            events::publish(
                &state.events,
                "workflow-event",
                serde_json::json!({ "tenant": tenant, "graph": listener_id, "event": event, "kind": "skip", "by_graph": emitter_name, "reason": "capacity" }),
            );
            continue;
        }
        // Coalesce-drop per listening graph, PER INSTANCE: respect the graph's concurrency limit
        // within an instance, and let different instances run side by side.
        //
        // The key used to be (tenant, graph, event) with no context, which made this the first of
        // three layers that assumed one context per graph — and the one that hid the other two. With
        // `SkipIfRunning` (the default), camera B's PersonDetection was dropped here while camera A's
        // run was in flight, so B never reached the window it couldn't have armed anyway. Adding the
        // instance means one run per instance, concurrent ACROSS instances, which is what a
        // per-camera graph means. `Single` scope keys on "" and behaves exactly as before.
        let (max_allowed, instance_key): (usize, String) = {
            let s = state.store.lock().unwrap();
            s.iter()
                .find(|(t, g)| *t == tenant && g.id == listener_id)
                .map(|(_, g)| {
                    let max = match &g.meta.concurrency {
                        app_core::workflows::domain::ConcurrencyPolicy::Concurrent {
                            max_concurrent,
                        } => *max_concurrent as usize,
                        _ => 1,
                    };
                    (max, g.meta.instance_scope.key_for(&payload))
                })
                .unwrap_or((1, String::new()))
        };
        let key = (tenant, listener_id, format!("{event}\u{1}{instance_key}"));
        {
            let mut guard = state.event_runs.lock().unwrap();
            let count = guard.entry(key.clone()).or_insert(0);
            if *count >= max_allowed {
                events::publish(
                    &state.events,
                    "workflow-event",
                    serde_json::json!({ "tenant": tenant, "graph": listener_id, "event": event, "kind": "skip", "by_graph": emitter_name }),
                );
                if let Some(rid) = emitting_run_id {
                    workflows::repo::insert_run_logs(
                        &state.db,
                        rid,
                        &[RunLogLine {
                            node_id: None,
                            label: None,
                            kind: "skip".into(),
                            node_kind: None,
                            message: format!("Listener '{listener_name}' skipped '{event}' — {count}/{max_allowed} handlers running"),
                        }],
                    );
                }
                continue;
            }
            *count += 1;
        }
        let _ = listener_name; // listener's display name comes from its graph in the handler
        spawn_event_handler(
            state,
            tenant,
            listener_id,
            entry,
            event.clone(),
            emitter_id,
            emitter_name.clone(),
            key,
            payload.clone(),
        );
    }
    listener_count
}

/// Emit a tenant event from system code (login, user-create, chat, alerts…). The seam for
/// future `System-*` events — drives the same fan-out as a graph emit, with no emitting run.
/// Unused today; wired up when system emitters land.
#[allow(dead_code)]
pub fn fire_system_event(state: &Arc<AppState>, tenant: Uuid, label: &str, event: String) {
    fire_event(
        state,
        tenant,
        EventSource::System {
            label: label.to_string(),
        },
        event,
        None,
        PortValues::new(),
    );
}

/// Run one node's subtree as its own run — the delivery half of a durable timer (card #86).
///
/// Forces `node_id` so only it and its live descendants execute; nothing else in the graph is
/// seeded. Two callers: a `DelayTrigger` whose quiet window elapsed (payload carries the window
/// marker so the node can tell this from a re-trigger), and a `When Event` node in cron mode.
///
/// `source` and `payload` are parameters rather than hardcoded because addressing a single node is
/// the shared mechanism, and the reason for doing it is not. Two cron nodes in one graph rely on
/// this: each fire wakes ONE entry point, so nothing else in the graph runs.
pub fn spawn_node_subrun(
    state: &Arc<AppState>,
    tenant: Uuid,
    graph_id: Uuid,
    node_id: u32,
    source: TriggerSource,
    payload: PortValues,
) {
    let graph: WfGraph = match state::find_graph(&state.store, tenant, graph_id) {
        Some(g) => g,
        None => return,
    };
    let state = Arc::clone(state);
    let run_id = Uuid::new_v4();
    // Register before the thread starts — see spawn_graph_run.
    mark_run_started(&state, tenant, graph_id, run_id, source.as_str());
    std::thread::spawn(move || {
        workflows::repo::insert_run(
            &state.db,
            tenant,
            run_id,
            graph_id,
            &graph.name,
            source,
            None,
            None,
            None,
            &state.pod_name,
        );
        events::publish(
            &state.events,
            "workflow-run-start",
            serde_json::json!({ "tenant": tenant, "graph": graph_id, "name": graph.name, "run_id": run_id, "trigger": source.as_str() }),
        );

        let (ok, summary) = execute_run(
            &state,
            tenant,
            &graph,
            run_id,
            Some(node_id),
            payload,
            std::collections::HashMap::new(),
        );

        mark_run_finished(&state, tenant, graph_id);
        if !ok {
            log::warn!(
                "{} sub-run ({tenant}/{graph_id} node {node_id}) failed: {summary}",
                source.as_str()
            );
        }
        events::publish(
            &state.events,
            "workflow-run",
            serde_json::json!({ "tenant": tenant, "graph": graph_id, "name": graph.name, "ok": ok, "summary": summary, "run_id": run_id }),
        );
    });
}

/// Spawn one `OnEvent` handler sub-run for a listening graph (the coalesce key is already
/// reserved by the caller and released here on finish). Visible like any other run: it gets
/// a run record (trigger = event, source = the emitter) and live activity.
#[allow(clippy::too_many_arguments)]
fn spawn_event_handler(
    state: &Arc<AppState>,
    tenant: Uuid,
    listener_id: Uuid,
    entry: u32,
    event: String,
    emitter_id: Option<Uuid>,
    emitter_name: String,
    key: (Uuid, Uuid, String),
    payload: PortValues,
) {
    let graph: WfGraph = match state::find_graph(&state.store, tenant, listener_id) {
        Some(g) => g,
        None => {
            decrement_event_run(&state.event_runs, &key);
            return;
        }
    };
    let state = Arc::clone(state);
    let run_id = Uuid::new_v4();
    // Register BEFORE the thread starts — see spawn_graph_run. This is the storm path the
    // backstop exists for, so the dispatch loop must see each handler the moment it's taken.
    mark_run_started(&state, tenant, listener_id, run_id, "event");
    std::thread::spawn(move || {
        workflows::repo::insert_run(
            &state.db,
            tenant,
            run_id,
            listener_id,
            &graph.name,
            TriggerSource::Event,
            emitter_id,
            Some(&emitter_name),
            Some(&event),
            &state.pod_name,
        );
        events::publish(
            &state.events,
            "workflow-event",
            serde_json::json!({ "tenant": tenant, "graph": listener_id, "event": event, "kind": "trigger", "by_graph": emitter_name, "run_id": run_id }),
        );
        events::publish(
            &state.events,
            "workflow-run-start",
            serde_json::json!({ "tenant": tenant, "graph": listener_id, "name": graph.name, "run_id": run_id, "trigger": "event" }),
        );
        workflows::repo::insert_run_logs(
            &state.db,
            run_id,
            &[RunLogLine {
                node_id: None,
                label: None,
                kind: "trigger".into(),
                node_kind: None,
                message: format!("Triggered by {emitter_name} via event '{event}'"),
            }],
        );

        let (ok, summary) = execute_run(
            &state,
            tenant,
            &graph,
            run_id,
            Some(entry),
            payload,
            std::collections::HashMap::new(),
        );

        mark_run_finished(&state, tenant, listener_id);
        if !ok {
            log::warn!("event handler '{event}' ({tenant}/{listener_id}) failed: {summary}");
        }
        events::publish(
            &state.events,
            "workflow-run",
            serde_json::json!({ "tenant": tenant, "graph": listener_id, "name": graph.name, "ok": ok, "summary": summary, "run_id": run_id }),
        );
        decrement_event_run(&state.event_runs, &key);
    });
}

fn decrement_event_run(
    event_runs: &Arc<Mutex<std::collections::HashMap<(Uuid, Uuid, String), usize>>>,
    key: &(Uuid, Uuid, String),
) {
    let mut guard = event_runs.lock().unwrap();
    if let Some(count) = guard.get_mut(key) {
        if *count <= 1 {
            guard.remove(key);
        } else {
            *count -= 1;
        }
    }
}

/// Build a [`ToolContext`] for one run of `tenant`, wired to the in-process ports, with
/// stores freshly loaded from Postgres (devices + beacons + alerts can change between
/// keep-alive agent), so concurrent runs of different tenants never collide. The returned
/// `JoinHandle` is the thread draining UI side-effects to the SSE bus; it ends when the
/// context (and its `Sender`) is dropped.
fn build_run_context(
    state: &Arc<AppState>,
    tenant: Uuid,
    graph_id: Uuid,
    run_id: Uuid,
) -> (ToolContext, std::thread::JoinHandle<()>) {
    let devices = Arc::new(std::sync::Mutex::new(app_core::devices::repo::load(
        &state.db, tenant,
    )));
    let notif: NotificationStore = Arc::new(std::sync::Mutex::new(
        app_core::notification::repo::load_beacons(&state.db, tenant),
    ));
    let notif_cfg = resolve_telegram_config(state, tenant);

    let (tx, rx) = std::sync::mpsc::channel::<ToolUiEvent>();
    let bus = state.events.clone();
    let node_frames = state.node_frames.clone();
    let drain = std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            // ShowImage carries raw jpeg bytes — stash them for out-of-band fetch
            // (`GET /runs/frame`); the SSE event itself only carries a reference.
            if let ToolUiEvent::ShowImage {
                graph, node, jpeg, ..
            } = &ev
            {
                node_frames
                    .lock()
                    .unwrap()
                    .insert((tenant, *graph, *node), jpeg.clone());
            }
            events::publish_tool_event(&bus, tenant, graph_id, ev);
        }
    });

    let mut ctx = ToolContext::new_local(devices, state.db.clone(), tenant, notif, notif_cfg, tx);
    // The event bus, reachable by the emitting node itself rather than through a scheduler
    // callback. `fire_event` needs this runtime's own state — the listener registry, the tenant's
    // devices for camera-scoped delivery — none of which belongs in an engine trait.
    // What a person reads when this run asks them something. Looked up rather than threaded
    // through the signature: the id is what identifies the graph, and only messages meant for
    // people need the name.
    ctx.graph_name = crate::state::find_graph(&state.store, tenant, graph_id)
        .map(|g| g.name)
        .unwrap_or_default();
    ctx.emitter = Some(Arc::new(BusEmitter {
        state: Arc::clone(state),
        tenant,
    }));
    // keeps the connection warm across every `GetFrame`/`Snapshot` in the run, so a
    // `memoize=false` node in a loop re-grabs a fresh frame without re-handshaking.
    ctx.frames = Arc::new(app_core::ports::EdgeFrameSource::with_sse(
        state.edge_base(tenant),
        state.db.clone(),
        tenant,
    ));
    // Notifications go through the durable queue — this run only ENQUEUEs; the
    // sentinel-notification service consumes + sends (card #57). Image-bearing sends are
    // still delivered in-process for now (until the object-store media handoff).
    // Feature-flagged (NOTIFY_QUEUE) so flow is safe to deploy before the consumer is up:
    // with the flag unset it keeps sending in-process (LocalNotifier); flip it on once the
    // sentinel-notification service is confirmed running, or queued sends pile up unconsumed.
    if std::env::var("NOTIFY_QUEUE").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) {
        ctx.notifier = Arc::new(app_core::ports::RemoteNotifier {
            db: state.db.clone(),
            tenant_id: tenant,
        });
    }
    ctx.active_gates = std::sync::Arc::clone(&state.active_gates);
    // So a Device Gate can record WHICH run asked (#176). Two entries in the history, joined
    // by this column — see the field's doc on ToolContext.
    ctx.run_id = run_id;
    // Snapshot the tenant's dictionaries (#113) for run-start Table resolution + read nodes. Mutate
    // nodes bypass this snapshot and hit the DB row under a lock, so cross-run writes stay visible.
    ctx.dictionaries = app_core::dictionaries::list(&state.db, tenant)
        .into_iter()
        .map(|d| (d.id.to_string(), d))
        .collect();
    (ctx, drain)
}

/// Resolve the active Telegram config for `tenant`: the saved config (Postgres) if it has
/// a non-empty token, else the environment.
fn resolve_telegram_config(state: &Arc<AppState>, tenant: Uuid) -> Option<NotificationConfig> {
    // tenant-own bot → shared GLOBAL provider bot → env. The global fallback is the fix: a tenant
    // relying on the shared bot used to get "not configured" here even though pairing/test worked.
    app_core::notification::repo::resolve_effective_telegram(&state.db, tenant)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn activity_with(runs: &[(&str, u32)]) -> Activity {
        let mut m: HashMap<(Uuid, Uuid), GraphActivity> = HashMap::new();
        for (trigger, running) in runs {
            m.insert(
                (Uuid::new_v4(), Uuid::new_v4()),
                GraphActivity {
                    running: *running,
                    trigger: Some((*trigger).to_string()),
                    ..Default::default()
                },
            );
        }
        Arc::new(Mutex::new(m))
    }

    /// The bug: the backstop summed `WorkflowEngine::runs`, which only `fire_graph`
    /// (cron/manual) feeds — so an event storm read as 0 and the cap never engaged.
    /// Whatever the backstop counts must include event-triggered runs.
    #[test]
    fn total_in_flight_counts_event_runs_not_just_cron() {
        let only_events = activity_with(&[("event", 5)]);
        assert_eq!(total_in_flight(&only_events), 5, "event runs must count");

        let mixed = activity_with(&[("cron", 2), ("event", 3), ("manual", 1)]);
        assert_eq!(total_in_flight(&mixed), 6, "every trigger source counts");

        assert_eq!(total_in_flight(&activity_with(&[])), 0);
    }

    /// Graphs that have finished settle back to 0 and stop occupying the ceiling.
    #[test]
    fn total_in_flight_ignores_settled_graphs() {
        let a = activity_with(&[("event", 0), ("cron", 4)]);
        assert_eq!(total_in_flight(&a), 4);
    }
}

/// Fires a graph's events onto this runtime's bus.
///
/// The graph's NAME is resolved here rather than passed in: it is for display, and the id is
/// what identifies the emitter. A caller holding only an id should not have to carry a label
/// around to fire an event.
/// What a run tells the person watching it.
///
/// Four different audiences, one per method, and they are not interchangeable:
///
/// - `node_started` → the canvas highlight, plus the live activity map that says which graph is
///   running what right now;
/// - `arm` → which EDGE control travelled along. Told only the destination node an editor has to
///   guess the edge, and the guess breaks the moment two branches converge on one node (#206);
/// - `node_finished` → a line in the Console, and a row in the run's stored log. Only when the
///   graph has debug on: this is the gate, and it is here rather than in a scheduler because
///   whether a line is worth writing is a fact about the product, not about execution;
/// - `ui` → what a node produced for somebody to LOOK at, a snapshot or a printed value. Forwarded
///   to a UiObserver rather than restated, so there is one mapping and not two.
struct FlowObserver {
    state: Arc<AppState>,
    tenant: Uuid,
    graph_id: Uuid,
    run_id: Uuid,
    debug: bool,
    /// Node id → its human label, for the log line. Snapshotted before the run because the graph
    /// is not borrowed once it starts.
    labels: std::collections::HashMap<u32, &'static str>,
    /// Node id → its kind slug. Owned, not `&'static str`: a kind's slug comes from the stored
    /// document now, and a document can name a kind this build has no constant for.
    slugs: std::collections::HashMap<u32, String>,
    /// The run's log lines, collected here and written once at the end. Shared with the caller,
    /// which is what reads them back out.
    logs: Arc<Mutex<Vec<RunLogLine>>>,
    ui: workflows::host::UiObserver,
}

impl workflows::Observer for FlowObserver {
    fn node_started(&self, nid: u32) {
        let snapshot = {
            let mut a = self.state.activity.lock().unwrap();
            let e = a.entry((self.tenant, self.graph_id)).or_default();
            e.current = Some(nid);
            e.clone()
        };
        publish_activity(&self.state, self.tenant, self.graph_id, &snapshot);
        events::publish(
            &self.state.events,
            "workflow-node",
            serde_json::json!({
                "tenant": self.tenant, "graph": self.graph_id,
                "node": nid, "run_id": self.run_id
            }),
        );
        if self.debug {
            self.logs.lock().unwrap().push(RunLogLine {
                node_id: Some(nid),
                label: self.labels.get(&nid).map(|s| s.to_string()),
                kind: "node".into(),
                node_kind: self.slugs.get(&nid).map(|s| s.to_string()),
                message: self.labels.get(&nid).copied().unwrap_or("node").to_string(),
            });
        }
    }

    fn arm(&self, nid: u32, port: &str) {
        events::publish(
            &self.state.events,
            "workflow-arm",
            serde_json::json!({
                "tenant": self.tenant, "graph": self.graph_id, "node": nid,
                "port": port, "run_id": self.run_id
            }),
        );
    }

    fn node_finished(&self, nid: u32, msg: &str, ms: u128) {
        if !self.debug {
            return;
        }
        let label = self.labels.get(&nid).copied().unwrap_or("node");
        let slug = self.slugs.get(&nid).cloned().unwrap_or_default();
        events::publish(
            &self.state.events,
            "workflow-log",
            serde_json::json!({
                "tenant": self.tenant, "graph": self.graph_id, "node": nid, "label": label,
                "node_kind": slug, "message": msg, "ms": ms as u64, "run_id": self.run_id
            }),
        );
        self.logs.lock().unwrap().push(RunLogLine {
            node_id: Some(nid),
            label: Some(label.to_string()),
            kind: "log".into(),
            node_kind: Some(slug),
            message: msg.to_string(),
        });
    }

    fn ui(&self, node: u32, event: workflows::UiEvent) {
        self.ui.ui(node, event);
    }
}

struct BusEmitter {
    state: Arc<AppState>,
    tenant: Uuid,
}

impl app_core::workflows::emit::EventEmitter for BusEmitter {
    fn emit(&self, graph: Uuid, event: &str, run_id: Uuid, payload: &PortValues) {
        // Looked up rather than carried: the name is for display, and a caller holding an id
        // should not have to carry a label around to fire an event.
        let name = crate::state::find_graph(&self.state.store, self.tenant, graph)
            .map(|g| g.name)
            .unwrap_or_else(|| graph.to_string());
        // The listener count is only actionable for durable inbound events (see the consumer),
        // so it is discarded here.
        let _ = fire_event(
            &self.state,
            self.tenant,
            EventSource::Graph { id: graph, name },
            event.to_string(),
            Some(run_id),
            payload.clone(),
        );
    }
}
