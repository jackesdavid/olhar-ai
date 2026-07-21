//! An instance's value context: what each node last produced, so a later run of the same instance
//! can read it.
//!
//! A `DelayTrigger`'s `stop` runs from a timer, in a different run from the `start`. Without this,
//! `StopRecording.camera` (from `OnEvent`) and `clip_id` (from `StartRecording`) resolve to nothing
//! on the trailing edge and the engine skips the branch as dead — silently, run reporting `ok`. The
//! graph is right; the engine had thrown the values away.
//!
//! `PortValue` has no serde derive and shouldn't get one: it carries decoded images and detector
//! structs that have no business in a database. This is a deliberately narrow, tagged codec for the
//! kinds worth persisting. Anything else is dropped **with a warning** rather than silently
//! half-stored — a value that quietly vanishes is a dead branch again, which is the bug this file
//! exists to end.
//!
//! Media (`Image`, `File`) is NOT persisted here yet. It belongs in MinIO with a URL in the row —
//! `media.rs` already does exactly that for notifications — and putting base64 in Postgres is the
//! wart #94 exists to remove, not one to add. Until then a stop branch that needs an image gets a
//! logged miss, not a lie.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::domain::{BBox, CameraEntity};
use super::node::PortValue;
use super::PortValues;

/// What a node produced, by port.
pub type NodeValues = HashMap<u32, PortValues>;

pub fn value_to_json(v: &PortValue) -> Option<Value> {
    Some(match v {
        PortValue::Text(s) => json!({ "t": "text", "v": s }),
        PortValue::Num(n) => json!({ "t": "num", "v": n }),
        PortValue::Bool(b) => json!({ "t": "bool", "v": b }),
        PortValue::Recipient(r) => json!({ "t": "recipient", "v": r }),
        PortValue::Camera(c) => json!({ "t": "camera", "id": c.id, "ip": c.ip, "name": c.name }),
        PortValue::Bbox(b) => json!({ "t": "bbox", "v": [b.x1, b.y1, b.x2, b.y2] }),
        PortValue::List(items) => {
            let vs: Vec<Value> = items.iter().filter_map(value_to_json).collect();
            if vs.len() != items.len() {
                return None; // a partial list is a wrong answer, not a smaller one
            }
            json!({ "t": "list", "v": vs })
        }
        _ => return None,
    })
}

pub fn value_from_json(v: &Value) -> Option<PortValue> {
    let t = v.get("t")?.as_str()?;
    let field = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(match t {
        "text" => PortValue::Text(v.get("v")?.as_str()?.to_string()),
        "num" => PortValue::Num(v.get("v")?.as_i64()?),
        "bool" => PortValue::Bool(v.get("v")?.as_bool()?),
        "recipient" => PortValue::Recipient(v.get("v")?.as_str()?.to_string()),
        "camera" => PortValue::Camera(CameraEntity {
            id: field("id"),
            ip: field("ip"),
            name: field("name"),
        }),
        "bbox" => {
            let a = v.get("v")?.as_array()?;
            let nums: Vec<i64> = a.iter().filter_map(|x| x.as_i64()).collect();
            PortValue::Bbox(BBox::from_xyxy_slice(&nums)?)
        }
        "list" => PortValue::List(
            v.get("v")?
                .as_array()?
                .iter()
                .filter_map(value_from_json)
                .collect(),
        ),
        _ => return None,
    })
}

/// `{ "<node>": { "<port>": <tagged value> } }` — one instance's whole value context.
pub fn to_json(vals: &NodeValues) -> Value {
    let mut out = serde_json::Map::new();
    for (nid, pv) in vals {
        let mut ports = serde_json::Map::new();
        for (name, v) in pv {
            match value_to_json(v) {
                Some(j) => {
                    ports.insert(name.clone(), j);
                }
                None => log::warn!(
                    "node values: node {nid} port '{name}' is a kind we don't persist (media \
                     belongs in MinIO — see #94); a later run of this instance will see it missing"
                ),
            }
        }
        if !ports.is_empty() {
            out.insert(nid.to_string(), Value::Object(ports));
        }
    }
    Value::Object(out)
}

