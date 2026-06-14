//! Gateway shared state. The upstream go2rtc is resolved per request from the JWT's
//! tenant: the gateway asks control (`/internal/box-upstream`, forwarding the token)
//! for that tenant's reported box host, cached briefly. `EDGE_UPSTREAM` is the static
//! fallback (single-tenant / dev, and for the identical player UI assets).
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Shared across handlers (always behind an `Arc`).
pub struct AppState {
    /// The fallback edge go2rtc base URL (`EDGE_UPSTREAM`), e.g.
    /// `https://tenant-001.<tailnet>.ts.net` (tailnet) or `http://127.0.0.1:1984` (dev).
    pub upstream: String,
    /// Control plane base (`CONTROL_URL`) for the per-tenant box-host lookup.
    pub control_url: String,
    /// Shared HTTP client for snapshot/asset passthrough + the control lookup.
    pub http: reqwest::Client,
    /// Shared with sentinel-control — verifies the `live.view` JWT on live endpoints.
    pub jwt_secret: String,
    /// tenant_id → (resolved base URL, fetched_at). Short TTL so a box coming online is
    /// picked up quickly without a control round-trip on every frame.
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

const CACHE_TTL: Duration = Duration::from_secs(30);

impl AppState {
    pub fn new(upstream: String, control_url: String, jwt_secret: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("build reqwest client");
        AppState { upstream, control_url, http, jwt_secret, cache: Mutex::new(HashMap::new()) }
    }

    /// The static fallback upstream (health, player UI assets).
    pub fn resolve_upstream(&self) -> &str {
        &self.upstream
    }

    /// The live go2rtc base URL for a tenant. Resolves the tenant's box host via control
    /// (cached `CACHE_TTL`), forwarding `token`; falls back to the static upstream when
    /// there's no tenant context or no box has reported a host.
    pub async fn upstream_for(&self, tid: Option<&str>, token: &str) -> String {
        let Some(tid) = tid else { return self.upstream.clone() };
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
        let base = host.filter(|h| !h.is_empty()).unwrap_or_else(|| self.upstream.clone());
        self.cache.lock().unwrap().insert(tid.to_string(), (base.clone(), Instant::now()));
        base
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
