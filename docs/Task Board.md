---

kanban-plugin: board

---

## Backlog

- [ ] #1 School Student Frequency Case
	  - Plan: [[school-student-frequency]]
	  - Use case: [[school-frequence]]
- [ ] #2 Pricing simulator: surface managed vs self-hosted AI in Settings UI
- [ ] #3 Multi-pod sentinel-flow: shared event fabric
	  - Plan: [[multi-pod-flow-event-fabric]]
- [ ] #4 Box re-key / rotate enrollment action
	  - Plan: [[box-rekey-enrollment]]
- [ ] #5 Recipients management UI (add/edit/remove notification targets)
- [ ] #6 Define site/zones model in workflows domain
- [ ] #7 AI/Vision node ideas — research backlog
	  - Plan: [[ai-vision-node-ideas]]


## Bugs

- [ ] #8 Splash should retry on transient `boot_status` DB errors instead of hanging
- [ ] #9 We should encrypt the passwords in the db column level too
- [ ] #10 Update available — reload to get the latest version.
- [ ] #93 Box health copy leaks internals ("tailnet", "Tailscale") to users
	  - Plan: [[box-health-copy-no-internals]]


## ToDo

- [ ] #11 Update button stays loading until update completes
	  - Plan: [[update-button-loading]]
- [ ] #13 Cooldown node: `reset` exec port
	  - Plan: [[cooldown-reset-port]]
- [ ] #14 The data time on the logs should locally time and we should see the date too — for the current day show only the time
- [ ] #17 Face recognition: DetectFaces node + live-feed triggers
	  - Plan: [[face-recognition-next-steps]]
- [ ] #20 GetCameraRoom node — look up room name from a camera entity
	  - Plan: [[get-camera-room]]
- [ ] #21 Map: presence history + logs panel in SidePanel
	  - Plan: [[map-presence-history]]
- [ ] #58 Draw Text: inside/outside bbox placement
	  - Plan: [[draw-text-inside-outside-bbox]]
- [ ] #59 Person Re-Identification (ReID) — body-appearance matching
	  - Plan: [[reid-person-reidentification]]
- [ ] #60 Editor keybinds: hold Space to pan (temp H) — workflow + map
	  - Plan: [[editor-space-hold-pan]]
- [ ] #64 Cloud recording storage (R2) + GB-month billing
	  - Plan: [[cloud-recording-storage]]
- [ ] #65 User invite: onboarding email + set-password confirmation
	  - Plan: [[invite-onboarding-email]]
- [ ] #66 Agnostic event module — replace the Alert model
	  - Plan: [[agnostic-event-module]]
- [ ] #70 Branded email template system
	  - Plan: [[email-template-system]]
- [ ] #71 Storage billing economics — R2 cost vs quota, no-loss guardrails
	  - Plan: [[storage-billing-economics]]
- [ ] #74 Postgres performance tuning — kill the default config
	  - Plan: [[postgres-performance-tuning]]
- [ ] #76 OlharAI rebrand — domains, brand, app copy, landing rewrite
	  - Epic: [[olharai-rebrand]]
- [ ] #87 Segmented continuous recording — cap clip size, crash-durable, incremental upload
	  - Plan: [[segmented-continuous-recording]]
- [ ] #94 Unified node value store — one place, media in MinIO
	  - Plan: [[unified-node-value-store]]
- [ ] #96 Engine clarity — kill the ambiguity that hides bugs
	  - Plan: [[workflow-engine-clarity]]
- [ ] #99 Domain cutover — serve olharai.com / .com.br, transitional dual-domain
	  - Plan: [[olharai-domain-cutover]]
- [ ] #100 Landing page — Olhar AI rebrand + copy rewrite
	  - Plan: [[olharai-landing]]


## In Progress

- [ ] #80 Windows edge — installer, service, self-update
	  - Plan: [[edge-windows]]
- [ ] #83 Windows installer — signed .exe
	  - Plan: [[edge-windows-installer]]


## Review



## Done