pub fn from_json(v: &Value) -> NodeValues {
    let mut vals = NodeValues::new();
    let Some(obj) = v.as_object() else {
        return vals;
    };
    for (nid, ports) in obj {
        let Ok(nid) = nid.parse::<u32>() else { continue };
        let Some(ports) = ports.as_object() else {
            continue;
        };
        let mut pv = PortValues::new();
        for (name, jv) in ports {
            if let Some(val) = value_from_json(jv) {
                pv.insert(name.clone(), val);
            }
        }
        if !pv.is_empty() {
            vals.insert(nid, pv);
        }
    }
    vals
}

/// A short human display string for a persisted value — what the live instance view shows per port.
/// Only the kinds the codec persists ever reach here from a stored row; the media/detector variants
/// are handled anyway (a label, never bytes) so it stays total and never leaks base64.
pub fn display_value(v: &PortValue) -> String {
    match v {
        PortValue::Text(s) => s.clone(),
        PortValue::Num(n) => n.to_string(),
        PortValue::Bool(b) => b.to_string(),
        PortValue::Recipient(r) => r.clone(),
        PortValue::Camera(c) => match (c.name.is_empty(), c.ip.is_empty()) {
            (false, false) => format!("{} ({})", c.name, c.ip),
            (false, true) => c.name.clone(),
            (true, false) => c.ip.clone(),
            (true, true) => c.id.clone(),
        },
        PortValue::Bbox(b) => format!("[{}, {}, {}, {}]", b.x1, b.y1, b.x2, b.y2),
        PortValue::List(items) => {
            let inner: Vec<String> = items.iter().take(8).map(display_value).collect();
            let more = if items.len() > inner.len() { ", …" } else { "" };
            format!("[{}{}]", inner.join(", "), more)
        }
        // Kinds the codec never persists — a label, not bytes, so a stray value can't leak base64.
        PortValue::Image(b) => format!("image ({} KB)", b.len() / 1024),
        PortValue::File(f) => {
            if f.name.is_empty() {
                format!("file ({} KB)", f.bytes.len() / 1024)
            } else {
                f.name.clone()
            }
        }
        PortValue::Dict(m) => format!("{{{} fields}}", m.len()),
        PortValue::Table(t) => format!("table {}", t.id),
        PortValue::Embedding(v) => format!("embedding ({}-d)", v.len()),
        PortValue::Landmarks(v) => format!("landmarks ({} pts)", v.len()),
        PortValue::Person(_) => "person".to_string(),
        PortValue::Face(_) => "face".to_string(),
        PortValue::Region(_) => "region".to_string(),
        PortValue::TrackedObject(_) => "tracked object".to_string(),
    }
}

/// A variables map (`name → value`) as tagged JSON — the DelayTrigger window's snapshot of
/// `sys.variables`, so the trailing-edge stop run can restore what a `Set` wrote on the arming run.
/// Same narrow codec as node values: a kind we don't persist (media, detector structs) is dropped
/// **with a warning** rather than silently, because a variable that quietly vanishes is a dead
/// branch again — the bug this exists to end.
pub fn variables_to_json(vars: &HashMap<String, PortValue>) -> Value {
    let mut out = serde_json::Map::new();
    for (name, v) in vars {
        match value_to_json(v) {
            Some(j) => {
                out.insert(name.clone(), j);
            }
            None => log::warn!(
                "workflow variable '{name}' is a kind we don't persist (media belongs in MinIO — \
                 see #94); the trailing-edge stop run will see it missing"
            ),
        }
    }
    Value::Object(out)
}

