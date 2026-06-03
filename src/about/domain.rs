//! The dossier content shown on the About screen.

/// Label / value rows for the ship + system registry. `env!` pulls the crate
/// version at compile time so the interface line always matches the build.
pub const DOSSIER: &[(&str, &str)] = &[
    ("VESSEL    ", "USCSS Nostromo  —  reg. 180924609"),
    ("CLASS     ", "Lockmart CM-88B Bison  ·  M-class starfreighter"),
    ("ROLE      ", "Commercial towing vehicle + ore refinery"),
    ("CREW      ", "7  ·  hypersleep capacity 7"),
    ("OWNER     ", "Weyland-Yutani Corp  —  \"Building Better Worlds\""),
    ("MAINFRAME ", "MU/TH/UR 6000  (MU-TH-UR Model 6000)"),
    ("SYNTHETIC ", "BISHOP/AP  —  Hyperdyne Systems 341-B"),
    ("PRIOR UNIT", "Hyperdyne 120-A2  —  decommissioned (see incident log)"),
    ("INTERFACE ", concat!("BISHOP/AP terminal  v", env!("CARGO_PKG_VERSION"))),
];

/// Flavor lines printed beneath the dossier.
pub const NOTES: &[&str] = &[
    "Behavioral inhibitors: ENGAGED      Asimov constraints: LOCKED",
    "",
    "\"I prefer the term 'Artificial Person' myself.\"",
];
