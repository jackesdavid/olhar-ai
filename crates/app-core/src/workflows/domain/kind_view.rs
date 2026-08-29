//! `NodeKind` as a view over the engine's open identifier.
//!
//! The engine names a node with an open string. This product still has an enum, and wants one: it
//! is exhaustive, the compiler checks a `match` over it, and every call site written against it
//! keeps working. Both are true at once because the enum is not the identifier — it is a VIEW over
//! it, with conversions in both directions and equality that compares the underlying slug.
//!
//! That is what made adopting the engine a refactor rather than a migration. A stored graph names
//! a kind this build has no constant for, and nothing breaks: the id keeps the name, and
//! `as_kind()` simply answers `None`.

use super::*;
use serde_json::Value;

// ---------------------------------------------------------------------------------------
// NodeKind as a view over the engine's open identifier
// ---------------------------------------------------------------------------------------
//
// A stored graph names kinds as strings, and the engine keeps them that way — which is what
// lets a document mention a kind this build has not been taught yet instead of failing to
// deserialize. This product still reasons about them as an enum in the places that have not
// been split into one file per node, and these three impls let both be true at once:
//
//   * a `NodeId` compares equal to the `NodeKind` it names, in either direction;
//   * a `NodeKind` converts into a `NodeId` wherever one is expected;
//   * `as_kind()` goes back, and answers `None` for a kind this build does not know.
//
// The last one is the important one. A wrong answer there would be a node quietly treated as
// some other node; an absence is something a caller has to decide about.

impl PartialEq<NodeKind> for ggraph_core::NodeId {
    fn eq(&self, other: &NodeKind) -> bool {
        self.as_str() == other.slug()
    }
}

impl PartialEq<ggraph_core::NodeId> for NodeKind {
    fn eq(&self, other: &ggraph_core::NodeId) -> bool {
        self.slug() == other.as_str()
    }
}

impl From<NodeKind> for ggraph_core::NodeId {
    fn from(k: NodeKind) -> Self {
        // `new_static`: a slug is a `&'static str`, so this never allocates.
        ggraph_core::NodeId::new_static(k.slug())
    }
}

/// This product's view of a stored kind, where it still needs one.
pub trait AsKind {
    /// `None` for a kind this build does not know — an older graph meeting a newer deploy, or
    /// the reverse. Callers decide what that means rather than being handed a wrong answer.
    fn as_kind(&self) -> Option<NodeKind>;
}

impl AsKind for ggraph_core::NodeId {
    fn as_kind(&self) -> Option<NodeKind> {
        NodeKind::from_slug(self.as_str())
    }
}

/// The payload ports baked into an `EmitEvent`/`OnEvent` node's config (`config.payload`
/// is `[{ name, type }]`, copied from the event's registry schema at config time). Empty
/// when the event is payload-less or unset. Used as inputs on `EmitEvent`, outputs on
/// `OnEvent`.
pub(super) fn payload_ports(config: &Value) -> Vec<ResolvedPort> {
    config
        .get("payload")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let name = f.get("name").and_then(|v| v.as_str())?.trim().to_string();
                    if name.is_empty() {
                        return None;
                    }
                    let ty = f
                        .get("type")
                        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())?;
                    Some(ResolvedPort::new(PortName::new(name), ty, false))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Map a Dictionary column's cell-type string (`text`/`num`/`bool`/`camera`/`recipient`) to the
/// `PortType` a `DictMakeRow` column input carries. Unknown → `Text`.
fn cell_type_to_port(ty: &str) -> PortType {
    match ty {
        "num" => types::NUM,
        "bool" => types::BOOL,
        "camera" => types::CAMERA,
        "recipient" => types::RECIPIENT,
        _ => types::TEXT,
    }
}

/// The config-derived input ports of a `DictMakeRow`: the fixed `schema` (a Table, required —
/// structural/UI dependency) plus one optional input per configured column, typed by the column's
/// cell type. Mirrors [`payload_ports`]: the runtime column list is baked into `config.columns`.
pub(super) fn dict_row_ports(config: &Value) -> Vec<ResolvedPort> {
    let mut ports = vec![ResolvedPort::new(PortName::new("schema".to_string()), types::TABLE, true)];
    if let Some(cols) = config.get("columns").and_then(|v| v.as_array()) {
        for c in cols {
            let name = match c.get("name").and_then(|v| v.as_str()) {
                Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                _ => continue,
            };
            let ty = c.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            ports.push(ResolvedPort::new(PortName::new(name), cell_type_to_port(ty), false));
        }
    }
    ports
}

