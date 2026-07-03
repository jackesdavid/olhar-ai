//! Multi-object tracker (tracking-by-detection, ByteTrack-flavoured) — the algorithmic heart of the
//! on-edge object-tracking feature (card: edge tracking). Pure Rust, **no I/O and no OpenCV**: the
//! caller feeds per-frame detections (from the existing YOLOX detector, [`crate::agent::detector`])
//! and gets back stable, uuid-identified tracks + lifecycle events (Entered / Exited).
//!
//! Design (see the plan): a constant-velocity **Kalman** predictor per track (run independently on
//! the x and y centre axes — the model is axis-separable, so two 2-state filters replace one 4-state
//! matrix filter and need no linalg dep) plus **two-stage IoU association** (ByteTrack): high-confidence
//! detections match first, then low-confidence ones recover tracks through brief occlusion. Matching
//! is greedy-by-IoU (optimal Hungarian is a later refinement — greedy is ample for typical scenes).
//!
//! Identity: each track owns a `uuid` for its whole life ⇒ "same object, no duplication" within a
//! session; a `Lost` track is kept for `max_age` cycles so a re-appearing object re-attaches to its
//! uuid instead of spawning a new one. Cross-session re-ID (appearance embedding) is a later phase.
use uuid::Uuid;

use crate::agent::detector::Detection;

/// Tuning for [`MultiTracker`]. Defaults are ByteTrack-ish and work for person/vehicle tracking on a
/// skip-frame cadence.
#[derive(Debug, Clone, Copy)]
pub struct TrackerConfig {
    /// Detections at/above this score drive stage-1 association + can start new tracks.
    pub high_thresh: f32,
    /// Detections in `[low_thresh, high_thresh)` only *recover* existing tracks (stage 2), never start one.
    pub low_thresh: f32,
    /// Minimum IoU for a detection↔track match.
    pub iou_thresh: f32,
    /// Consecutive hits before a `Tentative` track is `Confirmed` (and an Entered event fires).
    pub min_hits: u32,
    /// Cycles a track may go unmatched before it's removed (an Exited event fires if it was confirmed).
    pub max_age: u32,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        TrackerConfig { high_thresh: 0.5, low_thresh: 0.1, iou_thresh: 0.3, min_hits: 3, max_age: 30 }
    }
}

/// A 2-state constant-velocity Kalman filter (position + velocity) for one axis. `F = [[1,1],[0,1]]`,
/// `H = [1,0]`. Cheap scalar math — no matrix library.
#[derive(Debug, Clone, Copy)]
struct Kalman1D {
    pos: f32,
    vel: f32,
    p: [[f32; 2]; 2], // covariance
}

impl Kalman1D {
    fn new(pos: f32) -> Self {
        // Start confident on position (from the first detection), very unsure on velocity.
        Kalman1D { pos, vel: 0.0, p: [[1.0, 0.0], [0.0, 1000.0]] }
    }
    fn predict(&mut self) {
        const Q_POS: f32 = 1.0;
        const Q_VEL: f32 = 1.0;
        self.pos += self.vel;
        let [[p00, p01], [p10, p11]] = self.p;
        // P = F P Fᵀ + Q, with F = [[1,1],[0,1]]
        self.p[0][0] = p00 + p10 + p01 + p11 + Q_POS;
        self.p[0][1] = p01 + p11;
        self.p[1][0] = p10 + p11;
        self.p[1][1] = p11 + Q_VEL;
    }
    fn update(&mut self, z: f32) {
        const R: f32 = 1.0; // measurement noise
        let s = self.p[0][0] + R;
        let k0 = self.p[0][0] / s;
        let k1 = self.p[1][0] / s;
        let y = z - self.pos;
        self.pos += k0 * y;
        self.vel += k1 * y;
        let [[p00, p01], _] = self.p;
        // P = (I - K H) P, H = [1,0]
        self.p[0][0] = (1.0 - k0) * p00;
        self.p[0][1] = (1.0 - k0) * p01;
        self.p[1][0] -= k1 * p00;
        self.p[1][1] -= k1 * p01;
    }
}

/// Lifecycle state of a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackState {
    /// Seen, but not yet enough hits to be trusted (no Entered emitted yet).
    Tentative,
    /// Trusted + currently matched to a detection.
    Confirmed,
    /// Was confirmed, currently unmatched (kept `max_age` cycles for occlusion re-match).
    Lost,
}

