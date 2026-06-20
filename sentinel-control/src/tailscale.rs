//! Transparent edge enrollment: the operator holds a Tailscale OAuth client; control
//! mints a fresh **pre-authorized, reusable, non-ephemeral, tagged** auth key per box
//! via the Tailscale API and bakes it into the install command. The tenant never logs
//! into Tailscale — they run one line and the edge joins the private tailnet automatically.
//!
//! The key is **reusable + non-ephemeral** on purpose: an appliance reboots, and on
//! every boot the edge re-runs `tailscale up --authkey=<the key in edge.env>`. A
//! single-use or ephemeral key would work exactly once and then leave the box off the
//! tailnet (node wiped on disconnect, key already consumed) — the reboot-drops-offline
//! bug. Non-ephemeral also keeps the node's MagicDNS name stable, which the gateway
//! relies on to resolve the box's live upstream.
//!
//! One-time operator setup (Tailscale admin console):
//!   1. Create an OAuth client with the `auth_keys` write scope, tagged `tag:sentinel-edge`.
//!   2. Define `tag:sentinel-edge` in the tailnet ACL (owned by that OAuth client).
//! Then set TS_OAUTH_CLIENT_ID / TS_OAUTH_CLIENT_SECRET (+ optional TS_TAILNET, TS_TAG).
use serde::Deserialize;

const TOKEN_URL: &str = "https://api.tailscale.com/api/v2/oauth/token";

#[derive(Clone)]
pub struct TailscaleConfig {
    client_id: String,
    client_secret: String,
    /// Tailnet name, or "-" for the OAuth client's default tailnet.
    tailnet: String,
    /// The ACL tag every enrolled edge gets (must be owned by the OAuth client).
    tag: String,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
}

#[derive(Deserialize)]
struct KeyResp {
    key: String,
}

impl TailscaleConfig {
    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var("TS_OAUTH_CLIENT_ID").ok().filter(|s| !s.is_empty())?;
        let client_secret = std::env::var("TS_OAUTH_CLIENT_SECRET").ok().filter(|s| !s.is_empty())?;
        let tailnet = std::env::var("TS_TAILNET").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "-".into());
        let tag = std::env::var("TS_TAG").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "tag:sentinel-edge".into());
        Some(Self { client_id, client_secret, tailnet, tag })
    }

    /// Mint a reusable, non-ephemeral, pre-authorized auth key tagged for the edge.
    /// `description` shows in the Tailscale admin (e.g. the box name). The key persists
    /// (default 90 days, the Tailscale max; override with `TS_KEY_EXPIRY_SECS`) so the
    /// appliance can re-authenticate on every reboot without operator intervention.
    pub async fn mint_auth_key(&self, http: &reqwest::Client, description: &str) -> Result<String, String> {
        // Tailscale rejects non-ASCII / odd chars in key descriptions — sanitize.
        let description: String = description
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || " -_.".contains(c) { c } else { '-' })
            .collect();
        // 1. OAuth client-credentials → short-lived access token.
        let token: TokenResp = http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await
            .map_err(|e| format!("ts token request: {e}"))?
            .error_for_status()
            .map_err(|e| format!("ts token status: {e}"))?
            .json()
            .await
            .map_err(|e| format!("ts token parse: {e}"))?;

        // 2. Create a pre-authorized, reusable, non-ephemeral tagged auth key so the box
        //    re-auths cleanly on every reboot (see module docs). 90 days is the Tailscale
        //    default max; ops can shorten/lengthen (within tailnet policy) via env.
        let expiry_secs: u64 = std::env::var("TS_KEY_EXPIRY_SECS")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .filter(|&s| s > 0)
            .unwrap_or(7_776_000); // 90 days
        let body = serde_json::json!({
            "capabilities": { "devices": { "create": {
                "reusable": true,
                "ephemeral": false,
                "preauthorized": true,
                "tags": [self.tag],
            }}},
            "expirySeconds": expiry_secs,
            "description": description,
        });
        let key: KeyResp = http
            .post(format!("https://api.tailscale.com/api/v2/tailnet/{}/keys", self.tailnet))
            .bearer_auth(&token.access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("ts key request: {e}"))?
            .error_for_status()
            .map_err(|e| format!("ts key status: {e}"))?
            .json()
            .await
            .map_err(|e| format!("ts key parse: {e}"))?;
        Ok(key.key)
    }
}
