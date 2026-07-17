# Workflow parameterization — input variables, clone, promote, templates

**Board card:** #102 Workflow parameterization — input vars + clone + promote + templates (ToDo)
**Goal:** A graph can declare **inputs** (variables marked as parameters), be **cloned**, have an
internal variable **promoted** to an input, and ship as a reusable **template** — so a default graph
works on someone else's cameras without editing the graph itself.

Requested repeatedly through 2026-07-17 and flagged out-of-scope in several plans without ever being
carded (the gap the owner caught). One epic, four moves that all serve one goal: **a template can't
hardcode your camera — it has to ask.**

---

## Why these four are one thing

- **Input variables** make a graph portable — the camera, the threshold, the recipient become
  *questions the graph asks* instead of literals baked into nodes.
- **Clone** is how you fork a template into your own copy.
- **Promote** is how an internal variable *becomes* the question (the UE model: a variable with an
  "expose as input" flag).
- **Templates** are the payoff — default graphs a new user starts from, parameterized so they run on
  their cameras on day one.

It also retires today's whole "literal vs wire" mess for cameras (the #86 saga): a template's camera
stops being a hardcoded id and becomes a declared input the user fills in once.

## What already exists to build on

- **#56 Workflow Variables** (`[[workflow-variables]]`) shipped: typed vars + Get/Set nodes,
  `VariableDecl` with defaults, seeded per run (`seed_variable_defaults`). Inputs are variables with
  one more bit: *is this a parameter?* — exactly the owner's "use the variables, mark them true/false
  as input parameters, like UE."
- **Instances** (#95): an instance already carries a per-run context. A graph's inputs are that
  context's natural home — the bridge between this epic and #95.
- Graph CRUD + the editor's variables panel already exist; clone is a copy with a new id.

## The crux — settle first: how do inputs ARRIVE, per trigger?

The one decision that shapes everything. A graph's inputs need values, and where they come from
depends on how the graph fired:

- **Manual run** — prompt the user for them at launch (a form from the input decls). The natural,
  interactive case.
- **Cron** — no one to ask. Inputs need **config-time defaults**, set when the schedule is created.
- **Event** — the payload IS the input source: `OnEvent`'s camera/bbox/confidence map onto inputs.
  A `PerCamera` template's "camera" input is filled from the event's camera automatically.

Leaning: inputs are variables with a `param: bool` flag + a default; the *trigger* decides the source
(manual → prompt, cron → default, event → payload-mapped), and the default is the fallback for all.
Decide this before building the UI, or the surface won't fit all three triggers.

## Phases (sketch — the crux above comes first)

1. **Input flag on variables** — `VariableDecl { …, param: bool }` (+ a description for the prompt
   label). Promote = flip the flag in the editor's variables panel. Additive; old graphs have none.
2. **Manual-run prompt** — when a graph with inputs is run by hand, a form from the decls; the values
   seed the run's variables (over the defaults).
3. **Clone** — duplicate a graph to a new id/name (nodes, edges, vars, scope), landing in the list
   editable. Cheap; mostly a CRUD add.
4. **Event/cron sourcing** — map an event payload onto inputs (the `PerCamera` camera is the first
   real case); cron uses the config-time defaults.
5. **Templates** — a curated set of default graphs (from the six real workflows: alert on person,
   record on motion, the Telegram AI bot, tracking) with their cameras/recipients as inputs. New
   tenant → offered these → fills inputs → running. The onboarding payoff.

## Open decisions

1. **The trigger-source model above** — THE decision.
2. **Where templates live** — seeded rows per new tenant, or a shared catalog cloned on pick? Leaning:
   a catalog the user clones from, so their copy is fully theirs to edit.
3. **Input types** — start with camera / number / text / recipient (the types that actually vary in
   the real graphs); not every `PortType` needs to be an input on day one.
4. **Prompt UI** — a modal at run time, or a persistent "inputs" section on the graph? Both,
   eventually; modal-at-run is the MVP.

## Relates to

- **#95 instances** — inputs live in the instance context.
- **#76 rebrand / #100 landing** — the landing already sells "start from a template"; templates make
  that real.
- **#56 workflow variables** — the substrate; inputs are variables + a flag.

## Out of scope (v1)

- Sharing templates *between tenants* / a public template gallery — later.
- Versioning templates or migrating a graph when its template updates — later.
