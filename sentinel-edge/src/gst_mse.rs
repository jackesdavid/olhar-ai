//!
//! **byte for byte**, so the gateway proxy, the web player and the mobile WebView player are all
//! untouched: client sends `{"type":"mse","value":"<codecs it supports>"}`, we answer
//! `{"type":"mse","value":"video/mp4; codecs=\"…\""}` (fed straight to `addSourceBuffer`), then the
//! socket carries nothing but fMP4 bytes.
//!
//! **Where the H264 comes from.** [`crate::capture`]'s shared ingest — the edge dials each camera
//! **once** and `tee`s its H264 to every consumer, so this module owns no connection and no
//! pipeline. It is handed `mp4mux` output boxes and turns them into what a viewer needs.
//!
//! **Why `mp4mux` and not `cmafmux`.** `cmafmux` (gst-plugins-rs) was the obvious pick and it does
//! produce valid fMP4 — but it closes a fragment only on a **keyframe**, so its cadence is the
//! camera's GOP no matter what `fragment-duration` says. Measured against this camera: one fragment
//! 0.22s. That is a latency regression a viewer would feel. `mp4mux fragment-duration` fragments on
//! portability question the Jetson raised: `cmafmux` is **absent** from the JetPack 6 GStreamer
//! (1.20.3) and would have to be built and shipped for aarch64 and Android, while `mp4mux` is in
//! gst-plugins-good and present everywhere we deploy. And it wants no DTS, which `cmafmux` did.

use std::sync::Arc;
use tokio::sync::broadcast;

/// How much video each fragment holds, in **milliseconds** (`mp4mux`'s unit). The browser cannot
/// play what the muxer has not closed yet, so this is the latency floor: at 100ms the muxer closes
pub(crate) const FRAGMENT_MS: u32 = 100;

/// Assembles the MSE init segment out of the leading boxes `mp4mux` emits one per buffer.
///
/// `ftyp` and `moov` arrive as separate buffers, and a viewer needs both before any fragment. The
/// first `moof` is what ends the init segment and begins the live stream.
#[derive(Default)]
struct InitSegment {
    bytes: Vec<u8>,
    done: bool,
}

impl InitSegment {
    /// Absorb `bytes` if it is still part of the init segment; `false` once the stream has started,
    /// meaning the caller should forward this buffer to viewers instead.
    fn take_box(&mut self, bytes: &[u8]) -> bool {
        if self.done {
            return false;
        }
        if bytes.len() >= 8 && &bytes[4..8] == b"moof" {
            self.done = true;
            return false;
        }
        self.bytes.extend_from_slice(bytes);
        true
    }

    fn finished(&self) -> Option<Vec<u8>> {
        self.done.then(|| self.bytes.clone())
    }
}

/// Regroups `mp4mux`'s per-box buffers back into whole fragments, so each viewer gets **one
/// WebSocket message per fragment** instead of three.
///
/// `mp4mux` emits `moof`, then the 8-byte `mdat` header, then the payload — measured at 3 messages
/// `appendBuffer` in the player. That overhead lands hardest on the mobile WebView.
///
/// The `mdat` header declares the box's total size, so we know exactly where the fragment ends and
/// can emit it the moment its last byte arrives — coalescing costs **no** added latency, which
/// waiting for the next `moof` would have.
#[derive(Default)]
struct Fragment {
    bytes: Vec<u8>,
    /// Payload bytes still owed to the open `mdat`; 0 means we are not inside one.
    owed: usize,
}

/// Total size of the box starting at `b`, if it declares a usable one.
fn box_size(b: &[u8]) -> Option<usize> {
    let size = u32::from_be_bytes(b.get(..4)?.try_into().ok()?) as usize;
    (size >= 8).then_some(size)
}

