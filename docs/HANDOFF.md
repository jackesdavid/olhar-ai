# Handoff log

Rolling session memory (newest first). Maintained via the `/handoff` skill. Each entry: what
changed, branch/commits, state, what's pending, blockers. Deep per-area detail lives in
`docs/ref/*.md`.

---

## 2026-08-25 (later) — production runs on the engine's scheduler, and the files got readable

- **Branch:** `feat/204-steps-durable` → !145. Engine at `v0.13.0`, pinned by tag.
- **Commits:** `d17fb6f` (DeferringObserver) · `99e2d8b` (differential: sub-runs + arms) ·
  `ef205dc` (the swap) · `c245398` `9507e98` `ff55a00` (domain, node, vision split).

### Done

**Every production run now goes through `ggraph_core::run`** — manual, cron, event delivery, timer
re-entry. This product's driver still exists and is still tested, but nothing calls it outside the
differential suite that proves the two agree.

The deferral of pure sources (#115, #205) moved out of the scheduler into a `DeferringObserver`,
which is where it belonged: holding back a ping is a decision about REPORTING, not execution. Its
tests drive it by hand with no scheduler at all, so the contract survives the driver it was
written against.

**Three things the swap needed that nothing would have failed loudly about:**

1. **Engine-driven nodes never reported their arms.** The driver announces an arm in one place —
   the `run_node` path. ForEach, Cooldown and FlipFlop mark theirs live directly and tell nobody,
   so `loop_body`, `completed` and a Cooldown's `exec_out` never reach the editor and **the edge
   out of a loop body does not light at all**. #206 again, in the half its fix never reached. The
   engine reports all of them.
2. **Restoring and checkpointing were one flag.** A `DelayTrigger`'s trailing edge fires from a
   timer with no payload; the only way it sees the arming run's `camera` is to reread what that
   run produced. The engine could restore but refused unless it was also writing checkpoints —
   the opposite of what Sentinel wants. `Entry::restore` (v0.12.0) asks directly.
3. **Nothing was written.** `remember()` in `workflows/run.rs` writes what each node produced —
   only nodes that RAN and produced something, which is what keeps a re-trigger correct.

**Seven silent regressions, all one family.** Found by sweeping for the class after the third,
not by luck. The shared shape: **two implementations of one rule — one used by the driver, one by
the new path.** None of them errored.

| | what broke | how it would show |
|---|---|---|
| 1 | values written without the `{"<node>": …}` envelope | a window closes and recognises nothing |
| 2 | variables in the wrong column, read by `values_json["__variables"]` that nothing writes | #114 returns; the restore is always empty |
| 3 | `instance_key` took a camera only as text, not as an entity | arms under `cam-1`, looks under `""` |
| 4 | the timeout declared from an EMPTY configuration | a node set to sixty seconds dies at fifteen |
| 5 | `run_orphans` read by the old driver alone | a forgotten node on the canvas **sends** the notification |
| 6 | the `Legacy` adapter never implemented `summary` | the Console loses its detail and keeps printing |
| 7 | Display / ShowImage / OnEvent returned no log | those three go silent in the Console |

The durable defence is the **differential comparing three dimensions** over fifteen shapes: which
nodes ran, which arm control left by, and the line a person reads. It compared only the first
before — and each new dimension caught at least one of these.

Method note worth keeping: the timeout test written that same hour **went on passing** after its
node stopped running, because a run with nothing to run returns `Ok`. A test asserting only "it did
not fail" stops testing the moment its subject stops executing.

A PRE-EXISTING trap found in passing and left alone: a node that overruns its timeout is abandoned
and its thread holds a clone of the context, so the UI channel never closes and `drain.join()`
blocks. The old driver did exactly the same, so it is not a regression — but it is real.


**Reorganisation.** `domain.rs` (3147) → seven files; `node.rs` (1944) → five plus its families;
`vision.rs` (1060) → five. In the engine: `exec.rs` → four, `spec.rs` → four, and twelve loose
root modules → six folders (`data`, `graph`, `exec`, `spec`, `host`, `nodes`). No path changed
anywhere — re-exports carry them — and 151 workflow tests pass throughout.

### Pending

- **The live test.** Nothing has run in production on the engine's scheduler yet. Watch a graph
  with a branch, a loop and a DelayTrigger window: the Console lines, the canvas highlights, and
  above all a window that closes and still sees its camera.
- **`engine.rs` (2672 lines) stays until that passes.** It is the way back and it is the oracle;
  deleting it now would throw away the safety net the night before needing it. `differential.rs`
  goes with it.
- `repo.rs` (1610 lines of SQL) + the twelve engine migrations → a `ggraph-pg` adapter.
- **#207** — the Time Gate holding a thread. Still needs a decision: it changes when `out` fires
  and splits run history.

---

## 2026-08-25 — the node set becomes engine behaviours, twelve of twelve

- **Branch:** `feat/204-steps-durable` → !145, CI running when this was written. Everything
  before it is merged: !141 through !144.
- **Commits:** `114babf` (NodeId + Graph<SentinelMeta>) · `16e6769` (display + emit + OnEvent) ·
  `2816efe` (DelayTrigger) · `ea39647` (DeviceGate).
- **Engine:** `v0.9.3`, pinned by tag. Nine releases today, every one from a real difference a
  consumer hit rather than from speculation.

### Done

**The big slice that was reverted yesterday, landed.** `NodeKind` → `NodeId`, `Graph<SentinelMeta>`
and `GraphNode`, all in one — because they are one change, not three. What made it an afternoon
instead of a rewrite was three impls written FIRST and compiling on their own:
`PartialEq<NodeKind> for NodeId` both ways, `From<NodeKind> for NodeId`, and an `AsKind` trait
returning `Option`. The enum becomes a VIEW over the open identifier, so `n.kind == NodeKind::If`
reads the way it always did at all sixteen sites, untouched.

**All twelve inline node kinds are engine behaviours now** (`workflows/steps/`, one file each),
bar `TimeGate` which is #207 and needs a decision. Both schedulers still exist, which is the
correct state: `workflows/differential.rs` runs the same graph through both and requires the same
node sequence. **Six shapes agree with `KNOWN_DIFFERENCES` empty.**

**#205 and #206 verified in production** by the reporter, on their own graph.

**#197 widened twice:** the cleanup was taking the Docker BUILD CACHE with the images (build:rust
went 24 → 43 min, `Build Cache 0B` after every run — now `image prune` plus
`builder prune --filter until=168h`), and the daemon died once with 24 GiB free, which produces
the same wall of red as a full disk.

### State

139 tests, workspace clean, edge compiles, both lockfiles current. The catalog snapshot is
UNTOUCHED across the whole session; the golden documents changed only in field ORDER, verified
key-by-key (jsonb normalises order, the editor reads by key).

In production: the extraction, #205, #206, Tauri iOS, engine v0.9.0. NOT yet: !145.

### Pending / next

1. **Merge !145** once CI is green (DelayTrigger + DeviceGate).
2. **The scheduler swap** — `run_dag_eventful` → `ggraph_core::run`. Only possible now. Extend
   the differential shape list first, then swap, then delete the inline blocks.
3. **#207 TimeGate** — needs a decision, see Blockers.
4. `repo.rs` and the 12 engine-owned migrations → a `ggraph-pg` adapter.

### Blockers

**#207 is a decision, not a task.** TimeGate spawns a thread that polls the database every 250 ms
for its whole window. `DelayTrigger` has the same shape and uses a `work_jobs` row with
`run_after`, the trailing edge arriving as a new run. Converting TimeGate means `out` stops firing
in the same run and fires in a new one: run history splits, and **run-scoped variables do not
survive** — that was #114, and the fix was a whole snapshot/restore mechanism which now lives in
`steps/delay_trigger.rs` and can be copied. All three consequences are ones DelayTrigger already
lives with, so none are hypothetical. Somebody still has to agree to them.

### Refs

`docs/ref/workflows.md`, `docs/plans/ggraph-engine-extraction.md`,
`docs/plans/time-gate-should-not-hold-a-thread.md`, `docs/plans/disk-exhaustion-breaks-everything.md`,
`crates/app-core/src/workflows/{steps/,differential.rs,registry.rs,host.rs,compat.rs}`.

---

## 2026-08-24 — the graph engine leaves home, and bishop starts consuming it

- **Branch:** `docs/handoff-engine-extraction` (this entry). Everything else is **merged to
  `main`**: !133, !134, !135, !136, !137, !138.
