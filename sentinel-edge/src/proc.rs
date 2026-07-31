//! Child-process helpers that need to differ per platform.

use std::ffi::OsStr;
use std::process::Command;

/// Build a `Command` that won't flash a console window on Windows — a drop-in for `Command::new`.
/// Returns an owned `Command`, so it slots straight into the existing chained builders
/// (`command("sh").args(…).spawn()`).
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    no_window(&mut cmd);
    cmd
}

/// Stop Windows from flashing a console window for a child process.
///
/// nvidia-smi. On Windows a GUI/service parent with no console makes each of those pop a black
/// console window — a jarring flicker on a customer's screen every time a clip records. The
/// `CREATE_NO_WINDOW` creation flag suppresses it. No-op on Unix, which has no such window.
pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
