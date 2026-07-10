//! Asaas (Brazil) payment gateway — recurring Pix Automático / boleto / cartão, with
//! native NF-e (enabled later). Brazil-only (CNPJ + BRL rails). Kept behind the seam so it
//! can replace Stripe for Brazilian tenants when its cheaper Pix fees / NF-e matter.
//!
//! Unlike Stripe, Asaas creates the subscription up front and *then* exposes the first
//! charge's hosted invoice URL, so [`start_subscription`] returns both immediately.

use std::collections::HashMap;
use std::time::Duration;

use uuid::Uuid;

use super::{EventKind, PaymentGateway, PlanRef, StartedSubscription, WebhookEvent};

pub struct AsaasGateway {
    api_key: String,
    base: String,
    webhook_token: Option<String>,
}

impl AsaasGateway {
    /// Config from env: `ASAAS_API_KEY` (required), `ASAAS_BASE_URL` (default prod v3;
    /// set the sandbox URL for testing), `ASAAS_WEBHOOK_TOKEN` (optional — when set,
    /// inbound webhooks must present it in the `asaas-access-token` header).
    pub fn from_env() -> Option<Self> {
        let api_key = env_nonempty("ASAAS_API_KEY")?;
        let base = env_nonempty("ASAAS_BASE_URL")
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| "https://api.asaas.com/v3".to_string());
        Some(Self { api_key, base, webhook_token: env_nonempty("ASAAS_WEBHOOK_TOKEN") })
    }

    fn post(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
        ureq::post(&format!("{}{}", self.base, path))
            .set("access_token", &self.api_key)
            .set("Content-Type", "application/json")
            .timeout(Duration::from_secs(20))
            .send_string(&body.to_string())
            .map_err(|e| format!("asaas {path}: {e}"))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("asaas {path}: bad JSON: {e}"))
    }

    fn get(&self, path: &str) -> Result<serde_json::Value, String> {
        ureq::get(&format!("{}{}", self.base, path))
            .set("access_token", &self.api_key)
            .timeout(Duration::from_secs(20))
            .call()
            .map_err(|e| format!("asaas {path}: {e}"))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("asaas {path}: bad JSON: {e}"))
    }

    /// The hosted checkout URL of the subscription's first charge (Pix QR / boleto / card).
    fn first_payment_url(&self, subscription_ref: &str) -> Option<String> {
        let out = self.get(&format!("/subscriptions/{subscription_ref}/payments")).ok()?;
        out.get("data")?
            .as_array()?
            .first()?
            .get("invoiceUrl")
            .and_then(|v| v.as_str())
            .map(String::from)
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl PaymentGateway for AsaasGateway {
    /// Asaas takes recurring Pix natively — `billingType` is passed straight through, no mandate
    /// object to build.
    fn payment_methods(&self) -> &'static [&'static str] {
        &["PIX", "BOLETO", "CREDIT_CARD"]
    }

    fn provider(&self) -> &'static str {
        "asaas"
    }

    fn ensure_customer(&self, name: &str, email: &str, tax_id: Option<&str>) -> Result<String, String> {
        let mut body = serde_json::json!({ "name": name, "email": email });
        if let Some(doc) = tax_id {
            body["cpfCnpj"] = serde_json::json!(doc);
        }
        let out = self.post("/customers", body)?;
        out.get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("asaas: customer create returned no id: {out}"))
    }

    fn start_subscription(&self, customer_ref: &str, plan: &PlanRef<'_>, billing_type: &str, _tenant_id: Uuid) -> Result<StartedSubscription, String> {
        // Asaas bills in BRL only.
        if !plan.currency.eq_ignore_ascii_case("BRL") {
            return Err(format!("asaas bills in BRL only, tenant currency is {}", plan.currency));
        }
        // First charge tomorrow; Asaas then bills monthly on that day.
        let next_due = (chrono::Local::now() + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
        let out = self.post(
            "/subscriptions",
            serde_json::json!({
                "customer": customer_ref,
                "billingType": billing_type,
                "value": plan.amount,
                "nextDueDate": next_due,
                "cycle": "MONTHLY",
                "description": plan.name,
            }),
        )?;
        let subscription_ref = out
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("asaas: subscription create returned no id: {out}"))?;
        let payment_url = self.first_payment_url(&subscription_ref);
        Ok(StartedSubscription { subscription_ref: Some(subscription_ref), payment_url, price_ref: None })
    }

    fn verify_webhook(&self, _raw_body: &[u8], headers: &HashMap<String, String>) -> bool {
        match &self.webhook_token {
            Some(expected) => headers.get("asaas-access-token").map(String::as_str) == Some(expected.as_str()),
            None => true, // no token configured (dev) → accept
        }
    }

    fn parse_event(&self, raw_body: &[u8]) -> Option<WebhookEvent> {
        let body: serde_json::Value = serde_json::from_slice(raw_body).ok()?;
        let event = body.get("event")?.as_str()?;
        let status = match event {
            "PAYMENT_CONFIRMED" | "PAYMENT_RECEIVED" => "active",
            "PAYMENT_OVERDUE" => "past_due",
            "PAYMENT_DELETED" | "PAYMENT_REFUNDED" | "SUBSCRIPTION_DELETED" => "canceled",
            _ => return None,
        };
        // Payment events nest the subscription id under `payment.subscription`; a
        // subscription event carries it under `subscription.id`.
        let subscription_ref = body
            .get("payment")
            .and_then(|p| p.get("subscription"))
            .and_then(|v| v.as_str())
            .or_else(|| body.get("subscription").and_then(|s| s.get("id")).and_then(|v| v.as_str()))?
            .to_string();
        // Asaas has no event id; the payment id is unique per delivery-worthy event.
        let event_id = body
            .get("payment")
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_str())
            .map(|id| format!("{event}:{id}"))
            .unwrap_or_else(|| format!("{event}:{subscription_ref}"));
        Some(WebhookEvent { event_id, kind: EventKind::StatusChanged { subscription_ref, status } })
    }
}
