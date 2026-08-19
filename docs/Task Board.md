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

- [ ] #183 CI: image push races the build cache and fails ~once a day
	  - Plan: [[ci-registry-blob-race]]
- [ ] #8 Splash should retry on transient `boot_status` DB errors instead of hanging
- [ ] #9 We should encrypt the passwords in the db column level too
- [ ] #10 Update available — reload to get the latest version.
- [ ] #93 Box health copy leaks internals ("tailnet", "Tailscale") to users
	  - Plan: [[box-health-copy-no-internals]]


## Specification

- [ ] #198 Devices, Recipients and Integrations are split by the wrong rule
	  - Plan: [[one-name-one-place]]
- [ ] #196 On-demand graphs are indistinguishable from switched-off ones
	  - Plan: [[on-demand-graphs-read-as-disabled]]
- [ ] #194 Outcome-based pricing — bill for results, not capacity
	  - Plan: [[outcome-based-pricing]]
- [ ] #193 Meter what the AI assistant costs, per tenant
	  - Plan: [[llm-cost-metering]]
- [ ] #189 Organise the Recipients screen into tabs
	  - Plan: [[recipients-tabs]]
	  - NARROWED by #198: no Onboarding tab — enrollment lives in Integrations.
	    Two tabs, Telegram and Phones, or fold into #198.
- [ ] #186 Device Gate offers recipients that can never answer
	  - Plan: [[device-gate-phone-only-recipients]]
- [ ] #184 State-aware device control — act on the real state, confirm it changed
	  - Plan: [[tuya-state-aware-control]]
- [ ] #175 Mobile as a real edge — device camera as a face-capture source
	  - Plan: [[mobile-edge-face-capture]]
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
- [ ] #60 Editor keybinds: hold Space to pan (temp H) — workflow + map
	  - Plan: [[editor-space-hold-pan]]
	  - Phase 1 (workflow editor) SHIPPED: hold Space → temporary pan, release restores the tool;
	    key-repeat guarded, window blur force-releases. Phase 2 (map editor) remains — the map has
	    no pan tool at all, so it needs a force-pan flag rather than a mode switch.
- [ ] #109 Comment node missing from add-node palette
	  - Plan: [[comment-node-palette-restore]]
- [ ] #110 Node palette — focus search on open
	  - Plan: [[node-palette-search-autofocus]]
- [ ] #111 Scheduled workflow trigger — calendar schedule builder
	  - Plan: [[workflow-schedule-trigger]]
- [ ] #121 Retire local builds — full CI/CD (build + host on GitHub)
	  - Plan: [[retire-local-builds-ci]]


## Generation

- [ ] #190 Push delivery is never verified — we read the ticket, never the receipt
	  - Plan: [[push-receipts-not-just-tickets]]
	  - Phase 1 BUILT: receipts read on a delayed work_jobs job; only DeviceNotRegistered
	    disables a device. Phases 2-3 (run log, Recipients screen, fail the gate) remain.
- [ ] #188 A dangling recipient/camera reference auto-selects the first option
	  - Plan: [[dangling-recipient-must-not-autoselect]]
	  - Editor no longer substitutes: a configured value that no longer exists resolves to
	    nothing and says which name died, for recipients AND cameras. Phase 2 verified as
	    already-correct — the engine fails closed and names the label.
- [ ] #185 Mobile production launch — App Store review readiness
	  - Plan: [[mobile-app-store-launch]]
	    a per-device Test that reads status + reports the round-trip. Phase 5 (sync count +
	    land on naming) remains. Nothing verified against a real Tuya project yet.
- [ ] #174 Faces — complete the pipeline
	  - Plan: [[faces-completion]]
	  - Steps 1 (per-tenant capture stream) and 2 (index-backed cluster neighbours) shipped.
	    Next: step 3 (capture volume — blocks 4 and 5), then 6, 6.5 (one Faces settings tab),
	    and the reference doc last.
- [ ] #160 Tenant isolation by construction — row-level security
	  - Plan: [[tenant-isolation-rls]]
	  - UNBLOCKED (2026-08-12): control connects as `sentinel_app` (rolsuper=f, rolbypassrls=f),
	    so policies would actually apply. But RLS is unbuilt — 0 of 64 public tables have it on.
- [ ] #167 Terms of Service + Privacy Policy + consent at signup
	  - Plan: [[launch-readiness-selfserve]]
- [ ] #151 Security plan §1.8 — scoped media tokens, get JWTs out of query strings
	  - Plan: [[security-hardening-remainder]]


## Validation

