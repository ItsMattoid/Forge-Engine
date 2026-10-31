//! Draws the dock: every window of the editor as a tab of a [`egui_dock::DockArea`].
//!
//! `dock` holds the layout (where each panel is, which are shown); this file draws each panel's contents,
//! one function per [`Panel`], and tells the rest of the editor what the dock found out each frame:
//! where the 3D viewport is, whether the mouse is over it, and which panel the mouse is over (shortcuts
//! depend on that).
//!
//! The dock is moved out of the editor while it is drawn (the panels need `&mut Editor`), so changes asked
//! for from inside a panel are queued and applied right after (see `state::layout`).

use bevy::prelude::Vec2;
use bevy_egui::egui::{self, Margin, Stroke};
use egui_dock::{DockArea, TabStyle};

use super::{assets, bottom, debug, explorer, inspector};
use crate::{
    dock::{style_for, Panel},
    keymap::Context,
    state::Editor,
    theme::SecondaryText,
    viewport::ViewportRect,
};

struct Viewer<'a> {
    ed: &'a mut Editor,
    vp: &'a mut ViewportRect,
    /// The viewport's tab was drawn this frame (it is not when the panel is hidden or behind another tab).
    viewport_drawn: bool,
    /// The panel with keyboard shortcuts of its own that the mouse is over.
    hover: Option<Context>,
}

impl Viewer<'_> {
    /// Notes that the mouse is over this panel's contents.
    fn track(&mut self, ui: &egui::Ui, context: Context) {
        let rect = ui.max_rect();
        if ui.rect_contains_pointer(rect) {
            self.hover = Some(context);
        }
        match context {
            Context::Explorer => self.ed.explorer_rect = Some(rect),
            Context::Properties => self.ed.properties_rect = Some(rect),
            _ => {}
        }
    }

    /// The 3D view. Its two tabs, the Editor (`Panel::Viewport`) and the Game (`Panel::Game`, only while a game
    /// plays), are two holes onto the same picture: the one that is in front (the dock draws only the open tab of a
    /// group) gives it its rectangle, and which of them that is decides what the picture shows (see
    /// `Editor::sync_game_view_from_dock`).
    fn viewport(&mut self, ui: &mut egui::Ui) {
        // The Debug dropdown sits in a strip at the top; what is below it is the picture.
        debug::viewport_bar(ui, self.ed);
        let rect = ui.available_rect_before_wrap();
        // (Hover only: the 3D view is not an egui widget, and egui must not take the clicks and drags in it.)
        ui.allocate_rect(rect, egui::Sense::hover());
        let ppp = ui.ctx().pixels_per_point();
        self.vp.min_pt = Vec2::new(rect.min.x, rect.min.y);
        self.vp.max_pt = Vec2::new(rect.max.x, rect.max.y);
        self.vp.min_px = self.vp.min_pt * ppp;
        self.vp.max_px = self.vp.max_pt * ppp;
        self.vp.valid = rect.width() > 8.0 && rect.height() > 8.0;
        // Over the picture itself - not a window or a menu on top of it, and not while a tab or a divider is
        // being dragged.
        let over = ui.rect_contains_pointer(rect);
        self.ed.viewport_over = over;
        self.vp.hovered = over && ui.ctx().dragged_id().is_none();
        self.viewport_drawn = true;
    }
}

