//! Application lifecycle: construction, per-tick updates, key routing, and the
//! main run loop.
use std::time::{Duration, Instant};

use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    DefaultTerminal,
};

use super::domain::{Activity, ActivityGuard, App, Say, ScanEvent, Screen};
use super::view;
use crate::agent::tools::{ToolContext, ToolRegistry, ToolUiEvent};
use crate::agent::AgentOutcome;
use crate::audio;
use crate::boot::BootSequence;
use crate::chat::{random_greeting, Conversation};
use crate::device::{DeviceDetail, Field};
use crate::menu::Menu;
use crate::onvif;
use crate::scan::{Device, ScanState};

const TICK_MS: u64 = 50; // render/sim tick

impl App {
    pub fn new() -> Self {
        // Load saved settings first, then build the shared mixer from them so the
        // boot hum honors the saved volume.
        let settings = crate::config::load_settings();
        let levels = std::sync::Arc::new(crate::audio::AudioLevels::new(
            settings.master,
            settings.voice,
            settings.ui,
            settings.ambient,
            settings.voice_enabled,
        ));
        let sfx = audio::init(levels.clone());
        sfx.boot_loop(); // start the hum; no-op if silent

        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let (display_tx, display_rx) = std::sync::mpsc::channel();
        let (scan_tx, scan_rx) = std::sync::mpsc::channel();
        let (tool_ui_tx, tool_ui_rx) = std::sync::mpsc::channel();
        let (stt_tx, stt_rx) = std::sync::mpsc::channel();

        // Trusted notification targets + the inbound Telegram pairing poller
        // (spawned only when a bot token + pairing code are configured).
        let notification: crate::notification::NotificationStore =
            std::sync::Arc::new(std::sync::Mutex::new(crate::notification::load_targets()));
        if let Some(cfg) = crate::notification::NotificationConfig::from_env() {
            crate::notification::spawn_pairing(notification.clone(), cfg);
        }
        let (beacon_tx, beacon_rx) = std::sync::mpsc::channel();

        // The ALERTS subsystem: its own persisted store + a delivery-status channel.
        let alert_store: crate::alerts::AlertStore =
            std::sync::Arc::new(std::sync::Mutex::new(crate::alerts::domain::load_alerts()));
        let (alert_status_tx, alert_status_rx) = std::sync::mpsc::channel();

        // The WORKFLOWS subsystem: its persisted graphs plus the run engine,
        // seeded so the existing alert backlog doesn't fire `OnNewAlert` graphs.
        let workflow_seed = alert_store.lock().unwrap().iter().map(|a| a.id).max().unwrap_or(0);
        let workflows = crate::workflows::WorkflowsState::new();
        let engine = crate::workflows::WorkflowEngine::new(workflow_seed);
        let (workflow_tx, workflow_rx) = std::sync::mpsc::channel();

        // Reflect the saved CRT-sweep preferences into the shared flags the GPU
        // post-processor reads each frame (no-op on the TTY frontend).
        crate::ui::video::set_sweep(settings.crt_sweep);
        crate::ui::video::set_sweep_intensity(settings.crt_sweep_intensity);

        App {
            screen: Screen::Splash,
            splash: crate::splash::SplashSequence::new(),
            boot: BootSequence::new(),
            menu: Menu::new(),
            scan: ScanState::new(),
            detail: None,
            convo: Conversation::new(),
            sfx,
            settings,
            levels,
            config: crate::config::ConfigScreen::new(),
            reply_tx,
            reply_rx,
            display_tx,
            display_rx,
            scan_tx,
            scan_rx,
            tool_ui_tx,
            tool_ui_rx,
            snapshot_modal: None,
            live: None,
            live_modal: false,
            listen: crate::listen::ListenState::new(),
            stt_tx,
            stt_rx,
            notification,
            beacons: crate::notification::BeaconsScreen::new(),
            beacon_detail: None,
            beacon_tx,
            beacon_rx,
            alert_store,
            alerts: crate::alerts::AlertsScreen::new(),
            alert_status_tx,
            alert_status_rx,
            map: crate::map::MapState::new(),
            workflows,
            engine,
            workflow_tx,
            workflow_rx,
            activity: Activity::new(),
        }
    }

    /// Push the current [`Settings`](crate::config::Settings) into the shared
    /// audio mixer so volume/mute changes take effect immediately. Cheap — called
    /// on every adjustment.
    fn sync_levels(&self) {
        self.levels.set_master(self.settings.master);
        self.levels.set_voice(self.settings.voice);
        self.levels.set_ui(self.settings.ui);
        self.levels.set_ambient(self.settings.ambient);
        self.levels.set_voice_enabled(self.settings.voice_enabled);
    }

    /// Push the CRT refresh-roll settings into the shared flags the GPU
    /// post-processor reads each frame. No-op effect on the `--tty` frontend.
    fn sync_crt_sweep(&self) {
        crate::ui::video::set_sweep(self.settings.crt_sweep);
        crate::ui::video::set_sweep_intensity(self.settings.crt_sweep_intensity);
    }

    /// Advance the always-on simulation (the radar sweep runs in every screen so
    /// the boot cursor can blink off it and the tracker is live the instant it
    /// opens), deliver any Claude replies, reveal speech whose audio is ready,
    /// and apply any scan/camera results.
    pub fn on_tick(&mut self) {
        self.scan.tick();
        self.poll_replies();
        self.poll_display();
        self.advance_typing();
        self.poll_scan_events();
        self.poll_tool_ui();
        self.poll_stt();
        self.poll_beacons();
        self.poll_alert_status();
        self.poll_workflows();
        if let Some(s) = self.live.as_mut() {
            let now = std::time::Instant::now();
            s.drain(now);
            s.supervise(now, &self.activity);
        }
        // Loop the "working" sound while processing (but never on the splash/boot screens).
        let working =
            !matches!(self.screen, Screen::Splash | Screen::Boot) && self.activity.is_busy();
        self.sfx.set_working(working);
    }

    /// Start playing a device's RTSP stream (dropping any existing one). The
    /// `connecting` activity guard rides into the stream and clears on the first
    /// frame.
    fn start_live(&mut self, ip: String, rtsp: String, user: String, pass: String) {
        let connecting = self.activity.begin();
        let params = crate::stream::SpawnParams {
            rtsp,
            user,
            pass,
            size: crate::stream::FrameSize::DEFAULT,
            fps: crate::stream::DEFAULT_FPS,
            transport: self.settings.transport,
        };
        match crate::stream::client::start(params, ip, connecting) {
            Ok(stream) => self.live = Some(stream),
            Err(e) => {
                if let Some(d) = self.detail.as_mut() {
                    d.status = format!("Live failed: {e}");
                }
            }
        }
    }

    fn stop_live(&mut self) {
        self.live = None; // Drop kills + reaps ffmpeg
        self.live_modal = false;
    }

