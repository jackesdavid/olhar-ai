//! Menu state: the list of entries and which one is highlighted. Each entry
//! knows the [`Screen`] it opens and carries the helper copy shown on the right.
use crate::app::domain::Screen;

/// One selectable entry in the home menu.
pub struct MenuItem {
    pub label: &'static str,           // shown in the left-hand list
    pub blurb: &'static str,           // one-line tagline under the label
    pub help: &'static [&'static str], // descriptive copy for the right panel
    pub target: Screen,                // screen opened on [Enter]
}

/// The home menu: its entries and the current selection.
pub struct Menu {
    pub items: Vec<MenuItem>,
    pub selected: usize,
}

impl Menu {
    pub fn new() -> Self {
        Menu { items: entries(), selected: 0 }
    }

    pub fn selected_item(&self) -> &MenuItem {
        &self.items[self.selected]
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

/// The fixed set of features reachable from the home screen.
fn entries() -> Vec<MenuItem> {
    vec![
        MenuItem {
            label: "SCANNER",
            blurb: "motion tracker / perimeter sweep",
            target: Screen::Scan,
            help: &[
                "MOTION TRACKER  —  M314 PULSE ARRAY",
                "",
                "Active sweep of the local volume. The array resolves",
                "range and bearing on every mover it can see —",
                "personnel, drones, and unidentified contacts.",
                "",
                "Contacts flare as the sweep crosses them, then fade.",
                "The TRACKING readout locks onto the strongest signal.",
            ],
        },
        MenuItem {
            label: "CHAT",
            blurb: "converse with BISHOP/AP",
            target: Screen::Chat,
            help: &[
                "BISHOP/AP  —  ARTIFICIAL PERSON",
                "",
                "A direct text link to the ship's synthetic. Ask for a",
                "status report, a read on the tracker, or simply talk.",
                "",
                "Replies are generated locally for now; a live model",
                "uplink is scheduled for a future refit.",
            ],
        },
        MenuItem {
            label: "CONFIG",
            blurb: "ship systems & sound levels",
            target: Screen::Config,
            help: &[
                "SHIP CONFIGURATION  —  ENVIRONMENT CONTROL",
                "",
                "Trim the ship's audio environment and interface to",
                "taste: master and per-channel volume, Bishop's voice,",
                "the teletype cadence, and the voice-command link.",
                "",
                "Changes take hold at once and are logged to the ship's",
                "registry for the next power-up.",
            ],
        },
        MenuItem {
            label: "ALERTS",
            blurb: "remote notification targets",
            target: Screen::Alerts,
            help: &[
                "ALERT DISPATCH  —  REMOTE NOTIFICATION",
                "",
                "Trusted phones that receive Bishop's alerts over the",
                "comm link. A handset is enrolled by sending the pairing",
                "code to the link; from here you can mute, remove, or",
                "test each target.",
                "",
                "Bishop also sends alerts here on request, and scheduled",
                "workflows will dispatch through the same channel.",
            ],
        },
        MenuItem {
            label: "MAP",
            blurb: "deck schematic & camera coverage",
            target: Screen::Map,
            help: &[
                "DECK SCHEMATIC  —  COMPARTMENT MAP",
                "",
                "Lay out the deck compartment by compartment and assign",
                "the cameras that cover each. Add a room, then move and",
                "resize it on the grid until the plan matches the deck.",
                "",
                "Each room records real geometry, so a future refit can",
                "plot the contacts its cameras resolve inside it.",
            ],
        },
        MenuItem {
            label: "ABOUT",
            blurb: "ship dossier & system versions",
            target: Screen::About,
            help: &[
                "SYSTEM DOSSIER",
                "",
                "Registry, hardware, and firmware versions for the",
                "USCSS Nostromo and her resident intelligence.",
                "",
                "Plus build and credits for this interface.",
            ],
        },
    ]
}