impl egui_dock::TabViewer for Viewer<'_> {
    type Tab = Panel;

    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        match tab {
            Panel::Properties => self.ed.properties_title().into(),
            // While a game plays the viewport's tab says which of its two views it is.
            Panel::Viewport if self.ed.playing() => "Editor".into(),
            _ => tab.title().into(),
        }
    }

    fn id(&mut self, tab: &mut Panel) -> egui::Id {
        egui::Id::new(("forge_panel", tab.key()))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        match tab {
            Panel::Viewport | Panel::Game => self.viewport(ui),
            Panel::Explorer => {
                if self.ed.project.is_none() {
                    ui.secondary("Open or create a project to see its objects.");
                } else {
                    explorer::explorer(ui, self.ed);
                }
                self.track(ui, Context::Explorer);
            }
            Panel::Properties => {
                if self.ed.project.is_none() {
                    ui.secondary("Open or create a project to edit its objects.");
                } else {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| inspector::properties(ui, self.ed));
                }
                self.track(ui, Context::Properties);
            }
            Panel::Output => bottom::output(ui, self.ed),
            Panel::Assets => assets::assets_tab(ui, self.ed),
            Panel::ScriptEditor => bottom::script_tab(ui, self.ed),
        }
    }

    /// Closing a tab hides the panel (the View menu shows it again).
    fn on_close(&mut self, _tab: &mut Panel) -> egui_dock::widgets::tab_viewer::OnCloseResponse {
        egui_dock::widgets::tab_viewer::OnCloseResponse::Close
    }

    /// The Game tab goes when the game stops, not before: it has no close button.
    fn is_closeable(&self, tab: &Panel) -> bool {
        *tab != Panel::Game
    }

    /// It is a tab of the viewport: it cannot be pulled out into a window of its own.
    fn allowed_in_windows(&self, tab: &mut Panel) -> bool {
        *tab != Panel::Game
    }

    /// Every panel but the viewport (and its Game tab) has a background; they are the holes through which the 3D
    /// view shows.
    fn clear_background(&self, tab: &Panel) -> bool {
        !matches!(tab, Panel::Viewport | Panel::Game)
    }

    /// The panels scroll their own contents.
    fn scroll_bars(&self, _tab: &Panel) -> [bool; 2] {
        [false, false]
    }

    fn tab_style_override(&self, tab: &Panel, global: &TabStyle) -> Option<TabStyle> {
        matches!(tab, Panel::Viewport | Panel::Game).then(|| {
            let mut style = global.clone();
            style.tab_body.inner_margin = Margin::ZERO;
            style.tab_body.stroke = Stroke::NONE;
            style.tab_body.bg_fill = egui::Color32::TRANSPARENT;
            style
        })
    }
}

/// Where a floating window is, as the dock wants to be told when it is put back. egui keeps that (the dock does
/// not), under the name egui_dock gives the window after its surface. Since egui 0.36 the size the dock sets is the
/// size of the whole window, the same as egui reports, so the area's rectangle is what is kept.
pub(super) fn window_place(ctx: &egui::Context, surface: egui_dock::SurfaceIndex) -> Option<egui::Rect> {
    ctx.memory(|m| m.area_rect(egui::Id::new(format!("window {surface:?}"))))
}

