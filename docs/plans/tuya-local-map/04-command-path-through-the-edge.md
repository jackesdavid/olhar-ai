# How a workflow command reaches the edge

Type: grilling
Status: open
Blocked by: 01

## Question

`run_tuya_control` runs in flow and calls the Cloud API directly. It must instead reach the edge,
which owns the LAN connection.

The mechanism exists: `edge_http_via_sse_blocking` already carries cloud→edge calls for frames,
probes and recordings, and the edge is outbound-only (#144), so there is nothing to open. What needs
deciding is the shape.

- What the edge exposes: a generic `POST /tuya/command` taking device id + code + value, or
  something narrower? The edge's local server already has a small, deliberate surface.
- **Timeouts.** A cloud call today fails in seconds. A LAN command should be milliseconds — but a
  device that is asleep or gone will not answer, and the node has a 15s watchdog. What the node
  reports on timeout matters: "the gate did not open" and "we do not know if the gate opened" are
  different, and a graph branching on `ok` deserves the true one.
- **Which edge**, when a tenant has several. Camera ownership was just settled by `devices.box_id`
  (#175); a Tuya device needs the same answer, and probably the same mechanism — but nothing
  currently records which appliance can see which plug.
- Does the command wait for the device's acknowledgement, or return once sent?

Blocked by [What speaks the Tuya LAN protocol from the edge](./01-protocol-client-in-rust.md): the
client's own semantics — connection lifetime, acknowledgement, error surface — determine what this
endpoint can honestly promise.
