# How the server controls a device with no phone involved

Type: grilling
Status: open
Blocked by: 03

## Question

Easy to miss and central: **workflows run on the server, at 3am, with nobody's phone awake.** A
schedule that turns a light off does not have a signed-in app to ask.

Today that works because the tenant's own Cloud project credentials live in `tuya_config` and the
backend signs its own requests. With OEM, the app holds a user session — and a user session is not
what a cron node can use six hours later.

So: what does the backend authenticate as?

- Our OEM project's Cloud credentials, with the `allowed_uids` filter scoping each tenant to its own
  devices? The machinery for that already exists and is currently dead code — it was built for
  exactly this shape.
- Something else the SDK provides, if the SDK model differs from what the Cloud API expects?

Follow-on questions this decides:

- If ONE project's credentials reach every tenant's devices, then a bug in the uid filter is a
  cross-tenant device-control incident. What makes that filter trustworthy — and testable?
- Does a device paired in the app appear to the Cloud API immediately, or is there a sync delay a
  workflow could fall into?

Blocked by [Is the invisible Tuya account per user, or per tenant?](./03-account-per-user-or-per-tenant.md):
whose uid the server filters on is exactly what that ticket decides.