/// Draws the dock in the space the menu bar and the toolbar left. Fills in the viewport's rectangle and
/// whether the mouse is over it, and which panel the mouse is over.
pub(super) fn show(root: &mut egui::Ui, ed: &mut Editor, vp: &mut ViewportRect) {
    let ctx = &root.ctx().clone();
    ed.apply_panel_ops();
    ed.panel_hover = None;
    ed.explorer_rect = None;
    ed.properties_rect = None;
    let mut dock = std::mem::take(&mut ed.dock);
    ed.dock_busy = true;
    let style = style_for(ctx);
    let mut viewer = Viewer { ed: &mut *ed, vp: &mut *vp, viewport_drawn: false, hover: None };
    egui::CentralPanel::default().frame(egui::Frame::new()).show(root, |ui| {
        DockArea::new(&mut dock.state).style(style).show_add_buttons(false).show_close_buttons(true).draggable_tabs(true).show_inside(ui, &mut viewer);
    });
    let (viewport_drawn, hover) = (viewer.viewport_drawn, viewer.hover);
    ed.dock = dock;
    ed.dock_busy = false;
    ed.panel_hover = hover;
    // Which tab of the viewport is in front decides what it shows (and who has the keyboard and mouse).
    ed.sync_game_view_from_dock();
    if !viewport_drawn {
        // No picture this frame: nothing is picked or flown, and the camera does not draw behind the panels.
        vp.valid = false;
        vp.hovered = false;
        ed.viewport_over = false;
    }
    ed.apply_panel_ops();
    // Remembered for the next start, once nothing is being dragged.
    let idle = !ctx.input(|i| i.pointer.any_down());
    ed.save_layout_if_changed(idle, &|surface| window_place(ctx, surface));
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;

    use super::*;
    use crate::dock::{Dock, PlaceExt as _};

    fn frame(ctx: &egui::Context, ed: &mut Editor, vp: &mut ViewportRect, events: Vec<egui::Event>) {
        let raw = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| show(&mut forge_engine::compat::root_ui(ctx), ed, vp));
    }

    fn frames(ctx: &egui::Context, ed: &mut Editor, vp: &mut ViewportRect, n: usize) {
        for _ in 0..n {
            frame(ctx, ed, vp, vec![]);
        }
    }

    #[test]
    fn the_default_layout_draws_and_gives_the_viewport_the_middle() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        frames(&ctx, &mut ed, &mut vp, 4);
        assert!(vp.valid, "the viewport has a picture");
        let (w, h) = (vp.max_pt.x - vp.min_pt.x, vp.max_pt.y - vp.min_pt.y);
        assert!(w > 800.0 && w < 1200.0 && h > 500.0 && h < 780.0, "a rectangle between the side panels and above the bottom ones: {w} x {h}");
        assert!(vp.min_pt.x > 100.0, "the Explorer is to its left");
        assert!(vp.max_pt.x < 1550.0, "and Properties to its right");
        assert_eq!(vp.min_px, vp.min_pt * ctx.pixels_per_point(), "in pixels as well as points");
        assert!(!ed.dock_busy && ed.dock.visible(Panel::Viewport), "the layout is back in the editor");
    }

    #[test]
    fn the_properties_tab_is_titled_after_the_selection_and_the_other_tabs_keep_their_names() {
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        let mut title = |ed: &mut Editor, panel: Panel| {
            let mut viewer = Viewer { ed, vp: &mut vp, viewport_drawn: false, hover: None };
            egui_dock::TabViewer::title(&mut viewer, &mut { panel }).text().to_owned()
        };
        assert_eq!(title(&mut ed, Panel::Properties), "Properties");
        let workspace = ed.dm.borrow().workspace;
        ed.select_only(Some(workspace));
        assert_eq!(title(&mut ed, Panel::Properties), "Properties - Workspace \"Workspace\"");
        assert_eq!(title(&mut ed, Panel::Explorer), "Explorer");
        assert_eq!(title(&mut ed, Panel::Viewport), "Viewport");
        // The View menu and the shortcuts keep the plain name.
        assert_eq!(Panel::Properties.title(), "Properties");
    }

    #[test]
    fn the_game_is_a_tab_of_the_viewports_own_group_while_playing_and_goes_when_it_stops() {
        let ctx = egui::Context::default();
        let dir = std::env::temp_dir().join(format!("forge_game_tab_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        forge_engine::project::Project::create(&dir, "Game Tab").unwrap();
        let mut ed = Editor::new(Some(dir.clone()));
        let mut vp = ViewportRect::default();
        frames(&ctx, &mut ed, &mut vp, 4);
        let (min, max) = (vp.min_pt, vp.max_pt);
        let layout = ed.dock.to_json();
        let group = |ed: &Editor, p: Panel| {
            let (s, n, _) = ed.dock.place(p).unwrap();
            ed.dock.state[s][n].tabs().map(|t| t.to_vec()).unwrap_or_default()
        };
        assert_eq!(group(&ed, Panel::Viewport), [Panel::Viewport], "one tab: the viewport");
        assert!(!ed.dock.visible(Panel::Game) && !ed.game_view_active());

        // Play: the viewport's group gets a second tab, in front. Nothing moved, nothing new opened.
        ed.start_play();
        assert!(ed.playing() && ed.game_view_active());
        frames(&ctx, &mut ed, &mut vp, 3);
        assert_eq!(group(&ed, Panel::Viewport), [Panel::Viewport, Panel::Game], "the Editor and the Game tab share the viewport's own group");
        assert_eq!(ed.dock.place(Panel::Game).unwrap().0, ed.dock.place(Panel::Viewport).unwrap().0);
        assert!(ed.dock.is_front(Panel::Game) && ed.dock.game_in_front());
        assert_eq!(ed.dock.state.surfaces_count(), 1, "no window of its own");
        assert!(vp.valid && (vp.min_pt, vp.max_pt) == (min, max), "the picture is where the viewport was");
        let mut title = |ed: &mut Editor, panel: Panel| {
            let mut v = Viewer { ed, vp: &mut vp, viewport_drawn: false, hover: None };
            egui_dock::TabViewer::title(&mut v, &mut { panel }).text().to_owned()
        };
        assert_eq!((title(&mut ed, Panel::Viewport).as_str(), title(&mut ed, Panel::Game).as_str()), ("Editor", "Game"), "the two tabs are called Editor and Game");
        let saved = ed.dock.to_json();
        assert!(!saved.to_string().contains("\"game\""), "the Game tab is not part of the saved layout");
        let read = Dock::from_json(&saved).expect("the layout read back");
        let (s, n, _) = read.place(Panel::Viewport).unwrap();
        assert_eq!(read.state[s][n].tabs().map(|t| t.to_vec()).unwrap_or_default(), [Panel::Viewport], "and what is saved is the viewport alone");
        assert!(read.is_front(Panel::Viewport), "with a valid open tab");
        let _ = &layout;
        assert!(ed.game_has_input(), "the game has the keyboard and mouse in its tab");

        // Clicking the Editor tab: the game goes on, the editor has the keyboard. (The click sets which tab is open.)
        let before = ed.dm.borrow().instance_count();
        ed.dock.focus(Panel::Viewport);
        frames(&ctx, &mut ed, &mut vp, 3);
        assert!(ed.playing() && !ed.game_view_active() && !ed.game_has_input(), "still playing, now looking through the editor camera");
        assert!(vp.valid && (vp.min_pt, vp.max_pt) == (min, max));
        assert_eq!(ed.dm.borrow().instance_count(), before, "the scene was not rebuilt");
        ed.dock.focus(Panel::Game);
        frames(&ctx, &mut ed, &mut vp, 3);
        assert!(ed.game_view_active() && ed.game_has_input(), "and back to the game");
        // The same through the editor's own switch (what a script or a shortcut uses).
        ed.set_game_view(false);
        frames(&ctx, &mut ed, &mut vp, 2);
        assert!(!ed.game_view_active() && ed.dock.is_front(Panel::Viewport));

        // Restoring the default layout while playing keeps the tab.
        ed.restore_layout();
        frames(&ctx, &mut ed, &mut vp, 2);
        assert!(ed.dock.visible(Panel::Game) && ed.playing() && !ed.game_view_active(), "the open view stays what it was");

        // Stop: the viewport is one tab again, the editor view is back and the layout is as it was.
        ed.stop_play();
        frames(&ctx, &mut ed, &mut vp, 3);
        assert!(!ed.playing() && !ed.dock.visible(Panel::Game) && !ed.game_view_active());
        assert_eq!(group(&ed, Panel::Viewport), [Panel::Viewport]);
        assert!(ed.dock.is_front(Panel::Viewport) && vp.valid && (vp.min_pt, vp.max_pt) == (min, max));
        assert_eq!(ed.dock.visible_panels(), Dock::default_layout().visible_panels(), "every panel as before, and none more");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_game_tab_cannot_be_closed_or_pulled_out_and_a_saved_layout_never_holds_it() {
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        let viewer = Viewer { ed: &mut ed, vp: &mut vp, viewport_drawn: false, hover: None };
        assert!(!egui_dock::TabViewer::is_closeable(&viewer, &Panel::Game), "it goes when the game stops");
        assert!(!egui_dock::TabViewer::allowed_in_windows(&viewer, &mut Panel::Game), "it is part of the viewport, not a window");
        assert!(egui_dock::TabViewer::is_closeable(&viewer, &Panel::Viewport) && egui_dock::TabViewer::allowed_in_windows(&viewer, &mut Panel::Viewport));
        // A layout file that does name it (from a bug or by hand) does not bring it back.
        let mut dock = Dock::default_layout();
        dock.show(Panel::Game);
        let json = dock.to_json();
        assert!(!json.to_string().contains("\"game\""));
        let text = Dock::default_layout().to_json().to_string().replacen("\"tabs\":[\"viewport\"]", "\"tabs\":[\"viewport\",\"game\"]", 1);
        assert!(text.contains("\"game\""), "the test layout does name the game tab");
        let read = Dock::from_json(&serde_json::from_str(&text).unwrap()).unwrap();
        assert!(!read.visible(Panel::Game) && read.visible(Panel::Viewport));
    }
    #[test]
    fn hiding_the_viewport_takes_its_picture_away_and_showing_it_brings_it_back() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        frames(&ctx, &mut ed, &mut vp, 3);
        ed.hide_panel(Panel::Viewport);
        frames(&ctx, &mut ed, &mut vp, 3);
        assert!(!vp.valid && !vp.hovered && !ed.viewport_over);
        ed.show_panel(Panel::Viewport);
        frames(&ctx, &mut ed, &mut vp, 3);
        assert!(vp.valid);
        // A viewport that is a tab behind another one is not drawn, so it has no picture either.
        ed.show_panel(Panel::Explorer);
        let (surface, node, _) = ed.dock.place(Panel::Viewport).unwrap();
        ed.dock.state.move_tab(ed.dock.place(Panel::Explorer).unwrap().tab_path(), egui_dock::TabDestination::Node(egui_dock::NodePath::new(surface, node), egui_dock::TabInsert::Append));
        ed.dock.focus(Panel::Explorer);
        frames(&ctx, &mut ed, &mut vp, 3);
        assert!(!vp.valid, "behind the Explorer tab");
        ed.dock.focus(Panel::Viewport);
        frames(&ctx, &mut ed, &mut vp, 3);
        assert!(vp.valid);
    }

    #[test]
    fn the_mouse_over_the_viewport_hovers_it_and_over_a_panel_decides_the_shortcut_window() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        frames(&ctx, &mut ed, &mut vp, 4);
        let middle = egui::pos2((vp.min_pt.x + vp.max_pt.x) / 2.0, (vp.min_pt.y + vp.max_pt.y) / 2.0);
        frame(&ctx, &mut ed, &mut vp, vec![egui::Event::PointerMoved(middle)]);
        frame(&ctx, &mut ed, &mut vp, vec![]);
        assert!(vp.hovered && ed.viewport_over, "the pointer is over the picture");
        assert_eq!(ed.panel_hover, None);

        let explorer = ed.explorer_rect.expect("the Explorer was drawn");
        let inside = explorer.center();
        frame(&ctx, &mut ed, &mut vp, vec![egui::Event::PointerMoved(inside)]);
        frame(&ctx, &mut ed, &mut vp, vec![]);
        assert!(!vp.hovered, "not the viewport any more");
        assert_eq!(ed.panel_hover, Some(Context::Explorer));
        let properties = ed.properties_rect.expect("Properties was drawn").center();
        frame(&ctx, &mut ed, &mut vp, vec![egui::Event::PointerMoved(properties)]);
        frame(&ctx, &mut ed, &mut vp, vec![]);
        assert_eq!(ed.panel_hover, Some(Context::Properties));
    }

    #[test]
    fn requests_from_inside_a_panel_take_effect_in_the_same_frame() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        ed.hide_panel(Panel::Assets);
        frames(&ctx, &mut ed, &mut vp, 2);
        // (What opening a script from the Assets panel does while the dock is out of the editor.)
        ed.dock_busy = true;
        ed.show_panel(Panel::Assets);
        ed.dock_busy = false;
        frame(&ctx, &mut ed, &mut vp, vec![]);
        assert!(ed.panel_visible(Panel::Assets) && ed.panel_ops.is_empty());
    }

    #[test]
    fn a_panel_dragged_out_is_a_floating_window_that_keeps_its_state_and_can_be_docked_again() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        frames(&ctx, &mut ed, &mut vp, 3);
        let window = ed.dock.state.detach_tab(ed.dock.place(Panel::Output).unwrap().tab_path(), egui::Rect::from_min_size(egui::pos2(200.0, 150.0), egui::vec2(700.0, 500.0)));
        assert!(!window.is_main());
        frames(&ctx, &mut ed, &mut vp, 4);
        assert!(ctx.memory(|m| m.area_rect(egui::Id::new("window SurfaceIndex(1)"))).is_some(), "the floating window exists");
        let viewport = ed.dock.place(Panel::Viewport).unwrap();
        ed.dock.state.move_tab(ed.dock.place(Panel::Output).unwrap().tab_path(), egui_dock::TabDestination::Node(egui_dock::NodePath::new(viewport.0, viewport.1), egui_dock::TabInsert::Append));
        frames(&ctx, &mut ed, &mut vp, 3);
        assert!(ed.dock.place(Panel::Output).unwrap().0.is_main(), "docked again");
    }

    #[test]
    fn a_saved_layout_with_a_floating_window_comes_back_where_it_was() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        frames(&ctx, &mut ed, &mut vp, 2);
        ed.dock.state.detach_tab(ed.dock.place(Panel::Output).unwrap().tab_path(), egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(480.0, 310.0)));
        frames(&ctx, &mut ed, &mut vp, 4);
        let surface = ed.dock.place(Panel::Output).unwrap().0;
        ed.dock.state.get_window_state_mut(surface).unwrap().set_position(egui::pos2(333.0, 222.0)).set_size(egui::vec2(480.0, 310.0));
        frames(&ctx, &mut ed, &mut vp, 4);
        let before = ctx.memory(|m| m.area_rect(egui::Id::new("window SurfaceIndex(1)"))).unwrap();
        assert!((before.min.x - 333.0).abs() < 2.0 && (before.min.y - 222.0).abs() < 2.0, "{before:?}");

        // Written, and read by a new editor with a new egui context (the next start).
        let json = ed.dock.to_json_with_windows(&|surface| window_place(&ctx, surface));
        let text = json.to_string();
        let mut again = Editor::new(None);
        again.dock = crate::dock::Dock::from_json(&serde_json::from_str(&text).unwrap()).unwrap();
        let ctx2 = egui::Context::default();
        let mut vp2 = ViewportRect::default();
        frames(&ctx2, &mut again, &mut vp2, 4);
        let after = ctx2.memory(|m| m.area_rect(egui::Id::new("window SurfaceIndex(1)"))).expect("the window is back");
        assert!((after.min - before.min).length() < 3.0 && (after.size() - before.size()).length() < 3.0, "{after:?} vs {before:?}");
    }

    // ---- the settings windows are not part of the dock ----

    fn frame_with_windows(ctx: &egui::Context, ed: &mut Editor, vp: &mut ViewportRect, events: Vec<egui::Event>) {
        let raw = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| {
            show(&mut forge_engine::compat::root_ui(ctx), ed, vp);
            crate::ui::settings::editor_settings_window(ctx, ed);
            crate::ui::project_settings::project_settings_window(ctx, ed);
        });
    }

    fn window_rect(ctx: &egui::Context, key: &str) -> Option<egui::Rect> {
        ctx.memory(|m| m.area_rect(egui::Id::new(("forge_window", key))))
    }

    #[test]
    fn the_settings_windows_are_windows_of_their_own_and_the_dock_does_not_know_them() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        ed.open_editor_settings();
        for _ in 0..4 {
            frame_with_windows(&ctx, &mut ed, &mut vp, vec![]);
        }
        let rect = window_rect(&ctx, "editor_settings").expect("the window is drawn");
        assert!(rect.width() > 400.0 && rect.height() > 250.0, "{rect:?}");
        assert_eq!(ed.dock.state.surfaces_count(), 1, "no dock window was made for it");
        assert_eq!(ed.dock.state.iter_all_tabs().count(), 6, "and no tab");
        // Another panel still docks and floats as before.
        let window = ed.dock.state.detach_tab(ed.dock.place(Panel::Output).unwrap().tab_path(), egui::Rect::from_min_size(egui::pos2(200.0, 150.0), egui::vec2(600.0, 400.0)));
        assert!(!window.is_main());
        let viewport = ed.dock.place(Panel::Viewport).unwrap();
        ed.dock.state.move_tab(ed.dock.place(Panel::Output).unwrap().tab_path(), egui_dock::TabDestination::Node(egui_dock::NodePath::new(viewport.0, viewport.1), egui_dock::TabInsert::Append));
        for _ in 0..3 {
            frame_with_windows(&ctx, &mut ed, &mut vp, vec![]);
        }
        assert!(ed.dock.place(Panel::Output).unwrap().0.is_main() && ed.editor_settings_open(), "both still fine");
    }

    #[test]
    fn a_settings_window_comes_back_where_it_was_left_in_this_session_and_in_the_next() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        let mut vp = ViewportRect::default();
        ed.open_editor_settings();
        for _ in 0..3 {
            frame_with_windows(&ctx, &mut ed, &mut vp, vec![]);
        }
        // Dragged by its title bar to somewhere else.
        let start = window_rect(&ctx, "editor_settings").unwrap();
        let grab = egui::pos2(start.min.x + 60.0, start.min.y + 10.0);
        let button = |pressed| egui::Event::PointerButton { pos: grab, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        frame_with_windows(&ctx, &mut ed, &mut vp, vec![egui::Event::PointerMoved(grab)]);
        frame_with_windows(&ctx, &mut ed, &mut vp, vec![button(true)]);
        let drop = grab + egui::vec2(150.0, 90.0);
        for step in 1..=4 {
            frame_with_windows(&ctx, &mut ed, &mut vp, vec![egui::Event::PointerMoved(grab + (drop - grab) * (step as f32 / 4.0))]);
        }
        frame_with_windows(&ctx, &mut ed, &mut vp, vec![egui::Event::PointerButton { pos: drop, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() }]);
        for _ in 0..3 {
            frame_with_windows(&ctx, &mut ed, &mut vp, vec![]);
        }
        let moved = window_rect(&ctx, "editor_settings").unwrap();
        assert!((moved.min - start.min - egui::vec2(150.0, 90.0)).length() < 3.0, "{start:?} -> {moved:?}");
        assert!(ed.dock.state.surfaces_count() == 1, "dragging it did not dock it anywhere");        let place = *ed.settings.window_places.get("editor_settings").expect("remembered");
        assert!((place.x - moved.min.x).abs() < 2.0 && (place.y - moved.min.y).abs() < 2.0, "{place:?} vs {moved:?}");

        // Closed and opened again: the same place.
        ed.close_editor_settings();
        frame_with_windows(&ctx, &mut ed, &mut vp, vec![]);
        ed.open_editor_settings();
        for _ in 0..3 {
            frame_with_windows(&ctx, &mut ed, &mut vp, vec![]);
        }
        let again = window_rect(&ctx, "editor_settings").unwrap();
        assert!((again.min.x - place.x).abs() < 2.0 && (again.min.y - place.y).abs() < 2.0, "{again:?} vs {place:?}");

        // A new editor with a new egui context (the next start) reads the same place from the settings.
        let mut next = Editor::new(None);
        next.settings.window_places = ed.settings.window_places.clone();
        let ctx2 = egui::Context::default();
        let mut vp2 = ViewportRect::default();
        next.open_editor_settings();
        for _ in 0..3 {
            frame_with_windows(&ctx2, &mut next, &mut vp2, vec![]);
        }
        let reopened = window_rect(&ctx2, "editor_settings").unwrap();
        assert!((reopened.min.x - place.x).abs() < 2.0 && (reopened.min.y - place.y).abs() < 2.0, "{reopened:?} vs {place:?}");
        assert!((reopened.width() - again.width()).abs() < 3.0 && (reopened.height() - again.height()).abs() < 3.0, "{reopened:?} vs {again:?}");
    }

    #[test]
    fn the_two_settings_windows_remember_their_own_places() {
        let ctx = egui::Context::default();
        let dir = std::env::temp_dir().join(format!("forge_two_windows_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        forge_engine::project::Project::create(&dir, "Two Windows").unwrap();
        let mut ed = Editor::new(Some(dir.clone()));
        let mut vp = ViewportRect::default();
        ed.open_editor_settings();
        ed.open_project_settings();
        assert!(ed.project_settings_open());
        for _ in 0..4 {
            frame_with_windows(&ctx, &mut ed, &mut vp, vec![]);
        }
        let (a, b) = (window_rect(&ctx, "editor_settings").unwrap(), window_rect(&ctx, "project_settings").unwrap());
        assert_ne!(a.size(), b.size(), "each has its own size");
        assert!(ed.settings.window_places.contains_key("editor_settings") && ed.settings.window_places.contains_key("project_settings"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