    /// Toggle the live feed for the device on the detail screen.
    fn toggle_live(&mut self) {
        let Some(ip) = self.detail.as_ref().map(|d| d.ip.clone()) else { return };
        if self.live.as_ref().map(|s| s.device_ip.as_str()) == Some(ip.as_str()) {
            self.stop_live();
            return;
        }
        let Some(device) = self.scan.device_by_ip(&ip) else { return };
        if !device.has_credentials() {
            if let Some(d) = self.detail.as_mut() {
                d.status = "Save valid credentials before going live.".into();
            }
            return;
        }
        let Some(rtsp) = device.rtsp_url.clone() else {
            if let Some(d) = self.detail.as_mut() {
                d.status = "No RTSP URL — run TEST & SAVE first.".into();
            }
            return;
        };
        let user = device.username.clone().unwrap_or_default();
        let pass = device.password.clone().unwrap_or_default();
        self.start_live(ip, rtsp, user, pass);
    }

    /// Reveal a few more characters of Bishop's reply, clicking as it types, and
    /// commit the line once it's fully shown.
    fn advance_typing(&mut self) {
        if self.convo.typing.is_none() {
            return;
        }
        if self.convo.typing_complete() {
            self.convo.finish_typing();
            return;
        }
        self.convo.type_step(self.settings.typing_speed as usize);
        self.sfx.type_tick(); // quiet teletype chatter (swap the sample to taste)
    }

    /// Speak `text` and reveal it (in the log) only once its audio is ready.
    /// `wrap` tags how the revealed line should be recorded. An optional
    /// processing `guard` is released exactly when the audio is ready, so the
    /// top-bar "PROCESSING" indicator clears only when Bishop starts speaking.
    fn say(&self, text: String, wrap: fn(String) -> Say, guard: Option<ActivityGuard>) {
        let tx = self.display_tx.clone();
        let revealed = text.clone();
        self.sfx.speak(
            &text,
            Box::new(move || {
                let _busy = guard; // dropped here → PROCESSING clears as audio begins
                let _ = tx.send(wrap(revealed));
            }),
        );
    }

    /// Send the composed inquiry to Claude on a background thread. The reply is
    /// delivered back through `reply_rx` and handled in `poll_replies`.
    fn submit_chat(&mut self) {
        let text = self.convo.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.convo.clear_input();
        self.send_to_bishop(text);
    }

    /// Echo `text` as the operator's line and dispatch it to Bishop. Works from
    /// any screen — the spoken reply plays regardless of what's on screen, and
    /// the exchange is recorded in the chat log. Shared by typed input and the
    /// F9 voice link.
    fn send_to_bishop(&mut self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.convo.begin_user(&text);

        let history = self.convo.history.clone();
        let tx = self.reply_tx.clone();
        let store = self.scan.devices.clone();
        let ui_tx = self.tool_ui_tx.clone();
        let notif = self.notification.clone();
        let alerts = self.alert_store.clone();
        // Begin "processing". The guard rides with the reply and is only dropped
        // once the reply's audio is ready (see `say`).
        let busy = self.activity.begin();
        std::thread::spawn(move || {
            // Live Claude (with tools) if configured, else the offline persona stub.
            let registry = ToolRegistry::new();
            let ctx = ToolContext { store, ui_tx, notif, alerts };
            let outcome = crate::agent::client::ask(&history, &registry, &ctx)
                .unwrap_or_else(|| AgentOutcome::offline(crate::chat::handler::reply(&text)));
            let _ = tx.send((outcome, busy));
        });
    }

    // ---- speech-to-text (F9 talk to Bishop) -------------------------------

    /// Toggle the microphone: start capturing on the first F9, stop + transcribe
    /// on the second. A no-op if there's no microphone / the `audio` feature is
    /// off (capture returns `None`).
    fn toggle_listening(&mut self) {
        if self.listen.active {
            self.stop_listening();
        } else {
            self.start_listening();
        }
    }

    fn start_listening(&mut self) {
        self.listen.level.store(0.0f32.to_bits(), std::sync::atomic::Ordering::Relaxed);
        match crate::listen::recorder::start(self.listen.level.clone()) {
            Some(rec) => {
                self.listen.recording = Some(rec);
                self.listen.active = true;
            }
            None => {
                // No mic or audio disabled — note it in the log, stay put.
                self.convo
                    .push_system("Voice link unavailable — no microphone.");
            }
        }
    }

    /// Stop capturing and transcribe on a background thread; the transcript is
    /// delivered via `stt_rx` and routed to Bishop in `on_tick`.
    fn stop_listening(&mut self) {
        self.listen.active = false;
        let Some(rec) = self.listen.recording.take() else { return };
        let wav = rec.finish();
        let tx = self.stt_tx.clone();
        let busy = self.activity.begin(); // PROCESSING while transcribing
        std::thread::spawn(move || {
            let _busy = busy;
            let event = match crate::listen::client::transcribe(wav) {
                Ok(text) => crate::listen::SttEvent::Transcript(text),
                Err(e) => crate::listen::SttEvent::Failed(e),
            };
            let _ = tx.send(event);
        });
    }

    /// Abandon the current capture without transcribing (Esc while listening).
    fn cancel_listening(&mut self) {
        self.listen.active = false;
        self.listen.recording = None; // Drop stops the stream
    }

    /// Drain finished transcriptions: route speech to Bishop (auto-send per the
    /// settings) or report a failure in the log.
    fn poll_stt(&mut self) {
        while let Ok(event) = self.stt_rx.try_recv() {
            match event {
                crate::listen::SttEvent::Transcript(text) => {
                    if self.settings.stt_autosend {
                        self.send_to_bishop(text);
                    } else {
                        // Park it in the chat input for review next time chat opens.
                        self.convo.set_input(&text);
                    }
                }
                crate::listen::SttEvent::Failed(reason) => {
                    self.convo.push_system(&format!("Voice link: {reason}."));
                }
            }
        }
    }

    /// Drain finished Claude turns: splice the produced messages into history
    /// (so context is correct even if the reveal is delayed) and start voicing
    /// the final text. It's revealed later, by `poll_display`, the moment its
    /// audio begins — so the "working…" indicator stays up until Bishop speaks.
    fn poll_replies(&mut self) {
        while let Ok((outcome, busy)) = self.reply_rx.try_recv() {
            for m in outcome.appended {
                self.convo.history.push(m);
            }
            self.say(outcome.reply_text, Say::Reply, Some(busy));
        }
    }

    /// Reveal speech whose audio is now ready (or that has no audio to wait on).
    /// Greetings appear at once; replies type out one character at a time.
    fn poll_display(&mut self) {
        while let Ok(msg) = self.display_rx.try_recv() {
            match msg {
                Say::Greeting(text) => self.convo.push_bishop(&text),
                Say::Reply(text) => {
                    // History was already spliced in `poll_replies`; just reveal.
                    self.convo.pending = false;
                    self.convo.start_typing(text);
                }
            }
        }
    }

