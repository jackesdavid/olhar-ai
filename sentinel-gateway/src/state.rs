//! Gateway shared state. The upstream go2rtc is resolved per request from the JWT's
//! tenant: the gateway asks control (`/internal/box-upstream`, forwarding the token)
//! for that tenant's reported box host, cached briefly. `EDGE_UPSTREAM` is the static
//! fallback (single-tenant / dev, and for the identical player UI assets).
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::ws::WebSocket;
use futures::channel::oneshot;

/// A browser bridge waiting for the edge to dial in (live-over-outbound, card #144). The
/// `/edge/live/:session` handler hands the edge's upgraded socket through this once it connects.
pub type EdgeSession = oneshot::Sender<WebSocket>;

/// How control says to reach the edge for a live request.
pub enum LiveStart {
    /// The edge was told to dial the gateway back — await it on the rendezvous (no tailnet).
    Sse,
    /// No SSE box; the gateway dials this tailnet host itself, as before (`None` → static fallback).
    Tailnet(Option<String>),
}

/// Shared across handlers (always behind an `Arc`).
pub struct AppState {
    /// The fallback edge go2rtc base URL (`EDGE_UPSTREAM`), e.g.
    /// `https://tenant-001.<tailnet>.ts.net` (tailnet) or `http://127.0.0.1:1984` (dev).
    pub upstream: String,
    /// Control plane base (`CONTROL_URL`) for the per-tenant box-host lookup.
    pub control_url: String,
    /// This gateway's public WebSocket base (`GATEWAY_PUBLIC_WS`, e.g. `wss://gateway.example.com`)
    /// — the address an edge dials back for live-over-outbound. Empty → outbound live disabled
    /// (the gateway can't tell the edge where to reach it), so live falls back to the tailnet path.
    pub public_ws_url: String,
    /// Shared HTTP client for snapshot/asset passthrough + the control lookup.
    pub http: reqwest::Client,
    /// Shared with sentinel-control — verifies the `live.view` JWT on live endpoints.
    pub jwt_secret: String,
    /// tenant_id → (resolved base URL, fetched_at). Short TTL so a box coming online is
    /// picked up quickly without a control round-trip on every frame.
    cache: Mutex<HashMap<String, (String, Instant)>>,
    /// Upstreams that refused a WS connect, and when. Lets a box that cannot serve the preferred
    /// upstream be skipped instead of costing a failed connect on every single reconnect.
    dead: Mutex<HashMap<String, Instant>>,
    /// Pending live-over-outbound rendezvous: session id → the browser bridge awaiting the edge.
    pub edge_sessions: Mutex<HashMap<String, EdgeSession>>,
}

const CACHE_TTL: Duration = Duration::from_secs(30);
/// How long a refused upstream stays skipped. Short enough that a box upgraded mid-session starts
/// being preferred within minutes, without anyone restarting the gateway.
const DEAD_TTL: Duration = Duration::from_secs(300);

