//! Authenticated ONVIF SOAP calls (blocking, via ureq): validate credentials by
//! resolving the RTSP stream URI, and fetch a JPEG snapshot. Copied verbatim
//! from the ratatui `src/onvif/client.rs`.
use std::io::Read;
use std::process::Command;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use sha1::{Digest, Sha1};

use super::xml::{
    parse_first_profile_token, parse_media_url, parse_profiles, parse_uri, scan_open_tag,
};

const SOAP_TIMEOUT: Duration = Duration::from_secs(5);

/// Validate `username`/`password` against the camera at `xaddr`. On success
/// returns the RTSP stream URI (proof the credentials work). On failure returns
/// an error message — the caller must NOT persist credentials unless this is Ok.
pub fn test_credentials(xaddr: &str, username: &str, password: &str) -> Result<String, String> {
    let caps = get_capabilities(xaddr, username, password)?;
    let media = parse_media_url(&caps, xaddr);

    let profiles = get_profiles(&media, username, password)?;
    let token = parse_first_profile_token(&profiles)
        .ok_or_else(|| "No media profiles — credentials likely rejected".to_string())?;

    let stream = get_stream_uri_soap(&media, &token, username, password)?;
    parse_uri(&stream).ok_or_else(|| "Invalid credentials or no stream available".to_string())
}

/// Resolve both the **main** (highest-resolution) and **sub** (lowest-resolution) RTSP
/// stream URIs for a camera. The substream powers the low-latency preview feed; the main
/// stream backs the HD toggle. Cameras with a single profile return `(main, None)`.
///
/// Edge-only: these URIs embed nothing the cloud needs (they live on the LAN and the edge
/// injects credentials into them for go2rtc), so they're never reported to control.
pub fn resolve_stream_uris(
    xaddr: &str,
    username: &str,
    password: &str,
) -> Result<(String, Option<String>), String> {
    let caps = get_capabilities(xaddr, username, password)?;
    let media = parse_media_url(&caps, xaddr);

    let profiles_xml = get_profiles(&media, username, password)?;
    let profiles = parse_profiles(&profiles_xml);
    if profiles.is_empty() {
        return Err("No media profiles — credentials likely rejected".to_string());
    }

    // Highest-area profile is the main stream; lowest-area is the substream. With ties (or
    // missing resolutions) both can resolve to the same token → no distinct substream.
    let main = profiles.iter().max_by_key(|p| p.area()).unwrap();
    let sub = profiles.iter().min_by_key(|p| p.area()).unwrap();

    let main_uri = parse_uri(&get_stream_uri_soap(&media, &main.token, username, password)?)
        .ok_or_else(|| "Invalid credentials or no stream available".to_string())?;

    let sub_uri = if sub.token != main.token {
        parse_uri(&get_stream_uri_soap(&media, &sub.token, username, password)?)
            .filter(|u| u != &main_uri)
    } else {
        None
    };

    Ok((main_uri, sub_uri))
}

/// Fetch a JPEG snapshot. Tries ONVIF `GetSnapshotUri` + HTTP Basic GET; falls
/// back to a single RTSP frame via `ffmpeg` if the camera doesn't support it.
pub fn snapshot(
    xaddr: &str,
    username: &str,
    password: &str,
    rtsp_url: &str,
) -> Result<Vec<u8>, String> {
    let caps = get_capabilities(xaddr, username, password)?;
    let media = parse_media_url(&caps, xaddr);

    let profiles = get_profiles(&media, username, password)?;
    let token = parse_first_profile_token(&profiles)
        .ok_or_else(|| "No media profiles returned".to_string())?;

    let snap_xml = get_snapshot_uri_soap(&media, &token, username, password)?;

    // Camera doesn't support GetSnapshotUri → grab an RTSP frame instead.
    if scan_open_tag(&snap_xml, "Fault", 0).is_some() {
        return snapshot_from_rtsp(rtsp_url, username, password);
    }

    let url = parse_uri(&snap_xml).ok_or_else(|| "No snapshot Uri returned".to_string())?;
    let auth = format!("Basic {}", B64.encode(format!("{username}:{password}")));
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .get(&url)
        .set("Authorization", &auth)
        .call()
        .map_err(|e| format!("Snapshot request failed: {e}"))?;

    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read snapshot: {e}"))?;
    if bytes.is_empty() {
        return Err("Empty snapshot response".into());
    }
    Ok(bytes)
}

// ---- SOAP calls -------------------------------------------------------------

fn get_capabilities(xaddr: &str, user: &str, pass: &str) -> Result<String, String> {
    let body = r#"<tds:GetCapabilities xmlns:tds="http://www.onvif.org/ver10/device/wsdl">
      <tds:Category>Media</tds:Category>
    </tds:GetCapabilities>"#;
    soap_post(xaddr, &envelope(&wsse_header(user, pass), body))
}

