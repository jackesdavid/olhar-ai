//! MOG2 — Zivkovic's adaptive Gaussian-mixture background subtraction, in pure Rust.
//!
//! A faithful port of OpenCV's `BackgroundSubtractorMOG2` (`bgfg_gaussmix2.cpp`), for the
//! configuration the edge actually uses: 3-channel BGR input, `detectShadows = false`. It exists so
//! the edge can drop its **only native dependency** — OpenCV is what couples the binary to the host
//! (soname on Linux, DLLs on Windows) and what stops `sentinel-edge` from cross-compiling anywhere
//! (card #79; it unblocks the Windows edge, #80).
//!
//! **Fidelity is the whole point.** Every tenant's motion sensitivity is a cloud-side threshold
//! (`OnEvent("motion") → If score >= …`) calibrated against OpenCV's numbers, so a "close enough"
//! reimplementation would silently change what every existing graph fires on. The port therefore
//! follows the original's update rule step for step — including the details that look like
//! accidents:
//!
//! * The **weight prune** is `-α·CT`, so a mode's weight is decayed by `α·CT` every frame and
//!   dropped once it falls below that; this is Zivkovic's complexity-reduction prior, and it is
//!   what lets the mixture shrink back to one mode on a static scene.
//! * The **background test** (`Tb`) uses the running weight sum of the modes ranked *above* this
//!   one — not including it — against `backgroundRatio`. Fold the current weight in first and the
//!   background set grows by one mode.
//! * A matched mode is **bubbled up** the weight-sorted list, and its new weight is written at the
//!   position it ended up in, not the one it started in.
//! * `Tb` (`var_threshold`, the sensitivity knob users have) gates *background membership*, while
//!   `Tg` (`var_threshold_gen`) gates *mode matching* — two different thresholds on the same
//!   squared distance, and they are not interchangeable.
//!
//! Verified against OpenCV frame-by-frame in `tests::parity` (behind the `opencv-parity` feature).

/// One Gaussian in a pixel's mixture. Variance is isotropic — a single scalar over all three
/// channels, as in OpenCV.
#[derive(Clone, Copy, Default)]
struct Mode {
    weight: f32,
    variance: f32,
}

/// OpenCV's defaults, for the knobs we don't expose.
const N_MIXTURES: usize = 5;
/// `TB` — the top modes whose weights sum below this are the background set.
const BACKGROUND_RATIO: f32 = 0.9;
/// `Tg` — squared-distance threshold (in variances) for a pixel to *match* an existing mode.
const VAR_THRESHOLD_GEN: f32 = 9.0;
/// Variance a freshly created mode starts at, and the range one is clamped to.
const VAR_INIT: f32 = 15.0;
const VAR_MIN: f32 = 4.0;
const VAR_MAX: f32 = 5.0 * VAR_INIT;
/// `CT` — Zivkovic's complexity-reduction prior. Drives the per-frame weight decay that prunes
/// modes nothing has supported for a while.
const CT: f32 = 0.05;

/// A per-pixel adaptive Gaussian mixture over a fixed-size BGR frame.
pub struct Mog2 {
    w: usize,
    h: usize,
    /// Frames seen. Drives the learning-rate ramp — see [`Mog2::learning_rate`].
    frames: u64,
    /// `history`: the steady-state learning rate is `1/history`.
    history: u64,
    /// `Tb` — squared-distance threshold (in variances) for *background* membership. This is the
    /// user-facing sensitivity knob (`MOTION_MOG2_VAR` / `recording_config.motion_var_threshold`).
    var_threshold: f32,
    /// `w*h*N_MIXTURES`, each pixel's modes contiguous and kept sorted by descending weight.
    modes: Vec<Mode>,
    /// `w*h*N_MIXTURES` BGR means, parallel to `modes`.
    means: Vec<[f32; 3]>,
    /// How many of each pixel's `N_MIXTURES` slots are live.
    used: Vec<u8>,
}

