//! Outbound live push (card #144, live-over-outbound).
//!
//! The gateway can only dial *into* the edge's `/api/ws` where the edge is directly addressable.
//! An outbound-only edge can't be reached that way, so we
//! flip the direction: on a `start_live` command (delivered over the SSE channel), the edge dials
//! the gateway and pushes the camera's fMP4 up that connection. The **wire contract is identical**
//! to the inbound `mse_socket` — the gateway bridges browser↔edge transparently — only the socket's
//! direction changes. gst-linked only (live is the GStreamer MSE path, G2).

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::state::AppState;

/// Dial `url`, start/join camera `ip`'s shared pipeline, and stream init + fragments up until the
/// gateway (or the browser behind it) closes. `hd` picks main vs substream, matching `/api/ws`.
pub async fn run(url: String, ip: String, hd: bool, state: Arc<AppState>) {
    // Idempotent: starts the shared pipeline if this camera has no consumer yet, joins it otherwise.
    // Split keeps preview/HD in separate slots so motion (sub) and Live HD (main) coexist.
    let slot = crate::capture::stream_slot(&state, &ip, hd);
    crate::capture::ensure(&state, &ip, hd);

    // The pipeline needs a moment to reach its first fragment on a cold camera; poll rather than
    // block, and give up rather than hold a socket open on a camera that never comes.
    let mut feed = None;
    for _ in 0..100 {
        if let Some(f) = crate::capture::mse_feed(&ip, slot) {
            feed = Some(f);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let Some((init, mime, mut fragments)) = feed else {
        log::warn!("live-push: {ip} produced no fragment within 10s");
        return;
    };

    let ws = match tokio_tungstenite::connect_async(&url).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            log::warn!("live-push: connect {url} failed: {e}");
            return;
        }
    };
    let (mut tx, mut rx) = ws.split();

    // The browser speaks first, announcing what it can decode (piped through the gateway). Wait for
    // it so our reply cannot race ahead of the player's `sourceopen` — exactly as `mse_socket` does.
    loop {
        match rx.next().await {
            Some(Ok(Message::Text(_))) => break,
            Some(Ok(_)) => continue,
            _ => return,
        }
    }

    let hello = serde_json::json!({ "type": "mse", "value": mime }).to_string();
    if tx.send(Message::Text(hello.into())).await.is_err() {
        return;
    }
    // `ftyp` + `moov` first — a fragment before it is undecodable.
    if tx.send(Message::Binary(init.into())).await.is_err() {
        return;
    }
    log::info!("live-push: {ip} streaming to the gateway");

    // Chrome (unlike Safari) refuses a cold start on a P-frame — wait for the next IDR/IRAP
    // before the first media fragment. Mid-GOP joins were "all cameras work in Safari / none in
    // Chrome" after the default-base-is-moof fix.
    let mut started = false;
    loop {
        tokio::select! {
            frag = fragments.recv() => match frag {
                Ok(bytes) => {
                    if !started {
                        if !crate::gst_mse::fragment_has_keyframe(&bytes) {
                            continue;
                        }
                        started = true;
                    }
                    if tx.send(Message::Binary(bytes.to_vec().into())).await.is_err() {
                        break;
                    }
                }
                // A viewer that fell behind by more than the channel holds is resynced by closing:
                // the player reconnects and gets a fresh init segment.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("live-push: {ip} viewer lagged {n} fragments — dropping to resync");
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            // Notice a gateway/browser close promptly rather than at the next fragment.
            msg = rx.next() => {
                if msg.is_none() { break; }
            }
        }
    }
    log::info!("live-push: {ip} stream closed");
}
