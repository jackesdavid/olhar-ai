//! Gateway shared state. Today the upstream is a single configured edge box; the
//! `resolve_upstream` seam is exactly where the per-tenant registry lookup
//! (`tenant_boxes` keyed by the JWT's `tenant_id`) slots in for multi-tenancy.
use std::time::Duration;

/// Shared across handlers. Cheap to clone (the reqwest client is internally Arc'd).
#[derive(Clone)]
pub struct AppState {
    /// The edge's go2rtc base URL (`EDGE_UPSTREAM`), e.g.
    /// `https://tenant-001.<tailnet>.ts.net` (tailnet) or `http://127.0.0.1:1984` (dev).
    pub upstream: String,
    /// Shared HTTP client for snapshot/asset passthrough (rustls, connect timeout).
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(upstream: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("build reqwest client");
        AppState { upstream, http }
    }

    /// The upstream go2rtc base URL for this request. **The tenancy seam:** later
    /// this validates the JWT and looks up `tenant_boxes.magicdns_host` (503 if the
    /// box is offline). For now it's the single configured edge.
    pub fn resolve_upstream(&self) -> &str {
        &self.upstream
    }

    /// The same upstream as a WebSocket origin (`http`→`ws`, `https`→`wss`).
    pub fn ws_upstream(&self) -> String {
        let base = self.resolve_upstream();
        if let Some(rest) = base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            base.to_string()
        }
    }
}
