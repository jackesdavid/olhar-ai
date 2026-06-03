//! Microphone capture. With the `audio` feature this opens the default input
//! device via cpal, accumulates samples on the audio thread, and tracks a live
//! amplitude for the UI meter; [`Recording::finish`] encodes the captured audio
//! as an in-memory 16-bit PCM WAV. Without the feature, capture is a no-op stub
//! so the rest of the app compiles and F9 simply does nothing.
//!
//! The `Recording` stays on the main thread (cpal streams aren't `Send`); only
//! the finished WAV bytes are handed to the transcription worker.

#[cfg(feature = "audio")]
pub use imp::{start, Recording};
#[cfg(not(feature = "audio"))]
pub use stub::{start, Recording};

#[cfg(feature = "audio")]
mod imp {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::SampleFormat;

    /// An in-progress microphone capture. Dropping it stops the stream.
    pub struct Recording {
        _stream: cpal::Stream,
        samples: Arc<Mutex<Vec<i16>>>,
        sample_rate: u32,
        channels: u16,
    }

    impl Recording {
        /// Stop capturing and encode the audio as a mono 16-bit PCM WAV.
        pub fn finish(self) -> Vec<u8> {
            let interleaved = self.samples.lock().map(|v| v.clone()).unwrap_or_default();
            let mono = downmix(&interleaved, self.channels);
            encode_wav(&mono, self.sample_rate)
        }
    }

    /// Begin capturing from the default input device. Returns `None` if there is
    /// no microphone or the stream can't be built.
    pub fn start(level: Arc<AtomicU32>) -> Option<Recording> {
        let host = cpal::default_host();
        let device = host.default_input_device()?;
        let supported = device.default_input_config().ok()?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let channels = config.channels;
        let sample_rate = config.sample_rate.0;

        let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
        let buf = samples.clone();
        let err_fn = |_e| {}; // ignore stream errors; we degrade to whatever we got

        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    let mut peak = 0.0f32;
                    if let Ok(mut v) = buf.lock() {
                        for &s in data {
                            peak = peak.max(s.abs());
                            v.push((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                        }
                    }
                    store_level(&level, peak);
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let mut peak = 0.0f32;
                    if let Ok(mut v) = buf.lock() {
                        for &s in data {
                            peak = peak.max((s as f32 / i16::MAX as f32).abs());
                            v.push(s);
                        }
                    }
                    store_level(&level, peak);
                },
                err_fn,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    let mut peak = 0.0f32;
                    if let Ok(mut v) = buf.lock() {
                        for &s in data {
                            let centered = s as i32 - 32768;
                            peak = peak.max((centered as f32 / i16::MAX as f32).abs());
                            v.push(centered as i16);
                        }
                    }
                    store_level(&level, peak);
                },
                err_fn,
                None,
            ),
            _ => return None,
        }
        .ok()?;

        stream.play().ok()?;
        Some(Recording {
            _stream: stream,
            samples,
            sample_rate,
            channels,
        })
    }

    /// Store a 0..1 level (f32 bits) for the UI meter, lightly smoothed toward
    /// the new peak so the bar doesn't strobe.
    fn store_level(level: &AtomicU32, peak: f32) {
        let prev = f32::from_bits(level.load(Ordering::Relaxed));
        let next = (prev * 0.6 + peak * 0.4).clamp(0.0, 1.0);
        level.store(next.to_bits(), Ordering::Relaxed);
    }

    /// Average interleaved channels down to a single mono track.
    fn downmix(interleaved: &[i16], channels: u16) -> Vec<i16> {
        if channels <= 1 {
            return interleaved.to_vec();
        }
        let ch = channels as usize;
        interleaved
            .chunks(ch)
            .map(|frame| {
                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                (sum / frame.len() as i32) as i16
            })
            .collect()
    }

    /// Write a minimal 16-bit PCM mono WAV (44-byte header + samples).
    fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let byte_rate = sample_rate * 2; // mono * 16-bit
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for &s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn wav_header_is_well_formed() {
            let wav = encode_wav(&[0, 1, -1, 100], 16000);
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..12], b"WAVE");
            assert_eq!(&wav[36..40], b"data");
            // 44-byte header + 4 samples * 2 bytes.
            assert_eq!(wav.len(), 44 + 8);
            // data chunk length field matches the sample bytes.
            let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
            assert_eq!(data_len, 8);
        }

        #[test]
        fn downmix_averages_stereo() {
            // two frames of stereo: (10,20) -> 15, (−10,−30) -> −20
            let mono = downmix(&[10, 20, -10, -30], 2);
            assert_eq!(mono, vec![15, -20]);
        }
    }
}

#[cfg(not(feature = "audio"))]
mod stub {
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;

    /// No-op capture used when the `audio` feature is off.
    pub struct Recording;

    impl Recording {
        pub fn finish(self) -> Vec<u8> {
            Vec::new()
        }
    }

    pub fn start(_level: Arc<AtomicU32>) -> Option<Recording> {
        None
    }
}
