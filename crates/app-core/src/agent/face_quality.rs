//! Face quality signals (#126) — **sharpness** and **head pose**, computed from what the detector
//! already produced. Both are cheap, pure functions with no model and no extra decode, which is why
//! they can run on the edge inside the existing detect loop.
//!
//! WHY. Recognition, clustering and every threshold downstream are garbage-in/garbage-out. Until
//! now the only quality signals reaching a graph were `confidence` (the detector believed it was a
//! face) and `face_width` (it was big enough) — neither of which says whether the face was in
//! FOCUS or pointed at the camera. A motion-blurred profile shot clears both gates, embeds into a
//! vector that resembles nothing, and then drags a cluster with it.
//!
//! These are deliberately NOT decisions. They are numbers, exposed as ports on
//! `When Event("face_detected")`, so the gate is composed in the graph
//! (`NumGte(blur) + NumLte(|yaw|) → And → Branch`) rather than hardcoded in a node — the same
//! reason `StoreFace` only stores and never judges.

/// Sentinel `blur` for a face reported by an edge too old to measure it (#126).
///
/// A quality gate is composed as `NumGte(blur, threshold)`, and `run_num_cmp` resolves a missing
/// input to **0** — so simply omitting the field would make every gate FAIL CLOSED and silently
/// drop the entire face pipeline of any box that has not updated yet. Pose has the opposite and
/// harmless default (`NumLte(|yaw|, max)` passes at 0), so only sharpness needs a sentinel.
///
/// The value is far outside the natural range (real crops land in the hundreds to low thousands),
/// so it passes any sane threshold and is unmistakable in the UI. It is NOT persisted: `StoreFace`
/// maps it back to NULL, because "unknown" must not enter the calibration data as "excellent".
pub const BLUR_UNKNOWN: f64 = 1.0e6;

/// Head pose in degrees, estimated from the 5 detector landmarks.
///
/// Sign conventions, in image space: `yaw` > 0 means the face is turned so the nose moves toward
/// the right of the image; `pitch` > 0 means tilted down (chin toward the camera); `roll` > 0 means
/// the head is rotated clockwise in the image plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

/// The ArcFace input size. Sharpness is measured at the resolution the EMBEDDER will actually see,
/// not at the source resolution — otherwise the metric mostly reports how many megapixels the
/// camera has. A 400 px face that is soft and a 90 px face that is crisp both get resampled to this
/// before embedding, and it is the detail surviving that resample which decides the embedding.
const ALIGN_SIZE: u32 = 112;

/// Anthropometric ratios used to turn landmark geometry into an angle.
///
/// `EYE_SPAN_OVER_NOSE_DEPTH` — inter-ocular distance ÷ how far the nose tip sits IN FRONT of the
/// eye plane (≈ 63 mm ÷ 20 mm). This is the constant that converts the nose's sideways drift into a
/// yaw angle; it is a population average, so per-person error is expected and bounded by the test.
const EYE_SPAN_OVER_NOSE_DEPTH: f32 = 3.15;
/// Where the nose tip sits vertically between the eye line (0) and the mouth line (1), frontally.
const NOSE_HEIGHT_FRACTION: f32 = 0.5;
/// Eye-to-mouth distance ÷ nose depth (≈ 60 mm ÷ 20 mm) — the pitch counterpart of the above.
const EYE_MOUTH_OVER_NOSE_DEPTH: f32 = 3.0;

/// Landmark indices as the detector emits them (right eye, left eye, nose, right mouth, left mouth).
/// Referred to by position rather than by side: which physical eye lands at index 0 depends on the
/// model's convention, and the geometry below only needs "the two eyes" and "the two mouth corners".
const EYE_A: usize = 0;
const EYE_B: usize = 1;
const NOSE: usize = 2;
const MOUTH_A: usize = 3;
const MOUTH_B: usize = 4;

