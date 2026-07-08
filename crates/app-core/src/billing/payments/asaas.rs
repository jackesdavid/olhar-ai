//! Asaas (Brazil) payment gateway — recurring Pix Automático / boleto / cartão, with
//! native NF-e (enabled later). Asaas owns the billing cycle + dunning; we create the
//! customer + subscription and react to its webhooks. Brazil-only (CNPJ + BRL rails).

use std::time::Duration;

use super::{PaymentEvent, PaymentGateway};

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
        let api_key = std::env::var("ASAAS_API_KEY")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        let base = std::env::var("ASAAS_BASE_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://api.asaas.com/v3".to_string());
        let webhook_token = std::env::var("ASAAS_WEBHOOK_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Some(Self { api_key, base, webhook_token })
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
}

impl PaymentGateway for AsaasGateway {
    fn provider(&self) -> &'static str {
        "asaas"
    }

    fn ensure_customer(&self, name: &str, email: &str, cpf_cnpj: Option<&str>) -> Result<String, String> {
        let mut body = serde_json::json!({ "name": name, "email": email });
        if let Some(doc) = cpf_cnpj {
            body["cpfCnpj"] = serde_json::json!(doc);
        }
        let out = self.post("/customers", body)?;
        out.get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("asaas: customer create returned no id: {out}"))
    }

    fn create_subscription(&self, customer_ref: &str, amount: f64, billing_type: &str, description: &str) -> Result<String, String> {
        // First charge tomorrow; Asaas then bills monthly on that day.
        let next_due = (chrono::Local::now() + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
        let body = serde_json::json!({
            "customer": customer_ref,
            "billingType": billing_type,
            "value": amount,
            "nextDueDate": next_due,
            "cycle": "MONTHLY",
            "description": description,
        });
        let out = self.post("/subscriptions", body)?;
        out.get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("asaas: subscription create returned no id: {out}"))
    }

    fn first_payment_url(&self, subscription_ref: &str) -> Option<String> {
        let out = self.get(&format!("/subscriptions/{subscription_ref}/payments")).ok()?;
        out.get("data")?
            .as_array()?
            .first()?
            .get("invoiceUrl")
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    fn verify_webhook(&self, token: Option<&str>) -> bool {
        match &self.webhook_token {
            Some(expected) => token == Some(expected.as_str()),
            None => true, // no token configured (dev) → accept
        }
    }

    fn parse_event(&self, body: &serde_json::Value) -> Option<PaymentEvent> {
        let event = body.get("event").and_then(|v| v.as_str())?;
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
        Some(PaymentEvent { subscription_ref, status })
    }
}
