//! Running one node, and deciding how long it may take.
//!
//! The one place a kind becomes behaviour. Everything above it has been about shape and naming;
//! this is where a node does its work, sees only its own inputs, and gives back only its declared
//! outputs. It never sees the graph or another node — that is the isolation the whole layer
//! exists for.
//!
//! Timeouts are per kind and per configuration, because "too long" means something different for
//! a comparison and for a model answering a question.

use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::ToolContext;
use crate::workflows::domain::types;
use serde_json::Value;
use std::collections::HashMap;

use super::*;

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
        NodeKind::GetTrackedObjects => camera::run_get_tracked_objects(inputs, sys),
        NodeKind::GetCurrentFrame => camera::run_get_current_frame(config, inputs, sys),
        NodeKind::GetCameraByName => camera::run_get_camera_by_name(config, sys),
        NodeKind::BreakCamera => camera::run_break_camera(inputs),
        NodeKind::Region => camera::run_region(config, inputs, sys),
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
        NodeKind::BreakTrackedObject => vision::run_break_tracked_object(inputs),
        NodeKind::BreakDetection => vision::run_break_detection(inputs),
        NodeKind::BreakFaceDetection => vision::run_break_face_detection(inputs),
        NodeKind::RecognizePerson => vision::run_recognize_person(config, inputs, sys),
        NodeKind::FaceEmbedding => vision::run_face_embedding(config, inputs, sys),
        NodeKind::RecognizeFace => vision::run_recognize_face(config, inputs, sys),
        NodeKind::StoreFace => vision::run_store_face(config, inputs, sys),
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
        // Display annotation nodes never execute — pure canvas decoration.
        NodeKind::Title | NodeKind::Paragraph | NodeKind::Markdown => Ok(PortValues::new()),
        // Display live sinks — the render side-effect (persist + SSE) is engine-driven; the node
        // itself returns nothing, exec passes through (like ShowImage).
        NodeKind::Text | NodeKind::Gauge => Ok(PortValues::new()),
        NodeKind::StartRecording => recording::run_start_recording(config, inputs, sys),
        NodeKind::StopRecording => recording::run_stop_recording(inputs, sys),
        // TimeGate is engine-driven (reads/writes cross-run state, polls until expiry).
        // Never called through run_node — the engine handles it in forward_epoch.
        NodeKind::TimeGate => Ok(PortValues::new()),
        // DelayTrigger is engine-driven too (arms/extends cross-run state and schedules its
        // own trailing stop). Never called through run_node.
        NodeKind::DelayTrigger => Ok(PortValues::new()),
        // DeviceGate is engine-driven for the same reason, one step further: it mints a request
        // row, pushes it, and takes NO branch — the run ends there. The answer (or the TTL) comes
        // back later as a sub-run forced at this node, which is when the arm is chosen. Never
        // called through run_node.
        NodeKind::DeviceGate => Ok(PortValues::new()),
        NodeKind::GetVariable => variable::run_get_variable(config, sys),
        NodeKind::SetVariable => variable::run_set_variable(config, inputs, sys),
        NodeKind::MultiSetVariable => variable::run_multi_set_variables(config, inputs, sys),
        NodeKind::MultiGetVariable => variable::run_multi_get_variables(config, sys),
        NodeKind::ReplyToSender => telegram::run_reply_to_sender(config, inputs, sys),
        NodeKind::AiAnswer => ai::run_ai_answer(config, inputs, sys),
        NodeKind::ReplyWithRecording => telegram::run_reply_with_recording(config, inputs, sys),
        // AiSwitch computes the chosen arm here; the engine reads o["arm"] to fire it (like AskAi).
        NodeKind::AiSwitch => ai::run_ai_switch(config, inputs, sys),
        NodeKind::AiExtract => ai::run_ai_extract(config, inputs, sys),
        // Dictionaries (#113): read nodes work off the resolved `table` snapshot; mutate nodes
        // act on the live DB row under a lock (thread-safe across concurrent runs).
        NodeKind::DictRowCount => dictionary::run_row_count(inputs),
        NodeKind::DictGetRow => dictionary::run_get_row(inputs),
        NodeKind::DictFindRow => dictionary::run_find_row(inputs),
        NodeKind::DictGetCell => dictionary::run_get_cell(inputs),
        NodeKind::DictAddRow => dictionary::run_add_row(inputs, sys),
        NodeKind::DictSetCell => dictionary::run_set_cell(inputs, sys),
        NodeKind::DictUpdateRow => dictionary::run_update_row(inputs, sys),
        NodeKind::DictDeleteRow => dictionary::run_delete_row(inputs, sys),
        NodeKind::DictClear => dictionary::run_clear(inputs, sys),
        // Build nodes: pure, compose a table in-graph (no DB).
        NodeKind::DictSchema => dictionary::run_schema(config),
        NodeKind::DictMakeRow => dictionary::run_make_row(config, inputs),
        NodeKind::DictAppendRow => dictionary::run_append_row(inputs),
        NodeKind::TuyaControl => tuya::run_tuya_control(config, inputs, sys),
        NodeKind::TuyaReadStatus => tuya::run_tuya_read_status(config, sys),
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
        NodeKind::CameraSnapshot | NodeKind::GetCurrentFrame | NodeKind::GetTrackedObjects => 10,
        NodeKind::AskAi => 65,
        NodeKind::PersonDetection | NodeKind::ObjectDetection | NodeKind::MotionDetection => 30,
        NodeKind::MotionWatch => 20, // two camera grabs + the sample gap
        NodeKind::RecognizePerson => 30,
        NodeKind::FaceEmbedding => 30,
        NodeKind::RecognizeFace | NodeKind::StoreFace => 15,
        NodeKind::DescribeImage => 65,
        NodeKind::HttpPost | NodeKind::NotifyRecipient | NodeKind::DeliverAlert => 35,
        NodeKind::CreateAlert | NodeKind::RecordPresence => 15,
        NodeKind::StartRecording | NodeKind::StopRecording => 30,
        // Tuya Cloud round trips (token + call); the client's ureq agent caps at 12s.
        NodeKind::TuyaControl | NodeKind::TuyaReadStatus => 15,
        NodeKind::ReplyToSender => 35,
        NodeKind::AiAnswer => 30,
        NodeKind::AiSwitch => 30,
        NodeKind::AiExtract => 30,
        NodeKind::ReplyWithRecording => 120, // fetch clip from edge + upload
        // Dictionary mutators do a row-locked DB write.
        NodeKind::DictAddRow
        | NodeKind::DictSetCell
        | NodeKind::DictUpdateRow
        | NodeKind::DictDeleteRow
        | NodeKind::DictClear => 15,
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
        | NodeKind::BreakTrackedObject
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
        | NodeKind::Region
        | NodeKind::Comment
        | NodeKind::Reroute
        | NodeKind::Title
        | NodeKind::Paragraph
        | NodeKind::Markdown
        | NodeKind::Text
        | NodeKind::Gauge
        | NodeKind::TimeGate
        | NodeKind::DelayTrigger
        // No watchdog, and NOT because it is instant: the wait for a person is a row in Postgres,
        // not a held handler, so there is nothing here for a timeout to kill. The human deadline
        // is the request's own TTL, enforced by the expiry sweep.
        | NodeKind::DeviceGate
        | NodeKind::GetVariable
        | NodeKind::SetVariable
        | NodeKind::MultiSetVariable
        | NodeKind::MultiGetVariable
        | NodeKind::DictRowCount
        | NodeKind::DictGetRow
        | NodeKind::DictFindRow
        | NodeKind::DictGetCell
        | NodeKind::DictSchema
        | NodeKind::DictMakeRow
        | NodeKind::DictAppendRow => return None,
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
    config.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
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
