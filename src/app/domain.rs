//! Aggregate application state.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use crate::agent::tools::ToolUiEvent;
use crate::agent::AgentOutcome;
use crate::audio::{AudioLevels, Sfx};
use crate::boot::BootSequence;
use crate::chat::Conversation;
use crate::config::{ConfigScreen, Settings};
use crate::device::DeviceDetail;
use crate::listen::{ListenState, SttEvent};
use crate::menu::Menu;
use crate::scan::{Device, ScanState};
use crate::stream::LiveStream;

/// Which screen is currently shown. The app opens on `Boot`, hands off to the
/// home `Menu`, and from there opens one of the feature screens. `Esc` from a
/// feature returns to its parent (`DeviceDetail` → `Scan` → `Menu`); `Esc` at
/// the menu quits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Boot,
    Menu,
    Scan,
    DeviceDetail,
    Chat,
    About,
    Config,
    Alerts,
}

/// Results from scan/camera background workers, drained by the main loop.
pub enum ScanEvent {
    /// A discovery sweep finished with these devices.
    Discovered(Vec<Device>),
    /// A credential test finished; `result` is the RTSP URL on success.
    Credentials {
        ip: String,
        username: String,
        password: String,
        result: Result<String, String>,
    },
    /// A snapshot finished; the decoded grayscale image on success.
    Snapshot(Result<image::GrayImage, String>),
    /// A "describe image" finished: the snapshot to show plus Bishop's
    /// description of what's visible, or an error message.
    Describe(Result<(image::GrayImage, String), String>),
}

/// A line of Bishop's speech to reveal once its audio is ready to play. The
/// variant decides how it's recorded: a `Greeting` is log-only, a `Reply` also
/// goes into the API history and clears the pending state.
pub enum Say {
    Greeting(String),
    Reply(String),
}

/// A shared, thread-safe "Bishop is processing" indicator that any module can
/// drive. Clone a handle wherever work starts, call [`Activity::begin`], and the
/// returned guard clears the flag when dropped — even across threads. The top
/// bar shows a thinking animation whenever the count is greater than zero.
#[derive(Clone, Default)]
pub struct Activity {
    count: Arc<AtomicUsize>,
}

impl Activity {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a unit of work as in progress. Hold the guard for its duration.
    pub fn begin(&self) -> ActivityGuard {
        self.count.fetch_add(1, Ordering::SeqCst);
        ActivityGuard { count: self.count.clone() }
    }

    /// Whether any work is currently in progress.
    pub fn is_busy(&self) -> bool {
        self.count.load(Ordering::SeqCst) > 0
    }
}

/// Decrements the [`Activity`] count when dropped.
pub struct ActivityGuard {
    count: Arc<AtomicUsize>,
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The whole app: the current screen plus one piece of state per feature slice
/// and the audio backend they all share.
pub struct App {
    pub screen: Screen,
    pub boot: BootSequence,
    pub menu: Menu,
    pub scan: ScanState,
    /// The open device-detail screen, if any.
    pub detail: Option<DeviceDetail>,
    pub convo: Conversation,
    pub sfx: Box<dyn Sfx>,
    /// User-tunable settings (volumes, voice toggle, teletype speed, STT
    /// auto-send), persisted to `config.json`.
    pub settings: Settings,
    /// The live mixer shared with the audio backend; the config screen pushes
    /// changes here so volume/mute take effect immediately.
    pub levels: Arc<AudioLevels>,
    /// Cursor state for the Ship Configurations screen.
    pub config: ConfigScreen,
    /// Background Claude turns land here (final text + the history to splice),
    /// each carrying the processing guard so the top-bar "PROCESSING" stays lit
    /// until the reply's audio is ready.
    pub reply_tx: Sender<(AgentOutcome, ActivityGuard)>,
    pub reply_rx: Receiver<(AgentOutcome, ActivityGuard)>,
    /// Speech ready to be shown (once its audio starts); drained each tick.
    pub display_tx: Sender<Say>,
    pub display_rx: Receiver<Say>,
    /// Scan / camera background results, drained each tick.
    pub scan_tx: Sender<ScanEvent>,
    pub scan_rx: Receiver<ScanEvent>,
    /// UI side-effects requested by chat tools (snapshot image, list block, …).
    pub tool_ui_tx: Sender<ToolUiEvent>,
    pub tool_ui_rx: Receiver<ToolUiEvent>,
    /// A camera snapshot shown as a modal over the chat (Esc/Enter to close).
    pub snapshot_modal: Option<crate::ui::image::CameraImage>,
    /// The active live RTSP stream (one at a time). Shown in the device-detail
    /// right column or, when `live_modal` is set, as a modal over the chat.
    pub live: Option<LiveStream>,
    pub live_modal: bool,
    /// Speech-to-text: the F9 "talk to Bishop" capture state and its result
    /// channel, drained each tick.
    pub listen: ListenState,
    pub stt_tx: Sender<SttEvent>,
    pub stt_rx: Receiver<SttEvent>,
    /// Trusted Telegram notification targets, shared with the pairing poller and
    /// the agent thread (the `send_notification` tool).
    pub notification: crate::notification::NotificationStore,
    /// Cursor + status for the ALERTS management screen.
    pub alerts: crate::notification::AlertsScreen,
    /// Background test-alert results land here as a status line, drained each tick.
    pub alert_tx: Sender<String>,
    pub alert_rx: Receiver<String>,
    /// App-wide "processing" indicator, shared with background workers.
    pub activity: Activity,
}
