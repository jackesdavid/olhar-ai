# Moving the tenants who already use BYO

Type: grilling
Status: open
Blocked by: 01

## Question

OEM replaces BYO — decided. There is at least one tenant connected today with real devices and
working automations, and replacing means **they re-pair every device**. That is a cost imposed on
someone who already has a working setup, so the plan for it is a product decision, not a rollout
detail.

- Does BYO keep working during the transition, or is there a cutover date? Both paths alive at once
  is the thing this effort chose to avoid — but so is turning off someone's gate opener.
- What happens to a workflow that references a device by name when that device is re-paired? A new
  `device_id` behind the same name either heals automatically or silently stops working, and which
  one it is depends on how the node resolves selectors.
- Who does the re-pairing — the customer, or someone walking them through it?
- Is there any migration at all, or is BYO simply frozen for existing tenants and OEM used for new
  ones? That is a legitimate answer and it costs the least.

Blocked by [What the OEM licence actually costs](./01-what-the-licence-costs.md): if the licence is
prohibitive there is no migration to plan, and planning one first would be work thrown away.
