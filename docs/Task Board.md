---

kanban-plugin: board

---

## Bugs

- [ ] Splash should retry on transient `boot_status` DB errors instead of hanging
- [ ] We should encrypt the passwords in the db column level too


## Backlog

- [ ] Document camera onboarding flow (ONVIF discovery → go2rtc feed)
- [x] Pricing simulator: surface managed vs self-hosted AI in Settings UI
- [ ] Recipients management UI (add/edit/remove notification targets)
- [ ] Define site/zones model in workflows domain


## ToDo

- [ ] Add a 3 seconds at lest on splash screen
- [ ] Box re-key / rotate enrollment action (Boxes page)
	  - Re-mint a reusable Tailscale auth key for an existing box **without recreating it** (keep box id/token/cameras); push the new key into `edge.env` + restart. Optionally disable node-key expiry so appliances survive past the ~90-day key lifetime.
	  - Why: enrollment keys now mint reusable + non-ephemeral + 90d (`sentinel-control/src/tailscale.rs`, fixed `20814c1`), so reboots recover — but after 90d a box still needs re-enrollment, and today the only recovery is delete + recreate + reinstall.
	  - Ref: docs/ref/edge.md (Install / env → enrollment key)


## In Progress

- [ ] Map redesign — top-down 3D blueprint editor + live presence
	  - Epic: [[map-redesign]]
	  - Ref: docs/ref/map.md · Branch: feat/map-redesign
	  - Phases 1–6 done + deployed to prod (592a3da→c885862). Pending: verify presence live, remove [presence] debug logs, then release (version bump + merge to new-ui).
- [ ] School Student Frequency Case
	  - Plan: [[school-student-frequency]]
	  - Use case: [[school-frequence]]


## Review



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