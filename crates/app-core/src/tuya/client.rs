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

/// Cap below the node watchdog (15s) so a stalled Tuya call fails fast with a clean error.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(12)).build()
}

fn now_ms() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0).to_string()
}

fn sha256_hex(body: &str) -> String {
    let mut h = Sha256::new();
    h.update(body.as_bytes());
    hex::encode(h.finalize())
}

fn sign(secret: &str, payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    hex::encode_upper(mac.finalize().into_bytes())
}

/// One signed request. `path` includes any query string (it is part of the signature).
/// Tuya returns HTTP 200 even for API errors, so success is read from the `success` field.
fn call(cfg: &TuyaConfig, method: &str, path: &str, token: Option<&str>, body: Option<&str>) -> Result<Value, String> {
    let t = now_ms();
    let body_str = body.unwrap_or("");
    let string_to_sign = format!("{method}\n{}\n\n{path}", sha256_hex(body_str));
    let sign_src = match token {
        Some(tok) => format!("{}{}{}{}", cfg.access_id, tok, t, string_to_sign),
        None => format!("{}{}{}", cfg.access_id, t, string_to_sign),
    };
    let signature = sign(&cfg.access_key, &sign_src);

    let url = format!("{}{}", cfg.endpoint, path);
    let mut req = if method == "POST" { agent().post(&url) } else { agent().get(&url) };
    req = req
        .set("client_id", &cfg.access_id)
        .set("sign", &signature)
        .set("t", &t)
        .set("sign_method", "HMAC-SHA256")
        .set("Content-Type", "application/json");
    if let Some(tok) = token {
        req = req.set("access_token", tok);
    }

    let resp = if method == "POST" { req.send_string(body_str) } else { req.call() };
    let json: Value = match resp {
        Ok(r) => r.into_json().map_err(|e| format!("tuya: malformed response: {e}"))?,
        Err(ureq::Error::Status(code, r)) => {
            return Err(format!("tuya http {code}: {}", r.into_string().unwrap_or_default()));
        }
        Err(e) => return Err(format!("tuya request failed: {e}")),
    };
    if json.get("success").and_then(Value::as_bool).unwrap_or(false) {
        Ok(json)
    } else {
        let code = json.get("code").and_then(Value::as_i64).unwrap_or(-1);
        let msg = json.get("msg").and_then(Value::as_str).unwrap_or("unknown error");
        Err(format!("tuya api error {code}: {msg}"))
    }
}

/// Verify a set of credentials by fetching a token. `Ok(())` = the access_id/access_key/endpoint
/// are valid; `Err` carries the Tuya error (wrong keys, wrong data center, unsubscribed API…).
pub fn validate(cfg: &TuyaConfig) -> Result<(), String> {
    token(cfg).map(|_| ())
}

/// A project token (grant_type=1). Valid ~2h; we fetch per operation (v1 — no cache).
fn token(cfg: &TuyaConfig) -> Result<String, String> {
    let j = call(cfg, "GET", "/v1.0/token?grant_type=1", None, None)?;
    j["result"]["access_token"].as_str().map(str::to_string).ok_or_else(|| "tuya: no access_token in response".to_string())
}

/// Every device linked to the project's app accounts, with its `functions` (type metadata)
/// attached per device. The `functions` are what the editor needs to type the value widget,
/// so we pay one extra call per device at sync time (rare, few devices).
/// `allowed_uids` scopes the result to devices owned by those linked Tuya app-account uids (the
/// tenant's OAuth-linked accounts). Empty = no filter (the operator/Default-tenant case, which was
/// linked via the console and has no per-tenant uid mapping).
pub fn fetch_devices(cfg: &TuyaConfig, allowed_uids: &[String]) -> Result<Vec<TuyaDevice>, String> {
    let tok = token(cfg)?;
    let listing = call(cfg, "GET", "/v1.0/iot-01/associated-users/devices?size=100", Some(&tok), None)?;
    let devices = listing["result"]["devices"].as_array().cloned().unwrap_or_default();

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
        let functions_json = match call(cfg, "GET", &format!("/v1.0/devices/{device_id}/functions"), Some(&tok), None) {
            Ok(f) => f["result"]["functions"].to_string(),
            Err(e) => {
                log::warn!("tuya: functions for {device_id} failed: {e}");
                "[]".to_string()
            }
        };
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

/// Send one control command (`{"commands":[{"code","value"}]}`) to a device.
pub fn send_command(cfg: &TuyaConfig, device_id: &str, code: &str, value: &Value) -> Result<Value, String> {
    let tok = token(cfg)?;
    let body = json!({ "commands": [ { "code": code, "value": value } ] }).to_string();
    call(cfg, "POST", &format!("/v1.0/devices/{device_id}/commands"), Some(&tok), Some(&body))
}

/// The device's current status as `(code, value)` pairs.
pub fn get_status(cfg: &TuyaConfig, device_id: &str) -> Result<Vec<(String, Value)>, String> {
    let tok = token(cfg)?;
    let j = call(cfg, "GET", &format!("/v1.0/devices/{device_id}/status"), Some(&tok), None)?;
    let arr = j["result"].as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .filter_map(|s| Some((s["code"].as_str()?.to_string(), s["value"].clone())))
        .collect())
}