- **Commits:** `4f0fa6c` (ports) · `77297dd` (memoize + Edge + registry) · `e4c7b46` (#205 fix).
  Engine repo: `dfcff65`, tags `v0.1.0` → `v0.5.2`.
- **Engine repo:** [github.com/jackesdavid/graph-engine](https://github.com/jackesdavid/graph-engine)
  — **public**, `ggraph-core`, 125 tests, no consumer named in it (CI greps for both domain
  vocabulary and product names).

### Done

**The engine exists and is consumable.** Topology, open string identifiers, typed ports, values
with an `Extern` escape hatch, the `Host` seam, the node registry, a scheduler with durable
checkpointing, and 21 standard nodes. Proven from outside by a separate crate that pulls it over
git, implements `Host` against the public API only, and runs a graph — kept as `examples/host.rs`
and run by CI, not merely compiled.

**bishop consumes it.** `PortType`, `Port`, `PortName`, `Edge`, the compatibility rule, and the
91-kind registry that the catalog and wire validation read. `SentinelHost` implements `Host`;
`compat` bridges `PortValue` ⇄ `Value` with one wrapper for all ten domain types.

**`memoize` deleted end to end** — field, scheduler branch, API endpoint, editor switch, i18n,
tests. Checked in production first: 249 nodes off, two on, both in a graph with no cycle, so the
flag was never consulted.

**#205 fixed** — the canvas highlight follows the branch that ran. #115 had fixed the Console
line and stopped, because the highlight comes from `on_node` and the log from `on_log`, and only
the second was deferred.

**#197** — `clean-space` reclaims 29 G now (5.3 GiB → 38 GiB) and names what it left alone. The
cause was never Docker: 22 G of Xcode symbol and derived caches the old task never looked at.

### State

`check:rust` now RUNS the engine's tests instead of only compiling them — that was the single
most important change of the session and it is what made everything after it verifiable. 131
tests green, `svelte-check` clean, edge compiles, both lockfiles regenerated. The extraction was
**verified in production by the reporter**: one `value` pin on `tuya_control`, switch arms once,
old graphs opening, runs identical.

The #205 deploy (`e4c7b46`) was **still building** when this was written — the fix is merged but
not yet confirmed live. The reporter has confirmed the bug still reproduces on the deploy before
it, which is the expected before-half of the comparison.

### Pending / next

1. **Confirm #205 live** once `e4c7b46` deploys: same graph, Get on the dead arm no longer lights.
2. **`Graph<SentinelMeta>`** — bishop's `Graph` (14 methods) onto the engine's, via an extension
   trait. Prerequisite for the scheduler swap.
3. **The scheduler swap** — the 19 inline `n.kind ==` blocks become `NodeStep`. Needs a
   differential test (same graph, both schedulers, same node sequence) before it is safe.
4. **One node per file** — 70 functions, cosmetic, a family per MR.
5. **`ggraph-pg`** — the persistence adapter, plus the queue gaps the second product needs:
   priority, tenant fairness, heartbeat, durable dead-letter. None blocks Sentinel.

### Blockers

None on the code. Two judgement calls left with the user: whether to rewrite the engine's git
history to strip product names from old commit messages (would change every tag SHA and break
bishop's lockfile — recommended against), and whether `GetCameraByName` / `GetRecipient` should
get the same highlight deferral `GetVariable` now has.

### Refs

`docs/ref/workflows.md` (rewritten intro + a new "Highlight vs log" section + Build status),
`docs/plans/ggraph-engine-extraction.md`, `docs/plans/pure-sources-run-on-dead-branches.md`,
`docs/plans/disk-exhaustion-breaks-everything.md`.

---

## 2026-08-17 — push worked end to end, after TWO stacked 403s

- **Branch:** `main`, all merged and deployed. HEAD `1807f6a`.
- **Commits:** `8892899` (#178 Expo token) · `c823748` (mobile fullscreen + run list) ·
  `582f5df`/`8e23425` (cards #187–#190) · `ef90e90` (#187 phone test/delete) · `4e3ee23` (close #187).
- **MRs:** !105, !106, !107, !108, !109 — all merged.

**The headline.** Device Gate pushes had been failing since the TestFlight install. There were
**two** independent 403s stacked, and the second only became visible once the first was fixed:

1. **Expo refused us.** An org-owned Expo project has *Enhanced Security for Push Notifications* on,
   so every send without `Authorization: Bearer <access token>` is rejected. Fixed in
   `push.rs` (#178) and the token written into `/ibbf/sentinel.env` — never through a terminal.
2. **Apple refused Expo.** The `.p8` key `58958PJYH9` had been created **Sandbox-only**, while a
   TestFlight build registers a *production* device token → `BadEnvironmentKeyInToken (403)`.
   Replaced with `UN3WKYR259` (Sandbox & Production), uploaded to EAS by API. **No rebuild was
   needed** — push keys are server-side.

**What made this expensive is worth more than the fix.** Between the two, the run log read
`Device Gate: asked iPhone` → `expired → failure` — which is exactly what a person ignoring the
prompt looks like. We read Expo's *ticket* ("queued") and never its *receipt* ("APNs said no, and
here is why"). Card **#190** exists for that. The key was a ten-minute correction once named; the
silence cost the day.

**How the cause was actually pinned**, since deduction kept producing plausible wrong answers:
sending a deliberately invalid device token to both Apple hosts with a JWT signed by the key. APNs
validates the provider token before the device token, so production answering `403
BadEnvironmentKeyInToken` while sandbox answered `400 BadDeviceToken` isolated the key, with nothing
delivered to anyone. Recipe kept in `docs/ref/push.md`.

- **Done:** #178 (Expo token), the APNs key swap, #187 (test + remove a phone Recipient, validated
  in production by the owner on iPhone AND iPad), mobile fullscreen live video + compact run list.
- **State:** `task check` 0 errors. Deploy pipeline `2766568151` green, `sentinel-app` recreated.
  Push verified three ways: my receipt probe (`status: "ok"`), the product's own Test button, and
  the owner's devices. Android/FCM **not exercised at all**.
- **Pending / next:** **#188** (a dangling recipient auto-selects the first in the list — the most
  dangerous of the open set, and easy to trigger now that three devices have similar names) ·
  **#190** (read receipts) · **#189** (Recipients in tabs) · **#186** (Device Gate phone-only
  picker) · the half of #187 Phase 2 that counts affected workflows before deleting.
- **Blockers:** none technical. Left for the owner to decide: the stale `.vscode/settings.json` +
  `tasks.json` deletions and untracked `ai/.vscode/` sitting in the working tree, unrelated to this
  work and deliberately not swept into a commit. Also still open from #185: App Privacy answers,
  screenshots/listing, the iPad keep-or-drop call (an iPad is now registered and receiving push),
  and counsel review of the legal text.
- **Refs:** `docs/ref/push.md` (new), `docs/plans/push-receipts-not-just-tickets.md`,
  `docs/plans/dangling-recipient-must-not-autoselect.md`, `docs/plans/recipients-phone-parity.md`,
  `docs/plans/recipients-tabs.md`

---

## 2026-08-13 — #180 copy/paste rewritten to spec (paste at the cursor)

- **Branch:** `main`, deployed and CONFIRMED WORKING by the user. HEAD `898255d`.
  Open: **!81** (`docs/board-180-review`, board move #180 → Done, pipeline still running).
- **Commits:** `1243a69` (spec) → `87489dd` (implementation).

**What changed.** Copy/paste had been touched four times the day before, and every round fixed how
the *keystroke arrives* — never what the operation does. This card specified the behaviour, which
was wrong on three counts:

- **Paste lands at the cursor**, not at the original position `+48`. `hoverWorld` already tracked
  the pointer (it is what the `n`-to-add-here shortcut uses); the node is centred on it the same
  way `addNodeKind` centres a palette drop.
- **Wires do not travel.** This is also what makes "only the values chosen in the panel" free: a
  wired input's value lives on the edge, not in `config`, so dropping edges drops exactly those.
- **A clipboard entry is no longer a node** — `ClipNode` is `{kind, config, memoize, dx, dy}`, where
  dx/dy are offsets from the copied set's top-left, so multi-node copies keep their layout.

Three things the spec did not cover: `hoverWorld` is null until the pointer has been over the canvas
(viewport-centre fallback, or a paste from the Edit menu lands at NaN); repeated pastes at an
unmoved anchor cascade 24px instead of stacking; `parseNodePayload` validates rather than trusts,
coercing a missing dx/dy so an older payload still pastes.

**Duplicate was decoupled from the clipboard.** It used to be `clipboard = snap; pasteClipboard()`,
which would have dragged it along to "at the cursor". Behaviour unchanged, and it no longer clobbers
what you copied. It still has **no spec of its own** — that is the obvious next card.

- **State:** `svelte-check` clean. Deploy verified by fetching the served editor chunk
  (`35.CsCXGenx.js`) and finding the new `snapshotSelection`/`parseNodePayload` in it. User confirmed
  it works in the browser.
- **Pending:** merge !81. Write the duplicate spec. In Progress is unchanged: #174, #160
  (unblocked, unbuilt), #167, #151.
- **CI note:** `build:rust` failed again with `blob unknown to registry` pushing
  `sentinel-flow:latest` — same flake as 2026-08-12, cleared on retry. Second occurrence in two
  days; if it recurs it is worth a card rather than a retry.

- **Refs:** `docs/plans/editor-copy-paste-spec.md`, `sentinel-app/src/routes/workflows/[[id]]/+page.svelte`

## 2026-08-12 — Editor clipboard (four rounds), duplicate off ⌘D, and #179 Set/Get Variables

- **Branch:** `main` — everything merged and deployed. HEAD `c2e223e`. Open MR: **!78**
  (`docs/board-179-review`, board move only).
- **Commits:** `ba0c5f2` (copy/paste feedback) → `cf182fb` (native clipboard events) → `3a69d7a`
  (duplicate off ⌘D) → `88f6675` (**#179** Set/Get Variables) → `0c49a11` (dual clipboard path).

### Done

**#179 Set Variables / Get Variables** — the multi form of the variable nodes. One node instead of
a column: the face pipeline needed sixteen single nodes and thirty-two wires to move eight values.
Config is `variables: [{name,type}]`; each entry derives one port NAMED after the variable
(`multi_variable_ports` in `domain.rs`, mirrored in `effectiveKind()`). Properties panel has an
*Add a variable…* picker + per-entry remove and a drift/resync marker. Full detail and the three
load-bearing rules are in `docs/ref/workflows.md` § Variables.

**Editor clipboard — four rounds, and the last one is the honest fix.** ⌘C/⌘V/⌘D were reported
broken three times running. Each fix closed one hole in a *reconstruction* of ⌘C from `keydown`
(match the key, check modifiers, hope nothing claimed it first): the `V` tool had been eating ⌘V;
an armed graph refused to copy and said nothing; a focused inline input swallowed everything.

Replacing keydown with the browser's real `copy`/`cut`/`paste` events was better — it carries the
system clipboard, so nodes now paste **between graphs and tabs** — but it was still an assertion
about a mechanism I had not verified. A browser only guarantees those events when the page has a
selection or an editable focus, and this canvas is an SVG with neither.

Final shape: **both paths, interlocked.** The native event stamps a timestamp; the keydown handler
queues a fallback that cancels itself if the stamp moved. Exactly one runs, never both — ordering
matters because keydown fires *before* the clipboard event, so acting immediately would double-paste
wherever the native path does work.

**Duplicate moved off ⌘D to a bare `d`.** ⌘D is Chrome's bookmark shortcut. `preventDefault` usually
suppresses it, but "usually" is not something to build a shortcut on, and the ⌘D binding was removed
rather than kept as a fallback — both firing means bookmarking AND duplicating on one keystroke.
`d` joins the existing bare tool letters (`v` `h` `l` `b`), which no browser contests.

Also: **copy and paste now say what they did.** Every failure mode had been silent, so an empty
selection was indistinguishable from a broken editor. That toast is the diagnostic that was missing
for three rounds.

### State

- `cargo check -p app-core` clean; three new `domain.rs` tests pass (port derivation,
  blank/duplicate dropping, pure/exec split). `svelte-check` 0 errors.
- Deploy **verified by grepping the served bundle and the running containers**, not the pipeline UI —
  `multi_set_variable` and `olharai/workflow-nodes@1` are both in
  `_app/immutable/nodes/35.CnUFrj3-.js`, and app/control/flow all restarted together.
- **Not verified:** that the clipboard works in the user's browser, and #179 has never run on a real
  graph. Those are the two open questions, and I have been wrong about the first one twice.

### Pending / next

- **#179 → Review.** The real test is rewiring the face graph's sixteen variable nodes down to two.
- **#160 RLS is NO LONGER BLOCKED** — the board and plan still say "blocked on #173", but production
  control connects as `sentinel_app` (`rolsuper=f`, `rolbypassrls=f`). RLS itself is unbuilt:
  **0 of 64 public tables** have row security on. Phase 2 onward is real work ending in a production
  cutover that must be proven against a restored copy first. **Update the plan's blocked note.**
- **#174 Faces** — steps 1–2 shipped; step 3 (capture volume) gates 4 and 5, then 6, 6.5, docs last.
- **#167 ToS/Privacy** — the consent plumbing is buildable now; the legal text is not mine to write.
- **#151 §1.8** — mid-soak.
- **#60 phase 2** — hold-Space pan in the MAP editor; needs a force-pan flag, not a mode switch
  (the map has no pan tool to switch to).

### Blockers

- Clipboard + #179 both need **user confirmation in a real browser**.
- #167 needs legal copy from the user.
- Device Gate test graph is still armed in production — the user said to leave it.

### Notes on the day's CI

Three unrelated infrastructure flakes, none of them code: two OrbStack socket drops mid-restart
(`docker.sock: no such file or directory`) and one GitLab registry `blob unknown to registry` while
pushing `sentinel-flow:latest`. All cleared on retry. Also: local `main` was tracking
`origin/docs/step3-prerequisites`, so `git pull` on main was quietly pulling a docs branch — repointed,
but worth checking in other clones.

- **Refs:** `docs/ref/workflows.md`, `docs/workflow-nodes.md`, `docs/plans/multiset-multiget-nodes.md`,
  `sentinel-app/src/routes/workflows/[[id]]/+page.svelte`,
  `crates/app-core/src/workflows/{domain.rs,node.rs,node/variable.rs}`

## 2026-07-22 — Tuya v2: friendly labels, dynamic typed I/O, self-service BYO project (#131)

- **Branch:** `main` — pushed + deployed via CI (multiple deploys, all clean). HEAD `03c8d2d`.
- **Commits:** `31a06c2` (labels + dynamic value input) → `86d49a9` (hide value_type) → `f94236a`
  (per-tenant BYO creds) → `cef3e44` (privacy: strip global env fallback) → `5dd9b5e` (platform.tuya.com)
  → `f19a295` (autocomplete off on credentials) → `9d7537a` (dynamic typed read output) → `03c8d2d`
  (ForEach `completed` last).
- **Done (card #131 → Done, issue #11):**
  - **Friendly function labels** — name each data point (`switch_1 → "Luz Escada"`) on Integrations;
    stored in `tuya_devices.function_labels_json`, preserved across sync. `PATCH /tuya/devices/:id/labels`.
  - **Dynamic typed value** — Control's `value` **input** + Read's `value` **output** are typed by the
    selected data point (Boolean→Bool/Integer→Num/Enum·String→Text), baked into `config.value_type`
    (hidden), resolved in `GraphNode::input_ports`/`output_ports` like Set Variable.
  - **Self-service "bring your own Tuya project"** (migration `0068_tuya_config`) — each tenant connects
    its OWN Tuya project (guided walkthrough + Access ID/Secret form, validated before save). **Strictly
    per-tenant, NO env fallback** (privacy — a tenant never sees another's devices). The HA/LocalTuya
    pattern. `PUT/DELETE /tuya/config`, `provider` returns `{configured,source}`, RBAC `tuya.write`.
  - **Autocomplete off** on all credential fields (Tuya/camera → `off`, new-password fields →
    `new-password`); login kept intact.
  - **ForEach `completed` pin renders last** (below item/index) — clearer loop layout.
- **State:** all deployed + **dogfooded end-to-end in prod** (BYO connect → sync → toggle relay; the
  user connected the Default tenant via the new flow). `cargo check --workspace` + `svelte-check` clean
  on every deploy. Migration 0068 validated on live DB.
- **Key finding (documented in docs/ref/tuya.md):** self-service to ONE Sentinel project is impossible
  without an OEM app (OAuth = OEM-only; user/password = `1106`; console-QR automation = ToS-gray +
  account-suspension risk; `x-frame-options: deny` + SOP + captcha block every browser trick). BYO
  project is the clean, standard answer.
- **Pending / next:** #132 (filter non-controllable String data points from the node dropdown), #133
  (local control via edge using `local_key` — offline/fast). OEM app = deferred product call.
- **Blockers:** none.
- **Refs:** **docs/ref/tuya.md** (fully rewritten: BYO model + options map + local-control note + runbook).

---

## 2026-07-21 — Tuya smart-home integration shipped (#129) + memoize bug (#130) + a migration outage

- **Branch:** `main` — **pushed + deployed via CI**. Tree clean. HEAD `acb111e`.
- **Commits:** `456f9cd` (memoize fix) → `5d791f2` (board) → `9c4f732` (merge Tuya #129) →
  `eeb7ec1` (board skill → GitHub-first) → `acb111e` (hotfix: renumber migration 0062→0066).
- **Done:**
  - **Tuya integration (#129 → Done, issue #9 closed) — LIVE in prod.** New third-party provider +
    2 workflow nodes (**Control Tuya Device** / **Read Tuya Status**, category Smart Home). app-core
    `tuya/` module (HMAC-SHA256 client + repo + domain), migration **0066_tuya_devices**, control
    `/tuya/*` routes + RBAC `tuya.read`, editor dropdowns (device/code by name) + adaptive value
    widget, **Settings → Integrations** page. `TUYA_ACCESS_ID/KEY/ENDPOINT` set in
    `/ibbf/sentinel.env`. Full detail: **docs/ref/tuya.md**.
  - **Bug #130 (Done):** new workflow nodes now persist `memoize: false` (frontend was omitting it →
    backend `default_true` flipped loop bodies ON, silently breaking iteration). Get/Set Variable
    `variable`+`type` are read-only in the inspector. Deployed + tested.
  - **Board process:** cards are now real **GitHub Issues assigned to `@me`** (not draft items) —
    codified in `.claude/skills/board/SKILL.md` + memory. The board skill was rewritten to be
    **GitHub Project #3 first** (`docs/Task Board.md` retired — don't edit it).
- **State:** verified in prod — Integrations shows **Connected** + "Entrada" **Online**; the
  Control node **toggled the physical relay** end-to-end. control+flow booted clean (migration 66
  applied, success). `cargo check --workspace` + `svelte-check` were clean pre-merge.
- **⚠️ Incident:** numbered the tuya migration `0062` trusting the **stale CLAUDE.md** "next 0062"
  note — collided with `0062_faces_labeling` (repo was already at 0065) → **control+flow boot-loop
  ~12 min** (gateway/live video stayed up). Fixed by renumber → `0066` + redeploy. **CLAUDE.md now
  says always `ls` the migrations dir.** Lesson in docs/ref/tuya.md.
- **Pending / next:** nothing blocking. Tuya v1 is **Default tenant only** (console QR link);
  self-service multi-tenant pairing is future work (`tuya_uid` column is the hook).
- **Blockers:** none.
- **Refs:** **docs/ref/tuya.md**, `crates/app-core/src/tuya/`, `crates/app-core/src/workflows/node/tuya.rs`,
  `sentinel-control/src/domain/tuya.rs`, `sentinel-app/src/routes/settings/integrations/+page.svelte`.

---

## 2026-07-19 — Faces: recognition pipeline hardening + the labeling/clustering workspace (#117, #119)

- **Branch:** `main` — **ahead of origin by 17 commits, NOT pushed** (push on resume). Everything
  committed, tree clean. All faces work is **deployed** to prod except the final `1ea8fe6` roll —
  a flow+control deploy (`deploy-nogate.log`) was **still building at handoff time**; verify it
  recreated flow+control and then it's fully live.
- **Commits:** `1ea8fe6` (StoreFace pure sink) ← `c66c009` (delete both objects) ← `0adec6d`
  (pre-crop + migration 0063) ← `723044c` (black-tile 404 fix) ← `9469080` (StoreFace conf/ratio
  ports) ← `e32bf56` (detail hero + re-match) ← `98cfc11` (found=labeled) ← `a40c4b5` (delete +
  crop-to-face + Context) ← `585f254` (scope filter) ← `72778f5`/`e08c114` (canvas P1/P2) ←
  `7cf1e22` (grouping slider + autocomplete) ← `435e40b` (GPU image actually uses GPU) ←
  `2dfd3e5`/`d9dc42e` (embed_face_crop fallback) ← `f991057` (grid view).
- **Done:**
  - **Faces workspace (#119)** — grid `/faces` + **2D similarity canvas** `/faces/canvas` (pan/zoom,
    LOD islands, Unknown/Known/All scope, click→**hero detail** w/ metadata + Assign-move-fly + Delete
    + Context), live **Grouping ε slider**, autocomplete labeling, per-face + per-island delete.
  - **Pipeline fixes (#117):** `embed_face_crop` never drops a face (detect→align-free fallback);
    `RecognizeFace found` = **labeled** match only (unknown-matches-unknown was firing True).
  - **Image proxy:** **pre-crop at capture** (crop_key passthrough, migration 0063) + Cache-Control
    immutable + retry + client throttle — killed the black-tile 404s under load. Delete purges both
    frame + crop.
  - **StoreFace** persists real confidence/face_ratio (new input ports); quality gating reverted OUT
    of the node (pure sink) — belongs in the graph.
  - **GPU sidecar bug found+fixed** (`sentinel-ai:gpu3`): image silently ran InsightFace on CPU
    (onnxruntime CPU wheel shadowed the GPU wheel; CUDA libs off the linker path). Live RunPod A4000
    benchmark: face_detect_embed 529 ms CPU → 218 ms GPU (only ~2.4×, overhead-bound). **Decision:
    stay on CPU, scale via a dedicated CPU box, not GPU.** Endpoint torn down.
- **State:** app builds + svelte-check clean (9 pre-existing baseline); workspace `cargo check` clean;
  app-core tests green. Migrations 0061/0062 applied live; **0063 validated + applied live** (idempotent).
  Image proxy verified (crop vs box; 39-concurrent → 0 404s after fix).
- **Pending / next:**
  - ~~Run `POST /faces/backfill-crops`~~ **DONE 2026-07-19** — 153 backfilled, **172/172 faces have
    crop_key**; verified crop read = 1.1 KB passthrough vs 25 KB frame (box=1).
  - **Wire `When Event.confidence/face_ratio → Store Face`** in the "Face Test" graph (`5ef37805…`)
    so detail shows real values (currently 0 for old rows). **Quality gate** = a 4-node graph pattern
    (NumGte×2 → And → Branch before embed), NOT StoreFace config — must be added in the UI.
  - ~~#3 face landmark points in the detail~~ **DONE 2026-07-19** (`b5d8753`) — shipped **on-demand**
    instead of capture-time: `GET /faces/:id/landmarks` runs the sidecar's cheap `face_detect` on the
    stored crop → "Face points" toggle draws the 5-keypoint constellation over the crop (one SVG,
    shared pixel viewBox). Works for ALL existing faces, no edge/migration. Detail panel widened 420→480.
    Verified: endpoint returns kps in ~0.9s.
  - **Canvas lasso** (Phase 3): Shift-drag a lasso, then a click at the right spot opens the **bulk
    label modal** for the enclosed faces (select-across-islands → stamp-label).
  - **Canvas cap params:** (a) **max samples per island** — cap faces rendered per island (perf/density;
    show "+N more"); (b) **max islands** — cap total islands returned. Both belong on `layout_faces`
    (`?max_per_island=`, `?max_islands=`) + UI controls.
  - **Auto-refresh dropdown** on the canvas — like the Events page (pick an interval; re-fetch layout on
    a timer).
  - More **filters** (by-date, by-similarity).
- **Blockers:** graph edits can't be applied programmatically — persistence is replace-all + the
  direct prod-DB write is safety-blocked; hand to the UI. User was live-editing the graph.
- **Refs:** memory `faces-workspace` / `face-pipeline-architecture` / `storeface-pure-sink` /
  `face-recognize-found-labeled` / `faces-clustering-and-proxy` / `face-compute-stays-cpu`;
  `crates/app-core/src/faces.rs`, `sentinel-control/src/domain/faces.rs`,
  `sentinel-app/src/routes/faces/{+page,canvas/+page}.svelte`, `ai/sidecar/testdata/` (CPU-vs-GPU harness).


## 2026-07-17 — Dictionaries (#113 P1-3), editor toggles (#112) + a recording bug

- **Branch:** `main` — **ahead of origin by ~4 commits, NOT pushed** (push on resume). #112 + #113 P1-3
  are **deployed** to prod (control + app recreated, migration 0059 applied). `sentinel-flow` NOT
  redeployed (no runtime changes landed yet).
- **Commits:** `6b1c9ca` (#113 P1-3 dictionaries) ← `d61dd0a` (#112 editor toggles) ← `69e22bc`,
  `b14215b`,`26b2873`,`7f1fe6a`,`fec3454`,`a266334` (#102 templates/perf/stagger, earlier this run).
- **Done:**
  - **#112 — Enabled/Debug/Orphans → top-of-canvas icon toggles.** Moved the three per-graph state
    toggles out of the Details tab into a floating cluster beside the transport (Enabled `Power` →
    Debug `Bug` → Orphans `Unplug`), HUD active-tint idiom; armed toggle gets a `status-live` pulse +
    ring. Same handlers/endpoints/guards. In Progress (awaiting `/board review`).
  - **#113 — Dictionaries, Phases 1–3** (named typed-column TABLES, reusable by reference). Migration
    0059 + `app_core::dictionaries` (domain + CRUD + **atomic `SELECT … FOR UPDATE` row mutators**,
    3 tests green); `PortType::Table` + `PortValue::Table`; control `/dictionaries` CRUD (delete
    guarded while referenced); `data.ts` methods+types; `table-editor.svelte`, `/workflows/dictionaries`
    full view, `dictionary-modal.svelte` (big modal), VariablesPanel Table reference editor + a
    Dictionaries nav button. In Progress.
- **State:** `task cargo` clean (1 pre-existing warning); `cargo test -p app-core --lib dictionaries`
  3/3 green; app `npm run build` clean; svelte-check **9 errors, all pre-existing** (sensor-field ctx,
  MapScene Drag ×2, `pending` null) — 0 new. Prod verified: control+app `Up`, `_sqlx_migrations` has 59.
- **Pending / next:**
  1. **#113 Phase 4 — runtime + dictionary nodes** (the piece that makes dictionaries usable *inside*
     graphs, and the user explicitly asked for it). `ToolContext.dictionaries` snapshot + `parse_variable_value`
     Table arm; add read/mutate Dict nodes to `NodeKind::ALL` + `node.rs` (mutators already atomic via
     `dictionaries::mutate`); update `docs/workflow-nodes.md`; **redeploy sentinel-flow**. Full spec in
     the plan's Build-status section.
  2. **#114 (Bug) — Stop Recording "missing required input 'camera'"** on the DelayTrigger trailing
     edge. Root-caused: the `stop` fires in a **separate timer run**; `sys.variables` is per-run (fresh
     empty map), so `Get current_camera` in the stop run reads type-zero "" though the graph is wired.
     0058 restored node *outputs* across the window but not the *variables map*. Fix directions in
     `[[stop-recording-missing-camera]]` (lean: snapshot+restore the vars map like 0058, keyed by
     instance).
  3. User asked to implement "a card to show the workflow current data saved" — **likely #95** (Graph
     instances → live instance view) or **#94** (unified node value store). Confirm which on resume.
  4. Push `main` to origin.
- **Blockers:** none — user stepped away mid-session ("handoff this section"); resume with Phase 4.
- **Refs:** `docs/plans/dictionaries-tables.md` (Build status), `docs/plans/stop-recording-missing-camera.md`,
  `crates/app-core/src/dictionaries.rs`, `sentinel-control/src/domain/dictionaries.rs`,
  `sentinel-app/src/lib/components/{table-editor,dictionary-modal}.svelte`,
  `sentinel-app/src/routes/workflows/dictionaries/+page.svelte`.

## 2026-07-15 — Edge perf + the whole install experience (#77–#83)

- **Branch:** main (all committed **and pushed**; 0 ahead of origin). Control + app + edge 0.5.39
  **deployed** to prod.
- **Commits:** `ef48609` (#83 Windows installer bundle) ← `7c92fd3`,`9da44d0` (#80 Windows cross-compile
  + runtime-aware) ← `244246a`,`f716ffc`,`608ba55`,`e85c24a`,`402e4f0` (#82 macOS .pkg signed) ←
  `74885ea`,`d811142`,`77abfec` (#81 pairing) ← `1fa8094`,`904819c`,`a689f57`,`9090b5a` (#79 no OpenCV)
  … and #77/#78 earlier in the run.
- **Done (one long session):**
  - **#77 frame hub** — one fetch+decode per camera shared by motion + every tracker; **motion-gated
    tracking** (YOLOX only runs on motion / live tracks / warmup / 30s sweep). `src/frames.rs`.
  - **#78 clip local retention** — opt-in `delete_local_after_upload`; Download prefers R2. Settings toggle.
  - **#79 no OpenCV** — motion is pure-Rust MOG2 (`src/mog2.rs`), **proven pixel-identical to OpenCV**
    (`cargo test --features opencv-parity`). Edge links nothing native now; reaped OpenCV from
    install.sh + Linux Dockerfiles (no more reboot on Bazzite). THIS is what unblocked Windows.
  - **#81 pairing** — enroll by short code (migration 0055 `box_pairings`, `POST /enroll`), edge serves
    a pairing page when unenrolled (`src/enroll.rs`), reads its own `edge.env` (`src/envfile.rs`).
    Hardened `/config/cameras` no-token fallback behind `cfg!(debug_assertions)`. **Validated e2e vs prod.**
    ourselves (LGPL, universal, signable). `deploy/macos-pkg/`. **DONE.** See [[apple-signing-82]] memory.
  - **#80 Windows edge** — cross-compiles from macOS with **zero code changes** (mingw-gnu target,
    `task edge-windows-build`); runtime-aware (`src/paths.rs` %ProgramData%, CREATE_NO_WINDOW on all
    child spawns, self-update gated). 70MB self-contained .exe.
  - **#83 Windows installer** — `deploy/windows/{install,uninstall}.ps1` + `build-installer.sh`: real
    Windows SERVICE via WinSW (auto-start, restart-on-failure), pairing page. enroll.rs exits 1 on
    Windows so WinSW relaunches after pairing. **Zip published:**
    `https://control.thesentinels.online/edge/SentinelEdge-Windows-0.5.39.zip`.
- **State:** macOS/Windows both build clean; MOG2 parity + gate + envfile tests pass. macOS .pkg
  validated on real hardware end-to-end (pairing → token/key minted → cameras pulled → online).
- **Pending / next:** ⏳ **User tests the Windows installer TONIGHT on their box (RTX 2060 Super).**
  Watch for: service reaches `Running` (WinSW), box goes online after pairing (the exit-1 relaunch
  Then: an Inno Setup **.exe GUI wrapper** + **code-signing cert** (deferred, needs user $) close #83;
  a **per-platform download button** in the app (serve .pkg + .zip) closes the "customer downloads it"
  journey; and an **enrollment-UX** intermediate "Code accepted — waiting for the appliance…" state.
- **Blockers:** #83 final polish waits on the Windows test result + the signing-cert decision. #82's
  only remaining touch is re-signing under the org account once Apple approves JDV GLOBAL SOLUTIONS
  LTDA (D-U-N-S 574899435, emailed 2026-07-14).
- **Refs:** docs/ref/edge.md (frame hub / MOG2 / pairing / no-OpenCV sections), docs/plans/{edge-frame-hub,
  clip-local-retention,edge-pure-rust-motion,box-pairing-code,edge-macos-pkg,edge-windows,edge-windows-installer}.md,
  deploy/{macos-pkg,windows}/, memory [[apple-signing-82]].

---

## 2026-07-12 — Visual identity (#68/#69): built, then direction reset (PAUSED)

- **Branch:** main (committed, **NOT deployed** — deploy intentionally held)
- **Commits:** `0868d82` (Sora logo lockup #68), `e43c3de` (favicons/OG #69), `c1c6d8c`
  (svg-design skill), `81b2f96` (guarded-perimeter mark 05a across logo/favicons/OG), + this stop.
- **Done:** Shipped a full monochrome identity — `SentinelLogo` lockup (dot-field→**05a
  guarded-perimeter** mark + Sora sentence-case wordmark), self-hosted Sora `--font-brand`, all
  favicons/app-icons/PWA manifest/OG (generated via sharp/png-to-ico/headless-Chrome), `docs/ref/brand.md`.
  Installed two logo skills (svg-design tryopendata, logo-designer neonwatty).
- **State:** `npm run build` clean; login lockup + app icon + OG verified in-app. All committed, **not deployed**.
- **THE PIVOT:** user rejected the dot-field + every wordmark typeface + a hand-built monoline as
  "too generic", then **reset the whole direction**: drop monochrome+dots → **warm & human,
  abstract mark, one warm accent colour**. 5 warm concepts generated (coral/amber/terracotta/**rose
  ribbon**/tangerine), saved to `docs/plans/logo-exploration/`. **User has not picked yet.**
- **Pending / next (resume here):** show `docs/plans/logo-exploration/warm-concepts-overview.png`,
  get the pick, Phase-3 refine the chosen warm concept, then rebuild the mark + favicons + OG +
  lockup to REPLACE the committed 05a+Sora identity, revise the identity docs/memories
  ([[design-system]], [[monochrome-rebrand]], impeccable, brand.md), and deploy as one drop.
- **Blockers:** none technical — awaiting the user's concept pick. Do NOT deploy 05a+Sora as-is
  (likely to be superseded).
- **Refs:** docs/plans/logo-brand-mark.md (⚠️ session-state section at top), docs/ref/brand.md,
  docs/plans/logo-exploration/, sentinel-app/src/lib/components/{sentinel-mark,sentinel-logo}.svelte


## 2026-07-11 — Playback Timeline (#67) planned + researched (no code)

- **Branch:** main (docs only; not deployed — nothing to deploy)
- **Commits:** docs commit for board card #67 + plan note + this entry
- **Done:**
  - `/board plan` → new card **#67 "Playback Timeline — unified inspection view (blocked on #66)"**
    in ToDo, plan note `docs/plans/recording-playback-timeline.md`.
  - This is the **apex inspection screen** the user wants: the single place to see *all* events
    from *all* modules laid along one playback clock, multi-camera in sync, jump ahead/back across
    all cameras, detection bboxes on the viewport, and (future) tracker trajectories moving along
    playback. It's the "time-travel Live module" #66's plan explicitly deferred.
  - `/impeccable` pass: reconciled the user's rainbow NLE mockup with the monochrome ink theme
    (cameras keyed by ink label not hue; markers = severity dots not flags; ink playhead + live
    `.status-live` ping). Refined mockup at
    `…/scratchpad/playback-timeline-impeccable.html` (scratchpad — throwaway, not in repo).
  - Research pass folded into the plan appendix: rVFC master/slave multi-video sync, ~4–9
    simultaneous decode ceiling + keyframe-snapshot/Birdseye fallbacks, fMP4/HLS+PROGRAM-DATE-TIME
    seeking, RTCP clock alignment, Frigate/Shinobi prior art, tracker-replay data model
    (timestamped polylines, model (c)).
  - Grounding win recorded: the edge **already runs a ByteTrack `MultiTracker`**
    (`crates/app-core/src/tracking.rs`) — future "trackers moving along video" = persist its
    sampled trajectory, not a new tracker. Reuse `recordings.list`/`cloudUrl`, `events.list` (#66),
    `/live` MSE plumbing.
- **State:** planning only — no build/check/cargo run (no code changed). #66 Phase 1+2 already
  landed on main (commits 722e477, e51737b, 0a602b8).
- **Pending / next:** BLOCKED on #66 completing. When it lands, spike the two early-risk decisions
  first — **container format (#8: MP4 vs fMP4/HLS, touches the edge recorder + #64)** and
  **decode budget (#10: benchmark simultaneous 1080p decode on target hardware)**. Then build
  Phase 1 (static multi-track day view).
- **Blockers:** #66 (agnostic event module) not fully done — this view consumes the finished event
  stream as its correlation backbone.
- **Refs:** docs/plans/recording-playback-timeline.md, docs/plans/agnostic-event-module.md,
  crates/app-core/src/tracking.rs, crates/app-core/src/events.rs, sentinel-app/src/lib/api.ts
  (`recordings`, `events`)


## 2026-06-21 — Motion-triggered stream recorder (full implementation)

- **Branch:** main (commit 5d32d0b, deployed)
- **Commits:** 5d32d0b
- **Done:**
  - DB migration 0030: `tenant_boxes.recording_config` JSONB + `recordings` table (RFC3339 TEXT timestamps)
  - sentinel-control: `recordings.read/write` permissions; `GET /recordings`, `POST /recordings`, `DELETE /recordings/:id`, `GET /recordings/:id/download` (proxies bytes from the edge); `PATCH /boxes/:id/recording-config`; edge-authenticated `GET /config/recording` + `POST /config/recordings/clip`
  - app-core: `StartRecording` + `StopRecording` NodeKind (ALL 48→50); edge host resolved from `tenant_boxes` via `sys.db`; ureq json feature enabled for `into_json`
  - sentinel-app: `Recording` type + `recordings` API (list/del/downloadUrl); `RecordingConfig` type + `boxes.patchRecordingConfig`; Recordings nav item (Clapperboard); `/recordings` page (table, empty state, download link, delete button); `recordings.read` NAV_PERM gate
  - E2E: `e2e/recordings.spec.ts` — empty-state check, seeded rows appear, delete removes row; seeds via `POST /recordings` (JWT with recordings.write)
- **State:** `task cargo` clean, `task check` no new errors (9 pre-existing), `task build` clean; deployed sentinel-app + sentinel-control (commit 5d32d0b)
- **Pending / next:**
  - `/recordings` page settings section in boxes (recording config UI — `pre_roll_secs`, `max_storage_mb`, `codec`)
  - sentinel-flow: `StartRecording` + `StopRecording` are wired in domain.rs/node.rs but the edge binary hasn't been rebuilt/deployed — edge recording routes exist but require a local binary push (see deploy ref)
  - E2E: tests skip gracefully if seeds fail (403 — recordings.write not in test tenant role). May need to grant recordings.write to the test tenant's viewer role, or the seed user needs Admin role
  - Run `npm run test:e2e -- recordings` against prod to verify
- **Blockers:** None — edge binary deploy requires local build (GH Actions billing-blocked)
- **Refs:** `docs/plans/motion-triggered-recorder.md`, `crates/app-core/migrations/0030_recordings.sql`, `sentinel-control/src/domain/recordings.rs`, `sentinel-edge/src/recording.rs`, `sentinel-app/src/routes/recordings/+page.svelte`

---

## 2026-06-20 — Wire-bend UX + E2E test suite + prod seed

- **Branch:** `main` (all work merged via PRs #2–#6; pushed)
- **Commits:** `529ac87` → `ec958e5` (PRs #2–#6 merged + final test fix)
- **Done:**
  - **Wire reroute UX (PR #2):** Fixed hover/drag — moved `onpointerenter/leave` from inner `<path>` to `<g>` wrapper so the bend dot doesn't vanish when the cursor slides from the hit-path to the dot.
  - **Direct wire bending (PR #3):** Removed the "reroute pin" intermediary. Click+drag anywhere on a wire to bend it at that point. Alt+click wire to delete the edge. Alt+click the bend dot to straighten back to a single bezier.
  - **Svelte reactivity fix (PR #4):** `edgeVias` was a `Map` — `Map.set()` mutations don't trigger Svelte 5 re-renders. Changed to a plain `$state<Record<string, {x,y}>>({})` object; assignments (`obj[k] = v`) are always reactive.
  - **E2E suite (PR #5):** `sentinel-app/e2e/workflow-wire-bend.spec.ts` — 3 Playwright tests: real-time drag reactivity (guards the Map bug), bend handle persist + alt+click straighten, alt+click wire delete. `playwright.config.ts`, `global.setup.ts`, `.env.test.example` added.
  - **Prod seed + fixture fixes (PR #6):** `e2e/seed.ts` — one command (`npm run test:e2e:seed`) creates `e2e-sentinel@thesentinels.online` on prod control (signup or login fallback) and writes `.env.test` with credentials + `E2E_TOKEN`. Fixed: `apiToken` sync (no localStorage read), empty-body API responses, unique timestamped workflow names per run, `<tr>` XPath ancestor click.
  - **SVG visibility fix (direct commit `ec958e5`):** Playwright marks SVG `<path>` elements with `fill:none` as "hidden". Changed assertions to `toBeAttached()`.
  - **Deployed:** `task cloud-deploy -- sentinel-app` — prod running commit `c0b81c3` with `data-canvas` attribute on the SVG canvas.
- **State:**
  - All 4 E2E tests pass locally (localhost:5173 → prod control) and against prod: `4 passed (37.4s)`.
  - `npm run test:e2e:seed` creates the test account idempotently.
  - No Rust changes this session; `cargo check` not re-run (not needed).
  - `svelte-check` not re-run (no type-unsafe changes).
- **Pending / next:**
  1. **Wire bend persistence** — bends (`edgeVias`) are in-memory only; lost on page refresh. Fix: add `via: Option<{x,y}>` to `Edge` struct in `crates/app-core/src/workflows/domain.rs` (graph is stored as a JSON blob so no migration needed), add `PATCH /workflows/:id/edges/:idx/via` endpoint in `sentinel-control/src/domain/workflows.rs`, frontend saves on `pointerup` in `_onCanvasUp` and populates `edgeVias` on graph load.
  2. Stale test workflows on prod e2e account (IDs 1–12, all named `__e2e_wire_bend__` from prior debug runs). Safe to delete manually via the Sentinel UI or a one-off curl — they belong to `e2e-sentinel@thesentinels.online`.
  3. GPU sidecar CUDA mismatch (ongoing from previous session — see 2026-06-19 entry).
- **Blockers:** none.
- **Refs:** `sentinel-app/src/routes/workflows/+page.svelte` (edgeVias, onCanvasMove/Up), `crates/app-core/src/workflows/domain.rs:1037` (Edge struct), `sentinel-control/src/domain/workflows.rs:53` (edge routes), `sentinel-app/e2e/`

---

## 2026-06-19 — GPU box setup: network + SSH + sidecar build (CUDA mismatch blocked)

- **Branch:** worktree `scalable-bouncing-hamming` off `new-ui`; plan note committed to `feat/map-redesign` via cherry-pick
- **Commits:** `d966e5f` (face-recognition-next-steps plan + board card, on `feat/map-redesign`)
- **Done:**
  - **Face recognition plan note:** `docs/plans/face-recognition-next-steps.md` — full state snapshot + 4-phase next-steps (DetectFaces node, live-feed triggers, GPU box upgrade, hardening). Board card added to ToDo.
  - **GPU box access:** Bazzite Linux box (`100.84.250.126`). SSH alias `gpu-box` in `~/.ssh/config`, key `~/.ssh/id_rsa`. NOPASSWD sudo for `/usr/bin/podman` via `/etc/sudoers.d/sentinel-ai`.
  - **GPU confirmed:** RTX 2060 SUPER, driver 595.71.05 (CUDA 13.2 capable), CDI config present (`/etc/cdi/nvidia.yaml`), GPU passthrough works with `sudo podman`.
  - **Sidecar image built:** `localhost/sentinel-ai:gpu` (8.4 GB) on the box; container `sentinel-ai` running on port 8000. `Dockerfile.gpu` on box patches `FROM` with `docker.io/` prefix (Podman Fedora policy).
- **State:** Sidecar running but on **CPUExecutionProvider** — GPU not active. `torch.cuda.is_available()` = True; problem is onnxruntime-gpu CUDA version.
- **Blocker — CUDA version mismatch:**
  PyPI onnxruntime-gpu skips CUDA 12: ≤1.17.x links `.so.11`, ≥1.19.x links `.so.13`. Dockerfile base (`pytorch:2.4.1-cuda12.4`) ships `.so.12`. CUDA 12.4 apt packages installed in running container. **Try `onnxruntime-gpu==1.18.1` next** — likely the CUDA 12 sweet spot. Fallback: direct wheel from ONNX Runtime GitHub releases.
- **Pending / next:**
  1. `ssh gpu-box "sudo podman exec sentinel-ai pip install onnxruntime-gpu==1.18.1"` → `ldd libonnxruntime_providers_cuda.so | grep "not found"` → if `.so.12`, update LD_LIBRARY_PATH to CUDA 12.4 apt paths + restart
  2. If 1.18.1 still wrong CUDA, get wheel from https://github.com/microsoft/onnxruntime/releases
  3. Once GPU confirmed: update `Dockerfile.gpu`, rebuild image, smoke-test with a real face image
  4. Wire into VPS: `sentinel.env` → `AI_SERVICE_URL=http://100.84.250.126:8000/runsync` + `extra_hosts` in sentinel-control service
- **Blockers:** onnxruntime CUDA gap — autonomous if 1.18.1 works; needs user if Dockerfile rebuild required
- **Refs:** `docs/ref/ai-sidecar.md` (new this session), `docs/plans/face-recognition-next-steps.md`

---

## 2026-06-19 — Workflow nodes: ObjectDetection + comparison nodes + SSE animation fix

- **Branch:** `feat/map-redesign` (all workflow work on this branch); pushed via `task cloud-deploy`.
- **Commits:** `14ef865` (new nodes), `c8c1f78` (SSE animation fix).
- **Done:**
  - **Generic Object Detection node** (`object_detection`): any COCO class via dropdown or wired `class` Text input; outputs `count`, `confidence`, `detections` (PersonArray), `ok`. Wired class pin grays out the dropdown.
  - **Break Detection node** (`break_detection`): splits a single detection into `bbox` (NumArray) + `confidence`.
  - **Missing comparison/logic nodes:** `Or`, `Not`, `TextCompare` (eq/ne/contains/starts_with/ends_with) — all pure pull-based with inline expression labels in the canvas.
  - **Workflow node animations fixed:** `events_stream` in sentinel-control returned 502 when `REDIS_URL` was unset — the promised single-pod fallback was never built. Fixed: (1) `GET /events` SSE on sentinel-flow streams the in-process bus; (2) sentinel-control proxies it and filters by tenant when Redis is absent. Redis path (production) unchanged.
  - **`.env.example`**: documented `REDIS_URL` as optional with fallback note.
  - **Board:** "Node run animation lags" → Done.
  - **Deployed:** `task cloud-deploy -- sentinel-flow sentinel-control` in flight at handoff time.
- **State:**
  - `cargo check` clean for both crates before commit. `svelte-check` not re-run (no frontend changes this session).
  - Deploy was still building when handoff was saved — verify containers restarted on VPS.
- **Pending / next:**
  1. Verify animations work in browser after deploy settles.
  2. Move "Generic Object Detection" + "Missing comparison nodes" board cards → Done.
  3. RecordPresence room picker bug (Review column) still open.
  4. Map: wall depth card still Review; remove `[presence]` debug console lines.
  5. Release: bump web version + merge `feat/map-redesign` → `new-ui`.
- **Blockers:** none.
- **Refs:** `sentinel-flow/src/http.rs:171` (new `/events`), `sentinel-control/src/domain/workflows.rs:353` (`events_stream`), `docs/ref/workflows.md`, `docs/workflow-nodes.md`.

---

## 2026-06-18 — Map 3D wall depth polish + no-Docker deploy path

- **Branch:** `feat/map-redesign` (off `main`); pushed. Not merged to `new-ui`.
- **Commits:** `022dbb5`, `9526caa`, `c28f792` (this session, on top of `c885862`).
- **Done (all deployed to prod via no-Docker path):**
  - **Wall height:** uniform `0.8` for all solid walls regardless of thickness — CAD/AutoCAD feel; wireframe stays `0.04` (`WallSegment.svelte:24`).
  - **Wall material:** `roughness 0.8→0.65`, `metalness 0.06` added — painted-plaster/concrete surface feel (`WallSegment.svelte:39`).
  - **Lighting:** ambient `0.85→0.5`, key light `1.15→1.7`, shadow map `2048→4096`, frustum `±120→±80`, sun inclined `[40,70,24]→[90,38,20]` — deep one-sided shadows from the right (`MapScene.svelte`).
  - **Vertex snap:** `0.4→0.65` — wider magnetic zone so wall endpoints connect cleanly (`tools.svelte.ts`).
  - **Board:** "Map: solid walls taller, richer material, deeper shadows" card created (ToDo → In Progress); plan at `docs/plans/map-walls-3d-depth.md`.
  - **No-Docker deploy path confirmed:** `npm run build` → `scp` tarball to VPS → `docker cp` into running `sentinel-app` container → `nginx -s reload`. Used twice this session (Docker Desktop was down). HTTP 200 confirmed via public URL.
- **State:** web build clean (same 7 pre-existing warnings); not cargo-checked this session (no backend changes). Deployed to prod; `app.thesentinels.online` returning 200.
- **Pending / next:**
  1. Run `task cloud-deploy -- sentinel-app` once Docker Desktop is back (proper image rebuild so the no-Docker inject is superseded by a clean image).
  2. Confirm `c885862` (i32 presence fix) deploy is still live.
  3. Verify presence live in browser console (`[presence] pulse …`).
  4. Remove `[presence]` debug console lines in `lib/map/presence.svelte.ts` once confirmed.
  5. Release: bump web version + merge `feat/map-redesign` → `new-ui`.
- **Blockers:** Docker Desktop needs a restart before `task cloud-deploy` can run.
- **Refs:** `docs/ref/map.md` (Build status §7), `docs/plans/map-walls-3d-depth.md`.

---

## 2026-06-17 — Map polish + presence end-to-end + edge install fix (deployed)

- **Branch:** `feat/map-redesign` (off `main`); pushed. **Not** merged to `new-ui`.
- **Commits:** `592a3da` → `c885862` (this session, on top of the earlier `321dc70…b39bcbf`).
- **Done (all deployed to prod incrementally):**
  - **Edge install:** `install.sh` self-heals the macOS system daemon on both full install and
    `--update` (idempotent, waits + warns) — fixed OfficeBox showing "offline". Moved the box
    install command from a modal to its own view
    (`/settings/boxes/install`, `lib/box-install.svelte.ts`).
  - **Map crash:** `lib/map/{history,tools,view3d}` were plain `.ts` using runes → `$state is not
    defined` at runtime; renamed to `.svelte.ts` (+ import paths).
  - **Map UX:** full-bleed layout (added `/map` to layout `FULL_BLEED`); undo/redo overlay
    top-left, tool palette center-right, zoom top-right; collapsible right panel with icon tabs
    **Properties | Explorer** (`SidePanel`, `ExplorerPanel`, `PropertiesPanel`); shortcuts overlay
    (`?` / keyboard button).
  - **Wireframe styling:** furniture markers redrawn as top-down plan symbols rendered as emerald
    outline + faint fill (line loops); walls get a bright emerald edge outline.
  - **Alignment:** per-icon `anchor` (door = hinge) + markers snap onto the nearest wall line.
  - **Rotation:** rooms (about centre, slider + `[`/`]`), walls (about midpoint, buttons + `[`/`]`),
    markers; camera **FOV width + length** per-camera; editable camera dx/dy after assign.
  - **Camera FOV = wall-occluded line-of-sight polygon** (Option B): `lib/map/visibility.ts`,
    world-space `CameraLayer` (cameras moved out of the rotated room group).
  - **Zoom fix:** single `$size`-driven effect re-applies the ortho camera on every resize (start +
    panel toggle no longer reset the view); **camera zoom/pan persisted** to localStorage.
  - **Presence end-to-end working:** room + category **dropdowns** on the node; camera id→ip resolve;
    pulse placed **ahead in the FOV**; configurable **`duration_secs`** now **persisted** (migration
    `0021_presence_duration`) so live + replay honor it; removed `timeout_secs` field + the 6s
    fallback; workflow config **saves as you type** (debounced, no Enter). Baseline replays last 30s.
- **State:** `svelte-check` + web build clean (only the 7 pre-existing `sensor-field`/`workflows`
  errors); `cargo check` clean for app-core/control/flow. Deployed throughout via `task cloud-deploy`
  (app many times; control+flow for presence/duration). **In flight at handoff:** control+flow deploy
  of `c885862` (the i32 fix) — building when this was written; verify it finished.
- **Pending / next:** (1) confirm `c885862` deploy landed (`OK: deployed …`, app 200); (2) **verify
  presence live** — run the workflow with console open, expect `[presence] pulse …` (no `dropped`);
  (3) **remove the `[presence]` debug `console` lines** in `lib/map/presence.svelte.ts` once
  confirmed; (4) release: web version bump + merge `feat/map-redesign` → `new-ui`.
- **Blockers:** none code-side. Transient GHCR/Docker-Hub TLS/network hiccups during deploys (retry
  worked). SSH to prod for debugging is **not authorized** (classifier-blocked) — diagnose from code.
- **Refs:** `docs/ref/map.md` (Build status), `docs/ref/edge.md`, `docs/epics/map-redesign.md`.

---

## 2026-06-18 — Map 3D wall depth polish + no-Docker deploy path

- **Branch:** `feat/map-redesign` (off `main`); pushed. Not merged to `new-ui`.
- **Commits:** `022dbb5`, `9526caa`, `c28f792` (this session, on top of `c885862`).
- **Done (all deployed to prod via no-Docker path):**
  - **Wall height:** uniform `0.8` for all solid walls regardless of thickness — CAD/AutoCAD feel; wireframe stays `0.04` (`WallSegment.svelte:24`).
  - **Wall material:** `roughness 0.8→0.65`, `metalness 0.06` added — painted-plaster/concrete surface feel (`WallSegment.svelte:39`).
  - **Lighting:** ambient `0.85→0.5`, key light `1.15→1.7`, shadow map `2048→4096`, frustum `±120→±80`, sun inclined `[40,70,24]→[90,38,20]` — deep one-sided shadows from the right (`MapScene.svelte`).
  - **Vertex snap:** `0.4→0.65` — wider magnetic zone so wall endpoints connect cleanly (`tools.svelte.ts`).
  - **Board:** "Map: solid walls taller, richer material, deeper shadows" card created (ToDo → In Progress); plan at `docs/plans/map-walls-3d-depth.md`.
  - **No-Docker deploy path confirmed:** `npm run build` → `scp` tarball to VPS → `docker cp` into running `sentinel-app` container → `nginx -s reload`. Used twice this session (Docker Desktop was down). HTTP 200 confirmed via public URL.
- **State:** web build clean (same 7 pre-existing warnings); not cargo-checked this session (no backend changes). Deployed to prod; `app.thesentinels.online` returning 200.
- **Pending / next:**
  1. Run `task cloud-deploy -- sentinel-app` once Docker Desktop is back (proper image rebuild so the no-Docker inject is superseded by a clean image).
  2. Confirm `c885862` (i32 presence fix) deploy is still live.
  3. Verify presence live in browser console (`[presence] pulse …`).
  4. Remove `[presence]` debug console lines in `lib/map/presence.svelte.ts` once confirmed.
  5. Release: bump web version + merge `feat/map-redesign` → `new-ui`.
- **Blockers:** Docker Desktop needs a restart before `task cloud-deploy` can run.
- **Refs:** `docs/ref/map.md` (Build status §7), `docs/plans/map-walls-3d-depth.md`.

---

## 2026-06-17 — Map redesign (top-down 3D blueprint editor + presence)

- **Branch:** `feat/map-redesign` (off `main`); pushed. **Not** merged to `new-ui`.
- **Commits:** `321dc70` → `b39bcbf` (Phases 1–5 + diagonal-walls + door-swing + docs handoff).
- **Done:** Rebuilt the Map page as a top-down floor-plan editor rendered with the 3D engine,
  navigated in flat 2D. Rooms are labeled zones (no forced walls); wall drawing (one primitive,
  thin=line/thick=solid, Shift snaps 45°/90°); furniture/landmark **markers** as emerald SVG
  glyphs (incl. door-swing + arc); camera FOV wedges; undo/redo; full keyboard; fit/zoom; dot-field
  floor + soft-shadow lighting. New **`RecordPresence`** workflow node persists categorized
  presence (person/known/car/cat/dog/unknown) + pulses it live on the map via SSE. Vertical file
  decomposition (`routes/map/components`, `lib/map`, `lib/three/map`). Migrations `0019_map_geometry`
  + `0020_presence`; `/map` now exchanges a `{rooms,walls,markers}` document; `GET /map/presence`.
- **State:** `npm run check` + `npm run build` clean (only 7 pre-existing `sensor-field`/`workflows`
  errors, untouched); `cargo check` clean for app-core/control/flow. **Not visually QA'd in a
  running app.**
- **Pending / next:** production deploy via `task cloud-deploy` (applies `0019`+`0020` on
  control/flow boot); then visual QA; optional version bump for release + merge to `new-ui`.
- **Blockers:** deploy blocked on Docker Desktop engine not ready (waiting on a macOS UI password);
  re-run `task cloud-deploy` once `docker info` responds.
- **Refs:** `docs/ref/map.md` (full design + Build status), `docs/ref/deploy.md`.
