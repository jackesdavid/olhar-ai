# One armed automation out of twelve

**Board card:** #196 Almost nothing is armed — automatic triggering is unproven in production
**Goal:** Understand why graphs are built and then left disabled, and make arming a deliberate,
visible step rather than a thing that quietly never happens.

## What the production database says (measured 2026-08-18)

| Table | Rows | Most recent |
|---|---|---|
| `recordings` | 6,609 | minutes ago |
| `events` | 52,022 | minutes ago |
| `faces` | **3** | 2026-08-07 — eleven days |
| `devices` (cameras) | 6 | — |
| `tenant_boxes` (edges) | 5 | — |

And of **12 workflows, exactly one is `enabled`**: `Recording By Movement`. Face recognition,
the occupancy report, the tracking demos, the gate graphs — all `enabled: false`.

**The one armed automation is the one journey with fresh data.** That is not a coincidence, and it
is the single most important fact about this product's readiness for paying customers: everything
that runs continuously in production today comes from one graph.

## Why this is a product question, not a bug

Nothing is broken. A disabled graph can still be run manually — the Portão P10 gate ran that way
all through #191, and the widget-driven Device Gate journey is manual by design. The question is
why a person builds a graph and then does not arm it. Candidate explanations, none yet confirmed:

1. **Arming is invisible.** If the editor does not make "this is now live" a distinct, obvious act
   with obvious consequences, people stop at "it ran once when I pressed Run".
2. **Arming is scary.** A graph that fires on its own touches gates, lights and phones. Without a
   dry-run or a clear blast radius, not arming is the safe choice — and the safe choice is sticky.
3. **The graphs are demos.** Several names (`Tracking Demo`, `Test Loop`, `Sem título`) suggest
   scratch work that was never meant to run. If so, the real number of *intended* automations is
   much smaller than twelve, and the finding is about clutter rather than adoption.

Explanation 3 is the cheapest to check and should be checked first — ask the owner which of the
twelve were ever meant to be live. **Do not build anything before that answer**: the fix for
"arming is hard" and the fix for "these are scratch graphs" have nothing in common.

## What to look at, in order

1. **Classify the twelve.** Intended-live, scratch, or abandoned. This is a conversation, not code.
2. **Find out what arming actually does.** Confirm what `enabled` changes in `sentinel-flow`:
   which triggers fire, what happens on restart (project memory records that Manual-trigger graphs
   need a re-run after a flow restart — if an armed graph also needs one, arming does not survive
   a deploy and that alone explains the number).
3. **Then**, and only then, decide whether the editor needs an arming affordance, a dry-run, or a
   "this graph has never run automatically" signal on the list.

## Verification criteria

1. Every one of the twelve is classified, and the scratch ones are deleted or renamed so the count
   means something.
2. For each intended-live graph, either it is armed, or the reason it is not is written down.
3. Restart `sentinel-flow` and confirm an armed graph is still armed and still fires afterwards.

## Out of scope

- Building an arming UI before step 1 answers whether one is needed.
- The Device Gate journey, which is manual on purpose.
