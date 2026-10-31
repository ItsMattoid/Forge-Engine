//! Debugging and performance monitoring: the Debug menu's overlays.
//!
//! * [`overlay`] - what can be switched on, and the settings (shared with the editor's keymap)
//! * [`frames`]  - frame-time history and the statistics worked out of it
//! * [`metrics`] - everything the overlays read, in one resource
//! * [`plugin`]  - the Bevy systems that collect it
//! * [`log`]     - warnings and errors reported while running
//! * [`ui`]      - the egui panels, the menu and the settings controls (used by the editor and the player)
//!
//! Only the frame times are recorded all the time (one push on a ring buffer per frame). Every other
//! measurement is switched on by the overlay that shows it, and refreshed a few times per second.

pub mod frames;
pub mod log;
pub mod metrics;
pub mod overlay;
pub mod plugin;
pub mod ui;

pub use metrics::{timed, timing, Metrics};
pub use overlay::{DebugSettings, Dock, Group, Overlay, OverlaySet, Placement, Wants, OVERLAYS};
pub use plugin::{DiagEnd, DiagPlugin};
