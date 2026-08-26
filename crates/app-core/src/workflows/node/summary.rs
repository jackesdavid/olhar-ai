//! One line saying what a node just did.
//!
//! This is what a person reads in the Console when a graph misbehaves, so it is written for them
//! and not for a log parser: the values that mattered, in the order somebody would ask about them.
//!
//! Long because it is one arm per kind, and each arm knows which of that node's inputs and outputs
//! are the interesting ones. Generic output — "ran ok" — would make the file short and the Console
//! useless.

use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::ToolContext;
use crate::workflows::domain::types;
use serde_json::Value;
use std::collections::HashMap;

use super::*;

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
        NodeKind::CreateAlert => txt("result")
            .map(clip)
            .unwrap_or_else(|| "alert created".into()),
        NodeKind::DeliverAlert => txt("result")
            .map(clip)
            .unwrap_or_else(|| "delivered".into()),
        NodeKind::CameraSnapshot => txt("result")
            .map(clip)
            .unwrap_or_else(|| "snapshot captured".into()),
        NodeKind::GetTrackedObjects => format!("{} tracked object(s)", num("count").unwrap_or(0)),
        NodeKind::BreakTrackedObject => {
            let cls = txt("class").unwrap_or("?");
            let zone = txt("zone").filter(|z| !z.is_empty());
            match zone {
                Some(z) => format!("break {cls} (in zone '{z}')"),
                None => format!("break {cls}"),
            }
        }
        NodeKind::GetCurrentFrame => {
            // The image is this node's OUTPUT. Its only INPUT is `camera` (GET_CURRENT_FRAME_IN),
            // so reading `inputs["image"]` made the size structurally always "0 KB" — the run log
            // reported an empty frame on every successful grab. `age_ms` was right all along:
            // it is a real output port where 0 legitimately means a fresh capture.
            let kb = outputs
                .get("image")
                .and_then(PortValue::as_image)
                .map_or(0, <[u8]>::len)
                / 1024;
            format!(
                "current frame ({} KB, {} ms old)",
                kb,
                num("age_ms").unwrap_or(0)
            )
        }
        NodeKind::DescribeImage => match txt("description") {
            Some(d) => clip(d),
            None => "described image".into(),
        },
        NodeKind::NotifyRecipient => txt("result")
            .map(clip)
            .unwrap_or_else(|| "notification sent".into()),
        NodeKind::Format => format!("→ {}", clip(txt("text").unwrap_or(""))),
        NodeKind::Wait => "delay complete".into(),
        NodeKind::HttpPost => format!(
            "HTTP {} {}",
            num("status").unwrap_or(0),
            if flag("ok") { "ok" } else { "failed" }
        ),
        NodeKind::If => {
            let cond = inputs
                .get("condition")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            format!(
                "condition {cond} → '{}' branch",
                if cond { "true" } else { "false" }
            )
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
                .and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                })
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
            config
                .get("camera")
                .and_then(|v| v.as_str())
                .unwrap_or("first"),
            num("score").unwrap_or(0),
            if flag("moved") { "moved" } else { "still" }
        ),
        NodeKind::PersonDetection => match num("count").unwrap_or(0) {
            0 => "no person detected".into(),
            c => format!(
                "{c} person(s) ({}% confidence)",
                num("confidence").unwrap_or(0)
            ),
        },
        NodeKind::ObjectDetection => {
            let class = config
                .get("class")
                .and_then(|v| v.as_str())
                .unwrap_or("person");
            match num("count").unwrap_or(0) {
                0 => format!("no {class} detected"),
                c => format!(
                    "{c} {class}(s) ({}% confidence)",
                    num("confidence").unwrap_or(0)
                ),
            }
        }
        NodeKind::BreakPersonDetection
        | NodeKind::BreakDetection
        | NodeKind::BreakFaceDetection => {
            format!("bbox + {}% confidence", num("confidence").unwrap_or(0))
        }
        NodeKind::Print => {
            let msg = inputs
                .get("message")
                .and_then(PortValue::as_text)
                .unwrap_or("");
            format!("print: {}", clip(msg))
        }
        NodeKind::RecognizePerson => {
            if flag("ok") {
                format!(
                    "recognised {} ({}%)",
                    txt("person").unwrap_or("?"),
                    num("confidence").unwrap_or(0)
                )
            } else {
                "no match".into()
            }
        }
        NodeKind::FaceEmbedding => {
            if flag("ok") {
                format!("embedded ({}-d)", num("dim").unwrap_or(0))
            } else {
                "no face".into()
            }
        }
        NodeKind::RecognizeFace => {
            if flag("found") {
                let who = txt("person").filter(|s| !s.is_empty()).unwrap_or("unknown");
                format!("matched {} ({}%)", who, num("confidence").unwrap_or(0))
            } else {
                "no match".into()
            }
        }
        NodeKind::StoreFace => {
            if flag("stored") {
                "face stored".into()
            } else {
                "skipped (duplicate)".into()
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
            let bytes = inputs
                .get("image")
                .and_then(PortValue::as_image)
                .map_or(0, <[u8]>::len);
            if bytes > 0 {
                format!("image shown ({} KB)", bytes / 1024)
            } else {
                "no image".into()
            }
        }
        NodeKind::FlipFlop => {
            let branch = outputs
                .get("is_a")
                .and_then(PortValue::as_bool)
                .map(|b| if b { "a" } else { "b" })
                .unwrap_or("?");
            format!("flip flop → {branch}")
        }
        NodeKind::Compare => {
            let a = inputs.get("a").and_then(PortValue::as_num).unwrap_or(0);
            let b = inputs.get("b").and_then(PortValue::as_num).unwrap_or(0);
            let op = config
                .get("operator")
                .and_then(|v| v.as_str())
                .unwrap_or("==");
            let result = outputs
                .get("result")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
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
            let text = outputs
                .get("text")
                .and_then(PortValue::as_text)
                .unwrap_or("");
            format!("{v} → \"{text}\"")
        }
        NodeKind::BoolToText => {
            let v = inputs
                .get("value")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            let text = outputs
                .get("text")
                .and_then(PortValue::as_text)
                .unwrap_or("");
            format!("{v} → \"{text}\"")
        }
        NodeKind::Cooldown => {
            let elapsed = outputs
                .get("elapsed_secs")
                .and_then(PortValue::as_num)
                .unwrap_or(0);
            let ok = outputs
                .get("ok")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            let window = config
                .get("window_secs")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                })
                .unwrap_or(60);
            format!(
                "{elapsed}s elapsed / {window}s → {}",
                if ok { "pass" } else { "blocked" }
            )
        }
        NodeKind::DrawBbox => {
            let bytes = outputs
                .get("image")
                .and_then(PortValue::as_image)
                .map_or(0, <[u8]>::len);
            format!("bbox drawn ({} KB)", bytes / 1024)
        }
        NodeKind::DrawText => {
            let t = inputs
                .get("text")
                .and_then(PortValue::as_text)
                .or_else(|| config.get("text").and_then(|v| v.as_str()))
                .unwrap_or("");
            format!("text drawn: {}", clip(t))
        }
        NodeKind::BlurOutsideBbox => {
            let bytes = outputs
                .get("image")
                .and_then(PortValue::as_image)
                .map_or(0, <[u8]>::len);
            format!("blurred outside bbox ({} KB)", bytes / 1024)
        }
        NodeKind::GetImageSize => {
            let w = outputs
                .get("width")
                .and_then(PortValue::as_num)
                .unwrap_or(0);
            let h = outputs
                .get("height")
                .and_then(PortValue::as_num)
                .unwrap_or(0);
            format!("{w}×{h}")
        }
        NodeKind::And => {
            let a = inputs
                .get("a")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            let b = inputs
                .get("b")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            let result = outputs
                .get("result")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            format!("{a} AND {b} → {result}")
        }
        NodeKind::Or => {
            let a = inputs
                .get("a")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            let b = inputs
                .get("b")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            let result = outputs
                .get("result")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            format!("{a} OR {b} → {result}")
        }
        NodeKind::Not => {
            let a = inputs
                .get("a")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            let result = outputs
                .get("result")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            format!("NOT {a} → {result}")
        }
        NodeKind::TextCompare => {
            let a = inputs.get("a").and_then(PortValue::as_text).unwrap_or("");
            let b = inputs
                .get("b")
                .and_then(PortValue::as_text)
                .or_else(|| config.get("b").and_then(|v| v.as_str()))
                .unwrap_or("");
            let op = config
                .get("operator")
                .and_then(|v| v.as_str())
                .unwrap_or("eq");
            let result = outputs
                .get("result")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            let clip = |s: &str| {
                if s.chars().count() > 20 {
                    format!("{}…", s.chars().take(19).collect::<String>())
                } else {
                    s.to_string()
                }
            };
            format!("\"{}\" {op} \"{}\" → {result}", clip(a), clip(b))
        }
        NodeKind::StringSwitch => {
            let val = inputs
                .get("value")
                .and_then(PortValue::as_text)
                .unwrap_or("");
            let arms = super::domain::string_switch_arms(config);
            let fired = if arms.iter().any(|a| a == val) {
                val
            } else {
                "default"
            };
            format!("\"{}\" → arm '{fired}'", clip(val))
        }
        NodeKind::CameraSwitch => {
            let cam = inputs.get("camera").and_then(PortValue::as_camera);
            if let Some(cam) = cam {
                let arms = super::domain::camera_switch_arms(config);
                let idx = arms
                    .iter()
                    .position(|a| !a.is_empty() && (a == &cam.ip || a == &cam.id));
                let fired = idx
                    .map(|i| format!("arm_{}", i + 1))
                    .unwrap_or_else(|| "default".to_string());
                format!("{} → {fired}", cam.name)
            } else {
                "no camera → default".to_string()
            }
        }
        NodeKind::AskAi => {
            let q = inputs
                .get("question")
                .and_then(PortValue::as_text)
                .or_else(|| config.get("question").and_then(|v| v.as_str()))
                .unwrap_or("");
            let answer = outputs
                .get("result")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            format!(
                "AI: \"{}\" → {}",
                clip(q),
                if answer { "true" } else { "false" }
            )
        }
        NodeKind::GetCameraByName => {
            let name = outputs
                .get("camera")
                .and_then(PortValue::as_camera)
                .map(|c| c.name.as_str())
                .unwrap_or("?");
            format!("camera: {name}")
        }
        NodeKind::BreakCamera => {
            let name = inputs
                .get("camera")
                .and_then(PortValue::as_camera)
                .map(|c| c.name.as_str())
                .unwrap_or("?");
            format!("break '{name}' → id/ip/name")
        }
        NodeKind::Region => match outputs.get("region").and_then(PortValue::as_region) {
            Some(r) => format!(
                "region '{}' ({} point{})",
                r.name,
                r.points.len(),
                if r.points.len() == 1 { "" } else { "s" }
            ),
            None => "no region drawn".into(),
        },
        NodeKind::Comment => "comment".into(),
        NodeKind::Reroute => "reroute".into(),
        NodeKind::Title
        | NodeKind::Paragraph
        | NodeKind::Markdown
        | NodeKind::Text
        | NodeKind::Gauge => "display".into(),
        NodeKind::StartRecording => {
            let new = outputs
                .get("recording_new")
                .and_then(|v| {
                    if let PortValue::Bool(b) = v {
                        Some(*b)
                    } else {
                        None
                    }
                })
                .unwrap_or(true);
            txt("clip_id")
                .map(|id| {
                    if new {
                        format!("recording started: {id}")
                    } else {
                        format!("recording resumed: {id}")
                    }
                })
                .unwrap_or_else(|| "recording started".into())
        }
        NodeKind::StopRecording => "recording stopped".into(),
        NodeKind::TimeGate => "time gate".into(),
        // The engine logs the real state transition (armed / extended / stopped) — this is
        // only the fallback for a node that somehow reached the generic summary path.
        NodeKind::DelayTrigger => "delay trigger".into(),
        // The engine logs the real outcome (asked / approved / denied / expired). This is only the
        // fallback for a node that somehow reached the generic summary path.
        NodeKind::DeviceGate => "device gate".into(),
        NodeKind::GetVariable => {
            let name = config
                .get("variable")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("get ${name}")
        }
        NodeKind::SetVariable => {
            let name = config
                .get("variable")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("set ${name}")
        }
        // A node that touches eight variables has to say WHICH in the run log, or the log is less
        // informative than the column of single nodes this replaces. Named up to four, then a
        // count — the point is to recognise the node, not to reprint its config.
        NodeKind::MultiSetVariable | NodeKind::MultiGetVariable => {
            let verb = if kind == NodeKind::MultiSetVariable {
                "set"
            } else {
                "get"
            };
            let names = variable::multi_var_names(config);
            let shown = names
                .iter()
                .take(4)
                .map(|n| format!("${n}"))
                .collect::<Vec<_>>()
                .join(", ");
            match (names.len(), names.len().saturating_sub(4)) {
                (0, _) => format!("{verb} — no variables"),
                (_, 0) => format!("{verb} {shown}"),
                (_, more) => format!("{verb} {shown} (+{more})"),
            }
        }
        NodeKind::ReplyToSender => {
            let chat = num("chat_id")
                .or_else(|| inputs.get("chat_id").and_then(PortValue::as_num))
                .unwrap_or(0);
            txt("result")
                .map(clip)
                .unwrap_or_else(|| format!("reply → chat {chat}"))
        }
        NodeKind::AiAnswer => txt("response")
            .map(clip)
            .unwrap_or_else(|| "AI answered".into()),
        NodeKind::ReplyWithRecording => {
            let chat = inputs
                .get("chat_id")
                .and_then(PortValue::as_num)
                .unwrap_or(0);
            txt("result")
                .map(clip)
                .unwrap_or_else(|| format!("clip → chat {chat}"))
        }
        NodeKind::AiSwitch => format!("→ {}", txt("arm").unwrap_or("default")),
        NodeKind::AiExtract => {
            let found = outputs
                .get("found")
                .and_then(PortValue::as_bool)
                .unwrap_or(false);
            match txt("value") {
                Some(v) => format!("extracted: {}", clip(v)),
                None => {
                    if found {
                        "extracted".into()
                    } else {
                        "not found".into()
                    }
                }
            }
        }
        NodeKind::DictRowCount => format!("{} row(s)", num("count").unwrap_or(0)),
        NodeKind::DictGetRow | NodeKind::DictFindRow => {
            if flag("found") {
                "row found".into()
            } else {
                "no matching row".into()
            }
        }
        NodeKind::DictGetCell => match txt("value") {
            Some(v) if flag("found") => format!("cell: {}", clip(v)),
            _ => "cell not found".into(),
        },
        NodeKind::DictAddRow => "row added".into(),
        NodeKind::DictSetCell => "cell set".into(),
        NodeKind::DictUpdateRow => "row updated".into(),
        NodeKind::DictDeleteRow => "row deleted".into(),
        NodeKind::DictClear => "dictionary cleared".into(),
        NodeKind::DictSchema => "schema built".into(),
        NodeKind::DictMakeRow => "row built".into(),
        NodeKind::DictAppendRow => "row appended".into(),
        NodeKind::TuyaControl => {
            let code = config
                .get("tuya_code")
                .and_then(Value::as_str)
                .unwrap_or("?");
            if flag("ok") {
                format!("Tuya {code} set")
            } else {
                format!("Tuya {code} failed")
            }
        }
        NodeKind::TuyaReadStatus => match txt("value") {
            Some(v) if !v.is_empty() => format!("Tuya status: {}", clip(v)),
            _ => "Tuya status read".into(),
        },
    }
}

