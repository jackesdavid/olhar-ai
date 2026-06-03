//! The ALERTS dashboard screen state: a Browse mode (today's alerts + stats) and
//! a Create mode (a form to raise an alert by hand). The alert data itself lives
//! in the shared [`AlertStore`](super::domain::AlertStore); this owns only the
//! cursor, the draft, and a status line.
use super::domain::AlertLevel;

/// Which sub-mode the dashboard is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertsMode {
    Browse,
    Create,
}

/// Focusable element on the CREATE form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftField {
    Level,
    Room,
    Source,
    Message,
    Submit,
}

/// The in-progress manual alert.
pub struct Draft {
    pub level: AlertLevel,
    /// Index into the rooms snapshot, or `None` for a system-wide alert.
    pub room_idx: Option<usize>,
    pub source: String,
    pub message: String,
    pub field: DraftField,
}

impl Draft {
    fn fresh() -> Self {
        Draft {
            level: AlertLevel::Info,
            room_idx: None,
            source: "operator".into(),
            message: String::new(),
            field: DraftField::Level,
        }
    }

    pub fn focus_next(&mut self) {
        self.field = match self.field {
            DraftField::Level => DraftField::Room,
            DraftField::Room => DraftField::Source,
            DraftField::Source => DraftField::Message,
            DraftField::Message => DraftField::Submit,
            DraftField::Submit => DraftField::Level,
        };
    }

    pub fn focus_prev(&mut self) {
        self.field = match self.field {
            DraftField::Level => DraftField::Submit,
            DraftField::Room => DraftField::Level,
            DraftField::Source => DraftField::Room,
            DraftField::Message => DraftField::Source,
            DraftField::Submit => DraftField::Message,
        };
    }

    /// Left/Right on the focused selector: cycle the level or the room.
    pub fn adjust(&mut self, delta: i8, rooms_len: usize) {
        match self.field {
            DraftField::Level => self.level = self.level.cycled(delta),
            DraftField::Room => self.cycle_room(delta, rooms_len),
            _ => {}
        }
    }

    /// Cycle the room selection across `[none, room 0 .. room n-1]`.
    fn cycle_room(&mut self, delta: i8, rooms_len: usize) {
        let n = (rooms_len + 1) as i32; // +1 for the "none" slot
        let cur = match self.room_idx {
            None => 0,
            Some(i) => i as i32 + 1,
        };
        let next = (cur + delta as i32).rem_euclid(n);
        self.room_idx = if next == 0 { None } else { Some((next - 1) as usize) };
    }

    pub fn type_char(&mut self, c: char) {
        match self.field {
            DraftField::Source => self.source.push(c),
            DraftField::Message => self.message.push(c),
            _ => {}
        }
    }

    pub fn backspace(&mut self) {
        match self.field {
            DraftField::Source => {
                self.source.pop();
            }
            DraftField::Message => {
                self.message.pop();
            }
            _ => {}
        }
    }
}

/// Transient UI state for the ALERTS dashboard.
pub struct AlertsScreen {
    pub mode: AlertsMode,
    pub selected: usize,
    pub status: String,
    pub draft: Draft,
}

impl AlertsScreen {
    pub fn new() -> Self {
        AlertsScreen {
            mode: AlertsMode::Browse,
            selected: 0,
            status: String::new(),
            draft: Draft::fresh(),
        }
    }

    /// Move the Browse cursor (over `len` = today's alert count).
    pub fn next(&mut self, len: usize) {
        if len > 0 {
            self.selected = (self.selected + 1).min(len - 1);
        }
    }

    pub fn prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn clamp(&mut self, len: usize) {
        self.selected = if len == 0 { 0 } else { self.selected.min(len - 1) };
    }

    pub fn enter_create(&mut self) {
        self.draft = Draft::fresh();
        self.mode = AlertsMode::Create;
        self.status.clear();
    }

    pub fn cancel_create(&mut self) {
        self.mode = AlertsMode::Browse;
        self.status.clear();
    }
}

impl Default for AlertsScreen {
    fn default() -> Self {
        Self::new()
    }
}