pub fn variables_from_json(v: &Value) -> HashMap<String, PortValue> {
    let mut out = HashMap::new();
    let Some(obj) = v.as_object() else {
        return out;
    };
    for (name, jv) in obj {
        if let Some(val) = value_from_json(jv) {
            out.insert(name.clone(), val);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(nid: u32, port: &str, v: PortValue) -> NodeValues {
        let mut pv = PortValues::new();
        pv.insert(port.to_string(), v);
        let mut n = NodeValues::new();
        n.insert(nid, pv);
        n
    }

    #[test]
    fn the_camera_survives_postgres() {
        // OnEvent.camera → StopRecording.camera is the whole reason this exists. It crosses a
        // database and possibly a pod restart between the start and the stop.
        let vals = one(18, "camera", PortValue::Camera(CameraEntity {
            id: "cam-1".into(),
            ip: "10.0.0.9".into(),
            name: "Hall".into(),
        }));
        let back = from_json(&to_json(&vals));
        match back[&18].get("camera") {
            Some(PortValue::Camera(c)) => {
                assert_eq!((c.id.as_str(), c.ip.as_str(), c.name.as_str()), ("cam-1", "10.0.0.9", "Hall"));
            }
            other => panic!("camera didn't survive: {other:?}"),
        }
    }

    #[test]
    fn the_clip_id_survives_postgres() {
        // StartRecording.clip_id → StopRecording.clip_id, written by the arming run and read by a
        // stop that happens in a different run entirely.
        let back = from_json(&to_json(&one(3, "clip_id", PortValue::Text("clip-abc".into()))));
        assert_eq!(back[&3].get("clip_id").and_then(PortValue::as_text), Some("clip-abc"));
    }

    #[test]
    fn scalars_and_bbox_round_trip() {
        for (port, v) in [
            ("n", PortValue::Num(42)),
            ("b", PortValue::Bool(true)),
            ("r", PortValue::Recipient("ops".into())),
            ("box", PortValue::Bbox(BBox::from_xyxy(1, 2, 3, 4))),
        ] {
            let back = from_json(&to_json(&one(1, port, v.clone())));
            let got = back[&1].get(port).expect("port survived");
            assert_eq!(format!("{got:?}"), format!("{v:?}"), "{port} round-trip");
        }
    }

    #[test]
    fn media_is_dropped_loudly_not_mangled() {
        // Image/File belong in MinIO with a URL in the row (#94). Until then they must vanish with
        // a warning and a MISSING port — never come back as an empty value that satisfies a
        // required input, which would turn a visible miss into a wrong answer.
        assert!(value_to_json(&PortValue::Image(vec![1, 2, 3])).is_none());
        let vals = one(9, "image", PortValue::Image(vec![0xFF; 64]));
        assert!(to_json(&vals).as_object().unwrap().is_empty(), "no row for an image-only node");
    }

    #[test]
    fn a_partial_list_is_refused() {
        // Half a list is a wrong answer, not a smaller one.
        let list = PortValue::List(vec![PortValue::Text("a".into()), PortValue::Image(vec![1])]);
        assert!(value_to_json(&list).is_none());
    }

    #[test]
    fn a_list_of_scalars_round_trips() {
        let list = PortValue::List(vec![PortValue::Text("a".into()), PortValue::Num(2)]);
        let back = from_json(&to_json(&one(1, "items", list)));
        match back[&1].get("items") {
            Some(PortValue::List(items)) => assert_eq!(items.len(), 2),
            other => panic!("list didn't survive: {other:?}"),
        }
    }

    #[test]
    fn garbage_from_the_database_is_ignored_not_trusted() {
        assert!(value_from_json(&json!({ "t": "wat", "v": 1 })).is_none());
        assert!(value_from_json(&json!("bare string")).is_none());
        assert!(from_json(&json!({ "not-a-node": {} })).is_empty());
        assert!(from_json(&json!([])).is_empty());
    }

    #[test]
    fn variables_snapshot_round_trips_by_name() {
        // The trailing-edge case that #114 fixes: `Set current_camera` (a Camera var) and a plain
        // text var written on the arming run must come back keyed by name on the stop run.
        let mut vars: HashMap<String, PortValue> = HashMap::new();
        vars.insert(
            "current_camera".into(),
            PortValue::Camera(CameraEntity { id: "cam-1".into(), ip: "10.0.0.9".into(), name: "Hall".into() }),
        );
        vars.insert("current_clip_id".into(), PortValue::Text("clip-abc".into()));

        let back = variables_from_json(&variables_to_json(&vars));
        match back.get("current_camera") {
            Some(PortValue::Camera(c)) => assert_eq!(c.id, "cam-1"),
            other => panic!("camera var didn't survive: {other:?}"),
        }
        assert_eq!(back.get("current_clip_id").and_then(PortValue::as_text), Some("clip-abc"));
    }

    #[test]
    fn a_media_variable_is_dropped_not_faked() {
        // An Image variable can't round-trip yet (#94) — it must vanish with a warning, never come
        // back as an empty value that would satisfy a required input on the stop branch.
        let mut vars: HashMap<String, PortValue> = HashMap::new();
        vars.insert("frame".into(), PortValue::Image(vec![0xFF; 8]));
        vars.insert("kept".into(), PortValue::Text("ok".into()));
        let back = variables_from_json(&variables_to_json(&vars));
        assert!(back.get("frame").is_none(), "media var is dropped, not faked");
        assert_eq!(back.get("kept").and_then(PortValue::as_text), Some("ok"));
    }
}