/// Estimate head pose from the 5 landmarks (source pixels).
///
/// Weak-perspective model. Put the eyes at `(±e, 0, 0)` and the nose tip at `(0, ny, nz)` with the
/// nose protruding toward the camera. Rotating by yaw θ about the vertical axis and dropping z:
///
/// ```text
///   inter-ocular span → 2e·cosθ        nose x, relative to the eye midpoint → nz·sinθ
///   ⇒ ratio r = nz·sinθ / (2e·cosθ) = (nz/2e)·tanθ   ⇒   θ = atan(r · 2e/nz)
/// ```
///
/// So the sideways drift of the nose between the eyes, in units of inter-ocular distance, IS the
/// yaw signal — and the conversion needs exactly one anthropometric constant. Pitch follows the
/// same derivation on the eye→mouth axis. Roll needs no model at all: it is the tilt of the eye
/// line, exact in 2D.
///
/// Measured in the eye-line frame (de-rotated by roll) so a tilted head does not leak into yaw.
/// Returns an all-zero pose when the landmarks are absent or degenerate — the caller cannot tell
/// "frontal" from "unknown" by the pose alone, which is why `kps` being empty is signalled by the
/// event carrying no landmarks rather than by a sentinel angle.
pub fn pose_from_kps(kps: &[[f32; 2]; 5]) -> Pose {
    let zero = Pose {
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
    };
    if kps.iter().all(|&[x, y]| x == 0.0 && y == 0.0) {
        return zero;
    }
    // Order the eyes left-to-right in the image so the yaw sign is stable regardless of which
    // index the detector assigned to which physical eye.
    let (left, right) = if kps[EYE_A][0] <= kps[EYE_B][0] {
        (kps[EYE_A], kps[EYE_B])
    } else {
        (kps[EYE_B], kps[EYE_A])
    };
    let (dx, dy) = (right[0] - left[0], right[1] - left[1]);
    let iod = (dx * dx + dy * dy).sqrt();
    if iod < 1e-3 {
        return zero; // both eyes at one point — no usable geometry
    }
    let roll = dy.atan2(dx);

    // Rotate everything into the eye-line frame: +x along the eye line, +y down the face.
    let (c, s) = (roll.cos(), roll.sin());
    let eye_mid = [(left[0] + right[0]) * 0.5, (left[1] + right[1]) * 0.5];
    let to_frame = |p: [f32; 2]| {
        let (px, py) = (p[0] - eye_mid[0], p[1] - eye_mid[1]);
        [px * c + py * s, -px * s + py * c]
    };
    let nose = to_frame(kps[NOSE]);
    let mouth_a = to_frame(kps[MOUTH_A]);
    let mouth_b = to_frame(kps[MOUTH_B]);
    let mouth_mid_y = (mouth_a[1] + mouth_b[1]) * 0.5;

    // Yaw: the nose's sideways drift, in inter-ocular units.
    let yaw = (nose[0] / iod * EYE_SPAN_OVER_NOSE_DEPTH).atan();

    // Pitch: the nose's height between eye line and mouth line drifts as the head tilts, because
    // the protruding nose tip projects into y while the eye→mouth span foreshortens.
    let pitch = if mouth_mid_y.abs() < 1e-3 {
        0.0
    } else {
        let s_ratio = nose[1] / mouth_mid_y;
        ((s_ratio - NOSE_HEIGHT_FRACTION) * EYE_MOUTH_OVER_NOSE_DEPTH).atan()
    };

    Pose {
        yaw: yaw.to_degrees(),
        pitch: pitch.to_degrees(),
        roll: roll.to_degrees(),
    }
}

/// Sharpness (variance of the Laplacian) of the face region, measured at [`ALIGN_SIZE`].
///
/// Higher is sharper; a flat or evenly-blurred patch tends to 0. The value is not in any physical
/// unit — it is a monotone focus score, which is all a `NumGte` gate needs. Because it is measured
/// after resampling to the embedder's input size, scores are comparable across cameras of different
/// resolutions and across faces at different distances, which raw variance-of-Laplacian is not.
///
/// `bgr` is the frame the detector already decoded (3 bytes/pixel, B G R), so this adds no decode.
/// Returns 0.0 for an empty or out-of-frame box.
pub fn sharpness_bgr(bgr: &[u8], w: u32, h: u32, xyxy: [f32; 4]) -> f32 {
    let Some(gray) = crop_to_luma(bgr, w, h, xyxy) else {
        return 0.0;
    };
    let resized = image::imageops::resize(
        &gray,
        ALIGN_SIZE,
        ALIGN_SIZE,
        image::imageops::FilterType::Triangle,
    );
    laplacian_variance(&resized)
}

