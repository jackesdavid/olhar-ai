//! What each kind of node LOOKS like: its pins, its arms, and what it starts out configured as.
//!
//! Split from the catalog because these are different questions asked at different times. The
//! catalog answers "what is this node called" — needed to load a saved graph. This answers "what
//! can be wired to it" — needed to draw it and to refuse a bad connection.
//!
//! Two of these decide how the SCHEDULER treats a node rather than how it is drawn, and the
//! distinction between them is easy to get backwards:
//!
//! - `is_pure` — no exec pins at all. It runs whenever its inputs are ready, the way a pure
//!   function does, rather than when control reaches it.
//! - `is_pure_source` — pure AND has no inputs of its own. It therefore activates on every run and
//!   every epoch, including on branches nobody took, which is deliberate (a sub-run entering
//!   halfway still needs its variables seeded) and is why reporting one is deferred until
//!   something reads it (#115, #205).
//!
//! Ports declared here are the node's STATIC shape. Eight kinds derive their real ports from how
//! the user configured them — see [`NodeExt`](super::NodeExt), which is what the engine and the
//! editor both ask.

use super::*;
use serde_json::{json, Value};

impl NodeKind {
    /// Pure data/logic nodes have no exec pins — they activate whenever their upstream
    /// data is ready, like pure functions in Unreal Blueprint. Comparison operators,
    /// logical operators, and type-conversion nodes are all pure.
    pub fn is_pure(self) -> bool {
        matches!(
            self,
            NodeKind::GetCameraByName
                | NodeKind::BreakCamera
                | NodeKind::Region
                | NodeKind::Compare
                | NodeKind::NumLt
                | NodeKind::NumLte
                | NodeKind::NumGt
                | NodeKind::NumGte
                | NodeKind::NumEq
                | NodeKind::NumNe
                | NodeKind::NumToText
                | NodeKind::BoolToText
                | NodeKind::And
                | NodeKind::Or
                | NodeKind::Not
                | NodeKind::TextCompare
                | NodeKind::Reroute
                | NodeKind::GetVariable
                | NodeKind::MultiGetVariable
                | NodeKind::DictRowCount
                | NodeKind::DictGetRow
                | NodeKind::DictFindRow
                | NodeKind::DictGetCell
                | NodeKind::DictSchema
                | NodeKind::DictMakeRow
                | NodeKind::DictAppendRow
        )
    }

