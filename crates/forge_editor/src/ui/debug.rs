//! The Debug menu above the viewport, the overlays drawn over it, and the system that feeds them
//! what only the editor knows (the data model, the running game's script host).
//!
//! The overlays themselves live in `forge_engine::diag` (they are shared with the player); this is
//! the editor's side: where the menu sits, which shortcut it shows next to each overlay, and how the
//! settings are saved.

use bevy::prelude::*;
use bevy_egui::egui;
use forge_engine::diag::{self, Metrics};

use crate::{
    keymap::Action,
    state::{Editor, SettingsTab},
};

/// The strip at the top of the Viewport panel (and of its Game tab), with the Debug dropdown.
pub(super) fn viewport_bar(ui: &mut egui::Ui, ed: &mut Editor) {
    let width = ui.available_width();
    egui::Frame::new().fill(ui.visuals().panel_fill).show(ui, |ui| {
        ui.set_min_size(egui::vec2(width, 24.0));
        egui::MenuBar::new().ui(ui, |ui| {
            // (The two borrows are of different fields of the settings.)
            let keymap = &ed.settings.keymap;
            // (A single-player game has no network to report on: its Network Statistics overlay is not offered.)
            let hide_network = ed.project.as_ref().is_some_and(|p| !p.manifest.game_type().is_multiplayer());
            let out = diag::ui::menu_button_without(ui, &mut ed.settings.debug, &|o| keymap.get(Action::Debug(o)).first().map(|c| c.display()), &|o| hide_network && o == diag::Overlay::Network);
            if out.changed {
                ed.settings.touch();
            }
            if out.open_settings {
                ed.open_editor_settings();
                ed.settings_ui.tab = SettingsTab::Debug;
            }
            if out.clear_data {
                ed.clear_debug_data = true;
            }
        });
    });
}

/// The overlays that are on, over `viewport`.
pub(super) fn overlays(ctx: &egui::Context, ed: &mut Editor, viewport: egui::Rect, metrics: &Metrics) {
    if diag::ui::overlays(ctx, viewport, &mut ed.settings.debug, metrics) {
        ed.settings.touch();
    }
}

/// Runs every frame, after the game was stepped: tells the collectors what the settings ask for and
/// hands them what the editor owns (instances, the script host's timings, physics statistics).
pub fn debug_feed(mut ed: NonSendMut<Editor>, mut metrics: ResMut<Metrics>) {
    let ed = &mut *ed;
    metrics.apply(&ed.settings.debug);
    if std::mem::take(&mut ed.clear_debug_data) {
        metrics.clear();
    }
    let clear = std::mem::take(&mut metrics.clear_requested);

    let wants = metrics.wants;
    if metrics.sampling && wants.instances {
        let dm = ed.dm.borrow();
        metrics.instances = Some((dm.instance_count() as u32, dm.component_count("ScriptRunner") as u32));
    }
    match ed.play.as_ref() {
        Some(play) => {
            play.host.set_profiling(wants.script_costs);
            if clear {
                play.host.clear_profile();
            }
            if metrics.timings.enabled {
                let (scripts_ms, physics_ms) = play.host.frame_ms();
                metrics.record_script_frame(scripts_ms, physics_ms);
            }
            if metrics.sampling {
                metrics.scripts = wants.scripts.then(|| play.host.profile());
                metrics.physics = wants.physics.then(|| play.host.physics_stats());
            }
        }
        None => {
            // No game, no scripts to measure.
            metrics.scripts = None;
            metrics.physics = None;
        }
    }
}
