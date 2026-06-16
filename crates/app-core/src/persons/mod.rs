//! The Person module — people the system manages, a generic superior→subordinate
//! hierarchy (parent→child, boss→employee), and per-superior permissions.
//!
//! Mirrors the devices module: a `Person` domain type plus a shared in-memory
//! `PersonStore`. Persistence lives in [`crate::db`] (Postgres, the `persons`
//! table). The facial `embedding` column is owned by enrollment (Phase 2) and is
//! deliberately NOT carried on this struct, so saving a person's metadata never
//! clobbers a stored embedding.
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub mod repo;

/// The shared person list — held by the Tauri managed state, hydrated from the DB.
pub type PersonStore = Arc<Mutex<Vec<Person>>>;

/// Permission actions a superior may perform over its subtree. Kept as plain
/// strings (a `text[]` column) so the set can grow without a migration.
pub const PERMISSIONS: [&str; 4] =
    ["view_photo", "view_location", "view_report", "receive_notifications"];

/// A managed person. The hierarchy is expressed by `parent_id` (its superior);
/// `permissions` are the actions this person may perform over the people beneath it.
/// Face biometrics live in their own table ([`PersonEmbedding`]), not here — a person
/// is "enrolled" when they have ≥1 approved embedding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Person {
    pub id: String,
    pub name: String,
    /// The superior in the hierarchy (guardian/parent/boss). `None` = top-level.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// This person's own notification target (a recipient label). `None` = none.
    #[serde(default)]
    pub recipient_label: Option<String>,
    /// Actions this person (as a superior) may perform over its subtree.
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl Person {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Person {
            id: id.into(),
            name: name.into(),
            parent_id: None,
            recipient_label: None,
            permissions: Vec::new(),
        }
    }
}

/// A recognition candidate: a person matched against the enrolled embeddings, with
/// the pgvector **cosine distance** (0 = identical, 2 = opposite). Lower is better.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct RecognitionMatch {
    pub person_id: String,
    pub name: String,
    pub distance: f32,
}

/// One enrolled face embedding for a person. Each row has its own id and approval
/// (per-sample consent); a person can have many. The raw `embedding` is returned so
/// the UI can render a privacy-safe point-cloud avatar (we never store the photo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct PersonEmbedding {
    pub id: String,
    pub person_id: String,
    pub approved: bool,
    pub source: String,
    pub created_at: String,
    /// The 512-d vector (read as `float4[]`), used only for the avatar visualisation.
    pub embedding: Vec<f32>,
}

/// The chosen recognition for the UI / nodes: a matched person + confidence (0..100).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecognitionResult {
    pub person_id: String,
    pub name: String,
    pub confidence: i64,
}

/// Pick the best match within `max_distance`, or `None` if nothing qualifies.
/// Confidence rewards both closeness *and* multiple supporting samples for the same
/// person, so more enrolled faces → higher confidence.
pub fn best_match(matches: &[RecognitionMatch], max_distance: f32) -> Option<RecognitionResult> {
    let best = matches.iter().min_by(|a, b| a.distance.total_cmp(&b.distance))?;
    if best.distance > max_distance {
        return None;
    }
    let supporting = matches
        .iter()
        .filter(|m| m.person_id == best.person_id && m.distance <= max_distance)
        .count();
    let base = (1.0 - best.distance.max(0.0)) as f64; // cosine similarity ≈ 0..1
    let bonus = supporting.saturating_sub(1) as f64 * 0.03; // +3% per extra sample
    let confidence = ((base + bonus).clamp(0.0, 1.0) * 100.0).round() as i64;
    Some(RecognitionResult { person_id: best.person_id.clone(), name: best.name.clone(), confidence })
}

/// Find a person by id.
pub fn find<'a>(people: &'a [Person], id: &str) -> Option<&'a Person> {
    people.iter().find(|p| p.id == id)
}

/// The chain of superiors above `id`, nearest first (parent, grandparent, …).
/// Cycle-safe: stops if it revisits a person.
pub fn ancestors<'a>(people: &'a [Person], id: &str) -> Vec<&'a Person> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cur = find(people, id).and_then(|p| p.parent_id.as_deref());
    while let Some(pid) = cur {
        if !seen.insert(pid.to_string()) {
            break; // cycle guard
        }
        match find(people, pid) {
            Some(p) => {
                out.push(p);
                cur = p.parent_id.as_deref();
            }
            None => break,
        }
    }
    out
}