**Complete**
- [x] #86 Delay Trigger node — decouple recording start/stop
	  - Plan: [[delay-trigger-recording-signal-flow]]
- [x] #88 Camera credential wizard (bug B)
	  - Plan: [[camera-credential-wizard]]
- [x] #92 Edge tailnet self-heal — a stopped Tailscale strands the box
	  - Plan: [[edge-tailnet-self-heal]]
- [x] #95 Graph instances — per-run context + live instance view
	  - Plan: [[workflow-graph-instances]]
- [x] #97 Add camera manually — IP + ONVIF port, not just discovered
	  - Plan: [[manual-camera-add]]
- [x] #98 Edge ring reconcile — stop rings for cameras that left
	  - Plan: [[edge-ring-reconcile]]
- [x] #101 Ring pre-roll ignores time — stale frames after a gap
	  - Plan: [[edge-ring-stale-preroll]]
- [x] #73 Notification providers: configured badge + config & enrollment
	  - Plan: [[notification-provider-ux]]
- [x] #91 Event-storm backstop never engages (WF_GLOBAL_CAP dead)
	  - Plan: [[event-storm-backstop-dead]]
- [x] #90 Editor: wire multiple exec outputs into one node (fan-in)
	  - Plan: [[editor-exec-fan-in]]
- [x] Flip Flop workflow node — alternates exec A/B across runs (+ Compare)
	  - Plan: [[flip-flop-node]]
- [x] #12 Node body: cooldown live state + wait config display
	  - Plan: [[node-body-live-state]]
- [x] #79 Edge without OpenCV — pure-Rust motion
	  - Plan: [[edge-pure-rust-motion]]
- [x] #81 Box pairing — enroll by code
	  - Plan: [[box-pairing-code]]
- [x] #78 Clip local retention — evict local copy after cloud upload
	  - Plan: [[clip-local-retention]]
- [x] #77 Edge frame hub — one decode per camera + motion-gated tracking
	  - Plan: [[edge-frame-hub]]
- [x] #82 macOS installer — signed + notarized .pkg
	  - Plan: [[edge-macos-pkg]]
- [x] #67 Playback Timeline — unified inspection view
	  - Plan: [[recording-playback-timeline]]
- [x] #63 Tenant Billing — per-tenant invoicing + cloud cost/margin
	  - Plan: [[tenant-cloud-cost]]
- [x] #32 Map redesign — top-down 3D blueprint editor + live presence
	  - Epic: [[map-redesign]]
- [x] #31 RecordPresence node: room not changeable in property panel
- [x] #30 Free text (filled and wireframe) tool map
- [x] #89 Live badge + loading states (bug C)
	  - Plan: [[live-badge-loading-states]]
- [x] #69 Favicons / app-icons / PWA / OG images
	  - Plan: [[favicons-app-icons]]
- [x] #29 Map Redesign 2 — wall style + architectural-neutral pass
	  - Plan: [[map-redesign-2]]
- [x] #28 Draw BBox node — annotate image with bounding box
	  - Plan: [[draw-bbox-node]]
- [x] #85 Pairing progress UX — "code accepted" state
	  - Plan: [[pairing-progress-ux]]
- [x] #84 Per-platform installer download
	  - Plan: [[installer-download-button]]
- [x] #68 Logo & brand-mark system
	  - Plan: [[logo-brand-mark]]
- [x] #56 Workflow Variables — typed vars + Get/Set nodes
	  - Plan: [[workflow-variables]]
- [x] #27 Map: solid walls taller, richer material, deeper shadows
	  - Plan: [[map-walls-3d-depth]]
- [x] #26 Unified Platform Logs — one table, one UI, all modules
	  - Plan: [[platform-logs-unified]]
- [x] #25 Motion-triggered stream recorder
	  - Plan: [[motion-triggered-recorder]]
- [x] #24 Black & White Rebrand — Sentinel Noir
	  - Plan: [[monochrome-rebrand]]
