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
- [ ] #5 Recipients management UI (add/edit/remove notification targets)
- [ ] #6 Define site/zones model in workflows domain
- [ ] #7 AI/Vision node ideas — research backlog
	  - Plan: [[ai-vision-node-ideas]]


## Bugs

- [ ] #207 Time Gate keeps a keeper thread; Delay Trigger already knows better
	  - Plan: [[time-gate-should-not-hold-a-thread]]
	  - TimeGate spawns a thread that POLLS the database every 250ms for the whole window, and
	    holds the run open while it does. DelayTrigger has the same shape and uses
	    `try_arm_node_state` + a `work_jobs` row with `run_after`, with the trailing edge
	    arriving as a NEW run. Same problem, two solutions, one of them from before the queue.
	  - BEHAVIOUR CHANGE, and it is the whole decision: `out` fires in the same run today and
	    in a new one afterwards. Run history splits, and downstream stops seeing run-scoped
	    variables set before the gate. DelayTrigger already lives with all three.
	  - TRAP (documented next door): the test for "window closed" is the MARKER in the entry
	    payload, not `forced` alone — a loop back-edge forces a node too.

- [x] #206 Edge highlight follows the target node, not the arm that fired
	  - Plan: [[edge-highlight-follows-the-target]]
	  - `+page.svelte:3911` lights an edge when its TARGET is live, so with two branches
	    converging on one node BOTH incoming edges animate. Reported on BotMessages.
	  - The editor cannot fix it alone: `workflow-node` says a node ran and nothing has ever
	    said HOW control reached it. Fix is `on_arm(node, port)` in the driver → a
	    `workflow-arm` event → the editor lights an edge by its own (from, from_port).
	  - Third bug in one family (#115, #205, #206): the editor inferring what the engine knew
	    and did not publish. The scheduler is right in all three.
	  - FIXED and VERIFIED in production (2026-08-25) on the BotMessages graph: `on_arm` in the
	    driver → `workflow-arm` event → the editor lights an edge by its own (from, from_port).
	    Test: both arms of an If wired to one node, only `false` is reported.

- [x] #205 Pure sources activate on dead branches — the canvas is telling the truth
	  - Plan: [[pure-sources-run-on-dead-branches]]
	  - `engine.rs:499` activates any `is_pure_source` kind unconditionally, so Get Variable /
	    Get Recipient / Get Camera light up (and RUN) on arms nobody took. Two of the four hit
	    the database. No wrong data flows — the arm's consumers still don't run — but the run
	    log implies the arm executed.
	  - ROOT CAUSE FOUND: #115 already defers the Console line for a dead-branch Get and works.
	    The canvas highlight comes from a DIFFERENT callback — `on_node` → `workflow-node` event
	    → the editor's highlight controller — which has no deferral. Two callbacks, one fix.
	  - FIX: defer `on_node` for pure sources the same way `on_log` already is. Do NOT remove
	    the activation: measured, that fails three tests including the #114/#115 one.
	  - Oracle now exists in `workflows::golden` (the orphan-source-in-a-sub-run shape).
	  - FIXED and VERIFIED in production (2026-08-25): a Get's `on_node` is held until a node
	    that READS it runs, and emitted just before that node. Proven twice — by reverting the
	    fix and watching the new test fail on the exact node id, and by the reporter seeing the
	    bug on the deploy before it and gone on the deploy after.
	  - FECHADO: o adiamento vale para os quatro `is_pure_source` agora, não só `GetVariable`.
	    #115 cobriu o caso reportado e deixou `GetCameraByName`/`GetRecipient` de fora por
	    conservadorismo; têm a mesma forma, ativam por época pela mesma razão, e dois deles
	    consultam o banco — então braço morto custava query além de realce errado.

- [ ] #148 Security §0 — rotate the exposed secrets (JWT_SECRET, Stripe pair, MASTER_PASSWORD)
	  - Plan: [[security-hardening-remainder]]
	  - Was missing from the board entirely while the plan still listed it open (P0).
	  - JWT_SECRET rotates on the box but logs everyone out; the Stripe pair cannot be
	    rotated from the box at all; MASTER_PASSWORD is deliberately the operator's call.
- [ ] #197 Disk exhaustion breaks builds, and clean-space can't reclaim the cause
	  - Plan: [[disk-exhaustion-breaks-everything]]
	  - clean-space now reclaims the cause: 29 G on this machine, 5.3 GiB → 38 GiB free.
	    The cause was never Docker — it was 22 G of Xcode symbol/derived caches, which the
	    old task did not look at. It also reports what it left alone and what is still big.
	  - REMAINS: nobody is watching. No alert at a low-water mark, so the first symptom is
	    still a dead pipeline or a shell that cannot run `df`. And it is WIDER than disk: the
	    OrbStack daemon died once today with 24 GiB free, producing the same wall of red. The
	    tell is duration — every job failing in ~10s means none of them started.
	  - FOLLOW-UP (2026-08-25): the cleanup was making builds slower. `docker system prune -af`
	    took the BUILD CACHE with the images, and this machine is the runner — `build:rust`
	    went 24 → 43 min and `docker system df` read `Build Cache 0B` after each run. Now
	    `image prune` plus `builder prune --filter until=168h`: trimmed, not emptied.
- [ ] #183 CI: image push races the build cache and fails ~once a day
	  - Plan: [[ci-registry-blob-race]]
- [ ] #8 Splash should retry on transient `boot_status` DB errors instead of hanging
- [ ] #9 We should encrypt the passwords in the db column level too
- [ ] #10 Update available — reload to get the latest version.


## Specification

- [ ] #203 Widget + deep link in the Tauri shell
	  - Plan: [[tauri-widget-and-deeplink]]
	  - Depends on #202; BLOCKS retiring sentinel-mobile — pinned.ts is what writes the
	    App Group the widget reads, so the shell needs a Rust write path first.
- [ ] #202 Tauri v2 mobile — share the web app instead of maintaining a second UI
	  - Plan: [[tauri-mobile-shared-web]]
	  - Blocked until #185 (App Store readiness) lands — swapping the shell restarts it.
- [ ] #201 Faces canvas background — match the workflow editor exactly
	  - Plan: [[faces-canvas-editor-background]]
- [ ] #200 Faces: one journey per task, delete means delete, then re-test from zero
	  - Plan: [[faces-one-journey]]
	  - Phase 0 is a destructive wipe of person_embeddings + faces (persons kept).
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
- [ ] #161 Drop the inert box columns left by the private-link era
	  - Plan: [[architecture-weaknesses]]
	  - Readers done in #199 — the column now has none. What remains is the migration
	    that drops it, validated against a live DB first.
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
- [ ] #83 Windows installer — signed .exe
	  - Plan: [[edge-windows-installer]]


## Revision



## Audit



## Done

- [x] #204 ggraph — a engine de grafos extraída para repo próprio
	  - Plan: [[ggraph-engine-extraction]] · Repo: github.com/jackesdavid/graph-engine
	  - FEITO e VERIFICADO em produção (2026-08-26). Todo run — manual, cron, entrega de evento,
	    re-entrada de temporizador — passa por `ggraph_core::run`. O scheduler deste produto foi
	    APAGADO: 3345 linhas fora, 156 dentro.
	  - Provado ao vivo antes de apagar: uma janela de DelayTrigger armou por run manual, fechou
	    por temporizador 11s depois, o ramo de stop disparou, e o run diz "ran 3 node(s)" com 2
	    nós no log — o terceiro foi RESTAURADO, não re-executado.
	  - SETE regressões silenciosas encontradas e corrigidas antes do merge, todas da mesma
	    família (duas implementações de uma regra, uma no driver e outra no caminho novo): o
	    envelope dos valores, a coluna das variáveis, a chave de instância, o timeout por nó,
	    `run_orphans`, o `summary` do adaptador, e três passos sem linha de log. Nenhuma dava erro.
	  - A defesa que ficou: os 28 testes do driver não morreram — foram repontados para a fachada
	    (`behaviour.rs`) e agora exercitam o scheduler que corre de verdade. Isso descobriu mais
	    três diferenças, todas escritas como teste.
	  - Licença: AGPL-3.0-only a partir da v0.17.0 (v0.1.0–v0.15.0 ficam MIT/Apache, v0.16.0 MPL —
	    apertar licença só vale para a frente).
	  - FORA deste cartão, e é o que resta da ideia de partilhar com o Redoma: `ggraph-pg` —
	    `repo.rs` (1610 linhas de SQL) + as oito migrações que são da engine. Sem isso o Redoma
	    partilha o motor mas escreve a persistência de raiz.

- [x] #154 Security hardening PRs 1–5 — prod verification complete
	  - Plan: [[security-hardening-remainder]]
	  - RE-VERIFIED live 2026-08-20: per-IP throttle present as sentinel-login-throttle
	    (20/min, burst 40, on the login router); traefik.yml still drops RequestPath;
	    WEBAUTHN_RP_ID=olharai.com + origin https://app.olharai.com. The remaining work
	    is NOT part of this card — it is #148, #151, #152.
- [x] #199 Retire the private-link remnants from the code, not just the architecture
	  - Plan: [[retire-the-private-link]]
	  - VALIDATED on the live DB 2026-08-20: Jack Mac (edge 0.5.73, which does not send the
	    old header) shows health_at 9s old — it could never have stamped under the old code.
	    Silicom is the bug's fingerprint: health_at frozen 2026-07-29, two days OLDER than
	    its last_seen, i.e. it kept heartbeating while health froze.
- [x] #93 Box health copy no longer names the plumbing to users
	  - Closed by #199: the words are gone from the copy and from the code behind it.
	    Devices "save without testing" now explains itself in plain terms.
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