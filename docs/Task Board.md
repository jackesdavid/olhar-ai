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

- [ ] #170 Runner hangs pulling any image it does not already have
	  - Plan: [[self-hosted-runners]]
- [ ] #169 Windows self-hosted runner + release:edge-windows
	  - Plan: [[self-hosted-runners]]
- [ ] #161 Retire the tailnet remnants — drop magicdns_host + tailscale_state (readers first)
	  - Plan: [[architecture-weaknesses]]
- [ ] #152 Edge signing §3 P2→P3 — soak, then flip signing_mode to required per box
	  - Plan: [[security-hardening-remainder]]
- [ ] #118 Edge detection timing metrics — motion/face detect ms + rate
	  - Plan: [[edge-detection-metrics]]
- [ ] #102 Workflow parameterization — input vars, clone, promote, templates
	  - Plan: [[workflow-parameterization]]
- [ ] #94 Unified node value store — one place, media in MinIO
	  - Plan: [[unified-node-value-store]]
- [ ] #95 Graph instances — per-run context + live instance view
	  - Plan: [[workflow-graph-instances]]
- [ ] #17 Face recognition: DetectFaces node + live-feed triggers
	  - Plan: [[face-recognition-next-steps]]
- [ ] #20 GetCameraRoom node — look up room name from a camera entity
	  - Plan: [[get-camera-room]]
- [ ] #80 Windows edge — installer, service, self-update
	  - Plan: [[edge-windows]]
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
- [ ] #96 Engine clarity — kill the ambiguity that hides bugs
	  - Plan: [[workflow-engine-clarity]]
- [ ] #99 Domain cutover — serve olharai.com / .com.br, transitional dual-domain
	  - Plan: [[olharai-domain-cutover]]
- [ ] #100 Landing page — Olhar AI rebrand + copy rewrite
	  - Plan: [[olharai-landing]]
- [ ] #104 Remove "Connection lost" startup toast
	  - Plan: [[remove-connection-lost-toast]]
- [ ] #105 Editor panel toggle — rebind off Tab
	  - Plan: [[editor-panel-toggle-rebind]]
- [ ] #106 Get/Set Variable node — width fits the variable name
	  - Plan: [[get-variable-node-width]]
- [ ] #107 Node input: inline-value indicator
	  - Plan: [[node-input-inline-value-indicator]]
- [ ] #108 Editor chrome-toggle — visible toolbar button
	  - Plan: [[editor-chrome-toggle-button]]
- [ ] #109 Comment node missing from add-node palette
	  - Plan: [[comment-node-palette-restore]]
- [ ] #110 Node palette — focus search on open
	  - Plan: [[node-palette-search-autofocus]]
- [ ] #111 Scheduled workflow trigger — calendar schedule builder
	  - Plan: [[workflow-schedule-trigger]]
- [ ] #121 Retire local builds — full CI/CD (build + host on GitHub)
	  - Plan: [[retire-local-builds-ci]]
- [ ] #172 Default face quality gate — wire into the seeded template + measured thresholds
	  - Plan: [[face-quality-gate-upstream]]
- [ ] #123 Recognition scoring — per-person template + margin test
	  - Plan: [[face-recognition-per-person-scoring]]
- [ ] #128 Similarity thresholds — per-tenant/camera calibration
	  - Plan: [[face-threshold-calibration]]


## In Progress

- [ ] #124 Face clustering — kill single-link chaining
	  - Plan: [[face-clustering-chaining-fix]]
- [ ] #160 Tenant isolation by construction — row-level security
	  - Plan: [[tenant-isolation-rls]]
- [ ] #167 Terms of Service + Privacy Policy + consent at signup
	  - Plan: [[launch-readiness-selfserve]]
- [ ] #148 Security plan §0 — rotate the exposed secrets + remove the demo admin
	  - Plan: [[security-hardening-remainder]]
- [ ] #151 Security plan §1.8 — scoped media tokens, get JWTs out of query strings
	  - Plan: [[security-hardening-remainder]]
- [ ] #120 Face pipeline on primary (HD) stream — GPU decode + per-tenant selector
	  - Plan: [[face-pipeline-hd-stream]]


## Review

- [ ] #154 Security hardening PRs 1–5 — landed, pending prod verification
	  - Plan: [[security-hardening-remainder]]
- [ ] #83 Windows installer — signed .exe
	  - Plan: [[edge-windows-installer]]


## Done

**Complete**

- [x] #173 Services connect to Postgres as a superuser (RCE reach on the DB host)
	  - Plan: [[db-superuser-privilege]]
