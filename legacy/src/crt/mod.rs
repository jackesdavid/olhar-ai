//! The self-rendered CRT window frontend.
//!
//! Instead of borrowing the user's terminal, this renders the *exact same*
//! ratatui UI (`crate::app::view::draw`) into our own GPU window via the
//! `ratatui-wgpu` backend, with a built-in CRT post-processing shader
//! (curvature, scanlines, aperture-grille mask, phosphor ghosting). This is what
//! gives the Nostromo "glass" look out of the box — no terminal, no extra app.
//!
//! Windowing is `winit`; input arrives as winit key events and is translated to
//! the crossterm `KeyEvent`s that [`crate::app::App::handle_key`] already speaks,
//! so the app logic is shared verbatim with the terminal frontend.
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent as CtKeyEvent, KeyModifiers};
use ratatui::style::Color;
use ratatui::Terminal;
use ratatui_wgpu::shaders::CrtSettings;
use ratatui_wgpu::{Builder, Dimensions, Font, WgpuBackend};

mod video_overlay;
use video_overlay::{NostromoData, NostromoPost};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::app::domain::Screen;
use crate::app::{view, App};

/// Simulation tick — kept in lockstep with the terminal frontend (50ms).
const TICK: Duration = Duration::from_millis(50);

/// Minimum gap between redraws (~30fps). Without this the window repaints at
/// uncapped vsync (~60fps), redoing GPU + per-cell work far faster than either
/// the sonar sweep or the 12fps camera feed actually change.
const FRAME: Duration = Duration::from_millis(33);

/// A monospace font with a SIL OFL license, vendored so the binary is
/// self-contained.
const FONT: &[u8] = include_bytes!("../../assets/fonts/CascadiaMono-Regular.ttf");

type CrtTerminal = Terminal<WgpuBackend<'static, 'static, NostromoPost>>;

/// Launch the app in its own CRT window and run until the user quits.
pub fn run(app: App) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut state = CrtApp {
        app,
        window: None,
        terminal: None,
        mods: ModifiersState::empty(),
        last_tick: Instant::now(),
        last_redraw: Instant::now(),
    };
    event_loop.run_app(&mut state)?;
    Ok(())
}

struct CrtApp {
    app: App,
    window: Option<Arc<Window>>,
    terminal: Option<CrtTerminal>,
    mods: ModifiersState,
    last_tick: Instant,
    last_redraw: Instant,
}

impl CrtApp {
    /// CRT shader settings tuned for the Nostromo's green/amber phosphor look.
    fn nostromo_crt() -> CrtSettings {
        CrtSettings {
            brightness: 0.11,
            curve_factor: 1.1,        // gentle glass bulge
            ghost_factor: 0.22,       // phosphor smear / bloom
            scanline_factor: 0.6,     // pronounced scanlines
            mask_type: 2.0,           // aperture-grille (fine vertical phosphor stripes)
            mask_strength: 0.25,
            corner_radius_factor: 210.0,
            slow_fade: 0.0,           // no inter-frame fade — keeps text crisp
            ..CrtSettings::default()
        }
    }

    /// Run any simulation ticks that are due, then redraw through the shader.
    fn frame(&mut self) {
        let Self { app, terminal, last_tick, .. } = self;

        let now = Instant::now();
        while now.duration_since(*last_tick) >= TICK {
            app.on_tick();
            match app.screen {
                Screen::Splash => app.splash_tick(),
                Screen::Boot => app.boot_tick(),
                _ => {}
            }
            *last_tick += TICK;
        }

        if let Some(terminal) = terminal.as_mut() {
            // Clear last frame's compositor flag and record the grid size so the
            // GPU side can map the viewport's cell rect to screen coordinates;
            // the view sets the flag again if it renders a camera frame.
            let size = terminal.size().unwrap_or_default();
            crate::ui::video::begin_draw(size.width, size.height);
            let _ = terminal.draw(|f| view::draw(f, app));
        }
    }
}

