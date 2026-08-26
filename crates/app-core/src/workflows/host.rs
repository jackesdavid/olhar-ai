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
    Host, HostError, Literals, NodeTarget, Observer, Slot, StateKey, StateStore, UiEvent, ValueIo,
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
/// The engine's `Graph<M>` carries id, name, nodes, edges and next_id — the parts any graph has
/// — and splices this in with `#[serde(flatten)]`, so every field below sits at the same level
/// of the document it always did. What changed is only which struct declares it, and therefore
/// who has an opinion about it: the engine has none about triggers, concurrency, templates, or
/// where somebody left a canvas scrolled to.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SentinelMeta {
    /// One-line summary shown as a caption under the name in the list.
    #[serde(default)]
    pub description: String,
    /// Organisational label. Workflows sharing a group cluster together in the list.
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trigger: super::domain::Trigger,
    #[serde(default)]
    pub concurrency: super::domain::ConcurrencyPolicy,
    /// What this graph runs one of. Old graphs deserialize to `Single`, which is exactly how
    /// they behaved before instances existed.
    #[serde(default)]
    pub instance_scope: super::domain::InstanceScope,
    /// Run nodes that have no edges. Default `false`, so leftover experiments do not fire.
    #[serde(default)]
    pub run_orphans: bool,
    /// Emit a per-node debug line during a run, for the editor console.
    #[serde(default)]
    pub debug: bool,
    /// A starting point to clone rather than a graph that runs on its own (#102).
    #[serde(default)]
    pub template: bool,
    /// A template visible to every tenant. Only a master can set it, and it means nothing
    /// unless `template` is also true.
    #[serde(default)]
    pub global: bool,
    /// Where the canvas was left. Here rather than in the engine: the engine has no canvas, and
    /// the forty-pixel starting offset is this editor's house style.
    #[serde(default)]
    pub viewport: super::domain::Viewport,
    /// Wire bend handles per edge, keyed `"from:from_port:to:to_port"`. Cosmetic — the logical
    /// edge is unchanged — so a user's wire organisation survives a reload.
    #[serde(default)]
    pub edge_vias: std::collections::HashMap<String, Vec<[i32; 2]>>,
    /// Typed per-graph variables. Each spawns Get/Set Variable nodes typed by its declaration.
    #[serde(default)]
    pub variables: Vec<super::domain::VariableDecl>,
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

/// Postgres, in the shape this product already stores.
///
/// Both translations here exist because the rows PREDATE the engine, and a store that only
/// understood the engine's shape would read every existing row as nonsense while the editor's live
/// value view (#94 phase 2) read the new ones the same way.
///
/// - **Values are wrapped by node id.** On disk a row's `values_json` is `{"<node>": {ports}}`,
///   one key, always this row's own node. The engine hands over and expects a bare port map, so
///   the envelope goes on here and comes off here. Writing the bare map instead would be
///   invisible until a window closed hours later and found nothing it recognised.
/// - **Variables have their own COLUMN.** `variables_json`, not a key inside `values_json`. This
///   used to route to the values row and read back `values_json["__variables"]` — a key nothing
///   ever wrote, so a `DelayTrigger`'s trailing edge restored an empty map every time and #114
///   came back silently, while the write clobbered the node's remembered values on its way past.
/// Put the on-disk envelope around a node's port map.
///
/// A row's `values_json` is `{"<node>": {ports}}` — one key, always this row's own node. Redundant
/// on the face of it, and kept because every row already written looks like this.
fn wrap_values(node: u32, ports: &Json) -> Json {
    serde_json::json!({ node.to_string(): ports })
}

