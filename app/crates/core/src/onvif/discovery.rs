//! ONVIF WS-Discovery over UDP multicast (blocking). Sends a SOAP `Probe` to
//! 239.255.255.250:3702 and collects the unicast `ProbeMatch` replies.
use std::net::UdpSocket;
use std::time::Duration;

use rand::RngCore;

use super::domain::DiscoveredDevice;
use super::xml::{parse_endpoint_address, parse_xaddrs_and_scopes};

const MULTICAST: &str = "239.255.255.250:3702";

/// Discover ONVIF video transmitters on the local network. Blocks for up to
/// ~10s (2 send rounds × 5 × 2s recv timeout). Returns `[]` on any socket error.
pub fn discover() -> Vec<DiscoveredDevice> {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return Vec::new();
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(2000)));

    let probe = build_probe(&random_uuid());
    let mut seen: Vec<String> = Vec::new();
    let mut raw: Vec<(String, Vec<String>, String)> = Vec::new();

    for _ in 0..2 {
        if sock.send_to(probe.as_bytes(), MULTICAST).is_err() {
            continue;
        }
        for _ in 0..5 {
            let mut buf = vec![0u8; 65535];
            match sock.recv_from(&mut buf) {
                Ok((n, addr)) => {
                    let key = addr.to_string();
                    if !seen.contains(&key) {
                        seen.push(key);
                        let xml = String::from_utf8_lossy(&buf[..n]).into_owned();
                        if let Some((xaddr, scopes)) = parse_xaddrs_and_scopes(&xml) {
                            let id = parse_endpoint_address(&xml).unwrap_or_default();
                            raw.push((xaddr, scopes, id));
                        }
                    }
                }
                Err(_) => break, // read timeout — move to the next send round
            }
        }
    }

    raw.into_iter()
        .map(|(xaddr, scopes, id)| {
            let ip = extract_ip(&xaddr);
            let mut manufacturer = None;
            let mut model = None;
            let mut name = ip.clone();
            for scope in &scopes {
                if let Some(v) = scope.strip_prefix("onvif://www.onvif.org/hardware/") {
                    model = Some(v.to_string());
                    name = v.to_string();
                }
                if let Some(v) = scope.strip_prefix("onvif://www.onvif.org/name/") {
                    manufacturer = Some(v.to_string());
                }
            }
            DiscoveredDevice { id, xaddr, ip, name, manufacturer, model }
        })
        .collect()
}

fn extract_ip(xaddr: &str) -> String {
    xaddr
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(xaddr)
        .split(':')
        .next()
        .unwrap_or(xaddr)
        .to_string()
}

/// A random v4-style UUID for the probe MessageID (we don't pull the uuid crate).
fn random_uuid() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn build_probe(uuid: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"
            xmlns:w="http://schemas.xmlsoap.org/ws/2004/08/addressing"
            xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery"
            xmlns:dn="http://www.onvif.org/ver10/network/wsdl">
  <e:Header>
    <w:MessageID>uuid:{uuid}</w:MessageID>
    <w:To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</w:To>
    <w:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</w:Action>
  </e:Header>
  <e:Body>
    <d:Probe>
      <d:Types>dn:NetworkVideoTransmitter</d:Types>
    </d:Probe>
  </e:Body>
</e:Envelope>"#
    )
}
