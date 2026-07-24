# Recipient Gate — human-in-the-loop approval node (PLAN ONLY)

**Status:** planned, not started. **Depends on:** device-as-recipient registration (below) being real
first — a Gate that targets a phone is meaningless until a phone is a first-class, manageable
Recipient. **Board card:** _(to create)_

## The idea (operator's words)

A workflow node that sends a notification to a device and **waits for the person to decide**. The app
shows an actionable notification / an in-app card with an allow-or-deny control (button or slide):

- the person **allows** → the node's **True** branch fires;
- the person **denies**, or a **timeout** is reached → the **False** branch fires.

So it is a `Branch`, but the condition is a human on a phone. "Only arm the siren if the owner
approves." "Ask before unlocking." "Confirm this automation."

---

## Prerequisite — device-as-recipient registration (do this FIRST)

Today a phone auto-registers on login with a label derived from the device name ("iPhone"), and
`NotifyRecipient` merges push devices into the recipient picker (card #139). That is enough to *send*
to a phone, but not enough to *address one deliberately* in a Gate. Before Recipient Gate, solidify:

- **A management surface** (web + app): list this tenant's registered devices, **rename the label**
  (the label is the addressing key — "iPhone" is a poor name to pick from a dropdown months later),
  enable/disable, remove. Backend already has `push::list/register/delete` + `enabled`; missing is a
  rename/update and the UI.
- **Recipient identity clarity**: decide whether a device is its own Recipient (current model, card
  #139 decision) or a channel under a named Recipient. Recipient *groups* are already flagged as
  near-future. The Gate targets whatever this settles on.
- **Multiple devices per recipient**: define first-responder-wins (the first device to answer
  resolves the Gate; others' actions become no-ops).

This prerequisite is its own card; Recipient Gate builds on it.

---

## Why the engine already supports the hard part

Verified in `crates/app-core/src/workflows/engine.rs`: the suspend-with-timeout-then-resume pattern
already exists and drives `DelayTrigger`.

- **Suspend + timeout:** `repo::try_arm_node_state(...expires_at)` persists per-instance node state,
  and `work_queue::enqueue_window_stop_blocking(...)` schedules a timer job that fires a **window-stop
  run** to resume the instance when the deadline passes. That timer *is* the timeout → False branch.
- **Resume on external input:** the events system runs the subtree of a matching `OnEvent` as a
  decoupled sub-run seeded with the event payload (`engine::run subtree from entry`,
  `on_event_node`). The phone's answer arrives as an event that resolves the pending Gate.
- **Two-way branch:** `If` ("Branch") already models True/False exec outputs; the Gate is the same
  output shape with a human condition.

So no engine rewrite — the riskiest piece is the one already proven in production.

---

## Architecture — 5 parts

### 1. Node — `NodeKind::RecipientGate`
`crates/app-core/src/workflows/domain.rs` (add to `NodeKind::ALL`) + `node.rs`.
- **Inputs:** `exec` (in), `recipient` (Recipient), `message`/`prompt` (Text), `timeout_secs`
  (Num, config, default e.g. 60).
- **Exec outputs:** `approved` (True), `denied` (False — also the timeout path).
- **Data output:** `respondedBy` (Text — which device/user answered; empty on timeout).
- Execution is engine-driven like `DelayTrigger` (returns empty PortValues; the engine arms state,
  sends the push, schedules the timeout). It does NOT resolve inline — it suspends.

### 2. Migration `0071_approval_requests.sql`
(next number — confirmed `ls`, current tail is 0070). The pending store, RFC3339 `text` timestamps
per project rule:
```
approval_requests(
  id uuid pk default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  graph_id text not null, node_id int not null, instance_key text not null, run_id text,
  recipient_label text not null,
  status text not null default 'pending',   -- pending | approved | denied | timeout
  responded_by text, responded_at text,
  created_at text not null, expires_at text not null
);
-- unique(tenant_id, graph_id, node_id, instance_key) so one live request per gate instance
```

### 3. Control — response endpoint
`sentinel-control/src/domain/approvals.rs`:
- `POST /approvals/:id/respond` (auth = user JWT, gated on a recipient/beacons perm) body
  `{ decision: "allow" | "deny" }`. Resolves the row (first responder wins; a second call is a
  no-op), records `responded_by`, and **triggers the resume run** — fire the Gate's `approved` or
  `denied` subtree via the same event/sub-run path `sentinel-flow` uses.
- `GET /approvals?status=pending` — the app's list of things awaiting the operator.
- **Timeout side:** the window-stop job, on firing, checks the row is still `pending` → sets
  `timeout` and resolves the **denied** branch. Whoever resolves first (human vs timer) wins; the
  loser is a no-op.

### 4. Push — actionable payload
`crates/app-core/src/notification/push.rs`: extend the send to carry a `data` blob with
`approval_id` and a category id (e.g. `RECIPIENT_GATE`). Expo push supports `categoryId` + `data`;
iOS maps the category to notification actions (Allow / Deny) registered in the app.

### 5. Mobile — the approval UX
- **Notification category** `RECIPIENT_GATE` with two actions (Allow / Deny) registered via
  `expo-notifications` `setNotificationCategoryAsync`. Background taps hit
  `POST /approvals/:id/respond` directly from the action handler.
- **Foreground / in-app card**: a slide-to-allow control (the operator's ask) plus a Deny button,
  shown when the app is open or when the notification is tapped. A **Pending Approvals** list
  (from `GET /approvals`) so a missed notification is still actionable.
- Reuse the session `api()` and the tokens already wired in card #139.

---

## Open decisions
1. **Node name / verbs**: "Recipient Gate" vs "Ask Recipient" vs "Approval Gate"; output labels
   `approved/denied` vs `allowed/denied` vs `true/false`. Operator leaned "Recipient Gate".
2. **Timeout = denied, or a third `timeout` exec output?** Operator said timeout → False. Simpler is
   two outputs (timeout folds into denied); a distinct third output is possible if a workflow needs
   to tell "said no" from "didn't answer".
3. **Re-arm / dedup**: if the same gate instance fires again while a request is pending, top up vs
   ignore (DelayTrigger tops up its window — mirror that, or treat pending as exclusive).
4. **Auth of the responder**: any user in the tenant, or only the device's owning user? Ties to the
   device-recipient identity decision above.
5. **Audit**: record approvals on the product `events` stream (who approved what, when) — almost
   certainly yes, it is a security-relevant action.

## Out of scope (v1)
- Multi-approver quorum ("2 of 3 must allow"). Single first-responder resolves.
- Rich payloads (a snapshot image in the approval card) — text prompt first; the push image path
  from #139 can layer on later.
- Web-side approval UI — the Gate targets phones; a browser approver is a later card.