/// One tracked object. `bbox` is the current best estimate in source pixels `[x1,y1,x2,y2]`.
#[derive(Debug, Clone)]
pub struct Track {
    pub uuid: Uuid,
    pub class: u32,
    pub state: TrackState,
    kx: Kalman1D,
    ky: Kalman1D,
    w: f32,
    h: f32,
    /// Consecutive matched cycles (drives Tentative→Confirmed).
    pub hits: u32,
    /// Cycles since last matched (drives Lost→removed).
    pub time_since_update: u32,
    first_ts_ms: u64,
    last_ts_ms: u64,
}

impl Track {
    fn new(det: &Detection, ts_ms: u64) -> Self {
        let (cx, cy, w, h) = xyxy_to_cxcywh(det.xyxy);
        Track {
            uuid: Uuid::new_v4(),
            class: det.class,
            state: TrackState::Tentative,
            kx: Kalman1D::new(cx),
            ky: Kalman1D::new(cy),
            w,
            h,
            hits: 1,
            time_since_update: 0,
            first_ts_ms: ts_ms,
            last_ts_ms: ts_ms,
        }
    }
    /// Current bbox estimate `[x1,y1,x2,y2]` from the filtered centre + smoothed size.
    pub fn bbox(&self) -> [f32; 4] {
        cxcywh_to_xyxy(self.kx.pos, self.ky.pos, self.w, self.h)
    }
    /// Filtered centre point (for line-crossing + map placement).
    pub fn centre(&self) -> (f32, f32) {
        (self.kx.pos, self.ky.pos)
    }
    pub fn dwell_ms(&self) -> u64 {
        self.last_ts_ms.saturating_sub(self.first_ts_ms)
    }
    fn predict(&mut self) {
        self.kx.predict();
        self.ky.predict();
    }
    fn apply(&mut self, det: &Detection, ts_ms: u64) {
        let (cx, cy, w, h) = xyxy_to_cxcywh(det.xyxy);
        self.kx.update(cx);
        self.ky.update(cy);
        // Size low-passed (EMA) — the Kalman only models the centre.
        self.w = 0.6 * self.w + 0.4 * w;
        self.h = 0.6 * self.h + 0.4 * h;
        self.hits += 1;
        self.time_since_update = 0;
        self.last_ts_ms = ts_ms;
        self.class = det.class;
    }
}

/// Lifecycle events emitted by [`MultiTracker::update`]. The edge translates these into
/// `object_entered` / `object_exited` cloud events.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackEvent {
    /// A track became Confirmed for the first time.
    Entered { uuid: Uuid, class: u32, bbox: [f32; 4] },
    /// A confirmed track was removed after `max_age` unmatched cycles.
    Exited { uuid: Uuid, class: u32, bbox: [f32; 4], dwell_ms: u64 },
}

/// The tracker: hold and evolve the set of [`Track`]s across detection cycles.
#[derive(Debug)]
pub struct MultiTracker {
    cfg: TrackerConfig,
    tracks: Vec<Track>,
    /// Whether a track has already emitted Entered (so a Lost→Confirmed re-match doesn't re-emit).
    confirmed_emitted: Vec<bool>,
}

impl MultiTracker {
    pub fn new(cfg: TrackerConfig) -> Self {
        MultiTracker { cfg, tracks: Vec::new(), confirmed_emitted: Vec::new() }
    }

    /// The confirmed tracks currently on screen (for count + map position emit).
    pub fn confirmed(&self) -> impl Iterator<Item = &Track> {
        self.tracks.iter().filter(|t| t.state == TrackState::Confirmed)
    }
    /// Count of confirmed tracks (the "how many in the viewport" number).
    pub fn count(&self) -> usize {
        self.confirmed().count()
    }

