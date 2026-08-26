//! What a node is once it is on a canvas, and the policies a graph runs under.
//!
//! [`NodeExt`] answers what a node's ports actually are — derived from its configuration, not from
//! its kind alone. Eight kinds have ports that depend on how the user filled them in: a switch's
//! arms are the arms somebody typed, a multi-variable read has one port per variable named.
//!
//! The rest is policy — what starts a run, what counts as one instance of a graph, and what
//! happens when a run is asked for while one is already going.

use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Graph elements
// ---------------------------------------------------------------------------

/// A node in a stored graph — the engine's.
///
/// Its `kind` is a [`NodeId`](ggraph_core::NodeId), an open string, rather than this product's
/// closed enum. That is what lets a document name a kind this build has not been taught yet
/// instead of failing to deserialize the whole graph. Compare one against [`NodeKind`] directly:
/// the impls above make that read the way it always did.
pub type GraphNode = ggraph_core::GraphNode;

/// What a node in THIS product's graphs can answer, beyond what any node can.
///
/// A trait rather than an inherent impl, because the node type is the engine's now. The
/// methods are unchanged; only where they are declared moved.
pub trait NodeExt {
    fn cron(&self) -> Option<CronConfig>;
    fn input_ports(&self) -> Vec<ResolvedPort>;
    fn output_ports(&self) -> Vec<ResolvedPort>;
    fn input_port(&self, name: &str) -> Option<ResolvedPort>;
    fn output_port(&self, name: &str) -> Option<ResolvedPort>;
}

