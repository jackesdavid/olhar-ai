//! The notification logic: the background pairing poller, the provider- and
//! UI-agnostic core [`dispatch`] (the single send entry point), and the beacon
//! management helpers (rename / toggle / delete by `chat_id`).
//!
//! Copied from the ratatui `src/notification/handler.rs`. The ratatui
//! `BeaconsScreen` / `BeaconDetail` / `BeaconField` view state is DROPPED — the
//! web app drives those from Svelte. Persistence is path-parameterized so the
//! poller writes under the OS app-data dir.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::client::{self, Update};
use super::domain::{NotificationConfig, NotificationStore, NotificationTarget};
use crate::db::Db;

/// Match an inbound message against the pairing code. Accepts either
/// `/pair <CODE>` or the bare `<CODE>`, trimmed and exact (case-sensitive).
pub fn parse_pairing_code(text: &str, code: &str) -> bool {
    if code.trim().is_empty() {
        return false;
    }
    let t = text.trim();
    let candidate = t
        .strip_prefix("/pair")
        .or_else(|| t.strip_prefix("/start"))
        .map(str::trim_start)
        .unwrap_or(t)
        .trim();
    candidate == code
}

/// Resolve which target a send should go to: a named target (case-insensitive),
/// or the sole enabled target, else an error listing what's available.
pub fn resolve_target<'a>(
    targets: &'a [NotificationTarget],
    target: Option<&str>,
) -> Result<&'a NotificationTarget, String> {
    let live: Vec<&NotificationTarget> = targets.iter().filter(|t| t.enabled).collect();
    if live.is_empty() {
        return Err("No notification targets are paired. Send the pairing code to the \
                    Telegram bot first."
            .into());
    }
    match target {
        Some(name) => live
            .iter()
            .copied()
            .find(|t| t.label.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| format!("No target named '{}'. Available: {}.", name.trim(), label_list(&live))),
        None if live.len() == 1 => Ok(live[0]),
        None => Err(format!("Which target? Available: {}.", label_list(&live))),
    }
}

fn label_list(live: &[&NotificationTarget]) -> String {
    live.iter().map(|t| t.label.as_str()).collect::<Vec<_>>().join(", ")
}

/// The single send entry point used by ALL consumers. Resolves the target, sends
/// text or text+photo, and returns a human confirmation string. Free of
/// agent/UI dependencies.
pub fn dispatch(
    store: &NotificationStore,
    cfg: &NotificationConfig,
    target: Option<&str>,
    message: &str,
    image: Option<Vec<u8>>,
) -> Result<String, String> {
    if message.trim().is_empty() && image.is_none() {
        return Err("Nothing to send — provide a message or an image.".into());
    }
    // Resolve under the lock, clone out what we need, release before the IO.
    let (chat_id, label) = {
        let targets = store.lock().unwrap();
        let t = resolve_target(&targets, target)?;
        (t.chat_id, t.label.clone())
    };
    match image {
        Some(jpeg) => client::send_photo(cfg, chat_id, message, &jpeg)?,
        None => client::send_message(cfg, chat_id, message)?,
    }
    let what = if message.trim().is_empty() { "snapshot" } else { "message" };
    Ok(format!("Sent {what} to '{label}'."))
}

