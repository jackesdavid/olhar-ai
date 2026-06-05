//! Bishop's internal tool registry — the web-app port of the ratatui
//! `src/agent/tools.rs`. Each [`ToolSpec`] exposes a name, an API `input_schema`,
//! and an executor; adding a capability is one `push`. Executors run on a
//! background task and reach app state through a [`ToolContext`] (the shared
//! stores + app-data paths + the resolved Telegram config + a UI-effect channel).
//!
//! Two deliberate adaptations from the ratatui original:
//!   1. **Persistence is path-parameterized** — mutating tools save through the
//!      app-data paths carried on the context (not a hardcoded cwd file).
//!   2. **[`ToolUiEvent`] is web-friendly** — instead of a `GrayImage` modal /
//!      styled block, it carries JPEG bytes + "changed" pokes + an open-live
//!      request that the Tauri shell drains off the channel and re-emits as Tauri
//!      events while the turn runs.
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use serde_json::{json, Value};

use crate::alerts::{self, AlertLevel, AlertStore};
use crate::device_actions as actions;
use crate::devices::{save_devices, DeviceStore};
use crate::map;
use crate::notification::{self, NotificationConfig, NotificationStore};

/// A side-effect a tool asks the UI layer to perform. The Tauri shell drains
/// these off the channel during the turn and re-emits them as Tauri events.
pub enum ToolUiEvent {
    /// A camera snapshot to show inline in the chat (JPEG bytes → base64 data URL).
    Snapshot { camera: String, jpeg: Vec<u8> },
    /// The device store changed on disk — pages should re-list.
    DevicesChanged,
    /// The alert store changed on disk — the dashboard should refresh.
    AlertsChanged,
    /// Offer the operator a live feed for this device (chat shows an "open" action).
    OpenLive { ip: String },
}

/// What a tool executor can touch. Built by the Tauri shell from the managed
/// stores + app-data paths + the resolved Telegram config.
pub struct ToolContext {
    pub devices: DeviceStore,
    pub devices_path: PathBuf,
    pub alerts: AlertStore,
    pub alerts_path: PathBuf,
    pub notif: NotificationStore,
    pub map_path: PathBuf,
    /// Resolved Telegram config (saved `telegram.json` else env), or `None` when
    /// notifications aren't configured — the `notify_beacon` tool errors then.
    pub notif_cfg: Option<NotificationConfig>,
    pub ui_tx: Sender<ToolUiEvent>,
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
                create_alert(),
                list_alerts(),
                alert_stats(),
                deliver_alert(),
                delete_alert(),
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
            let devices = ctx.devices.lock().unwrap();
            let summaries = actions::list_summaries(&devices);
            Ok(actions::summaries_text(&summaries))
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
                let devices = ctx.devices.lock().unwrap();
                let idx = actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
                devices[idx].clone()
            };
            let bytes = actions::capture_snapshot_bytes(&device)?;
            let jpeg = to_jpeg(&bytes)?;
            let _ = ctx.ui_tx.send(ToolUiEvent::Snapshot { camera: device.name.clone(), jpeg });
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
                let devices = ctx.devices.lock().unwrap();
                let idx = actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
                devices[idx].clone()
            };
            // Hit the camera once; show the still and feed the same bytes to vision.
            let bytes = actions::capture_snapshot_bytes(&device)?;
            if let Ok(jpeg) = to_jpeg(&bytes) {
                let _ = ctx.ui_tx.send(ToolUiEvent::Snapshot { camera: device.name.clone(), jpeg });
            }
            let description = super::vision::describe_image(&bytes, &device.name)?;
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
                let devices = ctx.devices.lock().unwrap();
                let idx = actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
                devices[idx].clone()
            };
            if !device.has_credentials() {
                return Err(format!("Camera '{}' has no saved credentials.", device.name));
            }
            let _ = ctx.ui_tx.send(ToolUiEvent::OpenLive { ip: device.ip.clone() });
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
            let name = input.get("name").and_then(Value::as_str).ok_or("Missing 'name'.")?;
            let removed = {
                let mut devices = ctx.devices.lock().unwrap();
                let removed = actions::delete_by_name(&mut devices, name)?;
                save_devices(&ctx.devices_path, &devices);
                removed
            };
            let _ = ctx.ui_tx.send(ToolUiEvent::DevicesChanged);
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
            let index = input.get("index").and_then(Value::as_u64).ok_or("Missing 'index'.")? as usize;
            let removed = {
                let mut devices = ctx.devices.lock().unwrap();
                let removed = actions::delete_by_index(&mut devices, index)?;
                save_devices(&ctx.devices_path, &devices);
                removed
            };
            let _ = ctx.ui_tx.send(ToolUiEvent::DevicesChanged);
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
            let new_name = input.get("new_name").and_then(Value::as_str).ok_or("Missing 'new_name'.")?;
            let trimmed = new_name.trim();
            if trimmed.is_empty() {
                return Err("The new name can't be empty.".into());
            }
            let renamed = {
                let mut devices = ctx.devices.lock().unwrap();
                let idx = actions::resolve_device(&devices, camera).map_err(|e| e.to_string())?;
                devices[idx].name = trimmed.to_string();
                save_devices(&ctx.devices_path, &devices);
                trimmed.to_string()
            };
            let _ = ctx.ui_tx.send(ToolUiEvent::DevicesChanged);
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

            let cfg = ctx
                .notif_cfg
                .as_ref()
                .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;

            // Optional snapshot: resolve + capture on this (background) task.
            let image = match camera {
                Some(name) => {
                    let device = {
                        let devices = ctx.devices.lock().unwrap();
                        let idx = actions::resolve_device(&devices, name).map_err(|e| e.to_string())?;
                        devices[idx].clone()
                    };
                    Some(to_jpeg(&actions::capture_snapshot_bytes(&device)?)?)
                }
                None => None,
            };

            notification::dispatch(&ctx.notif, cfg, target, message, image)
        },
    }
}

