# Telegram Bot — Two-Way Interaction Use Cases (Research)

**Board card:** Telegram bot: two-way use cases research (ToDo)
**Goal:** Survey realistic use cases for Sentinel using the Telegram bot in both directions — outbound (notify user) and inbound (user commands the app) — and assess what's worth building.

---

## Research questions

1. What inbound commands would be genuinely useful in a security/monitoring context?
2. Which outbound notification patterns go beyond a plain text/image alert?
3. What do leading camera/security SaaS apps do with Telegram bots?
4. What Telegram Bot API features (inline keyboards, callback queries, commands menu, `/start` deep links) would lift the experience?
5. What are the privacy/security implications of inbound commands (bot as a control channel)?

---

## Candidate outbound patterns (notify → user)

| Pattern | Description | Telegram API |
|---------|-------------|-------------|
| Motion alert with snapshot | Current: text + JPEG | sendPhoto |
| Presence pulse | "Dave arrived home at 14:32" | sendMessage |
| Workflow completion | "Backup completed ✓ (3 min)" | sendMessage |
| Daily digest | Scheduled summary of events | sendMessage |
| Alert with snooze button | Inline button "Snooze 1h" | sendMessage + InlineKeyboardMarkup |
| Tiered severity | 🔴🟡🟢 colour-coded alerts | sendMessage (emoji) |
| Live snapshot on demand | Triggered by user command | sendPhoto |

---

## Candidate inbound patterns (user → app)

| Command | What it does | Risk |
|---------|-------------|------|
| `/snapshot <camera>` | Returns a live snapshot | Low |
| `/status` | Lists connected cameras + health | Low |
| `/arm` / `/disarm` | Enables/disables motion detection globally | Medium — needs confirmation |
| `/snooze <minutes>` | Suppresses alerts for N minutes | Medium |
| `/workflow <name>` | Triggers a named workflow manually | High — arbitrary execution |
| `/alert list` | Lists last 10 alerts | Low |
| Inline reply "Snooze 1h" | Callback query from inline button in prior alert | Low — scoped to alert |

---

## Open design questions

1. **Authorization**: inbound commands can only come from already-paired chat_ids — is that enough, or do we need per-command permission levels?
2. **Command routing**: a single bot token serves all tenants; how does the bot know which tenant a chat_id belongs to? (Answer: `notification_targets` maps chat_id → tenant_id — already in the DB.)
3. **Callback queries**: inline buttons require the bot to stay listening even outside the pairing window — does the existing long-poll setup handle `callback_query` update types?
4. **Stateful conversations**: multi-step commands (e.g. "which camera?" follow-up) require session state — is that worth the complexity?
5. **Scope for v1 inbound**: narrowest useful slice is probably `/snapshot` + inline "Snooze" reply on alerts.

---

## Recommended next steps (from research)

After this research card, propose a focused "Telegram inbound commands v1" card covering:
- `/snapshot <camera>` — safe, high value, easy to implement
- Inline "Snooze Xh" keyboard button on motion alerts — closes the notification loop without user leaving Telegram

Everything else (arm/disarm, workflow triggers) deferred — higher risk, lower daily use.

---

## Out of scope

- Replacing the web UI with a bot-only interface
- Telegram groups / channels
- Broadcast to all paired users simultaneously (multi-cast)
