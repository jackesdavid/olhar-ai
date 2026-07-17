# Olhar AI landing page — rebrand + a rewrite that sells the platform

**Board card:** #100 Landing page — Olhar AI rebrand + copy rewrite (ToDo)
**Parent epic:** [[olharai-rebrand]] (#76). Pairs with [[olharai-domain-cutover]] (#99, routing).
**Goal:** The public landing page is Olhar AI — the eclipse-eye mark, the "Olhar AI" name, the new
domain — and its copy actually conveys what the platform does, which the owner has twice said it
doesn't.

---

## Two jobs, one page

1. **Rebrand the visuals.** `landing/` is a *separate* SvelteKit app with its **own copy of the old
   dot-field mark** (`landing/src/lib/components/sentinel-mark.svelte`, used in `landing/src/routes/+page.svelte`).
   The app's mark was replaced with the eclipse-eye (#76, `8f29a66`); the landing's was deliberately
   left, since the owner said we'd handle the landing separately. Port the eclipse-eye here too —
   ideally by lifting the app's `olhar-mark.svelte` rather than keeping two marks that will drift.
   Name → "Olhar AI"; wordmark → the OLHAR AI lockup; favicon/OG → the new set.

2. **Rewrite the copy.** The epic records the owner's words: the current copy *"is not reflecting the
   true power of this application — it undersells the platform."* This is the real work, and it's
   content, not code: cameras that see (person/object/face detection, tracking, presence on a floor
   plan), a visual workflow engine that automates on what they see, recording with cloud storage, a
   calm assistant you can talk to. Written for a buyer, not a changelog.

## Grounding (don't invent capabilities)

Pull the feature claims from what actually ships, so the page doesn't promise what the product
doesn't do:
- Vision — person/object/face detection, tracking, presence (the workflow node catalog + Map).
- Automation — the graph engine (`docs/ref/workflows.md`, the node catalog).
- Recording — motion/event-triggered clips, cloud storage + billing.
- Assistant — the chat agent (now "Olhar AI").
- The appliance model — LAN-bound cameras, an on-prem edge, everything else cloud.

## Open decisions

1. **Rebuild or rewrite in place?** The landing is functional; the ask is copy + brand, not
   architecture. Leaning: in-place rebrand + a section-by-section copy pass, not a from-scratch
   rebuild — unless the current structure actively fights the new story.
2. **How much of the platform to show.** It's broad; a landing page shouldn't list all 60+ workflow
   nodes. Pick the three or four capabilities that land the "cameras that understand, and act"
   promise, and let the rest be depth behind a signup.
3. **One mark, or accept two.** Lifting `olhar-mark.svelte` into `landing/` means a shared source or
   a copy. Leaning: copy the component but keep it byte-identical, and note the intent to unify —
   `landing/` is a separate build with no shared lib today.

## Depends on / relates to

- **#99 domain cutover** — the landing serves `thesentinels.online` today (Traefik) and will serve
  `olharai.com`. Land the copy independently; the routing switch is #99's.
- **#76 og.png** — the shared OG image regeneration lives in the epic.

## Verification

- The landing renders the eclipse-eye, says "Olhar AI", and reads as a product a buyer understands in
  one scroll.
- No old dot-field mark or "Sentinel" copy remains in `landing/`.
- Every capability claimed maps to something that actually ships.

## Out of scope

- The domain/routing change (#99).
- Marketing SEO/analytics beyond the existing setup.