// ---- the alert tools --------------------------------------------------------

fn create_alert() -> ToolSpec {
    ToolSpec {
        name: "create_alert",
        description: "Raise an alert in the alert registry. Give a message; optionally a level \
            (\"debug\", \"info\", \"warn\", or \"error\" — default \"info\"), a room name (the \
            location, matched against the map's compartments; omit for a system-wide alert), and a \
            source (the subsystem raising it — default \"bishop\"). The alert is logged but NOT sent \
            to anyone; use deliver_alert to dispatch it to the beacons.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "the alert text" },
                "level": { "type": "string", "description": "debug | info | warn | error (default info)" },
                "room": { "type": "string", "description": "optional map room name (the location)" },
                "source": { "type": "string", "description": "optional subsystem name (default bishop)" }
            },
            "required": ["message"]
        }),
        run: |input, ctx| {
            let message = input.get("message").and_then(Value::as_str).ok_or("Missing 'message'.")?;
            let level = input
                .get("level")
                .and_then(Value::as_str)
                .and_then(AlertLevel::parse)
                .unwrap_or_default();
            let source = input
                .get("source")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("bishop")
                .to_string();

            // Resolve an optional room name against the saved map.
            let (room_id, room_name) = match input
                .get("room")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                Some(name) => {
                    let rooms = map::load_map(&ctx.map_path);
                    match rooms.iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())) {
                        Some(r) => (Some(r.id), Some(r.name.clone())),
                        None => {
                            let known: Vec<&str> = rooms.iter().map(|r| r.name.as_str()).collect();
                            return Err(format!(
                                "No room named '{}'. Known rooms: {}.",
                                name.trim(),
                                if known.is_empty() { "none".into() } else { known.join(", ") }
                            ));
                        }
                    }
                }
                None => (None, None),
            };

            let a = alerts::create(
                &ctx.alerts,
                &ctx.alerts_path,
                level,
                message.to_string(),
                room_id,
                room_name.clone(),
                source,
            );
            let _ = ctx.ui_tx.send(ToolUiEvent::AlertsChanged);
            let loc = room_name.map(|n| format!(" in {n}")).unwrap_or_default();
            Ok(format!("Raised alert {} [{}]{loc}.", a.id, a.level.label()))
        },
    }
}

fn list_alerts() -> ToolSpec {
    ToolSpec {
        name: "list_alerts",
        description: "List alerts from the registry (id, level, time, location, message, and \
            whether they've been delivered). By default lists today's alerts; pass today_only=false \
            for the full log. Optional filters: level, room (name), undelivered_only.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "today_only": { "type": "boolean", "description": "limit to today (default true)" },
                "level": { "type": "string", "description": "filter by level" },
                "room": { "type": "string", "description": "filter by room name" },
                "undelivered_only": { "type": "boolean", "description": "only alerts not yet delivered" }
            },
            "required": []
        }),
        run: |input, ctx| {
            let today_only = input.get("today_only").and_then(Value::as_bool).unwrap_or(true);
            let level = input.get("level").and_then(Value::as_str).and_then(AlertLevel::parse);
            let room = input.get("room").and_then(Value::as_str).filter(|s| !s.trim().is_empty());
            let undelivered_only = input.get("undelivered_only").and_then(Value::as_bool).unwrap_or(false);

            let alerts = ctx.alerts.lock().unwrap();
            let mut lines = Vec::new();
            for a in alerts.iter().rev() {
                if today_only && !alerts::is_today(&a.created_at) {
                    continue;
                }
                if level.is_some() && Some(a.level) != level {
                    continue;
                }
                if let Some(r) = room {
                    if a.room_name.as_deref().map(|n| !n.eq_ignore_ascii_case(r.trim())).unwrap_or(true) {
                        continue;
                    }
                }
                if undelivered_only && a.delivered() {
                    continue;
                }
                let time = chrono::DateTime::parse_from_rfc3339(&a.created_at)
                    .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
                    .unwrap_or_else(|_| "--:--".into());
                let loc = a.room_name.clone().unwrap_or_else(|| "ship".into());
                let status = if a.delivered() { "delivered" } else { "pending" };
                lines.push(format!(
                    "#{} [{}] {time} {loc}: {} ({status})",
                    a.id, a.level.label(), a.message
                ));
            }
            Ok(if lines.is_empty() { "No matching alerts.".into() } else { lines.join("\n") })
        },
    }
}