impl Fragment {
    /// Feed one buffer; returns a complete fragment when this buffer closed one.
    fn push(&mut self, bytes: &[u8]) -> Option<Vec<u8>> {
        if self.owed > 0 {
            self.bytes.extend_from_slice(bytes);
            self.owed = self.owed.saturating_sub(bytes.len());
            return (self.owed == 0).then(|| finalize_fragment(std::mem::take(&mut self.bytes)));
        }
        let kind = bytes.get(4..8);
        // A new `moof` while something is still buffered means the previous fragment never declared
        // a size we could follow — flush it rather than gluing two fragments together.
        let flushed = (kind == Some(b"moof") && !self.bytes.is_empty())
            .then(|| finalize_fragment(std::mem::take(&mut self.bytes)));
        self.bytes.extend_from_slice(bytes);
        // Only a *sized* mdat tells us where the fragment ends. Without one we must wait for the
        // next `moof`, so an unsized mdat must not be mistaken for an already-complete one.
        if kind == Some(b"mdat") {
            if let Some(total) = box_size(bytes) {
                // The header names the whole box, of which we already hold `bytes.len()`.
                self.owed = total.saturating_sub(bytes.len());
                if self.owed == 0 {
                    return Some(finalize_fragment(std::mem::take(&mut self.bytes)));
                }
            }
        }
        flushed
    }
}

/// MSE-safe a finished `moof`+`mdat`: set `default-base-is-moof`, then collapse `mp4mux`'s
/// per-frame `trun` list into one. Safari forgives multiple `trun`s; Chrome's demuxer answers
/// `CHUNK_DEMUXER_ERROR_APPEND_FAILED: Failed to prepare video sample for decode`.
fn finalize_fragment(mut frag: Vec<u8>) -> Vec<u8> {
    ensure_default_base_is_moof(&mut frag);
    coalesce_truns(frag)
}

/// `tfhd` flag bit: base for `trun.data_offset` is the first byte of the enclosing `moof`
/// (ISO/IEC 14496-12). Without it, the offset is file-absolute — fine for a growing `.mp4` on
/// disk, fatal for MSE (init + live fragments, no single file byte position).
///
/// GStreamer 1.20's `mp4mux fragment-duration=… streamable=true` (Jetson / JetPack 6) does **not**
/// set this bit. Chrome then seeks sample data at absolute offsets inside the init segment →
/// `appendBuffer` "succeeds", the playhead never advances, Live stays black. Mac's newer
/// GStreamer often sets it; patching here keeps every box we ship MSE-safe regardless of muxer.
const TFHD_DEFAULT_BASE_IS_MOOF: u32 = 0x02_0000;

/// OR `default-base-is-moof` into every `tfhd` inside `frag` (a `moof`+`mdat` message). No layout
/// change — the flag adds no fields — so sizes stay valid.
fn ensure_default_base_is_moof(frag: &mut [u8]) {
    patch_tfhd_flags(frag);
}

fn patch_tfhd_flags(buf: &mut [u8]) {
    let mut i = 0;
    while i + 8 <= buf.len() {
        let size = match box_size(&buf[i..]) {
            Some(s) if i + s <= buf.len() => s,
            _ => break,
        };
        let kind = [buf[i + 4], buf[i + 5], buf[i + 6], buf[i + 7]];
        if &kind == b"tfhd" && size >= 12 {
            // FullBox: byte0 = version, bytes1..4 = flags (24-bit BE).
            let flags_off = i + 9;
            let flags =
                u32::from_be_bytes([0, buf[flags_off], buf[flags_off + 1], buf[flags_off + 2]]);
            let patched = flags | TFHD_DEFAULT_BASE_IS_MOOF;
            buf[flags_off] = ((patched >> 16) & 0xff) as u8;
            buf[flags_off + 1] = ((patched >> 8) & 0xff) as u8;
            buf[flags_off + 2] = (patched & 0xff) as u8;
        } else if &kind == b"moof" || &kind == b"traf" {
            patch_tfhd_flags(&mut buf[i + 8..i + size]);
        }
        // Never walk into `mdat` payload (NAL bytes can look like box headers).
        if &kind == b"mdat" {
            break;
        }
        i += size;
    }
}

/// Walk top-level (or nested) boxes; yields `(type, body)` where body excludes the 8-byte header.
fn iter_boxes(buf: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> + '_ {
    let mut i = 0usize;
    std::iter::from_fn(move || {
        while i + 8 <= buf.len() {
            let size = box_size(&buf[i..])?;
            if size < 8 || i + size > buf.len() {
                return None;
            }
            let typ = &buf[i + 4..i + 8];
            let body = &buf[i + 8..i + size];
            i += size;
            return Some((typ, body));
        }
        None
    })
}

