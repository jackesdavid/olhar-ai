//! Between this product's `PortValue` and the engine's `Value`.
//!
//! The engine understands text, numbers, booleans, bytes, lists and maps. Everything else this
//! product puts on a wire — a camera, a face, an embedding, a bounding box — it carries as an
//! [`ExternValue`](ggraph_core::ExternValue) without looking inside.
//!
//! **One wrapper for all ten domain types, not ten wrappers.** They are already one enum here,
//! and the only thing the engine asks of an extern value is its type name, which that enum can
//! answer. Ten structs would restate the same match ten times and give ten chances to name a
//! type differently from the port it travels on.
//!
//! The type name is the port type string, always, and there is a test for it — a persisted value
//! whose tag disagrees with its port is one the codec cannot round-trip and the editor colours
//! wrongly.

use super::node::PortValue;
use ggraph_core::{Bytes, ExternValue, PortName, PortValues as EngineValues, Value};
use std::any::Any;
use std::collections::HashMap;

/// This product's own value, riding on an engine wire.
#[derive(Debug, Clone)]
pub struct Domain(pub PortValue);

impl ExternValue for Domain {
    fn type_name(&self) -> &'static str {
        // The port type string, so a value can never disagree with the wire it is on.
        match &self.0 {
            PortValue::Recipient(_) => "recipient",
            PortValue::Table(_) => "table",
            PortValue::Embedding(_) => "embedding",
            PortValue::Landmarks(_) => "landmarks",
            PortValue::Person(_) => "person",
            PortValue::Face(_) => "face",
            PortValue::Camera(_) => "camera",
            PortValue::Bbox(_) => "bbox",
            PortValue::Region(_) => "region",
            PortValue::TrackedObject(_) => "tracked_object",
            // The scalar variants never reach here — `to_engine` handles them natively — but a
            // name is still owed rather than a panic.
            PortValue::Text(_) => "text",
            PortValue::Num(_) => "num",
            PortValue::Bool(_) => "bool",
            PortValue::Image(_) => "image",
            PortValue::File(_) => "file",
            PortValue::Dict(_) => "dictionary",
            PortValue::List(_) => "list",
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn summary(&self) -> String {
        format!("{} ({})", self.type_name(), short(&self.0))
    }

    fn as_text(&self) -> Option<String> {
        match &self.0 {
            PortValue::Recipient(s) => Some(s.clone()),
            PortValue::Camera(c) => Some(c.name.clone()),
            _ => None,
        }
    }
}

fn short(v: &PortValue) -> String {
    match v {
        PortValue::Recipient(s) => s.clone(),
        PortValue::Camera(c) => c.name.clone(),
        PortValue::Embedding(e) => format!("{}-d", e.len()),
        PortValue::Landmarks(p) => format!("{} point(s)", p.len()),
        _ => String::new(),
    }
}

/// A `PortValue` as the engine sees it.
pub fn to_engine(v: &PortValue) -> Value {
    match v {
        PortValue::Text(s) => Value::Text(s.clone()),
        PortValue::Num(n) => Value::int(*n),
        PortValue::Bool(b) => Value::Bool(*b),
        // The mime is what makes an image port distinguishable from an arbitrary download on the
        // engine's side, so it is not guessed: these really are JPEGs everywhere they are made.
        PortValue::Image(b) => Value::Bytes(Bytes::new("image/jpeg", b.clone())),
        PortValue::File(f) => Value::Bytes(Bytes::new(&f.mime, f.bytes.clone()).named(&f.name)),
        PortValue::List(items) => Value::List(items.iter().map(to_engine).collect()),
        PortValue::Dict(pairs) => Value::Map(
            pairs
                .iter()
                .map(|(k, v)| (k.clone(), Value::Text(v.clone())))
                .collect(),
        ),
        other => Value::ext(Domain(other.clone())),
    }
}

/// Back again. `None` for a shape this product has no variant for — a float, a nested map —
/// which is honest: inventing one would put a value on a wire that nothing downstream expects.
pub fn from_engine(v: &Value) -> Option<PortValue> {
    Some(match v {
        Value::Text(s) => PortValue::Text(s.clone()),
        Value::Num(n) => PortValue::Num(n.as_i64()?),
        Value::Bool(b) => PortValue::Bool(*b),
        Value::Bytes(b) if b.mime.starts_with("image/") => PortValue::Image(b.data.to_vec()),
        Value::Bytes(b) => PortValue::File(super::node::FileVal {
            name: b.name.clone().map(|n| n.to_string()).unwrap_or_default(),
            mime: b.mime.to_string(),
            bytes: b.data.to_vec(),
        }),
        Value::List(items) => PortValue::List(items.iter().filter_map(from_engine).collect()),
        Value::Map(pairs) => PortValue::Dict(
            pairs
                .iter()
                .filter_map(|(k, v)| v.as_text().map(|s| (k.clone(), s)))
                .collect(),
        ),
        Value::Extern(e) => e.as_any().downcast_ref::<Domain>()?.0.clone(),
        Value::Json(_) => return None,
    })
}

/// A whole port map, into the engine's shape.
pub fn to_engine_values(v: &HashMap<String, PortValue>) -> EngineValues {
    v.iter()
        .map(|(k, v)| (PortName::new(k), to_engine(v)))
        .collect()
}

/// And back. A value with no counterpart here is dropped rather than faked — a dropped port
/// reads downstream as "not provided", which every node already knows how to report.
pub fn from_engine_values(v: &EngineValues) -> HashMap<String, PortValue> {
    v.iter()
        .filter_map(|(k, v)| from_engine(v).map(|v| (k.as_str().to_string(), v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::domain::{types, BBox};
    use super::*;

    fn round_trip(v: PortValue) -> Option<PortValue> {
        from_engine(&to_engine(&v))
    }

    #[test]
    fn scalars_cross_natively_rather_than_as_extern() {
        assert!(matches!(
            to_engine(&PortValue::Text("a".into())),
            Value::Text(_)
        ));
        assert!(matches!(to_engine(&PortValue::Num(3)), Value::Num(_)));
        assert!(matches!(to_engine(&PortValue::Bool(true)), Value::Bool(_)));
        assert!(matches!(
            round_trip(PortValue::Num(-7)),
            Some(PortValue::Num(-7))
        ));
    }

    #[test]
    fn a_domain_value_survives_the_trip_intact() {
        let bbox = PortValue::Bbox(BBox::from_xyxy(1, 2, 3, 4));
        let back = round_trip(bbox.clone()).expect("round trips");
        assert_eq!(format!("{back:?}"), format!("{bbox:?}"));
    }

    #[test]
    fn an_image_keeps_being_an_image_and_not_a_file() {
        let img = PortValue::Image(vec![1, 2, 3]);
        assert_eq!(
            to_engine(&img).port_type().as_str(),
            "image",
            "an image port must not accept an arbitrary download, and the mime is what the \
             engine tells them apart by"
        );
        assert!(matches!(round_trip(img), Some(PortValue::Image(_))));
    }

    #[test]
    fn every_domain_value_names_the_port_it_travels_on() {
        // The tag and the port type are the same string, always. A value whose tag disagrees is
        // one the codec cannot round-trip and the editor colours wrongly.
        for (v, expected) in [
            (PortValue::Recipient("x".into()), types::RECIPIENT),
            (PortValue::Bbox(BBox::from_xyxy(0, 0, 1, 1)), types::BBOX),
            (PortValue::Embedding(vec![0.0; 4]), types::EMBEDDING),
            (PortValue::Landmarks(vec![[0.0, 0.0]]), types::LANDMARKS),
        ] {
            assert_eq!(
                to_engine(&v).port_type().as_str(),
                expected.as_str(),
                "{v:?} rides a {expected} wire"
            );
        }
    }

    #[test]
    fn a_shape_with_no_counterpart_here_is_dropped_rather_than_invented() {
        assert!(
            from_engine(&Value::float(1.5)).is_none(),
            "this product has no float; rounding it would put a wrong number downstream"
        );
        assert!(from_engine(&Value::Json(serde_json::json!({}))).is_none());
    }
}