/// Spawn ONE inbound pairing poller for a bot token that is shared by one or more tenants.
///
/// Telegram permits only a single `getUpdates` long-poll per bot token — a second concurrent poller
/// on the same token gets HTTP 409 and the two race, splitting/dropping incoming messages (the
/// "only some Telegram messages reach the workflow" bug). So when several tenants resolve to the
/// same token (e.g. both fall back to the operator's global bot), they must share ONE poller. This
/// poller holds a beacon store per tenant and routes each update:
///   * a **pairing code** registers the chat to the tenant whose code matched;
///   * a **regular message** fans out to `enqueue_telegram_message` for every tenant — a no-op for
///     any tenant that hasn't paired that chat, so the message only reaches the owning tenant's
///     graphs (the security boundary is preserved per-tenant).
///
/// `cfg.pairing_code` is unused here — each tenant's code is passed in `tenants`. Never panics; on
/// any error it backs off and retries.
pub fn spawn_pairing_bot(
    cfg: NotificationConfig,
    db: Db,
    tenants: Vec<(uuid::Uuid, String)>, // (tenant_id, pairing_code) sharing this token
    stop: Arc<AtomicBool>,
) {
    if tenants.is_empty() {
        return;
    }
    thread::spawn(move || {
        // One in-memory beacon store per tenant sharing this bot, seeded from Postgres. Kept live by
        // `register_target` as chats pair, so subsequent regular messages route to the right tenant.
        let stores: std::collections::HashMap<uuid::Uuid, NotificationStore> = tenants
            .iter()
            .map(|(t, _)| (*t, Arc::new(std::sync::Mutex::new(super::repo::load_beacons(&db, *t)))))
            .collect();
        let mut offset: i64 = 0; // 0 = take the current backlog once
        let mut err_streak: u32 = 0;
        loop {
            if stop.load(Ordering::Relaxed) {
                break; // a newer config replaced us
            }
            // 15s long-poll so the stop flag is checked promptly on a config change.
            match client::get_updates(&cfg, offset, 15) {
                Ok(updates) => {
                    err_streak = 0;
                    for u in &updates {
                        offset = offset.max(u.update_id + 1); // ack
                        // 1. Pairing: the tenant whose code matches this message claims the chat.
                        let paired_tenant = tenants
                            .iter()
                            .find(|(_, code)| parse_pairing_code(&u.text, code))
                            .map(|(t, _)| *t);
                        if let Some(tenant_id) = paired_tenant {
                            if let Some(store) = stores.get(&tenant_id) {
                                if register_target(store, Some(&db), tenant_id, u) {
                                    let _ = client::send_message(
                                        &cfg,
                                        u.chat_id,
                                        &format!(
                                            "\u{2705} Paired as {}. You'll receive alerts here.",
                                            u.sender_label
                                        ),
                                    );
                                }
                            }
                            continue;
                        }
                        // 2. Regular message → fire it onto the durable event_queue as a
                        // `telegram_message` trigger (card #15) for whichever tenant paired the chat.
                        // Flow's event_consumer claims it and runs that tenant's
                        // `On event("telegram_message")` graphs. `enqueue_telegram_message` no-ops for
                        // tenants that haven't paired this chat, so a message never crosses tenants.
                        for (tenant_id, _) in &tenants {
                            if let Some(store) = stores.get(tenant_id) {
                                enqueue_telegram_message(store, &db, *tenant_id, u);
                            }
                        }
                    }
                }
                Err(e) => {
                    // A persistent getUpdates failure (e.g. HTTP 409 when a webhook is set, or a
                    // bad token) used to be totally silent. Surface it, but throttle: first
                    // occurrence, then every ~12th retry (~once/minute at the 5s backoff).
                    if err_streak == 0 || err_streak % 12 == 0 {
                        let tenants_dbg: Vec<uuid::Uuid> = tenants.iter().map(|(t, _)| *t).collect();
                        log::warn!("pairing getUpdates failing for bot (tenants {tenants_dbg:?}): {e}");
                    }
                    err_streak = err_streak.saturating_add(1);
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(Duration::from_secs(5));
                }
            }
        }
    });
}

/// Split a message into `(command, args)`. The command is a leading slash-word (`/snap`) —
/// lower-cased — else empty; `args` is everything after it (or the whole trimmed text when
/// there's no command). Pure + testable.
pub fn split_command(text: &str) -> (String, String) {
    let t = text.trim();
    match t.strip_prefix('/') {
        Some(rest) => {
            let mut it = rest.splitn(2, char::is_whitespace);
            let cmd = it.next().unwrap_or("");
            let args = it.next().unwrap_or("").trim();
            (format!("/{}", cmd.to_lowercase()), args.to_string())
        }
        None => (String::new(), t.to_string()),
    }
}

/// Enqueue a paired chat's message as a `telegram_message` event. No-op for chats this tenant
/// hasn't paired (an enabled target must match `chat_id`) — the security boundary that keeps a
/// message from ever reaching another tenant's graphs.
fn enqueue_telegram_message(store: &NotificationStore, db: &Db, tenant_id: uuid::Uuid, u: &Update) {
    let paired = {
        let targets = store.lock().unwrap();
        targets.iter().any(|t| t.enabled && t.chat_id == u.chat_id)
    };
    if !paired {
        return;
    }
    let (command, args) = split_command(&u.text);
    let payload = serde_json::json!({
        "text": u.text,
        "chat_id": u.chat_id,
        "from_name": u.sender_label,
        "command": command,
        "args": args,
    });
    crate::work_queue::enqueue_event_blocking(db, tenant_id, "telegram_message", "telegram", &payload);
}

/// Register a paired chat if not already present. Returns true if newly added.
fn register_target(store: &NotificationStore, db: Option<&Db>, tenant_id: uuid::Uuid, u: &Update) -> bool {
    let mut targets = store.lock().unwrap();
    if targets.iter().any(|t| t.chat_id == u.chat_id) {
        return false; // already paired — idempotent
    }
    let label = dedupe_label(&targets, &u.sender_label);
    targets.push(NotificationTarget { label, chat_id: u.chat_id, enabled: true });
    if let Some(db) = db {
        super::repo::save_beacons(db, tenant_id, &targets);
    }
    true
}

/// Keep labels unique so `resolve_target` stays unambiguous: "Dave", "Dave 2", …
fn dedupe_label(targets: &[NotificationTarget], base: &str) -> String {
    let base = if base.trim().is_empty() { "operator" } else { base.trim() };
    if !targets.iter().any(|t| t.label == base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} {n}")).find(|c| !targets.iter().any(|t| &t.label == c)).unwrap()
}

// ---- beacon management (drives the web Beacons screen) ----------------------

/// Remove the beacon identified by `chat_id` and persist to `path`. Returns the
/// removed label, or `None` if no beacon matched.
pub fn delete_target(store: &NotificationStore, db: Option<&Db>, tenant_id: uuid::Uuid, chat_id: i64) -> Option<String> {
    let mut targets = store.lock().unwrap();
    let pos = targets.iter().position(|t| t.chat_id == chat_id)?;
    let removed = targets.remove(pos);
    if let Some(db) = db {
        super::repo::save_beacons(db, tenant_id, &targets);
    }
    Some(removed.label)
}