fn num_cmp_summary(op: &str, inputs: &PortValues, outputs: &PortValues) -> String {
    let a = inputs.get("a").and_then(PortValue::as_num).unwrap_or(0);
    let b = inputs.get("b").and_then(PortValue::as_num).unwrap_or(0);
    let result = outputs
        .get("result")
        .and_then(PortValue::as_bool)
        .unwrap_or(false);
    format!("{a} {op} {b} → {result}")
}

pub(crate) fn req_text<'a>(inputs: &'a PortValues, name: &str) -> Result<&'a str, String> {
    inputs
        .get(name)
        .and_then(PortValue::as_text)
        .ok_or_else(|| format!("missing required input '{name}'"))
}

/// Decode just the JPEG/PNG header to extract image dimensions. Returns (0, 0) on error
/// so callers can treat unknown dimensions as absent without failing.
pub(crate) fn jpeg_dimensions(bytes: &[u8]) -> (i64, i64) {
    match image::load_from_memory(bytes) {
        Ok(img) => (img.width() as i64, img.height() as i64),
        Err(_) => (0, 0),
    }
}

pub(crate) fn to_jpeg(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode snapshot: {e}"))?;
    let mut out = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Jpeg,
    )
    .map_err(|e| format!("encode snapshot: {e}"))?;
    Ok(out)
}
