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
//!
//! The layer is split by what each part does with a node:
//!
//! - [`value`] — what a wire carries;
//! - [`literals`] — reading what the user typed when nothing is wired;
//! - [`resolve`] — turning names in a configuration into the things they name, and working out
//!   which exec arms fire;
//! - [`dispatch`] — running one node, and how long it may take;
//! - [`summary`] — the one line a person reads afterwards.
//!
//! The per-family files next to them (`vision`, `camera`, `telegram`, …) hold the nodes
//! themselves. Everything is re-exported here, so `node::anything` resolves as it always did.

mod ai;
mod alerts;
mod camera;
mod control;
mod debug_node;
mod dictionary;
mod logic;
mod network;
mod presence;
mod recipients;
mod recording;
mod telegram;
mod text;
mod tuya;
mod variable;
mod vision;

pub mod dispatch;
pub mod literals;
pub mod resolve;
pub mod summary;
pub mod value;

pub use dispatch::*;
pub use literals::*;
pub use resolve::*;
pub use summary::*;
pub use value::*;

use super::domain::{NodeKind, Port, PortType};
use crate::agent::tools::ToolContext;
use crate::workflows::domain::types;
use base64::Engine as _;
use serde_json::Value;
use std::collections::HashMap;

use super::*;

#[cfg(test)]
mod tests {
    use super::super::domain::NodeKind;
    use super::*;
    use crate::workflows::domain::PortName;
    use serde_json::json;

    fn resolved(
        name: &str,
        ty: crate::workflows::domain::PortType,
    ) -> crate::workflows::domain::ResolvedPort {
        crate::workflows::domain::ResolvedPort {
            name: PortName::new(name),
            ty,
            required: false,
        }
    }

    /// An Image-typed event field arrives base64 (JSON has no byte type) and must reach the graph
    /// as real `Image` bytes — otherwise every node requiring an image reports it missing, however
    /// the graph is wired (#117: `face_detected.frame` → Extract Face Embedding).
    #[test]
    fn base64_frame_payload_becomes_image_bytes() {
        let jpeg = vec![0xFFu8, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg);
        let mut payload = PortValues::new();
        payload.insert("frame".to_string(), PortValue::Text(b64));

        let out = seed_event_payload(
            &payload,
            &[resolved("frame", crate::workflows::domain::types::IMAGE)],
            &test_ctx(),
        );

        assert_eq!(
            out.get("frame").and_then(PortValue::as_image),
            Some(&jpeg[..])
        );
    }

    /// A value that isn't base64 leaves the port unfilled — an honest "missing input" beats
    /// handing garbage bytes to a decoder.
    #[test]
    fn non_base64_frame_payload_is_left_unfilled() {
        let mut payload = PortValues::new();
        payload.insert(
            "frame".to_string(),
            PortValue::Text("!!not base64!!".into()),
        );

        let out = seed_event_payload(
            &payload,
            &[resolved("frame", crate::workflows::domain::types::IMAGE)],
            &test_ctx(),
        );

        assert!(out.get("frame").and_then(PortValue::as_image).is_none());
    }

    fn test_ctx() -> ToolContext {
        let (tx, _rx) = std::sync::mpsc::channel();
        ToolContext::new_local(
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            crate::db::Db::lazy("postgres://sentinel:sentinel@localhost/sentinel_test"),
            crate::tenancy::DEFAULT_TENANT_ID,
            std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            None,
            tx,
        )
    }

