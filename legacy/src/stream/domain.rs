//! Live-stream state: the running ffmpeg child, the frame channel, the latest
//! frame, and lifecycle — now with supervised auto-reconnect. Teardown is tied
//! to `Drop` (kill + reap the child) — the hook that fires for Esc, leaving the
//! screen, AND app exit.
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use image::GrayImage;

use super::client::{spawn_ffmpeg, SpawnParams, StreamChannels};
use crate::app::domain::{Activity, ActivityGuard};
use crate::ui::image::CameraImage;

/// Pixel dimensions ffmpeg scales each frame to (the renderer resizes to the
/// cell area, so this is decoupled from the terminal size — keep it modest).
#[derive(Clone, Copy)]
pub struct FrameSize {
    pub w: u32,
    pub h: u32,
}

impl FrameSize {
    // 960×540 (16:9) gives the octant downscaler real detail to supersample
    // from even when the CRT window is enlarged — at smaller window sizes the
    // extra source pixels just antialias the cell split a little cleaner. Still
    // light over a LAN (~520KB/frame gray; ~6MB/s at the default 12fps).
    pub const DEFAULT: FrameSize = FrameSize { w: 960, h: 540 };
    pub fn bytes(self) -> usize {
        (self.w * self.h) as usize
    }
}

pub const DEFAULT_FPS: u32 = 12;

/// Reconnect tuning. Attempts are capped so a dead camera doesn't loop forever.
pub const MAX_RECONNECT_ATTEMPTS: u32 = 6;
const BACKOFF_BASE_MS: u64 = 500;
const BACKOFF_CAP_MS: u64 = 8_000;
/// Consecutive stalled ticks (50ms each) on a *Live* feed before we treat it as
/// frozen and reconnect — ~6s, comfortably past the view's "NO SIGNAL" mark
/// (40 ticks ≈ 2s) so the operator sees NO SIGNAL first. This is how a frozen /
/// half-open socket is caught: no ffmpeg socket-timeout flag is used (those are
/// version-specific and unreliable on RTSP), so when frames stop arriving the
/// supervisor kills the stuck ffmpeg and respawns it.
pub const STALL_RECONNECT: u32 = 120;

/// Backoff before reconnect attempt `attempt` (0-based): `min(base·2^n, cap)`.
/// Pure + unit-tested. 0→500ms, 1→1s, 2→2s, 3→4s, 4→8s, then capped.
pub fn backoff_delay(attempt: u32) -> Duration {
    let ms = BACKOFF_BASE_MS
        .saturating_mul(1u64 << attempt.min(20))
        .min(BACKOFF_CAP_MS);
    Duration::from_millis(ms)
}

/// What the reader thread reports when ffmpeg stops, with severity baked in so
/// the supervisor knows whether retrying is worthwhile.
pub enum StreamEnd {
    /// Network / timeout / refused / EOF — worth retrying with backoff.
    Transient(String),
    /// Auth / permission — retrying is pointless; surface the reason and stop.
    Fatal(String),
}

impl StreamEnd {
    /// Classify from ffmpeg's last stderr line. Pure + unit-tested. Kept tight
    /// (auth-specific tokens only) so legitimate transient drops still retry.
    pub fn classify(stderr: &str) -> StreamEnd {
        let s = stderr.to_ascii_lowercase();
        let fatal = s.contains("401")
            || s.contains("unauthorized")
            || s.contains("403")
            || s.contains("method describe failed")
            || s.contains("authentication");
        let reason = if stderr.trim().is_empty() {
            "stream ended".to_string()
        } else {
            stderr.to_string()
        };
        if fatal {
            StreamEnd::Fatal(reason)
        } else {
            StreamEnd::Transient(reason)
        }
    }
}

/// Where a live feed is in its lifecycle.
pub enum StreamState {
    Connecting,
    Live,
    Reconnecting { attempt: u32 },
    /// Terminal: a fatal error, or reconnect attempts exhausted.
    Failed(String),
}

/// A running RTSP playback. Owns the ffmpeg child + the frame channel, and the
/// parameters needed to respawn it on a transient drop. Killing the child on
/// `Drop` closes the pipe so the reader thread's blocking read returns and the
/// thread exits cleanly.
pub struct LiveStream {
    child: Child,
    frames_rx: Receiver<GrayImage>,
    ended_rx: Receiver<StreamEnd>,
    /// The device this stream belongs to (so the detail view shows it only for
    /// the matching device).
    pub device_ip: String,
    pub latest: Option<CameraImage>,
    pub state: StreamState,
    pub stalled_ticks: u32,
    /// Everything needed to respawn ffmpeg on reconnect.
    spawn: SpawnParams,
    /// Reconnect attempts made so far (reset to 0 on a fresh frame).
    attempt: u32,
    /// When the next respawn is due. `Some` only while a retry is pending.
    next_retry_at: Option<Instant>,
    /// Held until the first frame arrives, then dropped — clears the top-bar
    /// "PROCESSING / CONNECTING" indicator. Re-acquired on each reconnect.
    _connecting: Option<ActivityGuard>,
}

