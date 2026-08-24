//! The node catalog — static per-kind metadata (slug, label, category, typed ports, default
//! config) that the editor palette is built from.
//!
//! This lives here rather than in `sentinel-control` for one reason: it is the ONLY contract
//! between the Rust node set and the Svelte editor, and nothing verified it. `catalog_json()`
//! being in a lib crate is what lets `catalog_snapshot.rs` compare it, byte for byte, against a
//! committed fixture — so a renamed slug, a moved port or a changed default shows up as a diff
//! in a merge request instead of as a broken palette in front of a customer.
//!
//! The contract has already drifted once without anyone noticing: `sentinel-app`'s
//! `src/lib/workflows/portColors.ts` hand-mirrors `PortType` and lists 22 of the 23 variants
//! (`landmarks` is missing).
//!
//! The projection itself is unchanged from the handler it replaced — same keys, same order,
//! same `is_pure()` rule for omitting the exec pins.

use super::domain::{NodeKind, Port, EXEC_IN};
use serde_json::{json, Value};

fn ports(list: &[Port]) -> Vec<Value> {
    list.iter()
        .map(|p| {
            json!({
                "name": p.name,
                "type": serde_json::to_value(p.ty).unwrap_or(Value::Null),
                "required": p.required,
            })
        })
        .collect()
}

fn with_exec(exec: &[Port], list: &[Port]) -> Vec<Value> {
    let mut v = ports(exec);
    v.extend(ports(list));
    v
}

/// One entry per user-visible node kind (`NodeKind::ALL`), in declaration order.
pub fn catalog_json() -> Value {
    let exec_in = [EXEC_IN];
    let kinds = NodeKind::ALL
        .iter()
        .map(|k| {
            // Pure data/logic nodes have no exec pins — omit exec_in/exec_out entirely so the UI
            // can hide the exec circles and the engine knows to pull them.
            let (inputs, outputs) = if k.is_pure() {
                (ports(k.input_ports()), ports(k.output_ports()))
            } else {
                (
                    with_exec(&exec_in, k.input_ports()),
                    with_exec(k.exec_out_ports(), k.output_ports()),
                )
            };
            json!({
                "slug": k.slug(),
                "label": k.label(),
                "category": k.category(),
                "inputs": inputs,
                "outputs": outputs,
                "default_config": k.default_config(),
            })
        })
        .collect::<Vec<_>>();
    json!({ "kinds": kinds })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed shape of the catalog. Regenerate deliberately, never casually:
    ///
    ///     UPDATE_CATALOG_SNAPSHOT=1 cargo test -p app-core --lib workflows::catalog
    ///
    /// A diff here means the editor's palette changed. That is sometimes right (a new node) and
    /// sometimes a customer's saved graph failing to load (a renamed slug, a moved port). The
    /// point is that it is never silent.
    const SNAPSHOT: &str = include_str!("catalog_snapshot.json");

    fn rendered() -> String {
        let mut s = serde_json::to_string_pretty(&catalog_json()).expect("catalog serializes");
        s.push('\n');
        s
    }

    #[test]
    fn catalog_matches_the_committed_snapshot() {
        let now = rendered();
        if std::env::var("UPDATE_CATALOG_SNAPSHOT").is_ok() {
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/workflows/catalog_snapshot.json"
            );
            std::fs::write(path, &now).expect("snapshot is writable");
            return;
        }
        if now != SNAPSHOT {
            let a: Vec<&str> = SNAPSHOT.lines().collect();
            let b: Vec<&str> = now.lines().collect();
            let first = (0..a.len().max(b.len()))
                .find(|i| a.get(*i) != b.get(*i))
                .unwrap_or(0);
            panic!(
                "node catalog changed at line {}:\n  committed: {:?}\n  now:       {:?}\n\
                 \nIf the change is intended, regenerate with \
                 UPDATE_CATALOG_SNAPSHOT=1 cargo test -p app-core --lib workflows::catalog",
                first + 1,
                a.get(first),
                b.get(first),
            );
        }
    }

    /// `ALL` is the palette, not the node set: `Reroute` is a real kind that is deliberately
    /// excluded (it is collapsed before execution). Catch a kind that falls out of the palette
    /// by accident — the catalog is how the editor learns a node exists.
    #[test]
    fn every_palette_kind_round_trips_through_its_slug() {
        for k in NodeKind::ALL {
            let slug = k.slug();
            assert_eq!(
                NodeKind::from_slug(slug),
                Some(k),
                "slug {slug:?} does not resolve back to its kind"
            );
        }
    }

    /// The slug and the serde representation must agree. They are two independent decode paths
    /// into the same enum — `Graph` deserialization uses serde, `from_slug` is used by the API —
    /// and a kind whose `slug()` disagrees with its serde name loads through one and not the
    /// other.
    #[test]
    fn slug_and_serde_name_agree() {
        for k in NodeKind::ALL {
            let via_serde = serde_json::to_value(k).expect("kind serializes");
            assert_eq!(
                via_serde.as_str(),
                Some(k.slug()),
                "kind {:?}: slug() and serde disagree",
                k
            );
        }
    }
}