    #[test]
    fn parse_variable_value_matches_default_seeding() {
        let sys = test_ctx();
        // The types a manual-run input can carry — parsed identically to a stored default.
        assert!(
            matches!(parse_variable_value(types::TEXT, "hi", &sys), Some(PortValue::Text(t)) if t == "hi")
        );
        assert!(matches!(
            parse_variable_value(types::NUM, "42", &sys),
            Some(PortValue::Num(42))
        ));
        assert!(matches!(
            parse_variable_value(types::BOOL, "true", &sys),
            Some(PortValue::Bool(true))
        ));
        assert!(matches!(
            parse_variable_value(types::BOOL, "nope", &sys),
            Some(PortValue::Bool(false))
        ));
        assert!(
            matches!(parse_variable_value(types::RECIPIENT, "ops", &sys), Some(PortValue::Recipient(r)) if r == "ops")
        );
        // Rejections fall back to the Get's type-zero, never a wrong value or a run failure.
        assert!(parse_variable_value(types::NUM, "not-a-number", &sys).is_none());
        assert!(
            parse_variable_value(types::TEXT, "   ", &sys).is_none(),
            "blank is no value"
        );
        assert!(
            parse_variable_value(types::IMAGE, "x", &sys).is_none(),
            "non-defaultable type"
        );
        // Camera needs a resolvable device; the empty test ctx has none → None (not a crash).
        assert!(parse_variable_value(types::CAMERA, "192.168.1.1", &sys).is_none());
    }

