//! Bishop's internal tool registry. Each [`ToolSpec`] exposes a name, an API
//! `input_schema`, and an executor; adding a new capability is one `push`. The
//! executors run on the agent's background thread and reach app state through a
//! [`ToolContext`] (the shared device store + a UI-effect channel).
use std::sync::mpsc::Sender;

use image::GrayImage;
use serde_json::{json, Value};

use crate::device::actions::{self, DeviceSummary};
use crate::notification::NotificationStore;
use crate::scan::DeviceStore;

/// A side-effect a tool asks the main render loop to perform.
pub enum ToolUiEvent {
    /// Show a camera snapshot in a modal over the chat.
    Image(GrayImage),
    /// Render a styled device-list block in the chat log.
    DevicesListBlock(Vec<DeviceSummary>),
    /// The device store changed on disk — re-validate UI selection/detail.
    DevicesPersisted,
    /// Open a live RTSP feed in a modal over the chat.
    StartLive {
        ip: String,
        rtsp: String,
        user: String,
        pass: String,
    },
}

/// What a tool executor can touch.
pub struct ToolContext {
    pub store: DeviceStore,
    pub ui_tx: Sender<ToolUiEvent>,
    /// Trusted notification targets, for the `send_notification` tool.
    pub notif: NotificationStore,
}

type ToolFn = fn(&Value, &ToolContext) -> Result<String, String>;

pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub run: ToolFn,
}

pub struct ToolRegistry {
    specs: Vec<ToolSpec>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        ToolRegistry {
            specs: vec![
                list_devices(),
                get_camera_snapshot(),
                describe_camera(),
                show_live_feed(),
                delete_device(),
                delete_device_by_index(),
                rename_device(),
                send_notification(),
            ],
        }
    }

    /// The `tools` array for the API request.
    pub fn specs_json(&self) -> Vec<Value> {
        self.specs
            .iter()
            .map(|s| json!({ "name": s.name, "description": s.description, "input_schema": s.input_schema }))
            .collect()
    }

    /// Run a tool by name with parsed input.
    pub fn run(&self, name: &str, input: &Value, ctx: &ToolContext) -> Result<String, String> {
        match self.specs.iter().find(|s| s.name == name) {
            Some(s) => (s.run)(input, ctx),
            None => Err(format!("Unknown tool: {name}")),
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---- the device tools -------------------------------------------------------

fn list_devices() -> ToolSpec {
    ToolSpec {
        name: "list_devices",
        description: "List the operator's saved/scanned cameras (name, IP, and whether \
            credentials are saved). Use this to show the operator their devices, or to find a \
            device by name before acting on it.",
        input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
        run: |_input, ctx| {
            let summaries = {
                let devices = ctx.store.lock().unwrap();
                actions::list_summaries(&devices)
            };
            let text = actions::summaries_text(&summaries);
            let _ = ctx.ui_tx.send(ToolUiEvent::DevicesListBlock(summaries));
            Ok(text)
        },
    }
}

fn get_camera_snapshot() -> ToolSpec {
    ToolSpec {
        name: "get_camera_snapshot",
        description: "Capture a still image from a camera and display it to the operator. Give \
            the camera's name, or \"first\" for the first saved camera. The camera must have \
            saved credentials.",
        input_schema: json!({
            "type": "object",
            "properties": { "camera": { "type": "string", "description": "camera name, or \"first\"" } },
            "required": ["camera"]
        }),
        run: |input, ctx| {
            let camera = input.get("camera").and_then(Value::as_str).unwrap_or("first");
            let device = {
                let devices = ctx.store.lock().unwrap();
                let idx = actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
                devices[idx].clone()
            };
            let img = actions::capture_snapshot(&device)?;
            let _ = ctx.ui_tx.send(ToolUiEvent::Image(img));
            Ok(format!("Snapshot of '{}' captured and displayed.", device.name))
        },
    }
}

fn describe_camera() -> ToolSpec {
    ToolSpec {
        name: "describe_camera",
        description: "Capture a still from a camera and describe what is currently visible in it \
            — people, vehicles, objects, activity, the setting and lighting. Use this whenever the \
            operator asks what is on a camera, what can be seen, or to check on a location. Give \
            the camera's name, or \"first\" for the first saved camera. The camera must have saved \
            credentials. The snapshot is also shown to the operator.",
        input_schema: json!({
            "type": "object",
            "properties": { "camera": { "type": "string", "description": "camera name, or \"first\"" } },
            "required": ["camera"]
        }),
        run: |input, ctx| {
            let camera = input.get("camera").and_then(Value::as_str).unwrap_or("first");
            let device = {
                let devices = ctx.store.lock().unwrap();
                let idx = actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
                devices[idx].clone()
            };
            // Hit the camera once; show the still and feed the same bytes to vision.
            let bytes = actions::capture_snapshot_bytes(&device)?;
            if let Ok(img) = actions::decode_luma(&bytes) {
                let _ = ctx.ui_tx.send(ToolUiEvent::Image(img));
            }
            let description = crate::agent::vision::describe_image(&bytes, &device.name)?;
            Ok(format!("Camera '{}' shows: {description}", device.name))
        },
    }
}

fn show_live_feed() -> ToolSpec {
    ToolSpec {
        name: "show_live_feed",
        description: "Open a live video feed from a camera for the operator. Give the camera's \
            name, or \"first\" for the first saved camera. The camera must have saved credentials.",
        input_schema: json!({
            "type": "object",
            "properties": { "camera": { "type": "string", "description": "camera name, or \"first\"" } },
            "required": ["camera"]
        }),
        run: |input, ctx| {
            let camera = input.get("camera").and_then(Value::as_str).unwrap_or("first");
            let device = {
                let devices = ctx.store.lock().unwrap();
                let idx = actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
                devices[idx].clone()
            };
            if !device.has_credentials() {
                return Err(format!("Camera '{}' has no saved credentials.", device.name));
            }
            let rtsp = device
                .rtsp_url
                .clone()
                .ok_or("No stream URL — validate the credentials first.")?;
            let _ = ctx.ui_tx.send(ToolUiEvent::StartLive {
                ip: device.ip.clone(),
                rtsp,
                user: device.username.clone().unwrap_or_default(),
                pass: device.password.clone().unwrap_or_default(),
            });
            Ok(format!("Opening the live feed for '{}'.", device.name))
        },
    }
}

fn delete_device() -> ToolSpec {
    ToolSpec {
        name: "delete_device",
        description: "Permanently remove a saved device by name. Only call this AFTER the \
            operator has explicitly confirmed they want that specific device removed — never on \
            the first mention.",
        input_schema: json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        }),
        run: |input, ctx| {
            let name = input
                .get("name")
                .and_then(Value::as_str)
                .ok_or("Missing 'name'.")?;
            let removed = actions::delete_by_name(&ctx.store, name)?;
            let _ = ctx.ui_tx.send(ToolUiEvent::DevicesPersisted);
            Ok(format!("Deleted '{removed}'."))
        },
    }
}