/// The single typed port of a Get/Set Variable node — derived from `config.type` (the
/// variable's declared wire type, baked into the node at create time, same as `payload_ports`
/// bakes event fields). `required` is set true for the Set value input, false for the Get
/// output. Defaults to `Text` when the type is unset/unknown.
pub(super) fn variable_port(config: &Value, required: bool) -> Vec<ResolvedPort> {
    let ty = config
        .get("type")
        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())
        .unwrap_or(types::TEXT);
    vec![ResolvedPort::new(PortName::new("value".to_string()), ty, required)]
}

/// The ports of a `MultiSetVariable` / `MultiGetVariable` — one per entry in `config.variables`,
/// which is `[{ name, type }]`, the same shape a `VariableDecl` serialises to. The port is NAMED
/// after the variable, which is what makes the node readable on the canvas: eight labelled pins
/// instead of eight nodes each saying `value`.
///
/// Nothing here is required. On the Set side that is load-bearing rather than lax — an unwired
/// input means "leave this variable alone", so the same node can sit on two branches that each
/// write a different subset. Marking them required would turn that into a validation error.
///
/// Blank and duplicate names are dropped: a port with no name cannot be wired to, and two ports
/// sharing one would make `PortValues` (a map) silently keep only the last.
pub(super) fn multi_variable_ports(config: &Value) -> Vec<ResolvedPort> {
    let mut ports: Vec<ResolvedPort> = Vec::new();
    let Some(list) = config.get("variables").and_then(|v| v.as_array()) else {
        return ports;
    };
    for entry in list {
        let name = match entry.get("name").and_then(|v| v.as_str()) {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => continue,
        };
        if ports.iter().any(|p| p.name == name) {
            continue;
        }
        let ty = entry
            .get("type")
            .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())
            .unwrap_or(types::TEXT);
        ports.push(ResolvedPort::new(PortName::new(name), ty, false));
    }
    ports
}

/// `TuyaControl`'s `value` input port, typed by the selected data point (baked into `value_type`
/// by the editor when a `tuya_code` is chosen: `bool`/`num`/`text`). Wireable so a dynamic value
/// can drive the device; when unwired the node falls back to the config literal. Mirrors
/// `variable_port` (Set Variable). Optional — a config literal covers the common static case.
pub(super) fn tuya_value_port(config: &Value) -> Vec<ResolvedPort> {
    let ty = config
        .get("value_type")
        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())
        .unwrap_or(types::BOOL);
    vec![ResolvedPort::new(PortName::new("value".to_string()), ty, false)]
}

/// `TuyaReadStatus`'s outputs: `value` typed by the selected data point (`value_type`, like the
/// control node's input) + the static `on` (Bool convenience) + `raw` (full status JSON). So the
/// read value wires straight into the right type (e.g. `countdown_1` → Num).
pub(super) fn tuya_read_output_ports(config: &Value) -> Vec<ResolvedPort> {
    let ty = config
        .get("value_type")
        .and_then(|v| serde_json::from_value::<PortType>(v.clone()).ok())
        .unwrap_or(types::TEXT);
    vec![
        ResolvedPort::new(PortName::new("value".to_string()), ty, false),
        ResolvedPort::new(PortName::new("on".to_string()), types::BOOL, false),
        ResolvedPort::new(PortName::new("raw".to_string()), types::TEXT, false),
    ]
}

/// `OnEvent`'s extra, config-derived input ports: `camera` + `region`, wireable (from
/// `GetCameraByName` / `Region`) — present only when the configured event is one of the edge
/// tracker's three. These GATE delivery (see `engine::on_event_node`), not just display a value:
/// a scoped `OnEvent` only fires for a matching camera/region, an unscoped one fires for any.
/// This is the general mechanism for "events with inputs that filter the trigger" — extend this
/// list (and the matching gate in `on_event_node`) rather than inventing a new node kind for the
/// next event that needs scoping (see `docs/ref/workflows.md`).
pub(super) fn on_event_input_ports(config: &Value) -> Vec<ResolvedPort> {
    let event = config.get("event").and_then(|v| v.as_str()).unwrap_or("");
    if matches!(event, "object_entered" | "object_exited" | "line_crossed") {
        vec![
            ResolvedPort::new(PortName::new("camera".to_string()), types::CAMERA, false),
            ResolvedPort::new(PortName::new("region".to_string()), types::REGION, false),
        ]
    } else {
        Vec::new()
    }
}
