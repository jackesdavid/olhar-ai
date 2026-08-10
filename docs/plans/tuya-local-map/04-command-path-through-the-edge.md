# How a workflow command reaches the edge

Type: grilling
Status: open — unblocked, ready to run

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

## Unblocked by 01 — what the client's semantics now constrain

[01](./01-protocol-client-in-rust.md) resolved to `rustuya`, holding **one persistent, heartbeated
connection per device**. That narrows three of the questions above before this ticket even starts:

- The edge is not a stateless command forwarder. It holds a long-lived per-device task, so the
  endpoint hands work to something already connected rather than dialling. Connection setup cost
  disappears from the command path — which is what makes the millisecond claim honest.
- The device tolerates **only one TCP connection**, so "which edge" is not a routing preference, it
  is an exclusivity constraint: two appliances that can both see a plug must not both hold it. That
  is the same problem `devices.box_id` solved for cameras, and it now needs an answer here rather
  than being deferred.
- The connection can be up or down independently of any command, so the edge already knows whether
  a device is reachable. The timeout question changes from "did it answer in 15s" to "was it
  connected at all" — which distinguishes "the gate did not open" from "we do not know", the
  distinction this ticket cares about.
