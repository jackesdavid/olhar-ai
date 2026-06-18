---

kanban-plugin: board

---

## Bugs

- [ ] Splash should retry on transient `boot_status` DB errors instead of hanging
- [ ] We should encrypt the passwords in the db column level too


## Backlog

- [ ] School Student Frequency Case
	  - Plan: [[school-student-frequency]]
	  - Use case: [[school-frequence]]
- [ ] Box re-key / rotate enrollment action (Boxes page)
	  - Re-mint a reusable Tailscale auth key for an existing box **without recreating it** (keep box id/token/cameras); push the new key into `edge.env` + restart. Optionally disable node-key expiry so appliances survive past the ~90-day key lifetime.
	  - Why: enrollment keys now mint reusable + non-ephemeral + 90d (`sentinel-control/src/tailscale.rs`, fixed `20814c1`), so reboots recover — but after 90d a box still needs re-enrollment, and today the only recovery is delete + recreate + reinstall.
	  - Ref: docs/ref/edge.md (Install / env → enrollment key)
- [ ] Multi-pod sentinel-flow: shared event fabric for SSE + activity
	  - Today flow is a single multi-tenant pod; control's relay opens ONE connection to `FLOW_URL/events` and the activity map + event bus are per-pod, in-memory (`state.rs`, `events.rs`). Scaling flow to N replicas would mean the relay only sees one pod's runs (affects ALL SSE: run/node/log/activity — and the old poll had the same blindness).
	  - Fix when needed: shared pub-sub (Redis/NATS) for the event bus + shared/queryable activity state, and a relay that fans in from all replicas.
	  - Note: orthogonal to the SSE-vs-poll work — surfaced while doing [[workflow-runs-realtime]].
- [ ] Document camera onboarding flow (ONVIF discovery → go2rtc feed)
- [x] Pricing simulator: surface managed vs self-hosted AI in Settings UI
- [ ] Recipients management UI (add/edit/remove notification targets)
- [ ] Define site/zones model in workflows domain


## ToDo

- [ ] The Node Show Image is not Showing the image correctly  
	and on full screen click the image is not showing
- [ ] Add a 3 seconds at lest on splash screen
- [ ] The Console Logs should be for the current workflow Not All


## In Progress

- [ ] Workflow runs: kill the 700 ms poll, go real-time over SSE
	  - Plan: [[workflow-runs-realtime]]
	  - Root cause: workflows page polls `GET /workflows/runs` every 700ms for the live activity map, but the SSE bus + tenant relay + frontend EventSource already exist — the `GraphActivity` map just was never published. Publish it, drop the poll. SSE not WebSocket (server→client only).


## Review

- [x] Map Redesign 2 — wall style property + architectural-neutral pass
	  - Plan: [[map-redesign-2]]
	  - Wall `style` (wireframe vs solid) now distinct from thickness; map shifted to blue-slate + near-white, emerald only for live accents.
	  - Ref: docs/ref/map.md · Branch: feat/map-redesign
- [ ] Map redesign — top-down 3D blueprint editor + live presence
	  - Epic: [[map-redesign]]
	  - Ref: docs/ref/map.md · Branch: feat/map-redesign
	  - Phases 1–6 done + deployed to prod (592a3da→c885862). Pending: verify presence live, remove [presence] debug logs, then release (version bump + merge to new-ui).


## Done

**Complete**
- [x] Devices → Postgres + pgvector migration follow-ups
- [x] Show image (debug) node: renders the input frame inside the node + a button to enlarge in a modal
- [x] Vision detector: local person detection via tract + YOLOX
- [x] Snapshots grabbed from shared go2rtc feed (fast path)
- [x] RunPod `/status` polling on cold-start (IN_QUEUE/IN_PROGRESS)
- [x] CPU-only torch in the AI Docker image




%% kanban:settings
```
{"kanban-plugin":"board","list-collapse":[false,false,false,false,false,false],"show-checkboxes":true,"date-display-format":"YYYY-MM-DD","hide-date-in-title":true}
```
%%