//! Presence: a categorized detection of something in a room, written by the
//! `RecordPresence` workflow node when movement/recognition fires. Persisted (so the
//! map can replay recent presence) and surfaced live as a pulsing dot via the SSE
//! relay. Mirrors the `alerts` slice: pure domain here, Postgres in [`repo`].
use serde::{Deserialize, Serialize};

use crate::db::Db;

pub mod repo;

/// What was seen. Drives the pulse colour/icon on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    #[default]
    Person,
    KnownPerson,
    Car,
    Cat,
    Dog,
    Unknown,
}

impl Category {
    /// Lenient parse from a node input / config string (case + separator insensitive).
    pub fn parse(s: &str) -> Category {
        match s
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_")
            .as_str()
        {
            "person" | "unknown_person" | "people" => Category::Person,
            "known_person" | "known" => Category::KnownPerson,
            "car" | "vehicle" => Category::Car,
            "cat" => Category::Cat,
            "dog" => Category::Dog,
            "" => Category::Person,
            _ => Category::Unknown,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Category::Person => "person",
            Category::KnownPerson => "known_person",
            Category::Car => "car",
            Category::Cat => "cat",
            Category::Dog => "dog",
            Category::Unknown => "unknown",
        }
    }
}

/// One recorded presence event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presence {
    pub id: u64,
    pub category: Category,
    /// Optional caption (e.g. the recognized person's name).
    #[serde(default)]
    pub label: Option<String>,
    /// The map room this was seen in, if resolved (optional).
    #[serde(default)]
    pub room_id: Option<u32>,
    /// The room's name at record time (survives a rename/delete).
    #[serde(default)]
    pub room_name: Option<String>,
    /// The camera/IP that detected it, if known (used to place the pulse).
    #[serde(default)]
    pub camera: Option<String>,
    /// How long the map keeps the pulse alive, in seconds.
    #[serde(default = "default_duration")]
    pub duration_secs: u32,
    pub source: String,
    pub created_at: String, // RFC3339
    /// Normalised horizontal position within the camera FOV (-0.5 left, 0 centre, 0.5 right).
    /// Computed from the detection bbox when `frame_w`/`frame_h` are wired into RecordPresence.
    #[serde(default)]
    pub detect_x: Option<f32>,
    /// Normalised depth within the camera range (0 = near camera, 1 = far edge of range).
    #[serde(default)]
    pub detect_depth: Option<f32>,
}

fn default_duration() -> u32 {
    8
}

/// Persist a new presence event for the tenant and return the stored record (with
/// its server-assigned id). Best-effort: a DB error yields id 0 (the live pulse and
/// node still fire).
#[allow(clippy::too_many_arguments)]
pub fn create(
    db: &Db,
    tenant_id: uuid::Uuid,
    category: Category,
    label: Option<String>,
    room_id: Option<u32>,
    room_name: Option<String>,
    camera: Option<String>,
    duration_secs: u32,
    source: String,
    detect_x: Option<f32>,
    detect_depth: Option<f32>,
) -> Presence {
    let created_at = crate::timeutil::now_rfc3339();
    let id = repo::insert(
        db,
        tenant_id,
        category,
        label.as_deref(),
        room_id,
        room_name.as_deref(),
        camera.as_deref(),
        duration_secs,
        &source,
        &created_at,
        detect_x,
        detect_depth,
    )
    .unwrap_or(0);
    Presence {
        id,
        category,
        label,
        room_id,
        room_name,
        camera,
        duration_secs,
        source,
        created_at,
        detect_x,
        detect_depth,
    }
}