fn make_box(typ: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&(8u32 + body.len() as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(body);
    out
}

/// One media sample collected from a `trun` (duration / size / flags; CTS ignored — our cameras
/// are B-frame-free so mp4mux rarely emits it, and Chrome is fine without per-sample CTS).
struct Sample {
    duration: u32,
    size: u32,
    flags: u32,
}

/// Collapse every `trun` inside the fragment's `traf` into a single run. `mp4mux fragment-duration`
/// emits one `trun` per input buffer (often per frame); Chrome MSE rejects that layout even when
/// the first sample is an IDR. Returns the original bytes if the shape is already fine or opaque.
fn coalesce_truns(frag: Vec<u8>) -> Vec<u8> {
    let Some(rewritten) = try_coalesce_truns(&frag) else {
        return frag;
    };
    rewritten
}

fn try_coalesce_truns(frag: &[u8]) -> Option<Vec<u8>> {
    let mut moof_body = None;
    let mut mdat_payload = None;
    for (typ, body) in iter_boxes(frag) {
        match typ {
            b"moof" => moof_body = Some(body),
            b"mdat" => mdat_payload = Some(body),
            _ => {}
        }
    }
    let moof_body = moof_body?;
    let mdat_payload = mdat_payload?;

    let mut mfhd = None;
    let mut traf_body = None;
    for (typ, body) in iter_boxes(moof_body) {
        match typ {
            b"mfhd" => mfhd = Some(body),
            b"traf" => traf_body = Some(body),
            _ => {}
        }
    }
    let mfhd = mfhd?;
    let traf_body = traf_body?;

    let mut tfhd = None;
    let mut tfdt = None;
    let mut truns: Vec<&[u8]> = Vec::new();
    for (typ, body) in iter_boxes(traf_body) {
        match typ {
            b"tfhd" => tfhd = Some(body),
            b"tfdt" => tfdt = Some(body),
            b"trun" => truns.push(body),
            _ => {}
        }
    }
    let tfhd = tfhd?;
    let tfdt = tfdt?;
    if truns.len() <= 1 {
        return None; // already a single run (or empty — leave alone)
    }

    // Default sample flags from tfhd, when present — samples that omit per-sample flags inherit it.
    let tfhd_flags = u32::from_be_bytes([0, tfhd[1], tfhd[2], tfhd[3]]);
    let mut off = 8usize; // version/flags(4) + track_id(4)
    if tfhd_flags & 0x1 != 0 {
        off += 8;
    }
    if tfhd_flags & 0x2 != 0 {
        off += 4;
    }
    if tfhd_flags & 0x8 != 0 {
        off += 4;
    }
    if tfhd_flags & 0x10 != 0 {
        off += 4;
    }
    let def_flags = if tfhd_flags & 0x20 != 0 {
        u32::from_be_bytes(tfhd.get(off..off + 4)?.try_into().ok()?)
    } else {
        0
    };

    let mut samples: Vec<Sample> = Vec::new();
    for tr in &truns {
        if tr.len() < 8 {
            return None;
        }
        let fl = u32::from_be_bytes([0, tr[1], tr[2], tr[3]]);
        let sc = u32::from_be_bytes(tr[4..8].try_into().ok()?) as usize;
        let mut o = 8usize;
        if fl & 0x1 != 0 {
            o += 4;
        } // data_offset
        let mut first_flags: Option<u32> = None;
        if fl & 0x4 != 0 {
            first_flags = Some(u32::from_be_bytes(tr.get(o..o + 4)?.try_into().ok()?));
            o += 4;
        }
        for n in 0..sc {
            let mut duration = 0u32;
            let mut size = 0u32;
            let mut flags = def_flags;
            if fl & 0x100 != 0 {
                duration = u32::from_be_bytes(tr.get(o..o + 4)?.try_into().ok()?);
                o += 4;
            }
            if fl & 0x200 != 0 {
                size = u32::from_be_bytes(tr.get(o..o + 4)?.try_into().ok()?);
                o += 4;
            }
            if fl & 0x400 != 0 {
                flags = u32::from_be_bytes(tr.get(o..o + 4)?.try_into().ok()?);
                o += 4;
            } else if n == 0 {
                if let Some(f) = first_flags {
                    flags = f;
                }
            }
            if fl & 0x800 != 0 {
                o += 4;
            } // composition time offset — dropped
            samples.push(Sample {
                duration,
                size,
                flags,
            });
        }
    }
    if samples.is_empty() {
        return None;
    }
    // Keyframe AU: clear the non-sync bit so Chrome treats the first decodable sample as a sync.
    if fragment_has_keyframe(frag) {
        samples[0].flags &= !0x0001_0000;
        // sample_depends_on = 2 (does not depend on others)
        samples[0].flags = (samples[0].flags & !0x0300_0000) | 0x0200_0000;
    }

    // New tfhd: only default-base-is-moof + track_id (strip default-sample-flags — per-sample now).
    let track_id = u32::from_be_bytes(tfhd.get(4..8)?.try_into().ok()?);
    let mut new_tfhd = vec![0u8, 0x02, 0x00, 0x00];
    new_tfhd.extend_from_slice(&track_id.to_be_bytes());

    // trun flags: data-offset | sample-duration | sample-size | sample-flags
    const TRUN_FLAGS: u32 = 0x701;
    let build = |data_offset: i32| -> Vec<u8> {
        let mut trun = Vec::with_capacity(8 + 4 + samples.len() * 12);
        trun.push(0); // version
        trun.extend_from_slice(&TRUN_FLAGS.to_be_bytes()[1..]);
        trun.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        trun.extend_from_slice(&data_offset.to_be_bytes());
        for s in &samples {
            trun.extend_from_slice(&s.duration.to_be_bytes());
            trun.extend_from_slice(&s.size.to_be_bytes());
            trun.extend_from_slice(&s.flags.to_be_bytes());
        }
        let traf = make_box(
            b"traf",
            &[
                make_box(b"tfhd", &new_tfhd),
                make_box(b"tfdt", tfdt),
                make_box(b"trun", &trun),
            ]
            .concat(),
        );
        let moof = make_box(b"moof", &[make_box(b"mfhd", mfhd), traf].concat());
        let mut out = moof;
        out.extend_from_slice(&make_box(b"mdat", mdat_payload));
        out
    };

    // data_offset is relative to the moof start (default-base-is-moof) → moof_size + 8.
    let mut data_offset = 0i32;
    let mut out = build(data_offset);
    for _ in 0..3 {
        let moof_size = u32::from_be_bytes(out.get(..4)?.try_into().ok()?) as i32;
        data_offset = moof_size + 8;
        out = build(data_offset);
    }
    Some(out)
}

/// Whether this fragment's `mdat` carries a random-access NAL (H264 IDR or H265 IRAP). New MSE
/// viewers must wait for one — Chrome will not decode a P-frame cold-start, and Safari's leniency
/// is what made "works in Safari / black in Chrome" look like a codec problem.
pub(crate) fn fragment_has_keyframe(frag: &[u8]) -> bool {
    let Some(mdat) = iter_boxes(frag).find_map(|(t, b)| (t == b"mdat").then_some(b)) else {
        return false;
    };
    let mut i = 0usize;
    while i + 4 <= mdat.len() {
        let n = u32::from_be_bytes(mdat[i..i + 4].try_into().unwrap()) as usize;
        let nal = i + 4;
        if n == 0 || nal + n > mdat.len() {
            break;
        }
        let b0 = mdat[nal];
        // H264: nal_unit_type in low 5 bits — 5 = IDR.
        if b0 & 0x1f == 5 {
            return true;
        }
        // H265: nal_unit_type in bits 1..6 — 16..=23 are IRAP (BLA/IDR/CRA).
        let hevc = (b0 >> 1) & 0x3f;
        if (16..=23).contains(&hevc) {
            return true;
        }
        i = nal + n;
    }
    false
}

/// The `codecs=` string MSE needs, read from whichever decoder-config box the init segment
/// carries: `avcC` (H264) or `hvcC` (H265) — a camera's codec (probed in `capture.rs`) decides
/// which one `mp4mux` emits. `None` here means the mux branch is producing fragments but MSE has
/// no codec string to hand the player, which is just as fatal to Live as no fragment at all
/// (`Feed::subscribe` withholds until both exist) — this was H265's actual remaining gap after
/// the keyframe fix: `mp4mux` fragments fine, but every init segment was silently unusable.
fn codecs_from_init(init: &[u8]) -> Option<String> {
    if let Some(pos) = init.windows(4).position(|w| w == b"avcC") {
        return avc_codec_string(init.get(pos + 4..)?);
    }
    if let Some(pos) = init.windows(4).position(|w| w == b"hvcC") {
        return hevc_codec_string(init.get(pos + 4..)?);
    }
    None
}

/// `avc1.` + profile / constraint flags / level as hex, from an `AVCDecoderConfigurationRecord`
/// (`01 64 00 16` → `avc1.640016`).
fn avc_codec_string(avcc: &[u8]) -> Option<String> {
    let b = avcc.get(..4)?; // configurationVersion, profile, compat, level
    Some(format!("avc1.{:02x}{:02x}{:02x}", b[1], b[2], b[3]))
}

/// `hvc1.` + profile space/idc + '.' + compatibility flags (bit-reversed, hex) + '.' + tier/level
/// + '.' + constraint flags (trailing zero bytes trimmed, hex), from an
/// `HEVCDecoderConfigurationRecord` — the layout ISO/IEC 14496-15 §8.3.3.1 and the codec-string
/// grammar in Annex E.3 both define. e.g. profile_idc=1, tier=Main, level_idc=153,
/// constraint=0xB0.. → `hvc1.1.6.L153.B0`.
fn hevc_codec_string(hvcc: &[u8]) -> Option<String> {
    // configurationVersion (1) + profile byte (1) + compatibility flags (4) + constraint flags
    // (6) + level_idc (1) = 13 bytes before the rest of the record.
    let b = hvcc.get(1..13)?;
    let profile_space = (b[0] >> 6) & 0x3;
    let tier_flag = (b[0] >> 5) & 0x1;
    let profile_idc = b[0] & 0x1f;
    let compat = u32::from_be_bytes([b[1], b[2], b[3], b[4]]).reverse_bits();
    let constraint = &b[5..11];
    let level_idc = b[11];

    let space = match profile_space {
        1 => "A",
        2 => "B",
        3 => "C",
        _ => "",
    };
    let tier = if tier_flag == 1 { "H" } else { "L" };
    let end = constraint
        .iter()
        .rposition(|&b| b != 0)
        .map_or(0, |i| i + 1);
    let mut s = format!("hvc1.{space}{profile_idc}.{compat:X}.{tier}{level_idc}");
    for byte in &constraint[..end] {
        s.push_str(&format!(".{byte:X}"));
    }
    Some(s)
}

/// The live fMP4 feed of one camera, fed by the shared ingest pipeline in [`crate::capture`].
///
/// It is **fed**, not owned: the migration's whole point is that a camera is dialled once and its
/// H264 `tee`'d to every consumer, so the muxer lives on a branch of that one pipeline rather than
/// opening a second connection of its own.
pub struct Feed {
    init: InitSegment,
    fragment: Fragment,
    /// Derived from the init segment's `avcC` the moment it completes.
    mime: Option<String>,
    tx: broadcast::Sender<Arc<Vec<u8>>>,
}

impl Default for Feed {
    fn default() -> Self {
        // `mp4mux` emits a box per buffer (~50/s at 15fps), so this is roughly 10s of slack: enough
        // to ride out a stalled viewer, short enough that one which fell further behind is better
        // served by being dropped and resynced than by playing minutes-old video.
        let (tx, _) = broadcast::channel(512);
        Feed {
            init: InitSegment::default(),
            fragment: Fragment::default(),
            mime: None,
            tx,
        }
    }
}

impl Feed {
    /// Absorb one `mp4mux` output buffer: init segment first, then whole fragments to viewers.
    pub fn on_box(&mut self, bytes: &[u8]) {
        if self.init.take_box(bytes) {
            return;
        }
        if self.mime.is_none() {
            if let Some(init) = self.init.finished() {
                self.mime = codecs_from_init(&init).map(|c| format!("video/mp4; codecs=\"{c}\""));
            }
        }
        if let Some(whole) = self.fragment.push(bytes) {
            let _ = self.tx.send(Arc::new(whole));
        }
    }

    /// What a newly-attached viewer needs: the init segment, the MIME for `addSourceBuffer`, and the
    /// live fragments from here on. `None` until the first keyframe has closed the init segment.
    pub fn subscribe(&self) -> Option<(Vec<u8>, String, broadcast::Receiver<Arc<Vec<u8>>>)> {
        Some((
            self.init.finished()?,
            self.mime.clone()?,
            self.tx.subscribe(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `codecs=` string is what decides whether the browser will play the stream at all, and it
    /// is derived by byte-offset into a box we do not control — worth pinning.
    #[test]
    fn reads_the_codec_string_out_of_avcc() {
        let mut init = b"\x00\x00\x00\x08ftypfake\x00\x00\x00\x0eavcC".to_vec();
        init.extend_from_slice(&[0x01, 0x64, 0x00, 0x16]); // High profile, level 2.2
        assert_eq!(codecs_from_init(&init).as_deref(), Some("avc1.640016"));
    }

    /// `mp4mux` splits the init segment across buffers, so the assembler decides whether a viewer
    /// gets a usable `moov` at all — and it must stop at exactly the right buffer.
    #[test]
    fn init_segment_is_every_box_before_the_first_moof() {
        let mut init = InitSegment::default();
        assert!(init.take_box(b"\x00\x00\x00\x20ftyp...."));
        assert!(init.take_box(b"\x00\x00\x03\x46moovbody"));
        assert_eq!(init.finished(), None, "not closed until a moof arrives");
        assert!(
            !init.take_box(b"\x00\x00\x00\x60moofdata"),
            "moof belongs to the live stream"
        );
        let got = init.finished().expect("closed by the moof");
        assert_eq!(got, b"\x00\x00\x00\x20ftyp....\x00\x00\x03\x46moovbody");
        assert!(
            !init.take_box(b"\x00\x00\x00\x08mdat"),
            "later boxes are live too"
        );
    }

    fn boxed(kind: &[u8], size: u32, body: &[u8]) -> Vec<u8> {
        let mut b = size.to_be_bytes().to_vec();
        b.extend_from_slice(kind);
        b.extend_from_slice(body);
        b
    }

    /// The whole point of coalescing is that a fragment is emitted the instant its last byte lands —
    /// not one fragment late. The mdat size field is what makes that possible, so pin the arithmetic.
    #[test]
    fn a_fragment_is_emitted_as_soon_as_its_mdat_is_full() {
        let mut f = Fragment::default();
        assert_eq!(f.push(&boxed(b"moof", 16, b"moofbody")), None);
        // mdat declares 8 header + 12 payload; the header buffer alone must not close it.
        assert_eq!(f.push(&boxed(b"mdat", 20, b"")), None);
        assert_eq!(f.push(b"1234"), None, "still owed 8 bytes");
        let whole = f
            .push(b"56789012")
            .expect("last payload byte closes the fragment");
        assert_eq!(whole.len(), 16 + 8 + 12);
        assert_eq!(&whole[4..8], b"moof");
    }

    /// An mdat with no usable size must not swallow the following fragments into one message.
    #[test]
    fn an_unsized_mdat_falls_back_to_flushing_on_the_next_moof() {
        let mut f = Fragment::default();
        assert_eq!(f.push(&boxed(b"moof", 16, b"moofbody")), None);
        assert_eq!(f.push(&boxed(b"mdat", 0, b"")), None);
        assert_eq!(f.push(b"payload"), None);
        let first = f
            .push(&boxed(b"moof", 16, b"moofbody"))
            .expect("next moof flushes it");
        assert_eq!(&first[4..8], b"moof");
        assert!(first.ends_with(b"payload"));
    }

    #[test]
    fn no_avcc_or_hvcc_means_no_codec_string() {
        assert_eq!(codecs_from_init(b"\x00\x00\x00\x08ftypfake"), None);
    }

    /// Main profile, level 5.1 (level_idc 153), one non-zero constraint byte (0xB0) — matches the
    /// `hvc1.1.6.L153.B0` example that motivated this fix (H265 cameras with an otherwise-healthy
    /// mux branch: `mp4mux` fragmented fine, but the init segment carries `hvcC` not `avcC`, so the
    /// old `avcC`-only reader left `codecs_from_init` returning `None` forever).
    #[test]
    fn reads_the_codec_string_out_of_hvcc() {
        let mut hvcc = b"\x00\x00\x00\x17hvcC".to_vec();
        hvcc.push(0x01); // configurationVersion
        hvcc.push(0x01); // profile_space=0, tier=0 (Main), profile_idc=1
        hvcc.extend_from_slice(&[0x60, 0x00, 0x00, 0x00]); // compatibility flags, pre-reverse
        hvcc.extend_from_slice(&[0xB0, 0x00, 0x00, 0x00, 0x00, 0x00]); // constraint flags
        hvcc.push(153); // level_idc = 153 (level 5.1)
        assert_eq!(codecs_from_init(&hvcc).as_deref(), Some("hvc1.1.6.L153.B0"));
    }

    #[test]
    fn hvcc_drops_trailing_zero_constraint_bytes_but_keeps_interior_ones() {
        let mut hvcc = b"\x00\x00\x00\x17hvcC".to_vec();
        hvcc.push(0x01);
        hvcc.push(0x02); // profile_idc=2
        hvcc.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        hvcc.extend_from_slice(&[0x00, 0x0A, 0x00, 0x00, 0x00, 0x00]); // non-zero in the middle
        hvcc.push(93);
        assert_eq!(codecs_from_init(&hvcc).as_deref(), Some("hvc1.2.0.L93.0.A"));
    }

    /// Jetson GStreamer 1.20's mp4mux emits `tfhd` without `default-base-is-moof`; we must set it
    /// or Chrome MSE treats `trun.data_offset` as a file-absolute seek and never paints a frame.
    #[test]
    fn sets_default_base_is_moof_on_tfhd() {
        // moof { mfhd, traf { tfhd flags=0x000020, track=1 } } + mdat header (no payload needed)
        let mut frag = Vec::new();
        // tfhd: ver=0, flags=0x000020, track_id=1  → 16-byte box
        let tfhd = {
            let mut b = vec![0u8, 0x00, 0x00, 0x20, 0, 0, 0, 1];
            let mut box_ = (8 + b.len() as u32).to_be_bytes().to_vec();
            box_.extend_from_slice(b"tfhd");
            box_.append(&mut b);
            box_
        };
        let traf = {
            let mut box_ = (8 + tfhd.len() as u32).to_be_bytes().to_vec();
            box_.extend_from_slice(b"traf");
            box_.extend_from_slice(&tfhd);
            box_
        };
        let mfhd = boxed(b"mfhd", 16, &[0, 0, 0, 0, 0, 0, 0, 1]);
        let moof_body_len = mfhd.len() + traf.len();
        let mut moof = (8 + moof_body_len as u32).to_be_bytes().to_vec();
        moof.extend_from_slice(b"moof");
        moof.extend_from_slice(&mfhd);
        moof.extend_from_slice(&traf);
        frag.extend_from_slice(&moof);
        frag.extend_from_slice(&boxed(b"mdat", 8, &[]));

        ensure_default_base_is_moof(&mut frag);
        // flags at tfhd FullBox bytes 1..3 must now include 0x020000
        let tfhd_at = frag.windows(4).position(|w| w == b"tfhd").expect("tfhd");
        let flags =
            u32::from_be_bytes([0, frag[tfhd_at + 5], frag[tfhd_at + 6], frag[tfhd_at + 7]]);
        assert_eq!(flags & TFHD_DEFAULT_BASE_IS_MOOF, TFHD_DEFAULT_BASE_IS_MOOF);
        assert_eq!(flags & 0x20, 0x20, "pre-existing flags preserved");
    }
}
