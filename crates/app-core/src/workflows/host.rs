//! `SentinelHost` — this product's answer to everything the engine asks for.
//!
//! The engine reaches the world through [`ggraph_core::Host`] and nothing else: no database
//! handle, no HTTP client, no clock. This module is where those traits meet what Sentinel
//! actually has, which is a [`ToolContext`].
//!
//! Note what is not a parameter anywhere below: the tenant. A [`PgState`] is built already
//! scoped to one, so the engine cannot address another — not by mistake, not by a shifted
//! argument. Multi-tenancy that relies on every call site remembering to pass an id is
//! multi-tenancy that fails once, and it failed here once already (2026-08-01, a removed bind
//! shifted every later placeholder and box authentication went down).

use crate::agent::tools::{ToolContext, ToolUiEvent};
use crate::db::Db;
use ggraph_core::host::{
    ApprovalRequest, Approvals, Host, HostError, Http, HttpRequest, HttpResponse, Llm, LlmRequest,
    NodeTarget, Observer, Slot, StateKey, StateStore, TableStore, UiEvent, ValueIo,
};
use ggraph_core::SmolStr;
use ggraph_core::Value;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::repo;

/// The per-graph fields that are this product's policy rather than the engine's topology.
///
/// They sit in [`Graph`](super::domain::Graph) today; when that becomes `Graph<SentinelMeta>`
/// they move here, and `#[serde(flatten)]` keeps them at exactly the same place in the document.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SentinelMeta {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trigger: super::domain::Trigger,
    #[serde(default)]
    pub concurrency: super::domain::ConcurrencyPolicy,
    #[serde(default)]
    pub instance_scope: super::domain::InstanceScope,
    #[serde(default)]
    pub run_orphans: bool,
    #[serde(default)]
    pub debug: bool,
}

// ---------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------

/// Durable node state, in Postgres, scoped to one tenant.
pub struct PgState {
    db: Db,
    tenant: Uuid,
}

/// The engine addresses three slots per node; this product stores them in three columns of one
/// row. Keeping the mapping in one function means a new slot cannot quietly land in the wrong
/// column.
fn slot_of(key: &StateKey) -> Slot {
    key.slot
}

impl StateStore for PgState {
    fn get(&self, key: &StateKey) -> Option<Json> {
        let t = &key.target;
        match slot_of(key) {
            Slot::State => {
                repo::get_node_state(&self.db, self.tenant, t.graph, t.node, &t.instance)
            }
            Slot::Values => {
                repo::get_node_values(&self.db, self.tenant, t.graph, t.node, &t.instance)
            }
            // The run's variable snapshot rides along with the values row.
            Slot::Variables => {
                repo::get_node_values(&self.db, self.tenant, t.graph, t.node, &t.instance)
                    .and_then(|v| v.get("__variables").cloned())
            }
        }
    }

    fn set(&self, key: &StateKey, value: &Json) {
        let t = &key.target;
        match slot_of(key) {
            Slot::State => {
                repo::set_node_state(&self.db, self.tenant, t.graph, t.node, &t.instance, value)
            }
            Slot::Values | Slot::Variables => {
                repo::set_node_values(&self.db, self.tenant, t.graph, t.node, &t.instance, value)
            }
        }
    }

    fn clear(&self, key: &StateKey) {
        let t = &key.target;
        repo::clear_node_state(&self.db, self.tenant, t.graph, t.node);
    }

    fn try_arm(&self, key: &StateKey, expires_at: i64) -> bool {
        let t = &key.target;
        repo::try_arm_node_state(
            &self.db,
            self.tenant,
            t.graph,
            t.node,
            &t.instance,
            &crate::timeutil::plus_secs_rfc3339(expires_at - now_secs()),
        )
    }

    fn extend(&self, key: &StateKey, expires_at: i64) {
        let t = &key.target;
        repo::extend_node_state(
            &self.db,
            self.tenant,
            t.graph,
            t.node,
            &t.instance,
            &crate::timeutil::plus_secs_rfc3339(expires_at - now_secs()),
        );
    }

