//! Why a Tuya connection is not working, in terms the tenant can act on (#182).
//!
//! Connecting is four things in sequence — credentials, API authorisations, a linked Smart Life
//! account, devices on it — and **three of the four failures look identical from the UI**: an empty
//! device list. The most common of them is the project living in a different data centre, which
//! reads as "your credentials are broken" and sends people to re-copy an Access Secret that was
//! never wrong.
//!
//! So this module answers a different question than `validate` does. `validate` says yes/no. This
//! says *which of the four steps failed, and what to open in the console*. It returns a machine
//! `kind` rather than a sentence, because the wording belongs in the frontend where it can be
//! localised.
use serde::Serialize;

use super::client::{self, TuyaError};
use super::domain::TuyaConfig;

/// Every data centre Tuya runs, matching the picker in the Integrations page. Probed in this order
/// when the configured one fails, so the common ones answer first.
pub const DATA_CENTERS: &[(&str, &str)] = &[
    ("https://openapi.tuyaus.com", "Western America"),
    ("https://openapi.tuyaeu.com", "Central Europe"),
    ("https://openapi.tuyain.com", "India"),
    ("https://openapi.tuyacn.com", "China"),
];

/// Which of the four steps a diagnosis stopped at. `Ok` means all four passed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Credentials,
    Apis,
    Account,
    Devices,
    Ok,
}

/// The result of probing one set of credentials. Serialised straight to the Integrations page.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    pub ok: bool,
    /// Where it stopped.
    pub step: Step,
    /// Stable identifier for the UI's localised sentence. See the `kind` values set below.
    pub kind: &'static str,
    /// Tuya's own error code, when there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i64>,
    /// Tuya's own message, shown as supporting detail under our sentence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// How many devices the project can see.
    pub devices: usize,
    /// How many Smart Life accounts this tenant has linked (0 is a diagnosis in itself).
    pub linked_accounts: usize,
    /// Set when the credentials failed HERE but worked against another data centre. This is the
    /// single most valuable field in the struct: it converts the most common failure from "your
    /// credentials are broken" into one click.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_region: Option<&'static str>,
}

impl Diagnosis {
    fn failed(step: Step, kind: &'static str, e: &TuyaError) -> Self {
        Diagnosis {
            ok: false,
            step,
            kind,
            code: e.code(),
            detail: Some(e.detail().to_string()),
            devices: 0,
            linked_accounts: 0,
            suggested_endpoint: None,
            suggested_region: None,
        }
    }
}

/// Probe `cfg` and say what is wrong. Never returns `Err` — a diagnosis IS the answer, including
/// when everything failed. `linked_uids` comes from `repo::linked_uids` (the caller owns the DB).
///
/// Costs one token call in the happy path. On a credentials failure it costs up to three more, and
/// only there: the connection is already broken, so nobody is waiting on a fast success.
pub fn diagnose(cfg: &TuyaConfig, linked_uids: &[String]) -> Diagnosis {
    // 1. Credentials — can we get a token at all?
    if let Err(e) = client::validate_typed(cfg) {
        // A missing API authorisation is NOT a credentials problem, even though it fails here.
        // Saying "wrong Access Secret" to someone whose secret is perfect is how support tickets
        // start, so it is reported as its own step.
        if matches!(e, TuyaError::NotSubscribed { .. }) {
            return Diagnosis::failed(Step::Apis, "not_subscribed", &e);
        }
        let mut d = Diagnosis::failed(Step::Credentials, e.kind(), &e);
        // Wrong data centre? Ask the others. A signature error never is one — the secret is
        // checked before the project is looked up — so only probe when it could be.
        if !matches!(e, TuyaError::Signature { .. }) {
            if let Some((endpoint, region)) = probe_data_centers(cfg) {
                d.kind = "wrong_data_center";
                d.suggested_endpoint = Some(endpoint.to_string());
                d.suggested_region = Some(region);
            }
        }
        return d;
    }

    // 2 + 3. Devices, and whether an account was ever linked. Tuya answers "no devices" the same
    // way whether nobody linked an account or the linked account is empty, so `linked_uids` is
    // what separates them.
    let devices = match client::fetch_devices_typed(cfg, linked_uids) {
        Ok(d) => d,
        Err(e) => {
            let step = if matches!(e, TuyaError::NotSubscribed { .. }) {
                Step::Apis
            } else {
                Step::Devices
            };
            return Diagnosis::failed(step, e.kind(), &e);
        }
    };

    let base = Diagnosis {
        ok: false,
        step: Step::Devices,
        kind: "no_devices",
        code: None,
        detail: None,
        devices: devices.len(),
        linked_accounts: linked_uids.len(),
        suggested_endpoint: None,
        suggested_region: None,
    };

    if devices.is_empty() {
        // No devices AND no linked account is the actionable one: scan the QR in step 4. No
        // devices WITH a linked account means the account itself is empty — a different fix, in
        // the Smart Life app rather than the console.
        if linked_uids.is_empty() {
            return Diagnosis {
                step: Step::Account,
                kind: "no_linked_account",
                ..base
            };
        }
        return base;
    }

    Diagnosis {
        ok: true,
        step: Step::Ok,
        kind: "ok",
        ..base
    }
}

/// Try the same credentials against every OTHER data centre. Returns the first that answers.
///
/// A Tuya project lives in exactly one data centre and its credentials are meaningless in the
/// others, so a success anywhere else is conclusive rather than a guess.
fn probe_data_centers(cfg: &TuyaConfig) -> Option<(&'static str, &'static str)> {
    for (endpoint, region) in DATA_CENTERS {
        if cfg.endpoint.trim_end_matches('/') == *endpoint {
            continue;
        }
        let probe = TuyaConfig {
            endpoint: endpoint.to_string(),
            ..cfg.clone()
        };
        if client::validate_typed(&probe).is_ok() {
            return Some((endpoint, region));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kinds are a contract with the frontend's i18n table — every one needs a key there, and
    /// a rename silently falls back to raw Tuya wording. Pinned so a rename fails here first.
    #[test]
    fn kinds_are_the_documented_set() {
        let all = [
            "signature",
            "not_subscribed",
            "api",
            "network",
            "wrong_data_center",
            "no_linked_account",
            "no_devices",
            "ok",
        ];
        assert_eq!(all.len(), 8, "update the i18n table when this changes");
    }

    /// A signature error must NOT trigger the data-centre probe: the secret is verified before the
    /// project is resolved, so every other endpoint would fail identically and we would burn three
    /// calls to learn nothing.
    #[test]
    fn signature_errors_do_not_suggest_a_region() {
        let e = TuyaError::Signature {
            code: 1004,
            msg: "sign invalid".into(),
        };
        let d = Diagnosis::failed(Step::Credentials, e.kind(), &e);
        assert_eq!(d.kind, "signature");
        assert!(d.suggested_endpoint.is_none());
        assert_eq!(d.code, Some(1004));
    }

    #[test]
    fn data_centers_cover_the_ui_picker() {
        // The Integrations page offers exactly these four; a probe that knows fewer would fail to
        // find a project sitting in the missing one.
        assert_eq!(DATA_CENTERS.len(), 4);
        assert!(DATA_CENTERS.iter().any(|(u, _)| u.contains("tuyaus")));
        assert!(DATA_CENTERS.iter().any(|(u, _)| u.contains("tuyaeu")));
        assert!(DATA_CENTERS.iter().any(|(u, _)| u.contains("tuyain")));
        assert!(DATA_CENTERS.iter().any(|(u, _)| u.contains("tuyacn")));
    }
}