- [ ] #195 Capture-from-camera always failed — enroll and recognize called a stub
- [ ] #181 Mobile onboarding — invite a person, pair their phone
	  - Plan: [[mobile-tenant-onboarding]]
	  - All three phases shipped and deployed: invite tokens + public preview/accept, the QR
	    and /join page on the web, and the scanner in the app.
	  - Awaiting a SECOND iPhone — the real test is a person who is not the owner, on a fresh
	    install. Note that device needs the build installed first (no TestFlight yet, #185).
	    face graph: eight Set Variable nodes → one, eight Get Variable → one.
	  - Ports are NAMED after the variables, so a rename repoints config AND edges together.
- [ ] #123 Recognition scoring — per-person template + margin test
	  - Plan: [[face-recognition-per-person-scoring]]
- [ ] #154 Security hardening PRs 1–5 — landed, pending prod verification
	  - Plan: [[security-hardening-remainder]]
- [ ] #83 Windows installer — signed .exe
	  - Plan: [[edge-windows-installer]]


## Revision



## Audit



## Done

- [x] #192 Split `tuya.control` out of `tuya.write` — editing labels should not open gates
	  - The test endpoint picks the permission from its PAYLOAD: code+value writes and needs
	    tuya.control; without them it only reads. Admins keep working via the boot seed; a
	    custom role holding tuya.write loses command-sending, which is the point.
- [x] #191 Integrations screen in tabs + a testable Tuya device console
	  - Plan: [[integrations-tabs-tuya-console]]
	  - VALIDATED in production by the owner on the Garen gate board. Needed no backend:
	    /tuya/devices/:id/test already wrote when given {code, value}. Audit raised #192 —
	    tuya.write now opens gates and its description said 'edit device labels'.
- [x] #133 Tuya local control [map] — the edge talks to the device on the LAN, cloud out of the path
	  - Map: [[tuya-local-map]] · today's model: [[tuya]]
	  - CLOSED 2026-08-17 by the owner: we are not pursuing LAN control this way. Cloud control
	    through the one operator project is good enough in practice — the gate was proven moving
	    end to end (#182) and the round-trip is acceptable. The map and its four decision tickets
	    stay on disk as research, not as work.
	  - What replaces it is #191: make the integration PROVABLE rather than faster.
- [x] #187 Recipients: a phone can't be tested or deleted
	  - Plan: [[recipients-phone-parity]]
	  - VALIDATED in production by the owner. The dialog names the consequence of removing a
	    phone but does NOT count the affected workflows — that half is left with #188.
- [x] #179 Set Variables / Get Variables — one node instead of a column of them
	  - Plan: [[multiset-multiget-nodes]]
	  - VALIDATED by the owner on the real face graph — the column of single Set/Get nodes it
	    was built to replace.
- [x] #182 Tuya onboarding — diagnose the failure, prove the device moves
	  - Plan: [[tuya-onboarding-journey]]
	  - CONFIRMED on the owner's own devices: connect → diagnose → sync → control. The gate
	    opens from the iPhone widget through Device Gate + Face ID.
	  - The find that mattered: the gate module is published as a THING MODEL, and the whole
	    v1.0/v1.1 API family answers 2009/2003/2008 for it. Two fixes deduced from docs and
	    logs failed; only probing every endpoint against the real device found it.
	  - Also ships the 4-step checklist, the data-centre probe, and a per-device Test.
- [x] #180 Copy / Paste — paste at the cursor, config only, no wires
	  - Plan: [[editor-copy-paste-spec]]
	  - Confirmed working in the editor. Paste lands where the mouse is, carries only the
	    panel config, and brings no wires — a pasted node is a fresh node to wire by hand.
	  - Duplicate (`d`) was decoupled from the clipboard here so it keeps its own behaviour;
	    it still has no spec of its own.
- [x] #176 Device Gate node — a human approves from their phone before the graph continues
	  - Map: [[device-gate-map]] (6/6 decisions resolved) · baseline: [[device-gate-node]]
	  - PROVEN END TO END on a real iPhone: all three arms fired (approve → Positive, deny →
	    Negative, expiry → Failure), `auth_factor` recorded as `biometric`, and the sweep resumed
	    the graph 21ms after the answer via the `device_gate_settled` NOTIFY.
	  - Three bugs only a device could find: Expo rejects `interruptionLevel: "timeSensitive"`
	    (it wants `time-sensitive`) so EVERY gate push 400-ed and the feature was dead on arrival;
	    the slider was frozen by a closure captured before layout; the Android channel the backend
	    stamps did not exist app-side.
	  - Per-phone cap `device_gate_max_pending` (default 10), per tenant, enforced in one
	    transaction behind a per-device advisory lock.
- [x] #177 Retire the graph-level Cron/Schedule trigger — scheduling is a node property
	  - Deployed and verified in production: migration `0095` applied, all 30 graphs on `"Manual"`,
	    flow loaded them all (nothing failed to deserialize — the risk of dropping the variants),
	    and `Turn On/Off Daily` went from two clocks to the two cron nodes it should have.
	  - Sub-minute polling is gone with it, deliberately. Ref: [[workflows]] "Triggers".
- [x] #178 Run a workflow from the phone — Run tab, deep link, home-screen widget
	  - Tap → the workflow runs → its Device Gate asks this phone → Face ID → done. The widget is
	    configurable the iOS way (long-press → Edit Widget → choose), holds no credentials and
	    speaks no API: every button is `olharai://run/<id>`, the deep link the app already handles.
	  - The deep link is a ROUTE, not an event — expo-router owns incoming URLs, so a `Linking`
	    listener never sees the tap and the widget landed on "Unmatched Route".
	  - A gate push arriving while the app is open opens the approval screen by itself, so the
	    notification tap is optional.
- [x] #108 Editor chrome-toggle — visible toolbar button
	  - Plan: [[editor-chrome-toggle-button]]
- [x] #107 Node input: inline-value indicator
	  - Plan: [[node-input-inline-value-indicator]]




%% kanban:settings
```
{"kanban-plugin":"board","list-collapse":[false,true,false,false,false,false,null,true],"show-checkboxes":true,"date-display-format":"YYYY-MM-DD","hide-date-in-title":true,"full-list-lane-width":true,"move-tags":true,"tag-colors":[{"tagKey":"","color":"","backgroundColor":""}],"inline-metadata-position":"footer","move-task-metadata":true}
```
%%