    fn try_disarm_expired(&self, key: &StateKey, _now: i64) -> bool {
        // The `now` the engine offers is ignored on purpose: this store's backend has an
        // authoritative clock and the disarm is a single conditional UPDATE against it. Two pods
        // agree with Postgres; they do not necessarily agree with each other.
        let t = &key.target;
        repo::try_disarm_expired_node_state(&self.db, self.tenant, t.graph, t.node, &t.instance)
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------------------

/// Forwards what the engine reports to the editor's live view.
pub struct UiObserver {
    tx: std::sync::mpsc::Sender<ToolUiEvent>,
}

impl Observer for UiObserver {
    fn ui(&self, _node: u32, event: UiEvent) {
        let _ = match event {
            UiEvent::Image { label, bytes } => self.tx.send(ToolUiEvent::Snapshot {
                camera: label,
                jpeg: bytes,
            }),
            UiEvent::Value { label, value } => self.tx.send(ToolUiEvent::Print {
                message: if label.is_empty() {
                    value.summary()
                } else {
                    format!("{label}: {}", value.summary())
                },
                level: "info".into(),
                on_screen: false,
            }),
        };
    }
}

// ---------------------------------------------------------------------------------------
// Blobs
// ---------------------------------------------------------------------------------------

/// Large values go to object storage; what is persisted is a reference.
pub struct MediaIo;

impl ValueIo for MediaIo {
    fn enabled(&self) -> bool {
        crate::media::enabled()
    }

    fn put(&self, bytes: &[u8], mime: &str) -> Result<String, HostError> {
        let ext = mime.rsplit('/').next().unwrap_or("bin");
        crate::media::put(bytes, mime, ext).map_err(HostError::new)
    }

    fn get(&self, key: &str) -> Result<Vec<u8>, HostError> {
        crate::media::get(key).map_err(HostError::new)
    }
}

// ---------------------------------------------------------------------------------------
// The capabilities this product has not routed through the engine yet
// ---------------------------------------------------------------------------------------

/// Approvals, HTTP, models and tables reach Sentinel's nodes through `ToolContext` directly
/// rather than through these traits, because those nodes are this product's own and can.
///
/// They are wired to refuse rather than left unimplemented so the shape is honest: a graph that
/// somehow reaches an engine-provided node needing one gets a clear message instead of a panic.
pub struct NotRouted;

impl Approvals for NotRouted {
    fn ask(&self, _: ApprovalRequest) -> Result<Uuid, HostError> {
        Err(HostError::permanent(
            "approvals reach this product's Device Gate node directly, not through the engine",
        ))
    }
}

impl Http for NotRouted {
    fn send(&self, _: HttpRequest) -> Result<HttpResponse, HostError> {
        Err(HostError::permanent(
            "http reaches this product's HTTP Post node directly, not through the engine",
        ))
    }
}

impl Llm for NotRouted {
    fn ask_text(&self, _: LlmRequest) -> Result<String, HostError> {
        Err(HostError::permanent(
            "models reach this product's AI nodes directly",
        ))
    }
    fn ask_bool(&self, _: LlmRequest) -> Result<Option<bool>, HostError> {
        Err(HostError::permanent(
            "models reach this product's AI nodes directly",
        ))
    }
    fn classify(&self, _: LlmRequest, _: &[String]) -> Result<Option<String>, HostError> {
        Err(HostError::permanent(
            "models reach this product's AI nodes directly",
        ))
    }
}

impl TableStore for NotRouted {
    fn list(&self) -> Result<Vec<String>, HostError> {
        Ok(Vec::new())
    }
    fn read(&self, _: &str) -> Result<Vec<Vec<(String, Value)>>, HostError> {
        Err(HostError::permanent(
            "tables reach this product's Dictionary nodes directly",
        ))
    }
    fn row_count(&self, _: &str) -> Result<u64, HostError> {
        Ok(0)
    }
    fn append(&self, _: &str, _: &[(String, Value)]) -> Result<(), HostError> {
        Err(HostError::permanent(
            "tables reach this product's Dictionary nodes directly",
        ))
    }
    fn set_cell(&self, _: &str, _: u64, _: &str, _: &Value) -> Result<(), HostError> {
        Err(HostError::permanent(
            "tables reach this product's Dictionary nodes directly",
        ))
    }
    fn delete_row(&self, _: &str, _: u64) -> Result<(), HostError> {
        Err(HostError::permanent(
            "tables reach this product's Dictionary nodes directly",
        ))
    }
    fn clear(&self, _: &str) -> Result<(), HostError> {
        Err(HostError::permanent(
            "tables reach this product's Dictionary nodes directly",
        ))
    }
}

// ---------------------------------------------------------------------------------------
// The host
// ---------------------------------------------------------------------------------------

struct Inner {
    tools: ToolContext,
    state: PgState,
    io: MediaIo,
    /// Boxed rather than the concrete [`UiObserver`], so a caller can watch a run without the
    /// editor being involved. The differential test that compares this product's scheduler with
    /// the engine's needs the node ORDER, and order only exists in the observer — the run's
    /// return value is a map.
    observer: Box<dyn Observer + Send + Sync>,
    not_routed: NotRouted,
    vars: Mutex<HashMap<String, Value>>,
}

/// Everything the engine can reach, plus — through [`tools`](SentinelHost::tools) — everything
/// this product's own nodes reach, which is cameras, faces, recipients and the rest.
#[derive(Clone)]
pub struct SentinelHost(Arc<Inner>);

impl SentinelHost {
    pub fn new(tools: ToolContext) -> Self {
        let state = PgState {
            db: tools.db.clone(),
            tenant: tools.tenant_id,
        };
        let observer = UiObserver {
            tx: tools.ui_tx.clone(),
        };
        SentinelHost(Arc::new(Inner {
            state,
            io: MediaIo,
            observer: Box::new(observer),
            not_routed: NotRouted,
            vars: Mutex::new(HashMap::new()),
            tools,
        }))
    }

    /// The same host, reporting to `observer` instead of to the editor.
    pub fn watching(tools: ToolContext, observer: Box<dyn Observer + Send + Sync>) -> Self {
        let state = PgState {
            db: tools.db.clone(),
            tenant: tools.tenant_id,
        };
        SentinelHost(Arc::new(Inner {
            state,
            io: MediaIo,
            observer,
            not_routed: NotRouted,
            vars: Mutex::new(HashMap::new()),
            tools,
        }))
    }

    /// This product's world. Sentinel's own nodes are written against the concrete host, so they
    /// reach cameras and faces here without any of it appearing in an engine trait.
    pub fn tools(&self) -> &ToolContext {
        &self.0.tools
    }
}

impl std::fmt::Debug for SentinelHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SentinelHost(run {})", self.0.tools.run_id)
    }
}

