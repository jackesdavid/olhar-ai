//! Workflows node layer — the isolation boundary. Ported from the ratatui
//! `src/workflows/node.rs` and re-pointed at the **web** `ToolContext` (devices +
//! app-data paths + resolved Telegram config + the UI-effect channel).
//!
//! A node declares typed ports; the engine resolves its inputs and hands it ONLY
//! its own `PortValues`, taking back ONLY its declared outputs. A node never sees
//! the graph or another node.
//!
//! `PortValue` is a typed enum (not `serde_json::Value`) so an `Image` port can
//! carry raw JPEG bytes — JSON can't hold image bytes cleanly. Action nodes adapt
//! the core domain functions directly (typed in/out, no string-result parsing).

mod alerts;
mod ai;
mod camera;
mod control;
mod debug_node;
mod logic;
mod network;
mod presence;
mod recipients;
mod recording;
mod telegram;
mod text;
mod variable;
mod vision;

use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::ToolContext;
use crate::device_actions as dev_actions;
use serde_json::Value;
use std::collections::HashMap;

/// A file value: raw bytes plus the filename and MIME used for multipart uploads.
#[derive(Debug, Clone)]
pub struct FileVal {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// A typed value flowing along an edge / sitting on a port.
#[derive(Debug, Clone)]
pub enum PortValue {
    Text(String),
    Num(i64),
    Bool(bool),
    Image(Vec<u8>),
    /// A notification recipient (the recipient's label).
    Recipient(String),
    /// An arbitrary string→string dictionary (HTTP headers / form fields).
    Dict(Vec<(String, String)>),
    /// Any file (bytes + name + MIME).
    File(FileVal),
    /// A typed array — elements are themselves `PortValue`s. Carried by the `*Array`
    /// ports and iterated by `ForEach`.
    List(Vec<PortValue>),
    /// A detected person (element of a `PersonDetection[]`).
    Person(crate::agent::detector::PersonDetection),
    /// A detected face (element of a `FaceDetection[]`).
    Face(crate::agent::face::FaceDetection),
    /// A camera device entity (id + ip + name, credential-free). Carried by `Camera` ports.
    Camera(crate::workflows::domain::CameraEntity),
    /// A bounding box (canonical xyxy pixels). Carried by `Bbox` ports.
    Bbox(crate::workflows::domain::BBox),
}

impl PortValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            PortValue::Text(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_num(&self) -> Option<i64> {
        match self {
            PortValue::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PortValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_image(&self) -> Option<&[u8]> {
        match self {
            PortValue::Image(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_recipient(&self) -> Option<&str> {
        match self {
            PortValue::Recipient(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_dict(&self) -> Option<&[(String, String)]> {
        match self {
            PortValue::Dict(m) => Some(m),
            _ => None,
        }
    }
    pub fn as_file(&self) -> Option<&FileVal> {
        match self {
            PortValue::File(f) => Some(f),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[PortValue]> {
        match self {
            PortValue::List(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_person(&self) -> Option<&crate::agent::detector::PersonDetection> {
        match self {
            PortValue::Person(p) => Some(p),
            _ => None,
        }
    }
    pub fn as_face(&self) -> Option<&crate::agent::face::FaceDetection> {
        match self {
            PortValue::Face(f) => Some(f),
            _ => None,
        }
    }
    pub fn as_camera(&self) -> Option<&crate::workflows::domain::CameraEntity> {
        match self {
            PortValue::Camera(c) => Some(c),
            _ => None,
        }
    }
    pub fn as_bbox(&self) -> Option<crate::workflows::domain::BBox> {
        match self {
            PortValue::Bbox(b) => Some(*b),
            _ => None,
        }
    }
}

/// Read a file path into a `FileVal` (bytes + basename + guessed MIME).
fn read_file_val(path: &str) -> Option<FileVal> {
    let bytes = std::fs::read(path).ok()?;
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
    .to_string();
    Some(FileVal { name, mime, bytes })
}

/// One node's resolved port values (inputs the engine assembles, or outputs a node returns).
pub type PortValues = HashMap<String, PortValue>;

/// Convert a node's config literal into a typed value for an UNCONNECTED input port.
/// Empty text / missing keys / `Image` ports yield `None` (treated as "not provided").
pub fn literal_for(config: &Value, port: Port) -> Option<PortValue> {
    literal_for_named(config, port.name, port.ty)
}

/// As [`literal_for`], but by port name + type — so config-derived ports (with owned
/// names, e.g. EmitEvent payload inputs) resolve a literal too. Payload ports carry no
/// config literal (their value arrives by wire), so this returns `None` for them.
pub fn literal_for_named(config: &Value, name: &str, ty: PortType) -> Option<PortValue> {
    let v = config.get(name)?;
    match ty {
        PortType::Text => {
            let s = v.as_str()?;
            if s.trim().is_empty() {
                None
            } else {
                Some(PortValue::Text(s.to_string()))
            }
        }
        PortType::Num => {
            if let Some(n) = v.as_i64() {
                Some(PortValue::Num(n))
            } else {
                v.as_str().and_then(|s| s.trim().parse::<i64>().ok()).map(PortValue::Num)
            }
        }
        PortType::Bool => v.as_bool().map(PortValue::Bool),
        // An image literal is a file path the user picked; read it at run time.
        PortType::Image => v
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .and_then(|p| std::fs::read(p).ok())
            .map(PortValue::Image),
        // A recipient literal is the recipient's label (also produced by GetRecipient).
        PortType::Recipient => {
            let s = v.as_str()?;
            if s.trim().is_empty() {
                None
            } else {
                Some(PortValue::Recipient(s.to_string()))
            }
        }
        // A dictionary literal is a JSON object of string→string in config.
        PortType::Dictionary => {
            let obj = v.as_object()?;
            if obj.is_empty() {
                return None;
            }
            let pairs = obj
                .iter()
                .map(|(k, val)| {
                    let s = val.as_str().map(str::to_string).unwrap_or_else(|| val.to_string());
                    (k.clone(), s)
                })
                .collect();
            Some(PortValue::Dict(pairs))
        }
        // A file literal is a path the user picked; read it at run time.
        PortType::File => {
            v.as_str().filter(|s| !s.trim().is_empty()).and_then(read_file_val).map(PortValue::File)
        }
        // Control-flow ports never carry a value.
        PortType::Exec => None,
        // Camera literals are resolved by the engine (needs sys.devices); not a plain config value.
        PortType::Camera => None,
        // Arrays / detection elements / the Any wildcard are produced by upstream nodes
        // (or, for ForEach `items`, parsed from text in the engine) — never a config literal.
        PortType::TextArray
        | PortType::NumArray
        | PortType::PersonArray
        | PortType::FaceArray
        | PortType::Person
        | PortType::Face
        | PortType::Bbox
        | PortType::Any => None,
    }
}

// ---------------------------------------------------------------------------
// Camera entity helpers (used by the engine + nodes)
// ---------------------------------------------------------------------------

/// Resolve a query string (device id, IP, or name) to a `CameraEntity` using `sys.devices`.
/// Returns `None` if the query is empty or no device matches.
pub fn resolve_camera_entity(query: &str, sys: &ToolContext) -> Option<crate::workflows::domain::CameraEntity> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let devices = sys.devices.lock().unwrap();
    let idx = dev_actions::resolve_device(&devices, q).ok()?;
    let d = &devices[idx];
    Some(crate::workflows::domain::CameraEntity { id: d.id.clone(), ip: d.ip.clone(), name: d.name.clone() })
}

/// Seed a run's variables map from the graph's declared defaults (card #5b), so a `Get Variable`
/// before any `Set` returns the author's default instead of a type-zero. Called once at run start.
/// Unparseable / typeless defaults are skipped (the `Get` then falls back to its type-zero).
pub fn seed_variable_defaults(vars: &[crate::workflows::domain::VariableDecl], sys: &ToolContext) {
    use crate::workflows::domain::PortType;
    let mut seeded: Vec<(String, PortValue)> = Vec::new();
    for v in vars {
        let Some(raw) = v.default.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let pv = match v.ty {
            PortType::Text => PortValue::Text(raw.to_string()),
            PortType::Recipient => PortValue::Recipient(raw.to_string()),
            PortType::Num => match raw.parse::<i64>() {
                Ok(n) => PortValue::Num(n),
                Err(_) => continue,
            },
            PortType::Bool => PortValue::Bool(matches!(raw, "true" | "1" | "yes" | "on")),
            PortType::Camera => match resolve_camera_entity(raw, sys) {
                Some(c) => PortValue::Camera(c),
                None => continue,
            },
            _ => continue, // Image/File/arrays/Bbox/Person/Face: no author-typable default
        };
        seeded.push((v.name.clone(), pv));
    }
    if seeded.is_empty() {
        return;
    }
    let mut map = sys.variables.lock().unwrap();
    for (name, pv) in seeded {
        map.insert(name, pv);
    }
}

/// Resolve a `Camera` config literal (the dropdown-stored device id) to a `PortValue::Camera`.
/// Called by the engine when a `PortType::Camera` input port has no wired edge.
pub fn resolve_camera_literal(config: &Value, key: &str, sys: &ToolContext) -> Option<PortValue> {
    let id = config.get(key).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty())?;
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
    use crate::workflows::domain::{BBox, PortType};
    let mut out = payload.clone();
    for port in ports {
        match port.ty {
            PortType::Camera => {
                if let Some(PortValue::Text(s)) = out.get(&port.name).cloned() {
                    if let Some(cam) = resolve_camera_entity(&s, sys) {
                        out.insert(port.name.clone(), PortValue::Camera(cam));
                    }
                }
            }
            PortType::Bbox => {
                if let Some(PortValue::List(items)) = out.get(&port.name).cloned() {
                    let nums: Vec<i64> = items.iter().filter_map(PortValue::as_num).collect();
                    if let Some(b) = BBox::from_xyxy_slice(&nums) {
                        out.insert(port.name.clone(), PortValue::Bbox(b));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Get the full `Device` from `sys.devices` using a `CameraEntity`'s stable id.
fn device_from_camera<'a>(
    cam: &crate::workflows::domain::CameraEntity,
    devices: &'a [crate::devices::Device],
) -> Result<&'a crate::devices::Device, String> {
    devices
        .iter()
        .find(|d| d.id == cam.id)
        .ok_or_else(|| format!("camera '{}' not found (was it removed?)", cam.name))
}

/// Execute one node against its resolved inputs. Returns its declared outputs.
pub fn run_node(
    kind: NodeKind,
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    match kind {
        NodeKind::GetLastAlert => alerts::run_get_last_alert(sys),
        NodeKind::GetRecipient => recipients::run_get_recipient(config),
        NodeKind::CreateAlert => alerts::run_create_alert(config, inputs, sys),
        NodeKind::DeliverAlert => alerts::run_deliver_alert(inputs, sys),
        NodeKind::CameraSnapshot => camera::run_camera_snapshot(inputs, sys),
        NodeKind::GetCurrentFrame => camera::run_get_current_frame(config, inputs, sys),
        NodeKind::GetCameraByName => camera::run_get_camera_by_name(config, sys),
        NodeKind::BreakCamera => camera::run_break_camera(inputs),
        NodeKind::DescribeImage => ai::run_describe_image(config, inputs, sys),
        NodeKind::NotifyRecipient => recipients::run_notify_recipient(inputs, sys),
        NodeKind::Format => text::run_format(config, inputs),
        NodeKind::Wait => control::run_wait(config),
        NodeKind::HttpPost => network::run_http_post(config, inputs),
        NodeKind::If => control::run_if(inputs),
        // ForEach is engine-driven (like If/EmitEvent): the scheduler parses `items`,
        // advances the index across epochs, sets item/index, and fires loop_body/completed.
        NodeKind::ForEach => Ok(PortValues::new()),
        NodeKind::MotionDetection => vision::run_motion_detection(config, inputs),
        NodeKind::MotionWatch => vision::run_motion_watch(config, inputs, sys),
        NodeKind::PersonDetection => vision::run_person_detection(inputs, sys),
        NodeKind::ObjectDetection => vision::run_object_detection(config, inputs, sys),
        NodeKind::BreakPersonDetection => vision::run_break_person_detection(inputs),
        NodeKind::BreakDetection => vision::run_break_detection(inputs),
        NodeKind::BreakFaceDetection => vision::run_break_face_detection(inputs),
        NodeKind::RecognizePerson => vision::run_recognize_person(config, inputs, sys),
        NodeKind::RecordPresence => presence::run_record_presence(config, inputs, sys),
        // EmitEvent/OnEvent are exec-flow markers: the engine handles the event fire
        // (spawning the guarded sub-run) and the OnEvent entry, like the If/ShowImage
        // special-cases. Here they're no-ops that just pass exec through.
        NodeKind::EmitEvent | NodeKind::OnEvent => Ok(PortValues::new()),
        NodeKind::Print => debug_node::run_print(config, inputs, sys),
        // ShowImage is a debug sink whose only effect — mirroring its input frame to
        // the editor — is driven by the engine, which owns the node id the UI needs to
        // place the preview. Here it's a pure pass-through (exec flows on, no data out).
        NodeKind::ShowImage => Ok(PortValues::new()),
        NodeKind::DrawBbox => vision::run_draw_bbox(config, inputs),
        NodeKind::DrawText => vision::run_draw_text(config, inputs),
        NodeKind::BlurOutsideBbox => vision::run_blur_outside_bbox(config, inputs),
        NodeKind::GetImageSize => vision::run_get_image_size(inputs),
        // FlipFlop is engine-driven (reads/writes cross-run state + routes exec).
        // Never called directly — the engine handles it before reaching run_node.
        NodeKind::FlipFlop => Ok(PortValues::new()),
        NodeKind::Compare => logic::run_compare(config, inputs),
        NodeKind::NumLt => logic::run_num_cmp(config, inputs, |a, b| a < b),
        NodeKind::NumLte => logic::run_num_cmp(config, inputs, |a, b| a <= b),
        NodeKind::NumGt => logic::run_num_cmp(config, inputs, |a, b| a > b),
        NodeKind::NumGte => logic::run_num_cmp(config, inputs, |a, b| a >= b),
        NodeKind::NumEq => logic::run_num_cmp(config, inputs, |a, b| a == b),
        NodeKind::NumNe => logic::run_num_cmp(config, inputs, |a, b| a != b),
        NodeKind::And => logic::run_and(config, inputs),
        NodeKind::Or => logic::run_or(config, inputs),
        NodeKind::Not => logic::run_not(config, inputs),
        NodeKind::TextCompare => logic::run_text_compare(config, inputs),
        NodeKind::NumToText => logic::run_num_to_text(inputs),
        NodeKind::BoolToText => logic::run_bool_to_text(inputs),
        // Cooldown is engine-driven (reads/writes cross-run node state).
        NodeKind::Cooldown => Ok(PortValues::new()),
        // StringSwitch / CameraSwitch are engine-driven (the engine reads the value and
        // fires the matching arm in forward_epoch). Here we just validate the required input.
        NodeKind::StringSwitch => control::run_string_switch(inputs),
        NodeKind::CameraSwitch => control::run_camera_switch(inputs),
        NodeKind::AskAi => ai::run_ask_ai(config, inputs, sys),
        // Comment is a canvas organizer — it never executes.
        // Reroute is collapsed to a direct edge before the engine runs (see engine::flatten_reroutes).
        NodeKind::Comment | NodeKind::Reroute => Ok(PortValues::new()),
        NodeKind::StartRecording => recording::run_start_recording(config, inputs, sys),
        NodeKind::StopRecording => recording::run_stop_recording(inputs, sys),
        // TimeGate is engine-driven (reads/writes cross-run state, polls until expiry).
        // Never called through run_node — the engine handles it in forward_epoch.
        NodeKind::TimeGate => Ok(PortValues::new()),
        NodeKind::GetVariable => variable::run_get_variable(config, sys),
        NodeKind::SetVariable => variable::run_set_variable(config, inputs, sys),
        NodeKind::ReplyToSender => telegram::run_reply_to_sender(config, inputs, sys),
        NodeKind::AiAnswer => ai::run_ai_answer(config, inputs, sys),
        NodeKind::ReplyWithRecording => telegram::run_reply_with_recording(config, inputs, sys),
        // AiSwitch computes the chosen arm here; the engine reads o["arm"] to fire it (like AskAi).
        NodeKind::AiSwitch => ai::run_ai_switch(config, inputs, sys),
        NodeKind::AiExtract => ai::run_ai_extract(config, inputs, sys),
    }
}

/// The per-node execution deadline. `None` ⇒ the node runs inline with no watchdog —
/// pure/instant kinds (nothing to hang on) and `Wait` (a deliberate sleep that must not be
/// killed). `Some(d)` ⇒ an IO-bound kind that could block on the network/DB: a sensible
/// per-kind default, overridable per node via a numeric `timeout_secs` in its config
/// (`0` disables the watchdog for that node). The engine wraps these via [`run_node_timed`]
/// so a stuck node aborts the run (and frees its concurrency slot) instead of hanging forever.
pub fn node_timeout(kind: NodeKind, config: &Value) -> Option<std::time::Duration> {
    let default_secs = match kind {
        NodeKind::CameraSnapshot | NodeKind::GetCurrentFrame => 10,
        NodeKind::AskAi => 65,
        NodeKind::PersonDetection | NodeKind::ObjectDetection | NodeKind::MotionDetection => 30,
        NodeKind::MotionWatch => 20, // two camera grabs + the sample gap
        NodeKind::RecognizePerson => 30,
        NodeKind::DescribeImage => 65,
        NodeKind::HttpPost | NodeKind::NotifyRecipient | NodeKind::DeliverAlert => 35,
        NodeKind::CreateAlert | NodeKind::RecordPresence => 15,
        NodeKind::StartRecording | NodeKind::StopRecording => 30,
        NodeKind::ReplyToSender => 35,
        NodeKind::AiAnswer => 30,
        NodeKind::AiSwitch => 30,
        NodeKind::AiExtract => 30,
        NodeKind::ReplyWithRecording => 120, // fetch clip from edge + upload
        // Inline (no watchdog): instant/pure nodes + the deliberate `Wait` sleep + the
        // engine-driven markers (ForEach/EmitEvent/OnEvent/ShowImage/FlipFlop).
        NodeKind::Wait
        | NodeKind::Format
        | NodeKind::If
        | NodeKind::Compare
        | NodeKind::NumLt
        | NodeKind::NumLte
        | NodeKind::NumGt
        | NodeKind::NumGte
        | NodeKind::NumEq
        | NodeKind::NumNe
        | NodeKind::NumToText
        | NodeKind::BoolToText
        | NodeKind::FlipFlop
        | NodeKind::Cooldown
        | NodeKind::Print
        | NodeKind::GetRecipient
        | NodeKind::GetLastAlert
        | NodeKind::BreakPersonDetection
        | NodeKind::BreakDetection
        | NodeKind::BreakFaceDetection
        | NodeKind::EmitEvent
        | NodeKind::OnEvent
        | NodeKind::ShowImage
        | NodeKind::DrawBbox
        | NodeKind::DrawText
        | NodeKind::BlurOutsideBbox
        | NodeKind::GetImageSize
        | NodeKind::And
        | NodeKind::Or
        | NodeKind::Not
        | NodeKind::TextCompare
        | NodeKind::ForEach
        | NodeKind::StringSwitch
        | NodeKind::CameraSwitch
        | NodeKind::GetCameraByName
        | NodeKind::BreakCamera
        | NodeKind::Comment
        | NodeKind::Reroute
        | NodeKind::TimeGate
        | NodeKind::GetVariable
        | NodeKind::SetVariable => return None,
    };
    let secs = config_u64(config, "timeout_secs").unwrap_or(default_secs);
    if secs == 0 {
        None
    } else {
        Some(std::time::Duration::from_secs(secs))
    }
}

/// Read a non-negative integer config value, accepting both a JSON number and a numeric
/// string (the editor often stores config fields as strings).
fn config_u64(config: &Value, key: &str) -> Option<u64> {
    config
        .get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

/// Run a node under a hard deadline (see [`node_timeout`]). The body runs on a detached
/// worker thread; if it doesn't finish within `deadline` we return a timeout error so the
/// engine aborts the run instead of blocking forever. The orphaned worker keeps running
/// until its own IO returns — every node's IO path is bounded (frame source / ONVIF /
/// recognize), so it ends promptly rather than leaking indefinitely.
pub fn run_node_timed(
    kind: NodeKind,
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
    deadline: std::time::Duration,
) -> Result<PortValues, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let config = config.clone();
    let inputs = inputs.clone();
    let sys = sys.clone();
    std::thread::spawn(move || {
        // The receiver may already be gone (we timed out) — ignore the send error.
        let _ = tx.send(run_node(kind, &config, &inputs, &sys));
    });
    match rx.recv_timeout(deadline) {
        Ok(out) => out,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("timed out after {}s", deadline.as_secs()))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("node worker thread died before returning".into())
        }
    }
}

/// A short, human-readable line describing what a node did this run — shown in the
/// editor console when the graph's `debug` flag is on. Reads the node's own outputs
/// (and a couple of inputs/config) so each kind reports something useful.
pub fn debug_summary(
    kind: NodeKind,
    config: &Value,
    inputs: &PortValues,
    outputs: &PortValues,
) -> String {
    let txt = |k: &str| outputs.get(k).and_then(PortValue::as_text);
    let num = |k: &str| outputs.get(k).and_then(PortValue::as_num);
    let flag = |k: &str| outputs.get(k).and_then(PortValue::as_bool) == Some(true);
    let clip = |s: &str| {
        if s.chars().count() > 80 {
            format!("{}…", s.chars().take(80).collect::<String>())
        } else {
            s.to_string()
        }
    };
    match kind {
        NodeKind::GetLastAlert => match txt("message") {
            Some(m) => format!("last alert: {}", clip(m)),
            None => "no alert in store".into(),
        },
        NodeKind::GetRecipient => match txt("recipient") {
            Some(r) if !r.is_empty() => format!("recipient: {r}"),
            _ => "no recipient set".into(),
        },
        NodeKind::CreateAlert => txt("result").map(clip).unwrap_or_else(|| "alert created".into()),
        NodeKind::DeliverAlert => txt("result").map(clip).unwrap_or_else(|| "delivered".into()),
        NodeKind::CameraSnapshot => txt("result").map(clip).unwrap_or_else(|| "snapshot captured".into()),
        NodeKind::GetCurrentFrame => {
            let kb = inputs.get("image").and_then(PortValue::as_image).map_or(0, <[u8]>::len) / 1024;
            format!("current frame ({} KB, {} ms old)", kb, num("age_ms").unwrap_or(0))
        }
        NodeKind::DescribeImage => match txt("description") {
            Some(d) => clip(d),
            None => "described image".into(),
        },
        NodeKind::NotifyRecipient => txt("result").map(clip).unwrap_or_else(|| "notification sent".into()),
        NodeKind::Format => format!("→ {}", clip(txt("text").unwrap_or(""))),
        NodeKind::Wait => "delay complete".into(),
        NodeKind::HttpPost => format!("HTTP {} {}", num("status").unwrap_or(0), if flag("ok") { "ok" } else { "failed" }),
        NodeKind::If => {
            let cond = inputs.get("condition").and_then(PortValue::as_bool).unwrap_or(false);
            format!("condition {cond} → '{}' branch", if cond { "true" } else { "false" })
        }
        NodeKind::ForEach => match num("index") {
            Some(i) => match txt("item") {
                Some(it) => format!("item[{i}] = {}", clip(it)),
                None => format!("item[{i}]"),
            },
            None => "loop complete".into(),
        },
        NodeKind::MotionDetection => {
            let sens = config
                .get("sensitivity")
                .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
                .unwrap_or(2);
            format!(
                "score {}% (threshold {}%) → {}",
                num("score").unwrap_or(0),
                sens,
                if flag("moved") { "moved" } else { "still" }
            )
        }
        NodeKind::MotionWatch => format!(
            "{} · score {}% → {}",
            config.get("camera").and_then(|v| v.as_str()).unwrap_or("first"),
            num("score").unwrap_or(0),
            if flag("moved") { "moved" } else { "still" }
        ),
        NodeKind::PersonDetection => match num("count").unwrap_or(0) {
            0 => "no person detected".into(),
            c => format!("{c} person(s) ({}% confidence)", num("confidence").unwrap_or(0)),
        },
        NodeKind::ObjectDetection => {
            let class = config.get("class").and_then(|v| v.as_str()).unwrap_or("person");
            match num("count").unwrap_or(0) {
                0 => format!("no {class} detected"),
                c => format!("{c} {class}(s) ({}% confidence)", num("confidence").unwrap_or(0)),
            }
        }
        NodeKind::BreakPersonDetection | NodeKind::BreakDetection | NodeKind::BreakFaceDetection => {
            format!("bbox + {}% confidence", num("confidence").unwrap_or(0))
        }
        NodeKind::Print => {
            let msg = inputs.get("message").and_then(PortValue::as_text).unwrap_or("");
            format!("print: {}", clip(msg))
        }
        NodeKind::RecognizePerson => {
            if flag("ok") {
                format!("recognised {} ({}%)", txt("person").unwrap_or("?"), num("confidence").unwrap_or(0))
            } else {
                "no match".into()
            }
        }
        NodeKind::RecordPresence => format!("presence recorded (#{})", num("id").unwrap_or(0)),
        NodeKind::EmitEvent => {
            let ev = config.get("event").and_then(|v| v.as_str()).unwrap_or("");
            format!("emit '{ev}'")
        }
        NodeKind::OnEvent => {
            let ev = config.get("event").and_then(|v| v.as_str()).unwrap_or("");
            format!("on '{ev}'")
        }
        NodeKind::ShowImage => {
            let bytes = inputs.get("image").and_then(PortValue::as_image).map_or(0, <[u8]>::len);
            if bytes > 0 {
                format!("image shown ({} KB)", bytes / 1024)
            } else {
                "no image".into()
            }
        }
        NodeKind::FlipFlop => {
            let branch = outputs.get("is_a").and_then(PortValue::as_bool).map(|b| if b { "a" } else { "b" }).unwrap_or("?");
            format!("flip flop → {branch}")
        }
        NodeKind::Compare => {
            let a = inputs.get("a").and_then(PortValue::as_num).unwrap_or(0);
            let b = inputs.get("b").and_then(PortValue::as_num).unwrap_or(0);
            let op = config.get("operator").and_then(|v| v.as_str()).unwrap_or("==");
            let result = outputs.get("result").and_then(PortValue::as_bool).unwrap_or(false);
            format!("{a} {op} {b} → {result}")
        }
        NodeKind::NumLt => num_cmp_summary("<", inputs, outputs),
        NodeKind::NumLte => num_cmp_summary("<=", inputs, outputs),
        NodeKind::NumGt => num_cmp_summary(">", inputs, outputs),
        NodeKind::NumGte => num_cmp_summary(">=", inputs, outputs),
        NodeKind::NumEq => num_cmp_summary("==", inputs, outputs),
        NodeKind::NumNe => num_cmp_summary("!=", inputs, outputs),
        NodeKind::NumToText => {
            let v = inputs.get("value").and_then(PortValue::as_num).unwrap_or(0);
            let text = outputs.get("text").and_then(PortValue::as_text).unwrap_or("");
            format!("{v} → \"{text}\"")
        }
        NodeKind::BoolToText => {
            let v = inputs.get("value").and_then(PortValue::as_bool).unwrap_or(false);
            let text = outputs.get("text").and_then(PortValue::as_text).unwrap_or("");
            format!("{v} → \"{text}\"")
        }
        NodeKind::Cooldown => {
            let elapsed = outputs.get("elapsed_secs").and_then(PortValue::as_num).unwrap_or(0);
            let ok = outputs.get("ok").and_then(PortValue::as_bool).unwrap_or(false);
            let window = config.get("window_secs").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(60);
            format!("{elapsed}s elapsed / {window}s → {}", if ok { "pass" } else { "blocked" })
        }
        NodeKind::DrawBbox => {
            let bytes = outputs.get("image").and_then(PortValue::as_image).map_or(0, <[u8]>::len);
            format!("bbox drawn ({} KB)", bytes / 1024)
        }
        NodeKind::DrawText => {
            let t = inputs.get("text").and_then(PortValue::as_text)
                .or_else(|| config.get("text").and_then(|v| v.as_str()))
                .unwrap_or("");
            format!("text drawn: {}", clip(t))
        }
        NodeKind::BlurOutsideBbox => {
            let bytes = outputs.get("image").and_then(PortValue::as_image).map_or(0, <[u8]>::len);
            format!("blurred outside bbox ({} KB)", bytes / 1024)
        }
        NodeKind::GetImageSize => {
            let w = outputs.get("width").and_then(PortValue::as_num).unwrap_or(0);
            let h = outputs.get("height").and_then(PortValue::as_num).unwrap_or(0);
            format!("{w}×{h}")
        }
        NodeKind::And => {
            let a = inputs.get("a").and_then(PortValue::as_bool).unwrap_or(false);
            let b = inputs.get("b").and_then(PortValue::as_bool).unwrap_or(false);
            let result = outputs.get("result").and_then(PortValue::as_bool).unwrap_or(false);
            format!("{a} AND {b} → {result}")
        }
        NodeKind::Or => {
            let a = inputs.get("a").and_then(PortValue::as_bool).unwrap_or(false);
            let b = inputs.get("b").and_then(PortValue::as_bool).unwrap_or(false);
            let result = outputs.get("result").and_then(PortValue::as_bool).unwrap_or(false);
            format!("{a} OR {b} → {result}")
        }
        NodeKind::Not => {
            let a = inputs.get("a").and_then(PortValue::as_bool).unwrap_or(false);
            let result = outputs.get("result").and_then(PortValue::as_bool).unwrap_or(false);
            format!("NOT {a} → {result}")
        }
        NodeKind::TextCompare => {
            let a = inputs.get("a").and_then(PortValue::as_text).unwrap_or("");
            let b = inputs.get("b").and_then(PortValue::as_text)
                .or_else(|| config.get("b").and_then(|v| v.as_str()))
                .unwrap_or("");
            let op = config.get("operator").and_then(|v| v.as_str()).unwrap_or("eq");
            let result = outputs.get("result").and_then(PortValue::as_bool).unwrap_or(false);
            let clip = |s: &str| if s.chars().count() > 20 { format!("{}…", s.chars().take(19).collect::<String>()) } else { s.to_string() };
            format!("\"{}\" {op} \"{}\" → {result}", clip(a), clip(b))
        }
        NodeKind::StringSwitch => {
            let val = inputs.get("value").and_then(PortValue::as_text).unwrap_or("");
            let arms = super::domain::string_switch_arms(config);
            let fired = if arms.iter().any(|a| a == val) { val } else { "default" };
            format!("\"{}\" → arm '{fired}'", clip(val))
        }
        NodeKind::CameraSwitch => {
            let cam = inputs.get("camera").and_then(PortValue::as_camera);
            if let Some(cam) = cam {
                let arms = super::domain::camera_switch_arms(config);
                let idx = arms.iter().position(|a| !a.is_empty() && (a == &cam.ip || a == &cam.id));
                let fired = idx.map(|i| format!("arm_{}", i + 1)).unwrap_or_else(|| "default".to_string());
                format!("{} → {fired}", cam.name)
            } else {
                "no camera → default".to_string()
            }
        }
        NodeKind::AskAi => {
            let q = inputs.get("question").and_then(PortValue::as_text)
                .or_else(|| config.get("question").and_then(|v| v.as_str()))
                .unwrap_or("");
            let answer = outputs.get("result").and_then(PortValue::as_bool).unwrap_or(false);
            format!("AI: \"{}\" → {}", clip(q), if answer { "true" } else { "false" })
        }
        NodeKind::GetCameraByName => {
            let name = outputs.get("camera").and_then(PortValue::as_camera).map(|c| c.name.as_str()).unwrap_or("?");
            format!("camera: {name}")
        }
        NodeKind::BreakCamera => {
            let name = inputs.get("camera").and_then(PortValue::as_camera).map(|c| c.name.as_str()).unwrap_or("?");
            format!("break '{name}' → id/ip/name")
        }
        NodeKind::Comment => "comment".into(),
        NodeKind::Reroute => "reroute".into(),
        NodeKind::StartRecording => {
            let new = outputs.get("recording_new").and_then(|v| if let PortValue::Bool(b) = v { Some(*b) } else { None }).unwrap_or(true);
            txt("clip_id").map(|id| if new { format!("recording started: {id}") } else { format!("recording resumed: {id}") }).unwrap_or_else(|| "recording started".into())
        }
        NodeKind::StopRecording => "recording stopped".into(),
        NodeKind::TimeGate => "time gate".into(),
        NodeKind::GetVariable => {
            let name = config.get("variable").and_then(|v| v.as_str()).unwrap_or("?");
            format!("get ${name}")
        }
        NodeKind::SetVariable => {
            let name = config.get("variable").and_then(|v| v.as_str()).unwrap_or("?");
            format!("set ${name}")
        }
        NodeKind::ReplyToSender => {
            let chat = num("chat_id").or_else(|| inputs.get("chat_id").and_then(PortValue::as_num)).unwrap_or(0);
            txt("result").map(clip).unwrap_or_else(|| format!("reply → chat {chat}"))
        }
        NodeKind::AiAnswer => txt("response").map(clip).unwrap_or_else(|| "AI answered".into()),
        NodeKind::ReplyWithRecording => {
            let chat = inputs.get("chat_id").and_then(PortValue::as_num).unwrap_or(0);
            txt("result").map(clip).unwrap_or_else(|| format!("clip → chat {chat}"))
        }
        NodeKind::AiSwitch => format!("→ {}", txt("arm").unwrap_or("default")),
        NodeKind::AiExtract => {
            let found = outputs.get("found").and_then(PortValue::as_bool).unwrap_or(false);
            match txt("value") {
                Some(v) => format!("extracted: {}", clip(v)),
                None => if found { "extracted".into() } else { "not found".into() },
            }
        }
    }
}

fn num_cmp_summary(op: &str, inputs: &PortValues, outputs: &PortValues) -> String {
    let a = inputs.get("a").and_then(PortValue::as_num).unwrap_or(0);
    let b = inputs.get("b").and_then(PortValue::as_num).unwrap_or(0);
    let result = outputs.get("result").and_then(PortValue::as_bool).unwrap_or(false);
    format!("{a} {op} {b} → {result}")
}

fn req_text<'a>(inputs: &'a PortValues, name: &str) -> Result<&'a str, String> {
    inputs
        .get(name)
        .and_then(PortValue::as_text)
        .ok_or_else(|| format!("missing required input '{name}'"))
}

/// Decode just the JPEG/PNG header to extract image dimensions. Returns (0, 0) on error
/// so callers can treat unknown dimensions as absent without failing.
fn jpeg_dimensions(bytes: &[u8]) -> (i64, i64) {
    match image::load_from_memory(bytes) {
        Ok(img) => (img.width() as i64, img.height() as i64),
        Err(_) => (0, 0),
    }
}

pub(crate) fn to_jpeg(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode snapshot: {e}"))?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| format!("encode snapshot: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::domain::NodeKind;
    use super::*;
    use serde_json::json;

    fn test_ctx() -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext::new_local(
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            crate::db::Db::lazy("postgres://sentinel:sentinel@localhost/sentinel_test"),
            crate::tenancy::DEFAULT_TENANT_ID,
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            None,
            tx,
        )
    }

    #[test]
    fn node_timeout_maps_io_vs_pure_kinds() {
        use std::time::Duration;
        for k in [NodeKind::Wait, NodeKind::Format, NodeKind::If, NodeKind::Print, NodeKind::ForEach, NodeKind::ShowImage] {
            assert_eq!(node_timeout(k, &json!({})), None, "{k:?} should run inline");
        }
        assert_eq!(node_timeout(NodeKind::CameraSnapshot, &json!({})), Some(Duration::from_secs(10)));
        assert_eq!(node_timeout(NodeKind::DescribeImage, &json!({})), Some(Duration::from_secs(65)));
        assert_eq!(node_timeout(NodeKind::RecognizePerson, &json!({})), Some(Duration::from_secs(30)));
    }

    #[test]
    fn node_timeout_config_override_wins_and_zero_disables() {
        use std::time::Duration;
        assert_eq!(node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": "3" })), Some(Duration::from_secs(3)));
        assert_eq!(node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": 3 })), Some(Duration::from_secs(3)));
        assert_eq!(node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": "0" })), None);
    }

    #[test]
    fn run_node_timed_aborts_a_slow_node_promptly() {
        use std::time::{Duration, Instant};
        let cfg = json!({ "seconds": "5" });
        let start = Instant::now();
        let err = run_node_timed(NodeKind::Wait, &cfg, &PortValues::new(), &test_ctx(), Duration::from_millis(200))
            .unwrap_err();
        let elapsed = start.elapsed();
        assert!(err.contains("timed out"), "expected a timeout error, got: {err}");
        assert!(elapsed < Duration::from_secs(2), "should return at the deadline, not after the 5s body: {elapsed:?}");
    }

    #[test]
    fn run_node_timed_returns_a_fast_nodes_output() {
        use std::time::Duration;
        let cfg = json!({ "template": "hello" });
        let out = run_node_timed(NodeKind::Format, &cfg, &PortValues::new(), &test_ctx(), Duration::from_secs(5)).unwrap();
        assert_eq!(out.get("text").and_then(PortValue::as_text), Some("hello"));
    }

    #[test]
    fn literal_for_coerces_and_skips_empty() {
        let cfg = json!({ "camera": "front", "blank": "  ", "n": "7" });
        let text = Port { name: "camera", ty: PortType::Text, required: true };
        let blank = Port { name: "blank", ty: PortType::Text, required: false };
        let num = Port { name: "n", ty: PortType::Num, required: true };
        assert_eq!(literal_for(&cfg, text).unwrap().as_text(), Some("front"));
        assert!(literal_for(&cfg, blank).is_none());
        assert_eq!(literal_for(&cfg, num).unwrap().as_num(), Some(7));
    }

    #[test]
    fn literal_for_parses_a_string_dictionary() {
        let cfg = json!({ "headers": { "Authorization": "Bearer x", "X-Env": "prod" }, "empty": {} });
        let dict = Port { name: "headers", ty: PortType::Dictionary, required: false };
        let empty = Port { name: "empty", ty: PortType::Dictionary, required: false };
        let got = literal_for(&cfg, dict).unwrap();
        let pairs = got.as_dict().unwrap();
        assert_eq!(pairs.len(), 2);
        assert!(pairs.contains(&("Authorization".into(), "Bearer x".into())));
        assert!(literal_for(&cfg, empty).is_none());
    }

    #[test]
    fn http_post_requires_a_url_before_any_request() {
        let err = run_node(NodeKind::HttpPost, &json!({ "url": "  " }), &PortValues::new(), &test_ctx())
            .unwrap_err();
        assert!(err.contains("missing required input 'url'"), "got: {err}");
    }

    fn solid_jpeg(w: u32, h: u32, luma: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([luma, luma, luma]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .unwrap();
        buf
    }

    #[test]
    fn motion_detects_difference_between_frames() {
        let black = solid_jpeg(64, 48, 0);
        let white = solid_jpeg(64, 48, 255);
        let run = |a: &[u8], b: &[u8]| {
            let mut inputs = PortValues::new();
            inputs.insert("image_a".into(), PortValue::Image(a.to_vec()));
            inputs.insert("image_b".into(), PortValue::Image(b.to_vec()));
            run_node(NodeKind::MotionDetection, &json!({}), &inputs, &test_ctx()).unwrap()
        };
        let same = run(&black, &black);
        assert_eq!(same["moved"].as_bool(), Some(false));
        assert_eq!(same["score"].as_num(), Some(0));
        let diff = run(&black, &white);
        assert_eq!(diff["moved"].as_bool(), Some(true));
        assert!(diff["score"].as_num().unwrap() > 90);
    }

    #[test]
    fn person_detection_requires_an_image() {
        let err = run_node(NodeKind::PersonDetection, &json!({}), &PortValues::new(), &test_ctx())
            .unwrap_err();
        assert!(err.contains("missing required input 'image'"), "got: {err}");
    }

    #[test]
    fn recognize_person_requires_an_image() {
        let err = run_node(NodeKind::RecognizePerson, &json!({}), &PortValues::new(), &test_ctx())
            .unwrap_err();
        assert!(err.contains("missing required input 'image'"), "got: {err}");
    }

    #[test]
    fn show_image_is_a_sink_with_no_data_outputs() {
        let mut inputs = PortValues::new();
        inputs.insert("image".into(), PortValue::Image(solid_jpeg(32, 24, 128)));
        let out =
            run_node(NodeKind::ShowImage, &json!({ "label": "frame" }), &inputs, &test_ctx())
                .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn motion_requires_both_frames() {
        let err = run_node(NodeKind::MotionDetection, &json!({}), &PortValues::new(), &test_ctx())
            .unwrap_err();
        assert!(err.contains("missing required input 'image_a'"), "got: {err}");
    }

    #[test]
    fn if_requires_a_condition() {
        let err =
            run_node(NodeKind::If, &json!({}), &PortValues::new(), &test_ctx()).unwrap_err();
        assert!(err.contains("missing required input 'condition'"), "got: {err}");
    }

    #[test]
    fn wait_is_pure_control_flow_no_data_outputs() {
        let out = run_node(NodeKind::Wait, &json!({ "seconds": "0" }), &PortValues::new(), &test_ctx())
            .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn format_substitutes_present_inputs_only() {
        let cfg = json!({ "template": "{p1} — {p2}" });
        let mut inputs = PortValues::new();
        inputs.insert("p1".into(), PortValue::Text("door".into()));
        let out = run_node(NodeKind::Format, &cfg, &inputs, &test_ctx()).unwrap();
        assert_eq!(out["text"].as_text(), Some("door — "));
    }

    #[test]
    fn create_alert_requires_message_before_any_io() {
        let ctx = test_ctx();
        let cfg = NodeKind::CreateAlert.default_config();
        let err = run_node(NodeKind::CreateAlert, &cfg, &PortValues::new(), &ctx).unwrap_err();
        assert!(err.contains("missing required input 'message'"), "got: {err}");
        assert!(ctx.alerts.lock().unwrap().is_empty());
    }

    #[test]
    fn ask_ai_requires_a_question() {
        let err = run_node(NodeKind::AskAi, &json!({ "question": "" }), &PortValues::new(), &test_ctx())
            .unwrap_err();
        assert!(err.contains("question is required"), "got: {err}");
    }

    #[test]
    fn ask_ai_timeout_is_65s() {
        assert_eq!(node_timeout(NodeKind::AskAi, &json!({})), Some(std::time::Duration::from_secs(65)));
    }
}