    /// Advance one detection cycle: predict every track, associate `dets` (ByteTrack two-stage),
    /// update matched tracks, age/remove the rest, spawn new tracks. `ts_ms` is this frame's wall
    /// time (for dwell). Returns the lifecycle events that occurred this cycle.
    pub fn update(&mut self, dets: &[Detection], ts_ms: u64) -> Vec<TrackEvent> {
        let mut events = Vec::new();
        for t in &mut self.tracks {
            t.predict();
            t.time_since_update += 1;
        }

        // Split detections by confidence (ByteTrack): high drive stage 1 + spawn; low only recover.
        let mut high: Vec<usize> = Vec::new();
        let mut low: Vec<usize> = Vec::new();
        for (i, d) in dets.iter().enumerate() {
            if d.score >= self.cfg.high_thresh {
                high.push(i);
            } else if d.score >= self.cfg.low_thresh {
                low.push(i);
            }
        }

        let mut track_matched = vec![false; self.tracks.len()];

        // Stage 1: high-confidence detections vs. all tracks.
        let m1 = self.associate(&high, dets, &track_matched);
        let mut det_matched_high = vec![false; high.len()];
        for (hi, ti) in &m1 {
            self.tracks[*ti].apply(&dets[high[*hi]], ts_ms);
            track_matched[*ti] = true;
            det_matched_high[*hi] = true;
        }

        // Stage 2: leftover (still-unmatched) tracks vs. low-confidence detections.
        let m2 = self.associate(&low, dets, &track_matched);
        for (li, ti) in &m2 {
            self.tracks[*ti].apply(&dets[low[*li]], ts_ms);
            track_matched[*ti] = true;
        }

        // Promote tentative→confirmed (emit Entered once) + revive Lost→Confirmed on re-match.
        for (ti, t) in self.tracks.iter_mut().enumerate() {
            if track_matched[ti] {
                if t.hits >= self.cfg.min_hits && t.state != TrackState::Confirmed {
                    t.state = TrackState::Confirmed;
                    if !self.confirmed_emitted[ti] {
                        self.confirmed_emitted[ti] = true;
                        events.push(TrackEvent::Entered { uuid: t.uuid, class: t.class, bbox: t.bbox() });
                    }
                }
            } else if t.state == TrackState::Confirmed {
                t.state = TrackState::Lost;
            }
        }

        // Remove stale tracks (emit Exited for ones the user ever saw).
        let max_age = self.cfg.max_age;
        let mut i = 0;
        while i < self.tracks.len() {
            if self.tracks[i].time_since_update > max_age {
                let t = self.tracks.remove(i);
                let emitted = self.confirmed_emitted.remove(i);
                if emitted {
                    events.push(TrackEvent::Exited {
                        uuid: t.uuid,
                        class: t.class,
                        bbox: t.bbox(),
                        dwell_ms: t.dwell_ms(),
                    });
                }
            } else {
                i += 1;
            }
        }

        // Spawn new tentative tracks from unmatched high-confidence detections.
        for (hi, matched) in det_matched_high.iter().enumerate() {
            if !matched {
                self.tracks.push(Track::new(&dets[high[hi]], ts_ms));
                self.confirmed_emitted.push(false);
            }
        }

        events
    }

    /// Greedy IoU association: pair each detection in `subset` (indices into `dets`) with the best
    /// unmatched track above `iou_thresh`, highest IoU first. Returns `(subset_idx, track_idx)` pairs.
    fn associate(&self, subset: &[usize], dets: &[Detection], track_matched: &[bool]) -> Vec<(usize, usize)> {
        let mut candidates: Vec<(f32, usize, usize)> = Vec::new(); // (iou, subset_idx, track_idx)
        for (si, di) in subset.iter().enumerate() {
            let dbox = dets[*di].xyxy;
            for (ti, t) in self.tracks.iter().enumerate() {
                if track_matched[ti] {
                    continue;
                }
                let iou = iou(dbox, t.bbox());
                if iou >= self.cfg.iou_thresh {
                    candidates.push((iou, si, ti));
                }
            }
        }
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut used_det = vec![false; subset.len()];
        let mut used_track = vec![false; self.tracks.len()];
        let mut out = Vec::new();
        for (_, si, ti) in candidates {
            if !used_det[si] && !used_track[ti] {
                used_det[si] = true;
                used_track[ti] = true;
                out.push((si, ti));
            }
        }
        out
    }
}

fn xyxy_to_cxcywh(b: [f32; 4]) -> (f32, f32, f32, f32) {
    let w = (b[2] - b[0]).max(0.0);
    let h = (b[3] - b[1]).max(0.0);
    (b[0] + w / 2.0, b[1] + h / 2.0, w, h)
}

fn cxcywh_to_xyxy(cx: f32, cy: f32, w: f32, h: f32) -> [f32; 4] {
    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
}