/// Take it off again, returning `None` for a row that does not carry this node.
fn unwrap_values(node: u32, row: &Json) -> Option<Json> {
    row.get(node.to_string().as_str()).cloned()
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
                    .as_ref()
                    .and_then(|row| unwrap_values(t.node, row))
            }
            Slot::Variables => {
                repo::get_node_variables(&self.db, self.tenant, t.graph, t.node, &t.instance)
            }
        }
    }

    fn set(&self, key: &StateKey, value: &Json) {
        let t = &key.target;
        match slot_of(key) {
            Slot::State => {
                repo::set_node_state(&self.db, self.tenant, t.graph, t.node, &t.instance, value)
            }
            Slot::Values => repo::set_node_values(
                &self.db,
                self.tenant,
                t.graph,
                t.node,
                &t.instance,
                &wrap_values(t.node, value),
            ),
            Slot::Variables => {
                repo::set_node_variables(&self.db, self.tenant, t.graph, t.node, &t.instance, value)
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

impl UiObserver {
    /// Build one over a run's UI channel.
    ///
    /// Public because a product that replaces the whole observer still owes the editor its live
    /// view: it wraps one of these rather than restating the mapping from a node's output to what
    /// a person sees, which is the sort of duplicate that drifts quietly.
    pub fn new(tx: std::sync::mpsc::Sender<ToolUiEvent>) -> Self {
        UiObserver { tx }
    }
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

// ---------------------------------------------------------------------------------------
// The host
// ---------------------------------------------------------------------------------------

/// Reads a configuration literal for a typed port.
///
/// Two of the three cases need `sys` and so could never be a pure read of the config: a camera
/// literal is a device id resolved against the device store, a table literal a dictionary id
/// against the run-start snapshot. That is why the engine asks rather than working it out.
pub struct SentinelLiterals(Arc<ToolContext>);

impl Literals for SentinelLiterals {
    fn read(
        &self,
        _kind: &ggraph_core::NodeId,
        port: &ggraph_core::Port,
        config: &Json,
    ) -> Option<Value> {
        let name = port.name.as_str();
        let v = if port.ty == super::domain::types::CAMERA {
            super::node::resolve_camera_literal(config, name, &self.0)
        } else if port.ty == super::domain::types::TABLE {
            super::node::resolve_table_literal(config, name, &self.0)
        } else {
            super::node::literal_for_named(config, name, port.ty.clone())
        };
        v.as_ref().map(super::compat::to_engine)
    }
}

struct Inner {
    tools: Arc<ToolContext>,
    literals: SentinelLiterals,
    state: PgState,
    io: MediaIo,
    /// Boxed rather than the concrete [`UiObserver`], so a caller can watch a run without the
    /// editor being involved. The differential test that compares this product's scheduler with
    /// the engine's needs the node ORDER, and order only exists in the observer — the run's
    /// return value is a map.
    observer: Box<dyn Observer + Send + Sync>,
}

/// Everything the engine can reach, plus — through [`tools`](SentinelHost::tools) — everything
/// this product's own nodes reach, which is cameras, faces, recipients and the rest.
#[derive(Clone)]
pub struct SentinelHost(Arc<Inner>);

impl SentinelHost {
    pub fn new(tools: ToolContext) -> Self {
        let tools = Arc::new(tools);
        let state = PgState {
            db: tools.db.clone(),
            tenant: tools.tenant_id,
        };
        let observer = UiObserver {
            tx: tools.ui_tx.clone(),
        };
        SentinelHost(Arc::new(Inner {
            literals: SentinelLiterals(tools.clone()),
            state,
            io: MediaIo,
            observer: Box::new(observer),
            tools,
        }))
    }

    /// The same host, reporting to `observer` instead of to the editor.
    pub fn watching(tools: ToolContext, observer: Box<dyn Observer + Send + Sync>) -> Self {
        let tools = Arc::new(tools);
        let state = PgState {
            db: tools.db.clone(),
            tenant: tools.tenant_id,
        };
        SentinelHost(Arc::new(Inner {
            literals: SentinelLiterals(tools.clone()),
            state,
            io: MediaIo,
            observer,
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
        //
        // Delegated rather than restated, and that is the whole point. This used to read the
        // camera with `as_text`, which answers None for a payload carrying a camera ENTITY rather
        // than an ip — and the wire carries both, depending on whether `seed_event_payload` has
        // run yet. So a per-camera graph armed a window under "cam-1" and looked for it under ""
        // one function later. One rule, one implementation, no chance of the two drifting.
        SmolStr::new(
            meta.instance_scope
                .key_for(&super::compat::from_engine_values(payload)),
        )
    }

    fn literals(&self) -> &dyn Literals {
        &self.0.literals
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

#[cfg(test)]
mod storage_shape {
    use super::*;

    /// The envelope goes on and comes off. Trivial, and it is the assertion that was missing when
    /// the scheduler swap started writing bare port maps into rows every existing reader — the
    /// editor's live value view, a closing window — expected to be wrapped.
    #[test]
    fn a_node_reads_back_exactly_what_it_wrote() {
        let ports = serde_json::json!({ "camera": "cam-1", "clip_id": "abc" });
        assert_eq!(unwrap_values(7, &wrap_values(7, &ports)), Some(ports));
    }

    /// A row written before any of this, by the old scheduler, still reads.
    #[test]
    fn a_row_written_by_the_previous_scheduler_still_reads() {
        let row = serde_json::json!({ "7": { "camera": "cam-1" } });
        assert_eq!(
            unwrap_values(7, &row),
            Some(serde_json::json!({ "camera": "cam-1" }))
        );
    }

    /// Another node's row is not this node's values.
    #[test]
    fn a_row_belonging_to_another_node_reads_as_nothing() {
        let row = serde_json::json!({ "9": { "camera": "cam-1" } });
        assert_eq!(unwrap_values(7, &row), None);
    }

    /// The variables snapshot has no `__variables` key and never did.
    ///
    /// The state store used to read one out of the values row, so a DelayTrigger's trailing edge
    /// restored an empty map every single time and #114 came back without a word. It is a column
    /// of its own now; this pins the shape that made the old read impossible to satisfy.
    #[test]
    fn a_variables_snapshot_is_a_flat_map_of_names() {
        use crate::workflows::node::PortValue;
        let mut vars = std::collections::HashMap::new();
        vars.insert(
            "current_camera".to_string(),
            PortValue::Text("cam-1".into()),
        );
        let j = crate::workflows::values::variables_to_json(&vars);
        assert!(
            j.get("__variables").is_none(),
            "nothing writes that key, so nothing may read it: {j}"
        );
        assert!(
            j.get("current_camera").is_some(),
            "the names are the keys, tagged with their types underneath: {j}"
        );
        let back = crate::workflows::values::variables_from_json(&j);
        assert_eq!(
            back.get("current_camera").and_then(PortValue::as_text),
            Some("cam-1"),
            "and it comes back as what it went in as"
        );
    }
}

#[cfg(test)]
mod instance_keys {
    use super::*;
    use crate::workflows::domain::{CameraEntity, InstanceScope};
    use crate::workflows::node::PortValue;
    use ggraph_core::host::Host;

    fn host() -> SentinelHost {
        SentinelHost::new(crate::workflows::golden::test_ctx())
    }

    fn meta(scope: InstanceScope) -> SentinelMeta {
        let mut m = SentinelMeta::default();
        m.instance_scope = scope;
        m
    }

    fn payload(camera: PortValue) -> ggraph_core::PortValues {
        let mut legacy = crate::workflows::PortValues::new();
        legacy.insert("camera".into(), camera);
        super::super::compat::to_engine_values(&legacy)
    }

    /// A camera arriving as an ENTITY keys the same instance as one arriving as an ip.
    ///
    /// The wire carries both, depending on whether the event payload has been seeded yet, and a
    /// window can be armed on either side of that. Reading only the text form answered "" for the
    /// entity, so a per-camera graph armed under one key and looked under another — and a window
    /// that closes without finding what armed it looks exactly like a window that never armed.
    #[test]
    fn a_camera_keys_the_same_instance_however_it_arrived() {
        let h = host();
        let m = meta(InstanceScope::PerCamera);

        let as_text = h.instance_key(&m, &payload(PortValue::Text("cam-1".into())));
        let as_entity = h.instance_key(
            &m,
            &payload(PortValue::Camera(CameraEntity {
                id: "cam-1".into(),
                ip: "10.0.0.7".into(),
                name: "Front door".into(),
            })),
        );

        assert_eq!(as_text, "cam-1");
        assert_eq!(
            as_entity, as_text,
            "an entity and an ip name the same camera, so they must name the same instance"
        );
    }

    /// A single-instance graph has one instance, whatever the payload says.
    #[test]
    fn a_graph_that_is_not_per_camera_has_one_instance() {
        let h = host();
        let key = h.instance_key(
            &meta(InstanceScope::Single),
            &payload(PortValue::Text("cam-1".into())),
        );
        assert_eq!(
            key, "",
            "the single instance is the empty key, byte for byte"
        );
    }
}
