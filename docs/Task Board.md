---

kanban-plugin: board

---

## Bugs

- [ ] Splash should retry on transient `boot_status` DB errors instead of hanging
- [ ] We should encrypt the passwords in the db column level too
- [ ] Pricing simulator: surface managed vs self-hosted AI in Settings UI


## Backlog

- [ ] School Student Frequency Case
	  - Plan: [[school-student-frequency]]
	  - Use case: [[school-frequence]]
- [ ] Multi-pod sentinel-flow: shared event fabric
	  - Plan: [[multi-pod-flow-event-fabric]]
- [ ] Box re-key / rotate enrollment action
	  - Plan: [[box-rekey-enrollment]]
- [ ] Recipients management UI (add/edit/remove notification targets)
- [ ] Define site/zones model in workflows domain
- [ ] AI/Vision node ideas — research backlog
	  - Plan: [[ai-vision-node-ideas]]


## ToDo

- [ ] Telegram bot: two-way use cases research
	  - Plan: [[telegram-bot-two-way-research]]
- [ ] Chat agent on the web (TTS / STT / tools)
	  - Plan: [[chat-agent-web]]
- [ ] Face recognition: DetectFaces node + live-feed triggers
	  - Plan: [[face-recognition-next-steps]]
- [ ] Workflow list improvements: last run, search, description, duplicate
	  - Plan: [[workflow-list-improvements]]
- [ ] Comment node — canvas organizer
	  - Plan: [[comment-node]]
- [ ] AI If node — yes/no question routes exec like If
	  - Plan: [[ai-question-node]]


## In Progress

- [ ] Telegram Beacons: fix enrollment + polish + observability
	  - Plan: [[telegram-beacons-fix-polish]]
- [ ] Generic Object Detection node — configurable COCO class
	  - Plan: [[generic-object-detection-node]]
- [ ] String Switch node — multi-arm exec router on string value
	  - Plan: [[string-switch-node]]
- [ ] Missing comparison nodes: TextCompare + And / Or / Not
	  - Plan: [[missing-comparison-nodes]]


## Review

- [ ] Map: solid walls taller, richer material, deeper shadows
	  - Plan: [[map-walls-3d-depth]]
- [ ] Draw BBox node — annotate image with bounding box
	  - Plan: [[draw-bbox-node]]
- [x] Map Redesign 2 — wall style + architectural-neutral pass
	  - Plan: [[map-redesign-2]]
- [ ] Free text ( filled and wireframe) tool map
- [ ] In the (Record Presence) node is not possible to change the room in the property panel
- [ ] Map redesign — top-down 3D blueprint editor + live presence
	  - Epic: [[map-redesign]]


## Done

**Complete**
- [x] Camera typed port + GetCameraByName node
	  - Plan: [[camera-typed-port]]
- [x] Node run animation lags — light on arrival, keep min duration
	  - Plan: [[node-animation-timing]]
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
{"kanban-plugin":"board","list-collapse":[false,false,false,false,false,false],"show-checkboxes":true,"date-display-format":"YYYY-MM-DD","hide-date-in-title":true}
```
%%