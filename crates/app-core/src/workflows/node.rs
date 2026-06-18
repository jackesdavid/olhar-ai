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
use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use crate::alerts::{self, AlertLevel};
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
        // Arrays / detection elements / the Any wildcard are produced by upstream nodes
        // (or, for ForEach `items`, parsed from text in the engine) — never a config literal.
        PortType::TextArray
        | PortType::NumArray
        | PortType::PersonArray
        | PortType::FaceArray
        | PortType::Person
        | PortType::Face
        | PortType::Any => None,
    }
}

/// Execute one node against its resolved inputs. Returns its declared outputs.
pub fn run_node(
    kind: NodeKind,
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    match kind {
        NodeKind::GetLastAlert => run_get_last_alert(sys),
        NodeKind::GetRecipient => run_get_recipient(config),
        NodeKind::CreateAlert => run_create_alert(config, inputs, sys),
        NodeKind::DeliverAlert => run_deliver_alert(inputs, sys),
        NodeKind::CameraSnapshot => run_camera_snapshot(inputs, sys),
        NodeKind::GetCurrentFrame => run_get_current_frame(config, inputs, sys),
        NodeKind::DescribeImage => run_describe_image(config, inputs, sys),
        NodeKind::NotifyRecipient => run_notify_recipient(inputs, sys),
        NodeKind::Format => run_format(config, inputs),
        NodeKind::Wait => run_wait(config),
        NodeKind::HttpPost => run_http_post(config, inputs),
        NodeKind::If => run_if(inputs),
        // ForEach is engine-driven (like If/EmitEvent): the scheduler parses `items`,
        // advances the index across epochs, sets item/index, and fires loop_body/completed.
        NodeKind::ForEach => Ok(PortValues::new()),
        NodeKind::MotionDetection => run_motion_detection(config, inputs),
        NodeKind::PersonDetection => run_person_detection(inputs, sys),
        NodeKind::BreakPersonDetection => run_break_person_detection(inputs),
        NodeKind::BreakFaceDetection => run_break_face_detection(inputs),
        NodeKind::RecognizePerson => run_recognize_person(config, inputs, sys),
        NodeKind::RecordPresence => run_record_presence(config, inputs, sys),
        // EmitEvent/OnEvent are exec-flow markers: the engine handles the event fire
        // (spawning the guarded sub-run) and the OnEvent entry, like the If/ShowImage
        // special-cases. Here they're no-ops that just pass exec through.
        NodeKind::EmitEvent | NodeKind::OnEvent => Ok(PortValues::new()),
        NodeKind::Print => run_print(config, inputs, sys),
        // ShowImage is a debug sink whose only effect — mirroring its input frame to
        // the editor — is driven by the engine, which owns the node id the UI needs to
        // place the preview. Here it's a pure pass-through (exec flows on, no data out).
        NodeKind::ShowImage => Ok(PortValues::new()),
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
        NodeKind::PersonDetection | NodeKind::MotionDetection => 30,
        NodeKind::RecognizePerson => 30,
        NodeKind::DescribeImage => 65,
        NodeKind::HttpPost | NodeKind::NotifyRecipient | NodeKind::DeliverAlert => 35,
        NodeKind::CreateAlert | NodeKind::RecordPresence => 15,
        // Inline (no watchdog): instant/pure nodes + the deliberate `Wait` sleep + the
        // engine-driven markers (ForEach/EmitEvent/OnEvent/ShowImage).
        NodeKind::Wait
        | NodeKind::Format
        | NodeKind::If
        | NodeKind::Print
        | NodeKind::GetRecipient
        | NodeKind::GetLastAlert
        | NodeKind::BreakPersonDetection
        | NodeKind::BreakFaceDetection
        | NodeKind::EmitEvent
        | NodeKind::OnEvent
        | NodeKind::ShowImage
        | NodeKind::ForEach => return None,
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

/// Debug sink: echo `message` to the editor Console (always, regardless of the graph's
/// debug flag) and — unless `on_screen` is off — flash it over the canvas. Carries the
/// `level` so the UI can tint both. Returns no data; exec passes through.
fn run_print(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let message = inputs.get("message").and_then(PortValue::as_text).unwrap_or("").to_string();
    let on_screen = inputs.get("on_screen").and_then(PortValue::as_bool).unwrap_or(true);
    let level = config.get("level").and_then(Value::as_str).unwrap_or("info").to_string();
    let _ = sys.ui_tx.send(ToolUiEvent::Print { message, level, on_screen });
    Ok(PortValues::new())
}

/// Detect whether a person is present in the image with the local YOLOX-Nano model.
/// Outputs `person` (Bool), `count` (Num), `confidence` (Num %, 0..100), `ok`.
fn run_person_detection(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    // Per-person boxes → a `PersonDetection[]` (the `persons` output), with the scalar
    // summary derived from it (count, top confidence) for convenience / back-compat.
    let people = sys.vision.detect_persons_detailed(bytes)?;
    let count = people.len() as i64;
    let confidence = people.iter().map(|p| p.confidence).fold(0.0_f32, f32::max);
    let persons: Vec<PortValue> = people.into_iter().map(PortValue::Person).collect();
    let mut o = PortValues::new();
    o.insert("count".into(), PortValue::Num(count));
    o.insert("confidence".into(), PortValue::Num((confidence * 100.0).round() as i64));
    o.insert("persons".into(), PortValue::List(persons));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Break a `PersonDetection` element into its fields: `bbox` (a `[x1,y1,x2,y2]` NumArray)
/// and `confidence` (percent 0..100). Like Unreal's "Break" struct node.
fn run_break_person_detection(inputs: &PortValues) -> Result<PortValues, String> {
    let p = inputs
        .get("person")
        .and_then(PortValue::as_person)
        .ok_or("missing required input 'person'")?;
    let bbox = p.bbox.iter().map(|v| PortValue::Num(v.round() as i64)).collect();
    let mut o = PortValues::new();
    o.insert("bbox".into(), PortValue::List(bbox));
    o.insert("confidence".into(), PortValue::Num((p.confidence * 100.0).round() as i64));
    Ok(o)
}

/// Break a `FaceDetection` element into `bbox` (NumArray), `confidence` (percent), and
/// `landmarks` (count of the 5-point landmark set).
fn run_break_face_detection(inputs: &PortValues) -> Result<PortValues, String> {
    let f = inputs
        .get("face")
        .and_then(PortValue::as_face)
        .ok_or("missing required input 'face'")?;
    let bbox = f.bbox.iter().map(|v| PortValue::Num(v.round() as i64)).collect();
    let mut o = PortValues::new();
    o.insert("bbox".into(), PortValue::List(bbox));
    o.insert("confidence".into(), PortValue::Num((f.confidence * 100.0).round() as i64));
    o.insert("landmarks".into(), PortValue::Num(f.landmarks.len() as i64));
    Ok(o)
}

/// Recognise a face against the enrolled people: embed the frame, run a cosine kNN
/// over approved embeddings, and pick the best person within `max_distance`. Outputs
/// `person` (name), `person_id`, `confidence` (%), and `ok` (whether a match was found).
fn run_recognize_person(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let max_distance = config
        .get("max_distance")
        .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(0.4) as f32;
    let embedding = crate::agent::face::embed(bytes)?;
    let matches = sys.recognizer.recognize(&embedding, 5);
    let mut o = PortValues::new();
    match crate::persons::best_match(&matches, max_distance) {
        Some(m) => {
            o.insert("person".into(), PortValue::Text(m.name));
            o.insert("person_id".into(), PortValue::Text(m.person_id));
            o.insert("confidence".into(), PortValue::Num(m.confidence));
            o.insert("ok".into(), PortValue::Bool(true));
        }
        None => {
            o.insert("confidence".into(), PortValue::Num(0));
            o.insert("ok".into(), PortValue::Bool(false));
        }
    }
    Ok(o)
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
                None => format!("item[{i}]"), // non-text element (num / detection)
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
        NodeKind::PersonDetection => match num("count").unwrap_or(0) {
            0 => "no person detected".into(),
            c => format!("{c} person(s) ({}% confidence)", num("confidence").unwrap_or(0)),
        },
        NodeKind::BreakPersonDetection | NodeKind::BreakFaceDetection => {
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
    }
}

/// Control-flow branch: just validates the `condition` Bool is present. The engine
/// reads the resolved condition to choose the live `true`/`false` exec branch.
fn run_if(inputs: &PortValues) -> Result<PortValues, String> {
    inputs
        .get("condition")
        .and_then(PortValue::as_bool)
        .ok_or("missing required input 'condition'")?;
    Ok(PortValues::new())
}

/// Compare two frames (resized to a common small grayscale) and report how much
/// they differ. `moved` = changed% >= `sensitivity`; `score` = changed% (0..100).
fn run_motion_detection(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let a = inputs.get("image_a").and_then(PortValue::as_image).ok_or("missing required input 'image_a'")?;
    let b = inputs.get("image_b").and_then(PortValue::as_image).ok_or("missing required input 'image_b'")?;
    let cfg_num = |k: &str, d: i64| -> i64 {
        config.get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(d)
    };
    let pixel_threshold = cfg_num("pixel_threshold", 25).clamp(0, 255) as i16;
    let sensitivity = cfg_num("sensitivity", 2).clamp(0, 100) as f64;

    const W: u32 = 320;
    const H: u32 = 240;
    let gray = |raw: &[u8]| -> Result<Vec<u8>, String> {
        let img = image::load_from_memory(raw).map_err(|e| format!("Could not decode frame: {e}"))?;
        Ok(img.resize_exact(W, H, image::imageops::FilterType::Triangle).to_luma8().into_raw())
    };
    let ga = gray(a)?;
    let gb = gray(b)?;
    let total = ga.len().max(1);
    let changed = ga
        .iter()
        .zip(gb.iter())
        .filter(|(x, y)| (**x as i16 - **y as i16).abs() > pixel_threshold)
        .count();
    let score = (changed as f64 / total as f64) * 100.0;

    let mut o = PortValues::new();
    o.insert("moved".into(), PortValue::Bool(score >= sensitivity));
    o.insert("score".into(), PortValue::Num(score.round() as i64));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// POST form data to `url`. `fields` (a string map) are the form fields and
/// `headers` (a string map) are extra request headers. If a `file` is provided
/// the request is sent as `multipart/form-data` (fields as text parts + the file);
/// otherwise as `application/x-www-form-urlencoded`. Outputs status, body, ok.
fn run_http_post(_config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let url = req_text(inputs, "url")?.to_string();
    let headers = inputs.get("headers").and_then(PortValue::as_dict).unwrap_or(&[]);
    let fields = inputs.get("fields").and_then(PortValue::as_dict).unwrap_or(&[]);
    let file = inputs.get("file").and_then(PortValue::as_file);

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let mut req = agent.post(&url);
    for (k, v) in headers {
        req = req.set(k, v);
    }

    let result = if let Some(f) = file {
        let boundary = "----sentinelFormBoundary7MA4YWxkTrZu0gW";
        let body = multipart_body(fields, f, boundary);
        req.set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
            .send_bytes(&body)
    } else {
        let form: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        req.send_form(&form)
    };

    let (status, body, ok) = match result {
        Ok(resp) => {
            let s = resp.status();
            (s as i64, resp.into_string().unwrap_or_default(), (200..300).contains(&s))
        }
        // ureq treats 4xx/5xx as an error but still carries the response.
        Err(ureq::Error::Status(code, resp)) => {
            (code as i64, resp.into_string().unwrap_or_default(), false)
        }
        Err(e) => return Err(format!("HTTP POST to {url} failed: {e}")),
    };

    let mut o = PortValues::new();
    o.insert("status".into(), PortValue::Num(status));
    o.insert("response".into(), PortValue::Text(body));
    o.insert("ok".into(), PortValue::Bool(ok));
    Ok(o)
}

/// Build a `multipart/form-data` body: each field as a text part, then the file.
fn multipart_body(fields: &[(String, String)], file: &FileVal, boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(file.bytes.len() + 1024);
    for (k, v) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{k}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(v.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            file.name
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", file.mime).as_bytes());
    body.extend_from_slice(&file.bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

/// Delay the run by `seconds` (clamped 0..=3600). A pure control-flow node: it
/// has no data ports — its exec_out simply fires the next node after the delay.
/// The node stays active for the whole delay since the engine runs it synchronously.
fn run_wait(config: &Value) -> Result<PortValues, String> {
    let secs = config
        .get("seconds")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(1)
        .clamp(0, 3600);
    std::thread::sleep(std::time::Duration::from_secs(secs as u64));
    Ok(PortValues::new())
}

/// Output the recipient chosen in this node's config as a typed `Recipient`.
fn run_get_recipient(config: &Value) -> Result<PortValues, String> {
    let label = config.get("recipient").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let ok = !label.is_empty();
    let mut o = PortValues::new();
    o.insert("recipient".into(), PortValue::Recipient(label));
    o.insert("ok".into(), PortValue::Bool(ok));
    Ok(o)
}

fn req_text<'a>(inputs: &'a PortValues, name: &str) -> Result<&'a str, String> {
    inputs
        .get(name)
        .and_then(PortValue::as_text)
        .ok_or_else(|| format!("missing required input '{name}'"))
}

fn run_get_last_alert(sys: &ToolContext) -> Result<PortValues, String> {
    let mut o = PortValues::new();
    let alerts = sys.alerts.lock().unwrap();
    match alerts.last() {
        Some(a) => {
            o.insert("id".into(), PortValue::Num(a.id as i64));
            o.insert("level".into(), PortValue::Text(a.level.label().to_string()));
            o.insert("message".into(), PortValue::Text(a.message.clone()));
            o.insert("room".into(), PortValue::Text(a.room_name.clone().unwrap_or_default()));
            o.insert("source".into(), PortValue::Text(a.source.clone()));
            o.insert("created_at".into(), PortValue::Text(a.created_at.clone()));
            o.insert("ok".into(), PortValue::Bool(true));
        }
        None => {
            o.insert("ok".into(), PortValue::Bool(false));
        }
    }
    Ok(o)
}

fn run_create_alert(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let message = req_text(inputs, "message")?.to_string();
    let level = config
        .get("level")
        .and_then(Value::as_str)
        .and_then(AlertLevel::parse)
        .unwrap_or_default();
    let source = config
        .get("source")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("workflow")
        .to_string();
    let (room_id, room_name) = match inputs
        .get("room")
        .and_then(PortValue::as_text)
        .filter(|s| !s.trim().is_empty())
    {
        Some(name) => {
            let rooms = crate::map::repo::load(&sys.db, sys.tenant_id);
            match rooms.iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())) {
                Some(r) => (Some(r.id), Some(r.name.clone())),
                None => (None, Some(name.to_string())),
            }
        }
        None => (None, None),
    };
    let a = alerts::create(&sys.alerts, Some(&sys.db), sys.tenant_id, level, message, room_id, room_name, source);
    let _ = sys.ui_tx.send(ToolUiEvent::AlertsChanged);
    let mut o = PortValues::new();
    o.insert("id".into(), PortValue::Num(a.id as i64));
    o.insert(
        "result".into(),
        PortValue::Text(format!("Raised alert {} [{}]", a.id, a.level.label())),
    );
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Record a categorized presence in a room: persist it and pulse it live on the map
/// (via the `Presence` UI event → SSE relay). Inputs override config; the room name
/// resolves to a `room_id` like `CreateAlert`.
fn run_record_presence(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let cat_str = inputs
        .get("category")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("category").and_then(Value::as_str))
        .unwrap_or("person");
    let category = crate::presence::Category::parse(cat_str);

    let label = inputs
        .get("label")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("label").and_then(Value::as_str))
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty());

    let camera = inputs
        .get("camera")
        .and_then(PortValue::as_text)
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty());

    // How long the map keeps the pulse alive (seconds). Input overrides config; default 8.
    let duration_secs = inputs
        .get("duration_secs")
        .and_then(PortValue::as_num)
        .or_else(|| {
            config
                .get("duration_secs")
                .and_then(Value::as_str)
                .and_then(|s| s.trim().parse::<i64>().ok())
        })
        .filter(|n| *n > 0)
        .unwrap_or(8) as u32;

    let (room_id, room_name) = match inputs
        .get("room")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("room").and_then(Value::as_str))
        .filter(|s| !s.trim().is_empty())
    {
        Some(name) => {
            let rooms = crate::map::repo::load(&sys.db, sys.tenant_id);
            match rooms.iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())) {
                Some(r) => (Some(r.id), Some(r.name.clone())),
                None => (None, Some(name.to_string())),
            }
        }
        None => (None, None),
    };

    let p = crate::presence::create(
        &sys.db,
        sys.tenant_id,
        category,
        label.clone(),
        room_id,
        room_name.clone(),
        camera.clone(),
        duration_secs,
        "workflow".to_string(),
    );

    let _ = sys.ui_tx.send(ToolUiEvent::Presence {
        category: category.slug().to_string(),
        room_id,
        room_name,
        label,
        camera,
        duration_secs,
        created_at: p.created_at.clone(),
    });

    let mut o = PortValues::new();
    o.insert("id".into(), PortValue::Num(p.id as i64));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_deliver_alert(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let id = inputs
        .get("id")
        .and_then(PortValue::as_num)
        .ok_or("missing required input 'id'")? as u64;
    let cfg = sys
        .notif_cfg
        .as_ref()
        .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;
    let result = alerts::deliver(&sys.alerts, Some(&sys.db), sys.tenant_id, &sys.notif, cfg, id)?;
    let _ = sys.ui_tx.send(ToolUiEvent::AlertsChanged);
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_camera_snapshot(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let camera = req_text(inputs, "camera")?;
    let device = {
        let devices = sys.devices.lock().unwrap();
        let idx = dev_actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
        devices[idx].clone()
    };
    let bytes = sys.frames.snapshot(&device)?;
    // Mirror the chat tool: also surface the snapshot in the UI.
    if let Ok(jpeg) = to_jpeg(&bytes) {
        let _ = sys.ui_tx.send(ToolUiEvent::Snapshot { camera: device.name.clone(), jpeg });
    }
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(bytes));
    o.insert("result".into(), PortValue::Text(format!("Snapshot of '{}' captured", device.name)));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Fast frame source: return the camera's most-recent warm in-memory frame (no per-call
/// camera round trip), starting/keeping a background grabber. For high-rate loops like a
/// 1 Hz recognition graph. Falls back to an on-demand snapshot until the cache is warm.
fn run_get_current_frame(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let camera = req_text(inputs, "camera")?;
    let device = {
        let devices = sys.devices.lock().unwrap();
        let idx = dev_actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
        devices[idx].clone()
    };
    let max_age_ms = config
        .get("max_age_ms")
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(2000);
    let (bytes, age_ms) = sys.frames.current(&device, max_age_ms)?;
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(bytes));
    o.insert("age_ms".into(), PortValue::Num(age_ms as i64));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_describe_image(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let label = config
        .get("label")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("camera");
    let description = sys.vision.describe(bytes, label)?;
    let mut o = PortValues::new();
    o.insert("description".into(), PortValue::Text(description));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_notify_recipient(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let message = req_text(inputs, "message")?;
    // A wired Recipient picks the target; unwired → auto (the sole enabled one).
    let target = inputs
        .get("recipient")
        .and_then(PortValue::as_recipient)
        .filter(|s| !s.trim().is_empty());
    let image = inputs.get("image").and_then(PortValue::as_image).map(|b| b.to_vec());
    let cfg = sys
        .notif_cfg
        .as_ref()
        .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;
    let result = sys.notifier.dispatch(&sys.notif, cfg, target, message, image)?;
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Text(result));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

fn run_format(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let template = config.get("template").and_then(Value::as_str).unwrap_or("");
    let get = |k: &str| inputs.get(k).and_then(PortValue::as_text).unwrap_or("").to_string();
    let text = template
        .replace("{p1}", &get("p1"))
        .replace("{p2}", &get("p2"))
        .replace("{p3}", &get("p3"));
    let mut o = PortValues::new();
    o.insert("text".into(), PortValue::Text(text));
    Ok(o)
}

/// Re-encode snapshot bytes to JPEG for the UI event (same helper as the chat tool).
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
        // Pure/instant kinds + the deliberate `Wait` sleep + engine-driven markers ⇒ inline.
        for k in [NodeKind::Wait, NodeKind::Format, NodeKind::If, NodeKind::Print, NodeKind::ForEach, NodeKind::ShowImage] {
            assert_eq!(node_timeout(k, &json!({})), None, "{k:?} should run inline");
        }
        // IO-bound kinds ⇒ a positive per-kind deadline.
        assert_eq!(node_timeout(NodeKind::CameraSnapshot, &json!({})), Some(Duration::from_secs(10)));
        assert_eq!(node_timeout(NodeKind::DescribeImage, &json!({})), Some(Duration::from_secs(65)));
        assert_eq!(node_timeout(NodeKind::RecognizePerson, &json!({})), Some(Duration::from_secs(30)));
    }

    #[test]
    fn node_timeout_config_override_wins_and_zero_disables() {
        use std::time::Duration;
        // A per-node override (string or number) beats the default…
        assert_eq!(node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": "3" })), Some(Duration::from_secs(3)));
        assert_eq!(node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": 3 })), Some(Duration::from_secs(3)));
        // …and 0 disables the watchdog for that node.
        assert_eq!(node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": "0" })), None);
    }

    #[test]
    fn run_node_timed_aborts_a_slow_node_promptly() {
        use std::time::{Duration, Instant};
        // `Wait` sleeps for its configured 5s, but a 200ms deadline must cut it short.
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
        // A fast node finishes well within its deadline and returns its real output.
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
        assert!(literal_for(&cfg, empty).is_none()); // empty dictionary → not provided
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
        // The debug preview is emitted by the engine (it owns the node id); the node
        // itself is a pure exec pass-through that yields no data ports.
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
        // seconds=0 → no real delay; a control-flow node emits no data ports.
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
}
