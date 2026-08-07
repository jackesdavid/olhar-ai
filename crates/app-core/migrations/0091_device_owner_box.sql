-- Card #175 — a camera belongs to ONE box, and no other box may touch it.
--
-- `capture.rs` states the invariant plainly: one connection per camera, ever, because a consumer
-- camera caps concurrent RTSP sessions at 2-4 and the third client is what makes it go dark. That
-- invariant was only ever enforced INSIDE one edge. Nothing coordinated two.
--
-- It held by accident while every tenant had a single box. Running the phone edge alongside the Mac
-- broke it immediately: `/config/cameras` is tenant-scoped, so the phone pulled the Mac's wall
-- cameras and started dialling them. The observable damage was a recording that finalised at 587
-- bytes because its RTSP session had been taken, but the real risk is the camera itself going dark
-- for the customer, which reads as their hardware failing.
--
-- NULL means unclaimed, and unclaimed is served to every box — today's behaviour exactly. Deploying
-- this changes nothing until a box claims a camera, so there is no window where a working box
-- suddenly has no cameras. Claiming then happens on its own: a box reporting a camera as STREAMING
-- is the honest evidence of who the consumer is, so the first such report takes ownership.
--
-- ON DELETE SET NULL is the release path. Delete a box and its cameras become unclaimed rather than
-- orphaned to an id that no longer exists — otherwise replacing an appliance would silently strand
-- every camera it owned, with no error to explain why the new one sees nothing.
ALTER TABLE devices
  ADD COLUMN IF NOT EXISTS box_id uuid
  REFERENCES tenant_boxes (id) ON DELETE SET NULL;

-- `/config/cameras` filters on (tenant_id, box_id) on every pull, from every box, every few seconds.
CREATE INDEX IF NOT EXISTS devices_tenant_box_idx ON devices (tenant_id, box_id);
