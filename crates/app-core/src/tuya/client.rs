//! Signed HTTP client for the Tuya Cloud (OpenAPI). Auth is HMAC-SHA256 over a canonical
//! string, per Tuya's "Sign Requests" spec — token requests sign `client_id + t + STS`,
//! business requests sign `client_id + access_token + t + STS`, where
//! `STS = METHOD\nSHA256(body)\n\n<path>`. Synchronous `ureq` with a bounded timeout (below
//! the node watchdog), mirroring `workflows::node::network`/`recording`.
//!
//! Ported 1:1 from the validated `tuya_probe.py`/`tuya_control.py` prototypes.
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::domain::{TuyaConfig, TuyaDevice};

type HmacSha256 = Hmac<Sha256>;

/// What went wrong, as a KIND rather than a sentence (#182).
///
/// Every Tuya failure used to surface as the same `tuya api error {code}: {msg}` string, which is
/// Tuya's own wording written for a developer reading API docs — not for someone who has to know
/// which console screen to open. Worse, three of the five real failures are indistinguishable from
/// the UI because they all end in an empty device list.
///
/// The kind is a stable machine string, so the **frontend** owns the wording and can localise it
/// (i18n lives in `sentinel-app/src/lib/i18n`, never in backend strings). `Display` still produces
/// an English sentence, because plenty of call sites — workflow node errors, run logs — only have
/// somewhere to put a string.
#[derive(Debug, Clone, PartialEq)]
pub enum TuyaError {
    /// `1004` — the signature did not verify. In practice: the Access Secret is wrong or was
    /// copied with whitespace. Notably NOT "the account is wrong".
    Signature { code: i64, msg: String },
    /// `1106` / `2406` / `28841105` — the project exists but this API is not authorised for it.
    /// The fix is a specific console screen (Service API → Go to Authorize), not new credentials.
    NotSubscribed { code: i64, msg: String },
    /// Anything else Tuya answered with.
    Api { code: i64, msg: String },
    /// Never reached Tuya at all — DNS, TLS, timeout. Distinct because retrying is the fix.
    Network(String),
}

impl TuyaError {
    /// The stable identifier the UI maps to localised copy. Adding a variant means adding a key.
    pub fn kind(&self) -> &'static str {
        match self {
            TuyaError::Signature { .. } => "signature",
            TuyaError::NotSubscribed { .. } => "not_subscribed",
            TuyaError::Api { .. } => "api",
            TuyaError::Network(_) => "network",
        }
    }

    pub fn code(&self) -> Option<i64> {
        match self {
            TuyaError::Signature { code, .. }
            | TuyaError::NotSubscribed { code, .. }
            | TuyaError::Api { code, .. } => Some(*code),
            TuyaError::Network(_) => None,
        }
    }

    /// Tuya's own message, kept verbatim as supporting detail. We show our sentence AND this —
    /// inventing a friendly message for a code we have not seen is worse than showing the raw one.
    pub fn detail(&self) -> &str {
        match self {
            TuyaError::Signature { msg, .. }
            | TuyaError::NotSubscribed { msg, .. }
            | TuyaError::Api { msg, .. }
            | TuyaError::Network(msg) => msg,
        }
    }

    /// Classify a Tuya API error code. The three "not subscribed" codes all mean the same thing to
    /// a person — the project is missing an API authorisation — even though Tuya words them
    /// differently depending on which call tripped it.
    fn from_api(code: i64, msg: String) -> Self {
        match code {
            1004 => TuyaError::Signature { code, msg },
            1106 | 2406 | 28841105 => TuyaError::NotSubscribed { code, msg },
            _ => TuyaError::Api { code, msg },
        }
    }
}

impl std::fmt::Display for TuyaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TuyaError::Signature { msg, .. } => {
                write!(f, "tuya: signature rejected — check the Access Secret ({msg})")
            }
            TuyaError::NotSubscribed { code, msg } => write!(
                f,
                "tuya: the project is not authorised for this API — subscribe it in the console (code {code}: {msg})"
            ),
            TuyaError::Api { code, msg } => write!(f, "tuya api error {code}: {msg}"),
            TuyaError::Network(e) => write!(f, "tuya request failed: {e}"),
        }
    }
}

