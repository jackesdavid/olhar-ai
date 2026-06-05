//! Device-detail form behaviour: focus cycling and text editing.
use super::domain::{DeviceDetail, Field};

impl DeviceDetail {
    pub fn focus_next(&mut self) {
        self.field = match self.field {
            Field::Name => Field::Username,
            Field::Username => Field::Password,
            Field::Password => Field::Test,
            Field::Test => Field::Snapshot,
            Field::Snapshot => Field::Describe,
            Field::Describe => Field::Live,
            Field::Live => Field::Rename,
            Field::Rename => Field::Name,
        };
    }

    pub fn focus_prev(&mut self) {
        self.field = match self.field {
            Field::Name => Field::Rename,
            Field::Username => Field::Name,
            Field::Password => Field::Username,
            Field::Test => Field::Password,
            Field::Snapshot => Field::Test,
            Field::Describe => Field::Snapshot,
            Field::Live => Field::Describe,
            Field::Rename => Field::Live,
        };
    }

    /// Type into the focused text field (no-op on the action buttons).
    pub fn type_char(&mut self, c: char) {
        match self.field {
            Field::Name => self.name.push(c),
            Field::Username => self.username.push(c),
            Field::Password => self.password.push(c),
            _ => {}
        }
    }

    pub fn backspace(&mut self) {
        match self.field {
            Field::Name => {
                self.name.pop();
            }
            Field::Username => {
                self.username.pop();
            }
            Field::Password => {
                self.password.pop();
            }
            _ => {}
        }
    }
}