fn delete_device_by_index() -> ToolSpec {
    ToolSpec {
        name: "delete_device_by_index",
        description: "Permanently remove a device by its position in the list (1 = first, 2 = \
            second, and so on — matching the numbers shown by list_devices). Only call this AFTER \
            the operator has explicitly confirmed.",
        input_schema: json!({
            "type": "object",
            "properties": { "index": { "type": "integer", "description": "1-based position (1 = first)" } },
            "required": ["index"]
        }),
        run: |input, ctx| {
            let index = input
                .get("index")
                .and_then(Value::as_u64)
                .ok_or("Missing 'index'.")? as usize;
            let removed = actions::delete_by_index(&ctx.store, index)?;
            let _ = ctx.ui_tx.send(ToolUiEvent::DevicesPersisted);
            Ok(format!("Deleted '{removed}'."))
        },
    }
}

fn rename_device() -> ToolSpec {
    ToolSpec {
        name: "rename_device",
        description: "Rename a saved device. Identify it by its current name (or \"first\") and \
            give the new name.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "camera": { "type": "string", "description": "current name, or \"first\"" },
                "new_name": { "type": "string" }
            },
            "required": ["camera", "new_name"]
        }),
        run: |input, ctx| {
            let camera = input.get("camera").and_then(Value::as_str).unwrap_or("first");
            let new_name = input
                .get("new_name")
                .and_then(Value::as_str)
                .ok_or("Missing 'new_name'.")?;
            let ip = {
                let devices = ctx.store.lock().unwrap();
                let idx = actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
                devices[idx].ip.clone()
            };
            let renamed = actions::rename_device(&ctx.store, &ip, new_name)?;
            let _ = ctx.ui_tx.send(ToolUiEvent::DevicesPersisted);
            Ok(format!("Renamed to '{renamed}'."))
        },
    }
}

fn send_notification() -> ToolSpec {
    ToolSpec {
        name: "send_notification",
        description: "Send the operator an alert on their phone via Telegram — text, or text plus \
            a snapshot from one of their cameras. Use this when the operator asks to be notified, \
            texted, alerted, or pinged about something, or to forward what a camera sees to their \
            phone. Give the message; optionally name the target (the phone/person to reach) and a \
            camera whose current snapshot to attach. A phone must be paired first (the operator \
            sends a one-time pairing code to the bot); if none is paired, tell the operator to \
            pair one. If several targets exist and none is named, ask which.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "the alert text to send" },
                "target": { "type": "string", "description": "the paired phone/person label to send to (optional if only one is paired)" },
                "camera": { "type": "string", "description": "optional camera name (or \"first\") whose current snapshot to attach" }
            },
            "required": ["message"]
        }),
        run: |input, ctx| {
            let message = input.get("message").and_then(Value::as_str).ok_or("Missing 'message'.")?;
            let target = input.get("target").and_then(Value::as_str).filter(|s| !s.trim().is_empty());
            let camera = input.get("camera").and_then(Value::as_str).filter(|s| !s.trim().is_empty());

            let cfg = crate::notification::NotificationConfig::from_env()
                .ok_or("Notifications are not configured (set TELEGRAM_BOT_TOKEN).")?;

            // Optional snapshot: resolve + capture on this (agent) thread — a
            // synchronous network hit here is fine, like describe_camera.
            let image = match camera {
                Some(name) => {
                    let device = {
                        let devices = ctx.store.lock().unwrap();
                        let idx = actions::resolve_device(&devices, name).map_err(|e| e.to_string())?;
                        devices[idx].clone()
                    };
                    Some(to_jpeg(&actions::capture_snapshot_bytes(&device)?)?)
                }
                None => None,
            };

            crate::notification::dispatch(&ctx.notif, &cfg, target, message, image)
        },
    }
}

/// Re-encode arbitrary snapshot bytes (JPEG over HTTP, or a frame from the RTSP
/// fallback) to JPEG, which is what Telegram's `sendPhoto` expects.
fn to_jpeg(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode snapshot: {e}"))?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| format!("encode snapshot: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_errors() {
        let reg = ToolRegistry::new();
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = ToolContext {
            store: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            ui_tx: tx,
            notif: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        };
        assert!(reg.run("nope", &json!({}), &ctx).is_err());
        // list_devices always succeeds, even with an empty store
        assert!(reg.run("list_devices", &json!({}), &ctx).is_ok());
    }
}