impl ApplicationHandler for CrtApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("BISHOP/AP  —  MU/TH/UR 6000")
            // Larger default window → more cells at the same 20px font (higher
            // resolution, unchanged text size). The window re-grids on resize.
            .with_inner_size(LogicalSize::new(1600.0, 1000.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );

        let size = window.inner_size();
        let font = Font::new(FONT).expect("failed to load bundled font");
        // Install the shared compositor so the image renderer hands camera
        // frames to the GPU instead of drawing octants into the cell grid.
        let state = crate::ui::video::install();
        let user_data = NostromoData { crt: Self::nostromo_crt(), state };
        let backend = pollster::block_on(
            Builder::<NostromoPost>::from_font_and_user_data(font, user_data)
                .with_width_and_height(Dimensions {
                    width: NonZeroU32::new(size.width.max(1)).unwrap(),
                    height: NonZeroU32::new(size.height.max(1)).unwrap(),
                })
                .with_font_size_px(20)
                // The backend defaults to a WHITE reset background; force the
                // CRT to black phosphor so only lit text glows.
                .with_bg_color(Color::Black)
                .with_fg_color(crate::ui::theme::PHOSPHOR)
                .build_with_target(window.clone()),
        )
        .expect("failed to build wgpu backend");

        self.terminal = Some(Terminal::new(backend).expect("failed to build terminal"));
        self.window = Some(window);
        self.last_tick = Instant::now();
        self.last_redraw = Instant::now();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),

            WindowEvent::Resized(size) => {
                if let Some(terminal) = self.terminal.as_mut() {
                    terminal.backend_mut().resize(size.width, size.height);
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                // Ctrl-C always quits.
                if self.mods.control_key() {
                    if let Key::Character(s) = &event.logical_key {
                        if s.eq_ignore_ascii_case("c") {
                            event_loop.exit();
                            return;
                        }
                    }
                }
                if let Some(code) = map_key(&event.logical_key) {
                    let quit = self.app.handle_key(CtKeyEvent::new(code, to_crossterm_mods(self.mods)));
                    if quit {
                        event_loop.exit();
                    }
                }
            }

            WindowEvent::RedrawRequested => self.frame(),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Drive redraws, but no faster than ~30fps: request a paint when the
        // frame interval has elapsed, otherwise sleep until it does (instead of
        // busy-spinning at vsync). Keyboard events still wake the loop promptly,
        // so input stays responsive.
        let now = Instant::now();
        let next = self.last_redraw + FRAME;
        if now >= next {
            self.last_redraw = now;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
        }
    }
}

/// Translate winit modifier state into the crossterm `KeyModifiers` the app's
/// key handling inspects (word delete, line clear, Ctrl-C).
fn to_crossterm_mods(mods: ModifiersState) -> KeyModifiers {
    let mut out = KeyModifiers::empty();
    if mods.control_key() {
        out |= KeyModifiers::CONTROL;
    }
    if mods.alt_key() {
        out |= KeyModifiers::ALT;
    }
    if mods.shift_key() {
        out |= KeyModifiers::SHIFT;
    }
    if mods.super_key() {
        out |= KeyModifiers::SUPER;
    }
    out
}

/// Translate a winit logical key into the crossterm `KeyCode` the app expects.
fn map_key(key: &Key) -> Option<KeyCode> {
    match key {
        Key::Named(NamedKey::Enter) => Some(KeyCode::Enter),
        Key::Named(NamedKey::Escape) => Some(KeyCode::Esc),
        Key::Named(NamedKey::Backspace) => Some(KeyCode::Backspace),
        Key::Named(NamedKey::Delete) => Some(KeyCode::Delete),
        Key::Named(NamedKey::Tab) => Some(KeyCode::Tab),
        Key::Named(NamedKey::ArrowUp) => Some(KeyCode::Up),
        Key::Named(NamedKey::ArrowDown) => Some(KeyCode::Down),
        Key::Named(NamedKey::ArrowLeft) => Some(KeyCode::Left),
        Key::Named(NamedKey::ArrowRight) => Some(KeyCode::Right),
        Key::Named(NamedKey::Home) => Some(KeyCode::Home),
        Key::Named(NamedKey::End) => Some(KeyCode::End),
        Key::Named(NamedKey::Space) => Some(KeyCode::Char(' ')),
        Key::Named(NamedKey::F9) => Some(KeyCode::F(9)),
        Key::Character(s) => s.chars().next().map(KeyCode::Char),
        _ => None,
    }
}
