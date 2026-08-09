//! Tuya config + wire/row types. No credentials ever live on [`TuyaDevice`] — it is the
//! safe subset the editor and workflow nodes read (mirrors `devices::Device` vs the edge's
//! credentialed cache).
use serde::{Deserialize, Serialize};

/// The operator's single Tuya Cloud project. Resolved from the environment (like
/// `AiServiceConfig::from_env`) — `None` when unset, so callers report "not configured"
/// instead of panicking. `TUYA_ENDPOINT` defaults to the Western America data center
/// (validated for BR accounts).
#[derive(Debug, Clone)]
pub struct TuyaConfig {
    pub access_id: String,
    pub access_key: String,
    pub endpoint: String,
    /// The console's own id for this project (`p1784…`), captured from a URL the tenant pastes.
    ///
    /// NOT derivable from what we already hold: `access_id` is the project's Client ID, a different
    /// identifier, and no Cloud API exposes the console id for it. Optional — without it the link
    /// falls back to the region's project list.
    pub console_project_id: Option<String>,
    /// The console's data-centre code (`AZ`, `WE`, …), also taken from the pasted URL. The console
    /// wants its own code, not the API host, so it cannot be inferred from `endpoint`.
    pub console_region: Option<String>,
}

/// Console host for an API endpoint. `openapi.tuyaus.com` → `us.platform.tuya.com`.
///
/// Falls back to the global host rather than guessing: a wrong regional host lands on a login page
/// for a data centre the tenant has no account in, which reads as "your credentials are broken".
fn console_host(endpoint: &str) -> &'static str {
    if endpoint.contains("tuyaus") {
        "us.platform.tuya.com"
    } else if endpoint.contains("tuyaeu") {
        "eu.platform.tuya.com"
    } else if endpoint.contains("tuyain") {
        "in.platform.tuya.com"
    } else {
        "platform.tuya.com"
    }
}

/// Take the tenant to the console tab where another Smart Life account is linked.
///
/// Adding a second app account is a console action — there is no public API for the QR (see
/// `docs/ref/tuya.md`). The most this can do is remove the hunting, which is most of the problem:
/// after the first account is linked that tab shows a LIST, and the QR only returns behind an
/// "Add App Account" button that nobody finds by accident.
///
/// With no project id, links to the region's project list instead. Degrading beats demanding: the
/// button works from the moment credentials are saved, and gets sharper if the tenant pastes a URL.
pub fn console_link_url(cfg: &TuyaConfig) -> String {
    let host = console_host(&cfg.endpoint);
    match (&cfg.console_project_id, &cfg.console_region) {
        (Some(id), region) if !id.trim().is_empty() => {
            let dc = region.as_deref().unwrap_or("").trim();
            let region_q = if dc.is_empty() {
                String::new()
            } else {
                format!("&region={dc}")
            };
            // `toptab=related&deviceTab=4` IS the app-account tab; the ids are the console's, not
            // ours, so they are copied verbatim from a working URL rather than derived.
            format!("https://{host}/cloud/basic?id={id}{region_q}&toptab=related&deviceTab=4")
        }
        _ => format!("https://{host}/cloud/"),
    }
}

/// Pull the project id and data-centre code out of a console URL the tenant pasted.
///
/// Accepts a URL from ANY tab of the project, not just the account one — someone copying from the
/// address bar has whatever page they were on, and refusing it because the query string says
/// `deviceTab=1` would be pedantry. We keep the identity and rebuild the tab ourselves.
pub fn parse_console_url(url: &str) -> Option<(String, Option<String>)> {
    let q = url.split_once('?')?.1;
    let mut id = None;
    let mut region = None;
    for pair in q.split('&') {
        match pair.split_once('=') {
            Some(("id", v)) if !v.is_empty() => id = Some(v.to_string()),
            Some(("region", v)) if !v.is_empty() => region = Some(v.to_string()),
            _ => {}
        }
    }
    id.map(|i| (i, region))
}

impl TuyaConfig {
    pub fn from_env() -> Option<Self> {
        let access_id = env_nonempty("TUYA_ACCESS_ID")?;
        let access_key = env_nonempty("TUYA_ACCESS_KEY")?;
        let endpoint = env_nonempty("TUYA_ENDPOINT")
            .unwrap_or_else(|| "https://openapi.tuyaus.com".to_string())
            .trim_end_matches('/')
            .to_string();
        Some(TuyaConfig {
            access_id,
            access_key,
            endpoint,
            // env config is the dev path and has no console identity to carry
            console_project_id: None,
            console_region: None,
        })
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// One Tuya device linked to a tenant — the credential-free row the editor lists and the
/// nodes resolve. `functions_json` is the raw Tuya `functions` array (`[{code,type,values}]`),
/// carried through so the editor can render a value widget typed to the chosen function.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TuyaDevice {
    pub device_id: String,
    pub name: String,
    pub category: Option<String>,
    pub product_id: Option<String>,
    pub online: bool,
    pub functions_json: Option<String>,
    /// User-assigned friendly labels per data point, `{ "<code>": "<label>" }` JSON. Edited on the
    /// Integrations page; preserved across sync (not in `upsert_many`'s ON CONFLICT set).
    pub function_labels_json: Option<String>,
}

/// One controllable data point of a device (parsed from `functions_json`). `ty` is Tuya's
/// type name (`Boolean`/`Integer`/`Enum`/`String`); `values` is the raw constraint JSON
/// (Enum's `{"range":[…]}`, Integer's `{"min","max","step"}`, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuyaFunction {
    pub code: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub values: String,
}