impl Host for SentinelHost {
    type Meta = SentinelMeta;

    fn state(&self) -> &dyn StateStore {
        &self.0.state
    }
    fn io(&self) -> &dyn ValueIo {
        &self.0.io
    }
    fn observer(&self) -> &dyn Observer {
        self.0.observer.as_ref()
    }
    fn approvals(&self) -> &dyn Approvals {
        &self.0.not_routed
    }
    fn http(&self) -> &dyn Http {
        &self.0.not_routed
    }
    fn llm(&self) -> &dyn Llm {
        &self.0.not_routed
    }
    fn tables(&self) -> &dyn TableStore {
        &self.0.not_routed
    }
    fn vars(&self) -> &Mutex<HashMap<String, Value>> {
        &self.0.vars
    }
    fn run_id(&self) -> Uuid {
        self.0.tools.run_id
    }
    fn now_epoch_secs(&self) -> i64 {
        now_secs()
    }

    fn instance_key(&self, meta: &SentinelMeta, payload: &ggraph_core::PortValues) -> SmolStr {
        // "One run per camera" is a rule about cameras, and it belongs here rather than in an
        // engine that has never heard of one. The engine only needs two signals with different
        // keys not to share node state.
        match meta.instance_scope {
            super::domain::InstanceScope::Single => SmolStr::default(),
            super::domain::InstanceScope::PerCamera => payload
                .get("camera")
                .and_then(Value::as_text)
                .map(|s| SmolStr::new(s.trim()))
                .unwrap_or_default(),
        }
    }

    fn literal(
        &self,
        kind: &ggraph_core::NodeId,
        port: &ggraph_core::Port,
        config: &Json,
    ) -> Option<Value> {
        // Two of these need `sys` and so cannot be a pure read of the config: a camera literal is
        // a device id resolved against the device store, and a table literal is a dictionary id
        // resolved against the run-start snapshot. That is exactly why this is a host method and
        // not something the engine could work out on its own.
        let name = port.name.as_str();
        let v = if port.ty == super::domain::types::CAMERA {
            super::node::resolve_camera_literal(config, name, &self.0.tools)
        } else if port.ty == super::domain::types::TABLE {
            super::node::resolve_table_literal(config, name, &self.0.tools)
        } else {
            super::node::literal_for_named(config, name, port.ty.clone())
        };
        let _ = kind;
        v.as_ref().map(super::compat::to_engine)
    }

    fn schedule(&self, at_epoch_secs: i64, target: NodeTarget) -> Result<(), HostError> {
        crate::work_queue::enqueue_window_stop_blocking(
            &self.0.tools.db,
            self.0.tools.tenant_id,
            target.graph,
            target.node,
            &target.instance,
            &crate::timeutil::plus_secs_rfc3339(at_epoch_secs - now_secs()),
        );
        Ok(())
    }
}