/// Intersection-over-union of two `[x1,y1,x2,y2]` boxes.
pub fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    let iw = (x2 - x1).max(0.0);
    let ih = (y2 - y1).max(0.0);
    let inter = iw * ih;
    let area_a = ((a[2] - a[0]) * (a[3] - a[1])).max(0.0);
    let area_b = ((b[2] - b[0]) * (b[3] - b[1])).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(class: u32, score: f32, xyxy: [f32; 4]) -> Detection {
        Detection { class, score, xyxy }
    }
    fn cfg() -> TrackerConfig {
        // min_hits=3 → confirm on the 3rd frame; max_age=3 → removed after 3 unmatched cycles.
        TrackerConfig { high_thresh: 0.5, low_thresh: 0.1, iou_thresh: 0.2, min_hits: 3, max_age: 3 }
    }

    #[test]
    fn box_moving_left_to_right_keeps_one_uuid() {
        let mut mt = MultiTracker::new(cfg());
        let mut uuid = None;
        let mut entered = 0;
        for f in 0..10u64 {
            let x = 10.0 + f as f32 * 20.0;
            let evs = mt.update(&[det(0, 0.9, [x, 100.0, x + 40.0, 180.0])], f * 100);
            for e in &evs {
                if let TrackEvent::Entered { uuid: u, .. } = e {
                    entered += 1;
                    uuid = Some(*u);
                }
            }
        }
        assert_eq!(entered, 1, "exactly one Entered for one moving object");
        assert_eq!(mt.count(), 1);
        assert_eq!(mt.confirmed().next().unwrap().uuid, uuid.unwrap(), "same uuid throughout");
    }

    #[test]
    fn object_exits_after_max_age_with_dwell() {
        let mut mt = MultiTracker::new(cfg());
        // 4 frames present (confirms on frame 3), spanning 300ms.
        for f in 0..4u64 {
            mt.update(&[det(0, 0.9, [50.0, 50.0, 90.0, 130.0])], f * 100);
        }
        assert_eq!(mt.count(), 1);
        // Now empty frames — after max_age (3) unmatched cycles the track exits.
        let mut exited = None;
        for f in 4..9u64 {
            for e in mt.update(&[], f * 100) {
                if let TrackEvent::Exited { dwell_ms, .. } = e {
                    exited = Some(dwell_ms);
                }
            }
        }
        assert!(exited.is_some(), "an Exited event fired");
        assert_eq!(exited.unwrap(), 300, "dwell = last_seen - first_seen (frames 0..3 = 300ms)");
        assert_eq!(mt.count(), 0);
    }

    #[test]
    fn brief_occlusion_reattaches_same_uuid_no_second_entered() {
        let mut mt = MultiTracker::new(cfg());
        let mut uuid = None;
        let mut entered = 0;
        for f in 0..4u64 {
            let x = 10.0 + f as f32 * 15.0;
            for e in mt.update(&[det(0, 0.9, [x, 100.0, x + 40.0, 180.0])], f * 100) {
                if let TrackEvent::Entered { uuid: u, .. } = e {
                    entered += 1;
                    uuid = Some(u);
                }
            }
        }
        // Occluded for 2 frames (< max_age) → track goes Lost but is kept.
        mt.update(&[], 400);
        mt.update(&[], 500);
        // Reappears near the predicted position → should re-attach, NOT spawn a new uuid/Entered.
        let x = 10.0 + 6.0 * 15.0;
        for e in mt.update(&[det(0, 0.9, [x, 100.0, x + 40.0, 180.0])], 600) {
            if let TrackEvent::Entered { .. } = e {
                entered += 1;
            }
        }
        assert_eq!(entered, 1, "no second Entered after brief occlusion");
        assert_eq!(mt.count(), 1);
        assert_eq!(mt.confirmed().next().unwrap().uuid, uuid.unwrap());
    }

    #[test]
    fn two_objects_get_two_distinct_uuids() {
        let mut mt = MultiTracker::new(cfg());
        let mut uuids = std::collections::HashSet::new();
        for f in 0..5u64 {
            let a = 20.0 + f as f32 * 5.0;
            let b = 300.0 - f as f32 * 5.0;
            for e in mt.update(
                &[det(0, 0.9, [a, 100.0, a + 40.0, 180.0]), det(0, 0.9, [b, 100.0, b + 40.0, 180.0])],
                f * 100,
            ) {
                if let TrackEvent::Entered { uuid, .. } = e {
                    uuids.insert(uuid);
                }
            }
        }
        assert_eq!(uuids.len(), 2, "two objects → two uuids");
        assert_eq!(mt.count(), 2);
    }

    #[test]
    fn low_confidence_alone_never_confirms() {
        let mut mt = MultiTracker::new(cfg());
        let mut entered = 0;
        for f in 0..8u64 {
            // score below high_thresh → can't start a track (ByteTrack: low only recovers).
            for e in mt.update(&[det(0, 0.3, [50.0, 50.0, 90.0, 130.0])], f * 100) {
                if let TrackEvent::Entered { .. } = e {
                    entered += 1;
                }
            }
        }
        assert_eq!(entered, 0, "low-confidence-only detections never spawn a confirmed track");
        assert_eq!(mt.count(), 0);
    }
}
