//! The notification logic: the background pairing poller, the provider- and
//! UI-agnostic core [`dispatch`] (the single send entry point shared by the
//! Bishop tool today and the workflows engine tomorrow), and the pure helpers
//! they build on.
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

/// The single send entry point used by ALL consumers (the Bishop tool today, the
/// workflows engine tomorrow). Resolves the target, sends text or text+photo,
/// and returns a human confirmation string. Free of agent/UI dependencies.
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

/// Spawn the inbound pairing poller. Long-polls getUpdates; when a message
/// carries the pairing code from an unknown chat, registers it as a trusted
/// target, persists it, and replies to confirm. Wrong codes / unpaired chats are
/// ignored. Never panics; on any error it backs off and retries.
pub fn spawn_pairing(store: NotificationStore, cfg: NotificationConfig) {
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
                        if register_target(&store, u) {
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
fn register_target(store: &NotificationStore, u: &Update) -> bool {
    let mut targets = store.lock().unwrap();
    if targets.iter().any(|t| t.chat_id == u.chat_id) {
        return false; // already paired — idempotent
    }
    let label = dedupe_label(&targets, &u.sender_label);
    targets.push(NotificationTarget { label, chat_id: u.chat_id, enabled: true });
    save_targets(&targets);
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

// ---- ALERTS management screen ----------------------------------------------

/// Transient UI state for the ALERTS screen: the highlighted row plus a one-line
/// status (the target data itself lives in the shared [`NotificationStore`]).
pub struct AlertsScreen {
    pub selected: usize,
    pub status: String,
}

impl AlertsScreen {
    pub fn new() -> Self {
        AlertsScreen { selected: 0, status: String::new() }
    }

    pub fn next(&mut self, len: usize) {
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }

    pub fn prev(&mut self, len: usize) {
        if len > 0 {
            self.selected = (self.selected + len - 1) % len;
        }
    }

    /// Keep the cursor in range after a deletion.
    pub fn clamp(&mut self, len: usize) {
        self.selected = if len == 0 { 0 } else { self.selected.min(len - 1) };
    }
}

impl Default for AlertsScreen {
    fn default() -> Self {
        Self::new()
    }
}

/// Flip a target's `enabled` flag and persist. Returns `(label, new_state)`.
pub fn toggle_enabled(store: &NotificationStore, idx: usize) -> Option<(String, bool)> {
    let mut targets = store.lock().unwrap();
    let t = targets.get_mut(idx)?;
    t.enabled = !t.enabled;
    let out = (t.label.clone(), t.enabled);
    save_targets(&targets);
    Some(out)
}

/// Remove a target by index and persist. Returns the removed label.
pub fn delete_target(store: &NotificationStore, idx: usize) -> Option<String> {
    let mut targets = store.lock().unwrap();
    if idx >= targets.len() {
        return None;
    }
    let removed = targets.remove(idx);
    save_targets(&targets);
    Some(removed.label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(label: &str, enabled: bool) -> NotificationTarget {
        NotificationTarget { label: label.into(), chat_id: 1, enabled }
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
        let one = vec![target("Dave", true)];
        assert_eq!(resolve_target(&one, None).unwrap().label, "Dave");
        let many = vec![target("Dave", true), target("Ripley", true)];
        assert!(resolve_target(&many, None).is_err()); // ambiguous
        assert_eq!(resolve_target(&many, Some("ripley")).unwrap().label, "Ripley"); // case-insens
        assert!(resolve_target(&many, Some("nobody")).is_err());
        // disabled targets are excluded
        let off = vec![target("Dave", false)];
        assert!(resolve_target(&off, None).is_err());
    }

    #[test]
    fn dedupe_label_suffixes_collisions() {
        let existing = vec![target("Dave", true)];
        assert_eq!(dedupe_label(&existing, "Dave"), "Dave 2");
        assert_eq!(dedupe_label(&existing, "Ripley"), "Ripley");
        assert_eq!(dedupe_label(&[], ""), "operator");
    }
}