impl From<TuyaError> for String {
    fn from(e: TuyaError) -> String {
        e.to_string()
    }
}

/// Cap below the node watchdog (15s) so a stalled Tuya call fails fast with a clean error.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .build()
}

fn now_ms() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
        .to_string()
}

fn sha256_hex(body: &str) -> String {
    let mut h = Sha256::new();
    h.update(body.as_bytes());
    hex::encode(h.finalize())
}

fn sign(secret: &str, payload: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    hex::encode_upper(mac.finalize().into_bytes())
}

/// One signed request. `path` includes any query string (it is part of the signature).
/// Tuya returns HTTP 200 even for API errors, so success is read from the `success` field.
fn call(
    cfg: &TuyaConfig,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> Result<Value, TuyaError> {
    let t = now_ms();
    let body_str = body.unwrap_or("");
    let string_to_sign = format!("{method}\n{}\n\n{path}", sha256_hex(body_str));
    let sign_src = match token {
        Some(tok) => format!("{}{}{}{}", cfg.access_id, tok, t, string_to_sign),
        None => format!("{}{}{}", cfg.access_id, t, string_to_sign),
    };
    let signature = sign(&cfg.access_key, &sign_src);

    let url = format!("{}{}", cfg.endpoint, path);
    let mut req = if method == "POST" {
        agent().post(&url)
    } else {
        agent().get(&url)
    };
    req = req
        .set("client_id", &cfg.access_id)
        .set("sign", &signature)
        .set("t", &t)
        .set("sign_method", "HMAC-SHA256")
        .set("Content-Type", "application/json");
    if let Some(tok) = token {
        req = req.set("access_token", tok);
    }

    let resp = if method == "POST" {
        req.send_string(body_str)
    } else {
        req.call()
    };
    let json: Value = match resp {
        Ok(r) => r
            .into_json()
            .map_err(|e| TuyaError::Network(format!("malformed response: {e}")))?,
        // An HTTP status error is still Tuya answering, but not in the shape we can classify —
        // keep the body, it is the only clue.
        Err(ureq::Error::Status(code, r)) => {
            return Err(TuyaError::Api {
                code: code as i64,
                msg: r.into_string().unwrap_or_default(),
            });
        }
        Err(e) => return Err(TuyaError::Network(e.to_string())),
    };
    if json
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Ok(json)
    } else {
        let code = json.get("code").and_then(Value::as_i64).unwrap_or(-1);
        let msg = json
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        Err(TuyaError::from_api(code, msg.to_string()))
    }
}

/// Verify a set of credentials by fetching a token. `Ok(())` = the access_id/access_key/endpoint
/// are valid; `Err` carries the Tuya error (wrong keys, wrong data center, unsubscribed API…).
pub fn validate(cfg: &TuyaConfig) -> Result<(), String> {
    validate_typed(cfg).map_err(String::from)
}

/// `validate`, keeping the KIND. The diagnosis path (#182) needs to tell "wrong secret" from
/// "API not authorised" from "never reached Tuya" — three different things to do about it.
pub fn validate_typed(cfg: &TuyaConfig) -> Result<(), TuyaError> {
    token(cfg).map(|_| ())
}

