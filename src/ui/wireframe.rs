//! Dependency-free 3D wireframe rendering for the ratatui braille `Canvas`.
//!
//! ratatui's `Canvas` has no notion of 3D, so we keep a tiny vertex model in
//! normalized space, rotate it by the global sweep phase, project it onto the
//! canvas's 2D [-1, 1] plane with a perspective divide, and draw each edge as a
//! `CanvasLine`. Far edges fade to faint phosphor and near ones glow bright, so
//! the shapes read with depth — the classic green MU/TH/UR vector look.
use std::f64::consts::{PI, TAU};

use ratatui::style::Color;
use ratatui::widgets::canvas::{Context, Line as CanvasLine};

use crate::ui::theme::{PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

/// Eye distance along +Z. The model sits at the origin; the camera looks toward
/// -Z. Larger values weaken the perspective (more orthographic).
const CAM_Z: f64 = 3.2;
/// Field-of-view scale applied after the perspective divide. Tuned so a unit
/// model fills most of the canvas without touching the panel border.
const FOV: f64 = 1.7;

/// A point in 3D model space. Model units run roughly [-1, 1] on each axis.
#[derive(Clone, Copy)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl P3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        P3 { x, y, z }
    }

    /// Rotate about the Y axis (yaw) — the workhorse for the turntable spin.
    pub fn rot_y(self, a: f64) -> P3 {
        let (s, c) = a.sin_cos();
        P3 { x: c * self.x + s * self.z, y: self.y, z: -s * self.x + c * self.z }
    }

    /// Rotate about the X axis (pitch).
    pub fn rot_x(self, a: f64) -> P3 {
        let (s, c) = a.sin_cos();
        P3 { x: self.x, y: c * self.y - s * self.z, z: s * self.y + c * self.z }
    }

    /// Rotate about the Z axis (roll).
    pub fn rot_z(self, a: f64) -> P3 {
        let (s, c) = a.sin_cos();
        P3 { x: c * self.x - s * self.y, y: s * self.x + c * self.y, z: self.z }
    }

    /// Perspective-project to canvas 2D space. Returns `(x, y)` in ~[-1, 1] plus
    /// the camera-space depth (for shading). Points at or behind the eye are
    /// clamped to a small epsilon so we never divide by zero or flip the image.
    fn project(self) -> (f64, f64, f64) {
        let zc = (CAM_Z - self.z).max(0.05); // depth in front of the eye, > 0
        let k = FOV / zc; // the perspective divide
        (self.x * k, self.y * k, zc)
    }
}

/// True if a projected point is comfortably outside the visible plane. An edge
/// with both ends out of view is skipped; the canvas clips the rest.
fn out_of_view(x: f64, y: f64) -> bool {
    !(-1.05..=1.05).contains(&x) || !(-1.05..=1.05).contains(&y)
}

/// Pick a phosphor shade from a camera-space depth: near = bright, far = faint.
fn shade(depth: f64) -> Color {
    if depth < CAM_Z - 0.35 {
        PHOSPHOR
    } else if depth > CAM_Z + 0.35 {
        PHOSPHOR_FAINT
    } else {
        PHOSPHOR_DIM
    }
}

/// Draw an edge between two model-space points (already rotated by the caller),
/// shading it by mean depth so the far side of a shape recedes.
fn draw_edge_shaded(ctx: &mut Context<'_>, a: P3, b: P3) {
    let (x1, y1, za) = a.project();
    let (x2, y2, zb) = b.project();
    if out_of_view(x1, y1) && out_of_view(x2, y2) {
        return;
    }
    ctx.draw(&CanvasLine { x1, y1, x2, y2, color: shade((za + zb) * 0.5) });
}

/// Draw an edge in a fixed colour — used for the bright foreground elements
/// (the scanner sweep, the oscilloscope trace) that should not depth-fade.
fn draw_edge(ctx: &mut Context<'_>, a: P3, b: P3, color: Color) {
    let (x1, y1, _) = a.project();
    let (x2, y2, _) = b.project();
    if out_of_view(x1, y1) && out_of_view(x2, y2) {
        return;
    }
    ctx.draw(&CanvasLine { x1, y1, x2, y2, color });
}

/// A static wireframe: vertices plus index pairs into them.
pub struct Mesh {
    pub verts: &'static [P3],
    pub edges: &'static [(usize, usize)],
}

impl Mesh {
    /// Rotate every vertex with `xf`, then draw every edge with depth shading.
    fn draw(&self, ctx: &mut Context<'_>, xf: impl Fn(P3) -> P3) {
        for &(i, j) in self.edges {
            draw_edge_shaded(ctx, xf(self.verts[i]), xf(self.verts[j]));
        }
    }
}

// -- SCANNER: rotating wireframe globe with a bright sweep meridian -----------

