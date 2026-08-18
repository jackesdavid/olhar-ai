# A graph you run from your phone reads as "disabled"

**Board card:** #196 On-demand graphs are indistinguishable from switched-off ones
**Goal:** The workflows list tells apart three different things it currently shows identically:
armed and firing on its own, **run on demand by a person**, and genuinely off.

## The correction that produced this card

This card was first written as *"almost nothing is armed — automatic triggering is unproven"*,
after measuring that 11 of 12 production workflows carry `enabled: false`. **That framing was
wrong**, and the owner corrected it: a graph does not need to be armed to be useful, and the
on-demand path is the product's best feature, not a gap.

The real journey it misread: a **home-screen widget on the phone** deep-links `olharai://run/<id>`,
which runs the graph; the graph's Device Gate pushes back an approval prompt; Face ID confirms and
the gate opens. From the person's side that is one tap on their home screen. `enabled` has nothing
to do with it — that flag governs automatic triggering, and this graph is triggered by a human who
already decided.

So the number is not evidence of disuse. `Recording By Movement` is armed because motion is not a
person; `Portão P10` is not armed because opening a gate should be. Both are correct.

## What survives — and it is a real defect

**I read `enabled: false` across eleven graphs and concluded the product was unused.** I was wrong,
but I was reading the same signal a customer reads. The list gives one boolean to cover three
distinct states:

| What it is | How it should read | How it reads today |
|---|---|---|
| Armed: fires on its own from an event or schedule | "Live" | enabled |
| **On demand: run from the app, a widget, or the Run button** | **"On demand"** | **disabled** |
| Off: built but deliberately not in use | "Off" | disabled |

Showing an on-demand graph as *disabled* is telling the owner their best feature is switched off.
It also makes the list useless for the question people actually ask it — *what is running right
now, and what can I trigger?*

## Phase 1 — derive the state instead of showing the flag

The three states are already derivable from the graph, not from a new field: a graph whose only
trigger is Manual is on demand by construction; one carrying an event or schedule trigger is armed
when `enabled`, and off when not. Show that, not the boolean.

Worth confirming against the real data first: of the twelve, which carry `on_event` /
`delay_trigger` nodes and which carry none. A graph with no trigger node at all is on demand
whatever the flag says.

## Phase 2 — surface the on-demand ones as first-class

If a graph is run from a phone, the list should say so — and ideally which ones are pinned to the
home-screen widget, since that is the shortest path a person has to the product. Today the widget
star lives only in the app; the web list does not know about it.

## Phase 3 — the classification that is still worth doing

Several names (`Tracking Demo`, `Test Loop`, `Sem título`) look like scratch work. Deleting or
renaming those is worth doing so the count means something — but it is tidying, not a finding.

## Verification criteria

1. A Manual-trigger graph reads as **on demand**, never as disabled.
2. An event-triggered graph with `enabled: false` reads as **off**, and is visibly different from
   the above.
3. `Portão P10` and `Recording By Movement` sit in different states in the list, and both read as
   working.

## Out of scope
- Any change to how graphs are triggered. The manual path is correct as designed.
- An arming UI. The original card proposed one to solve a problem that was not there.
