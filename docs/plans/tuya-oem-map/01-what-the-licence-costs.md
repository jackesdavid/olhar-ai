# What the OEM licence actually costs

Type: research
Status: open

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