    /// Advance the splash card; hand off to the boot scroll when its ~3s elapse.
    pub fn splash_tick(&mut self) {
        if self.splash.tick() {
            self.screen = Screen::Boot;
        }
    }

    /// Skip the splash card (any keypress) straight to the boot scroll.
    fn skip_splash(&mut self) {
        if self.screen == Screen::Splash {
            self.screen = Screen::Boot;
        }
    }

    /// Advance the boot scroll; hand off to the menu when it completes.
    pub fn boot_tick(&mut self) {
        if self.boot.tick(&*self.sfx) {
            self.enter_menu();
        }
    }

    /// Skip the boot animation entirely (any keypress during boot).
    pub fn skip_boot(&mut self) {
        self.boot.skip(&*self.sfx);
        self.enter_menu();
    }

    /// Hand off from the boot scroll to the home menu: kill the hum and have
    /// Bishop voice a (randomly chosen) greeting. The greeting line is revealed
    /// when its audio is ready. Idempotent — only fires from `Boot`.
    fn enter_menu(&mut self) {
        if self.screen != Screen::Boot {
            return;
        }
        self.sfx.hush(); // kill the boot hum
        self.say(random_greeting().to_string(), Say::Greeting, None);
        self.screen = Screen::Menu;
    }

    // ---- scan feature -----------------------------------------------------

    /// Kick off an ONVIF discovery sweep on a background thread.
    fn start_discovery(&mut self) {
        if self.scan.scanning {
            return;
        }
        self.scan.scanning = true;
        let tx = self.scan_tx.clone();
        let busy = self.activity.begin();
        std::thread::spawn(move || {
            let _busy = busy; // processing indicator for the whole sweep
            let found: Vec<Device> = onvif::discovery::discover()
                .into_iter()
                .map(Device::from_discovered)
                .collect();
            let _ = tx.send(ScanEvent::Discovered(found));
        });
    }

    /// Open the device-detail screen for the selected device.
    fn open_detail(&mut self) {
        if let Some(d) = self.scan.selected_device() {
            self.detail = Some(DeviceDetail::new(
                d.ip.clone(),
                d.name.clone(),
                d.username.clone().unwrap_or_default(),
                d.password.clone().unwrap_or_default(),
            ));
            self.screen = Screen::DeviceDetail;
        }
    }

    /// Commit the edited name (RENAME action on the detail screen).
    fn rename_device(&mut self) {
        let Some(detail) = self.detail.as_mut() else { return };
        match crate::device::actions::rename_device(&self.scan.devices, &detail.ip, &detail.name) {
            Ok(name) => detail.status = format!("Renamed to '{name}'."),
            Err(e) => detail.status = e,
        }
    }

    /// Validate the entered credentials against the camera (background thread).
    fn test_credentials(&mut self) {
        let Some(detail) = self.detail.as_mut() else { return };
        let Some(device) = self.scan.device_by_ip(&detail.ip) else { return };
        let xaddr = device.xaddr.clone();
        let ip = device.ip.clone();
        let username = detail.username.clone();
        let password = detail.password.clone();
        detail.status = "Testing credentials…".into();

        let tx = self.scan_tx.clone();
        let busy = self.activity.begin();
        std::thread::spawn(move || {
            let _busy = busy;
            let result = onvif::client::test_credentials(&xaddr, &username, &password);
            let _ = tx.send(ScanEvent::Credentials { ip, username, password, result });
        });
    }

    /// Request a snapshot for the current device (background thread). Only works
    /// once credentials are saved.
    fn request_snapshot(&mut self) {
        let Some(detail) = self.detail.as_mut() else { return };
        let Some(device) = self.scan.device_by_ip(&detail.ip) else { return };
        if !device.has_credentials() {
            detail.status = "Save valid credentials before requesting a snapshot.".into();
            return;
        }
        detail.status = "Requesting snapshot…".into();

        let tx = self.scan_tx.clone();
        let busy = self.activity.begin();
        std::thread::spawn(move || {
            let _busy = busy;
            let result = crate::device::actions::capture_snapshot(&device);
            let _ = tx.send(ScanEvent::Snapshot(result));
        });
    }

    /// Capture a snapshot for the current device and ask Bishop to describe what
    /// is visible (background thread). The still is shown in the viewport and the
    /// description lands in the status line.
    fn request_describe(&mut self) {
        let Some(detail) = self.detail.as_mut() else { return };
        let Some(device) = self.scan.device_by_ip(&detail.ip) else { return };
        if !device.has_credentials() {
            detail.status = "Save valid credentials before describing the camera.".into();
            return;
        }
        detail.status = "Describing what the camera sees…".into();

        let tx = self.scan_tx.clone();
        let busy = self.activity.begin();
        std::thread::spawn(move || {
            let _busy = busy;
            // One capture feeds both the on-screen still and the vision call.
            let result = crate::device::actions::capture_snapshot_bytes(&device)
                .and_then(|bytes| {
                    let img = crate::device::actions::decode_luma(&bytes)?;
                    let text = crate::agent::vision::describe_image(&bytes, &device.name)?;
                    Ok((img, text))
                });
            let _ = tx.send(ScanEvent::Describe(result));
        });
    }

    /// Apply finished discovery / credential / snapshot results.
    fn poll_scan_events(&mut self) {
        while let Ok(event) = self.scan_rx.try_recv() {
            match event {
                ScanEvent::Discovered(found) => {
                    let added = self.scan.merge(found);
                    self.scan.scanning = false;
                    if added > 0 {
                        self.scan.save();
                    }
                }
                ScanEvent::Credentials { ip, username, password, result } => {
                    match result {
                        Ok(rtsp) => {
                            {
                                let mut devices = self.scan.devices.lock().unwrap();
                                if let Some(d) = devices.iter_mut().find(|d| d.ip == ip) {
                                    d.username = Some(username);
                                    d.password = Some(password);
                                    d.rtsp_url = Some(rtsp);
                                }
                            }
                            self.scan.save();
                            if let Some(detail) = self.detail.as_mut() {
                                detail.status = "Credentials verified and saved.".into();
                            }
                        }
                        Err(e) => {
                            if let Some(detail) = self.detail.as_mut() {
                                detail.status = format!("Rejected: {e}");
                            }
                        }
                    }
                }
                ScanEvent::Snapshot(result) => {
                    if let Some(detail) = self.detail.as_mut() {
                        match result {
                            Ok(img) => {
                                detail.image = Some(crate::ui::image::CameraImage::new(img));
                                detail.status = "Snapshot received.".into();
                            }
                            Err(e) => detail.status = format!("Snapshot failed: {e}"),
                        }
                    }
                }
                ScanEvent::Describe(result) => {
                    if let Some(detail) = self.detail.as_mut() {
                        match result {
                            Ok((img, text)) => {
                                detail.image = Some(crate::ui::image::CameraImage::new(img));
                                detail.status = text;
                            }
                            Err(e) => detail.status = format!("Describe failed: {e}"),
                        }
                    }
                }
            }
        }
    }

