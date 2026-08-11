//! Stripe gateway — serves **both** Brazil and the American market: multi-currency Prices,
//! card, boleto, and recurring **Pix Automático** (Stripe added it 2026-04).
//!
//! Checkout-style: we open a hosted Checkout Session and the customer pays there; Stripe
//! creates the Subscription afterwards and tells us via `checkout.session.completed`. The
//! session carries `client_reference_id` (our tenant) and metadata (plan, billing type) so
//! the webhook can bind it. Webhooks are authenticated with an HMAC-SHA256 signature over
//! the raw body — never a shared token.

use std::collections::HashMap;
use std::time::Duration;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use super::{EventKind, PaymentGateway, PlanRef, StartedSubscription, WebhookEvent};

/// How stale a Stripe webhook may be before we refuse it.
///
/// Stripe's own guidance is five minutes. It has to absorb Stripe's retry backoff and our own
/// queueing, so it is a replay bound rather than a freshness guarantee.
const WEBHOOK_TOLERANCE_SECS: i64 = 300;

type HmacSha256 = Hmac<Sha256>;

pub struct StripeGateway {
    api_key: String,
    base: String,
    webhook_secret: Option<String>,
    success_url: String,
    cancel_url: String,
}

impl StripeGateway {
    /// Config from env: `STRIPE_API_KEY` (required — `sk_…` or a restricted `rk_…`),
    /// `STRIPE_WEBHOOK_SECRET` (`whsec_…`; when set, webhook signatures are verified),
    /// `STRIPE_SUCCESS_URL` / `STRIPE_CANCEL_URL` (where Checkout returns the customer),
    /// `STRIPE_BASE_URL` (override, defaults to the public API).
    pub fn from_env() -> Option<Self> {
        let api_key = env_nonempty("STRIPE_API_KEY")?;
        let base = env_nonempty("STRIPE_BASE_URL")
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| "https://api.stripe.com".to_string());
        Some(Self {
            api_key,
            base,
            webhook_secret: env_nonempty("STRIPE_WEBHOOK_SECRET"),
            success_url: env_nonempty("STRIPE_SUCCESS_URL")
                .unwrap_or_else(|| "https://app.olharai.com/billing?paid=1".into()),
            cancel_url: env_nonempty("STRIPE_CANCEL_URL")
                .unwrap_or_else(|| "https://app.olharai.com/billing".into()),
        })
    }

    /// Stripe's REST API is form-encoded, not JSON.
    fn post_form(&self, path: &str, form: &[(&str, String)]) -> Result<serde_json::Value, String> {
        let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
        ureq::post(&format!("{}{}", self.base, path))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .timeout(Duration::from_secs(20))
            .send_form(&pairs)
            .map_err(|e| format!("stripe {path}: {e}"))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("stripe {path}: bad JSON: {e}"))
    }

    /// Create a Product + recurring monthly Price for a tier, returning the Price id.
    /// Amounts are minor units (cents) — Stripe never takes floats.
    fn create_price(&self, plan: &PlanRef<'_>) -> Result<String, String> {
        let product = self.post_form("/v1/products", &[("name", plan.name.to_string())])?;
        let product_id = product
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("stripe: product create returned no id: {product}"))?;

        let cents = (plan.amount * 100.0).round() as i64;
        let price = self.post_form(
            "/v1/prices",
            &[
                ("product", product_id.to_string()),
                ("unit_amount", cents.to_string()),
                ("currency", plan.currency.to_lowercase()),
                ("recurring[interval]", "month".to_string()),
            ],
        )?;
        price
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("stripe: price create returned no id: {price}"))
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The subscription id carried by an Invoice object.
///
/// Stripe **moved this field** in API `2025-05-28`: it used to be top-level
/// `invoice.subscription`, and now lives at `parent.subscription_details.subscription`.
/// We read the new location first and fall back to the legacy one, so both API versions
/// work. Getting this wrong silently drops `invoice.payment_failed` — a delinquent
/// customer would keep access.
fn invoice_subscription(obj: &serde_json::Value) -> Option<String> {
    obj.get("parent")
        .and_then(|p| p.get("subscription_details"))
        .and_then(|d| d.get("subscription"))
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("subscription").and_then(|v| v.as_str()))
        .map(String::from)
}

