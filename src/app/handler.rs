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
        let (alert_tx, alert_rx) = std::sync::mpsc::channel();

        App {
            screen: Screen::Boot,
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
            alerts: crate::notification::AlertsScreen::new(),
            alert_tx,
            alert_rx,
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
        self.poll_alerts();
        if let Some(s) = self.live.as_mut() {
            let now = std::time::Instant::now();
            s.drain(now);
            s.supervise(now, &self.activity);
        }
        // Loop the "working" sound while processing (but never on the boot screen).
        let working = self.screen != Screen::Boot && self.activity.is_busy();
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
        // Begin "processing". The guard rides with the reply and is only dropped
        // once the reply's audio is ready (see `say`).
        let busy = self.activity.begin();
        std::thread::spawn(move || {
            // Live Claude (with tools) if configured, else the offline persona stub.
            let registry = ToolRegistry::new();
            let ctx = ToolContext { store, ui_tx, notif };
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
        if key.code == KeyCode::F(9) && self.screen != Screen::Boot {
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
                }
                KeyCode::Right
                | KeyCode::Char('l')
                | KeyCode::Char('+')
                | KeyCode::Char('=')
                | KeyCode::Enter => {
                    self.config.adjust(&mut self.settings, 1);
                    self.sync_levels();
                }
                _ => {}
            },
            Screen::Alerts => match key.code {
                KeyCode::Esc => self.screen = Screen::Menu,
                KeyCode::Up | KeyCode::Char('k') => {
                    let n = self.notification.lock().unwrap().len();
                    self.alerts.prev(n);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let n = self.notification.lock().unwrap().len();
                    self.alerts.next(n);
                }
                KeyCode::Enter | KeyCode::Char(' ') => self.toggle_alert(),
                KeyCode::Char('t') | KeyCode::Char('T') => self.test_alert(),
                KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Delete => self.delete_alert(),
                _ => {}
            },
        }
        false
    }

    // ---- ALERTS screen actions --------------------------------------------

    /// Drain background test-alert results into the on-screen status line.
    fn poll_alerts(&mut self) {
        while let Ok(status) = self.alert_rx.try_recv() {
            self.alerts.status = status;
        }
    }

    /// Toggle delivery on/off for the highlighted target.
    fn toggle_alert(&mut self) {
        if let Some((label, on)) =
            crate::notification::handler::toggle_enabled(&self.notification, self.alerts.selected)
        {
            self.alerts.status = format!("'{label}' {}", if on { "enabled" } else { "muted" });
        }
    }

    /// Remove the highlighted target.
    fn delete_alert(&mut self) {
        if let Some(label) =
            crate::notification::handler::delete_target(&self.notification, self.alerts.selected)
        {
            let len = self.notification.lock().unwrap().len();
            self.alerts.clamp(len);
            self.alerts.status = format!("Removed '{label}'.");
        }
    }

    /// Fire a test alert to the highlighted target on a background thread.
    fn test_alert(&mut self) {
        let Some(t) = self.notification.lock().unwrap().get(self.alerts.selected).cloned() else {
            self.alerts.status = "No target selected.".into();
            return;
        };
        let Some(cfg) = crate::notification::NotificationConfig::from_env() else {
            self.alerts.status = "Set TELEGRAM_BOT_TOKEN to send.".into();
            return;
        };
        self.alerts.status = format!("Sending test to '{}'\u{2026}", t.label);
        let store = self.notification.clone();
        let tx = self.alert_tx.clone();
        std::thread::spawn(move || {
            let msg = "BISHOP/AP test alert \u{2014} this target is paired and reachable.";
            let result = crate::notification::dispatch(&store, &cfg, Some(&t.label), msg, None);
            let _ = tx.send(result.unwrap_or_else(|e| format!("Test failed: {e}")));
        });
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
            if app.screen == Screen::Boot {
                app.boot_tick();
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
    fn boot_reveals_all_lines_then_opens_menu() {
        let mut app = App::new();
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
        app.skip_boot();
        assert_eq!(app.screen, Screen::Menu);
        assert_eq!(app.boot.revealed, BOOT_SCRIPT.len());
    }

    #[test]
    fn menu_enter_opens_selected_feature() {
        let mut app = App::new();
        app.skip_boot();
        // default selection is the first entry → Scan
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Scan);
        // Esc returns to the menu
        app.handle_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(app.screen, Screen::Menu);
    }
}
