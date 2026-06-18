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
- [ ] Box re-key / rotate enrollment action
	  - Plan: [[box-rekey-enrollment]]
- [ ] Multi-pod sentinel-flow: shared event fabric
	  - Plan: [[multi-pod-flow-event-fabric]]
- [ ] Pricing simulator: surface managed vs self-hosted AI in Settings UI
- [ ] Recipients management UI (add/edit/remove notification targets)
- [ ] Define site/zones model in workflows domain


## ToDo

- [ ] Node run animation lags — light on arrival, keep min duration
	  - Plan: [[node-animation-timing]]
- [ ] Workflow list improvements: last run, search, description, duplicate
	  - Plan: [[workflow-list-improvements]]


## In Progress



## Review

- [x] Map Redesign 2 — wall style + architectural-neutral pass
	  - Plan: [[map-redesign-2]]
- [ ] Map redesign — top-down 3D blueprint editor + live presence
	  - Epic: [[map-redesign]]


## Done

**Complete**
- [x] Events carry typed payload data
	  - Plan: [[event-payload-data]]
- [x] Motion Detection Node v2 (camera-bound; emit+throttle deferred)
	  - Plan: [[motion-detection-node-v2]]
- [x] Workflow runs: real-time over SSE
	  - Plan: [[workflow-runs-realtime]]
- [x] Show Image node: image not rendering (inline + fullscreen)
	  - Plan: [[show-image-node-rendering]]
- [x] Devices → Postgres + pgvector migration follow-ups
- [x] The Console Logs should be for the current workflow Not All
- [x] Add a 3 seconds at lest on splash screen
- [x] Show image (debug) node: renders the input frame inside the node + a button to enlarge in a modal
- [x] Vision detector: local person detection via tract + YOLOX
- [x] Document camera onboarding flow (ONVIF discovery → go2rtc feed)
- [x] Snapshots grabbed from shared go2rtc feed (fast path)
- [x] RunPod `/status` polling on cold-start (IN_QUEUE/IN_PROGRESS)
- [x] CPU-only torch in the AI Docker image




%% kanban:settings
```
{"kanban-plugin":"board","list-collapse":[false,false,false,false,false,true],"show-checkboxes":true,"date-display-format":"YYYY-MM-DD","hide-date-in-title":true}
```
%%