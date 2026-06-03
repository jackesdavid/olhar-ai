//! BISHOP/AP — a retro Weyland-Yutani style terminal assistant with a live
//! "motion tracker" sonar, built with ratatui.
//!
//! Controls:  type to compose an inquiry · [Enter] send · [Esc]/Ctrl-C quit
//!
//! The codebase is organized as vertical slices, one folder per concern. After
//! boot the app shows a home menu that opens one of three feature screens:
//!   app/        aggregate state, screen routing, run loop, top-level view
//!   boot/       the Nostromo boot sequence
//!   menu/       the home menu (feature list + briefing panel)
//!   scan/       feature: the motion-tracker device scan
//!   chat/       feature: text conversation with BISHOP/AP
//!   about/      feature: ship dossier & system versions
//!   audio/      sound effects + the playback device (adapter)
//!   voice/      ElevenLabs text-to-speech (client)
//!   ui/         shared theme + window chrome
//!
//! Each slice follows the same layering: `domain` (data/types), `handler`
//! (logic), `view` (rendering), and `adapter`/`client` for external IO.

mod about;
mod agent;
mod app;
mod audio;
mod boot;
mod chat;
mod config;
#[cfg(feature = "crt")]
mod crt;
mod device;
mod listen;
mod map;
mod menu;
mod notification;
mod onvif;
mod scan;
mod stream;
mod ui;
#[cfg(feature = "audio")]
mod voice;

use app::App;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env (Claude + ElevenLabs keys) into the environment, so `cargo run`
    // works the same as F5. Ignored if the file is absent.
    let _ = dotenvy::dotenv();
    quiet_stderr();

    // The self-rendered CRT window is the default experience. Pass `--tty` to
    // run as a plain terminal app instead (SSH, no GPU, scripting).
    #[cfg(feature = "crt")]
    {
        if !std::env::args().any(|a| a == "--tty") {
            crt::run(App::new())?;
            return Ok(());
        }
    }

    // Plain-terminal frontend: the user's terminal font almost certainly lacks
    // the Unicode 16 octant glyphs the CRT window uses, so fall back to the
    // universally-supported quadrant set for camera images.
    ui::image::use_quadrant_glyphs();

    let mut terminal = ratatui::init();
    let result = app::handler::run(&mut terminal, App::new());
    ratatui::restore();
    result?;
    Ok(())
}

/// The TUI owns stdout's alternate screen, but macOS frameworks (CoreAudio,
/// Security.framework) and os_log mirror noise like `SecTaskLoadEntitlements`
/// to **stderr** during audio/TLS init — which would paint over the interface.
/// Point stderr at a log file (`$TMPDIR/bishop.log`) for the life of the
/// process so the screen stays clean while panics/errors remain inspectable.
#[cfg(unix)]
fn quiet_stderr() {
    use std::os::unix::io::AsRawFd;

    let path = std::env::temp_dir().join("bishop.log");
    if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        // dup2 makes fd 2 (stderr) refer to the log file. The original `file`
        // fd is then redundant and closed on drop; fd 2 keeps the redirection.
        unsafe {
            libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO);
        }
    }
}

#[cfg(not(unix))]
fn quiet_stderr() {}