- [x] #171 Hollow deploy — a merge can skip every build job and still report success
	  - Plan: [[ci-hollow-deploy]]
- [x] #166 Self-service password reset — a locked-out customer has no way back in
	  - Plan: [[launch-readiness-selfserve]]
- [x] #165 Email delivery foundation — a provider wired as a notification channel
	  - Plan: [[email-delivery-foundation]]
- [x] #127 pgvector face kNN — HNSW index + retrieve-then-rerank
	  - Plan: [[face-knn-hnsw-rerank]]
- [x] #126 Face quality gate — blur + pose signals as composable ports
	  - Plan: [[face-quality-gate-upstream]]
- [x] #163 macOS CI runner — stop building the edge .pkg by hand (the last local build)
	  - Plan: [[self-hosted-runners]]
- [x] #159 Media pipeline smoke test in CI — frames, clip, snapshot
	  - Plan: [[media-smoke-test-ci]]
- [x] #168 Self-hosted runners — build on our own hardware, no shared minutes
	  - Plan: [[self-hosted-runners]]
- [x] #155 Camera status — show WHICH stage failed, per camera
	  - Plan: [[camera-status-visibility]]
- [x] #164 Capture watchdog kills each fresh pipeline — cameras never recover
	  - Plan: [[capture-watchdog-stale-frame]]
- [x] #162 edge:full-disk must watch FREE SPACE, not the recording quota
	  - Plan: [[edge-full-disk-free-space]]
- [x] #158 Deploy must assert what it deployed — compare running digests
	  - Plan: [[deploy-assert-digests]]
- [x] #157 PREPARE every sqlx statement in CI — catch placeholder/bind drift
	  - Plan: [[sql-prepare-ci]]
- [x] #156 Build every production image in CI — sentinel-postgres + sentinel-ai
	  - Plan: [[architecture-weaknesses]]
- [x] #121 Retire local builds — full CI/CD
	  - Plan: [[retire-local-builds-ci]]
- [x] #99 Domain cutover — olharai.com live, transitional dual-domain serving
- [x] #149 Login rate limiting §1.7 — apply the per-IP throttle at Traefik
	  - Plan: [[security-hardening-remainder]]
- [x] #150 Traefik access logging + container log rotation
	  - Plan: [[security-hardening-remainder]]
- [x] #153 WebAuthn RP ID unset in prod — passkeys live but pointed at localhost
	  - Plan: [[security-hardening-remainder]]
- [x] #119 Faces view — cluster + bulk-label unknown faces → people
	  - Plan: [[faces-labeling-view]]
- [x] #130 Workflow node memoize off by default + Variable panel read-only
	  - Plan: [[workflow-memoize-off-default]]
- [x] #11 Update button stays loading until update completes
	  - Plan: [[update-button-loading]]
- [x] #129 Tuya integration — provider + workflow nodes to control devices
	  - Plan: [[tuya-integration]]
- [x] #117 Face recognition pipeline — edge detect → embed → recognize → store
	  - Plan: [[face-recognition-pipeline]]
- [x] #115 Get Variable over-activation — fires on branches that don't use it
	  - Plan: [[get-variable-over-activation]]
- [x] #114 Stop Recording missing camera on DelayTrigger stop
	  - Plan: [[stop-recording-missing-camera]]
- [x] #113 Dictionaries — reusable typed tables + nodes
	  - Plan: [[dictionaries-tables]]
- [x] #112 Enabled / Debug / Orphans → top-of-canvas icon toggles
	  - Plan: [[editor-toggles-top-cluster]]
- [x] #103 Remove per-run workflow toast
	  - Plan: [[remove-workflow-run-toast]]
- [x] #122 Face embeddings — landmark-aligned (kps → face_embed)
	  - Plan: [[face-embed-landmark-align]]
- [x] #125 Assert L2-normalization on face embeddings at store time
	  - Plan: [[face-embedding-l2-normalization]]
- [x] #86 Delay Trigger node — decouple recording start/stop
	  - Plan: [[delay-trigger-recording-signal-flow]]
- [x] #88 Camera credential wizard (bug B)
	  - Plan: [[camera-credential-wizard]]
- [x] #92 Edge tailnet self-heal — a stopped Tailscale strands the box
	  - Plan: [[edge-tailnet-self-heal]]
- [x] #116 Playback view fixes — clip-bar width, granular zoom, per-kind lanes, full-height
	  - Plan: [[playback-view-fixes]]
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