    /// Apply UI side-effects requested by chat tools.
    fn poll_tool_ui(&mut self) {
        while let Ok(ev) = self.tool_ui_rx.try_recv() {
            match ev {
                ToolUiEvent::Image(img) => {
                    self.snapshot_modal = Some(crate::ui::image::CameraImage::new(img))
                }
                ToolUiEvent::DevicesListBlock(summaries) => self.convo.push_list_block(&summaries),
                ToolUiEvent::StartLive { ip, rtsp, user, pass } => {
                    self.start_live(ip, rtsp, user, pass);
                    self.live_modal = true;
                }
                ToolUiEvent::DevicesPersisted => {
                    self.scan.clamp_selection();
                    // Stop a live feed whose device was just removed.
                    if let Some(s) = &self.live {
                        if self.scan.device_by_ip(&s.device_ip).is_none() {
                            self.stop_live();
                        }
                    }
                    // A tool deleted a device — close the detail screen if it was that one.
                    if let Some(detail) = &self.detail {
                        if self.scan.device_by_ip(&detail.ip).is_none() {
                            self.detail = None;
                            if self.screen == Screen::DeviceDetail {
                                self.screen = Screen::Scan;
                            }
                        }
                    }
                }
                ToolUiEvent::AlertsPersisted => {
                    // A tool changed the alert log — keep the dashboard cursor valid.
                    let n = self.today_alert_count();
                    self.alerts.clamp(n);
                }
            }
        }
    }

    /// Delete the selected device manually (Scan screen).
    fn delete_selected(&mut self) {
        if let Some(d) = self.scan.selected_device() {
            let _ = crate::device::actions::delete_by_name(&self.scan.devices, &d.name);
            self.scan.clamp_selection();
        }
    }

    /// Route a keypress for the current screen. Returns `true` to quit.
    /// Both the terminal (`run`) and the windowed CRT frontends call this.
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.sfx.keystroke(); // mechanical click on every keypress

        // Global voice link: F9 toggles "talk to Bishop" from any screen (the
        // boot scroll excepted). The reply is spoken wherever you are.
        if key.code == KeyCode::F(9) && !matches!(self.screen, Screen::Splash | Screen::Boot) {
            self.toggle_listening();
            return false;
        }
        // While capturing, Esc abandons it (and is swallowed so it doesn't also
        // back out of the current screen).
        if self.listen.active && key.code == KeyCode::Esc {
            self.cancel_listening();
            return false;
        }

