//! The ALERTS subsystem: a ship-wide alert log with its own storage
//! (`alerts.json`), a dashboard of today's events with level/location stats, a
//! manual create form, and an explicit "deliver to beacons" action that records
//! who received each alert and when. Bishop also drives it through agent tools.
//!
//! `domain` holds the [`Alert`] model + store + stats helpers; `handler` owns the
//! dashboard screen state (Browse/Create); `actions` are the create/deliver/delete
//! operations shared by the UI and the tools; `view` renders it.
pub mod actions;
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::{AlertLevel, AlertStore};
pub use handler::{AlertsMode, AlertsScreen, DraftField};