/// Our billing-type slugs → Stripe payment method types.
fn payment_method(billing_type: &str) -> &'static str {
    match billing_type {
        "PIX" => "pix",
        "BOLETO" => "boleto",
        _ => "card",
    }
}

impl PaymentGateway for StripeGateway {
    /// **Pix is deliberately absent.** A recurring Pix charge runs on *Pix Automático*, which
    /// needs a customer-authorized mandate — `payment_method_options[pix][mandate_options]`
    /// carrying a per-charge ceiling and a schedule. [`Self::start_subscription`] sends neither,
    /// so a Pix checkout could not renew even on an account where Stripe has enabled it. Add it
    /// here only together with the mandate (and derive the ceiling from `plan.amount`: the
    /// default is far below the Business tier, so a mandate at the default would pass at signup
    /// and fail on the first renewal).
    fn payment_methods(&self) -> &'static [&'static str] {
        &["CREDIT_CARD", "BOLETO"]
    }

    fn provider(&self) -> &'static str {
        "stripe"
    }

    fn ensure_customer(
        &self,
        name: &str,
        email: &str,
        tax_id: Option<&str>,
    ) -> Result<String, String> {
        let mut form = vec![("name", name.to_string()), ("email", email.to_string())];
        // Brazilian CPF/CNPJ is optional metadata for Stripe (required only for boleto).
        if let Some(doc) = tax_id {
            form.push(("metadata[tax_id]", doc.to_string()));
        }
        let out = self.post_form("/v1/customers", &form)?;
        out.get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("stripe: customer create returned no id: {out}"))
    }

    /// A Billing Portal session — Stripe's hosted page for updating the card, seeing invoices and
    /// cancelling. This is the answer to "my payment failed"; without it the only button in the
    /// product was Subscribe, which starts a SECOND subscription on the same customer.
    ///
    /// Needs the portal configured once in the Stripe Dashboard (Settings → Billing → Customer
    /// portal). Until then Stripe answers with an error, which we pass through rather than
    /// swallow — a dead button that looks alive is worse than a message saying what is missing.
    fn manage_url(&self, customer_ref: &str, return_url: &str) -> Result<Option<String>, String> {
        let out = self.post_form(
            "/v1/billing_portal/sessions",
            &[
                ("customer", customer_ref.to_string()),
                ("return_url", return_url.to_string()),
            ],
        )?;
        if let Some(msg) = out
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return Err(format!("stripe portal: {msg}"));
        }
        Ok(out.get("url").and_then(|v| v.as_str()).map(String::from))
    }

    fn cancel_subscription(&self, subscription_ref: &str) -> Result<(), String> {
        let out = ureq::delete(&format!("{}/v1/subscriptions/{}", self.base, subscription_ref))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .timeout(Duration::from_secs(20))
            .call();
        match out {
            Ok(_) => Ok(()),
            // Already gone is the outcome we wanted, not a failure.
            Err(ureq::Error::Status(404, _)) => Ok(()),
            Err(e) => Err(format!("stripe cancel {subscription_ref}: {e}")),
        }
    }

    fn start_subscription(
        &self,
        customer_ref: &str,
        plan: &PlanRef<'_>,
        billing_type: &str,
        tenant_id: Uuid,
    ) -> Result<StartedSubscription, String> {
        // Reuse the tier's Price object, or create one now and hand it back to persist.
        let (price_id, created) = match plan.price_ref {
            Some(p) => (p.to_string(), None),
            None => {
                let p = self.create_price(plan)?;
                (p.clone(), Some(p))
            }
        };

        let session = self.post_form(
            "/v1/checkout/sessions",
            &[
                ("mode", "subscription".to_string()),
                ("customer", customer_ref.to_string()),
                ("line_items[0][price]", price_id),
                ("line_items[0][quantity]", "1".to_string()),
                (
                    "payment_method_types[0]",
                    payment_method(billing_type).to_string(),
                ),
                ("success_url", self.success_url.clone()),
                ("cancel_url", self.cancel_url.clone()),
                // Echoed back on `checkout.session.completed` so the webhook can bind it.
                ("client_reference_id", tenant_id.to_string()),
                ("metadata[plan_id]", plan.plan_id.to_string()),
                ("metadata[billing_type]", billing_type.to_string()),
                (
                    "subscription_data[metadata][plan_id]",
                    plan.plan_id.to_string(),
                ),
            ],
        )?;

        let payment_url = session
            .get("url")
            .and_then(|v| v.as_str())
            .map(String::from);
        if payment_url.is_none() {
            return Err(format!(
                "stripe: checkout session returned no url: {session}"
            ));
        }
        Ok(StartedSubscription {
            // Stripe creates the Subscription only after payment — the webhook brings it.
            subscription_ref: None,
            payment_url,
            price_ref: created,
        })
    }

    fn verify_webhook(&self, raw_body: &[u8], headers: &HashMap<String, String>) -> bool {
        let Some(secret) = &self.webhook_secret else {
            // Fail CLOSED. This endpoint mutates billing state from an unauthenticated request:
            // `parse_event` reads tenant_id and plan_id straight out of the body, so accepting
            // unverified events lets anyone grant themselves any plan. An unconfigured secret is a
            // deployment mistake, and the safe response to it is to reject, not to trust.
            if cfg!(debug_assertions) {
                log::warn!(
                    "stripe: STRIPE_WEBHOOK_SECRET unset — accepting unverified (debug build only)"
                );
                return true;
            }
            log::error!("stripe: STRIPE_WEBHOOK_SECRET unset — rejecting webhook");
            return false;
        };
        let Some(sig_header) = headers.get("stripe-signature") else {
            return false;
        };
        // `t=<unix>,v1=<hex>,v0=<hex>` — sign `"{t}.{body}"` and compare to any v1.
        let mut timestamp = None;
        let mut signatures: Vec<&str> = Vec::new();
        for part in sig_header.split(',') {
            match part.split_once('=') {
                Some(("t", v)) => timestamp = Some(v),
                Some(("v1", v)) => signatures.push(v),
                _ => {}
            }
        }
        let Some(t) = timestamp else { return false };
        if signatures.is_empty() {
            return false;
        }
        // The timestamp is inside the MAC, so it cannot be edited — but folding it in proves only
        // that Stripe once sent this, not that it sent it recently. Without a window, a single
        // captured webhook is replayable forever.
        match t.parse::<i64>() {
            Ok(ts) if (crate::signing::now_unix() - ts).abs() <= WEBHOOK_TOLERANCE_SECS => {}
            Ok(_) => {
                log::warn!("stripe: webhook timestamp outside the {WEBHOOK_TOLERANCE_SECS}s window — rejecting");
                return false;
            }
            Err(_) => return false,
        }

        // `verify_slice` is constant-time. A fresh MAC per candidate (it consumes self).
        signatures.iter().any(|s| {
            let Ok(expected) = hex::decode(s) else {
                return false;
            };
            let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
                return false;
            };
            mac.update(t.as_bytes());
            mac.update(b".");
            mac.update(raw_body);
            mac.verify_slice(&expected).is_ok()
        })
    }

    fn parse_event(&self, raw_body: &[u8]) -> Option<WebhookEvent> {
        let body: serde_json::Value = serde_json::from_slice(raw_body).ok()?;
        let event_id = body.get("id")?.as_str()?.to_string();
        let kind = body.get("type")?.as_str()?;
        let obj = body.get("data")?.get("object")?;

        let ev = match kind {
            // The customer paid: Stripe created the Subscription. Bind it to the tenant.
            "checkout.session.completed" => {
                let subscription_ref = obj.get("subscription")?.as_str()?.to_string();
                let tenant_id = obj
                    .get("client_reference_id")?
                    .as_str()?
                    .parse::<Uuid>()
                    .ok()?;
                let meta = obj.get("metadata");
                let plan_id = meta
                    .and_then(|m| m.get("plan_id"))
                    .and_then(|v| v.as_str())?
                    .to_string();
                let billing_type = meta
                    .and_then(|m| m.get("billing_type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("card")
                    .to_string();
                EventKind::SubscriptionStarted {
                    subscription_ref,
                    tenant_id,
                    plan_id,
                    billing_type,
                }
            }
            // Recurring cycles.
            "invoice.paid" | "invoice.payment_succeeded" => EventKind::StatusChanged {
                subscription_ref: invoice_subscription(obj)?,
                status: "active",
            },
            "invoice.payment_failed" => EventKind::StatusChanged {
                subscription_ref: invoice_subscription(obj)?,
                status: "past_due",
            },
            "customer.subscription.deleted" => EventKind::StatusChanged {
                subscription_ref: obj.get("id")?.as_str()?.to_string(),
                status: "canceled",
            },
            _ => return None,
        };
        Some(WebhookEvent { event_id, kind: ev })
    }
}

#[cfg(test)]
mod webhook_tests {
    use super::*;

    fn gw(secret: Option<&str>) -> StripeGateway {
        StripeGateway {
            api_key: "sk_test".into(),
            webhook_secret: secret.map(str::to_string),
            base: String::new(),
            success_url: String::new(),
            cancel_url: String::new(),
        }
    }

    fn signed(secret: &str, ts: i64, body: &[u8]) -> HashMap<String, String> {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(ts.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        let sig = hex::encode(mac.finalize().into_bytes());
        HashMap::from([("stripe-signature".to_string(), format!("t={ts},v1={sig}"))])
    }

    /// The check that keeps the new timestamp window from breaking every real webhook: a correctly
    /// signed, current event must still pass.
    #[test]
    fn a_current_correctly_signed_webhook_passes() {
        let body = br#"{"id":"evt_1","type":"invoice.paid"}"#;
        let now = crate::signing::now_unix();
        assert!(gw(Some("whsec_test")).verify_webhook(body, &signed("whsec_test", now, body)));
    }

    /// A captured webhook must stop working. Without a window it replays forever, and replaying
    /// `SubscriptionStarted` re-grants a plan.
    #[test]
    fn an_old_webhook_is_refused_even_though_its_mac_is_valid() {
        let body = br#"{"id":"evt_1"}"#;
        let stale = crate::signing::now_unix() - (WEBHOOK_TOLERANCE_SECS + 60);
        let headers = signed("whsec_test", stale, body);
        assert!(
            !gw(Some("whsec_test")).verify_webhook(body, &headers),
            "the MAC is genuine; only its age disqualifies it"
        );
    }

    #[test]
    fn a_tampered_body_is_refused() {
        let now = crate::signing::now_unix();
        let headers = signed("whsec_test", now, br#"{"plan":"free"}"#);
        assert!(!gw(Some("whsec_test")).verify_webhook(br#"{"plan":"enterprise"}"#, &headers));
    }

    #[test]
    fn a_wrong_secret_is_refused() {
        let body = br#"{"id":"evt_1"}"#;
        let now = crate::signing::now_unix();
        assert!(!gw(Some("whsec_real")).verify_webhook(body, &signed("whsec_forged", now, body)));
    }

    #[test]
    fn a_missing_signature_header_is_refused() {
        assert!(!gw(Some("whsec_test")).verify_webhook(b"{}", &HashMap::new()));
    }

    /// The fail-open that was there before: no secret configured meant every caller was believed,
    /// on an endpoint that reads tenant_id and plan_id out of the request body.
    #[test]
    fn an_unconfigured_secret_refuses_in_release() {
        let accepted = gw(None).verify_webhook(b"{}", &HashMap::new());
        if cfg!(debug_assertions) {
            assert!(accepted, "dev stays convenient");
        } else {
            assert!(
                !accepted,
                "release must not accept unverified billing events"
            );
        }
    }
}