/// Crop the face box out of a BGR frame into a grayscale image. `None` when the box does not
/// intersect the frame or is degenerate.
fn crop_to_luma(bgr: &[u8], w: u32, h: u32, xyxy: [f32; 4]) -> Option<image::GrayImage> {
    if w == 0 || h == 0 || bgr.len() < (w as usize * h as usize * 3) {
        return None;
    }
    let x1 = (xyxy[0].floor().max(0.0) as u32).min(w.saturating_sub(1));
    let y1 = (xyxy[1].floor().max(0.0) as u32).min(h.saturating_sub(1));
    let x2 = (xyxy[2].ceil().max(0.0) as u32).min(w);
    let y2 = (xyxy[3].ceil().max(0.0) as u32).min(h);
    let (cw, ch) = (x2.saturating_sub(x1), y2.saturating_sub(y1));
    if cw < 3 || ch < 3 {
        return None; // smaller than the Laplacian kernel — no meaningful measurement
    }
    let mut out = image::GrayImage::new(cw, ch);
    for y in 0..ch {
        for x in 0..cw {
            let i = (((y + y1) as usize * w as usize) + (x + x1) as usize) * 3;
            let (b, g, r) = (bgr[i] as f32, bgr[i + 1] as f32, bgr[i + 2] as f32);
            let luma = 0.114 * b + 0.587 * g + 0.299 * r;
            out.put_pixel(x, y, image::Luma([luma.round().clamp(0.0, 255.0) as u8]));
        }
    }
    Some(out)
}

