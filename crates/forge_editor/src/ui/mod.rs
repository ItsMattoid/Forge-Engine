//! All egui panels. Everything here reads and edits the [`Editor`] state; none of it
//! is specific to any class (the Properties panel and the Insert menu are generated
//! from class metadata).
//!
//! * [`menus`]     - menu bar (File, View, ...), toolbar, the Insert menu
//! * [`panels`]    - the dock: every panel of the editor as a tab that can be moved, split and floated
//! * [`settings`]  - the editor settings window (shortcuts, camera, snapping, colours, debug overlays)
//! * [`project_settings`] - the project settings window
//! * [`window_memory`] - those two windows are not dockable; this puts them back where they were
//! * [`debug`]     - the Debug menu at the top of the viewport and the overlays over it
//! * [`popups`]    - the insert-object search and the snapping-value popups
//! * [`explorer`]  - the instance tree
//! * [`inspector`] - the Properties panel
//! * [`assets`]    - the Asset Browser
//! * [`bottom`]    - the Output and Script Editor panels
//! * [`dialogs`]   - welcome window and modal dialogs

mod asset_field;
mod asset_picker;
mod assets;
mod bottom;
mod debug;
mod dialogs;
mod explorer;
mod inspector;
mod material_editor;
mod material_preview;
mod menus;
mod panels;
mod popups;
pub(crate) mod project_settings;
pub(crate) mod settings;
mod window_memory;

use bevy::{prelude::*, window::WindowCloseRequested};
use bevy_egui::{egui, EguiContexts};
use forge_engine::{compat::ExitWriter, diag::{timing, Metrics}};

use crate::{
    keymap::{Context, Frame},
    state::Editor,
    viewport::ViewportRect,
};

pub fn save_on_close(mut events: MessageReader<WindowCloseRequested>, mut ed: NonSendMut<Editor>) {
    if events.read().next().is_some() {
        ed.stop_play(); // put the scene back before it is written
        ed.save_all();
        ed.stop();
    }
}

/// Runs last each frame: what was edited this frame becomes an undo step, unless the mouse is
/// still held (a drag is one step, however many frames it lasts).
pub fn commit_edits(mouse: Res<ButtonInput<MouseButton>>, mut ed: NonSendMut<Editor>) {
    ed.commit_edits(mouse.pressed(MouseButton::Left));
}

pub use debug::debug_feed;

/// Files dragged onto the editor window from the operating system are imported into the folder the
/// Assets tab is showing.
pub fn os_file_drops(mut events: MessageReader<bevy::window::FileDragAndDrop>, mut ed: NonSendMut<Editor>) {
    let dropped: Vec<std::path::PathBuf> = events
        .read()
        .filter_map(|e| match e {
            bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } => Some(path_buf.clone()),
            _ => None,
        })
        .collect();
    if !dropped.is_empty() && ed.project.is_some() {
        ed.show_panel(crate::dock::Panel::Assets);
        ed.asset_import(dropped);
    }
}

