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
}

impl TuyaConfig {
    pub fn from_env() -> Option<Self> {
        let access_id = env_nonempty("TUYA_ACCESS_ID")?;
        let access_key = env_nonempty("TUYA_ACCESS_KEY")?;
        let endpoint = env_nonempty("TUYA_ENDPOINT")
            .unwrap_or_else(|| "https://openapi.tuyaus.com".to_string())
            .trim_end_matches('/')
            .to_string();
        Some(TuyaConfig { access_id, access_key, endpoint })
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
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