/// Every person beneath `id` in the hierarchy (its descendants, any depth).
pub fn subtree<'a>(people: &'a [Person], id: &str) -> Vec<&'a Person> {
    let mut out = Vec::new();
    let mut stack = vec![id.to_string()];
    let mut seen = std::collections::HashSet::new();
    while let Some(cur) = stack.pop() {
        for p in people.iter().filter(|p| p.parent_id.as_deref() == Some(cur.as_str())) {
            if seen.insert(p.id.clone()) {
                out.push(p);
                stack.push(p.id.clone());
            }
        }
    }
    out
}

/// Would setting `parent_id = new_parent` on `id` create a cycle? True if
/// `new_parent` is `id` itself or already somewhere beneath it.
pub fn would_cycle(people: &[Person], id: &str, new_parent: &str) -> bool {
    new_parent == id || subtree(people, id).iter().any(|p| p.id == new_parent)
}

/// Whether `superior` may perform `action` on `target`: the superior must hold the
/// permission AND `target` must lie within the superior's subtree.
pub fn can(people: &[Person], superior_id: &str, action: &str, target_id: &str) -> bool {
    match find(people, superior_id) {
        Some(s) if s.permissions.iter().any(|a| a == action) => {
            subtree(people, superior_id).iter().any(|p| p.id == target_id)
        }
        _ => false,
    }
}

/// The recipients to notify for `id`: each ancestor that has a recipient and the
/// `receive_notifications` permission. Nearest guardian first. Drives Phase 3's
/// NotifyGuardians node and the school "notify the parents" flow.
pub fn guardian_recipients(people: &[Person], id: &str) -> Vec<String> {
    ancestors(people, id)
        .into_iter()
        .filter(|g| g.permissions.iter().any(|a| a == "receive_notifications"))
        .filter_map(|g| g.recipient_label.clone())
        .filter(|r| !r.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// grandparent → parent → child, plus an unrelated person.
    fn family() -> Vec<Person> {
        let mut gp = Person::new("gp", "Grandparent");
        gp.permissions = vec!["receive_notifications".into(), "view_report".into()];
        gp.recipient_label = Some("GP phone".into());
        let mut parent = Person::new("p", "Parent");
        parent.parent_id = Some("gp".into());
        parent.permissions = vec!["receive_notifications".into()];
        parent.recipient_label = Some("Parent phone".into());
        let mut child = Person::new("c", "Child");
        child.parent_id = Some("p".into());
        vec![gp, parent, child, Person::new("x", "Unrelated")]
    }

    #[test]
    fn ancestors_walk_up_nearest_first() {
        let f = family();
        let ids: Vec<_> = ancestors(&f, "c").iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["p", "gp"]);
        assert!(ancestors(&f, "gp").is_empty());
    }

    #[test]
    fn subtree_collects_all_descendants() {
        let f = family();
        let mut ids: Vec<_> = subtree(&f, "gp").iter().map(|p| p.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["c", "p"]);
    }

    #[test]
    fn would_cycle_blocks_self_and_descendants() {
        let f = family();
        assert!(would_cycle(&f, "gp", "gp")); // self
        assert!(would_cycle(&f, "gp", "c")); // c is beneath gp
        assert!(!would_cycle(&f, "c", "x")); // x is unrelated → fine
    }

    #[test]
    fn can_requires_permission_and_descendant() {
        let f = family();
        assert!(can(&f, "gp", "view_report", "c")); // gp has perm, c in subtree
        assert!(!can(&f, "gp", "view_photo", "c")); // gp lacks view_photo
        assert!(!can(&f, "p", "view_report", "c")); // p lacks view_report
        assert!(!can(&f, "gp", "view_report", "x")); // x not in subtree
    }

    #[test]
    fn guardian_recipients_are_permitted_ancestors_with_a_target() {
        let f = family();
        // Both ancestors can receive; nearest (parent) first.
        assert_eq!(guardian_recipients(&f, "c"), vec!["Parent phone", "GP phone"]);
    }

    #[test]
    fn best_match_picks_closest_within_threshold_and_rewards_samples() {
        let m = |pid: &str, d: f32| RecognitionMatch {
            person_id: pid.into(),
            name: pid.to_uppercase(),
            distance: d,
        };
        // Closest wins; two supporting samples for "a" add a small bonus.
        let r = best_match(&[m("a", 0.10), m("a", 0.20), m("b", 0.30)], 0.4).unwrap();
        assert_eq!(r.person_id, "a");
        assert_eq!(r.confidence, ((1.0_f64 - 0.10) * 100.0).round() as i64 + 3);
        // Nothing within threshold → no match.
        assert!(best_match(&[m("a", 0.9)], 0.4).is_none());
        assert!(best_match(&[], 0.4).is_none());
    }
}