fn alert_stats() -> ToolSpec {
    ToolSpec {
        name: "alert_stats",
        description: "Summarize today's alerts: total count, how many are undelivered, the counts \
            by level, and the locations (rooms) with the most alerts.",
        input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
        run: |_input, ctx| {
            let alerts = ctx.alerts.lock().unwrap();
            let today: Vec<&alerts::Alert> =
                alerts.iter().filter(|a| alerts::is_today(&a.created_at)).collect();
            let total = today.len();
            let pending = today.iter().filter(|a| !a.delivered()).count();

            let levels = AlertLevel::ALL
                .iter()
                .map(|l| {
                    let n = today.iter().filter(|a| a.level == *l).count();
                    format!("{} {}", l.label(), n)
                })
                .collect::<Vec<_>>()
                .join(", ");

            // Count by room, then take the top 5 by count.
            let mut by_room: Vec<(String, usize)> = Vec::new();
            for a in &today {
                if let Some(name) = a.room_name.clone() {
                    match by_room.iter_mut().find(|(n, _)| *n == name) {
                        Some((_, c)) => *c += 1,
                        None => by_room.push((name, 1)),
                    }
                }
            }
            by_room.sort_by(|a, b| b.1.cmp(&a.1));
            let rooms = if by_room.is_empty() {
                "none".to_string()
            } else {
                by_room.iter().take(5).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", ")
            };
            Ok(format!(
                "Today: {total} alerts, {pending} undelivered. By level: {levels}. Top locations: {rooms}."
            ))
        },
    }
}

fn deliver_alert() -> ToolSpec {
    ToolSpec {
        name: "deliver_alert",
        description: "Deliver an alert (by its numeric id) to all live beacons, recording who \
            received it and when. A beacon must be paired first.",
        input_schema: json!({
            "type": "object",
            "properties": { "id": { "type": "integer", "description": "the alert id" } },
            "required": ["id"]
        }),
        run: |input, ctx| {
            let id = input.get("id").and_then(Value::as_u64).ok_or("Missing 'id'.")?;
            let cfg = ctx
                .notif_cfg
                .as_ref()
                .ok_or("Notifications are not configured (set the Telegram bot token in Beacons).")?;
            let result = alerts::deliver(&ctx.alerts, &ctx.alerts_path, &ctx.notif, cfg, id)?;
            let _ = ctx.ui_tx.send(ToolUiEvent::AlertsChanged);
            Ok(result)
        },
    }
}

fn delete_alert() -> ToolSpec {
    ToolSpec {
        name: "delete_alert",
        description: "Permanently delete an alert by its numeric id. Only call this AFTER the \
            operator has explicitly confirmed.",
        input_schema: json!({
            "type": "object",
            "properties": { "id": { "type": "integer", "description": "the alert id" } },
            "required": ["id"]
        }),
        run: |input, ctx| {
            let id = input.get("id").and_then(Value::as_u64).ok_or("Missing 'id'.")?;
            if alerts::delete(&ctx.alerts, &ctx.alerts_path, id) {
                let _ = ctx.ui_tx.send(ToolUiEvent::AlertsChanged);
                Ok(format!("Deleted alert {id}."))
            } else {
                Err(format!("Alert {id} not found."))
            }
        },
    }
}

/// Re-encode arbitrary snapshot bytes (JPEG over HTTP, or a frame from the RTSP
/// fallback) to JPEG, which is what both Telegram's `sendPhoto` and the chat
/// `<img>` data URL expect.
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

    fn ctx(tx: Sender<ToolUiEvent>) -> ToolContext {
        ToolContext {
            devices: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            devices_path: std::env::temp_dir().join("app_core_chat_devices.json"),
            alerts: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            alerts_path: std::env::temp_dir().join("app_core_chat_alerts.json"),
            notif: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            map_path: std::env::temp_dir().join("app_core_chat_map.json"),
            notif_cfg: None,
            ui_tx: tx,
        }
    }

    #[test]
    fn unknown_tool_errors_known_runs() {
        let reg = ToolRegistry::new();
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = ctx(tx);
        assert!(reg.run("nope", &json!({}), &ctx).is_err());
        // list_devices always succeeds, even with an empty store
        assert!(reg.run("list_devices", &json!({}), &ctx).is_ok());
    }

    #[test]
    fn notify_without_config_errors() {
        let reg = ToolRegistry::new();
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = ctx(tx);
        // No telegram config → notify_beacon path errors cleanly (not a panic).
        assert!(reg.run("send_notification", &json!({ "message": "hi" }), &ctx).is_err());
    }
}