fn get_profiles(media_url: &str, user: &str, pass: &str) -> Result<String, String> {
    soap_post(media_url, &envelope(&wsse_header(user, pass), "<trt:GetProfiles/>"))
}

fn get_stream_uri_soap(
    media_url: &str,
    token: &str,
    user: &str,
    pass: &str,
) -> Result<String, String> {
    let body = format!(
        r#"<trt:GetStreamUri>
      <trt:StreamSetup>
        <tt:Stream>RTP-Unicast</tt:Stream>
        <tt:Transport><tt:Protocol>RTSP</tt:Protocol></tt:Transport>
      </trt:StreamSetup>
      <trt:ProfileToken>{token}</trt:ProfileToken>
    </trt:GetStreamUri>"#
    );
    soap_post(media_url, &envelope(&wsse_header(user, pass), &body))
}

fn get_snapshot_uri_soap(
    media_url: &str,
    token: &str,
    user: &str,
    pass: &str,
) -> Result<String, String> {
    let body = format!(
        r#"<trt:GetSnapshotUri><trt:ProfileToken>{token}</trt:ProfileToken></trt:GetSnapshotUri>"#
    );
    soap_post(media_url, &envelope(&wsse_header(user, pass), &body))
}

/// POST a SOAP envelope, returning the body even on an HTTP error status (so we
/// can read SOAP Faults — e.g. bad credentials).
fn soap_post(url: &str, envelope: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout(SOAP_TIMEOUT).build();
    match agent
        .post(url)
        .set("Content-Type", "application/soap+xml; charset=utf-8")
        .send_string(envelope)
    {
        Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(_, resp)) => resp.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(format!("SOAP request to {url} failed: {e}")),
    }
}

// ---- WS-Security UsernameToken (PasswordDigest) ----------------------------

fn wsse_header(username: &str, password: &str) -> String {
    let mut nonce = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce);
    let nonce_b64 = B64.encode(nonce);
    let created = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut hasher = Sha1::new();
    hasher.update(nonce); // raw nonce bytes, not the base64
    hasher.update(created.as_bytes());
    hasher.update(password.as_bytes());
    let digest = B64.encode(hasher.finalize());

    format!(
        r#"<s:Header>
  <Security s:mustUnderstand="1"
    xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd">
    <UsernameToken>
      <Username>{username}</Username>
      <Password Type="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest">{digest}</Password>
      <Nonce EncodingType="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary">{nonce_b64}</Nonce>
      <Created xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd">{created}</Created>
    </UsernameToken>
  </Security>
</s:Header>"#
    )
}

fn envelope(header: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope
  xmlns:s="http://www.w3.org/2003/05/soap-envelope"
  xmlns:trt="http://www.onvif.org/ver10/media/wsdl"
  xmlns:tt="http://www.onvif.org/ver10/schema">
  {header}
  <s:Body>{body}</s:Body>
</s:Envelope>"#
    )
}

// ---- RTSP frame grab via ffmpeg --------------------------------------------

/// Insert `user:pass@` into an `rtsp://` URL unless it already carries creds.
/// Shared by the snapshot path and the live-stream path.
pub fn rtsp_url_with_creds(rtsp: &str, user: &str, pass: &str) -> String {
    if rtsp.contains('@') {
        rtsp.to_string()
    } else {
        rtsp.replacen("rtsp://", &format!("rtsp://{user}:{pass}@"), 1)
    }
}

fn snapshot_from_rtsp(rtsp: &str, user: &str, pass: &str) -> Result<Vec<u8>, String> {
    if rtsp.trim().is_empty() {
        return Err(
            "Camera has no RTSP URL — validate its credentials first (Devices → Test credentials)."
                .into(),
        );
    }
    let url = rtsp_url_with_creds(rtsp, user, pass);

    let out = Command::new("ffmpeg")
        .args([
            "-rtsp_transport", "tcp",
            "-i", &url,
            "-frames:v", "1",
            "-f", "image2",
            "-vcodec", "mjpeg",
            "-loglevel", "error",
            "pipe:1",
        ])
        .output()
        .map_err(|e| format!("ffmpeg not available: {e}"))?;

    if !out.status.success() {
        return Err(format!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    if out.stdout.is_empty() {
        return Err("ffmpeg produced no frame".into());
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtsp_url_with_creds_inserts_unless_present() {
        assert_eq!(
            rtsp_url_with_creds("rtsp://10.0.0.5/stream1", "admin", "pw"),
            "rtsp://admin:pw@10.0.0.5/stream1"
        );
        // already has credentials → unchanged
        assert_eq!(
            rtsp_url_with_creds("rtsp://x:y@10.0.0.5/stream1", "admin", "pw"),
            "rtsp://x:y@10.0.0.5/stream1"
        );
    }
}
