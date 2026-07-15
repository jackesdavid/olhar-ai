# Notification Providers — fix the "Not configured" badge + simplify config & enrollment — Implementation Plan

**Use case:** inline — "the Notification Provider admin view says *Not configured* but I'm using
the integration and it works — the badge is wrong; and let's make configuring providers and
enrolling recipients easier."
**Board card:** Notification providers: fix configured badge + simpler config & enrollment (ToDo, #73)
**Goal:** The provider status the UI shows must match reality, and both **operator configuration**
and **recipient enrollment** should be near-frictionless.

## STATUS (2026-07-15): all three phases shipped + deployed

- **Phase 1 (badge truth):** the master Providers badge no longer lies. Three states —
  "Global bot active · @name" / "No global bot · N tenants use their own bot" / "Not configured".
  Root cause was purely the badge reading only the global token; the read/persist path was fine.
- **Phase 2 (operator config):** `PUT /admin/telegram-provider` validates the token with Telegram
  (getMe) before persisting — invalid → 400 and NOT stored (verified against the real API); valid →
  saved + @username returned so the UI shows "verified as @name". Plus a BotFather guidance collapsible.
- **Phase 3 (recipient enrollment):** auto-generated pairing code (pre-filled, rotatable), inline QR
  of the enrollment link (bundled `qrcode` lib → SVG, no external service), one-tap Share, live "a
  new recipient just paired" toast, and a plain 3-step walkthrough.

Deployed. Open decision 4 (multi-provider structure) untouched — Telegram is still the only provider.

---

## Root cause — two disconnected token layers (grounding)

Telegram has **two** token stores, and the admin badge only reads one of them:

1. **Global/shared provider** — table `telegram_global_config` (single operator-managed row).
   Written/read by `crates/app-core/src/notification/repo.rs` `save_global_telegram` /
   `load_global_telegram` (+ `save_global_telegram_username`). Set via the **master-only** admin
   view `GET|PUT /admin/telegram-provider` (`sentinel-control/src/domain/admin_providers.rs`).
2. **Per-tenant** — table `telegram_config` (per `tenant_id`: `bot_token`, `pairing_code`,
   `bot_username`). Read by `repo::load_telegram(db, tid)`.

**Delivery** resolves `own token OR global` — `sentinel-control/src/domain/alerts.rs:153`
(`load_telegram`) and the resolver `repo.rs:78-108` returns
`(pairing_code, effective_bot_username, provider_configured)` where
`provider_configured = own_token set || global_token set`.

**The bug:** the admin **Notification Providers** page
(`sentinel-app/src/routes/admin/providers/+page.svelte:80`) shows its badge from
`admin_providers.rs get_provider`, which checks **only** `load_global_telegram`. So:
- If the working integration is a **per-tenant** bot token (set in Recipients), the global row is
  empty → the admin badge reads **"Not configured"** while delivery works. (Most likely case.)
- OR the global token *is* set but `load_global_telegram` returns `None` / `configured` is computed
  on the wrong field — a genuine persistence/read bug. **Diagnose against the live DB first**
  (`docs/ref/deploy.md` VPS access): `SELECT bot_token IS NOT NULL, bot_username FROM telegram_global_config;`

The Recipients page (`beacons/+page.svelte`) already uses the correct resolver
(`config.provider_configured`, "Provider active"), so the mismatch is specifically the **admin
global view's** notion of "configured."

## Phase 1 — Make the badge tell the truth

- **Diagnose:** confirm on the VPS DB whether `telegram_global_config` holds the token; check
  `load_global_telegram` returns it. Fix any real read/persist bug (e.g. row shape, trim, username
  overwrite blanking the token).
- **Correct the semantics.** The admin view is the *global* provider, so its badge should say
  whether the **global bot** is set — but it must not read as "nothing works." Options
  (Open decision 1):
  - Badge = global-set state, **plus** a truthful sub-line: e.g. *"Not set globally — N tenant(s)
    use their own bot"* (add a count from `telegram_config` where `bot_token <> ''`), or *"Global
    bot active · @name"*. So a master seeing per-tenant usage isn't told "Not configured".
  - Surface the **effective delivery health**: a small "Telegram delivery is working for N tenants"
    line, or a "Send test message" button that proves the effective path.
- Extend `TelegramProviderState` (`admin_providers.rs` + `lib/types.ts`) with the tenant-usage count
  / effective state; render it in `admin/providers/+page.svelte`.

## Phase 2 — Easier operator configuration

- **Validate on save.** `set_provider` already calls `client::get_me` — surface it: on save, show
  the resolved **@bot_username** + a green "verified" check, or a clear error if the token is
  invalid (currently a bad token silently saves with no username). Reject/warn on `get_me` failure.
- **Inline guidance:** a short "How to get a token" (BotFather steps) collapsible; link out to
  Telegram. Reduce the "paste a raw token" cliff.
- **Test message:** a "Send test" affordance (operator + per-tenant) using
  `notification::client` to confirm the whole path end-to-end.
- **Clear states:** loading / configured(+username) / invalid-token / not-set — each with the
  monochrome ink badge styling (retire the amber-400 literal for the ink theme where appropriate).

## Phase 3 — Easier recipient enrollment (the tenant/user side)

Today (`beacons/+page.svelte`): the tenant invents a "pairing code", then shares a `t.me` deep link
(`enrollmentLink(bot_username, pairing_code)`); a recipient opens it and the pairing poller matches.
Make it friction-free:

- **Auto-generate a strong pairing code** (don't ask the user to invent a secret) — a rotate button;
  keep it editable for power users. Reuses `repo::save_pairing_code`.
- **QR code** for the enrollment link so a recipient can scan it on their phone and land in the bot
  chat directly (self-contained SVG QR; no external service).
- **One-tap share**: prominent "Copy invite" + native share + the deep-link button (some exist).
- **Live enrollment feedback**: when a recipient completes pairing, reflect it in the Recipients
  list without a manual refresh (poll or SSE), with a success toast — so the user *sees* it worked.
- **Plain-language walkthrough** (3 steps: share link → they tap Start → they appear here), replacing
  the terse copy. Honour the "calm assistant / plain terms" brand.

## Open decisions (grill before building)

1. **Badge semantics for the global admin view:** global-only status + a tenant-usage sub-line
   (recommended — minimal, truthful), vs. redefining the view to show effective/aggregate delivery
   health. Decide what "configured" should mean to a master.
2. **Keep both token layers, or simplify?** The global-fallback + per-tenant-override model is
   flexible but confusing (it's the source of this bug). Do we (a) keep both but label them clearly,
   or (b) collapse to one (global-only, or tenant-only)? Recommend keep both, label clearly.
3. **Auto pairing code** default on vs. opt-in; and **per-recipient** codes vs. one shared tenant
   code (per-recipient enables revoke/attribution but is a bigger change — likely v2).
4. **Provider extensibility:** the page is titled "Notification Providers" (plural) but only
   Telegram exists. Do we structure Phase 1-2 to admit future providers (email/Resend from #70,
   SMS)? Recommend a small provider-agnostic shape now, Telegram the only entry.

## Out of scope (v1)

- Two-way Telegram bot commands (that's [[telegram-bot-two-way-research]] / #15).
- New providers (email/SMS) — only make the structure ready.
- Per-recipient pairing codes + revocation (v2).

## Testing / verification

- Reproduce the badge bug (per-tenant token set, global empty → admin shows correct new state, not a
  bare "Not configured"). Verify against the live DB state first.
- Save an invalid token → clear error; valid token → @username shown; test message delivers.
- Full enrollment: auto-code → QR/link → recipient pairs → appears live in Recipients with a toast.
- `task cargo` + `npm run build`/`check` clean; deploy control + app.

## Related

[[telegram-beacons-fix-polish]] (enrollment fixes + observability), [[notification-queue-service]]
(the send queue this rides on), [[telegram-bot-two-way-research]] (#15, inbound), [[invite-onboarding-email]]/#70 (email provider).
