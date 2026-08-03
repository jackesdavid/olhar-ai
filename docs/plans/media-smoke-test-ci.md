# Media pipeline smoke test in CI — Implementation Plan

**Weakness:** [[architecture-weaknesses]] §5 **Board card:** #159 Media pipeline smoke test in CI
**Goal:** prove a camera still produces frames, a clip, and a snapshot — on every merge that touches
the media path.

## Why

`check:rust` proves the pipeline compiles. **Nothing proved a camera still records.** That is not
theoretical: the capture watchdog bug (#164) killed every pipeline restart, shipped to a customer
box, survived several releases, and surfaced only when a human noticed their camera was dark. 44
edge tests passed throughout.

## What was already there, and why it counted for nothing

`capture.rs` carried a `gst_linked_it` integration test that looked exactly like this coverage. It
was **dead twice over**:

1. gated on `#[cfg(all(test, feature = "gst-linked"))]`, and `gst-linked` was deleted in `277f079`
   (`cargo` was warning `expected values for feature are: opencv-parity`);
2. it called `ensure_with_url` with four arguments; the function takes three.

It had never compiled, let alone run. A test that cannot build is indistinguishable from a test that
passes — the same failure as the edge `Device` fixture found in #162.

## What the test asserts

`capture::media_smoke`, against a synthetic RTSP source:

| Assertion | Why it is the right one |
|---|---|
| A JPEG ≥1 KB from `latest_fresh` | **This is also the snapshot path** — `GET /snapshot/:ip` is `capture::ensure` + `capture::latest_fresh`, so one assertion covers both |
| fMP4 init with `ftyp` + `moov`, `avc1` mime, 3 live fragments | The live view, off the **same single connection** — the ONE-PIPELINE invariant |
| A `.mp4` >8 KB with `ftyp` + `moov` | A clip that plays. A header with no video is not a recording |

It skips when `MEDIA_SMOKE_RTSP` is unset so `cargo test` stays hermetic.

## The skip cannot pass

A skip exits 0. `check:media` therefore greps the output for a `media_smoke: RAN` marker and fails
the job without it — so "the RTSP service was unreachable" can never read as "the media path
works". Same guard shape as `check:sql`'s coverage floors (#157), and for the same reason.

## The source

`bluenviron/mediamtx:latest-ffmpeg` as a CI service, publishing an H264 test pattern into itself via
`runOnDemand`. `-g 15` keeps keyframes frequent so the muxer finds an IDR to start a clip on
quickly. The job installs the real runtime plugins, not just dev headers: `rtspsrc` (good),
`h264parse` (bad), `jpegenc`/`mp4mux` (good), `decodebin` (base), software decode (libav).

## Verified before shipping (2026-08-02)

Run locally against a real mediamtx container, both directions:

- **passes** against a live source: `media_smoke: RAN frames=16355 B, clip=125557 B` — a real JPEG
  and a real, finalised 125 KB mp4.
- **fails** against a dead one: `decode branch produced no JPEG within 30s`.

## Out of scope (v1)

- Hardware decode paths (`nvv4l2decoder`, `vtdec`, …). CI has no GPU; `gst_selftest` already
  refuses to advertise a decoder it cannot build.
- H265. The synthetic source is H264; the H265 branch differs only in parser/depay elements.
- Motion, tracking and face detection off the decode branch — this asserts the pipeline delivers
  frames, not what the detectors conclude from them.