    /// Exec OUTPUT pins for this kind. Pure nodes have none; `If` splits into
    /// `true`/`false` branches; `StringSwitch` has user-defined arms (dynamic, see
    /// [`GraphNode::output_port`]); everything else gets a single `exec_out`.
    pub fn exec_out_ports(self) -> &'static [Port] {
        if self.is_pure() {
            return &NO_PORTS;
        }
        match self {
            NodeKind::If | NodeKind::AskAi => &EXEC_OUT_IF,
            NodeKind::ForEach => &EXEC_OUT_FOR_EACH,
            NodeKind::FlipFlop => &EXEC_OUT_FLIP_FLOP,
            // Dynamic: arms come from config, not a static slice. See GraphNode::output_port.
            NodeKind::StringSwitch | NodeKind::CameraSwitch | NodeKind::AiSwitch => &NO_PORTS,
            NodeKind::Comment => &NO_PORTS,
            // Display annotation nodes never execute — no exec pins (like Comment).
            NodeKind::Title | NodeKind::Paragraph | NodeKind::Markdown => &NO_PORTS,
            NodeKind::Reroute => &NO_PORTS, // pure passthrough, no exec pins
            NodeKind::TimeGate => &EXEC_OUT_TIME_GATE, // "out" fires when countdown expires
            NodeKind::DelayTrigger => &EXEC_OUT_DELAY_TRIGGER, // leading "start", trailing "stop"
            // Three arms, and the split is deliberate: `negative` is an ANSWER, `failure` is
            // the absence of one. See the kind's doc comment.
            NodeKind::DeviceGate => &EXEC_OUT_DEVICE_GATE,
            _ => &EXEC_OUT_DEFAULT,
        }
    }

    /// A value source with **no side effects** whose result depends only on its config plus a
    /// cheap in-memory lookup — safe to evaluate on any run, in any epoch.
    ///
    /// These activate unconditionally, because the alternative silently breaks them. A source with
    /// no incoming edges is a "true orphan", and orphans fall back to `seed_entries` — which is
    /// **false on every sub-run** (`run_subpath_eventful` passes it so). An orphan source therefore
    /// works on a cron/manual run and is *inert* inside any `OnEvent` or timer sub-path, taking its
    /// consumers down with it: a required input wired to a node that never ran makes the engine
    /// mark the whole branch dead, and skip it without an error. `GetVariable` was exempted by hand
    /// when someone hit exactly this; the others were left broken. This is that exemption,
    /// generalised — so the next source node doesn't have to rediscover it.
    ///
    /// Deliberately NOT included:
    /// - `GetLastAlert` — a database query. Re-running it per epoch isn't free, and whether "last"
    ///   should re-read mid-run is a real question, not an oversight.
    /// - Anything with data inputs (`Region` takes a camera): activating before its producers ran
    ///   would hand it empty inputs, trading a dead branch for a wrong answer.
    pub fn is_pure_source(self) -> bool {
        matches!(
            self,
            NodeKind::GetVariable
                | NodeKind::MultiGetVariable
                | NodeKind::GetCameraByName
                | NodeKind::GetRecipient
        )
    }

    pub fn input_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &NO_PORTS,
            NodeKind::GetRecipient => &NO_PORTS,
            NodeKind::CreateAlert => &CREATE_ALERT_IN,
            NodeKind::DeliverAlert => &DELIVER_ALERT_IN,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_IN,
            NodeKind::GetTrackedObjects => &GET_TRACKED_OBJECTS_IN,
            NodeKind::GetCurrentFrame => &GET_CURRENT_FRAME_IN,
            NodeKind::GetCameraByName => &NO_PORTS,
            NodeKind::BreakCamera => &BREAK_CAMERA_IN,
            NodeKind::Region => &REGION_IN,
            NodeKind::DescribeImage => &DESCRIBE_IMAGE_IN,
            NodeKind::NotifyRecipient => &NOTIFY_RECIPIENT_IN,
            NodeKind::DeviceGate => &DEVICE_GATE_IN,
            NodeKind::Format => &FORMAT_IN,
            NodeKind::Wait => &NO_PORTS, // control-flow only — sequenced via exec ports
            NodeKind::HttpPost => &HTTP_POST_IN,
            NodeKind::If => &IF_IN,
            NodeKind::ForEach => &FOR_EACH_IN,
            NodeKind::MotionDetection => &MOTION_IN,
            NodeKind::MotionWatch => &MOTION_WATCH_IN,
            NodeKind::PersonDetection => &PERSON_DETECTION_IN,
            NodeKind::ObjectDetection => &OBJECT_DETECTION_IN,
            NodeKind::BreakPersonDetection => &BREAK_PERSON_IN,
            NodeKind::BreakTrackedObject => &BREAK_TRACKED_OBJECT_IN,
            NodeKind::BreakDetection => &BREAK_DETECTION_IN,
            NodeKind::BreakFaceDetection => &BREAK_FACE_IN,
            NodeKind::RecognizePerson => &RECOGNIZE_PERSON_IN,
            NodeKind::FaceEmbedding => &FACE_EMBEDDING_IN,
            NodeKind::RecognizeFace => &RECOGNIZE_FACE_IN,
            NodeKind::StoreFace => &STORE_FACE_IN,
            NodeKind::RecordPresence => &RECORD_PRESENCE_IN,
            NodeKind::EmitEvent => &NO_PORTS, // exec-only (universal exec_in/out); event name in config
            NodeKind::OnEvent => &NO_PORTS,   // entry: only an exec_out; activated by the event
            NodeKind::Print => &PRINT_IN,
            NodeKind::ShowImage => &SHOW_IMAGE_IN,
            NodeKind::DrawBbox => &DRAW_BBOX_IN,
            NodeKind::DrawText => &DRAW_TEXT_IN,
            NodeKind::BlurOutsideBbox => &BLUR_OUTSIDE_BBOX_IN,
            NodeKind::GetImageSize => &GET_IMAGE_SIZE_IN,
            NodeKind::And | NodeKind::Or => &BOOL_OP_IN,
            NodeKind::Not => &NOT_IN,
            NodeKind::TextCompare => &TEXT_COMPARE_IN,
            NodeKind::FlipFlop => &NO_PORTS, // engine-driven; no data inputs
            NodeKind::Compare
            | NodeKind::NumLt
            | NodeKind::NumLte
            | NodeKind::NumGt
            | NodeKind::NumGte
            | NodeKind::NumEq
            | NodeKind::NumNe => &COMPARE_IN,
            NodeKind::NumToText => &NUM_TO_TEXT_IN,
            NodeKind::BoolToText => &BOOL_TO_TEXT_IN,
            NodeKind::Cooldown => &NO_PORTS, // engine-driven; window_secs is config
            NodeKind::StringSwitch => &STRING_SWITCH_IN,
            NodeKind::CameraSwitch => &CAMERA_SWITCH_IN,
            NodeKind::AskAi => &ASK_AI_IN,
            NodeKind::Comment => &NO_PORTS,
            // Display annotation nodes: pure canvas decoration, no data/exec ports.
            NodeKind::Title | NodeKind::Paragraph | NodeKind::Markdown => &NO_PORTS,
            // Display live sinks: one optional `value` input (Text / Num).
            NodeKind::Text => &TEXT_DISPLAY_IN,
            NodeKind::Gauge => &GAUGE_DISPLAY_IN,
            NodeKind::Reroute => &REROUTE_IN,
            NodeKind::StartRecording => &START_RECORDING_IN,
            NodeKind::StopRecording => &STOP_RECORDING_IN,
            NodeKind::TimeGate => &NO_PORTS, // exec-only; wait_secs/extend_secs are config
            NodeKind::DelayTrigger => &NO_PORTS, // exec-only; wait_secs/extend_secs are config
            // Get has no inputs; Set's typed `value` input is dynamic (GraphNode::input_ports).
            // The multi pair is entirely dynamic on both sides — every port comes from config.
            NodeKind::GetVariable
            | NodeKind::SetVariable
            | NodeKind::MultiSetVariable
            | NodeKind::MultiGetVariable => &NO_PORTS,
            NodeKind::ReplyToSender => &REPLY_TO_SENDER_IN,
            NodeKind::AiAnswer => &AI_ANSWER_IN,
            NodeKind::ReplyWithRecording => &REPLY_RECORDING_IN,
            NodeKind::AiSwitch => &AI_SWITCH_IN,
            NodeKind::AiExtract => &AI_EXTRACT_IN,
            NodeKind::DictRowCount => &DICT_ROW_COUNT_IN,
            NodeKind::DictGetRow => &DICT_GET_ROW_IN,
            NodeKind::DictFindRow => &DICT_FIND_ROW_IN,
            NodeKind::DictGetCell => &DICT_GET_CELL_IN,
            NodeKind::DictAddRow => &DICT_ADD_ROW_IN,
            NodeKind::DictSetCell => &DICT_SET_CELL_IN,
            NodeKind::DictUpdateRow => &DICT_UPDATE_ROW_IN,
            NodeKind::DictDeleteRow => &DICT_DELETE_ROW_IN,
            NodeKind::DictClear => &DICT_CLEAR_IN,
            NodeKind::DictSchema => &NO_PORTS, // inputs come from config columns only
            // DictMakeRow's inputs are dynamic (schema + one per column, see GraphNode::input_ports);
            // this static list is the fallback = just `schema`.
            NodeKind::DictMakeRow => &DICT_MAKE_ROW_IN,
            NodeKind::DictAppendRow => &DICT_APPEND_ROW_IN,
            // Device + data point + value all come from config dropdowns (no data inputs).
            NodeKind::TuyaControl | NodeKind::TuyaReadStatus => &NO_PORTS,
        }
    }

    pub fn output_ports(self) -> &'static [Port] {
        match self {
            NodeKind::GetLastAlert => &GET_LAST_ALERT_OUT,
            NodeKind::GetRecipient => &GET_RECIPIENT_OUT,
            NodeKind::CreateAlert => &CREATE_ALERT_OUT,
            NodeKind::DeliverAlert => &DELIVER_ALERT_OUT,
            NodeKind::CameraSnapshot => &CAMERA_SNAPSHOT_OUT,
            NodeKind::GetTrackedObjects => &GET_TRACKED_OBJECTS_OUT,
            NodeKind::GetCurrentFrame => &GET_CURRENT_FRAME_OUT,
            NodeKind::GetCameraByName => &GET_CAMERA_BY_NAME_OUT,
            NodeKind::BreakCamera => &BREAK_CAMERA_OUT,
            NodeKind::Region => &REGION_OUT,
            NodeKind::DescribeImage => &DESCRIBE_IMAGE_OUT,
            NodeKind::NotifyRecipient => &NOTIFY_RECIPIENT_OUT,
            NodeKind::DeviceGate => &DEVICE_GATE_OUT,
            NodeKind::Format => &FORMAT_OUT,
            NodeKind::Wait => &NO_PORTS, // control-flow only — sequenced via exec ports
            NodeKind::HttpPost => &HTTP_POST_OUT,
            NodeKind::If => &NO_PORTS, // outputs are exec branches (true/false)
            NodeKind::ForEach => &FOR_EACH_OUT, // item + index (exec outs are loop_body/completed)
            NodeKind::MotionDetection => &MOTION_OUT,
            NodeKind::MotionWatch => &MOTION_OUT,
            NodeKind::PersonDetection => &PERSON_DETECTION_OUT,
            NodeKind::ObjectDetection => &OBJECT_DETECTION_OUT,
            NodeKind::BreakPersonDetection => &BREAK_PERSON_OUT,
            NodeKind::BreakTrackedObject => &BREAK_TRACKED_OBJECT_OUT,
            NodeKind::BreakDetection => &BREAK_DETECTION_OUT,
            NodeKind::BreakFaceDetection => &BREAK_FACE_OUT,
            NodeKind::RecognizePerson => &RECOGNIZE_PERSON_OUT,
            NodeKind::FaceEmbedding => &FACE_EMBEDDING_OUT,
            NodeKind::RecognizeFace => &RECOGNIZE_FACE_OUT,
            NodeKind::StoreFace => &STORE_FACE_OUT,
            NodeKind::RecordPresence => &RECORD_PRESENCE_OUT,
            NodeKind::EmitEvent => &NO_PORTS, // exec passes through, no data out
            NodeKind::OnEvent => &NO_PORTS,   // drives its subtree via exec_out only
            NodeKind::Print => &NO_PORTS,     // a sink — exec passes through, no data out
            NodeKind::ShowImage => &NO_PORTS, // a sink — exec passes through, no data out
            NodeKind::DrawBbox => &DRAW_BBOX_OUT,
            NodeKind::DrawText => &DRAW_TEXT_OUT,
            NodeKind::BlurOutsideBbox => &BLUR_OUTSIDE_BBOX_OUT,
            NodeKind::GetImageSize => &GET_IMAGE_SIZE_OUT,
            NodeKind::And | NodeKind::Or | NodeKind::Not | NodeKind::TextCompare => &COMPARE_OUT,
            NodeKind::FlipFlop => &FLIP_FLOP_OUT,
            NodeKind::Compare
            | NodeKind::NumLt
            | NodeKind::NumLte
            | NodeKind::NumGt
            | NodeKind::NumGte
            | NodeKind::NumEq
            | NodeKind::NumNe => &COMPARE_OUT,
            NodeKind::NumToText | NodeKind::BoolToText => &CAST_TEXT_OUT,
            NodeKind::Cooldown => &COOLDOWN_OUT,
            NodeKind::StringSwitch | NodeKind::CameraSwitch => &NO_PORTS, // exec outputs are dynamic arms (+ "default")
            NodeKind::AskAi => &NO_PORTS, // exec outputs are true/false branches only
            NodeKind::Comment => &NO_PORTS,
            NodeKind::Title | NodeKind::Paragraph | NodeKind::Markdown => &NO_PORTS,
            // Display live sinks: exec passes through, no data out (like ShowImage).
            NodeKind::Text | NodeKind::Gauge => &NO_PORTS,
            NodeKind::Reroute => &REROUTE_OUT,
            NodeKind::StartRecording => &START_RECORDING_OUT,
            NodeKind::StopRecording => &NO_PORTS,
            NodeKind::TimeGate => &NO_PORTS, // exec-only; fires via exec "out" port
            NodeKind::DelayTrigger => &NO_PORTS, // exec-only; fires via exec "start"/"stop"
            // Get's typed `value` output is dynamic (GraphNode::output_ports); Set has none.
            NodeKind::GetVariable
            | NodeKind::SetVariable
            | NodeKind::MultiSetVariable
            | NodeKind::MultiGetVariable => &NO_PORTS,
            NodeKind::ReplyToSender => &REPLY_TO_SENDER_OUT,
            NodeKind::AiAnswer => &AI_ANSWER_OUT,
            NodeKind::ReplyWithRecording => &REPLY_RECORDING_OUT,
            NodeKind::AiSwitch => &NO_PORTS, // exec outputs are dynamic arms (+ "default")
            NodeKind::AiExtract => &AI_EXTRACT_OUT, // `value` type is refined per config (GraphNode)
            NodeKind::DictRowCount => &DICT_ROW_COUNT_OUT,
            NodeKind::DictGetRow => &DICT_GET_ROW_OUT,
            NodeKind::DictFindRow => &DICT_FIND_ROW_OUT,
            NodeKind::DictGetCell => &DICT_GET_CELL_OUT,
            // Mutate nodes: exec passes through, no data out.
            NodeKind::DictAddRow
            | NodeKind::DictSetCell
            | NodeKind::DictUpdateRow
            | NodeKind::DictDeleteRow
            | NodeKind::DictClear => &NO_PORTS,
            // Build nodes: pure, emit a composed value.
            NodeKind::DictSchema => &DICT_SCHEMA_OUT,
            NodeKind::DictMakeRow => &DICT_MAKE_ROW_OUT,
            NodeKind::DictAppendRow => &DICT_APPEND_ROW_OUT,
            NodeKind::TuyaControl => &TUYA_CONTROL_OUT,
            NodeKind::TuyaReadStatus => &TUYA_READ_STATUS_OUT,
        }
    }

    pub fn input_port(self, name: &str) -> Option<Port> {
        if name == EXEC_IN.name {
            return Some(EXEC_IN.clone());
        }
        self.input_ports().iter().find(|p| p.name == name).cloned()
    }

    pub fn output_port(self, name: &str) -> Option<Port> {
        if let Some(e) = self.exec_out_ports().iter().find(|p| p.name == name) {
            return Some(e.clone());
        }
        self.output_ports().iter().find(|p| p.name == name).cloned()
    }

    /// Static config + literal fallbacks for unconnected input ports.
    pub fn default_config(self) -> Value {
        match self {
            NodeKind::GetLastAlert => json!({}),
            NodeKind::GetRecipient => json!({ "recipient": "" }),
            NodeKind::CreateAlert => {
                json!({ "message": "", "room": "", "level": "info", "source": "workflow", "timeout_secs": "15" })
            }
            NodeKind::DeliverAlert => json!({ "id": "", "timeout_secs": "35" }),
            NodeKind::CameraSnapshot => json!({ "camera": "first", "timeout_secs": "10" }),
            NodeKind::GetTrackedObjects => json!({ "camera": "first", "timeout_secs": "10" }),
            NodeKind::GetCurrentFrame => {
                json!({ "camera": "first", "max_age_ms": "2000", "timeout_secs": "10" })
            }
            NodeKind::GetCameraByName => json!({ "camera": "" }),
            NodeKind::BreakCamera => json!({}),
            NodeKind::Region => json!({ "camera": "first", "name": "", "points": [] }),
            NodeKind::DescribeImage => json!({ "label": "", "question": "", "timeout_secs": "65" }),
            NodeKind::NotifyRecipient => json!({ "message": "", "timeout_secs": "35" }),
            // 10 minutes. Deliberately not seconds: decision 05 established that a push is
            // best-effort — it can be batched, silenced, or arrive late on a phone in Doze — so
            // the TTL has to assume the phone is CHECKED, not that the person was alerted. A
            // default measured in seconds would expire in a pocket and teach users the node is
            // unreliable.
            NodeKind::DeviceGate => json!({ "message": "", "ttl_secs": "600" }),
            NodeKind::Format => json!({ "template": "{p1} — {p2}", "p1": "", "p2": "", "p3": "" }),
            NodeKind::Wait => json!({ "seconds": "1" }),
            NodeKind::HttpPost => {
                json!({ "url": "", "headers": {}, "fields": {}, "file": "", "timeout_secs": "35" })
            }
            NodeKind::If => json!({}),
            NodeKind::ForEach => json!({ "items": "" }),
            NodeKind::MotionDetection => {
                json!({ "sensitivity": "2", "pixel_threshold": "25", "timeout_secs": "30" })
            }
            NodeKind::MotionWatch => {
                json!({ "camera": "first", "sensitivity": "2", "pixel_threshold": "25", "sample_gap_ms": "400", "timeout_secs": "15" })
            }
            NodeKind::PersonDetection => json!({ "timeout_secs": "30" }),
            NodeKind::ObjectDetection => json!({ "class": "person", "timeout_secs": "30" }),
            NodeKind::BreakPersonDetection => json!({}),
            NodeKind::BreakTrackedObject => json!({}),
            NodeKind::BreakDetection => json!({}),
            NodeKind::BreakFaceDetection => json!({}),
            NodeKind::RecognizePerson => json!({ "max_distance": "0.4", "timeout_secs": "30" }),
            NodeKind::FaceEmbedding => json!({ "timeout_secs": "30" }),
            NodeKind::RecognizeFace => {
                json!({ "top_k": "5", "max_distance": "0.5", "timeout_secs": "15" })
            }
            NodeKind::StoreFace => {
                json!({ "source": "snapshot", "dedup_distance": "0.15", "timeout_secs": "15" })
            }
            NodeKind::RecordPresence => {
                json!({ "category": "person", "room": "", "label": "", "duration_secs": "8" })
            }
            NodeKind::EmitEvent => json!({ "event": "" }),
            NodeKind::OnEvent => json!({ "event": "" }),
            NodeKind::Print => json!({ "message": "", "on_screen": true, "level": "info" }),
            NodeKind::ShowImage => json!({ "label": "" }),
            NodeKind::FlipFlop => json!({}),
            NodeKind::Compare => json!({ "operator": "==", "a": "0", "b": "0" }),
            NodeKind::NumLt
            | NodeKind::NumLte
            | NodeKind::NumGt
            | NodeKind::NumGte
            | NodeKind::NumEq
            | NodeKind::NumNe => json!({ "a": "0", "b": "0" }),
            NodeKind::NumToText | NodeKind::BoolToText => json!({}),
            NodeKind::Cooldown => json!({ "window_secs": "60" }),
            NodeKind::DrawBbox => {
                json!({ "color": "#00FF00", "thickness": "2", "style": "box", "expand": "0" })
            }
            NodeKind::DrawText => {
                json!({ "text": "", "position": "top-left", "size": "18", "text_color": "#FFFFFF", "background": true, "bg_color": "#000000" })
            }
            NodeKind::BlurOutsideBbox => json!({ "blur": "12" }),
            NodeKind::GetImageSize => json!({}),
            NodeKind::And | NodeKind::Or => json!({ "a": false, "b": false }),
            NodeKind::Not => json!({ "a": false }),
            NodeKind::TextCompare => json!({ "operator": "eq", "b": "" }),
            NodeKind::StringSwitch => json!({ "arms": ["arm_1", "arm_2"] }),
            NodeKind::CameraSwitch => json!({ "arms": [] }),
            NodeKind::AskAi => json!({ "question": "", "timeout_secs": "65" }),
            NodeKind::Comment => {
                json!({ "text": "Comment", "color": "#facc15", "width": 400, "height": 200 })
            }
            NodeKind::Title => json!({ "text": "Title", "color": "#e5e7eb", "size": "lg" }),
            NodeKind::Paragraph => {
                json!({ "text": "Description…", "color": "#9ca3af", "width": 320, "height": 160 })
            }
            NodeKind::Markdown => {
                json!({ "text": "## Heading\n\n- point one\n- point two", "width": 340, "height": 220 })
            }
            // `value` = the literal shown when the input port is unwired (also the port's fallback).
            NodeKind::Text => {
                json!({ "value": "", "bg_color": "#1f2937", "text_color": "#e5e7eb" })
            }
            NodeKind::Gauge => {
                json!({ "value": 0, "min": 0, "max": 100, "label": "", "color": "#22c55e", "size": 160 })
            }
            NodeKind::Reroute => json!({}),
            NodeKind::StartRecording => json!({ "timeout_secs": "30" }),
            NodeKind::StopRecording => json!({ "timeout_secs": "15" }),
            NodeKind::TimeGate => json!({ "wait_secs": 10, "extend_secs": 10 }),
            NodeKind::DelayTrigger => json!({ "wait_secs": 10, "extend_secs": 10 }),
            // `variable` = the declared variable's name; `type` = its wire type (baked in so the
            // dynamic port resolves without a graph lookup). Set by the Variables panel on create.
            NodeKind::GetVariable | NodeKind::SetVariable => {
                json!({ "variable": "", "type": "text" })
            }
            NodeKind::ReplyToSender => json!({ "message": "", "timeout_secs": "35" }),
            NodeKind::AiAnswer => json!({ "prompt": "", "timeout_secs": "30" }),
            NodeKind::ReplyWithRecording => json!({ "caption": "", "timeout_secs": "120" }),
            NodeKind::AiSwitch => json!({ "arms": ["arm_1", "arm_2"], "timeout_secs": "30" }),
            NodeKind::AiExtract => json!({ "field": "", "type": "text", "timeout_secs": "30" }),
            // Dictionaries: `table` is a dictionary id (dropdown); the rest are literal fallbacks
            // for the unwired input ports.
            NodeKind::DictRowCount | NodeKind::DictClear => json!({ "table": "" }),
            NodeKind::DictGetRow => json!({ "table": "", "index": "0" }),
            NodeKind::DictFindRow => json!({ "table": "", "column": "", "key": "" }),
            NodeKind::DictGetCell => json!({ "table": "", "row": "0", "column": "" }),
            NodeKind::DictAddRow => json!({ "table": "", "values": {} }),
            NodeKind::DictSetCell => json!({ "table": "", "row": "0", "column": "", "value": "" }),
            NodeKind::DictUpdateRow => {
                json!({ "table": "", "column": "", "key": "", "values": {} })
            }
            NodeKind::DictDeleteRow => json!({ "table": "", "column": "", "key": "" }),
            // Build nodes compose a table in-graph. `columns` = `[{name, type}]` (same shape as a
            // Dictionary's schema); DictSchema/DictMakeRow bake it, DictAppendRow is input-only.
            NodeKind::DictSchema | NodeKind::DictMakeRow => json!({ "columns": [] }),
            // The multi variable pair starts EMPTY — dropping the node lists nothing, and you add
            // the variables you want. A node that grew a port every time someone declared a
            // variable would rewire graphs behind the author's back.
            NodeKind::MultiSetVariable | NodeKind::MultiGetVariable => {
                json!({ "variables": [] })
            }
            NodeKind::DictAppendRow => json!({}),
            // `tuya_device` = the device dropdown (stores the stable device id); `tuya_code` =
            // the data point dropdown (e.g. `switch_1`); `value` = the value literal, rendered
            // by an adaptive widget typed to the point.
            NodeKind::TuyaControl => {
                json!({ "tuya_device": "", "tuya_code": "", "value": "", "value_type": "bool", "timeout_secs": "15" })
            }
            NodeKind::TuyaReadStatus => {
                json!({ "tuya_device": "", "tuya_code": "", "value_type": "text", "timeout_secs": "15" })
            }
        }
    }
}
