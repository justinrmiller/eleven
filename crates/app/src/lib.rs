//! The eleven application: controller, UI and platform integration.
//!
//! The binary (`main.rs`) only wires these together; everything here is testable without a
//! window or sound card.

pub mod backend;
pub mod controller;
pub mod media_keys;
pub mod paths;

pub use controller::{Controller, Persistence};
pub mod ui;

pub use ui::ElevenApp;