impl LiveStream {
    /// Construct from `client::start` — keeps the field layout private.
    pub(super) fn new(
        ch: StreamChannels,
        spawn: SpawnParams,
        device_ip: String,
        connecting: ActivityGuard,
    ) -> Self {
        LiveStream {
            child: ch.child,
            frames_rx: ch.frames_rx,
            ended_rx: ch.ended_rx,
            device_ip,
            latest: None,
            state: StreamState::Connecting,
            stalled_ticks: 0,
            spawn,
            attempt: 0,
            next_retry_at: None,
            _connecting: Some(connecting),
        }
    }

    /// Drain queued frames (keeping ONLY the latest), flip Connecting→Live on
    /// the first frame, track stalls, and react to end events — scheduling a
    /// reconnect for transient drops or going terminal on a fatal one. Call
    /// once per tick with the loop's `now`.
    pub fn drain(&mut self, now: Instant) {
        // Keep only the newest queued frame, then wrap it ONCE — the
        // resize+dither cache is per-frame, so wrapping every drained frame
        // would build grids we immediately discard.
        let mut newest = None;
        while let Ok(img) = self.frames_rx.try_recv() {
            newest = Some(img);
        }
        let got = newest.is_some();
        if let Some(img) = newest {
            self.latest = Some(CameraImage::new(img));
        }
        if got {
            self.stalled_ticks = 0;
            self.attempt = 0; // a real frame proves the link is healthy again
            self.next_retry_at = None;
            self._connecting = None; // CONNECTING clears on first frame
            if !matches!(self.state, StreamState::Live) {
                self.state = StreamState::Live;
            }
        } else {
            self.stalled_ticks = self.stalled_ticks.saturating_add(1);
        }

        // The reader thread reported a hard end.
        if let Ok(end) = self.ended_rx.try_recv() {
            match end {
                StreamEnd::Fatal(reason) => {
                    self.state = StreamState::Failed(reason);
                    self.next_retry_at = None;
                    self._connecting = None;
                }
                StreamEnd::Transient(reason) => self.schedule_retry(now, reason),
            }
        }

        // A frozen feed (e.g. half-open TCP that never EOFs). Only fires while
        // Live, after the cosmetic NO-SIGNAL window has elapsed.
        if matches!(self.state, StreamState::Live) && self.stalled_ticks > STALL_RECONNECT {
            self.schedule_retry(now, "frozen feed".into());
        }
    }

    /// Schedule a backoff'd reconnect, or go terminal once attempts are spent.
    fn schedule_retry(&mut self, now: Instant, _reason: String) {
        if self.attempt >= MAX_RECONNECT_ATTEMPTS {
            self.state = StreamState::Failed(format!(
                "reconnect failed after {MAX_RECONNECT_ATTEMPTS} attempts"
            ));
            self.next_retry_at = None;
            self._connecting = None;
            return;
        }
        self.next_retry_at = Some(now + backoff_delay(self.attempt));
        self.state = StreamState::Reconnecting { attempt: self.attempt + 1 };
        self.stalled_ticks = 0;
    }

    /// Perform a due respawn. Call once per tick *after* `drain`. Needs the
    /// shared `Activity` to re-light the CONNECTING indicator for the new try.
    pub fn supervise(&mut self, now: Instant, activity: &Activity) {
        let Some(due) = self.next_retry_at else { return };
        if now < due {
            return;
        }
        self.next_retry_at = None;
        // Reap the old (dead or frozen) child before respawning.
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.attempt += 1; // we are now making attempt #self.attempt

        match spawn_ffmpeg(&self.spawn) {
            Ok(ch) => {
                self.child = ch.child;
                self.frames_rx = ch.frames_rx;
                self.ended_rx = ch.ended_rx;
                self._connecting = Some(activity.begin());
                // state stays Reconnecting{attempt}; drain() flips to Live and
                // resets `attempt` on the first frame from the new process.
            }
            Err(e) => self.schedule_retry(now, e), // couldn't spawn — try again
        }
    }
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        let _ = self.child.kill(); // unblocks the reader's read → thread exits
        let _ = self.child.wait(); // reap so no zombie ffmpeg remains
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_delay(0), Duration::from_millis(500));
        assert_eq!(backoff_delay(1), Duration::from_millis(1_000));
        assert_eq!(backoff_delay(2), Duration::from_millis(2_000));
        assert_eq!(backoff_delay(4), Duration::from_millis(8_000));
        assert_eq!(backoff_delay(10), Duration::from_millis(8_000)); // capped
    }

    #[test]
    fn classify_flags_auth_as_fatal() {
        assert!(matches!(StreamEnd::classify("401 Unauthorized"), StreamEnd::Fatal(_)));
        assert!(matches!(
            StreamEnd::classify("method DESCRIBE failed: 401 Unauthorized"),
            StreamEnd::Fatal(_)
        ));
        assert!(matches!(StreamEnd::classify("UNAUTHORIZED"), StreamEnd::Fatal(_)));
    }

    #[test]
    fn classify_treats_network_and_eof_as_transient() {
        assert!(matches!(StreamEnd::classify("Connection refused"), StreamEnd::Transient(_)));
        assert!(matches!(StreamEnd::classify("Connection timed out"), StreamEnd::Transient(_)));
        assert!(matches!(StreamEnd::classify(""), StreamEnd::Transient(_)));
    }
}
