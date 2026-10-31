//! Bevy APIs that tend to change between releases live here, so an upgrade
//! touches one file instead of the whole codebase.
#![allow(deprecated)]

use bevy::{app::AppExit, prelude::*};
use bevy_egui::egui;

pub type ExitWriter<'w> = MessageWriter<'w, AppExit>;

pub fn request_exit(writer: &mut ExitWriter<'_>) {
    writer.write(AppExit::Success);
}

/// The root `Ui` of an egui frame. Since egui 0.36 panels (`Panel`, `CentralPanel`) are shown inside a `Ui` rather
/// than on the `Context`: they take their share of this one, in the order they are shown, exactly as they used to
/// take it from the context. (A system gets the context from `EguiContexts` and makes this once per frame.)
pub fn root_ui(ctx: &egui::Context) -> egui::Ui {
    egui::Ui::new(ctx.clone(), egui::Id::new("forge_root_ui"), egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()))
}

/// Runs one egui frame with `ui`-less code that wants the `Context` (the tests of everything drawn into egui).
pub fn run_frame(ctx: &egui::Context, raw: egui::RawInput, mut f: impl FnMut(&egui::Context)) -> egui::FullOutput {
    let mut out = ctx.run_ui(raw, |ui| {
        let ctx = ui.ctx().clone();
        f(&ctx)
    });
    // (Nothing here paints, so there is no texture to upload: egui insists the deltas are handled or cleared.)
    out.textures_delta.clear();
    out
}

/// `ctx.run_frame(raw, |ctx| ...)`: [`run_frame`] as a method, where `Context::run` used to be called.
pub trait RunFrame {
    fn run_frame(&self, raw: egui::RawInput, f: impl FnMut(&egui::Context)) -> egui::FullOutput;
}

impl RunFrame for egui::Context {
    fn run_frame(&self, raw: egui::RawInput, f: impl FnMut(&egui::Context)) -> egui::FullOutput {
        run_frame(self, raw, f)
    }
}

/// Despawns an entity and its children; silently ignores entities that are already gone.
pub fn despawn(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).try_despawn();
}

/// Attaches `child` to `parent` in the transform hierarchy.
pub fn set_parent(commands: &mut Commands, parent: Entity, child: Entity) {
    commands.entity(parent).add_child(child);
}

/// Inserts components on an entity, ignoring entities that no longer exist.
pub fn insert<B: Bundle>(commands: &mut Commands, entity: Entity, bundle: B) {
    commands.entity(entity).try_insert(bundle);
}

pub fn remove<B: Bundle>(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).try_remove::<B>();
}
