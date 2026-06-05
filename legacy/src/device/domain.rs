//! Device-detail screen state: which device, the credential form, status line,
//! and the decoded snapshot.
use crate::ui::image::CameraImage;

/// Focusable element on the detail form. Tab/arrows cycle through these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Username,
    Password,
    Test,
    Snapshot,
    Describe,
    Live,
    Rename,
}

pub struct DeviceDetail {
    /// IP of the device being edited — the stable identity (a list reorder or a
    /// chat-driven delete can't corrupt it, unlike a positional index).
    pub ip: String,
    pub field: Field,
    /// Editable display name (committed via the RENAME action).
    pub name: String,
    pub username: String,
    pub password: String,
    /// Last status / error line (e.g. "Credentials verified and saved.").
    pub status: String,
    /// Decoded snapshot, ready for monochrome rendering.
    pub image: Option<CameraImage>,
}

impl DeviceDetail {
    /// Open the form for the device at `ip`, pre-filling its name + credentials.
    pub fn new(ip: String, name: String, username: String, password: String) -> Self {
        DeviceDetail {
            ip,
            field: Field::Name,
            name,
            username,
            password,
            status: String::new(),
            image: None,
        }
    }
}
