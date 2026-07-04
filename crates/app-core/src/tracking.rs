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
    /// Centre at the end of the *previous* cycle (`None` for a track's first cycle) — the other
    /// endpoint of the movement segment [`MultiTracker::check_crossings`] tests against each [`Line`].
    prev_centre: Option<(f32, f32)>,
    /// Whether this track was last considered inside the configured [`Polygon`] zone (`None`
    /// before it's ever been evaluated) — [`MultiTracker::check_zone`] diffs against this to emit
    /// `ZoneEntered`/`ZoneExited` only on a transition, not every cycle it's inside/outside.
    zone_inside: Option<bool>,
    /// When this track was last considered to have entered the zone — for `ZoneExited`'s `dwell_ms`.
    zone_entered_ts_ms: Option<u64>,
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
            prev_centre: None,
            zone_inside: None,
            zone_entered_ts_ms: None,
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
    /// Whether this track is currently considered inside the job's zone (`None` if the job has no
    /// zone configured, or this track has never been evaluated against one yet) — for a live
    /// "state of the world" snapshot (`GetTrackedObjects`), not just the enter/exit transitions
    /// `check_zone` emits events for.
    pub fn zone_inside(&self) -> Option<bool> {
        self.zone_inside
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

/// Lifecycle events emitted by [`MultiTracker::update`] / [`MultiTracker::update_with_lines`] /
/// [`MultiTracker::update_full`]. The edge translates these into `object_entered` /
/// `object_exited` / `line_crossed` cloud events.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackEvent {
    /// A track became Confirmed for the first time (whole-frame — no zone configured).
    Entered { uuid: Uuid, class: u32, bbox: [f32; 4] },
    /// A confirmed track was removed after `max_age` unmatched cycles (whole-frame).
    Exited { uuid: Uuid, class: u32, bbox: [f32; 4], dwell_ms: u64 },
    /// A confirmed track's centre crossed a configured [`Line`] between this cycle and the last.
    Crossed { uuid: Uuid, class: u32, line: String, direction: Direction },
    /// A confirmed track's presence in a configured [`Polygon`] flipped outside → inside.
    ZoneEntered { uuid: Uuid, class: u32, zone: String },
    /// ... flipped inside → outside. `dwell_ms` is how long it was considered inside the zone.
    ZoneExited { uuid: Uuid, class: u32, zone: String, dwell_ms: u64 },

    // -- Diagnostic-only below: internal algorithm state transitions, never a domain lifecycle
    // signal on their own (no `object_entered`/`object_exited`/`line_crossed` follows from these
    // alone) — the edge logs them when a job's `debug` flag is on and never relays them to
    // control. Kept in the same enum (rather than a separate diagnostics channel) so every
    // caller gets them for free from the same `update()`/`update_full()` call.
    /// A `Confirmed` track went unmatched this cycle → `Lost` (kept for up to `max_age` cycles).
    Lost { uuid: Uuid, class: u32 },
    /// A `Lost` track re-matched a detection before ageing out → `Confirmed` again, same uuid.
    /// `cycles_lost` is how many consecutive cycles it had gone unmatched.
    Revived { uuid: Uuid, class: u32, cycles_lost: u32 },
    /// A track was dropped after exceeding `max_age` unmatched cycles. `was_confirmed` mirrors
    /// whether an `Exited` accompanies this (it does iff the track had ever been confirmed).
    Removed { uuid: Uuid, class: u32, cycles_survived: u32, was_confirmed: bool },
}

/// A user-drawn virtual line for crossing detection, in **normalized** frame coordinates
/// (`0.0..1.0` on both axes, origin top-left) — resolution-independent, so the same config
/// works whatever size frame the camera happens to produce. Scaled to source pixels at test
/// time via the frame's `(width, height)` (see [`MultiTracker::update_with_lines`]).
///
/// `Serialize`/`Deserialize` (`p1`/`p2` as `[x,y]` JSON arrays) so it rides the edge's
/// `TrackingConfig` (control-pulled `tracking_config` jsonb) verbatim — no separate wire DTO.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Line {
    /// Which camera this line applies to (id/ip/name — the edge resolves it against its device
    /// list the same way every other camera-config field does). `MultiTracker` itself doesn't
    /// use this field — it's a pure per-camera tracker instance already — the edge filters
    /// `tracking_config.lines` down to a camera's own lines before passing them in.
    #[serde(default)]
    pub camera: String,
    pub name: String,
    pub p1: (f32, f32),
    pub p2: (f32, f32),
}