pub fn ui_system(mut contexts: EguiContexts, mut ed: NonSendMut<Editor>, mut vp: ResMut<ViewportRect>, mut metrics: ResMut<Metrics>, mut exit: ExitWriter) -> Result {
    let started = metrics.timings.start();
    // NOTE: bevy_egui's context accessor changed between releases; this is the
    // only place it is used (see README, "Upgrading").
    let ctx = contexts.ctx_mut()?;
    let ed = &mut *ed;
    crate::theme::apply(ctx, &ed.settings.theme);
    ed.poll();
    // Files changed outside the editor, and script files that follow a renamed script.
    ed.sync_assets(false);
    ed.flush_script_renames(ctx.egui_wants_keyboard_input());
    // Sounds: the preview, and the scene's own while a game plays.
    let dt = ctx.input(|i| i.unstable_dt);
    ed.update_audio(dt);

    let pressed = ctx.input(|i| i.pointer.primary_pressed());
    // While a game plays, a click in the viewport hands it the keyboard and mouse;
    // a click anywhere else (a panel, a menu) gives them back to the editor.
    if pressed {
        let in_viewport = vp.hovered;
        if let Some(play) = ed.play.as_mut().filter(|p| p.game_camera) {
            play.focused = in_viewport;
        }
    }

    // Since egui 0.36 panels are shown inside a `Ui`: this one is the editor's whole window, and the menu bar, the
    // toolbar and the dock take their share of it in this order.
    let mut root = forge_engine::compat::root_ui(ctx);
    menus::menu_bar(&mut root, ed, &mut exit);
    menus::toolbar(&mut root, ed);

    // Every panel of the editor: the viewport, the Explorer, the Properties panel, the Output, the Assets and
    // the Script Editor, wherever the user has put them. The two settings windows are separate from the dock.
    let debug_started = metrics.timings.start();
    panels::show(&mut root, ed, &mut vp);
    settings::editor_settings_window(ctx, ed);
    project_settings::project_settings_window(ctx, ed);

    dialogs::dialogs(ctx, ed);
    dialogs::welcome(ctx, ed);
    project_settings::save_when_idle(ctx, ed);
    popups::popups(ctx, ed, &vp);

    viewport_asset_drop(ctx, ed);
    assets::preview_window(ctx, ed);
    material_editor::material_editor_window(ctx, ed);
    asset_picker::asset_picker(ctx, ed);
    // The overlays are drawn over the viewport, after the windows: while the mouse is over one, the viewport is not hovered.
    if vp.valid {
        let rect = egui::Rect::from_min_max(egui::pos2(vp.min_pt.x, vp.min_pt.y), egui::pos2(vp.max_pt.x, vp.max_pt.y));
        debug::overlays(ctx, ed, rect, &metrics);
    }
    let debug_ms = debug_started.map_or(0.0, |t| t.elapsed().as_secs_f32() * 1000.0);
    vp.pointer = ctx.input(|i| i.pointer.hover_pos()).map(|p| Vec2::new(p.x, p.y));
    vp.typing = ctx.egui_wants_keyboard_input();

    // Shortcuts. Which ones can fire depends on the window the mouse is over (the panels say which, as they are drawn) -
    // except while the camera is being flown or turned: that goes on whatever the mouse passes over.
    let rmb = ctx.input(|i| i.pointer.secondary_down());
    ed.nav_latched = navigation_latch(ctx, ed, vp.hovered, vp.typing, rmb);
    ed.hover = if vp.hovered || ed.nav_latched { Context::Viewport } else { ed.panel_hover.unwrap_or(Context::Global) };    // While a shortcut waits for new keys, none may fire (the keys are for the new binding).
    ed.settings.keymap.debug_while_typing = ed.settings.debug.shortcuts_while_typing;
    ed.keys = if ed.settings_ui.capturing.is_some() { Frame::default() } else { ctx.input(|i| ed.settings.keymap.poll(i, ed.hover, vp.typing)) };
    let frame = ed.keys.clone();
    ed.run_actions(&frame);

    if let Some(problem) = ed.settings.save_if_changed() {
        ed.error(problem);
    }

    assets::drag_hint(ctx);
    // Choosing a parent (Properties > Parent) or a primary part: Esc gives up; the pointer says what a click does.
    if ed.picking_any() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ed.cancel_pick();
    }
    inspector::parent_pick_hint(ctx, ed);

    // What this system took, apart from the debug overlays (which are timed on their own).
    if let Some(started) = started {
        let total = started.elapsed().as_secs_f32() * 1000.0;
        metrics.timings.add_ms(timing::EDITOR_UI, (total - debug_ms).max(0.0));
        metrics.timings.add_ms(timing::DEBUG_UI, debug_ms);
    }
    Ok(())
}

