//! The rodio-backed audio device — the real implementation of [`Sfx`].
//!
//! Designed to NEVER panic and NEVER block the render loop: construction returns
//! `None` if there's no device, and every method degrades to silence on failure.
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

use super::domain::Sfx;
use super::levels::AudioLevels;
use crate::voice;

/// Number of `keyNN.wav` keystroke samples in `assets/` (key01..key08).
const KEY_SOUNDS: usize = 8;

/// Number of `working_NN.wav` samples in `assets/` (working_01..working_05).
const WORKING_SOUNDS: usize = 5;

/// A shared list of currently-playing one-shot/voice sinks. We keep them alive
/// here (instead of detaching) so the top bar can report whether audio is live;
/// finished sinks are pruned whenever the list is touched.
type Voices = Arc<Mutex<Vec<Sink>>>;

pub struct Audio {
    // Keep the stream alive for the program's lifetime, or playback stops.
    _stream: OutputStream,
    handle: OutputStreamHandle,
    // A dedicated sink for the looping boot hum so we can stop it cleanly.
    boot_sink: Sink,
    // A dedicated sink for the looping "working" sound during processing.
    working_sink: Sink,
    // Live one-shot/voice sinks, retained so `is_active` can see them.
    voices: Voices,
    // Rotating index into the keyNN.wav keystroke samples.
    key_index: AtomicUsize,
    // Rotating index into the working_NN.wav samples.
    working_index: AtomicUsize,
    assets: PathBuf,
    // Shared, live mixer levels (master/voice/ui/ambient + voice mute).
    levels: Arc<AudioLevels>,
}

impl Audio {
    pub fn new(levels: Arc<AudioLevels>) -> Option<Self> {
        // No device? `try_default` returns Err — we degrade to silence.
        let (stream, handle) = OutputStream::try_default().ok()?;
        let boot_sink = Sink::try_new(&handle).ok()?;
        let working_sink = Sink::try_new(&handle).ok()?;
        let assets = locate_assets();
        Some(Audio {
            _stream: stream,
            handle,
            boot_sink,
            working_sink,
            voices: Arc::new(Mutex::new(Vec::new())),
            key_index: AtomicUsize::new(0),
            working_index: AtomicUsize::new(0),
            assets,
            levels,
        })
    }

    fn load(&self, name: &str) -> Option<Decoder<BufReader<File>>> {
        let path = self.assets.join(name);
        let file = File::open(path).ok()?;
        Decoder::new(BufReader::new(file)).ok()
    }

    /// Retain a playing sink (pruning any that have finished) so it keeps
    /// playing and stays visible to `is_active`.
    fn track(voices: &Voices, sink: Sink) {
        let mut v = voices.lock().unwrap();
        v.retain(|s| !s.empty());
        v.push(sink);
    }

    /// Play the next keystroke sample (cycling key01.wav .. key08.wav so
    /// repeated keys don't sound identical) at the given volume.
    fn key_click(&self, amplify: f32) {
        let i = self.key_index.fetch_add(1, Ordering::Relaxed) % KEY_SOUNDS;
        self.one_shot(&format!("key{:02}.wav", i + 1), amplify);
    }

    /// Fire-and-forget one-shot, tracked so the audio indicator can see it. The
    /// sound's inherent `amplify` is scaled by the live UI mixer gain.
    fn one_shot(&self, name: &str, amplify: f32) {
        if let Some(src) = self.load(name) {
            if let Ok(sink) = Sink::try_new(&self.handle) {
                sink.append(src.amplify(amplify * self.levels.ui_gain()));
                Self::track(&self.voices, sink);
            }
        }
    }
}

impl Sfx for Audio {
    fn boot_loop(&self) {
        if let Some(src) = self.load("boot_loop.wav") {
            self.boot_sink
                .append(src.amplify(self.levels.ambient_gain()).repeat_infinite());
            self.boot_sink.play();
        }
    }
    fn beep(&self) {
        self.one_shot("beep.wav", 0.6);
    }
    fn ready(&self) {
        self.one_shot("ready.wav", 0.9);
    }
    fn keystroke(&self) {
        self.key_click(0.45);
    }
    fn type_tick(&self) {
        // Half the keystroke volume for the typewriter chatter.
        self.key_click(0.22);
    }
    fn set_working(&self, active: bool) {
        if !active {
            if !self.working_sink.empty() {
                self.working_sink.stop();
            }
            return;
        }
        // Keep the loop fed: when the current clip ends, queue the next sample
        // (cycling working_01..working_05) so it loops while processing.
        if self.working_sink.empty() {
            let i = self.working_index.fetch_add(1, Ordering::Relaxed) % WORKING_SOUNDS;
            if let Some(src) = self.load(&format!("working_{:02}.wav", i + 1)) {
                self.working_sink.append(src.amplify(0.1 * self.levels.ambient_gain()));
                self.working_sink.play();
            }
        }
    }
    fn hush(&self) {
        self.boot_sink.stop();
    }
    fn speak(&self, text: &str, on_ready: Box<dyn FnOnce() + Send>) {
        // Voice muted in the Ship Configurations? Skip the network call entirely
        // but still reveal the text right away.
        if !self.levels.voice_enabled() {
            on_ready();
            return;
        }
        // Clone the handle + voices list so the worker thread can own a sink. If
        // TTS isn't configured (no API key) or anything fails, this stays silent.
        let handle = self.handle.clone();
        let voices = self.voices.clone();
        let levels = self.levels.clone();
        let text = text.to_string();
        std::thread::spawn(move || {
            // Synthesize and queue playback. `on_ready` fires once the audio is
            // playing (or, on any failure, so the text still appears).
            if let Some(mp3) = voice::client::synthesize(&text) {
                if let Ok(src) = Decoder::new(std::io::Cursor::new(mp3)) {
                    if let Ok(sink) = Sink::try_new(&handle) {
                        sink.append(src.amplify(levels.voice_gain()));
                        Self::track(&voices, sink); // kept alive; plays to completion
                    }
                }
            }
            on_ready();
        });
    }
    fn is_active(&self) -> bool {
        let mut v = self.voices.lock().unwrap();
        v.retain(|s| !s.empty());
        // The boot hum / working loop run until stopped, so a non-empty sink
        // means audio is playing.
        !self.boot_sink.empty() || !self.working_sink.empty() || !v.is_empty()
    }
}

/// Look for `assets/` next to the executable first, then the CWD, so it works
/// both for `cargo run` and a copied-out release binary.
fn locate_assets() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("assets");
            if cand.is_dir() {
                return cand;
            }
        }
    }
    Path::new("assets").to_path_buf()
}
