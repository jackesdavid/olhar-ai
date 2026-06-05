//! Live RTSP playback: an ffmpeg subprocess streaming raw grayscale frames over
//! a pipe into the monochrome-phosphor renderer. One process + one reader thread
//! per open stream; killing the child on `Drop` guarantees no zombie ffmpeg.
pub mod client;
pub mod domain;
pub mod view;

pub use client::SpawnParams;
pub use domain::{FrameSize, LiveStream, DEFAULT_FPS};
