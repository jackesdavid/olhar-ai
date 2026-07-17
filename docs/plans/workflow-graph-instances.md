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
2. **What makes a debounce retrigger, then?** THE question. If every event opens a new instance, a
   re-trigger would start a second recording instead of extending the first — the debounce dies. So a
   `DelayTrigger` must find *the instance it belongs to*. By what? Leaning: the node declares a
   **scope expression** (default: the graph = today's behaviour; typically the camera), so B's motion
   joins B's instance and A's joins A's. This is the crux — get it wrong and either the debounce or
   the concurrency breaks.
3. **Does a waiting instance hold anything in memory?** It must not: #86's plan is explicit that a
   held thread would block the `start` branch, and the wait must survive a pod restart. An instance is
   rows + a `work_jobs` timer, exactly as the window is now.
4. **When does an instance end?** All latent work fired, no pending timers. And a cap/TTL, or a
   forgotten instance leaks forever.
5. **Retention** — instance value contexts are per-run rows. `0054`'s nightly `events` sweep is the
   precedent.

## Phases (sketch — decisions above come first)

1. **Model** — an instance = an unfinished `workflow_runs` row + its value context. No new identity.
2. **Scope** — how a latent node finds its instance (decision 2). Default scope = graph, so existing
   graphs behave exactly as today.
3. **Window per instance** — the window moves off `(graph, node)` onto the instance. This is the
   migration that fixes the two-camera collision.
4. **Values per instance** — #94's store, keyed by instance.
5. **The live view** — list running instances per graph; open one and see its context (the owner's
   "see the values of each run context").

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
