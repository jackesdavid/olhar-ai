//! Live **fMP4 over WebSocket** (card #138, touchpoint G2) — the go2rtc `/api/ws` replacement.
//!
//! This is the piece that lets the edge serve live video itself. It keeps go2rtc's wire contract
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
//! every **2.0s**, first frame after 5.6s, where go2rtc delivers one every **0.07s** starting at
//! 0.22s. That is a latency regression a viewer would feel. `mp4mux fragment-duration` fragments on
//! a timer instead — measured at **17 fragments/s**, matching go2rtc. It also settles the
//! portability question the Jetson raised: `cmafmux` is **absent** from the JetPack 6 GStreamer
//! (1.20.3) and would have to be built and shipped for aarch64 and Android, while `mp4mux` is in
//! gst-plugins-good and present everywhere we deploy. And it wants no DTS, which `cmafmux` did.

use std::sync::Arc;
use tokio::sync::broadcast;

/// How much video each fragment holds, in **milliseconds** (`mp4mux`'s unit). The browser cannot
/// play what the muxer has not closed yet, so this is the latency floor: at 100ms the muxer closes
/// a fragment per frame at any sane framerate, which is what go2rtc does today.
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
/// per fragment against go2rtc's 1, i.e. ~150 frames/s on the socket, each one an `onmessage` and an
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
            return (self.owed == 0).then(|| std::mem::take(&mut self.bytes));
        }
        let kind = bytes.get(4..8);
        // A new `moof` while something is still buffered means the previous fragment never declared
        // a size we could follow — flush it rather than gluing two fragments together.
        let flushed = (kind == Some(b"moof") && !self.bytes.is_empty())
            .then(|| std::mem::take(&mut self.bytes));
        self.bytes.extend_from_slice(bytes);
        // Only a *sized* mdat tells us where the fragment ends. Without one we must wait for the
        // next `moof`, so an unsized mdat must not be mistaken for an already-complete one.
        if kind == Some(b"mdat") {
            if let Some(total) = box_size(bytes) {
                // The header names the whole box, of which we already hold `bytes.len()`.
                self.owed = total.saturating_sub(bytes.len());
                if self.owed == 0 {
                    return Some(std::mem::take(&mut self.bytes));
                }
            }
        }
        flushed
    }
}

/// The `codecs=` string MSE needs, read from the `avcC` box in the init segment:
/// `avc1.` + profile / constraint flags / level as hex (`01 64 00 16` → `avc1.640016`).
fn codecs_from_init(init: &[u8]) -> Option<String> {
    let pos = init.windows(4).position(|w| w == b"avcC")?;
    let b = init.get(pos + 4..pos + 8)?; // configurationVersion, profile, compat, level
    Some(format!("avc1.{:02x}{:02x}{:02x}", b[1], b[2], b[3]))
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
        Some((self.init.finished()?, self.mime.clone()?, self.tx.subscribe()))
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
        assert!(!init.take_box(b"\x00\x00\x00\x60moofdata"), "moof belongs to the live stream");
        let got = init.finished().expect("closed by the moof");
        assert_eq!(got, b"\x00\x00\x00\x20ftyp....\x00\x00\x03\x46moovbody");
        assert!(!init.take_box(b"\x00\x00\x00\x08mdat"), "later boxes are live too");
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
        let whole = f.push(b"56789012").expect("last payload byte closes the fragment");
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
        let first = f.push(&boxed(b"moof", 16, b"moofbody")).expect("next moof flushes it");
        assert_eq!(&first[4..8], b"moof");
        assert!(first.ends_with(b"payload"));
    }

    #[test]
    fn no_avcc_means_no_codec_string() {
        assert_eq!(codecs_from_init(b"\x00\x00\x00\x08ftypfake"), None);
    }
}

