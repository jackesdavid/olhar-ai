//! Turning what a graph SAYS into what a run needs.
//!
//! A node's configuration names things — a camera by id, a variable by name, a region by label.
//! This is where a name becomes the thing itself, against this run's tenant and context.
//!
//! Also here: which exec arms a node fires. `arms_for` is called by both schedulers, which is why
//! it is a function rather than a branch inside one of them — the differential test fails the
//! moment the two would disagree, and an editor drawing a live edge the run never took is exactly
//! what that prevents.

use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::ToolContext;
use crate::workflows::domain::types;
use serde_json::Value;
use std::collections::HashMap;

use super::*;
use crate::device_actions as dev_actions;

// ---------------------------------------------------------------------------
// Camera entity helpers (used by the engine + nodes)
// ---------------------------------------------------------------------------

/// Resolve a query string (device id, IP, or name) to a `CameraEntity` using `sys.devices`.
/// Returns `None` if the query is empty or no device matches.
pub fn resolve_camera_entity(
    query: &str,
    sys: &ToolContext,
) -> Option<crate::workflows::domain::CameraEntity> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let devices = sys.devices.lock().unwrap();
    let idx = dev_actions::resolve_device(&devices, q).ok()?;
    let d = &devices[idx];
    Some(crate::workflows::domain::CameraEntity {
        id: d.id.clone(),
        ip: d.ip.clone(),
        name: d.name.clone(),
    })
}

/// Seed a run's variables map from the graph's declared defaults (card #5b), so a `Get Variable`
/// before any `Set` returns the author's default instead of a type-zero. Called once at run start.
/// Unparseable / typeless defaults are skipped (the `Get` then falls back to its type-zero).
pub fn seed_variable_defaults(vars: &[crate::workflows::domain::VariableDecl], sys: &ToolContext) {
    let mut seeded: Vec<(String, PortValue)> = Vec::new();
    for v in vars {
        let Some(raw) = v
            .default
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if let Some(pv) = parse_variable_value(v.ty.clone(), raw, sys) {
            seeded.push((v.name.clone(), pv));
        }
    }
    if seeded.is_empty() {
        return;
    }
    let mut map = sys.variables.lock().unwrap();
    for (name, pv) in seeded {
        map.insert(name, pv);
    }
}

/// Parse a string into a typed `PortValue` for a variable of type `ty`. The single place both a
/// declared **default** and a manual-run **input override** (#102) turn a string into a value, so
/// an input entered at launch parses identically to a stored default. `None` for an unparseable or
/// non-defaultable type (Image/File/arrays/Bbox/Person/Face) — the caller then falls back to the
/// `Get`'s type-zero.
pub fn parse_variable_value(
    ty: crate::workflows::domain::PortType,
    raw: &str,
    sys: &ToolContext,
) -> Option<PortValue> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if ty == types::TEXT {
        Some(PortValue::Text(raw.to_string()))
    } else if ty == types::RECIPIENT {
        Some(PortValue::Recipient(raw.to_string()))
    } else if ty == types::NUM {
        raw.parse::<i64>().ok().map(PortValue::Num)
    } else if ty == types::BOOL {
        Some(PortValue::Bool(matches!(raw, "true" | "1" | "yes" | "on")))
    } else if ty == types::CAMERA {
        resolve_camera_entity(raw, sys).map(PortValue::Camera)
    } else if ty == types::TABLE {
        // A Table default is a dictionary id; resolve it from the run-start snapshot.
        sys.dictionaries.get(raw).cloned().map(PortValue::Table)
    } else {
        // Everything else is produced upstream, not typed into a variable's default.
        None
    }
}

/// Which exec arm(s) a node fires.
///
/// Extracted from the scheduler so there is exactly one answer to "where does control go next".
/// It has to be callable from two places now — this product's scheduler and the engine's
/// `NodeRoute` — and two copies of a routing rule is how an editor starts drawing a live edge
/// the run never takes.
///
/// `outputs` matters for the two kinds that decide by asking a model: the choice is computed
/// during the run and left on an output port, so the arm cannot be worked out from the inputs
/// alone.
pub fn arms_for(
    kind: NodeKind,
    config: &Value,
    inputs: &PortValues,
    outputs: &PortValues,
) -> Vec<String> {
    let one = |s: &str| vec![s.to_string()];
    match kind {
        NodeKind::If => {
            let cond = inputs
                .get("condition")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            one(if cond { "true" } else { "false" })
        }
        NodeKind::AskAi => {
            let answer = outputs
                .get("result")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            one(if answer { "true" } else { "false" })
        }
        NodeKind::StringSwitch => {
            let val = inputs
                .get("value")
                .and_then(PortValue::as_text)
                .unwrap_or_default();
            let arms = crate::workflows::domain::string_switch_arms(config);
            if arms.iter().any(|a| a == val) {
                one(val)
            } else {
                one("default")
            }
        }
        NodeKind::AiSwitch => {
            // The model's choice is validated against the configured arms before it is trusted:
            // a model naming an arm that does not exist would otherwise fire nothing at all.
            let arm = outputs
                .get("arm")
                .and_then(PortValue::as_text)
                .unwrap_or("default");
            let arms = crate::workflows::domain::string_switch_arms(config);
            if arm != "default" && arms.iter().any(|a| a == arm) {
                one(arm)
            } else {
                one("default")
            }
        }
        NodeKind::CameraSwitch => {
            let Some(cam) = inputs.get("camera").and_then(PortValue::as_camera) else {
                return one("default");
            };
            let arms = crate::workflows::domain::camera_switch_arms(config);
            arms.iter()
                .position(|a| !a.is_empty() && (a == &cam.ip || a == &cam.id))
                .map(|i| vec![format!("arm_{}", i + 1)])
                .unwrap_or_else(|| one("default"))
        }
        // Everything else fires every arm it declares, which for almost every node is the single
        // "and then" pin.
        k => k
            .exec_out_ports()
            .iter()
            .map(|p| p.name.to_string())
            .collect(),
    }
}

