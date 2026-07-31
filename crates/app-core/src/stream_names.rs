//! Canonical per-camera stream names: `cam_<ip>` for the preview/substream, `cam_<ip>_hd` for main.
//!
//! `capture` all address a camera by this name, so it is the live wire contract between them. The
//! sidecar is gone; the naming stays, under a name that says what it is.

/// `cam_10_0_0_7` — the preview/substream name for a camera IP.
pub fn stream_name(ip: &str) -> String {
    format!("cam_{}", ip.replace('.', "_"))
}

/// `cam_10_0_0_7_hd` — the main (highest-resolution) stream name for a camera IP.
pub fn stream_name_hd(ip: &str) -> String {
    format!("{}_hd", stream_name(ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_ip_with_dots_replaced() {
        assert_eq!(stream_name("192.168.0.7"), "cam_192_168_0_7");
        assert_eq!(stream_name_hd("192.168.0.7"), "cam_192_168_0_7_hd");
    }
}