const GLOBE_SEG: usize = 24; // segments per latitude ring
const GLOBE_MERIDIANS: usize = 6;
const GLOBE_ARC: usize = 16; // segments per pole-to-pole meridian

/// One point on the unit sphere at latitude `lat` (-PI/2..PI/2) and longitude
/// `lon` (0..TAU).
fn sphere(lat: f64, lon: f64) -> P3 {
    let (slat, clat) = lat.sin_cos();
    let (slon, clon) = lon.sin_cos();
    P3::new(clat * clon, slat, clat * slon)
}

/// SCANNER schematic: a slowly yawing latitude/longitude cage with one bright
/// meridian sweeping faster than the cage — echoing the sonar's radar ray.
pub fn draw_globe(ctx: &mut Context<'_>, phase: f64) {
    let spin = |p: P3| p.rot_y(phase * 0.9).rot_x(0.35);

    // Latitude rings at -60, -30, 0, 30, 60 degrees.
    for k in 1..GLOBE_MERIDIANS {
        let lat = -PI / 2.0 + k as f64 * (PI / GLOBE_MERIDIANS as f64);
        let mut prev = spin(sphere(lat, 0.0));
        for s in 1..=GLOBE_SEG {
            let lon = s as f64 / GLOBE_SEG as f64 * TAU;
            let cur = spin(sphere(lat, lon));
            draw_edge_shaded(ctx, prev, cur);
            prev = cur;
        }
    }

    // Longitude half-meridians from south pole to north pole.
    for m in 0..GLOBE_MERIDIANS {
        let lon = m as f64 / GLOBE_MERIDIANS as f64 * PI;
        let mut prev = spin(sphere(-PI / 2.0, lon));
        for s in 1..=GLOBE_ARC {
            let lat = -PI / 2.0 + s as f64 / GLOBE_ARC as f64 * PI;
            let cur = spin(sphere(lat, lon));
            draw_edge_shaded(ctx, prev, cur);
            prev = cur;
        }
    }

    // Bright sweep meridian on top, circling at twice the cage speed.
    ctx.layer();
    let sweep_lon = phase * 2.0;
    let mut prev = spin(sphere(-PI / 2.0, sweep_lon));
    for s in 1..=GLOBE_ARC {
        let lat = -PI / 2.0 + s as f64 / GLOBE_ARC as f64 * PI;
        let cur = spin(sphere(lat, sweep_lon));
        draw_edge(ctx, prev, cur, PHOSPHOR);
        prev = cur;
    }
}

// -- CHAT: 3D oscilloscope (tumbling Lissajous trace) -------------------------

const SCOPE_STEPS: usize = 120;

/// CHAT schematic: a 3D Lissajous figure whose trace oscillates with the phase
/// while the whole curve slowly tumbles — BISHOP's "voice" waveform in 3D.
pub fn draw_scope(ctx: &mut Context<'_>, phase: f64) {
    let tumble = |p: P3| p.rot_y(phase * 0.4).rot_x(0.2);
    let pt = |t: f64| {
        P3::new(
            (3.0 * t + phase).sin() * 0.85,
            (2.0 * t).sin() * 0.85,
            (4.0 * t + phase * 0.5).cos() * 0.85,
        )
    };

    let mut prev = tumble(pt(0.0));
    for i in 1..=SCOPE_STEPS {
        let t = i as f64 / SCOPE_STEPS as f64 * TAU;
        let cur = tumble(pt(t));
        draw_edge(ctx, prev, cur, PHOSPHOR);
        prev = cur;
    }
}

// -- CONFIG: rotating wireframe torus -----------------------------------------

const TORUS_U: usize = 18; // segments around the major circle
const TORUS_V: usize = 10; // segments around the minor circle
const TORUS_R: f64 = 0.62; // major radius
const TORUS_R_MINOR: f64 = 0.24; // minor radius

/// A point on the torus surface at angles `u` (major) and `v` (minor).
fn torus_pt(u: f64, v: f64) -> P3 {
    let (su, cu) = u.sin_cos();
    let (sv, cv) = v.sin_cos();
    let ring = TORUS_R + TORUS_R_MINOR * cv;
    P3::new(ring * cu, TORUS_R_MINOR * sv, ring * su)
}

/// CONFIG schematic: a wireframe torus rolling on two axes — reads as a turning
/// mechanism / system component.
pub fn draw_torus(ctx: &mut Context<'_>, phase: f64) {
    let roll = |p: P3| p.rot_x(phase * 0.7).rot_z(phase * 0.3);
    for iu in 0..TORUS_U {
        let u0 = iu as f64 / TORUS_U as f64 * TAU;
        let u1 = (iu + 1) as f64 / TORUS_U as f64 * TAU;
        for iv in 0..TORUS_V {
            let v0 = iv as f64 / TORUS_V as f64 * TAU;
            let v1 = (iv + 1) as f64 / TORUS_V as f64 * TAU;
            draw_edge_shaded(ctx, roll(torus_pt(u0, v0)), roll(torus_pt(u0, v1)));
            draw_edge_shaded(ctx, roll(torus_pt(u0, v0)), roll(torus_pt(u1, v0)));
        }
    }
}