/// The kinds that CHOOSE an arm rather than firing all of them.
pub fn is_routing(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::If
            | NodeKind::AskAi
            | NodeKind::StringSwitch
            | NodeKind::AiSwitch
            | NodeKind::CameraSwitch
    )
}

/// Resolve a `Table` config literal (the dropdown-stored dictionary id) to a `PortValue::Table`,
/// from the run-start dictionaries snapshot. Called by the engine when a `types::TABLE` input
/// port has no wired edge — the counterpart to [`resolve_camera_literal`].
pub fn resolve_table_literal(config: &Value, key: &str, sys: &ToolContext) -> Option<PortValue> {
    let id = config
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())?;
    sys.dictionaries
        .get(id.trim())
        .cloned()
        .map(PortValue::Table)
}

/// Resolve a `Camera` config literal (the dropdown-stored device id) to a `PortValue::Camera`.
/// Called by the engine when a `types::CAMERA` input port has no wired edge.
pub fn resolve_camera_literal(config: &Value, key: &str, sys: &ToolContext) -> Option<PortValue> {
    let id = config
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())?;
    resolve_camera_entity(id, sys).map(PortValue::Camera)
}

/// Upgrade a `PortValues` map that came from an event payload to its declared port types.
/// Event payloads arrive as generic scalars/lists (no type info); here we coerce them using
/// the `OnEvent` node's port schema:
///   - `Camera` ports: a `Text` (IP or id) → `PortValue::Camera(CameraEntity)`.
///   - `Bbox` ports: a 4-element `List` of nums → `PortValue::Bbox` (canonical xyxy).
/// Called by the engine when seeding an `OnEvent` node.
pub fn seed_event_payload(
    payload: &PortValues,
    ports: &[crate::workflows::domain::ResolvedPort],
    sys: &ToolContext,
) -> PortValues {
    use crate::workflows::domain::BBox;
    let mut out = payload.clone();
    for port in ports {
        // An if-chain: port types are open strings, so there is nothing to match exhaustively.
        // Anything not named here arrives already in the right shape and is left alone.
        let name = port.name.as_str();
        if port.ty == types::CAMERA {
            if let Some(PortValue::Text(s)) = out.get(name).cloned() {
                if let Some(cam) = resolve_camera_entity(&s, sys) {
                    out.insert(name.to_string(), PortValue::Camera(cam));
                }
            }
        } else if port.ty == types::BBOX {
            if let Some(PortValue::List(items)) = out.get(name).cloned() {
                let nums: Vec<i64> = items.iter().filter_map(PortValue::as_num).collect();
                if let Some(b) = BBox::from_xyxy_slice(&nums) {
                    out.insert(name.to_string(), PortValue::Bbox(b));
                }
            }
        } else if port.ty == types::IMAGE {
            // An Image-typed event field (e.g. `face_detected.frame`) arrives base64-encoded —
            // JSON has no byte type, so the generic payload decoder makes it a `Text`. Decode it
            // here or every node that requires an image rejects it ("missing required input
            // 'image'"), however the graph is wired. Invalid base64 leaves the port unfilled
            // rather than handing downstream nodes garbage bytes.
            if let Some(PortValue::Text(s)) = out.get(name).cloned() {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(s.trim()) {
                    if !bytes.is_empty() {
                        out.insert(name.to_string(), PortValue::Image(bytes));
                    }
                }
            }
        } else if port.ty == types::LANDMARKS {
            // `kps` arrives as a List of `[x,y]` pairs (each pair itself a 2-element List of
            // nums, or a flat `[x0,y0,x1,y1,…]`) → Landmarks in source pixels (#122). A
            // malformed or empty payload just leaves the port unfilled and the embed node falls
            // back.
            if let Some(PortValue::List(items)) = out.get(name).cloned() {
                let pts = landmark_points(&items);
                if !pts.is_empty() {
                    out.insert(name.to_string(), PortValue::Landmarks(pts));
                }
            }
        }
    }
    out
}

/// Parse a `kps` payload `List` into `[x,y]` landmark points (source pixels, #122). Accepts either
/// a FLAT list of numbers `[x0,y0,x1,y1,…]` — the edge wire format, which survives the generic event
/// payload decoder that flattens nested arrays — or a nested list of 2-element `[x,y]` pairs. Short
/// or malformed input yields only the complete pairs (empty → the embed node falls back).
fn landmark_points(items: &[PortValue]) -> Vec<[f32; 2]> {
    // Nested form: each element is itself a 2-element List.
    if items.iter().any(|it| matches!(it, PortValue::List(_))) {
        return items
            .iter()
            .filter_map(|it| {
                let p = it.as_list()?;
                Some([p.first()?.as_num()? as f32, p.get(1)?.as_num()? as f32])
            })
            .collect();
    }
    // Flat form: chunk the numbers into (x, y) pairs.
    let nums: Vec<i64> = items.iter().filter_map(PortValue::as_num).collect();
    nums.chunks_exact(2)
        .map(|c| [c[0] as f32, c[1] as f32])
        .collect()
}

/// Get the full `Device` from `sys.devices` using a `CameraEntity`'s stable id.
pub(crate) fn device_from_camera<'a>(
    cam: &crate::workflows::domain::CameraEntity,
    devices: &'a [crate::devices::Device],
) -> Result<&'a crate::devices::Device, String> {
    devices
        .iter()
        .find(|d| d.id == cam.id)
        .ok_or_else(|| format!("camera '{}' not found (was it removed?)", cam.name))
}
