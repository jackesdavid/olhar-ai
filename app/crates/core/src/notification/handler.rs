//! The notification logic: the background pairing poller, the provider- and
//! UI-agnostic core [`dispatch`] (the single send entry point), and the beacon
//! management helpers (rename / toggle / delete by `chat_id`).
//!
//! Copied from the ratatui `src/notification/handler.rs`. The ratatui
//! `BeaconsScreen` / `BeaconDetail` / `BeaconField` view state is DROPPED — the
//! web app drives those from Svelte. Persistence is path-parameterized so the
//! poller writes under the OS app-data dir.
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use super::client::{self, Update};
use super::domain::{save_targets, NotificationConfig, NotificationStore, NotificationTarget};

/// Match an inbound message against the pairing code. Accepts either
/// `/pair <CODE>` or the bare `<CODE>`, trimmed and exact (case-sensitive).
pub fn parse_pairing_code(text: &str, code: &str) -> bool {
    if code.trim().is_empty() {
        return false;
    }
    let t = text.trim();
    let candidate = t.strip_prefix("/pair").map(str::trim_start).unwrap_or(t).trim();
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

/// Spawn the inbound pairing poller on a background thread. Long-polls
/// getUpdates; when a message carries the pairing code from an unknown chat,
/// registers it as a trusted target, persists it to `path`, and replies to
/// confirm. Wrong codes / unpaired chats are ignored. Never panics; on any error
/// it backs off and retries.
pub fn spawn_pairing(store: NotificationStore, cfg: NotificationConfig, path: PathBuf) {
    if !cfg.can_pair() {
        return; // no pairing code → accept no pairings
    }
    thread::spawn(move || {
        let mut offset: i64 = 0; // 0 = take the current backlog once
        loop {
            match client::get_updates(&cfg, offset, 30) {
                Ok(updates) => {
                    for u in &updates {
                        offset = offset.max(u.update_id + 1); // ack
                        if !parse_pairing_code(&u.text, &cfg.pairing_code) {
                            continue;
                        }
                        if register_target(&store, &path, u) {
                            let _ = client::send_message(
                                &cfg,
                                u.chat_id,
                                &format!(
                                    "\u{2705} Paired as {}. You'll receive Bishop's alerts here.",
                                    u.sender_label
                                ),
                            );
                        }
                    }
                }
                Err(_) => thread::sleep(Duration::from_secs(5)),
            }
        }
    });
}

/// Register a paired chat if not already present. Returns true if newly added.
fn register_target(store: &NotificationStore, path: &Path, u: &Update) -> bool {
    let mut targets = store.lock().unwrap();
    if targets.iter().any(|t| t.chat_id == u.chat_id) {
        return false; // already paired — idempotent
    }
    let label = dedupe_label(&targets, &u.sender_label);
    targets.push(NotificationTarget { label, chat_id: u.chat_id, enabled: true });
    save_targets(path, &targets);
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
pub fn delete_target(store: &NotificationStore, path: &Path, chat_id: i64) -> Option<String> {
    let mut targets = store.lock().unwrap();
    let pos = targets.iter().position(|t| t.chat_id == chat_id)?;
    let removed = targets.remove(pos);
    save_targets(path, &targets);
    Some(removed.label)
}

/// Commit a renamed label for the beacon identified by `chat_id` and persist to
/// `path`. Empty names are rejected. Returns the committed label on success.
pub fn rename_target(
    store: &NotificationStore,
    path: &Path,
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
    save_targets(path, &targets);
    Ok(label)
}

/// Set the `enabled` flag for the beacon identified by `chat_id` and persist to
/// `path`. Returns `(label, new_state)`, or `None` if no beacon matched.
pub fn set_enabled_by_id(
    store: &NotificationStore,
    path: &Path,
    chat_id: i64,
    enabled: bool,
) -> Option<(String, bool)> {
    let mut targets = store.lock().unwrap();
    let t = targets.iter_mut().find(|t| t.chat_id == chat_id)?;
    t.enabled = enabled;
    let out = (t.label.clone(), t.enabled);
    save_targets(path, &targets);
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
    fn dedupe_label_suffixes_collisions() {
        let existing = vec![target("Dave", 1, true)];
        assert_eq!(dedupe_label(&existing, "Dave"), "Dave 2");
        assert_eq!(dedupe_label(&existing, "Ripley"), "Ripley");
        assert_eq!(dedupe_label(&[], ""), "operator");
    }

    #[test]
    fn rename_toggle_delete_round_trip_through_path() {
        let dir = std::env::temp_dir().join("app_core_notif_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("notifications.json");
        let _ = std::fs::remove_file(&path);

        let store: NotificationStore =
            Arc::new(Mutex::new(vec![target("Dave", 42, true), target("Ripley", 7, true)]));
        save_targets(&path, &store.lock().unwrap());

        assert_eq!(rename_target(&store, &path, 42, "Phone").unwrap(), "Phone");
        assert!(rename_target(&store, &path, 42, "  ").is_err());
        assert_eq!(set_enabled_by_id(&store, &path, 7, false), Some(("Ripley".into(), false)));
        assert_eq!(delete_target(&store, &path, 42), Some("Phone".into()));
        assert!(delete_target(&store, &path, 999).is_none());

        let reloaded = super::super::domain::load_targets(&path);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].chat_id, 7);
        assert!(!reloaded[0].enabled);
        let _ = std::fs::remove_file(&path);
    }
}