impl Mog2 {
    /// `history` frames of adaptation; `var_threshold` is OpenCV's `varThreshold`.
    pub fn new(w: usize, h: usize, history: i32, var_threshold: f64) -> Self {
        let n = w * h;
        Mog2 {
            w,
            h,
            frames: 0,
            history: history.max(1) as u64,
            var_threshold: var_threshold as f32,
            modes: vec![Mode::default(); n * N_MIXTURES],
            means: vec![[0.0; 3]; n * N_MIXTURES],
            used: vec![0; n],
        }
    }

    /// `α = 1 / min(2·frames, history)` — **not** the constant `1/history` it's tempting to write.
    ///
    /// This is OpenCV's automatic learning rate (what you get by passing `learningRate = -1`, which
    /// is how the edge has always called it), and the ramp is not a detail: for the first
    /// `history/2` frames the model adapts *far* faster than steady state — α starts at 1/2 — so a
    /// freshly-started watcher settles on the scene in a couple of seconds instead of the ~83s a
    /// constant 1/333 would take. Get this wrong and a box that just booted reports the entire
    /// scene as motion for its first minute and a half.
    ///
    /// (This cost us a real bug: the first cut hardcoded `1/history`, and the parity test caught it
    /// — OpenCV absorbed a stationary object into the background on its 3rd frame while we were
    /// still calling it motion.)
    fn learning_rate(&self) -> f32 {
        1.0 / (2 * self.frames).min(self.history).max(1) as f32
    }

