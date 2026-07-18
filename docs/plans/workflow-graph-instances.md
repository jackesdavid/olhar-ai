# Graph instances — a run that's still alive, with its own context

**Board card:** #95 Graph instances — per-run context + live instance view (ToDo)
**Unblocks:** [[unified-node-value-store]] (#94), and closes [[delay-trigger-recording-signal-flow]]
(#86) properly.
**Goal:** A run that arms a latent node (a `DelayTrigger` window) **doesn't end** — it stays alive as
an *instance*, holding its own value context, until the latent work fires. Concurrent instances of
one graph run side by side without colliding, and you can see the live ones.

Owner's framing, 2026-07-17: *"we can have concurrent runs of the workflow, and each concurrent call
can create a new trigger call to start, and each stop should run in the correct context"* + *"can we
create the concept of the instances running"*. This is Unreal's model — a Blueprint **instance** with
latent actions — and it's the right one.

---

## The bug this starts from (found by reasoning, then confirmed)

A `DelayTrigger`'s window lives in `workflow_node_state`, whose primary key is:

```
workflow_node_state_pkey  PRIMARY KEY (tenant_id, graph_id, node_id)
```

**A window is scoped to a node.** So there is exactly **one window per Delay Trigger, ever** — no
matter how many contexts are live. Put two cameras through one recording graph:

- Camera A's motion arms the window → A's clip starts.
- Camera B's motion **cannot open its own window**: `try_arm_node_state` fails ("already armed"), so
  B silently **extends A's window** instead.
- One `stop` eventually fires, in one context. **B's clip is never stopped.**

Concurrent contexts don't merely lose their values — they **collide**. And note what this does to
#94's schema question: keying values by `(node, run)` can't help, because the *window* has no context
either. There can only ever be one.

Today's single-camera graph works by accident: with one context, "the node's window" and "this
context's window" are the same thing.

## It's three layers, not one (found 2026-07-17, before building)

The node-keyed window is only the middle of three places that assume one context per graph. An
instance scope has to key **all** of them or the fix does nothing:

| Layer | Keyed by | With two cameras |
|---|---|---|
| Concurrency gate (`sentinel-flow/src/engine.rs`, `fire_event`) | `(tenant, graph, event)` | B's event is **coalesce-dropped** while A's run is in flight |
| Window (`workflow_node_state` PK) | `(tenant, graph, node)` | B can't arm, so B **extends A's** window |
| Values (#94) | `(tenant, graph, node)` | B overwrites A's |

The gate is the one that would have made a window fix useless: with `SkipIfRunning` — **the default,
and what "Recording By Person Detection" is set to** — a listening graph allows *at most one*
simultaneous event handler, keyed by event name. Camera B's `PersonDetection` never even starts.

`Concurrent { max_concurrent }` raises the limit but doesn't fix it: the key still has no context, so
N concurrent runs of one camera are as allowed as N of different cameras — which is backwards. What
we want is one run **per instance**, concurrent **across** instances.

So: **the instance key scopes the gate, the window, and the values.** That's the unification — an
instance is the unit of concurrency, state, and value context, not three unrelated keys that happen
to agree today because there's only ever one of everything.

## Why instances, and not another key

The tempting fix is to add a discriminator to the window key (camera, say). That's a fork: the next
latent node needs its own discriminator, and `DelayTrigger` would have to know what a camera is —
which the owner already rejected once today, correctly (`c3ee151`).

The honest model is that **the run never ended**. It armed a window and is waiting. Everything that
hurts right now follows from pretending otherwise:

| Symptom | Because we pretend the run ended |
|---|---|
| The stop can't see `camera` / `clip_id` | the run's values were thrown away |
| Two cameras share one window | the window had to be parked somewhere node-scoped |
| The trailing edge needs a snapshot/carry | there's no context to read from |
| You can't see what's in flight | nothing models "in flight" |

Give the run a life, and each of those stops being a feature and becomes a consequence:

- **Per-context values** — an instance *has* a value context; the trailing edge reads its own.
- **Concurrency** — two motions → two instances → two windows → two clips. Multiple clips per camera
  works because the right `clip_id` lives in the right instance.
- **The live view** — instances without an end time. `workflow_runs` already carries
  `status='running'` and `mark_orphaned_runs` reaps crashed pods' rows; that's the seed of this.
- **The trailing edge stops being special** — it resumes its instance rather than forcing a sub-run.

## Open decisions (settle before building)

1. **What identifies an instance?** The `run_id` of the run that started it is the obvious answer, and
   `workflow_runs` already exists (id, tenant, graph, status, pod_name). Leaning: an instance IS a
   run row that hasn't finished — no new identity, just an honest `status`.
2. **What makes a debounce retrigger?** ~~THE question.~~ **SETTLED (owner, 2026-07-17): the GRAPH
   declares its instance scope, exactly like Unreal.** The Blueprint is the graph; the actor is the
   instance. A latent node never asks "which camera" — it asks for *its instance's* window, so
   `DelayTrigger` stays exec-only and learns nothing about cameras (the thing already rejected once
   today in `c3ee151`).
   - `single` (default) — one instance per graph. **Byte-for-byte today's behaviour**, so every
     existing graph is unaffected by the migration.
   - `per-camera` — the instance key is the event's camera. `fire_event` already derives
     `event_camera` from `payload["camera"]` for `on_event_node` gating, so the value is in hand.
   Retrigger then falls out for free: same camera → same instance → extends. Different camera →
   different instance → its own window and its own clip.
3. **Does a waiting instance hold anything in memory?** It must not: #86's plan is explicit that a
   held thread would block the `start` branch, and the wait must survive a pod restart. An instance is
   rows + a `work_jobs` timer, exactly as the window is now.
4. **When does an instance end?** All latent work fired, no pending timers. And a cap/TTL, or a
   forgotten instance leaks forever.
5. **Retention** — instance value contexts are per-run rows. `0054`'s nightly `events` sweep is the
   precedent.

## Phases

1. **Scope + all three keys — DONE (phase 1).** `InstanceScope` (`Single` default / `PerCamera`) on
   the graph; `key_for(payload)` derives the key from the firing signal, accepting the camera as
   either `Text` (raw wire) or `CameraEntity` (post-seeding) so an arm on either side lands on one
   instance. Migration `0057` puts `instance_key` in `workflow_node_state`'s PK; the engine derives
   it once per run and threads it through **every** latent read/write (not just the window —
   scoping one node kind would be the fork this model removes); the stop job carries it, because the
   trailing-edge run has no payload to re-derive it from; and the concurrency gate keys on it, which
   is what actually lets two cameras run at once. `Single` → `""` → byte-for-byte today's behaviour.
2. **Model** — an instance = an unfinished `workflow_runs` row + its value context. No new identity.
3. **Values per instance** — #94's store, keyed by instance. This is where #86 finally closes.
4. **The live view** — list running instances per graph; open one and see its context (the owner's
   "see the values of each run context"). Also closes a known gap: the editor's frame read keys on
   `""`, so a `PerCamera` graph's Show Image needs an instance to view.

## Status (2026-07-18) — implementing #94+#95 together

- **Phase 1 DONE** (instance keying: migration 0057 `instance_key` in the `workflow_node_state` PK,
  `InstanceScope`/`key_for`, the concurrency gate keyed by instance). Confirmed in code.
- Related machinery landed since: the #114 fix (`variables_json`, migration 0060) snapshots/restores
  the run's variables across a DelayTrigger window, keyed by instance — the same mechanism this epic
  generalizes.
- **This session's slice** (with #94): (a) **persist every executed node's outputs at run end**,
  keyed by instance (#94 Phase 2 — today only the window path writes `values_json`); (b) the **live
  instance view** (Phase 4 / pragmatic Phase 2) — list a graph's live instances (the distinct
  instance keys with saved state/active windows) and open one to **see its per-node value context**.
  This is the owner's "card to show the workflow current data saved / see the values of each run
  context." Backend endpoints: `GET /workflows/:id/instances` + `GET /workflows/:id/instance-values?key=`.
- **Deferred (still in this epic):** the full instance-as-run-row remodel, and the fork collapse
  (delete `node_frames`, kill the Show Image base64 at `engine.rs:998`) which is #94 Phase 4.

## Verification

- **Two cameras, one graph**: both record, both stop, neither extends the other's window. This fails
  today and is the whole point.
- Two motions on one camera inside `wait_secs` → **one** clip (the debounce survives — decision 2).
- Overlapping windows stop the right `clip_id`s.
- A pod restart mid-window still fires the stop (nothing lives in memory).
- The instance list shows what's in flight and empties when the windows close.

## Out of scope

- The parameterization epic (input variables, clone, promote, templates) — an instance's *inputs* are
  its natural home, so these will meet later; not now.
- Changing `DelayTrigger`'s ports. `start` and `stop`, nothing else.
