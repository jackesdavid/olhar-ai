use super::{PortValue, PortValues};
use crate::agent::tools::{ToolContext, ToolUiEvent};
use serde_json::Value;

/// Record a categorized presence in a room: persist it and pulse it live on the map.
/// Inputs override config; the room name resolves to a room_id like Create Alert.
pub(super) fn run_record_presence(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
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
        .and_then(PortValue::as_camera)
        .map(|c| c.id.clone());

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

    let detect_pos: Option<(f32, f32)> = (|| {
        let bbox = inputs.get("bbox").and_then(PortValue::as_bbox)?;
        let x1 = bbox.x1 as f32;
        let y1 = bbox.y1 as f32;
        let x2 = bbox.x2 as f32;
        let y2 = bbox.y2 as f32;
        let fw = inputs.get("frame_w").and_then(PortValue::as_num)? as f32;
        let fh = inputs.get("frame_h").and_then(PortValue::as_num)? as f32;
        if fw <= 0.0 || fh <= 0.0 { return None; }
        let cx = (x1 + x2) / 2.0 / fw - 0.5;
        let cy = (y1 + y2) / 2.0 / fh;
        let depth = 1.0 - cy;
        Some((cx.clamp(-0.5, 0.5), depth.clamp(0.0, 1.0)))
    })();
    let detect_x = detect_pos.map(|(x, _)| x);
    let detect_depth = detect_pos.map(|(_, d)| d);

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
        detect_x,
        detect_depth,
    );

    // Mirror the presence onto the agnostic event stream (card #66, Phase 3) so the map pulse and
    // the future time-travel timeline share one source. Anchored at the presence's own moment.
    let mut ev = crate::events::NewEvent::new(sys.tenant_id, "presence", "workflow");
    ev.occurred_at = p.created_at.clone();
    ev.camera = camera.clone();
    ev.room_id = room_id.map(|r| r as i64);
    ev.room_name = room_name.clone();
    ev.message = Some(match (&label, &room_name) {
        (Some(l), Some(r)) => format!("{} ({}) in {}", category.slug(), l, r),
        (Some(l), None) => format!("{} ({})", category.slug(), l),
        (None, Some(r)) => format!("{} in {}", category.slug(), r),
        (None, None) => category.slug().to_string(),
    });
    ev.data = Some(serde_json::json!({
        "category": category.slug(),
        "label": label,
        "duration_secs": duration_secs,
    }));
    crate::events::insert_event_sync(&sys.db, ev);

    let _ = sys.ui_tx.send(ToolUiEvent::Presence {
        category: category.slug().to_string(),
        room_id,
        room_name,
        label,
        camera,
        duration_secs,
        created_at: p.created_at.clone(),
        detect_x,
        detect_depth,
    });

    let mut o = PortValues::new();
    o.insert("id".into(), PortValue::Num(p.id as i64));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}