    /// Feed one BGR frame (`w*h*3` bytes, row-major) and write the 0/255 foreground mask into
    /// `mask` (`w*h` bytes). Both are caller-owned so a steady-state frame allocates nothing.
    pub fn apply(&mut self, bgr: &[u8], mask: &mut [u8]) {
        debug_assert_eq!(bgr.len(), self.w * self.h * 3);
        debug_assert_eq!(mask.len(), self.w * self.h);

        self.frames += 1;
        let alpha = self.learning_rate();
        let alpha1 = 1.0 - alpha;
        // Negative, so `weight += prune` decays it. A mode is dropped once it falls below `-prune`.
        let prune = -alpha * CT;

        for px in 0..self.w * self.h {
            let pixel = [
                f32::from(bgr[px * 3]),
                f32::from(bgr[px * 3 + 1]),
                f32::from(bgr[px * 3 + 2]),
            ];
            let base = px * N_MIXTURES;
            let mut n_modes = self.used[px] as usize;

            let mut total_weight = 0.0f32;
            let mut fits = false;
            let mut background = false;

            let mut mode = 0usize;
            while mode < n_modes {
                let i = base + mode;
                let mut weight = alpha1 * self.modes[i].weight + prune;
                let mut swaps = 0usize;

                if !fits {
                    let var = self.modes[i].variance;
                    let m = self.means[i];
                    let d = [m[0] - pixel[0], m[1] - pixel[1], m[2] - pixel[2]];
                    let dist2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];

                    // Background membership: only against the modes ranked ABOVE this one, whose
                    // weights are what `total_weight` holds right now.
                    if total_weight < BACKGROUND_RATIO && dist2 < self.var_threshold * var {
                        background = true;
                    }

                    if dist2 < VAR_THRESHOLD_GEN * var {
                        fits = true;
                        weight += alpha;
                        let k = alpha / weight;
                        self.means[i] = [m[0] - k * d[0], m[1] - k * d[1], m[2] - k * d[2]];
                        let varnew = (var + k * (dist2 - var)).clamp(VAR_MIN, VAR_MAX);
                        self.modes[i].variance = varnew;

                        // Bubble the matched mode up the weight-sorted list.
                        let mut j = mode;
                        while j > 0 {
                            if weight < self.modes[base + j - 1].weight {
                                break;
                            }
                            self.modes.swap(base + j, base + j - 1);
                            self.means.swap(base + j, base + j - 1);
                            swaps += 1;
                            j -= 1;
                        }
                    }
                }

                if weight < -prune {
                    weight = 0.0;
                    n_modes -= 1;
                }
                // The matched mode may have moved up by `swaps` places; write its weight where it
                // actually landed.
                self.modes[i - swaps].weight = weight;
                total_weight += weight;

                mode += 1;
            }

            // Renormalize.
            if total_weight > 0.0 {
                let inv = 1.0 / total_weight;
                for m in 0..n_modes {
                    self.modes[base + m].weight *= inv;
                }
            }

            // Nothing matched → spawn a mode for this pixel, replacing the weakest if full.
            if !fits && alpha > 0.0 {
                let slot = if n_modes == N_MIXTURES {
                    N_MIXTURES - 1
                } else {
                    let s = n_modes;
                    n_modes += 1;
                    s
                };

                if n_modes == 1 {
                    self.modes[base + slot].weight = 1.0;
                } else {
                    self.modes[base + slot].weight = alpha;
                    for m in 0..n_modes - 1 {
                        self.modes[base + m].weight *= alpha1;
                    }
                }
                self.means[base + slot] = pixel;
                self.modes[base + slot].variance = VAR_INIT;

                // Sort the newcomer into place.
                let mut j = n_modes - 1;
                while j > 0 {
                    if alpha < self.modes[base + j - 1].weight {
                        break;
                    }
                    self.modes.swap(base + j, base + j - 1);
                    self.means.swap(base + j, base + j - 1);
                    j -= 1;
                }
            }

            self.used[px] = n_modes as u8;
            mask[px] = if background { 0 } else { 255 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of one flat colour.
    fn flat(w: usize, h: usize, c: [u8; 3]) -> Vec<u8> {
        c.iter().copied().cycle().take(w * h * 3).collect()
    }

    /// A flat frame with a filled rectangle of a different colour.
    fn with_rect(
        w: usize,
        h: usize,
        bg: [u8; 3],
        fg: [u8; 3],
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
    ) -> Vec<u8> {
        let mut f = flat(w, h, bg);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * w + x) * 3;
                f[i..i + 3].copy_from_slice(&fg);
            }
        }
        f
    }

    /// A static scene converges to all-background: the model learns it and stops firing.
    #[test]
    fn a_static_scene_goes_quiet() {
        let (w, h) = (32, 24);
        let mut m = Mog2::new(w, h, 100, 16.0);
        let frame = flat(w, h, [40, 90, 140]);
        let mut mask = vec![0u8; w * h];
        for _ in 0..30 {
            m.apply(&frame, &mut mask);
        }
        assert_eq!(
            mask.iter().filter(|&&v| v > 0).count(),
            0,
            "a settled static scene must be pure background"
        );
    }

    /// Something that appears where the model learned background is foreground.
    #[test]
    fn a_new_object_is_foreground() {
        let (w, h) = (32, 24);
        let mut m = Mog2::new(w, h, 100, 16.0);
        let bg = flat(w, h, [40, 90, 140]);
        let mut mask = vec![0u8; w * h];
        for _ in 0..30 {
            m.apply(&bg, &mut mask);
        }

        let moved = with_rect(w, h, [40, 90, 140], [200, 30, 30], 8, 6, 16, 14);
        m.apply(&moved, &mut mask);

        let fg: usize = mask.iter().filter(|&&v| v > 0).count();
        assert_eq!(
            fg,
            8 * 8,
            "exactly the rectangle should be foreground, got {fg}px"
        );
        for y in 6..14 {
            for x in 8..16 {
                assert_eq!(mask[y * w + x], 255);
            }
        }
    }

    /// An object that stops and stays is absorbed back into the background — the property the
    /// tracker's motion gate leans on (a still person stops producing motion, so the gate has to
    /// hold on live tracks instead; see `tracking::should_detect`).
    #[test]
    fn a_parked_object_is_absorbed() {
        let (w, h) = (32, 24);
        let mut m = Mog2::new(w, h, 20, 16.0); // short history → absorbs fast
        let bg = flat(w, h, [40, 90, 140]);
        let mut mask = vec![0u8; w * h];
        for _ in 0..40 {
            m.apply(&bg, &mut mask);
        }

        let parked = with_rect(w, h, [40, 90, 140], [200, 30, 30], 8, 6, 16, 14);
        for _ in 0..200 {
            m.apply(&parked, &mut mask);
        }
        assert_eq!(
            mask.iter().filter(|&&v| v > 0).count(),
            0,
            "a long-parked object must become background"
        );
    }

    /// A pseudo-random but deterministic scene: a textured, gently-noisy background with a bright
    /// rectangle tracking across it. Stands in for a camera feed — real enough to exercise mode
    /// creation, matching, pruning and re-sorting, and reproducible enough to assert on.
    fn sequence(w: usize, h: usize, frames: usize) -> Vec<Vec<u8>> {
        let mut rng: u32 = 0x1234_5678;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng
        };
        // A fixed textured backdrop.
        let mut base = vec![0u8; w * h * 3];
        for b in base.iter_mut() {
            *b = (next() % 200) as u8;
        }
        (0..frames)
            .map(|t| {
                let mut f = base.clone();
                // Sensor noise: ±2 on every channel, so no pixel is ever perfectly static.
                for b in f.iter_mut() {
                    let n = (next() % 5) as i16 - 2;
                    *b = (i16::from(*b) + n).clamp(0, 255) as u8;
                }
                // A 10×10 bright box sweeping left to right, entering at frame 10.
                if t >= 10 {
                    let x0 = (t - 10) * 2;
                    for y in 5..15.min(h) {
                        for x in x0..(x0 + 10).min(w) {
                            let i = (y * w + x) * 3;
                            f[i..i + 3].copy_from_slice(&[250, 250, 250]);
                        }
                    }
                }
                f
            })
            .collect()
    }

    /// **The port's proof.** Run our MOG2 and OpenCV's over the same frames and require the
    /// foreground masks to agree on **every pixel of every frame**. Every tenant's motion
    /// sensitivity is a cloud-side threshold calibrated against OpenCV's numbers, so anything less
    /// than exact agreement means we quietly changed what their graphs fire on.
    ///
    /// Exact agreement is achievable — and it is a sharp instrument. It is what caught the one real
    /// bug in this port: the learning-rate ramp (see [`Mog2::learning_rate`]). A looser "within 1%"
    /// bound would have passed that bug and shipped it.
    ///
    /// Run with: `cargo test --features opencv-parity -- --nocapture`
    #[cfg(feature = "opencv-parity")]
    #[test]
    fn parity_with_opencv() {
        use opencv::{core, prelude::*, video};

        let (w, h) = (96, 64);
        let frames = sequence(w, h, 60);
        let (history, var) = (100, 16.0);

        let mut ours = Mog2::new(w, h, history, var);
        let mut theirs = video::create_background_subtractor_mog2(history, var, false).unwrap();
        let mut mask_ours = vec![0u8; w * h];
        let mut mask_cv = core::Mat::default();

        for (t, frame) in frames.iter().enumerate() {
            ours.apply(frame, &mut mask_ours);

            // Same bytes, viewed as a 3-channel Mat. `-1.0` = OpenCV's automatic learning rate,
            // which is how the edge has always driven it (and what `learning_rate` mirrors).
            let cv_frame = core::Mat::new_rows_cols_with_data(h as i32, w as i32, unsafe {
                std::slice::from_raw_parts(frame.as_ptr() as *const core::Vec3b, w * h)
            })
            .unwrap();
            video::BackgroundSubtractorTrait::apply(&mut theirs, &cv_frame, &mut mask_cv, -1.0)
                .unwrap();

            let disagree = mask_ours
                .iter()
                .zip(mask_cv.data_bytes().unwrap())
                .filter(|(a, b)| (**a > 0) != (**b > 0))
                .count();
            assert_eq!(
                disagree, 0,
                "frame {t}: {disagree} of {} pixels disagree with OpenCV — the port changed what the camera sees",
                w * h
            );
        }
    }
}