/// Whether the camera is being navigated: flown with the movement keys or turned with the right mouse button.
///
/// It starts only when the pointer is over the viewport's picture (a key held down over a menu or a panel is not
/// for the camera), and then lasts until every movement key and the button are let go, wherever the pointer goes
/// in the meantime - over a menu, a panel or another window - so a fly or a look is never cut short by what is
/// under the mouse. (Typing in a text field never starts it with the keys; the mouse button does not care.)
pub(crate) fn navigation_latch(ctx: &egui::Context, ed: &Editor, over_viewport: bool, typing: bool, rmb: bool) -> bool {
    let keys_down = ctx.input(|i| ed.settings.keymap.poll(i, Context::Viewport, typing).held.iter().any(|a| a.is_movement()));
    (rmb || keys_down) && (over_viewport || ed.nav_latched)
}

/// Assets being dragged over the viewport: shows what dropping them would do, and on release hands the drop
/// to the viewport (which knows the camera) to finish.
fn viewport_asset_drop(ctx: &egui::Context, ed: &mut Editor) {
    let Some(drag) = egui::DragAndDrop::payload::<crate::state::AssetDrag>(ctx) else { return };
    if ed.project.is_none() || !ed.viewport_over {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    match ed.check_drop(&drag.paths, None) {
        Ok(what) => assets::set_drag_hint(ctx, true, format!("{what} in the scene")),
        Err(why) => assets::set_drag_hint(ctx, false, why),
    }
    if ctx.input(|i| i.pointer.any_released()) {
        ed.pending_drop = Some(crate::state::PendingDrop { paths: drag.paths.clone(), pointer: Vec2::new(pos.x, pos.y) });
    }
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;
    use super::*;

    fn key(k: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key { key: k, physical_key: Some(k), pressed, repeat: false, modifiers: egui::Modifiers::default() }
    }

    /// One frame: what the latch says, given where the pointer is.
    fn latch(ctx: &egui::Context, ed: &mut Editor, events: Vec<egui::Event>, over_viewport: bool, typing: bool, rmb: bool) -> bool {
        let mut out = false;
        let _ = ctx.run_frame(egui::RawInput { events, ..Default::default() }, |ctx| {
            ed.nav_latched = navigation_latch(ctx, ed, over_viewport, typing, rmb);
            out = ed.nav_latched;
        });
        out
    }

    #[test]
    fn flying_goes_on_over_menus_and_panels_once_started_over_the_viewport() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        assert!(!latch(&ctx, &mut ed, vec![], true, false, false), "nothing pressed: not navigating");
        assert!(latch(&ctx, &mut ed, vec![key(egui::Key::W, true)], true, false, false), "W over the viewport starts it");
        assert!(latch(&ctx, &mut ed, vec![], false, false, false), "the pointer wandered over a menu: still flying");
        assert!(latch(&ctx, &mut ed, vec![key(egui::Key::D, true)], false, false, false), "a second key joins in over the panel");
        assert!(latch(&ctx, &mut ed, vec![key(egui::Key::W, false)], false, false, false), "one key let go, the other is still down");
        assert!(!latch(&ctx, &mut ed, vec![key(egui::Key::D, false)], false, false, false), "all let go: it ends");
        assert!(!latch(&ctx, &mut ed, vec![], true, false, false));
    }

    #[test]
    fn keys_held_down_over_a_panel_are_not_for_the_camera_and_typing_never_starts_it() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        assert!(!latch(&ctx, &mut ed, vec![key(egui::Key::W, true)], false, false, false), "pressed over a menu or a panel: not started");
        assert!(!latch(&ctx, &mut ed, vec![], true, false, false) || ed.nav_latched, "(the key is still down, and now the pointer is over the viewport)");
        let mut ed = Editor::new(None);
        let ctx = egui::Context::default();
        assert!(!latch(&ctx, &mut ed, vec![key(egui::Key::W, true)], true, true, false), "a text field has the keyboard: W is a letter");
    }

    #[test]
    fn the_right_button_latches_the_same_way_and_ends_when_released() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        assert!(!latch(&ctx, &mut ed, vec![], false, false, true), "pressed over a panel: not a look");
        let mut ed = Editor::new(None);
        assert!(latch(&ctx, &mut ed, vec![], true, false, true));
        assert!(latch(&ctx, &mut ed, vec![], false, false, true), "dragged over a menu: still looking");
        assert!(!latch(&ctx, &mut ed, vec![], false, false, false), "released");
    }
}