impl NodeExt for GraphNode {
    /// This node's cron configuration, or `None` when it is a plain event listener.
    ///
    /// Requires BOTH `source == "cron"` and a parseable schedule. A node marked cron whose schedule
    /// is malformed returns `None` rather than a default: silently scheduling something the user
    /// did not describe is worse than not scheduling it, and an entry point that never fires is
    /// visible in a way that one firing hourly-by-accident is not.
    fn cron(&self) -> Option<CronConfig> {
        if self.kind != NodeKind::OnEvent {
            return None;
        }
        if self.config.get("source").and_then(|v| v.as_str()) != Some("cron") {
            return None;
        }
        let schedule: Schedule =
            serde_json::from_value(self.config.get("schedule")?.clone()).ok()?;
        let grace_secs = self
            .config
            .get("grace_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_CRON_GRACE_SECS);
        Some(CronConfig {
            schedule,
            grace_secs,
        })
    }

    /// This node's **effective** input ports. Static for every kind except `EmitEvent`,
    /// whose inputs are one-per-payload-field derived from its config (see [`payload_ports`]).
    fn input_ports(&self) -> Vec<ResolvedPort> {
        match self.kind.as_kind() {
            Some(NodeKind::EmitEvent) => payload_ports(&self.config),
            // Set Variable: one `value` input typed by the variable (required).
            Some(NodeKind::SetVariable) => variable_port(&self.config, true),
            // Set Variables: one typed input per variable in `config.variables` (see the fn).
            Some(NodeKind::MultiSetVariable) => multi_variable_ports(&self.config),
            // Control Tuya Device: one `value` input typed by the selected data point (optional —
            // config literal is the fallback). See `tuya_value_port`.
            Some(NodeKind::TuyaControl) => tuya_value_port(&self.config),
            // OnEvent: camera/region inputs, only for the edge tracker's events (see the fn doc).
            Some(NodeKind::OnEvent) => on_event_input_ports(&self.config),
            // DictMakeRow: `schema` (Table) + one input per configured column (see `dict_row_ports`).
            Some(NodeKind::DictMakeRow) => dict_row_ports(&self.config),
            Some(k) => k.input_ports().iter().cloned().collect(),
            // A kind this build does not know has no ports we can name. An empty list is what a caller reads as 'nothing to wire', which is true.
            None => Vec::new(),
        }
    }

    /// This node's **effective** output ports. Static for every kind except `OnEvent`,
    /// whose outputs are one-per-payload-field derived from its config.
    fn output_ports(&self) -> Vec<ResolvedPort> {
        match self.kind.as_kind() {
            Some(NodeKind::OnEvent) => payload_ports(&self.config),
            // Get Variable: one `value` output typed by the variable.
            Some(NodeKind::GetVariable) => variable_port(&self.config, false),
            // Get Variables: one typed output per variable in `config.variables`.
            Some(NodeKind::MultiGetVariable) => multi_variable_ports(&self.config),
            // Read Tuya Status: `value` typed by the selected data point (+ static `on`/`raw`).
            Some(NodeKind::TuyaReadStatus) => tuya_read_output_ports(&self.config),
            // AI Extract: `value` typed per config.type (like a variable) + a static `found` bool.
            Some(NodeKind::AiExtract) => {
                let mut ports = variable_port(&self.config, false);
                ports.push(ResolvedPort {
                    name: PortName::new("found".to_string()),
                    ty: types::BOOL,
                    required: false,
                });
                ports
            }
            Some(k) => k.output_ports().iter().cloned().collect(),
            // Same as the inputs: unknown kind, nothing to offer.
            None => Vec::new(),
        }
    }

    /// Look up one input port by name (config-aware), incl. the universal `exec_in`.
    fn input_port(&self, name: &str) -> Option<ResolvedPort> {
        if name == EXEC_IN.name {
            return Some(EXEC_IN.clone());
        }
        self.input_ports().into_iter().find(|p| p.name == name)
    }

    /// Look up one output port by name (config-aware), incl. the kind's exec_out pins.
    fn output_port(&self, name: &str) -> Option<ResolvedPort> {
        // StringSwitch / AiSwitch: exec outputs are user-defined arm labels + the fixed "default"
        // arm. These are instance-level (stored in config), not on the static kind. AiSwitch shares
        // StringSwitch's arm shape — it just picks the arm by meaning instead of exact match.
        if self.kind == NodeKind::StringSwitch || self.kind == NodeKind::AiSwitch {
            let arms = string_switch_arms(&self.config);
            if arms.iter().any(|a| a == name) || name == "default" {
                return Some(ResolvedPort {
                    name: PortName::new(name.to_string()),
                    ty: types::EXEC,
                    required: false,
                });
            }
            return None;
        }
        // CameraSwitch: exec outputs are numbered "arm_1", "arm_2", ... + "default".
        if self.kind == NodeKind::CameraSwitch {
            let arms = camera_switch_arms(&self.config);
            let valid =
                (1..=arms.len()).any(|i| name == format!("arm_{i}").as_str()) || name == "default";
            if valid {
                return Some(ResolvedPort {
                    name: PortName::new(name.to_string()),
                    ty: types::EXEC,
                    required: false,
                });
            }
            return None;
        }
        if let Some(e) = self
            .kind
            .as_kind()
            .map(|k| k.exec_out_ports())
            .unwrap_or(&[])
            .iter()
            .find(|p| p.name == name)
        {
            return Some(e.clone());
        }
        self.output_ports().into_iter().find(|p| p.name == name)
    }
}

/// Default grace for a LATE cron fire, in seconds (#cron-node).
///
/// A fire missed while flow was down runs if it is less late than this, and is skipped otherwise.
/// 15 minutes covers the realistic outage — a deploy, a restart, a pod reschedule — and is short
/// enough that a scheduled light or lock never switches in a visibly wrong part of the day.
///
/// The behaviour it replaces was unbounded catch-up, and it was accidental rather than chosen: the
/// job simply waited in `work_jobs` with a past `run_after` and fired whenever flow returned, so a
/// missed bedtime switch could land at breakfast.
pub const DEFAULT_CRON_GRACE_SECS: u64 = 15 * 60;

/// A `When Event` node configured as a schedule rather than an event listener.
///
/// The selector lives in the node's config instead of in a separate `NodeKind` because everything
/// downstream is identical: it is an entry point, it seeds a sub-run, and the graph reads the same
/// either way. One entry-point concept in the catalog, two sources.
pub struct CronConfig {
    pub schedule: Schedule,
    pub grace_secs: u64,
}

fn default_true() -> bool {
    true
}

/// Extract the arm labels from a `StringSwitch` node's config. The "default" arm is
/// implicit — it fires when no arm label matches — and is NOT included in this list.
pub fn string_switch_arms(config: &Value) -> Vec<String> {
    config
        .get("arms")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Extract the camera keys from a `CameraSwitch` node's config (one per arm; may be empty
/// strings for unconfigured arms). Arm port names are "arm_1", "arm_2", etc. (NOT the key).
/// The "default" arm is implicit — fires when no arm matches — and is NOT included here.
pub fn camera_switch_arms(config: &Value) -> Vec<String> {
    config
        .get("arms")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// How a graph fires **as a whole** — which, since the cron selector landed on `When Event`, is
/// only ever "someone or something started it at the top". Everything recurring is a NODE now.
///
/// Two variants used to live here and are gone (migration `0095`):
///
///  * `Cron { every_secs }` — an interval poller in `sentinel-flow`.
///  * `Schedule(Schedule)` — a calendar, added by #111.
///
/// They were removed because a graph could carry a schedule AND hold cron nodes with their own,
/// and nothing said which won; one production graph really did run on two clocks at once. The node
/// is the better home for the same reason it always was: one graph, several independent schedules,
/// each firing only what hangs off it.
///
/// The one thing the node cannot do is a SUB-MINUTE interval — `Cron { every_secs: 2 }` was a legal
/// polling loop and a calendar's finest grain is the minute. A graph that needs to poll faster than
/// that builds it out of an exec loop and a `Delay`, or, far better, listens for the event it was
/// polling for.
///
/// Alert reactivity used to live here too, as `OnNewAlert`; it was retired in favor of the
/// cross-graph event system (a future `System-Alerts` event an `OnEvent` can listen for).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Trigger {
    /// Fired by Run-now, by a cron-mode `When Event` node coming due, or by an event handler.
    /// Kept as an enum rather than collapsed away so the stored `"trigger":"Manual"` in every
    /// graph's JSON still deserializes, and so a future top-level trigger has somewhere to go.
    Manual,
}

/// A recurring calendar schedule. Wall-clock, timezone-aware, minute granularity.
///
/// Outlived the graph-level `Trigger::Schedule` that introduced it: cron-mode `When Event` nodes
/// store one of these in their config (`GraphNode::cron`), and `schedule_consumer` does the
/// calendar math against it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub freq: ScheduleFreq,
    /// IANA timezone (e.g. `America/Sao_Paulo`); empty → UTC.
    #[serde(default)]
    pub tz: String,
    /// Local times of day to fire at, each `"HH:MM"`. Used by Daily/Weekly/Monthly/Yearly (one entry
    /// = "daily at a time"; several = "N times a day"). Ignored by Hourly.
    #[serde(default)]
    pub times: Vec<String>,
    /// Hourly: the minute of each hour to fire (0–59).
    #[serde(default)]
    pub minute: u8,
    /// Weekly: weekdays to fire on, 0=Mon … 6=Sun.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// Monthly/Yearly: day of month (1–31; clamped to the month's length).
    #[serde(default)]
    pub day_of_month: u8,
    /// Yearly: month (1–12).
    #[serde(default)]
    pub month: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleFreq {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Default for Trigger {
    fn default() -> Self {
        Trigger::Manual
    }
}

/// What kicked off a run — recorded on every run so the UI can show who/what triggered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerSource {
    Manual,
    Cron,
    /// A cross-graph (or future system) event fired this graph's `OnEvent` handler.
    Event,
    /// A durable timer came due and forced a run at one node — today, a `DelayTrigger`
    /// whose quiet window elapsed, delivering its trailing `stop`.
    Timer,
}

impl TriggerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerSource::Manual => "manual",
            TriggerSource::Cron => "cron",
            TriggerSource::Event => "event",
            TriggerSource::Timer => "timer",
        }
    }
}