    #[test]
    fn node_timeout_maps_io_vs_pure_kinds() {
        use std::time::Duration;
        for k in [
            NodeKind::Wait,
            NodeKind::Format,
            NodeKind::If,
            NodeKind::Print,
            NodeKind::ForEach,
            NodeKind::ShowImage,
        ] {
            assert_eq!(node_timeout(k, &json!({})), None, "{k:?} should run inline");
        }
        assert_eq!(
            node_timeout(NodeKind::CameraSnapshot, &json!({})),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            node_timeout(NodeKind::DescribeImage, &json!({})),
            Some(Duration::from_secs(65))
        );
        assert_eq!(
            node_timeout(NodeKind::RecognizePerson, &json!({})),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn node_timeout_config_override_wins_and_zero_disables() {
        use std::time::Duration;
        assert_eq!(
            node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": "3" })),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": 3 })),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            node_timeout(NodeKind::CameraSnapshot, &json!({ "timeout_secs": "0" })),
            None
        );
    }

    #[test]
    fn run_node_timed_aborts_a_slow_node_promptly() {
        use std::time::{Duration, Instant};
        let cfg = json!({ "seconds": "5" });
        let start = Instant::now();
        let err = run_node_timed(
            NodeKind::Wait,
            &cfg,
            &PortValues::new(),
            &test_ctx(),
            Duration::from_millis(200),
        )
        .unwrap_err();
        let elapsed = start.elapsed();
        assert!(
            err.contains("timed out"),
            "expected a timeout error, got: {err}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "should return at the deadline, not after the 5s body: {elapsed:?}"
        );
    }

    #[test]
    fn run_node_timed_returns_a_fast_nodes_output() {
        use std::time::Duration;
        let cfg = json!({ "template": "hello" });
        let out = run_node_timed(
            NodeKind::Format,
            &cfg,
            &PortValues::new(),
            &test_ctx(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(out.get("text").and_then(PortValue::as_text), Some("hello"));
    }

    #[test]
    fn literal_for_coerces_and_skips_empty() {
        let cfg = json!({ "camera": "front", "blank": "  ", "n": "7" });
        let text = Port {
            name: PortName::new_static("camera"),
            ty: types::TEXT,
            required: true,
        };
        let blank = Port {
            name: PortName::new_static("blank"),
            ty: types::TEXT,
            required: false,
        };
        let num = Port {
            name: PortName::new_static("n"),
            ty: types::NUM,
            required: true,
        };
        assert_eq!(literal_for(&cfg, text).unwrap().as_text(), Some("front"));
        assert!(literal_for(&cfg, blank).is_none());
        assert_eq!(literal_for(&cfg, num).unwrap().as_num(), Some(7));
    }

    #[test]
    fn literal_for_parses_a_string_dictionary() {
        let cfg =
            json!({ "headers": { "Authorization": "Bearer x", "X-Env": "prod" }, "empty": {} });
        let dict = Port {
            name: PortName::new_static("headers"),
            ty: types::DICTIONARY,
            required: false,
        };
        let empty = Port {
            name: PortName::new_static("empty"),
            ty: types::DICTIONARY,
            required: false,
        };
        let got = literal_for(&cfg, dict).unwrap();
        let pairs = got.as_dict().unwrap();
        assert_eq!(pairs.len(), 2);
        assert!(pairs.contains(&("Authorization".into(), "Bearer x".into())));
        assert!(literal_for(&cfg, empty).is_none());
    }

    #[test]
    fn http_post_requires_a_url_before_any_request() {
        let err = run_node(
            NodeKind::HttpPost,
            &json!({ "url": "  " }),
            &PortValues::new(),
            &test_ctx(),
        )
        .unwrap_err();
        assert!(err.contains("missing required input 'url'"), "got: {err}");
    }

    fn solid_jpeg(w: u32, h: u32, luma: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([luma, luma, luma]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut buf),
                image::ImageFormat::Jpeg,
            )
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
        let err = run_node(
            NodeKind::PersonDetection,
            &json!({}),
            &PortValues::new(),
            &test_ctx(),
        )
        .unwrap_err();
        assert!(err.contains("missing required input 'image'"), "got: {err}");
    }

    #[test]
    fn recognize_person_requires_an_image() {
        let err = run_node(
            NodeKind::RecognizePerson,
            &json!({}),
            &PortValues::new(),
            &test_ctx(),
        )
        .unwrap_err();
        assert!(err.contains("missing required input 'image'"), "got: {err}");
    }

    #[test]
    fn show_image_is_a_sink_with_no_data_outputs() {
        let mut inputs = PortValues::new();
        inputs.insert("image".into(), PortValue::Image(solid_jpeg(32, 24, 128)));
        let out = run_node(
            NodeKind::ShowImage,
            &json!({ "label": "frame" }),
            &inputs,
            &test_ctx(),
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn motion_requires_both_frames() {
        let err = run_node(
            NodeKind::MotionDetection,
            &json!({}),
            &PortValues::new(),
            &test_ctx(),
        )
        .unwrap_err();
        assert!(
            err.contains("missing required input 'image_a'"),
            "got: {err}"
        );
    }

    #[test]
    fn if_requires_a_condition() {
        let err = run_node(NodeKind::If, &json!({}), &PortValues::new(), &test_ctx()).unwrap_err();
        assert!(
            err.contains("missing required input 'condition'"),
            "got: {err}"
        );
    }

    #[test]
    fn wait_is_pure_control_flow_no_data_outputs() {
        let out = run_node(
            NodeKind::Wait,
            &json!({ "seconds": "0" }),
            &PortValues::new(),
            &test_ctx(),
        )
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
        assert!(
            err.contains("missing required input 'message'"),
            "got: {err}"
        );
    }

    #[test]
    fn ask_ai_requires_a_question() {
        let err = run_node(
            NodeKind::AskAi,
            &json!({ "question": "" }),
            &PortValues::new(),
            &test_ctx(),
        )
        .unwrap_err();
        assert!(err.contains("question is required"), "got: {err}");
    }

    #[test]
    fn ask_ai_timeout_is_65s() {
        assert_eq!(
            node_timeout(NodeKind::AskAi, &json!({})),
            Some(std::time::Duration::from_secs(65))
        );
    }

    /// Regression: the Current Frame run-log line reported "0 KB" on every successful grab.
    /// The summary read `inputs["image"]`, but this node's only input is `camera` — the image is
    /// an OUTPUT. The functionality was fine; the log lied about it.
    #[test]
    fn current_frame_summary_reports_the_size_it_actually_returned() {
        let mut outputs = PortValues::new();
        outputs.insert("image".to_string(), PortValue::Image(vec![0u8; 40 * 1024]));
        outputs.insert("age_ms".to_string(), PortValue::Num(0));
        let s = debug_summary(
            NodeKind::GetCurrentFrame,
            &json!({}),
            &PortValues::new(), // inputs carry only `camera`, never the image
            &outputs,
        );
        assert_eq!(s, "current frame (40 KB, 0 ms old)");
    }
}
