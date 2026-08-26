//! `DeviceGate` — stop, ask a person on their phone, and carry on with what they said.
//!
//! A run cannot wait for a human. The answer arrives minutes or days later, in another process,
//! possibly never — so the node **ends the run**: it mints a request, pushes it, and stops. When
//! somebody answers, that arrives as a fresh entry at this node carrying the verdict. The ask and
//! the answer are two runs, which is also how an auditor would want to read it.
//!
//! ## Two failures that are not the same failure
//!
//! `failure` means **a human was asked and did not decide** — the request expired, or the push
//! could not be delivered. An unset Recipient or an empty prompt is an AUTHORING mistake, and it
//! fails the run instead. Letting a broken gate arrive on the same arm is how a graph comes to
//! treat "never asked" as "asked and ignored", and the two want completely different responses.
//!
//! ## The trap
//!
//! Resuming is tested by the PRESENCE of the verdict key, not by `forced` alone. A loop
//! back-edge forces a node too, and firing an arm nobody chose is worse than not resuming.

use crate::workflows::domain::{DEVICE_GATE_ANSWERED_BY, DEVICE_GATE_FACTOR, DEVICE_GATE_VERDICT};
use crate::workflows::host::SentinelHost;
use crate::workflows::{compat, device_gate as gate, node::PortValue};
use ggraph_core::host::Host;
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};
use ggraph_core::{PortName, Value};

pub struct DeviceGate;

impl NodeStep<SentinelHost> for DeviceGate {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        let payload = compat::from_engine_values(cx.entry_payload);

        if cx.forced {
            if let Some(verdict) = payload
                .get(DEVICE_GATE_VERDICT)
                .and_then(PortValue::as_text)
            {
                let arm = gate::GateState::from_str(verdict)
                    .unwrap_or(gate::GateState::Failed)
                    .exec_arm();
                let carried = |k: &str| {
                    Value::text(
                        payload
                            .get(k)
                            .and_then(PortValue::as_text)
                            .unwrap_or_default(),
                    )
                };
                let mut out = ggraph_core::PortValues::new();
                out.insert(
                    PortName::new("answered_by"),
                    carried(DEVICE_GATE_ANSWERED_BY),
                );
                out.insert(PortName::new("factor"), carried(DEVICE_GATE_FACTOR));
                return Ok(Step::outputs(out)
                    .arm(arm)
                    .logged(format!("{verdict} → {arm}")));
            }
        }

        let inputs = compat::from_engine_values(cx.inputs);
        // Wired or authored in the panel; either satisfies the port.
        let text = |key: &str| -> String {
            inputs
                .get(key)
                .and_then(PortValue::as_text)
                .map(str::to_string)
                .or_else(|| {
                    cx.config
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_default()
        };
        let recipient = text("recipient");
        let message = text("message");
        let ttl_secs = cx
            .config
            .get("ttl_secs")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .unwrap_or(600);

        if recipient.is_empty() {
            return Err(NodeError::new("no Recipient set — nobody to ask"));
        }
        if message.trim().is_empty() {
            return Err(NodeError::new(
                "the message is empty — nothing to decide on",
            ));
        }

        let tools = cx.host.tools();
        let failed = |why: String| Ok(Step::default().arm("failure").logged(why));

        let (device_id, token) =
            match gate::resolve_device_by_label(&tools.db, tools.tenant_id, &recipient) {
                Ok(pair) => pair,
                Err(gate::ResolveError::NoDevice) => {
                    return failed(format!("'{recipient}' has no registered phone"))
                }
                Err(gate::ResolveError::Ambiguous(count)) => {
                    return failed(format!(
                        "'{recipient}' matches {count} phones — only the addressed device may \
                         answer, so there is no one device to ask"
                    ))
                }
            };

        // Read per ask rather than cached: it is one indexed row, and a tenant that lowers the
        // cap while a loop is running expects the next ask to obey.
        let cap = gate::max_pending_for_tenant(&tools.db, tools.tenant_id);
        let minted = match gate::mint(
            &tools.db,
            tools.tenant_id,
            cx.graph,
            cx.node,
            tools.run_id,
            device_id,
            &message,
            &tools.graph_name,
            ttl_secs,
            cap,
        ) {
            Ok(m) => m,
            Err(gate::MintError::AtCap(cap)) => {
                return failed(format!(
                    "'{recipient}' already has {cap} unanswered requests — not asking again until \
                     some are answered or expire"
                ))
            }
            Err(gate::MintError::Db) => {
                return failed("could not create the approval request".into())
            }
        };

        // The token rides in `data`, which the app reads and the OS never renders. It must never
        // reach `body` — that would print a gate-opening secret on a lock screen.
        let data = serde_json::json!({
            "kind": "device_gate",
            "request_id": minted.id,
            "token": minted.token,
            "expires_at": minted.expires_at,
        });
        // The title comes from the graph, never from text the prompt's author could make
        // disagree with what is actually being unlocked.
        let title = format!("{}: approval needed", tools.graph_name);

        match crate::notification::push::send_data(&[token], &title, &message, data) {
            Ok(sent) => {
                crate::notification::push::enqueue_receipt_check(
                    &tools.db,
                    tools.tenant_id,
                    "device_gate",
                    &sent,
                );
                // Expo QUEUED it. Nothing has reached a phone yet, which is why this says "asked"
                // rather than "delivered" — on 2026-08-17 every one of these was dying at Apple
                // while a line like it read exactly the same (#190).
                Ok(Step::default()
                    .halted()
                    .logged(format!("asked {recipient}, expires {}", minted.expires_at)))
            }
            Err(e) => {
                // Undeliverable. Settle it now rather than leaving a request nobody will see
                // waiting out its TTL: the graph learns in seconds instead of ten minutes, and
                // the audit trail records `failed` rather than `expired` — different events.
                gate::settle_failed(&tools.db, minted.id);
                failed(format!("push failed — {e}"))
            }
        }
    }
}