/// Commit a renamed label for the beacon identified by `chat_id` and persist to
/// `path`. Empty names are rejected. Returns the committed label on success.
pub fn rename_target(
    store: &NotificationStore,
    db: Option<&Db>,
    tenant_id: uuid::Uuid,
    chat_id: i64,
    new_label: &str,
) -> Result<String, String> {
    let label = new_label.trim().to_string();
    if label.is_empty() {
        return Err("Name can't be empty.".into());
    }
    let mut targets = store.lock().unwrap();
    let t = targets.iter_mut().find(|t| t.chat_id == chat_id).ok_or("Beacon not found.")?;
    t.label = label.clone();
    if let Some(db) = db {
        super::repo::save_beacons(db, tenant_id, &targets);
    }
    Ok(label)
}

/// Set the `enabled` flag for the beacon identified by `chat_id` and persist to
/// `path`. Returns `(label, new_state)`, or `None` if no beacon matched.
pub fn set_enabled_by_id(
    store: &NotificationStore,
    db: Option<&Db>,
    tenant_id: uuid::Uuid,
    chat_id: i64,
    enabled: bool,
) -> Option<(String, bool)> {
    let mut targets = store.lock().unwrap();
    let t = targets.iter_mut().find(|t| t.chat_id == chat_id)?;
    t.enabled = enabled;
    let out = (t.label.clone(), t.enabled);
    if let Some(db) = db {
        super::repo::save_beacons(db, tenant_id, &targets);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn target(label: &str, chat_id: i64, enabled: bool) -> NotificationTarget {
        NotificationTarget { label: label.into(), chat_id, enabled }
    }

    #[test]
    fn pairing_code_matches_prefixed_and_bare() {
        assert!(parse_pairing_code("/pair SECRET", "SECRET"));
        assert!(parse_pairing_code("  SECRET ", "SECRET"));
        assert!(parse_pairing_code("/pair   SECRET", "SECRET"));
        assert!(parse_pairing_code("/start SECRET", "SECRET")); // deep-link payload
        assert!(parse_pairing_code("/start   SECRET", "SECRET"));
        assert!(!parse_pairing_code("nope", "SECRET"));
        assert!(!parse_pairing_code("SECRET", "")); // empty code never matches
    }

    #[test]
    fn resolve_picks_named_sole_and_reports_choices() {
        assert!(resolve_target(&[], None).is_err());
        let one = vec![target("Dave", 1, true)];
        assert_eq!(resolve_target(&one, None).unwrap().label, "Dave");
        let many = vec![target("Dave", 1, true), target("Ripley", 2, true)];
        assert!(resolve_target(&many, None).is_err()); // ambiguous
        assert_eq!(resolve_target(&many, Some("ripley")).unwrap().label, "Ripley"); // case-insens
        assert!(resolve_target(&many, Some("nobody")).is_err());
        // disabled targets are excluded
        let off = vec![target("Dave", 1, false)];
        assert!(resolve_target(&off, None).is_err());
    }

    #[test]
    fn split_command_parses_slash_and_plain() {
        assert_eq!(split_command("/snap cam1"), ("/snap".into(), "cam1".into()));
        assert_eq!(split_command("  /Snap  cam1 cam2 "), ("/snap".into(), "cam1 cam2".into()));
        assert_eq!(split_command("/status"), ("/status".into(), "".into()));
        assert_eq!(split_command("hello there"), ("".into(), "hello there".into()));
        assert_eq!(split_command("  "), ("".into(), "".into()));
    }

    #[test]
    fn dedupe_label_suffixes_collisions() {
        let existing = vec![target("Dave", 1, true)];
        assert_eq!(dedupe_label(&existing, "Dave"), "Dave 2");
        assert_eq!(dedupe_label(&existing, "Ripley"), "Ripley");
        assert_eq!(dedupe_label(&[], ""), "operator");
    }

    #[test]
    fn rename_toggle_delete_mutate_the_store() {
        // `None` for the DB keeps the test hermetic — exercises the in-memory store
        // semantics (find-by-chat_id, rename, toggle, remove). The Postgres round-trip
        // is covered by the app's live verification, not a unit test.
        let store: NotificationStore =
            Arc::new(Mutex::new(vec![target("Dave", 42, true), target("Ripley", 7, true)]));

        let t = uuid::Uuid::nil();
        assert_eq!(rename_target(&store, None, t, 42, "Phone").unwrap(), "Phone");
        assert!(rename_target(&store, None, t, 42, "  ").is_err());
        assert_eq!(set_enabled_by_id(&store, None, t, 7, false), Some(("Ripley".into(), false)));
        assert_eq!(delete_target(&store, None, t, 42), Some("Phone".into()));
        assert!(delete_target(&store, None, t, 999).is_none());

        let remaining = store.lock().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].chat_id, 7);
        assert!(!remaining[0].enabled);
    }
}