/// Variance of the 3×3 Laplacian response over the interior pixels.
fn laplacian_variance(img: &image::GrayImage) -> f32 {
    let (w, h) = (img.width() as i64, img.height() as i64);
    if w < 3 || h < 3 {
        return 0.0;
    }
    let at = |x: i64, y: i64| img.get_pixel(x as u32, y as u32).0[0] as f32;
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut n = 0u64;
    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let lap = at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1) - 4.0 * at(x, y);
            sum += lap as f64;
            sum_sq += (lap as f64) * (lap as f64);
            n += 1;
        }
    }
    if n == 0 {
        return 0.0;
    }
    let mean = sum / n as f64;
    ((sum_sq / n as f64) - mean * mean).max(0.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- helpers -----------------------------------------------------------------------------

    /// A deterministic high-frequency BGR test pattern — the sharpest thing a camera could produce.
    fn checkerboard(w: u32, h: u32, cell: u32) -> Vec<u8> {
        let mut buf = vec![0u8; (w * h * 3) as usize];
        for y in 0..h {
            for x in 0..w {
                let on = ((x / cell) + (y / cell)) % 2 == 0;
                let v = if on { 235 } else { 20 };
                let i = ((y * w + x) * 3) as usize;
                buf[i] = v;
                buf[i + 1] = v;
                buf[i + 2] = v;
            }
        }
        buf
    }

    /// One pass of a 3×3 box blur over a BGR buffer — a stand-in for defocus/motion blur.
    fn box_blur(bgr: &[u8], w: u32, h: u32) -> Vec<u8> {
        let mut out = bgr.to_vec();
        for y in 1..(h - 1) {
            for x in 1..(w - 1) {
                for ch in 0..3 {
                    let mut acc = 0u32;
                    for dy in 0..3u32 {
                        for dx in 0..3u32 {
                            let i = (((y + dy - 1) * w + (x + dx - 1)) * 3 + ch) as usize;
                            acc += bgr[i] as u32;
                        }
                    }
                    out[((y * w + x) * 3 + ch) as usize] = (acc / 9) as u8;
                }
            }
        }
        out
    }

    /// Project a 3-D face model to 2-D landmarks under known yaw/pitch/roll (degrees).
    ///
    /// `scale_*` deliberately let the caller DEVIATE from the anthropometry the estimator assumes,
    /// so the tests measure robustness to individual variation rather than replaying the
    /// estimator's own algebra back at it.
    fn project(
        yaw_deg: f32,
        pitch_deg: f32,
        roll_deg: f32,
        nose_depth_scale: f32,
        eye_span_scale: f32,
    ) -> [[f32; 2]; 5] {
        let half_eye = 31.5 * eye_span_scale; // mm
        let nose_depth = 20.0 * nose_depth_scale;
        let mouth_y = 60.0;
        let nose_y = mouth_y * NOSE_HEIGHT_FRACTION;
        // (x, y, z); +y down the face, +z toward the camera.
        let pts = [
            [-half_eye, 0.0, 0.0],
            [half_eye, 0.0, 0.0],
            [0.0, nose_y, nose_depth],
            [-25.0, mouth_y, 0.0],
            [25.0, mouth_y, 0.0],
        ];
        let (ya, pa, ra) = (
            yaw_deg.to_radians(),
            pitch_deg.to_radians(),
            roll_deg.to_radians(),
        );
        let mut out = [[0.0f32; 2]; 5];
        for (i, p) in pts.iter().enumerate() {
            // Yaw about the vertical axis, then pitch about the horizontal, then roll in-plane.
            let (x1, y1, z1) = (
                p[0] * ya.cos() + p[2] * ya.sin(),
                p[1],
                -p[0] * ya.sin() + p[2] * ya.cos(),
            );
            let (x2, y2) = (x1, y1 * pa.cos() + z1 * pa.sin());
            let (x3, y3) = (
                x2 * ra.cos() - y2 * ra.sin(),
                x2 * ra.sin() + y2 * ra.cos(),
            );
            // Weak perspective: drop z, offset into image coordinates.
            out[i] = [320.0 + x3, 240.0 + y3];
        }
        out
    }

    // ---- sharpness ---------------------------------------------------------------------------

    /// The core property: blurring an image may only ever LOWER its sharpness score. Asserted
    /// across successive blur passes, so it is monotonicity that is proven, not one lucky pair.
    #[test]
    fn sharpness_decreases_monotonically_with_blur() {
        let (w, h) = (240u32, 240u32);
        let mut frame = checkerboard(w, h, 4);
        let bbox = [20.0, 20.0, 220.0, 220.0];
        let sharp = sharpness_bgr(&frame, w, h, bbox);
        let mut series = vec![sharp];
        let mut previous = sharp;
        for pass in 1..=8 {
            frame = box_blur(&frame, w, h);
            let score = sharpness_bgr(&frame, w, h, bbox);
            assert!(
                score < previous,
                "blur pass {pass} did not reduce sharpness: {score} >= {previous}"
            );
            previous = score;
            series.push(score);
        }
        println!("sharpness under successive blur passes: {series:?}");
        // Relative, not absolute: the metric has no physical unit, so the only honest claim is
        // that blur collapses it — an absolute floor would just encode whatever this particular
        // synthetic pattern happens to score.
        assert!(
            previous < sharp * 0.1,
            "8 blur passes only took sharpness from {sharp} to {previous} — \
             the metric is barely responding to blur"
        );
    }

    /// A flat patch has no detail to measure, blurred or not.
    #[test]
    fn sharpness_of_flat_region_is_zero() {
        let (w, h) = (120u32, 120u32);
        let frame = vec![128u8; (w * h * 3) as usize];
        let score = sharpness_bgr(&frame, w, h, [0.0, 0.0, 120.0, 120.0]);
        assert!(score < 1e-3, "a flat patch should score ~0, got {score}");
    }

    /// Comparability across resolutions is the whole reason the metric resamples to the embedder's
    /// input size. The SAME pattern captured at 2× the pixels must not score wildly differently,
    /// or a gate threshold tuned on one camera would be meaningless on the next.
    #[test]
    fn sharpness_is_comparable_across_capture_resolutions() {
        let small = checkerboard(120, 120, 3);
        let large = checkerboard(240, 240, 6); // same pattern, twice the pixels
        let a = sharpness_bgr(&small, 120, 120, [0.0, 0.0, 120.0, 120.0]);
        let b = sharpness_bgr(&large, 240, 240, [0.0, 0.0, 240.0, 240.0]);
        let ratio = a.max(b) / a.min(b).max(1e-6);
        assert!(
            ratio < 1.5,
            "the same pattern at two capture sizes scored {a} vs {b} ({ratio:.2}x apart) — \
             the metric is still resolution-dependent"
        );
    }

    /// Guard rails: a box outside the frame, or smaller than the kernel, must not panic.
    #[test]
    fn sharpness_handles_degenerate_boxes() {
        let frame = checkerboard(60, 60, 3);
        assert_eq!(sharpness_bgr(&frame, 60, 60, [0.0, 0.0, 2.0, 2.0]), 0.0);
        assert_eq!(sharpness_bgr(&frame, 60, 60, [500.0, 500.0, 600.0, 600.0]), 0.0);
        assert_eq!(sharpness_bgr(&[], 0, 0, [0.0, 0.0, 10.0, 10.0]), 0.0);
    }

    // ---- pose --------------------------------------------------------------------------------

    #[test]
    fn frontal_face_has_zero_pose() {
        let p = pose_from_kps(&project(0.0, 0.0, 0.0, 1.0, 1.0));
        assert!(p.yaw.abs() < 1.0, "yaw {} should be ~0", p.yaw);
        assert!(p.pitch.abs() < 1.0, "pitch {} should be ~0", p.pitch);
        assert!(p.roll.abs() < 1.0, "roll {} should be ~0", p.roll);
    }

    /// Roll is pure 2-D geometry, so it must be recovered EXACTLY — no anthropometry involved and
    /// therefore no excuse for error.
    #[test]
    fn roll_is_recovered_exactly() {
        for truth in [-40.0f32, -15.0, 0.0, 10.0, 25.0, 45.0] {
            let p = pose_from_kps(&project(0.0, 0.0, truth, 1.0, 1.0));
            assert!(
                (p.roll - truth).abs() < 0.5,
                "roll {truth}° recovered as {}°",
                p.roll
            );
        }
    }

    /// Yaw recovery under *individual variation* — the nose is 25% longer and the eyes 10% wider
    /// than the population constants the estimator assumes. Recovery must stay within a tolerance
    /// wide enough to be honest about a 5-point model and tight enough to gate on.
    #[test]
    fn yaw_survives_anthropometric_variation() {
        for truth in [-45.0f32, -30.0, -15.0, 15.0, 30.0, 45.0] {
            for (nose, eyes) in [(1.0, 1.0), (1.25, 1.1), (0.8, 0.95)] {
                let p = pose_from_kps(&project(truth, 0.0, 0.0, nose, eyes));
                let err = (p.yaw - truth).abs();
                assert!(
                    err < 12.0,
                    "yaw {truth}° with nose×{nose} eyes×{eyes} recovered as {:.1}° (err {err:.1}°)",
                    p.yaw
                );
                assert!(
                    p.yaw.signum() == truth.signum(),
                    "yaw {truth}° recovered with the wrong sign ({:.1}°)",
                    p.yaw
                );
            }
        }
    }

    /// Whatever the absolute error, the estimate must ORDER correctly — that is what a
    /// `NumLte(max_yaw)` gate actually relies on.
    #[test]
    fn yaw_is_monotonic_in_true_angle() {
        let mut previous = f32::NEG_INFINITY;
        for truth in [-50.0f32, -35.0, -20.0, -5.0, 5.0, 20.0, 35.0, 50.0] {
            let y = pose_from_kps(&project(truth, 0.0, 0.0, 1.0, 1.0)).yaw;
            assert!(y > previous, "yaw not monotonic at {truth}°: {y} <= {previous}");
            previous = y;
        }
    }

    /// Pitch is the weakest of the three from 5 points; the tolerance says so out loud rather than
    /// pretending otherwise. Monotonicity and sign are what the gate needs.
    #[test]
    fn pitch_is_monotonic_and_correctly_signed() {
        let mut previous = f32::NEG_INFINITY;
        for truth in [-30.0f32, -20.0, -10.0, 0.0, 10.0, 20.0, 30.0] {
            let p = pose_from_kps(&project(0.0, truth, 0.0, 1.0, 1.0)).pitch;
            assert!(
                p > previous,
                "pitch not monotonic at {truth}°: {p} <= {previous}"
            );
            if truth.abs() >= 10.0 {
                assert_eq!(
                    p.signum(),
                    truth.signum(),
                    "pitch {truth}° recovered with the wrong sign ({p:.1}°)"
                );
            }
            previous = p;
        }
    }

    /// A rolled head must not leak into yaw — the estimator de-rotates into the eye-line frame
    /// precisely so a tilted camera does not read as a turned head.
    #[test]
    fn roll_does_not_leak_into_yaw() {
        for roll in [-30.0f32, -10.0, 10.0, 30.0] {
            let p = pose_from_kps(&project(0.0, 0.0, roll, 1.0, 1.0));
            assert!(
                p.yaw.abs() < 3.0,
                "roll {roll}° leaked {:.1}° into yaw",
                p.yaw
            );
        }
    }

    #[test]
    fn missing_landmarks_yield_zero_pose() {
        let p = pose_from_kps(&[[0.0, 0.0]; 5]);
        assert_eq!(
            p,
            Pose {
                yaw: 0.0,
                pitch: 0.0,
                roll: 0.0
            }
        );
    }
}
