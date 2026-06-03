//! Bishop's behaviour: the offline persona and how an inquiry becomes a reply.
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::domain::{Conversation, TypeLine};
use crate::agent::Message;
use crate::device::actions::DeviceSummary;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM};

impl Conversation {
    /// Append a `BISHOP:` line to the log. Log-only — used by the startup
    /// greeting (which must stay out of the API history).
    pub fn push_bishop(&mut self, text: &str) {
        self.log.push(Line::from(vec![
            Span::styled("BISHOP: ", Style::default().fg(AMBER)),
            Span::styled(text.to_string(), Style::default().fg(PHOSPHOR)),
        ]));
        self.trim();
    }

    /// Append a dim system notice to the log (e.g. voice-link status). Log-only
    /// — kept out of the API history.
    pub fn push_system(&mut self, text: &str) {
        self.log.push(Line::from(Span::styled(
            format!("\u{00b7} {text}"),
            Style::default().fg(PHOSPHOR_DIM),
        )));
        self.trim();
    }

    /// Replace the input box contents, putting the cursor at the end. Used to
    /// park a transcript for review when speech auto-send is off.
    pub fn set_input(&mut self, text: &str) {
        self.input = text.to_string();
        self.cursor = self.input.len();
    }

    /// Push a styled device-list block (from the `list_devices` tool).
    pub fn push_list_block(&mut self, summaries: &[DeviceSummary]) {
        self.log.push(Line::from(Span::styled(
            "DEVICES",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        )));
        if summaries.is_empty() {
            self.log.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(PHOSPHOR_DIM),
            )));
        }
        for (i, s) in summaries.iter().enumerate() {
            let mark = if s.has_credentials { "\u{2713}" } else { "\u{00b7}" };
            self.log.push(Line::from(vec![
                Span::styled(format!("  {}. {mark} ", i + 1), Style::default().fg(PHOSPHOR_DIM)),
                Span::styled(s.name.clone(), Style::default().fg(PHOSPHOR)),
                Span::styled(format!("  {}", s.ip), Style::default().fg(PHOSPHOR_DIM)),
            ]));
        }
        self.trim();
    }

    /// Echo the operator's line and record it in the API history; mark a reply
    /// as pending.
    pub fn begin_user(&mut self, text: &str) {
        self.log.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(text.to_string(), Style::default().fg(PHOSPHOR)),
        ]));
        self.history.push(Message::user(text));
        self.pending = true;
        self.trim();
    }

    /// Start revealing a reply one character at a time.
    pub fn start_typing(&mut self, text: String) {
        self.typing = Some(TypeLine { full: text, shown: 0 });
    }

    /// Reveal `n` more characters of the typing line. Returns `true` once the
    /// whole line is visible.
    pub fn type_step(&mut self, n: usize) -> bool {
        match self.typing.as_mut() {
            Some(t) => {
                let total = t.full.chars().count();
                t.shown = (t.shown + n).min(total);
                t.shown >= total
            }
            None => true,
        }
    }

    /// Whether the typing line is fully revealed (and ready to commit).
    pub fn typing_complete(&self) -> bool {
        self.typing
            .as_ref()
            .is_some_and(|t| t.shown >= t.full.chars().count())
    }

    /// Commit the fully-typed line to the log and clear the typewriter.
    pub fn finish_typing(&mut self) {
        if let Some(t) = self.typing.take() {
            self.push_bishop(&t.full);
        }
    }

    // ---- cursor-aware line editing ---------------------------------------

    /// Insert a character at the cursor.
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete the character before the cursor (plain Backspace).
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.prev_boundary();
        self.input.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    /// Delete the word before the cursor (Ctrl/Alt-Backspace, Ctrl-W).
    pub fn delete_word(&mut self) {
        let target = self.word_left_target();
        self.input.replace_range(target..self.cursor, "");
        self.cursor = target;
    }

    /// Clear the whole input line (Cmd-Backspace, Ctrl-U).
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }

    pub fn move_left(&mut self) {
        self.cursor = self.prev_boundary();
    }
    pub fn move_right(&mut self) {
        self.cursor = self.next_boundary();
    }
    /// Jump the cursor back one word (Ctrl/Alt-Left).
    pub fn word_left(&mut self) {
        self.cursor = self.word_left_target();
    }
    /// Jump the cursor forward one word (Ctrl/Alt-Right).
    pub fn word_right(&mut self) {
        self.cursor = self.word_right_target();
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.input.len();
    }

    fn prev_boundary(&self) -> usize {
        self.input[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
    fn next_boundary(&self) -> usize {
        self.input[self.cursor..]
            .chars()
            .next()
            .map(|c| self.cursor + c.len_utf8())
            .unwrap_or(self.cursor)
    }
    fn word_left_target(&self) -> usize {
        let left = &self.input[..self.cursor];
        let end = left.trim_end_matches(|c: char| c == ' ' || c == '\t').len();
        self.input[..end]
            .rfind(|c: char| c == ' ' || c == '\t')
            .map(|i| i + 1)
            .unwrap_or(0)
    }
    fn word_right_target(&self) -> usize {
        let right = &self.input[self.cursor..];
        let after_word = right.trim_start_matches(|c: char| c != ' ' && c != '\t');
        let after_ws = after_word.trim_start_matches(|c: char| c == ' ' || c == '\t');
        self.input.len() - after_ws.len()
    }

    fn trim(&mut self) {
        if self.log.len() > 200 {
            self.log.drain(0..self.log.len() - 200);
        }
    }
}

/// Offline Bishop persona. Swap this for a real LLM call to go live.
pub fn reply(input: &str) -> String {
    let q = input.to_lowercase();
    if q.contains("status") || q.contains("report") {
        "All systems nominal. I'm monitoring the perimeter. No immediate cause for alarm.".into()
    } else if q.contains("scan") || q.contains("motion") || q.contains("contact") {
        "I read several signals on the tracker. Mostly stationary. I'll keep watching them.".into()
    } else if q.contains("hello") || q == "hi" || q.starts_with("hi ") {
        "Hello. I prefer the term 'artificial person' myself. How can I assist you?".into()
    } else {
        "Understood. I'm programmed to assist -- and never, through action or inaction, to let you come to harm.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bishop_stays_in_character() {
        assert!(reply("hello").contains("artificial person"));
        assert!(!reply("what is your status").is_empty());
    }

    #[test]
    fn echo_user_records_history_and_pending() {
        let mut convo = Conversation::new();
        let before = convo.log.len();
        convo.begin_user("status");
        assert!(convo.pending);
        assert_eq!(convo.history.len(), 1); // user turn recorded
        assert_eq!(convo.log.len(), before + 1); // echo line
    }

    #[test]
    fn typewriter_reveals_then_commits() {
        let mut convo = Conversation::new();
        let before = convo.log.len();
        convo.start_typing("All nominal.".into());
        assert!(!convo.typing_complete());
        // reveal three chars at a time until done
        while !convo.type_step(3) {}
        assert!(convo.typing_complete());
        convo.finish_typing();
        assert!(convo.typing.is_none());
        assert_eq!(convo.log.len(), before + 1); // committed exactly one line
    }

    #[test]
    fn cursor_editing() {
        let mut convo = Conversation::new();
        for c in "scan the perimeter".chars() {
            convo.insert_char(c);
        }
        assert_eq!(convo.cursor, convo.input.len());
        // jump back a word, then forward a word
        convo.word_left();
        assert_eq!(convo.cursor, "scan the ".len());
        convo.word_right();
        assert_eq!(convo.cursor, convo.input.len());
        // delete the word before the cursor
        convo.delete_word();
        assert_eq!(convo.input, "scan the ");
        // insert in the middle
        convo.home();
        convo.move_right();
        convo.insert_char('X');
        assert_eq!(convo.input, "sXcan the ");
        convo.clear_input();
        assert!(convo.input.is_empty() && convo.cursor == 0);
    }
}