impl AppState {
    pub fn new(
        upstream: String,
        control_url: String,
        jwt_secret: String,
        public_ws_url: String,
    ) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("build reqwest client");
        AppState {
            upstream,
            control_url,
            public_ws_url,
            http,
            jwt_secret,
            cache: Mutex::new(HashMap::new()),
            dead: Mutex::new(HashMap::new()),
            edge_sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Ask control to set up a live stream (card #144). Returns `LiveStart::Sse` when control told
    /// the tenant's edge to dial `callback` back (outbound; no tailnet), or `LiveStart::Tailnet`
    /// with the host the gateway should dial itself. Any failure degrades to the tailnet path.
    pub async fn live_start(&self, token: &str, src: &str, callback: &str) -> LiveStart {
        let url = format!("{}/internal/live-start", self.control_url);
        let body = serde_json::json!({ "src": src, "callback": callback });
        let resp = match self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("live-start lookup failed ({url}): {e}");
                return LiveStart::Tailnet(None);
            }
        };
        let v: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("live-start decode failed: {e}");
                return LiveStart::Tailnet(None);
            }
        };
        match v.get("mode").and_then(|m| m.as_str()) {
            Some("sse") => LiveStart::Sse,
            _ => LiveStart::Tailnet(v.get("host").and_then(|h| h.as_str()).map(str::to_string)),
        }
    }

    /// The static fallback upstream (health, player UI assets).
    pub fn resolve_upstream(&self) -> &str {
        &self.upstream
    }

    /// The live go2rtc base URL for a tenant. Resolves the tenant's box host via control
    /// (cached `CACHE_TTL`), forwarding `token`; falls back to the static upstream when
    /// there's no tenant context or no box has reported a host.
    pub async fn upstream_for(&self, tid: Option<&str>, token: &str) -> String {
        let Some(tid) = tid else {
            return self.upstream.clone();
        };
        if let Some((base, at)) = self.cache.lock().unwrap().get(tid).cloned() {
            if at.elapsed() < CACHE_TTL {
                return base;
            }
        }
        let url = format!("{}/internal/box-upstream", self.control_url);
        let host = match self.http.get(&url).bearer_auth(token).send().await {
            Ok(r) => r
                .text()
                .await
                .ok()
                .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok())
                .and_then(|v| v.get("host").and_then(|h| h.as_str().map(str::to_string))),
            Err(e) => {
                log::warn!("box-upstream lookup failed ({url}): {e}");
                None
            }
        };
        let base = host
            .filter(|h| !h.is_empty())
            .map(|h| Self::ensure_scheme(&h))
            .unwrap_or_else(|| self.upstream.clone());
        self.cache
            .lock()
            .unwrap()
            .insert(tid.to_string(), (base.clone(), Instant::now()));
        base
    }

    /// Normalize a box host to a base URL with exactly one scheme — the edge may report it
    /// with or without `https://`. Prepend `https://` only when absent (keep explicit `http://`),
    /// so `ws_origin`/passthrough always get a valid URL.
    fn ensure_scheme(host: &str) -> String {
        let h = host.trim().trim_end_matches('/');
        if h.starts_with("http://") || h.starts_with("https://") {
            h.to_string()
        } else {
            format!("https://{h}")
        }
    }

    /// The WebSocket origin of the **edge's own** MSE server, derived from the go2rtc base the box
    /// reported — the edge serves its API on `:8080` over the tailnet (`tailscale serve --https=8080
    /// 8788`) and on `:8788` locally. `None` when the base is a shape we cannot map, in which case
    /// the caller just uses go2rtc.
    ///
    /// Mirrors `app_core::ports`'s derivation for the warm-snapshot path, which resolves the same
    /// two endpoints on the same box.
    pub fn live_edge_ws(&self, base: &str) -> Option<String> {
        let b = base.trim_end_matches('/');
        if let Some(rest) = b.strip_prefix("https://") {
            if rest.contains(':') {
                return None; // an explicit non-default port — not a shape we know how to map
            }
            return Some(format!("wss://{rest}:8080"));
        }
        let local = b.strip_prefix("http://")?;
        local
            .contains(":1984")
            .then(|| format!("ws://{}", local.replace(":1984", ":8788")))
    }

    /// Has this upstream recently refused us?
    pub fn live_edge_is_dead(&self, url: &str) -> bool {
        self.dead
            .lock()
            .unwrap()
            .get(url)
            .is_some_and(|at| at.elapsed() < DEAD_TTL)
    }

    pub fn live_edge_mark_dead(&self, url: &str) {
        self.dead
            .lock()
            .unwrap()
            .insert(url.to_string(), Instant::now());
    }

    /// Convert an `http(s)://` base to its WebSocket origin (`ws(s)://`).
    pub fn ws_origin(base: &str) -> String {
        if let Some(rest) = base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            base.to_string()
        }
    }
}

#[cfg(test)]
mod live_edge_ws_tests {
    use super::*;

    fn state() -> AppState {
        AppState::new(
            "http://127.0.0.1:1984".into(),
            "http://control".into(),
            "s".into(),
        )
    }

    /// The tailnet shape: the box reports its MagicDNS host, go2rtc answers on 443 and the edge's
    /// own API on 8080 (`tailscale serve --https=8080 8788`).
    #[test]
    fn maps_a_tailnet_host_to_the_edge_api_port() {
        assert_eq!(
            state().live_edge_ws("https://tenant-001.taile123.ts.net"),
            Some("wss://tenant-001.taile123.ts.net:8080".into())
        );
    }

    #[test]
    fn maps_the_local_go2rtc_port_to_the_local_edge_port() {
        assert_eq!(
            state().live_edge_ws("http://127.0.0.1:1984"),
            Some("ws://127.0.0.1:8788".into())
        );
    }

    /// An explicit port we did not put there means someone is pointing the gateway somewhere
    /// deliberate — guessing a sibling port would send live video to the wrong place.
    #[test]
    fn refuses_to_guess_when_the_base_carries_its_own_port() {
        assert_eq!(state().live_edge_ws("https://box.example.com:9000"), None);
        assert_eq!(state().live_edge_ws("http://127.0.0.1:9000"), None);
    }

    /// A refused upstream is skipped, but only until the note expires — a box upgraded mid-session
    /// must start being preferred again without a gateway restart.
    #[test]
    fn a_refused_upstream_is_remembered() {
        let s = state();
        assert!(!s.live_edge_is_dead("wss://box/api/ws"));
        s.live_edge_mark_dead("wss://box/api/ws");
        assert!(s.live_edge_is_dead("wss://box/api/ws"));
        assert!(!s.live_edge_is_dead("wss://other/api/ws"));
    }
}