/// A project token (grant_type=1). Valid ~2h; we fetch per operation (v1 — no cache).
fn token(cfg: &TuyaConfig) -> Result<String, TuyaError> {
    let j = call(cfg, "GET", "/v1.0/token?grant_type=1", None, None)?;
    j["result"]["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| TuyaError::Network("no access_token in response".into()))
}

/// Every device linked to the project's app accounts, with its `functions` (type metadata)
/// attached per device. The `functions` are what the editor needs to type the value widget,
/// so we pay one extra call per device at sync time (rare, few devices).
/// `allowed_uids` scopes the result to devices owned by those linked Tuya app-account uids (the
/// tenant's OAuth-linked accounts). Empty = no filter (the operator/Default-tenant case, which was
/// linked via the console and has no per-tenant uid mapping).
pub fn fetch_devices(cfg: &TuyaConfig, allowed_uids: &[String]) -> Result<Vec<TuyaDevice>, String> {
    fetch_devices_typed(cfg, allowed_uids).map_err(String::from)
}

/// `fetch_devices`, keeping the KIND (see `validate_typed`).
pub fn fetch_devices_typed(
    cfg: &TuyaConfig,
    allowed_uids: &[String],
) -> Result<Vec<TuyaDevice>, TuyaError> {
    let tok = token(cfg)?;
    let listing = call(
        cfg,
        "GET",
        "/v1.0/iot-01/associated-users/devices?size=100",
        Some(&tok),
        None,
    )?;
    let devices = listing["result"]["devices"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::with_capacity(devices.len());
    for d in devices {
        let device_id = d["id"].as_str().unwrap_or_default().to_string();
        if device_id.is_empty() {
            continue;
        }
        if !allowed_uids.is_empty() {
            let uid = d["uid"].as_str().unwrap_or_default();
            if !allowed_uids.iter().any(|u| u == uid) {
                continue;
            }
        }
        let functions_json = device_functions(cfg, &tok, &device_id);
        out.push(TuyaDevice {
            device_id,
            name: d["name"].as_str().unwrap_or_default().to_string(),
            category: d["category"].as_str().map(str::to_string),
            product_id: d["product_id"].as_str().map(str::to_string),
            online: d["online"].as_bool().unwrap_or(false),
            functions_json: Some(functions_json),
            function_labels_json: None, // labels are user-set + preserved by upsert_many; sync never sets them
        });
    }
    Ok(out)
}


/// A device's controllable functions, as JSON.
///
/// TWO endpoints, in this order, because one of them does not work for a large class of real
/// devices. `/v1.0/devices/{id}/functions` only covers products migrated to Tuya's **standard
/// instruction set**; anything else answers `2003 function not support`. A gate opener
/// (category `ckmkzq`) is exactly that case — reported from production, where the device worked
/// perfectly in the Smart Life app and arrived here with an empty control list.
///
/// `/v1.0/iot-03/devices/{id}/specification` is the modern one and returns the instruction set
/// AND the status set for both kinds. It is tried FIRST; the legacy call stays as the fallback
/// rather than being deleted, since it is what the already-working devices were read with and
/// there is no reason to risk them on one report.
fn device_functions(cfg: &TuyaConfig, tok: &str, device_id: &str) -> String {
    // 1. THE THING MODEL (v2). Proven against a real gate module whose product answers 2009 on
    //    every v1.0/v1.1 instruction-set endpoint while working perfectly in the Smart Life app:
    //    the product is published as a thing model, and the v1 family simply does not describe it.
    //    `shadow/properties` carries code, dp_id, type and the CURRENT value for every property.
    if let Ok(j) = call(
        cfg,
        "GET",
        &format!("/v2.0/cloud/thing/{device_id}/shadow/properties"),
        Some(tok),
        None,
    ) {
        if let Some(props) = j["result"]["properties"].as_array() {
            if !props.is_empty() {
                let fns: Vec<Value> = props
                    .iter()
                    .filter_map(|p| {
                        let code = p["code"].as_str()?;
                        Some(json!({
                            "code": code,
                            "name": p["custom_name"].as_str().filter(|n| !n.is_empty()).unwrap_or(code),
                            "desc": "",
                            "type": thing_type_to_tuya(p["type"].as_str().unwrap_or("string")),
                            "values": "{}",
                        }))
                    })
                    .collect();
                if !fns.is_empty() {
                    return Value::Array(fns).to_string();
                }
            }
        }
    }

    // 2. The v1 instruction set, which is what the already-working devices use. Kept ahead of the
    //    legacy call and unchanged in behaviour.
    if let Ok(j) = call(
        cfg,
        "GET",
        &format!("/v1.0/iot-03/devices/{device_id}/specification"),
        Some(tok),
        None,
    ) {
        let f = &j["result"]["functions"];
        if f.as_array().is_some_and(|a| !a.is_empty()) {
            return f.to_string();
        }
    }

    // 3. The legacy reader.
    match call(
        cfg,
        "GET",
        &format!("/v1.0/devices/{device_id}/functions"),
        Some(tok),
        None,
    ) {
        Ok(f) => {
            let arr = &f["result"]["functions"];
            if arr.as_array().is_some_and(|a| !a.is_empty()) {
                return arr.to_string();
            }
        }
        Err(e) => log::warn!("tuya: functions for {device_id} failed: {e}"),
    }
    "[]".to_string()
}

/// Thing-model property types → the vocabulary the editor's value widget already understands.
fn thing_type_to_tuya(t: &str) -> &'static str {
    match t {
        "bool" => "Boolean",
        "value" | "int" | "integer" => "Integer",
        "enum" => "Enum",
        _ => "String",
    }
}

/// Send one control command (`{"commands":[{"code","value"}]}`) to a device.
pub fn send_command(
    cfg: &TuyaConfig,
    device_id: &str,
    code: &str,
    value: &Value,
) -> Result<Value, String> {
    let tok = token(cfg).map_err(String::from)?;
    // The v1 command endpoint answers `2008 command or value not support` for thing-model
    // products — verified against a real device, where the v2 write below turned the same data
    // point on and off while v1 refused it. v1 stays as the fallback because the devices that
    // already work go through it.
    let props = json!({ code: value }).to_string();
    let v2 = call(
        cfg,
        "POST",
        &format!("/v2.0/cloud/thing/{device_id}/shadow/properties/issue"),
        Some(&tok),
        Some(&json!({ "properties": props }).to_string()),
    );
    match v2 {
        Ok(j) => return Ok(j),
        Err(e) => log::warn!("tuya: thing-model write for {device_id} failed: {e}; trying v1"),
    }
    let body = json!({ "commands": [ { "code": code, "value": value } ] }).to_string();
    call(
        cfg,
        "POST",
        &format!("/v1.0/devices/{device_id}/commands"),
        Some(&tok),
        Some(&body),
    )
    .map_err(String::from)
}

/// The device's current status as `(code, value)` pairs.
pub fn get_status(cfg: &TuyaConfig, device_id: &str) -> Result<Vec<(String, Value)>, String> {
    let tok = token(cfg).map_err(String::from)?;
    // Same split as `device_functions`: the thing model answers for products the v1 status
    // endpoint rejects with `2003 function not support`.
    if let Ok(j) = call(
        cfg,
        "GET",
        &format!("/v2.0/cloud/thing/{device_id}/shadow/properties"),
        Some(&tok),
        None,
    ) {
        if let Some(props) = j["result"]["properties"].as_array() {
            if !props.is_empty() {
                return Ok(props
                    .iter()
                    .filter_map(|p| Some((p["code"].as_str()?.to_string(), p["value"].clone())))
                    .collect());
            }
        }
    }
    let arr = match call(
        cfg,
        "GET",
        &format!("/v1.0/devices/{device_id}/status"),
        Some(&tok),
        None,
    ) {
        Ok(j) => j["result"].as_array().cloned().unwrap_or_default(),
        Err(e) => {
            log::warn!("tuya: status for {device_id} failed ({e}); trying the device detail");
            let j = call(cfg, "GET", &format!("/v1.0/devices/{device_id}"), Some(&tok), None)
                .map_err(String::from)?;
            j["result"]["status"].as_array().cloned().unwrap_or_default()
        }
    };
    Ok(arr
        .into_iter()
        .filter_map(|s| Some((s["code"].as_str()?.to_string(), s["value"].clone())))
        .collect())
}

