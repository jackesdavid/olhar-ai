//! Boot-sequence state and the script it scrolls.

// Timing is matched to assets/boot_loop.wav (≈30s) at the app's 50ms tick:
//   - lines are revealed across the first BOOT_REVEAL_TICKS,
//   - then the screen holds on READY until BOOT_TOTAL_TICKS,
//   - then the app hands off to the home menu (and the hum is hushed).
pub const BOOT_TOTAL_TICKS: u32 = 600; // 30s — full length of the boot hum
pub const BOOT_REVEAL_TICKS: u32 = 520; // reveal everything over the first ~26s

/// The fake Nostromo-style boot scroll. Inspired by the film's look
/// (registration number, self-tests, MU/TH/UR handshake, the infamous Special
/// Order) without copying any exact on-screen text. The trailing line triggers
/// the "ready" chime.
pub const BOOT_SCRIPT: &[&str] = &[
    "WEYLAN-YUTANI CORP  -  USCSS NOSTROMO 180924609",
    "",
    "MU/TH/UR 6000  MAINFRAME ......... ONLINE",
    "COLD START / POWER-ON SELF TEST",
    "",
    "  CORE MEMORY ................... 2.1 TB  OK",
    "  PARITY / ECC SCRUB ............ PASS",
    "  REACTION CONTROL SYS .......... OK",
    "  MAIN BUS  A / B ............... NOMINAL",
    "  FUSION PLANT .................. 100%   OK",
    "  LIFE SUPPORT .................. NOMINAL",
    "  ATMOS / CO2 SCRUBBERS ......... OK",
    "  NAV / DEEP SLEEP MONITOR ...... OK",
    "  HYPERSLEEP BAY  7 CAPSULES .... SEALED",
    "  COMMS ARRAY / ANTENNA ......... ALIGNED",
    "  SENSOR ARRAY / MOTION TRACKER . CALIBRATED",
    "",
    "LOADING ARTIFICIAL-PERSON RUNTIME  [ BISHOP/AP ]",
    "  NEURAL EMULATION CORE ......... WARM",
    "  ETHICS GOVERNOR ............... ENGAGED",
    "  ASIMOV CONSTRAINTS ............ LOCKED",
    "  BEHAVIORAL INHIBITORS ......... ENGAGED",
    "",
    "  DECRYPTING SPECIAL ORDER 937 .. ACCESS DENIED",
    "",
    "INTERFACE 2037 READY FOR INQUIRY",
];

/// Tracks how far the boot scroll has progressed. Behaviour lives in
/// [`super::handler`].
pub struct BootSequence {
    pub revealed: usize,      // how many BOOT_SCRIPT lines are visible
    pub elapsed: u32,         // ticks since the sequence began
    pub done_at: Option<u32>, // tick on which the last line landed
}

impl BootSequence {
    pub fn new() -> Self {
        BootSequence { revealed: 0, elapsed: 0, done_at: None }
    }
}

impl Default for BootSequence {
    fn default() -> Self {
        Self::new()
    }
}
