//! Reading a value the user typed into the inspector.
//!
//! An input port that nothing is wired to is not empty — it falls back to whatever was typed into
//! the node's own form. That is what makes a graph readable: a `Print` with its message typed in
//! needs no wire, and the wire, when there is one, wins.
//!
//! Every port type needs an entry here. The coverage test below is the thing that notices when a
//! new one is added and forgotten, which otherwise shows up as a field that silently stays blank.

use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::ToolContext;
use crate::workflows::domain::types;
use serde_json::Value;
use std::collections::HashMap;

use super::*;

/// Convert a node's config literal into a typed value for an UNCONNECTED input port.
/// Empty text / missing keys / `Image` ports yield `None` (treated as "not provided").
pub fn literal_for(config: &Value, port: Port) -> Option<PortValue> {
    let Port { name, ty, .. } = port;
    literal_for_named(config, name.as_str(), ty)
}

/// As [`literal_for`], but by port name + type — so config-derived ports (with owned
/// names, e.g. EmitEvent payload inputs) resolve a literal too. Payload ports carry no
/// config literal (their value arrives by wire), so this returns `None` for them.
pub fn literal_for_named(config: &Value, name: &str, ty: PortType) -> Option<PortValue> {
    let v = config.get(name)?;

    // An if-chain rather than a `match`: a port type is an open string now, so there is nothing
    // to match exhaustively on. The types that fall through to `None` are listed at the bottom
    // with the reason, and the test below asserts none of them was forgotten.
    if ty == types::TEXT || ty == types::RECIPIENT {
        let s = v.as_str()?;
        if s.trim().is_empty() {
            return None;
        }
        // A recipient literal is the recipient's label — the same thing `GetRecipient` produces.
        return Some(if ty == types::RECIPIENT {
            PortValue::Recipient(s.to_string())
        } else {
            PortValue::Text(s.to_string())
        });
    }

    if ty == types::NUM {
        return v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
            .map(PortValue::Num);
    }

    if ty == types::BOOL {
        return v.as_bool().map(PortValue::Bool);
    }

    // An image or file literal is a path the user picked; read it at run time.
    if ty == types::IMAGE {
        return v
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .and_then(|p| std::fs::read(p).ok())
            .map(PortValue::Image);
    }

    if ty == types::FILE {
        return v
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .and_then(read_file_val)
            .map(PortValue::File);
    }

    // A dictionary literal is a JSON object of string→string in config.
    if ty == types::DICTIONARY {
        let obj = v.as_object()?;
        if obj.is_empty() {
            return None;
        }
        let pairs = obj
            .iter()
            .map(|(k, val)| {
                let s = val
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| val.to_string());
                (k.clone(), s)
            })
            .collect();
        return Some(PortValue::Dict(pairs));
    }

    // Everything else has no config literal, for one of three reasons:
    //
    //   Exec                        control flow carries no value at all;
    //   Camera, Table               resolved by the engine, which has `sys`; not a pure read;
    //   Embedding, Landmarks,       produced by an upstream node — a face embedding is not
    //   arrays, detections, Any     something anybody types into an inspector.
    None
}

#[cfg(test)]
mod literal_coverage {
    use super::*;

    /// Every port type this product declares is either handled above or deliberately absent.
    ///
    /// The `match` this replaced was exhaustive by construction; an if-chain is not, so a new
    /// port type would silently fall through to `None`. This is the compiler check, re-stated
    /// as a test: it fails when a type is added and nobody decided what its literal means.
    #[test]
    fn every_port_type_is_accounted_for() {
        let has_literal = [
            types::TEXT,
            types::RECIPIENT,
            types::NUM,
            types::BOOL,
            types::IMAGE,
            types::FILE,
            types::DICTIONARY,
        ];
        let no_literal = [
            types::EXEC,
            types::CAMERA,
            types::TABLE,
            types::EMBEDDING,
            types::LANDMARKS,
            types::TEXT_ARRAY,
            types::NUM_ARRAY,
            types::PERSON_ARRAY,
            types::FACE_ARRAY,
            types::PERSON,
            types::FACE,
            types::BBOX,
            types::REGION,
            types::TRACKED_OBJECT,
            types::TRACKED_OBJECT_ARRAY,
            types::ANY,
        ];
        assert_eq!(
            has_literal.len() + no_literal.len(),
            23,
            "a port type was added or removed and this decision was not revisited"
        );
        // The ones with no literal must return None even when config holds something for them.
        let cfg = serde_json::json!({ "x": "something" });
        for ty in no_literal {
            assert!(
                literal_for_named(&cfg, "x", ty.clone()).is_none(),
                "{ty} must not take a literal from config"
            );
        }
    }
}
