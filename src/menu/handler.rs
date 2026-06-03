//! Menu navigation: moving the selection and resolving the chosen target.
use super::domain::Menu;
use crate::app::domain::Screen;

impl Menu {
    /// Move the highlight down one entry, wrapping at the bottom.
    pub fn next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    /// Move the highlight up one entry, wrapping at the top.
    pub fn prev(&mut self) {
        let n = self.items.len();
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
        }
    }

    /// The screen the highlighted entry opens.
    pub fn target(&self) -> Screen {
        self.selected_item().target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_wraps_both_ways() {
        let mut m = Menu::new();
        let last = m.items.len() - 1;
        assert_eq!(m.selected, 0);
        m.prev();
        assert_eq!(m.selected, last); // wrapped to the bottom
        m.next();
        assert_eq!(m.selected, 0); // back to the top
    }

    #[test]
    fn target_tracks_selection() {
        let mut m = Menu::new();
        assert_eq!(m.target(), Screen::Scan);
        m.next();
        assert_eq!(m.target(), Screen::Chat);
        m.next();
        assert_eq!(m.target(), Screen::Config);
    }
}