// -- ALERTS: broadcast beacon emitting signal rings ---------------------------

const BEACON_RINGS: usize = 3;
const BEACON_SEG: usize = 28;

/// ALERTS schematic: a comm beacon — a central mast whose top node emits
/// expanding, rising signal rings that fade as they go, like a pulse broadcast
/// out to the paired handsets.
pub fn draw_beacon(ctx: &mut Context<'_>, phase: f64) {
    let tilt = |p: P3| p.rot_y(phase * 0.3).rot_x(0.62);

    // Mast + a small antenna node at the top.
    draw_edge(ctx, tilt(P3::new(0.0, -0.55, 0.0)), tilt(P3::new(0.0, 0.55, 0.0)), PHOSPHOR);
    draw_edge(ctx, tilt(P3::new(-0.07, 0.55, 0.0)), tilt(P3::new(0.07, 0.55, 0.0)), PHOSPHOR);
    draw_edge(ctx, tilt(P3::new(0.0, 0.55, -0.07)), tilt(P3::new(0.0, 0.55, 0.07)), PHOSPHOR);

    // Signal rings: each rises and widens on its own phase offset, so the beacon
    // pulses continuously. Dim at birth and death masks the loop reset.
    for k in 0..BEACON_RINGS {
        let t = (phase * 0.5 + k as f64 / BEACON_RINGS as f64).rem_euclid(1.0);
        let r = t * 0.85;
        let y = -0.05 + t * 0.65;
        let color = if t < 0.12 || t > 0.85 {
            PHOSPHOR_FAINT
        } else if t > 0.55 {
            PHOSPHOR_DIM
        } else {
            PHOSPHOR
        };
        let ring = |a: f64| P3::new(a.cos() * r, y, a.sin() * r);
        let mut prev = tilt(ring(0.0));
        for s in 1..=BEACON_SEG {
            let a = s as f64 / BEACON_SEG as f64 * TAU;
            let cur = tilt(ring(a));
            draw_edge(ctx, prev, cur, color);
            prev = cur;
        }
    }
}

// -- ABOUT: rotating low-poly ship hull ---------------------------------------

static SHIP_VERTS: &[P3] = &[
    P3::new(0.0, 0.00, 0.95),   // 0 nose
    P3::new(-0.30, 0.12, 0.30), // 1 upper-left fwd
    P3::new(0.30, 0.12, 0.30),  // 2 upper-right fwd
    P3::new(-0.30, -0.12, 0.30),// 3 lower-left fwd
    P3::new(0.30, -0.12, 0.30), // 4 lower-right fwd
    P3::new(-0.45, 0.18, -0.55),// 5 upper-left aft
    P3::new(0.45, 0.18, -0.55), // 6 upper-right aft
    P3::new(-0.45, -0.18, -0.55),// 7 lower-left aft
    P3::new(0.45, -0.18, -0.55),// 8 lower-right aft
    P3::new(-0.65, 0.05, -0.85),// 9 left engine pod
    P3::new(0.65, 0.05, -0.85), // 10 right engine pod
    P3::new(0.0, 0.40, -0.30),  // 11 dorsal fin tip
];

static SHIP_EDGES: &[(usize, usize)] = &[
    (0, 1), (0, 2), (0, 3), (0, 4), // nose to forward ring
    (1, 2), (2, 4), (4, 3), (3, 1), // forward ring
    (1, 5), (2, 6), (3, 7), (4, 8), // forward ring to aft ring
    (5, 6), (6, 8), (8, 7), (7, 5), // aft ring
    (5, 9), (7, 9), (6, 10), (8, 10), // engine pods
    (5, 11), (6, 11), // dorsal fin
];

static SHIP: Mesh = Mesh { verts: SHIP_VERTS, edges: SHIP_EDGES };

/// ABOUT schematic: a stylized low-poly hull turning on a slow turntable.
pub fn draw_ship(ctx: &mut Context<'_>, phase: f64) {
    SHIP.draw(ctx, |p| p.rot_y(phase * 0.8).rot_x(0.30));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_projects_to_centre_at_camera_depth() {
        let (x, y, z) = P3::new(0.0, 0.0, 0.0).project();
        assert!(x.abs() < 1e-9 && y.abs() < 1e-9);
        assert!((z - CAM_Z).abs() < 1e-9);
    }

    #[test]
    fn nearer_points_project_larger() {
        // Same world x, but the nearer point (larger z) must appear wider.
        let (near, _, _) = P3::new(0.5, 0.0, 0.5).project();
        let (far, _, _) = P3::new(0.5, 0.0, -0.5).project();
        assert!(near.abs() > far.abs());
    }
}
