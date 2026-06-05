//! Conversation state and Bishop's canned lines.
use rand::Rng;
use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::agent::Message;
use crate::ui::theme::PHOSPHOR;

/// Bishop's spoken greetings — one is chosen at random when the interface comes
/// online.
pub const GREETINGS: &[&str] = &[
    "BISHOP/AP online. All systems nominal — I'm at your disposal.",
    "Interface 2037 ready. I prefer the term 'artificial person,' but Bishop will do.",
    "Welcome back. I kept watch while you were away; nothing to report.",
    "Diagnostics complete. I'm functioning within normal parameters.",
    "The tracker's been quiet. Tell me what you need.",
    "Online and accounted for. Take your time — I don't tire.",
    "Powering up... steady now. How can I help?",
    "Everything is as you left it. Shall we begin?",
    "I'm here to assist, and to see that no harm comes to you. What's first?",
    "Good to see you. I'm ready when you are.",
];

/// Pick a greeting at random for this session.
pub fn random_greeting() -> &'static str {
    let i = rand::thread_rng().gen_range(0..GREETINGS.len());
    GREETINGS[i]
}

/// A reply being revealed one character at a time (80s teletype style). `shown`
/// counts how many characters of `full` are currently visible.
pub struct TypeLine {
    pub full: String,
    pub shown: usize,
}

/// The scrollback log, the line being composed, the message history sent to the
/// model, whether we're awaiting a reply, and any in-progress typewriter line.
pub struct Conversation {
    pub log: Vec<Line<'static>>,
    pub input: String,
    /// Byte offset of the edit cursor within `input`.
    pub cursor: usize,
    /// Turns sent to / received from Claude. The greeting is intentionally NOT
    /// here — the API requires the first message to be from the user.
    pub history: Vec<Message>,
    /// True while a reply is in flight (drives the "working…" indicator).
    pub pending: bool,
    /// Bishop's reply currently typing out, if any.
    pub typing: Option<TypeLine>,
}

impl Conversation {
    pub fn new() -> Self {
        Conversation {
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            pending: false,
            typing: None,
            log: vec![Line::from(Span::styled(
                "BISHOP/AP ONLINE. INTERFACE 2037 READY FOR INQUIRY.",
                Style::default().fg(PHOSPHOR),
            ))],
        }
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}
