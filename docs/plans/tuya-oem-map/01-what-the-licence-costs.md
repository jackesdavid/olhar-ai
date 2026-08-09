# What the OEM licence actually costs

Type: research
Status: resolved

## Question

Everything downstream is priced against this, so it is worth knowing before any of it is designed.

Establish, from Tuya's own material where possible:

- What commercial tier the **Smart Life App SDK** requires. Is there a free/trial tier that can ship
  to a real App Store, or is a paid plan mandatory from the first build?
- The shape of the fee: one-off, annual, per-app, per-device, per-active-user? Per-device or
  per-user pricing changes the product's unit economics, not just its budget.
- Whether the SDK app and our existing Cloud API usage are billed separately, and whether the
  current free-trial API subscriptions (IoT Core, Authorization, Smart Home Basic) still apply.
- Any minimum commitment, and what happens at renewal — the BYO path already has a trap where trial
  subscriptions expire and the integration dies silently; the same question applies here at a much
  larger blast radius, because it would be OUR account and every tenant at once.

Report the numbers and their source. If pricing is not public — likely — say so plainly and record
what is needed to get a quote, because that becomes a `task` ticket rather than a guess.

## Answer

**USD 5,000 the first year, USD 2,000/year to renew.** From Tuya's own pricing page for the
SmartLife App SDK ("Self-Developed App"):

| | Development | Official |
|---|---|---|
| Price | free | **USD 5,000/yr, renewing at USD 2,000/yr** |
| Registered end users | **100 per data centre** | unlimited |
| Cloud requests | 1 M/month | 100 M/month |
| Platforms | iOS, Android, HarmonyOS | same |
| Custom domain | no | yes |

**Shipping to the App Store REQUIRES the Official edition.** Verbatim: *"if your app developed based
on the SmartLife App SDK is planned to be launched on app stores or intended for commercial
purposes, you must sign a service agreement with Tuya … and subscribe to the official edition."*
The free tier is explicitly non-commercial — individuals, communities, preliminary debugging.

### What this decides

The licence is an annual commitment before a single customer is served, and it is **recurring**, so
it is not a build cost that amortises — it is a floor under the integration for as long as it
exists. At 2 000/yr steady-state that is roughly 170/month of fixed cost carried by however many
tenants use Tuya.

Two consequences worth carrying into the other tickets:

- **The 100-user development cap is enough to build and prove the whole thing** — the bridge, the
  invisible account, pairing, server-side control — without paying. The money is only needed to
  ship. So cost does not block the engineering questions; it blocks the launch, which is a different
  decision made later and with better information.
- **The blast radius argument from [[tuya]] returns, inverted.** BYO was chosen partly because one
  shared operator account being suspended would take down every tenant. Here the shared thing is a
  paid subscription: if it lapses — a card expiring, an unrenewed agreement — every tenant's smart
  home stops at once. That is now a billing failure with a physical consequence, and it deserves
  monitoring rather than trust.

Not answered here, and worth knowing before signing: whether the 100 M requests/month ceiling is
generous or tight at our device counts, and whether Cloud API subscriptions (IoT Core et al.) are
billed on top. Neither blocks the build.

Sources: [Pricing — Smart App SDK](https://developer.tuya.com/en/docs/app-development/app-sdk-price?id=Kbu0tcr2cbx3o) ·
[SmartLife App SDK purchase notice](https://support.tuya.com/en/help/_detail/Kby68pusjwgsl)