        match self.screen {
            Screen::Splash => {
                if key.code == KeyCode::Esc {
                    return true; // nothing to back out to yet
                }
                self.skip_splash(); // any other key jumps to the boot scroll
            }
            Screen::Boot => {
                if key.code == KeyCode::Esc {
                    return true; // nothing to back out to yet
                }
                self.skip_boot();
            }
            Screen::Menu => match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.menu.prev(),
                KeyCode::Down | KeyCode::Char('j') => self.menu.next(),
                KeyCode::Enter => self.screen = self.menu.target(),
                KeyCode::Esc => return true,
                _ => {}
            },
            Screen::About => {
                if key.code == KeyCode::Esc {
                    self.screen = Screen::Menu;
                }
            }
            Screen::Scan => match key.code {
                KeyCode::Esc => self.screen = Screen::Menu,
                KeyCode::Up | KeyCode::Char('k') => self.scan.prev(),
                KeyCode::Down | KeyCode::Char('j') => self.scan.next(),
                KeyCode::Char('d') | KeyCode::Char('D') => self.start_discovery(),
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Delete => self.delete_selected(),
                KeyCode::Enter => self.open_detail(),
                _ => {}
            },
            Screen::DeviceDetail => match key.code {
                KeyCode::Esc => {
                    self.stop_live();
                    self.detail = None;
                    self.screen = Screen::Scan;
                }
                KeyCode::Tab | KeyCode::Down => {
                    if let Some(d) = self.detail.as_mut() {
                        d.focus_next();
                    }
                }
                KeyCode::BackTab | KeyCode::Up => {
                    if let Some(d) = self.detail.as_mut() {
                        d.focus_prev();
                    }
                }
                KeyCode::Enter => match self.detail.as_ref().map(|d| d.field) {
                    Some(Field::Test) => self.test_credentials(),
                    Some(Field::Snapshot) => self.request_snapshot(),
                    Some(Field::Describe) => self.request_describe(),
                    Some(Field::Live) => self.toggle_live(),
                    Some(Field::Rename) => self.rename_device(),
                    // Enter on a text field advances focus, like Tab.
                    Some(_) => {
                        if let Some(d) = self.detail.as_mut() {
                            d.focus_next();
                        }
                    }
                    None => {}
                },
                KeyCode::Backspace => {
                    if let Some(d) = self.detail.as_mut() {
                        d.backspace();
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    if let Some(d) = self.detail.as_mut() {
                        d.type_char(c);
                    }
                }
                _ => {}
            },
            // Snapshot modal open: any Esc/Enter closes it; swallow other keys.
            Screen::Chat if self.snapshot_modal.is_some() => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    self.snapshot_modal = None;
                }
            }
            // Live modal open: Esc/Enter stops the feed; swallow other keys.
            Screen::Chat if self.live_modal => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    self.stop_live();
                }
            }
            Screen::Chat => match key.code {
                KeyCode::Esc => self.screen = Screen::Menu,
                KeyCode::Enter => self.submit_chat(),
                // Cursor movement. Alt/Ctrl jump a word; Cmd(Super) goes to the
                // line edge. Plain arrows move one character.
                KeyCode::Left => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        self.convo.word_left();
                    } else if key.modifiers.contains(KeyModifiers::SUPER) {
                        self.convo.home();
                    } else {
                        self.convo.move_left();
                    }
                }
                KeyCode::Right => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        self.convo.word_right();
                    } else if key.modifiers.contains(KeyModifiers::SUPER) {
                        self.convo.end();
                    } else {
                        self.convo.move_right();
                    }
                }
                KeyCode::Home => self.convo.home(),
                KeyCode::End => self.convo.end(),
                // Alt/Ctrl + Backspace (and Ctrl-W) delete a word; Cmd(Super) +
                // Backspace (and Ctrl-U) clear the line; plain Backspace deletes
                // one char before the cursor.
                KeyCode::Backspace => {
                    if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) {
                        self.convo.delete_word();
                    } else if key.modifiers.contains(KeyModifiers::SUPER) {
                        self.convo.clear_input();
                    } else {
                        self.convo.backspace();
                    }
                }
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.convo.delete_word();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.convo.clear_input();
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.convo.home();
                }
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.convo.end();
                }
                // Only insert printable characters typed without a command modifier.
                KeyCode::Char(c)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    self.convo.insert_char(c);
                }
                _ => {}
            },
            Screen::Config => match key.code {
                KeyCode::Esc => {
                    crate::config::save_settings(&self.settings); // persist on exit
                    self.screen = Screen::Menu;
                }
                KeyCode::Up | KeyCode::Char('k') => self.config.focus_prev(),
                KeyCode::Down | KeyCode::Char('j') => self.config.focus_next(),
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('-') => {
                    self.config.adjust(&mut self.settings, -1);
                    self.sync_levels();
                    self.sync_crt_sweep();
                }
                KeyCode::Right
                | KeyCode::Char('l')
                | KeyCode::Char('+')
                | KeyCode::Char('=')
                | KeyCode::Enter => {
                    self.config.adjust(&mut self.settings, 1);
                    self.sync_levels();
                    self.sync_crt_sweep();
                }
                _ => {}
            },
            Screen::Beacons => match key.code {
                KeyCode::Esc => self.screen = Screen::Menu,
                KeyCode::Left | KeyCode::Char('h') => {
                    let n = self.notification.lock().unwrap().len();
                    self.beacons.prev(n);
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    let n = self.notification.lock().unwrap().len();
                    self.beacons.next(n);
                }
                KeyCode::Up | KeyCode::Char('k') => self.beacons.up(),
                KeyCode::Down | KeyCode::Char('j') => {
                    let n = self.notification.lock().unwrap().len();
                    self.beacons.down(n);
                }
                KeyCode::Enter => self.open_beacon_detail(),
                KeyCode::Char('t') | KeyCode::Char('T') => self.test_beacon(),
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Delete => self.delete_beacon(),
                _ => {}
            },
            Screen::BeaconDetail => match key.code {
                KeyCode::Esc => {
                    self.beacon_detail = None;
                    self.screen = Screen::Beacons;
                }
                KeyCode::Tab | KeyCode::Down => {
                    if let Some(d) = self.beacon_detail.as_mut() {
                        d.focus_next();
                    }
                }
                KeyCode::BackTab | KeyCode::Up => {
                    if let Some(d) = self.beacon_detail.as_mut() {
                        d.focus_prev();
                    }
                }
                KeyCode::Enter => self.beacon_detail_activate(),
                KeyCode::Backspace => {
                    if let Some(d) = self.beacon_detail.as_mut() {
                        d.backspace();
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    if let Some(d) = self.beacon_detail.as_mut() {
                        d.type_char(c);
                    }
                }
                _ => {}
            },
            Screen::Alerts => match self.alerts.mode {
                crate::alerts::AlertsMode::Browse => match key.code {
                    KeyCode::Esc => self.screen = Screen::Menu,
                    KeyCode::Up | KeyCode::Char('k') => self.alerts.prev(),
                    KeyCode::Down | KeyCode::Char('j') => {
                        let n = self.today_alert_count();
                        self.alerts.next(n);
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => self.alerts.enter_create(),
                    KeyCode::Char('d') | KeyCode::Char('D') => self.deliver_selected_alert(),
                    KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Delete => {
                        self.delete_selected_alert()
                    }
                    _ => {}
                },
                crate::alerts::AlertsMode::Create => match key.code {
                    KeyCode::Esc => self.alerts.cancel_create(),
                    KeyCode::Tab | KeyCode::Down => self.alerts.draft.focus_next(),
                    KeyCode::BackTab | KeyCode::Up => self.alerts.draft.focus_prev(),
                    KeyCode::Left => {
                        let n = self.map.rooms.len();
                        self.alerts.draft.adjust(-1, n);
                    }
                    KeyCode::Right => {
                        let n = self.map.rooms.len();
                        self.alerts.draft.adjust(1, n);
                    }
                    KeyCode::Enter => {
                        if self.alerts.draft.field == crate::alerts::DraftField::Submit {
                            self.commit_alert_draft();
                        } else {
                            self.alerts.draft.focus_next();
                        }
                    }
                    KeyCode::Backspace => self.alerts.draft.backspace(),
                    KeyCode::Char(c)
                        if !key.modifiers.intersects(
                            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                        ) =>
                    {
                        self.alerts.draft.type_char(c)
                    }
                    _ => {}
                },
            },
            Screen::Map => match self.map.mode {
                crate::map::MapMode::Browse => match key.code {
                    KeyCode::Esc => self.screen = Screen::Menu,
                    KeyCode::Up | KeyCode::Char('k') => self.map.prev(),
                    KeyCode::Down | KeyCode::Char('j') => self.map.next(),
                    KeyCode::Char('n') | KeyCode::Char('N') => self.map.new_room(),
                    KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Delete => self.map.delete_room(),
                    KeyCode::Char('r') | KeyCode::Char('R') => self.map.enter_rename(),
                    KeyCode::Char('m') | KeyCode::Char('M') => self.map.enter_move(),
                    KeyCode::Char('s') | KeyCode::Char('S') => self.map.enter_resize(),
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        let ips: Vec<String> =
                            self.scan.devices_snapshot().into_iter().map(|d| d.ip).collect();
                        self.map.enter_cameras(&ips);
                    }
                    KeyCode::Char('+') | KeyCode::Char('=') => self.map.zoom_in(),
                    KeyCode::Char('-') | KeyCode::Char('_') => self.map.zoom_out(),
                    KeyCode::Char('p') | KeyCode::Char('P') => self.map.enter_pan(),
                    KeyCode::Char('f') | KeyCode::Char('F') => self.map.fit(),
                    _ => {}
                },
                crate::map::MapMode::Move => match key.code {
                    KeyCode::Left => self.map.nudge(-1, 0),
                    KeyCode::Right => self.map.nudge(1, 0),
                    KeyCode::Up => self.map.nudge(0, -1),
                    KeyCode::Down => self.map.nudge(0, 1),
                    KeyCode::Enter | KeyCode::Esc => self.map.commit_geometry(),
                    _ => {}
                },
                crate::map::MapMode::Resize => match key.code {
                    KeyCode::Left => self.map.resize(-1, 0),
                    KeyCode::Right => self.map.resize(1, 0),
                    KeyCode::Up => self.map.resize(0, -1),
                    KeyCode::Down => self.map.resize(0, 1),
                    KeyCode::Enter | KeyCode::Esc => self.map.commit_geometry(),
                    _ => {}
                },
                crate::map::MapMode::Rename => match key.code {
                    KeyCode::Enter => self.map.commit_rename(),
                    KeyCode::Esc => self.map.cancel_rename(),
                    KeyCode::Backspace => self.map.backspace(),
                    KeyCode::Char(c)
                        if !key.modifiers.intersects(
                            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                        ) =>
                    {
                        self.map.type_char(c)
                    }
                    _ => {}
                },
                crate::map::MapMode::Cameras => match key.code {
                    KeyCode::Esc => {
                        self.map.mode = crate::map::MapMode::Browse;
                        self.map.status.clear();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        let n = self.scan.device_count();
                        self.map.cam_prev(n);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let n = self.scan.device_count();
                        self.map.cam_next(n);
                    }
                    KeyCode::Char(' ') => self.toggle_room_camera(),
                    // Enter assigns an unassigned camera, or positions an
                    // already-assigned one.
                    KeyCode::Enter => self.cameras_enter(),
                    _ => {}
                },
                crate::map::MapMode::CamPlace => match key.code {
                    KeyCode::Left => self.move_selected_camera(-1, 0),
                    KeyCode::Right => self.move_selected_camera(1, 0),
                    KeyCode::Up => self.move_selected_camera(0, -1),
                    KeyCode::Down => self.move_selected_camera(0, 1),
                    KeyCode::Enter | KeyCode::Esc => self.map.commit_cam_place(),
                    _ => {}
                },
                crate::map::MapMode::Pan => match key.code {
                    KeyCode::Left => self.map.pan(-1, 0),
                    KeyCode::Right => self.map.pan(1, 0),
                    KeyCode::Up => self.map.pan(0, -1),
                    KeyCode::Down => self.map.pan(0, 1),
                    KeyCode::Char('+') | KeyCode::Char('=') => self.map.zoom_in(),
                    KeyCode::Char('-') | KeyCode::Char('_') => self.map.zoom_out(),
                    KeyCode::Enter | KeyCode::Esc => self.map.exit_pan(),
                    _ => {}
                },
            },
            Screen::Workflows => {
                self.handle_workflows_key(key);
                // A Run-now request from Browse fires the selected graph.
                if let Some(id) = self.workflows.run_request.take() {
                    self.spawn_run(id);
                }
            }
        }
        false
    }

    /// Route a keypress for the WORKFLOWS editor, dispatching on its mode
    /// (mirrors the `Screen::Map` arm). All mutations stay inside `WorkflowsState`.
    fn handle_workflows_key(&mut self, key: KeyEvent) {
        let wf = &mut self.workflows;
        match wf.mode {
            crate::workflows::WfMode::Browse => match key.code {
                KeyCode::Esc => self.screen = Screen::Menu,
                KeyCode::Up | KeyCode::Char('k') => wf.prev(),
                KeyCode::Down | KeyCode::Char('j') => wf.next(),
                KeyCode::Char('a') | KeyCode::Char('A') => wf.enter_palette(),
                KeyCode::Char('l') | KeyCode::Char('L') => wf.enter_link(),
                KeyCode::Char('c') | KeyCode::Char('C') => wf.enter_config(),
                KeyCode::Char('m') | KeyCode::Char('M') => wf.enter_move(),
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Delete => wf.delete_node(),
                KeyCode::Char('e') | KeyCode::Char('E') => wf.toggle_enabled(),
                KeyCode::Char('r') | KeyCode::Char('R') => wf.request_run(),
                KeyCode::Char('g') | KeyCode::Char('G') => wf.enter_graphs(),
                KeyCode::Char('p') | KeyCode::Char('P') => wf.enter_pan(),
                KeyCode::Char('+') | KeyCode::Char('=') => wf.zoom_in(),
                KeyCode::Char('-') | KeyCode::Char('_') => wf.zoom_out(),
                _ => {}
            },
            crate::workflows::WfMode::Palette => match key.code {
                KeyCode::Esc => {
                    wf.mode = crate::workflows::WfMode::Browse;
                    wf.status.clear();
                }
                KeyCode::Up | KeyCode::Char('k') => wf.palette_prev(),
                KeyCode::Down | KeyCode::Char('j') => wf.palette_next(),
                KeyCode::Enter => wf.drop_palette_node(),
                _ => {}
            },
            crate::workflows::WfMode::Move => match key.code {
                KeyCode::Left => wf.nudge(-1, 0),
                KeyCode::Right => wf.nudge(1, 0),
                KeyCode::Up => wf.nudge(0, -1),
                KeyCode::Down => wf.nudge(0, 1),
                KeyCode::Enter | KeyCode::Esc => wf.commit_move(),
                _ => {}
            },
            crate::workflows::WfMode::Link => match key.code {
                KeyCode::Esc => wf.cancel_link(),
                // Before lock-in ←/→ cycle the source output; after, they cycle
                // the target input and ↑/↓ pick the target node.
                KeyCode::Left => {
                    if wf.link_from.is_some() {
                        wf.link_cycle_input(-1);
                    } else {
                        wf.link_cycle(-1);
                    }
                }
                KeyCode::Right => {
                    if wf.link_from.is_some() {
                        wf.link_cycle_input(1);
                    } else {
                        wf.link_cycle(1);
                    }
                }
                KeyCode::Up => wf.link_cycle(-1),
                KeyCode::Down => wf.link_cycle(1),
                KeyCode::Enter => wf.link_enter(),
                _ => {}
            },
            crate::workflows::WfMode::Config => match key.code {
                KeyCode::Esc => wf.cancel_config(),
                KeyCode::Up => wf.config_prev(),
                KeyCode::Down => wf.config_next(),
                KeyCode::Left => wf.config_adjust(-1),
                KeyCode::Right => wf.config_adjust(1),
                KeyCode::Enter => wf.commit_config(),
                KeyCode::Backspace => wf.config_backspace(),
                KeyCode::Char(c)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    wf.config_type(c)
                }
                _ => {}
            },
            crate::workflows::WfMode::Pan => match key.code {
                KeyCode::Left => wf.pan(-1, 0),
                KeyCode::Right => wf.pan(1, 0),
                KeyCode::Up => wf.pan(0, -1),
                KeyCode::Down => wf.pan(0, 1),
                KeyCode::Char('+') | KeyCode::Char('=') => wf.zoom_in(),
                KeyCode::Char('-') | KeyCode::Char('_') => wf.zoom_out(),
                KeyCode::Enter | KeyCode::Esc => wf.exit_pan(),
                _ => {}
            },
            crate::workflows::WfMode::Graphs => {
                if wf.graphs_editing {
                    match key.code {
                        KeyCode::Esc => wf.graphs_cancel_edit(),
                        KeyCode::Enter => wf.graphs_commit_edit(),
                        KeyCode::Backspace => wf.graphs_backspace(),
                        KeyCode::Char(c)
                            if !key.modifiers.intersects(
                                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                            ) =>
                        {
                            wf.graphs_type(c)
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Esc => {
                            wf.mode = crate::workflows::WfMode::Browse;
                            wf.status.clear();
                        }
                        KeyCode::Up | KeyCode::Char('k') => wf.graphs_prev(),
                        KeyCode::Down | KeyCode::Char('j') => wf.graphs_next(),
                        KeyCode::Enter => wf.graphs_select(),
                        KeyCode::Char('n') | KeyCode::Char('N') => wf.graphs_begin_create(),
                        KeyCode::Char('r') | KeyCode::Char('R') => wf.graphs_begin_rename(),
                        KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Delete => {
                            wf.graphs_delete()
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // ---- WORKFLOWS engine + runs ------------------------------------------

    /// Drain finished workflow runs into the status line, release the engine's
    /// run slot, and spawn any queued fire that should now run (QueueSingle drain).
    fn poll_workflows(&mut self) {
        // 1. Finished runs.
        while let Ok((graph_id, status)) = self.workflow_rx.try_recv() {
            self.workflows.status = status;
            if self.engine.on_finish(graph_id) {
                self.spawn_run(graph_id); // a queued fire drains to a fresh run
            }
        }

        // 2. New alert levels, computed ONCE per tick.
        let levels = {
            let alerts = self.alert_store.lock().unwrap();
            self.engine.drain_new_alert_levels(&alerts)
        };

        // 3. Per-graph trigger evaluation (snapshot to avoid borrow conflicts).
        let graphs: Vec<(u32, bool, crate::workflows::Trigger, crate::workflows::ConcurrencyPolicy)> =
            self.workflows
                .workflows
                .graphs
                .iter()
                .map(|g| (g.id, g.enabled, g.trigger.clone(), g.concurrency.clone()))
                .collect();
        let now = std::time::Instant::now();
        for (id, enabled, trigger, concurrency) in graphs {
            if !enabled {
                continue;
            }
            let due = match trigger {
                crate::workflows::Trigger::Manual => false,
                crate::workflows::Trigger::Cron { every_secs } => {
                    self.engine.cron_due(id, every_secs, now)
                }
                crate::workflows::Trigger::OnNewAlert { min_level } => {
                    crate::workflows::WorkflowEngine::levels_qualify(&levels, min_level)
                }
            };
            if !due {
                continue;
            }
            match self.engine.decide(id, &concurrency) {
                crate::workflows::Fire::Spawn => self.spawn_run(id),
                crate::workflows::Fire::Queue => self.engine.on_queue(id),
                crate::workflows::Fire::Skip => {}
            }
        }
    }

    /// Run a graph once on a background thread (mirrors `deliver_selected_alert`):
    /// clone the graph so the thread owns it, build a `ToolContext` from the
    /// shared stores, register the spawn, and send the `(id, status)` result back.
    fn spawn_run(&mut self, graph_id: u32) {
        let Some(graph) =
            self.workflows.workflows.graphs.iter().find(|g| g.id == graph_id).cloned()
        else {
            return;
        };
        self.engine.on_spawn(graph_id);
        let ctx = ToolContext {
            store: self.scan.devices.clone(),
            ui_tx: self.tool_ui_tx.clone(),
            notif: self.notification.clone(),
            alerts: self.alert_store.clone(),
        };
        let tx = self.workflow_tx.clone();
        let guard = self.activity.begin();
        std::thread::spawn(move || {
            let _busy = guard;
            let msg = match crate::workflows::run_dag(&graph, &ctx) {
                Ok(outs) => format!("\u{2713} {} ran {} node(s)", graph.name, outs.len()),
                Err(e) => format!("\u{2717} {}: {e}", graph.name),
            };
            let _ = tx.send((graph_id, msg));
        });
    }

    /// Toggle the camera at the picker cursor on/off the selected room.
    fn toggle_room_camera(&mut self) {
        let devices = self.scan.devices_snapshot();
        if let Some(d) = devices.get(self.map.cam_cursor) {
            self.map.toggle_camera(d.ip.clone());
        }
    }

    /// Enter on the camera picker: assign the highlighted camera if it isn't
    /// assigned yet, otherwise drop into CamPlace to position it in the room.
    fn cameras_enter(&mut self) {
        let devices = self.scan.devices_snapshot();
        let Some(d) = devices.get(self.map.cam_cursor) else { return };
        let ip = d.ip.clone();
        let assigned =
            self.map.selected_room().is_some_and(|r| r.cameras.iter().any(|c| c.ip == ip));
        if assigned {
            self.map.enter_cam_place();
        } else {
            self.map.toggle_camera(ip);
        }
    }

    /// Move the highlighted (assigned) camera within its room (CamPlace mode).
    fn move_selected_camera(&mut self, ddx: i32, ddy: i32) {
        let devices = self.scan.devices_snapshot();
        if let Some(d) = devices.get(self.map.cam_cursor) {
            let ip = d.ip.clone();
            self.map.move_camera(&ip, ddx, ddy);
        }
    }

    // ---- BEACONS screen actions -------------------------------------------

    /// Drain background beacon-test results into the grid status line (and the
    /// open beacon detail, if any).
    fn poll_beacons(&mut self) {
        while let Ok(status) = self.beacon_rx.try_recv() {
            self.beacons.status = status.clone();
            if let Some(d) = self.beacon_detail.as_mut() {
                d.status = status;
            }
        }
    }

    /// Open the beacon-detail screen for the selected beacon.
    fn open_beacon_detail(&mut self) {
        let t = self.notification.lock().unwrap().get(self.beacons.selected).cloned();
        if let Some(t) = t {
            self.beacon_detail = Some(crate::notification::BeaconDetail::new(t.chat_id, t.label));
            self.screen = Screen::BeaconDetail;
        }
    }

    /// Fire a test alert to the selected beacon (grid).
    fn test_beacon(&mut self) {
        let label = self.notification.lock().unwrap().get(self.beacons.selected).map(|t| t.label.clone());
        match label {
            Some(label) => self.spawn_beacon_test(label),
            None => self.beacons.status = "No beacon selected.".into(),
        }
    }

    /// Remove the selected beacon (grid).
    fn delete_beacon(&mut self) {
        if let Some(label) =
            crate::notification::handler::delete_target(&self.notification, self.beacons.selected)
        {
            let len = self.notification.lock().unwrap().len();
            self.beacons.clamp(len);
            self.beacons.status = format!("Removed '{label}'.");
        }
    }

    /// Activate the focused action on the beacon-detail form.
    fn beacon_detail_activate(&mut self) {
        let Some(field) = self.beacon_detail.as_ref().map(|d| d.field) else { return };
        match field {
            crate::notification::BeaconField::Name => {
                if let Some(d) = self.beacon_detail.as_mut() {
                    d.focus_next();
                }
            }
            crate::notification::BeaconField::Rename => self.commit_beacon_rename(),
            crate::notification::BeaconField::Test => self.test_beacon_detail(),
            crate::notification::BeaconField::Mute => self.toggle_beacon_mute(),
            crate::notification::BeaconField::Back => {
                self.beacon_detail = None;
                self.screen = Screen::Beacons;
            }
        }
    }

    fn commit_beacon_rename(&mut self) {
        let Some((chat_id, name)) = self.beacon_detail.as_ref().map(|d| (d.chat_id, d.name.clone()))
        else {
            return;
        };
        match crate::notification::handler::rename_target(&self.notification, chat_id, &name) {
            Ok(label) => {
                if let Some(d) = self.beacon_detail.as_mut() {
                    d.name = label.clone();
                    d.status = format!("Renamed to '{label}'.");
                }
            }
            Err(e) => {
                if let Some(d) = self.beacon_detail.as_mut() {
                    d.status = e;
                }
            }
        }
    }

    fn toggle_beacon_mute(&mut self) {
        let Some(chat_id) = self.beacon_detail.as_ref().map(|d| d.chat_id) else { return };
        if let Some((label, on)) =
            crate::notification::handler::toggle_enabled_by_id(&self.notification, chat_id)
        {
            if let Some(d) = self.beacon_detail.as_mut() {
                d.status = format!("'{label}' {}", if on { "unmuted" } else { "muted" });
            }
        }
    }

    fn test_beacon_detail(&mut self) {
        let Some(chat_id) = self.beacon_detail.as_ref().map(|d| d.chat_id) else { return };
        let label =
            self.notification.lock().unwrap().iter().find(|t| t.chat_id == chat_id).map(|t| t.label.clone());
        if let Some(label) = label {
            self.spawn_beacon_test(label);
        }
    }

    /// Send a test message to a beacon by label on a background thread; the result
    /// lands in [`poll_beacons`].
    fn spawn_beacon_test(&mut self, label: String) {
        let Some(cfg) = crate::notification::NotificationConfig::from_env() else {
            let msg = "Set TELEGRAM_BOT_TOKEN to send.".to_string();
            self.beacons.status = msg.clone();
            if let Some(d) = self.beacon_detail.as_mut() {
                d.status = msg;
            }
            return;
        };
        let pending = format!("Sending test to '{label}'\u{2026}");
        self.beacons.status = pending.clone();
        if let Some(d) = self.beacon_detail.as_mut() {
            d.status = pending;
        }
        let store = self.notification.clone();
        let tx = self.beacon_tx.clone();
        std::thread::spawn(move || {
            let msg = "BISHOP/AP test alert \u{2014} this beacon is paired and reachable.";
            let result = crate::notification::dispatch(&store, &cfg, Some(&label), msg, None);
            let _ = tx.send(result.unwrap_or_else(|e| format!("Test failed: {e}")));
        });
    }

    // ---- ALERTS dashboard actions -----------------------------------------

    /// Drain background alert-delivery results into the dashboard status line.
    fn poll_alert_status(&mut self) {
        while let Ok(status) = self.alert_status_rx.try_recv() {
            self.alerts.status = status;
        }
    }

    fn today_alert_count(&self) -> usize {
        let alerts = self.alert_store.lock().unwrap();
        crate::alerts::domain::today_indices(&alerts).len()
    }

    /// The id of the alert the Browse cursor points at (today's list).
    fn selected_alert_id(&self) -> Option<u64> {
        let alerts = self.alert_store.lock().unwrap();
        let today = crate::alerts::domain::today_indices(&alerts);
        today.get(self.alerts.selected).map(|&i| alerts[i].id)
    }

    /// Dispatch the selected alert to the live beacons on a background thread.
    fn deliver_selected_alert(&mut self) {
        let Some(id) = self.selected_alert_id() else {
            self.alerts.status = "No alert selected.".into();
            return;
        };
        self.alerts.status = format!("Delivering alert {id}\u{2026}");
        let store = self.alert_store.clone();
        let notif = self.notification.clone();
        let tx = self.alert_status_tx.clone();
        std::thread::spawn(move || {
            let result = crate::alerts::actions::deliver(&store, &notif, id);
            let _ = tx.send(result.unwrap_or_else(|e| format!("Deliver failed: {e}")));
        });
    }

    fn delete_selected_alert(&mut self) {
        let Some(id) = self.selected_alert_id() else { return };
        if crate::alerts::actions::delete(&self.alert_store, id) {
            let n = self.today_alert_count();
            self.alerts.clamp(n);
            self.alerts.status = format!("Removed alert {id}.");
        }
    }

    /// Commit the CREATE form into a new alert and return to Browse.
    fn commit_alert_draft(&mut self) {
        if self.alerts.draft.message.trim().is_empty() {
            self.alerts.status = "Message can't be empty.".into();
            return;
        }
        let level = self.alerts.draft.level;
        let message = self.alerts.draft.message.trim().to_string();
        let source = {
            let s = self.alerts.draft.source.trim();
            if s.is_empty() { "operator".to_string() } else { s.to_string() }
        };
        let (room_id, room_name) = match self.alerts.draft.room_idx.and_then(|i| self.map.rooms.get(i)) {
            Some(r) => (Some(r.id), Some(r.name.clone())),
            None => (None, None),
        };
        let alert =
            crate::alerts::actions::create(&self.alert_store, level, message, room_id, room_name, source);
        self.alerts.mode = crate::alerts::AlertsMode::Browse;
        self.alerts.selected = 0;
        self.alerts.status = format!("Raised alert {} [{}].", alert.id, alert.level.label());
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// The main loop: draw, pump input, tick on a fixed cadence.
pub fn run(terminal: &mut DefaultTerminal, mut app: App) -> std::io::Result<()> {
    let tick_rate = Duration::from_millis(TICK_MS);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| view::draw(f, &app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Ctrl-C always quits, from any screen.
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        return Ok(());
                    }
                    if app.handle_key(key) {
                        return Ok(());
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            match app.screen {
                Screen::Splash => app.splash_tick(),
                Screen::Boot => app.boot_tick(),
                _ => {}
            }
            last_tick = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::domain::{BOOT_SCRIPT, BOOT_TOTAL_TICKS};

    #[test]
    fn splash_holds_then_hands_off_to_boot() {
        use crate::splash::domain::SPLASH_TOTAL_TICKS;
        let mut app = App::new();
        assert_eq!(app.screen, Screen::Splash);
        for _ in 0..SPLASH_TOTAL_TICKS {
            app.on_tick();
            if app.screen == Screen::Splash {
                app.splash_tick();
            }
        }
        assert_eq!(app.screen, Screen::Boot);
    }

    #[test]
    fn any_key_skips_splash_to_boot() {
        let mut app = App::new();
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Boot);
    }

    #[test]
    fn boot_reveals_all_lines_then_opens_menu() {
        let mut app = App::new();
        // Step past the splash card into the boot scroll.
        app.skip_splash();
        assert_eq!(app.screen, Screen::Boot);
        // Enough ticks to run the full boot window plus a small margin.
        let budget = BOOT_TOTAL_TICKS + 5;
        for _ in 0..budget {
            app.on_tick();
            if app.screen == Screen::Boot {
                app.boot_tick();
            }
        }
        assert_eq!(app.boot.revealed, BOOT_SCRIPT.len());
        assert_eq!(app.screen, Screen::Menu);
    }

    #[test]
    fn skip_boot_jumps_to_menu() {
        let mut app = App::new();
        app.skip_splash();
        app.skip_boot();
        assert_eq!(app.screen, Screen::Menu);
        assert_eq!(app.boot.revealed, BOOT_SCRIPT.len());
    }

    #[test]
    fn menu_enter_opens_selected_feature() {
        let mut app = App::new();
        app.skip_splash();
        app.skip_boot();
        // default selection is the first entry → Scan
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Scan);
        // Esc returns to the menu
        app.handle_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(app.screen, Screen::Menu);
    }
}