- [x] #23 TimeGate node — debounced motion wait
	  - Plan: [[time-gate-node]]
- [x] #22 Recordings: storage, names, filters, delete, sort, paginate
	  - Plan: [[recordings-improvements]]
- [x] #19 AI If node — yes/no question routes exec like If
	  - Plan: [[ai-question-node]]
- [x] #15 Telegram bot: workflow-graph brain + inbound commands
	  - Plan: [[telegram-bot-two-way-research]]
- [x] #62 Edge resource monitoring — CPU / mem / disk / GPU
	  - Plan: [[edge-resource-monitoring]]
- [x] #72 Dark palette: deeper elevation ramp (Unreal-style)
	  - Plan: [[dark-palette-elevation]]
- [x] #75 Workflow editor: one tabbed right rail (Details / Properties / Variables / Runs)
	  - Plan: [[workflow-editor-tabbed-inspector]]
- [x] #61 Tab toggle: collapse/expand editor side panels
	  - Plan: [[editor-panel-tab-toggle]]
- [x] #16 Chat agent on the web (TTS / STT / tools)
	  - Plan: [[chat-agent-web]]
- [x] #33 Telegram Beacons: fix enrollment + polish + observability
	  - Plan: [[telegram-beacons-fix-polish]]
- [x] #34 Generic Object Detection node — configurable COCO class
	  - Plan: [[generic-object-detection-node]]
- [x] #35 Show Image: persist last frame so it's visible when graph is closed
- [x] #36 String Switch node — multi-arm exec router on string value
	  - Plan: [[string-switch-node]]
- [x] #37 Missing comparison nodes: TextCompare + And / Or / Not
	  - Plan: [[missing-comparison-nodes]]
- [x] #38 Comment node — canvas organizer
	  - Plan: [[comment-node]]
- [x] #39 Presence: bbox-based detection positioning on map
	  - Plan: [[presence-bbox-positioning]]
- [x] #40 Camera typed port + GetCameraByName node
	  - Plan: [[camera-typed-port]]
- [x] #41 Node run animation lags — light on arrival, keep min duration
	  - Plan: [[node-animation-timing]]
- [x] #42 Events carry typed payload data
	  - Plan: [[event-payload-data]]
- [x] #43 Motion Detection Node v2 (camera-bound; emit+throttle deferred)
	  - Plan: [[motion-detection-node-v2]]
- [x] #44 Workflow runs: real-time over SSE
	  - Plan: [[workflow-runs-realtime]]
- [x] #45 Show Image node: image not rendering (inline + fullscreen)
	  - Plan: [[show-image-node-rendering]]
- [x] #46 Devices → Postgres + pgvector migration follow-ups
- [x] #47 Console Logs scoped to current workflow only
- [x] #48 Add 3-second minimum splash screen
- [x] #49 Show Image debug node: inline render + fullscreen modal
- [x] #50 Vision detector: local person detection via tract + YOLOX
- [x] #51 Document camera onboarding flow (ONVIF discovery → go2rtc feed)
- [x] #52 Snapshots grabbed from shared go2rtc feed (fast path)
- [x] #53 RunPod `/status` polling on cold-start (IN_QUEUE/IN_PROGRESS)
- [x] #54 CPU-only torch in the AI Docker image
- [x] #55 Workflow nodes — rename, categorize & split into files
	  - Plan: [[workflow-node-ux-refactor]]
- [x] #18 Workflow list improvements: last run, search, description, duplicate
	  - Plan: [[workflow-list-improvements]]
- [x] #57 Notification queue + sentinel-notification service
	  - Plan: [[notification-queue-service]]




%% kanban:settings
```
{"kanban-plugin":"board","list-collapse":[false,true,false,false,false,false],"show-checkboxes":true,"date-display-format":"YYYY-MM-DD","hide-date-in-title":true,"full-list-lane-width":true,"move-tags":true,"tag-colors":[{"tagKey":"","color":"","backgroundColor":""}],"inline-metadata-position":"footer","move-task-metadata":true}
```
%%