/// Terminal state of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Running,
    Ok,
    Failed,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Ok => "ok",
            RunStatus::Failed => "failed",
        }
    }
}

/// Who emitted an event into the tenant bus. A graph today; system code (login,
/// user-create, chat, alerts…) in the future via the same fan-out.
#[derive(Debug, Clone, PartialEq)]
pub enum EventSource {
    Graph { id: Uuid, name: String },
    System { label: String },
}

/// What a graph runs **one of** — its instance identity, in Unreal's sense: the graph is the
/// Blueprint, an instance is the actor. Latent state (a `DelayTrigger`'s window) and the
/// concurrency gate are both scoped to an instance, so two instances never collide.
///
/// Before this existed, everything was keyed `(tenant, graph, …)` with no context, which meant one
/// window and one in-flight run per graph *forever*. Two cameras through one recording graph:
/// B's event was coalesce-dropped while A ran, and if it did get through, B couldn't arm its own
/// window — it silently extended A's, and B's clip was never stopped. Single-camera graphs worked
/// by accident.
///
/// A latent node never asks "which camera"; it asks for **its instance's** window. That's what
/// keeps `DelayTrigger` a Control node that knows nothing about cameras.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InstanceScope {
    /// One instance per graph — byte-for-byte the behaviour before instances existed. Default, so
    /// every existing graph is unaffected.
    Single,
    /// One instance per camera: the event's camera is the instance key. Same camera re-triggers the
    /// same window (the debounce survives); different cameras get their own window and their own
    /// clip.
    PerCamera,
}

impl Default for InstanceScope {
    fn default() -> Self {
        InstanceScope::Single
    }
}

impl InstanceScope {
    /// This signal's instance key. `""` is the single instance — never a missing value: a
    /// `PerCamera` graph fired by an event with no camera has one honest instance, not a broken one.
    pub fn key_for(&self, payload: &crate::workflows::PortValues) -> String {
        match self {
            InstanceScope::Single => String::new(),
            InstanceScope::PerCamera => payload
                .get("camera")
                .map(|v| match v {
                    // The wire carries a camera as Text (ip/id) before `seed_event_payload` upgrades
                    // it; after, it's a CameraEntity. Accept both — the arm can happen either side.
                    crate::workflows::PortValue::Camera(c) => c.id.clone(),
                    other => other.as_text().unwrap_or_default().to_string(),
                })
                .unwrap_or_default(),
        }
    }
}

/// What happens when a graph fires while a previous run of the same graph is in flight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConcurrencyPolicy {
    /// ≤1 run; a fire while running is dropped.
    SkipIfRunning,
    /// ≤1 run; extra fires queue up to `max_queued`, then drop.
    QueueSingle { max_queued: u32 },
    /// Up to `max_concurrent` runs at once; a fire at capacity is dropped.
    Concurrent { max_concurrent: u32 },
}

impl Default for ConcurrencyPolicy {
    fn default() -> Self {
        ConcurrencyPolicy::SkipIfRunning
    }
}
