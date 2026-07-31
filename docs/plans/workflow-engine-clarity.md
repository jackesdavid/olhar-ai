# Workflow engine — say what it does, fail where you can see it

**Board card:** #96 Engine clarity — kill the ambiguity that hides bugs (ToDo)
**Goal:** Make the engine's behaviour legible: no silent skips, no comments that contradict the code,
no rule that only exists as a special case. Measured by whether the next bug of today's class takes
ten minutes instead of an afternoon.

Not a tidy-up. Every item below is something that **actively cost time on 2026-07-17**, or a comment
that **states the opposite of what the code does**. Cosmetic refactoring is explicitly out of scope —
this repo doesn't need prettier code, it needs code that can't lie.

---

## Why (the day this came from)

One report — "the Delay Trigger never calls Stop Recording" — took hours and turned up **four** real
faults. None announced itself; three actively hid:

1. A missing migration made every trailing-edge run fail at insert. Because `insert_run` only logs,
   the run still executed — and with no run row, **every node log then failed its FK back to it**. The
   feature's diagnostics were deleted by the bug that broke them. Run history showed *nothing*.
2. `StopRecording.camera` was wired from `OnEvent`, which never runs on a forced sub-run → the branch
   was marked **dead and skipped, with the run reporting `ok`**.
3. Orphan sources never activated on sub-runs. `GetVariable` had been hand-exempted; `GetCameraByName`
   hadn't — so it worked on cron and silently died on events.
4. A window is keyed `(tenant, graph, node)`, so two cameras **collide** on one window (#95).

Every one is invisible-by-default. That's the target.

## The work

### 1. A skipped node must leave a trace
Today a dead path settles and `continue`s — nothing logged. This is the single biggest time sink in
the list, and the fix is a log line the run viewer can show:
> `Stop Recording — skipped: required input 'camera' comes from 'When Event', which doesn't run on
> this trigger`

That sentence is the whole afternoon, in one line, at the moment it happens.

### 2. Comments that contradict the code (fix or delete — these are worse than none)
- `tailscale.rs` `reassert()` claimed a box "heals ITSELF within minutes… what makes the live path
  self-recover for a real customer". It never re-ran `up`; a stopped daemon stranded the box forever.
  Fixed in #92, but **the comment is why nobody looked there** — audit the file for the same.
- `#86`'s plan concluded **"No migration"** in bold. Right about `work_jobs`, wrong about the CHECK
  constraint that made the whole feature inert.
- Sweep for the pattern: a comment asserting a *behaviour* (heals, retries, validates) rather than a
  *constraint*. Each is a claim; check it or cut it.

### 3. Rules that exist only as special cases
`is_pure_source()` (#93/`753ef21`) is the template: `GetVariable` had a bespoke `if` in the activation
chain; `GetCameraByName` didn't and was broken. Find the others — a `matches!`/`if kind ==` in engine
control flow is a rule wearing a disguise. Promote each to a named, documented `NodeKind` property so
the *next* node kind inherits it instead of rediscovering the bug.

### 4. Ambiguity that reads as a bug
- **`clip_id` is optional, `camera` is required** — so one silently arrives empty and the other kills
  the branch. Defensible, but nothing says so. Document required-ness *as intent* on the ports.
- **A wire beats a literal.** Unwired `camera` resolves from config on any run; wiring it from
  `OnEvent` makes it dead on a timer branch. Same wire, correct on `start`, fatal on `stop` — with no
  hint anywhere. At minimum document it; ideally the editor warns (below).
- **`node_frames` vs `workflow_node_state` vs `media.rs`** — three answers to "what did this node
  produce" (#94).

### 5. Editor-side (the real fix for #4)
A required input on a `stop`-side node, wired from a source that can't run on a timer sub-run, is
**statically knowable from the graph**. The editor let this be drawn and gave no hint. Warn at wire
time, and the class of bug stops existing rather than being documented.

### 6. Publish integrity (adjacent, same lesson)
`task edge-publish` scp's the binary and trusts it. A dropped transfer left a **truncated edge binary**
in the dist dir — the file every new macOS install downloads — and only a manual sha check caught it.
Verify after upload; the same uplink dropped an 82MB .pkg twice.

## Explicitly NOT in scope

- Renaming, reformatting, or moving code that already works and reads clearly.
- `cargo fmt` across the repo. It is not fmt-clean; a blanket run buries real diffs in churn (it
  did exactly that twice today, once destroying uncommitted work).
- Anything in `legacy/`.

## Verification

- Re-run the #86 scenario against a graph with `camera` wired from `OnEvent` into `Stop`: the run
  viewer **names the skipped node and why**, in one line, without a database session.
- Every audited comment either matches the code or is gone.
- No `if kind == X` special cases left in the activation chain that aren't a documented property.