/// Which way a track crossed a [`Line`], relative to its `p1 → p2` direction: `Forward` is the
/// side the line's direction "turns left" away from ((p1→p2) × (p1→point) goes negative→positive);
/// `Backward` is the reverse. Arbitrary but consistent — pick the line's endpoint order to mean
/// whatever "in"/"out" is for the use case (e.g. p1=door frame, p2=inside the room).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::Forward => "forward",
            Direction::Backward => "backward",
        }
    }
}

/// A closed "region of interest" polygon for zone-scoped enter/exit, in **normalized** frame
/// coordinates (same `0.0..1.0` convention as [`Line`]) — an alternative to whole-frame
/// Entered/Exited: when a job configures a zone, entered/exited means crossing into/out of THIS
/// shape specifically, not just being tracked anywhere in the frame. Needs 3+ points to be
/// meaningful (fewer are simply never entered/exited — see [`MultiTracker::check_zone`]).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Polygon {
    /// Which camera this zone applies to — same convention/caveat as [`Line::camera`].
    #[serde(default)]
    pub camera: String,
    pub name: String,
    pub points: Vec<(f32, f32)>,
}

/// How a track's presence in a [`Polygon`] is judged. Defaults to `Intersects` — the more
/// forgiving option — so a config that sets a `zone` but omits `zone_trigger` still does
/// something sensible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneTrigger {
    /// The track's bbox overlaps the polygon at all — looser, catches a partial/edge entry.
    #[default]
    Intersects,
    /// The track's centre point is inside the polygon — stricter, the object's middle must be
    /// substantially inside, not just brushing the boundary.
    CentroidInside,
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
    /// `(tentative, confirmed, lost)` track counts — diagnostic only, for a debug-mode caller to
    /// log the algorithm's internal state breakdown (see `sentinel-edge`'s `run_track`).
    pub fn state_counts(&self) -> (usize, usize, usize) {
        let mut c = (0, 0, 0);
        for t in &self.tracks {
            match t.state {
                TrackState::Tentative => c.0 += 1,
                TrackState::Confirmed => c.1 += 1,
                TrackState::Lost => c.2 += 1,
            }
        }
        c
    }

    /// Advance one detection cycle: predict every track, associate `dets` (ByteTrack two-stage),
    /// update matched tracks, age/remove the rest, spawn new tracks. `ts_ms` is this frame's wall
    /// time (for dwell). Returns the lifecycle events that occurred this cycle.
    pub fn update(&mut self, dets: &[Detection], ts_ms: u64) -> Vec<TrackEvent> {
        // Snapshot each track's resting centre from the END of the last cycle — keyed by uuid (not
        // index) so it survives this cycle's removals/spawns unambiguously. Restored onto the
        // surviving tracks as `prev_centre` below, for `check_crossings` to diff against.
        let before: std::collections::HashMap<Uuid, (f32, f32)> =
            self.tracks.iter().map(|t| (t.uuid, t.centre())).collect();

        let mut events = Vec::new();
        for t in &mut self.tracks {
            t.predict();
            t.time_since_update += 1;
        }
        // Diagnostic-only snapshot: how long each track had been unmatched going into this
        // cycle, keyed by index — `apply()` below resets a matched track's `time_since_update`
        // to 0, so this is the only place a `Revived` event can still report it.
        let unmatched_before: Vec<u32> = self.tracks.iter().map(|t| t.time_since_update).collect();

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
                let was_lost = t.state == TrackState::Lost;
                if t.hits >= self.cfg.min_hits && t.state != TrackState::Confirmed {
                    t.state = TrackState::Confirmed;
                    if !self.confirmed_emitted[ti] {
                        self.confirmed_emitted[ti] = true;
                        events.push(TrackEvent::Entered { uuid: t.uuid, class: t.class, bbox: t.bbox() });
                    } else if was_lost {
                        events.push(TrackEvent::Revived { uuid: t.uuid, class: t.class, cycles_lost: unmatched_before[ti] });
                    }
                }
            } else if t.state == TrackState::Confirmed {
                t.state = TrackState::Lost;
                events.push(TrackEvent::Lost { uuid: t.uuid, class: t.class });
            }
        }

        // Remove stale tracks (emit Exited for ones the user ever saw).
        let max_age = self.cfg.max_age;
        let mut i = 0;
        while i < self.tracks.len() {
            if self.tracks[i].time_since_update > max_age {
                let t = self.tracks.remove(i);
                let emitted = self.confirmed_emitted.remove(i);
                events.push(TrackEvent::Removed {
                    uuid: t.uuid,
                    class: t.class,
                    cycles_survived: t.time_since_update,
                    was_confirmed: emitted,
                });
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

        // Freshly-spawned tracks aren't in `before` → `prev_centre` stays `None` (no crossing test
        // fires on a track's first cycle; there's no "previous frame" segment to test yet).
        for t in &mut self.tracks {
            t.prev_centre = before.get(&t.uuid).copied();
        }

        events
    }

    /// Like [`Self::update`], plus line-crossing: after the normal tracking cycle, test every
    /// **confirmed** track's movement segment (`prev_centre → centre`, this cycle) against every
    /// `Line` (scaled from normalized to `frame_wh` source pixels) and append a `Crossed` event per
    /// hit. `frame_wh` is the JPEG's `(width, height)` in pixels this cycle's detections came from.
    pub fn update_with_lines(
        &mut self,
        dets: &[Detection],
        ts_ms: u64,
        frame_wh: (f32, f32),
        lines: &[Line],
    ) -> Vec<TrackEvent> {
        let mut events = self.update(dets, ts_ms);
        events.extend(self.check_crossings(frame_wh, lines));
        events
    }

    /// Segment-intersect each confirmed track's `(prev_centre, centre)` movement this cycle against
    /// each line (scaled to pixels via `frame_wh`); a track with no `prev_centre` yet (its first
    /// cycle) is skipped — there's nothing to test against.
    fn check_crossings(&self, frame_wh: (f32, f32), lines: &[Line]) -> Vec<TrackEvent> {
        let mut events = Vec::new();
        if lines.is_empty() {
            return events;
        }
        let (fw, fh) = frame_wh;
        for t in self.tracks.iter().filter(|t| t.state == TrackState::Confirmed) {
            let Some(prev) = t.prev_centre else { continue };
            let cur = t.centre();
            for line in lines {
                let p1 = (line.p1.0 * fw, line.p1.1 * fh);
                let p2 = (line.p2.0 * fw, line.p2.1 * fh);
                if let Some(direction) = crossing_direction(p1, p2, prev, cur) {
                    events.push(TrackEvent::Crossed {
                        uuid: t.uuid,
                        class: t.class,
                        line: line.name.clone(),
                        direction,
                    });
                }
            }
        }
        events
    }

    /// Like [`Self::update`], plus line-crossing (when `lines` is non-empty) AND/OR zone-scoped
    /// enter/exit (when `zone` is `Some`) — the general form both [`Self::update_with_lines`] and
    /// a bare [`Self::update`] are special cases of. `frame_wh` is the JPEG's `(width, height)` in
    /// pixels this cycle's detections came from (only actually used if `lines`/`zone` need it).
    pub fn update_full(
        &mut self,
        dets: &[Detection],
        ts_ms: u64,
        frame_wh: (f32, f32),
        lines: &[Line],
        zone: Option<(&Polygon, ZoneTrigger)>,
    ) -> Vec<TrackEvent> {
        let mut events = self.update(dets, ts_ms);
        if !lines.is_empty() {
            events.extend(self.check_crossings(frame_wh, lines));
        }
        if let Some((z, trigger)) = zone {
            events.extend(self.check_zone(frame_wh, z, trigger, ts_ms));
        }
        events
    }

    /// Diff every **confirmed** track's presence in `zone` (scaled to pixels via `frame_wh`)
    /// against its last-known state, emitting `ZoneEntered`/`ZoneExited` only on a transition —
    /// a track that's simply *still* inside/outside produces nothing every cycle. `zone` needs
    /// 3+ points to be meaningful; fewer and every track is (silently) considered never inside.
    fn check_zone(&mut self, frame_wh: (f32, f32), zone: &Polygon, trigger: ZoneTrigger, ts_ms: u64) -> Vec<TrackEvent> {
        let mut events = Vec::new();
        if zone.points.len() < 3 {
            return events;
        }
        let (fw, fh) = frame_wh;
        let poly: Vec<(f32, f32)> = zone.points.iter().map(|&(x, y)| (x * fw, y * fh)).collect();
        for t in self.tracks.iter_mut().filter(|t| t.state == TrackState::Confirmed) {
            let inside = match trigger {
                ZoneTrigger::CentroidInside => point_in_polygon(t.centre(), &poly),
                ZoneTrigger::Intersects => bbox_intersects_polygon(t.bbox(), &poly),
            };
            match (t.zone_inside, inside) {
                (None, true) | (Some(false), true) => {
                    t.zone_inside = Some(true);
                    t.zone_entered_ts_ms = Some(ts_ms);
                    events.push(TrackEvent::ZoneEntered { uuid: t.uuid, class: t.class, zone: zone.name.clone() });
                }
                (Some(true), false) => {
                    let dwell_ms = t.zone_entered_ts_ms.map(|s| ts_ms.saturating_sub(s)).unwrap_or(0);
                    t.zone_inside = Some(false);
                    events.push(TrackEvent::ZoneExited { uuid: t.uuid, class: t.class, zone: zone.name.clone(), dwell_ms });
                }
                (None, false) => t.zone_inside = Some(false),
                (Some(true), true) | (Some(false), false) => {} // no change — still inside/outside
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

/// Signed area ×2 of the triangle `(a,b,c)` — positive when `c` is left of ray `a→b`, negative when
/// right, zero when collinear. The building block for segment intersection.
fn orient(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Whether segment `a→b` properly intersects segment `c→d` (not infinite-line) — the shared
/// building block for line-crossing and bbox/polygon overlap.
fn segments_intersect(a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32)) -> bool {
    let d1 = orient(c, d, a);
    let d2 = orient(c, d, b);
    let d3 = orient(a, b, c);
    let d4 = orient(a, b, d);
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// Whether the movement segment `prev→cur` crosses the line segment `line_p1→line_p2` — proper
/// segment intersection (not infinite-line), so a track passing near but outside the drawn line's
/// extent doesn't false-positive. Returns the crossing [`Direction`] (relative to `line_p1→line_p2`)
/// when it does, `None` otherwise (no crossing, or collinear/degenerate — never emit on a coin-flip).
fn crossing_direction(
    line_p1: (f32, f32),
    line_p2: (f32, f32),
    prev: (f32, f32),
    cur: (f32, f32),
) -> Option<Direction> {
    if !segments_intersect(line_p1, line_p2, prev, cur) {
        return None;
    }
    // prev right-of line_p1→line_p2 (< 0) crossing to left-of it (cur > 0): crossed "Forward".
    let d1 = orient(line_p1, line_p2, prev);
    Some(if d1 < 0.0 { Direction::Forward } else { Direction::Backward })
}

/// Ray-casting point-in-polygon test (even-odd rule). `poly` must have 3+ points (callers already
/// guard this); a point exactly on an edge may go either way — an inherent tie-break, not a bug.
fn point_in_polygon(p: (f32, f32), poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Whether an axis-aligned `bbox` (`[x1,y1,x2,y2]`) overlaps `poly` at all: any polygon vertex
/// inside the bbox, any bbox corner inside the polygon, or any polygon edge crossing any bbox
/// edge — covers full containment either way plus a partial/edge overlap. `poly` needs 3+ points.
fn bbox_intersects_polygon(bbox: [f32; 4], poly: &[(f32, f32)]) -> bool {
    let [x1, y1, x2, y2] = bbox;
    if poly.iter().any(|&(x, y)| x >= x1 && x <= x2 && y >= y1 && y <= y2) {
        return true;
    }
    let corners = [(x1, y1), (x2, y1), (x2, y2), (x1, y2)];
    if corners.iter().any(|&c| point_in_polygon(c, poly)) {
        return true;
    }
    let bbox_edges = [(corners[0], corners[1]), (corners[1], corners[2]), (corners[2], corners[3]), (corners[3], corners[0])];
    let n = poly.len();
    (0..n).any(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        bbox_edges.iter().any(|&(c, d)| segments_intersect(a, b, c, d))
    })
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

    // ---- line-crossing (P3) -----------------------------------------------------------------

    fn crossed_events(evs: &[TrackEvent]) -> Vec<(&str, Direction)> {
        evs.iter()
            .filter_map(|e| match e {
                TrackEvent::Crossed { line, direction, .. } => Some((line.as_str(), *direction)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn crossing_a_line_emits_crossed_with_direction() {
        let mut mt = MultiTracker::new(cfg());
        let gate = Line { camera: "cam".to_string(), name: "gate".to_string(), p1: (0.5, 0.0), p2: (0.5, 1.0) }; // pixel x=50
        let mut crossed = Vec::new();
        for f in 0..8u64 {
            let x = 10.0 + f as f32 * 13.0; // centre x: 30,43,56,69,82,95,108,121
            let evs = mt.update_with_lines(
                &[det(0, 0.9, [x, 40.0, x + 40.0, 80.0])],
                f * 100,
                (100.0, 100.0),
                std::slice::from_ref(&gate),
            );
            crossed.extend(crossed_events(&evs).into_iter().map(|(l, d)| (l.to_string(), d)));
        }
        assert_eq!(crossed.len(), 1, "exactly one crossing of a single line by a single pass");
        assert_eq!(crossed[0].0, "gate");
        assert_eq!(crossed[0].1, Direction::Backward, "centre 43→56 vs line p1=(50,0)→p2=(50,100)");
    }

    #[test]
    fn movement_confined_to_one_side_never_crosses() {
        let mut mt = MultiTracker::new(cfg());
        let gate = Line { camera: "cam".to_string(), name: "gate".to_string(), p1: (0.5, 0.0), p2: (0.5, 1.0) };
        let mut crossed = Vec::new();
        for f in 0..8u64 {
            let x = 10.0 + f as f32 * 2.0; // centre x: 30..44, always left of pixel x=50
            let evs = mt.update_with_lines(
                &[det(0, 0.9, [x, 40.0, x + 40.0, 80.0])],
                f * 100,
                (100.0, 100.0),
                std::slice::from_ref(&gate),
            );
            crossed.extend(crossed_events(&evs).into_iter().map(|(l, d)| (l.to_string(), d)));
        }
        assert!(crossed.is_empty(), "never reaches the line, so it never fires");
    }

    #[test]
    fn multiple_lines_fire_independently() {
        let mut mt = MultiTracker::new(cfg());
        let lines = vec![
            Line { camera: "cam".to_string(), name: "gate".to_string(), p1: (0.5, 0.0), p2: (0.5, 1.0) }, // pixel x=50
            Line { camera: "cam".to_string(), name: "till".to_string(), p1: (0.9, 0.0), p2: (0.9, 1.0) }, // pixel x=90
            Line { camera: "cam".to_string(), name: "far".to_string(), p1: (2.0, 0.0), p2: (2.0, 1.0) },  // pixel x=200, unreachable
        ];
        let mut crossed = Vec::new();
        for f in 0..8u64 {
            let x = 10.0 + f as f32 * 13.0; // centre x: 30,43,56,69,82,95,108,121
            let evs = mt.update_with_lines(
                &[det(0, 0.9, [x, 40.0, x + 40.0, 80.0])],
                f * 100,
                (100.0, 100.0),
                &lines,
            );
            crossed.extend(crossed_events(&evs).into_iter().map(|(l, d)| (l.to_string(), d)));
        }
        let names: Vec<&str> = crossed.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(names, vec!["gate", "till"], "gate then till, each exactly once; far never reached");
    }

    // ---- zone enter/exit (polygon region of interest) ---------------------------------------

    fn zone_square() -> Polygon {
        // Pixel square x:[60,100] y:[0,100] once scaled by frame_wh=(100,100).
        Polygon {
            camera: "cam".to_string(),
            name: "counter".to_string(),
            points: vec![(0.6, 0.0), (1.0, 0.0), (1.0, 1.0), (0.6, 1.0)],
        }
    }

    /// Runs the same L→R pass (bbox x1: 10,23,36,49,62,75,88,101 over 8 frames — confirms at
    /// frame 2) and returns the frame indices at which `ZoneEntered`/`ZoneExited` first fired.
    fn run_zone_pass(zone: &Polygon, trigger: ZoneTrigger) -> (Option<u64>, Option<u64>, Option<u64>) {
        let mut mt = MultiTracker::new(cfg());
        let (mut entered_at, mut exited_at, mut dwell) = (None, None, None);
        for f in 0..8u64 {
            let x = 10.0 + f as f32 * 13.0;
            for e in mt.update_full(
                &[det(0, 0.9, [x, 40.0, x + 40.0, 80.0])],
                f * 100,
                (100.0, 100.0),
                &[],
                Some((zone, trigger)),
            ) {
                match e {
                    TrackEvent::ZoneEntered { .. } if entered_at.is_none() => entered_at = Some(f),
                    TrackEvent::ZoneExited { dwell_ms, .. } if exited_at.is_none() => {
                        exited_at = Some(f);
                        dwell = Some(dwell_ms);
                    }
                    _ => {}
                }
            }
        }
        (entered_at, exited_at, dwell)
    }

    #[test]
    fn centroid_inside_zone_emits_entered_then_exited_with_dwell() {
        let zone = zone_square();
        let (entered_at, exited_at, dwell) = run_zone_pass(&zone, ZoneTrigger::CentroidInside);
        // Centre crosses x=60 between f2(56)→f3(69): enters at f3. Crosses x=100 between
        // f5(95)→f6(108): exits at f6. dwell = ts(f6) - ts(f3) = 600 - 300 = 300ms.
        assert_eq!(entered_at, Some(3), "centroid enters the zone at frame 3");
        assert_eq!(exited_at, Some(6), "centroid leaves the zone at frame 6");
        assert_eq!(dwell, Some(300));
    }

    #[test]
    fn intersects_trigger_fires_earlier_and_releases_later_than_centroid() {
        let zone = zone_square();
        let (in_entered, in_exited, _) = run_zone_pass(&zone, ZoneTrigger::Intersects);
        let (ci_entered, ci_exited, _) = run_zone_pass(&zone, ZoneTrigger::CentroidInside);
        // Intersects only needs the bbox to touch the zone — fires the moment the track is
        // first Confirmed (frame 2, bbox already overlapping) and releases only once the whole
        // bbox has cleared the zone (frame 7) — a strictly wider window than CentroidInside's.
        assert_eq!(in_entered, Some(2), "bbox already overlaps on first confirmed frame");
        assert_eq!(in_exited, Some(7), "bbox clears the zone only once fully past it");
        assert!(in_entered < ci_entered, "intersects enters no later than centroid");
        assert!(in_exited > ci_exited, "intersects exits no earlier than centroid");
    }

    #[test]
    fn zone_with_fewer_than_three_points_never_fires() {
        let degenerate = Polygon { camera: "cam".to_string(), name: "bad".to_string(), points: vec![(0.6, 0.0), (1.0, 1.0)] };
        let (entered_at, exited_at, _) = run_zone_pass(&degenerate, ZoneTrigger::Intersects);
        assert_eq!(entered_at, None);
        assert_eq!(exited_at, None);
    }
}
