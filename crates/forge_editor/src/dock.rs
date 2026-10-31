//! The editor's dockable windows: which windows ("panels") there are, the default layout, showing,
//! hiding and focusing them, and saving the layout. (The Editor Settings and Project Settings windows are
//! deliberately *not* panels: they are separate windows that cannot be docked or tabbed, see ui::window_memory.)
//!
//! The layout is an [`egui_dock::DockState`] of [`Panel`]s: panels sit in tab groups, tab groups are split
//! side by side or one above the other, and any panel can be pulled out into a floating window and put back.
//! *Whether a panel is shown is whether it is in the layout* - the View menu, the window shortcuts, the tab
//! close buttons and the dock itself all read and change that one thing, so they cannot disagree.
//!
//! What a panel draws, and its state (the scene, the script being edited, the search box...), live in the
//! [`Editor`](crate::state::Editor), not in the layout: moving, tabbing, floating and hiding a panel never
//! resets what is in it or loses unsaved work.
//!
//! Saved layouts (`editor_settings.json`) are read defensively: a panel this version does not know is
//! dropped, one listed twice is kept once, and a layout that makes no sense is replaced by the default.

use bevy_egui::egui::{self, Pos2, Rect, Vec2};
use egui_dock::{DockState, NodeIndex, NodePath, Surface, SurfaceIndex, TabIndex, TabPath};
use serde_json::Value as Json;

/// One window of the editor.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Panel {
    /// The 3D view of the scene.
    Viewport,
    Explorer,
    Properties,
    /// The log of the editor and of the running game.
    Output,
    /// The Asset Browser.
    Assets,
    ScriptEditor,
    /// The running game, as a second tab of the group the viewport is in. It exists only while a game is playing
    /// in the viewport (it is added when Play starts and removed when it stops), is never saved with the layout
    /// and is not in the View menu: it is part of the Viewport, not a window of its own.
    Game,
}

/// What is known about a panel besides how it is drawn.
pub struct PanelDef {
    pub panel: Panel,
    /// Stable name in saved layouts.
    pub key: &'static str,
    pub title: &'static str,
    /// Name of its "show / focus / hide" shortcut in the settings file, and what the Shortcuts tab calls it.
    pub action_id: &'static str,
    pub action_name: &'static str,
    /// The starter keys of that shortcut.
    pub shortcut: &'static [&'static str],
    pub description: &'static str,
}

/// Every panel, in the order the View menu lists them. The position is the `Panel` discriminant.
pub static PANELS: &[PanelDef] = &[
    PanelDef { panel: Panel::Viewport, key: "viewport", title: "Viewport", action_id: "window.viewport", action_name: "Show / hide Viewport", shortcut: &["Alt+1"], description: "The 3D view of the scene." },
    PanelDef { panel: Panel::Explorer, key: "explorer", title: "Explorer", action_id: "window.explorer", action_name: "Show / hide Explorer", shortcut: &["Alt+2"], description: "The tree of objects in the scene." },
    PanelDef { panel: Panel::Properties, key: "properties", title: "Properties", action_id: "window.properties", action_name: "Show / hide Properties", shortcut: &["Alt+3"], description: "The properties of the selected objects." },
    PanelDef { panel: Panel::Output, key: "output", title: "Output", action_id: "window.output", action_name: "Show / hide Output", shortcut: &["Alt+4"], description: "Messages from the editor and the running game." },
    PanelDef { panel: Panel::Assets, key: "assets", title: "Assets", action_id: "window.assets", action_name: "Show / hide Assets", shortcut: &["Alt+5"], description: "The project's files." },
    PanelDef { panel: Panel::ScriptEditor, key: "script_editor", title: "Script Editor", action_id: "window.script_editor", action_name: "Show / hide Script Editor", shortcut: &["Alt+6"], description: "Edit a Luau script." },
    PanelDef { panel: Panel::Game, key: "game", title: "Game", action_id: "window.game", action_name: "", shortcut: &[], description: "The running game: the other tab of the Viewport while Play is on." },
];

impl Panel {
    /// The panels people show and hide (the Game tab is not one of them: see [`Panel::Game`]).
    pub const ALL: [Panel; 6] = [Panel::Viewport, Panel::Explorer, Panel::Properties, Panel::Output, Panel::Assets, Panel::ScriptEditor];

    pub fn def(self) -> &'static PanelDef {
        &PANELS[self as usize]
    }

    pub fn title(self) -> &'static str {
        self.def().title
    }

    pub fn key(self) -> &'static str {
        self.def().key
    }

    pub fn from_key(key: &str) -> Option<Panel> {
        PANELS.iter().find(|d| d.key == key).map(|d| d.panel)
    }

    /// Where it goes when shown again after being hidden: with these (or, failing that, in their zone).
    fn zone(self) -> Zone {
        match self {
            Panel::Viewport | Panel::Game => Zone::Center,
            Panel::Explorer => Zone::Left,
            Panel::Properties => Zone::Right,
            Panel::Output | Panel::Assets | Panel::ScriptEditor => Zone::Bottom,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zone {
    Center,
    Left,
    Right,
    Bottom,
}

/// What a "show / focus / hide" shortcut did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toggled {
    /// It was hidden: now shown, and in front.
    Shown,
    /// It was shown but behind something or not the one in use: now in front.
    Focused,
    /// It was the one in use: now hidden.
    Hidden,
}

/// Where a tab is: its surface (the main area or a floating window), the group in it and the place in the group.
pub type Place = (SurfaceIndex, NodeIndex, TabIndex);

/// egui_dock names places with structs (`TabPath`, `NodePath`); the editor keeps them as plain tuples.
pub trait PlaceExt {
    fn tab_path(self) -> TabPath;
    fn node_path(self) -> NodePath;
}

impl PlaceExt for Place {
    fn tab_path(self) -> TabPath {
        TabPath::new(self.0, self.1, self.2)
    }

    fn node_path(self) -> NodePath {
        NodePath::new(self.0, self.1)
    }
}

/// The layout of all the panels. See the module docs.
pub struct Dock {
    pub state: DockState<Panel>,
}

impl Default for Dock {
    /// An empty layout (cheap to create: used as a placeholder while the real one is being drawn).
    fn default() -> Self {
        Dock { state: DockState::new(Vec::new()) }
    }
}

impl Dock {
    /// The starting layout: the Explorer on the left, Properties on the right, Output / Assets / Script
    /// Editor as tabs along the bottom (the full width), the Viewport in the middle.
    pub fn default_layout() -> Dock {
        let mut state = DockState::new(vec![Panel::Viewport]);
        let tree = state.main_surface_mut();
        // (The number is the share of the first child - the left or upper one - so for the Explorer, which goes
        // on the left, it is the Explorer's own share.)
        let [upper, _bottom] = tree.split_below(NodeIndex::root(), 0.77, vec![Panel::Output, Panel::Assets, Panel::ScriptEditor]);
        let [middle, _explorer] = tree.split_left(upper, 0.18, vec![Panel::Explorer]);
        let [_viewport, _properties] = tree.split_right(middle, 0.76, vec![Panel::Properties]);
        Dock { state }
    }

    pub fn place(&self, p: Panel) -> Option<Place> {
        self.state.find_tab(&p).map(|t| (t.surface, t.node, t.tab))
    }

    /// Whether the panel is shown (it is in the layout, in a group or a floating window).
    pub fn visible(&self, p: Panel) -> bool {
        self.place(p).is_some()
    }

    /// The panels that are shown.
    #[cfg(test)]
    pub fn visible_panels(&self) -> Vec<Panel> {
        Panel::ALL.into_iter().filter(|p| self.visible(*p)).collect()
    }

    /// Whether the panel is the one in use: its tab is the open tab of the group that has the focus.
    pub fn is_focused(&self, p: Panel) -> bool {
        let Some((surface, node, tab)) = self.place(p) else { return false };
        self.state.focused_leaf() == Some(NodePath::new(surface, node)) && matches!(&self.state[surface][node], egui_dock::Node::Leaf(leaf) if leaf.active == tab)
    }

    /// Whether the panel is the open tab of its group (the group need not have the focus).
    pub fn is_front(&self, p: Panel) -> bool {
        let Some((surface, node, tab)) = self.place(p) else { return false };
        matches!(&self.state[surface][node], egui_dock::Node::Leaf(leaf) if leaf.active == tab)
    }

    /// Whether the viewport shows the game rather than the editor: the Game tab is the open one of the group it
    /// is in. (If the Game tab was dragged to a group of its own, whichever of the two groups has the focus decides;
    /// with no Viewport tab at all there is only the game to show.)
    pub fn game_in_front(&self) -> bool {
        let Some(game) = self.place(Panel::Game) else { return false };
        let Some(viewport) = self.place(Panel::Viewport) else { return true };
        if (game.0, game.1) == (viewport.0, viewport.1) {
            return self.is_front(Panel::Game);
        }
        self.is_front(Panel::Game) && (self.state.focused_leaf() == Some(game.node_path()) || !self.is_front(Panel::Viewport))
    }

    /// Brings a panel that is shown to the front of its group and gives it the focus. `false` if it is hidden.
    pub fn focus(&mut self, p: Panel) -> bool {
        let Some((surface, node, tab)) = self.place(p) else { return false };
        let _ = self.state.set_active_tab(TabPath::new(surface, node, tab));
        self.state.set_focused_node_and_surface(NodePath::new(surface, node));
        true
    }

    /// Takes a panel out of the layout. `false` if it was not shown.
    pub fn hide(&mut self, p: Panel) -> bool {
        match self.place(p) {
            Some(place) => {
                self.state.remove_tab(place.tab_path());
                true
            }
            None => false,
        }
    }

    /// Shows a panel (where it was last, or with the panels it belongs with, or in its usual place) and
    /// focuses it. A panel that is already shown is just focused.
    pub fn show(&mut self, p: Panel) {
        if self.focus(p) {
            return;
        }
        let zone = p.zone();
        // With the panels it belongs with, if any of them is shown.
        let mate = Panel::ALL.into_iter().filter(|m| *m != p && m.zone() == zone).find_map(|m| self.place(m));
        if let Some((surface, node, _)) = mate {
            if let egui_dock::Node::Leaf(_) = &self.state[surface][node] {
                self.state[surface][node].append_tab(p);
            }
        } else {
            self.place_in_zone(p, zone);
        }
        self.focus(p);
    }

    /// Splits the main area to make room for a panel whose group-mates are all hidden.
    fn place_in_zone(&mut self, p: Panel, zone: Zone) {
        let main = SurfaceIndex::main();
        // (A tree whose groups were all removed holds only empty nodes: it has nothing to split.)
        if self.state[main].iter().all(|n| n.is_empty()) {
            self.state.push_to_focused_leaf(p);
            return;
        }
        // The group that holds the viewport (or the whole area, without one): the side panels go next to it.
        let viewport_node = self.place(Panel::Viewport).filter(|(s, _, _)| *s == main).map(|(_, n, _)| n).unwrap_or_else(NodeIndex::root);
        let tree = &mut self.state[main];
        match zone {
            Zone::Left => {
                tree.split_left(viewport_node, 0.18, vec![p]);
            }
            Zone::Right => {
                tree.split_right(viewport_node, 0.76, vec![p]);
            }
            Zone::Bottom => {
                tree.split_below(NodeIndex::root(), 0.77, vec![p]);
            }
            Zone::Center => self.state.push_to_focused_leaf(p),
        }
    }

    /// What a window shortcut does: show a hidden panel, focus a shown one that is not in use, hide the one in use.
    pub fn toggle(&mut self, p: Panel) -> Toggled {
        if !self.visible(p) {
            self.show(p);
            Toggled::Shown
        } else if self.is_focused(p) {
            self.hide(p);
            Toggled::Hidden
        } else {
            self.focus(p);
            Toggled::Focused
        }
    }

    /// Puts the default layout back.
    pub fn restore_default(&mut self) {
        *self = Dock::default_layout();
    }

    // ---- saving ----------------------------------------------------------------------------------

    /// The layout as JSON, without the places of the floating windows (see [Dock::to_json_with_windows]).
    pub fn to_json(&self) -> Json {
        self.to_json_with_windows(&|_| None)
    }

    /// The layout as JSON (what goes in the editor settings): panels by their stable names, and where each
    /// floating window is (`rect_of` says: egui keeps that, not the dock). The sizes egui_dock measured last
    /// frame are left out - they are not infinite before the first frame, which JSON cannot hold, and they are
    /// measured again anyway.
    pub fn to_json_with_windows(&self, rect_of: &dyn Fn(SurfaceIndex) -> Option<Rect>) -> Json {
        // (The Game tab is not part of the saved layout: it belongs to a running game.)
        let mut named: DockState<String> = self.state.filter_map_tabs(|p| (*p != Panel::Game).then(|| p.key().to_owned()));
        for (_, node) in named.iter_all_nodes_mut() {
            node.set_rect(Rect::ZERO);
            if let egui_dock::Node::Leaf(leaf) = node {
                leaf.viewport = Rect::ZERO;
                // (A tab that was left out - the Game tab - may have been the open one: the open tab must exist.)
                if leaf.active.0 >= leaf.tabs.len() {
                    leaf.active = TabIndex(leaf.tabs.len().saturating_sub(1));
                }
            }
        }
        // (map_tabs leaves out empty surfaces, so the windows are numbered as they will be when read back.)
        let windows: Vec<Json> = self
            .state
            .iter_surfaces()
            .enumerate()
            .filter(|(_, surface)| !surface.is_empty())
            .enumerate()
            .filter_map(|(kept, (original, surface))| {
                let rect = matches!(surface, Surface::Window(..)).then(|| rect_of(SurfaceIndex(original))).flatten().filter(|r| r.is_finite() && r.width() > 1.0 && r.height() > 1.0)?;
                Some(serde_json::json!({ "surface": kept, "x": rect.min.x, "y": rect.min.y, "width": rect.width(), "height": rect.height() }))
            })
            .collect();
        serde_json::json!({ "version": 1, "dock": serde_json::to_value(&named).unwrap_or(Json::Null), "windows": windows })
    }

    /// Reads a saved layout. `None` when it cannot be used at all; otherwise panels this version does not
    /// know are dropped, a panel listed twice is kept once, and floating windows are put back where they were.
    pub fn from_json(json: &Json) -> Option<Dock> {
        let named: DockState<String> = serde_json::from_value(json.get("dock")?.clone()).ok()?;
        let mut seen: Vec<Panel> = Vec::new();
        let mut state = named.filter_map_tabs(|key| {
            let panel = Panel::from_key(key).filter(|p| *p != Panel::Game)?;
            if seen.contains(&panel) {
                return None; // listed twice
            }
            seen.push(panel);
            Some(panel)
        });
        if state.iter_all_tabs().next().is_none() {
            return None;
        }
        // A window comes back where it was and as big as it was.
        let places: Vec<&Json> = json.get("windows").and_then(Json::as_array).map(|a| a.iter().collect()).unwrap_or_default();
        for place in places {
            let number = |key: &str| place.get(key).and_then(Json::as_f64).map(|v| v as f32).filter(|v| v.is_finite());
            let (Some(surface), Some(x), Some(y), Some(w), Some(h)) = (place.get("surface").and_then(Json::as_u64), number("x"), number("y"), number("width"), number("height")) else { continue };
            if let Some(window) = state.get_window_state_mut(SurfaceIndex(surface as usize)) {
                if w > 1.0 && h > 1.0 {
                    window.set_position(Pos2::new(x, y)).set_size(Vec2::new(w, h));
                }
            }
        }
        Some(Dock { state })
    }

    /// The layout in settings JSON, or the default when there is none or it cannot be read.
    pub fn from_saved(json: Option<&Json>) -> Dock {
        json.and_then(Dock::from_json).unwrap_or_else(Dock::default_layout)
    }
}

/// A style for the dock that follows the editor's colours (so changing them in the settings changes the dock).
pub fn style_for(ctx: &egui::Context) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(ctx.global_style().as_ref());
    let visuals = ctx.global_style().visuals.clone();
    style.tab.tab_body.inner_margin = egui::Margin::same(4);
    style.tab.tab_body.bg_fill = visuals.panel_fill;
    style.tab.tab_body.stroke = egui::Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color);
    style.tab_bar.bg_fill = visuals.extreme_bg_color;
    style.tab_bar.hline_color = visuals.widgets.noninteractive.bg_stroke.color;
    style.separator.color_idle = visuals.widgets.noninteractive.bg_stroke.color;
    style.separator.color_hovered = visuals.widgets.hovered.bg_stroke.color;
    style.separator.color_dragged = visuals.selection.stroke.color;
    style.main_surface_border_stroke = egui::Stroke::NONE;
    style
}

#[cfg(test)]
mod tests {
    use egui_dock::Node;

    use super::*;

    fn leaf_tabs(dock: &Dock, p: Panel) -> Vec<Panel> {
        let (s, n, _) = dock.place(p).expect("shown");
        dock.state[s][n].tabs().map(|t| t.to_vec()).unwrap_or_default()
    }

    #[test]
    fn the_default_layout_has_every_ordinary_panel_once_in_the_familiar_places() {
        let dock = Dock::default_layout();
        for p in [Panel::Viewport, Panel::Explorer, Panel::Properties, Panel::Output, Panel::Assets, Panel::ScriptEditor] {
            assert_eq!(dock.state.iter_all_tabs().filter(|(_, t)| **t == p).count(), 1, "{p:?}");
        }
        assert_eq!(leaf_tabs(&dock, Panel::Output), [Panel::Output, Panel::Assets, Panel::ScriptEditor], "three tabs along the bottom");
        assert_eq!(leaf_tabs(&dock, Panel::Viewport), [Panel::Viewport]);
        assert_eq!(dock.state.surfaces_count(), 1, "everything is docked: no floating windows");
        let (viewport, explorer, properties, output) = (dock.place(Panel::Viewport).unwrap().1, dock.place(Panel::Explorer).unwrap().1, dock.place(Panel::Properties).unwrap().1, dock.place(Panel::Output).unwrap().1);
        let all = [viewport, explorer, properties, output];
        assert!(all.iter().enumerate().all(|(i, a)| all.iter().skip(i + 1).all(|b| a != b)), "four separate groups");
    }

    #[test]
    fn showing_hiding_and_toggling_keep_one_truth_about_what_is_visible() {
        let mut dock = Dock::default_layout();
        assert!(dock.hide(Panel::Explorer) && !dock.visible(Panel::Explorer));
        assert!(!dock.hide(Panel::Explorer), "hiding what is hidden does nothing");
        assert!(dock.visible(Panel::Viewport) && dock.visible(Panel::Properties), "the others are untouched");

        dock.show(Panel::Explorer);
        assert!(dock.visible(Panel::Explorer) && dock.is_focused(Panel::Explorer));
        dock.show(Panel::Explorer);
        assert_eq!(dock.state.iter_all_tabs().filter(|(_, t)| **t == Panel::Explorer).count(), 1, "showing a shown panel does not add another");

        // The shortcut: hidden -> shown, shown but not in use -> focused, in use -> hidden.
        assert_eq!(dock.toggle(Panel::Properties), Toggled::Focused, "it was shown but another group had the focus");
        assert_eq!(dock.toggle(Panel::Properties), Toggled::Hidden);
        assert_eq!(dock.toggle(Panel::Properties), Toggled::Shown);
        assert!(dock.is_focused(Panel::Properties));

        // A bottom panel comes back into the tab group it belongs to, and becomes the open tab.
        dock.hide(Panel::Assets);
        assert_eq!(leaf_tabs(&dock, Panel::Output), [Panel::Output, Panel::ScriptEditor]);
        dock.show(Panel::Assets);
        assert_eq!(leaf_tabs(&dock, Panel::Output).len(), 3);
        assert!(dock.is_focused(Panel::Assets));
        // Focusing one tab of a group makes the others "not in use".
        dock.focus(Panel::Output);
        assert!(dock.is_focused(Panel::Output) && !dock.is_focused(Panel::Assets));
    }

    #[test]
    fn a_hidden_zone_comes_back_in_its_place_and_the_layout_survives_losing_everything() {
        let mut dock = Dock::default_layout();
        for p in [Panel::Output, Panel::Assets, Panel::ScriptEditor] {
            dock.hide(p);
        }
        assert!(!dock.visible(Panel::Output));
        dock.show(Panel::Assets);
        let output_zone = dock.place(Panel::Assets).unwrap().1;
        let viewport_zone = dock.place(Panel::Viewport).unwrap().1;
        assert_ne!(output_zone, viewport_zone, "the bottom group is split off again, not mixed into the viewport");
        dock.show(Panel::Output);
        assert_eq!(leaf_tabs(&dock, Panel::Assets), [Panel::Assets, Panel::Output]);

        // Everything hidden, then one panel shown: the layout is usable again.
        for p in Panel::ALL {
            dock.hide(p);
        }
        assert!(dock.visible_panels().is_empty());
        dock.show(Panel::Explorer);
        dock.show(Panel::Viewport);
        assert_eq!(dock.visible_panels().len(), 2);
        assert!(dock.focus(Panel::Viewport));
    }

    #[test]
    fn putting_the_default_layout_back_restores_every_panel_and_its_place() {
        let mut dock = Dock::default_layout();
        dock.hide(Panel::Explorer);
        let properties = dock.place(Panel::Properties).unwrap();
        dock.state.move_tab(dock.place(Panel::Output).unwrap().tab_path(), egui_dock::TabDestination::Node(egui_dock::NodePath::new(properties.0, properties.1), egui_dock::TabInsert::Append));
        assert_eq!(leaf_tabs(&dock, Panel::Properties), [Panel::Properties, Panel::Output], "rearranged");
        dock.restore_default();
        assert!(dock.visible(Panel::Explorer));
        assert_eq!(leaf_tabs(&dock, Panel::Output), [Panel::Output, Panel::Assets, Panel::ScriptEditor]);
    }

    #[test]
    fn a_panel_dragged_out_is_a_floating_window_and_can_be_hidden_again() {
        let mut dock = Dock::default_layout();
        let window = dock.state.detach_tab(dock.place(Panel::Output).unwrap().tab_path(), Rect::from_min_size(Pos2::new(50.0, 60.0), Vec2::new(400.0, 300.0)));
        assert!(!window.is_main() && dock.visible(Panel::Output));
        assert!(dock.hide(Panel::Output) && !dock.visible(Panel::Output));
    }

    #[test]
    fn a_tab_can_be_moved_to_another_group_floated_and_docked_back_without_being_lost() {
        let mut dock = Dock::default_layout();
        // Output joins the Properties group as another tab.
        let (target_surface, target_node, _) = dock.place(Panel::Properties).unwrap();
        dock.state.move_tab(dock.place(Panel::Output).unwrap().tab_path(), egui_dock::TabDestination::Node(egui_dock::NodePath::new(target_surface, target_node), egui_dock::TabInsert::Append));
        assert_eq!(leaf_tabs(&dock, Panel::Properties), [Panel::Properties, Panel::Output]);
        // Pulled out into a floating window.
        let window = dock.state.detach_tab(dock.place(Panel::Output).unwrap().tab_path(), Rect::from_min_size(Pos2::new(50.0, 60.0), Vec2::new(400.0, 300.0)));
        assert!(!window.is_main() && dock.visible(Panel::Output));
        assert_eq!(dock.state.surfaces_count(), 2);
        // Docked back next to the viewport.
        let viewport = dock.place(Panel::Viewport).unwrap();
        dock.state.move_tab(dock.place(Panel::Output).unwrap().tab_path(), egui_dock::TabDestination::Node(egui_dock::NodePath::new(viewport.0, viewport.1), egui_dock::TabInsert::Append));
        assert_eq!(leaf_tabs(&dock, Panel::Viewport), [Panel::Viewport, Panel::Output]);
        assert!(dock.state.iter_surfaces().filter(|s| !s.is_empty()).count() == 1, "the empty floating window is gone");
        assert_eq!(dock.state.iter_all_tabs().count(), 6, "no panel was lost or duplicated");
    }

    #[test]
    fn a_saved_layout_comes_back_exactly_and_the_default_can_be_saved_before_it_was_ever_drawn() {
        let mut dock = Dock::default_layout();
        dock.hide(Panel::Explorer);
        dock.focus(Panel::Assets);
        // (Never drawn: the leaves still hold infinite rectangles, which must not be written.)
        let json = dock.to_json();
        let text = serde_json::to_string(&json).unwrap();
        let back = Dock::from_json(&serde_json::from_str(&text).unwrap()).expect("reads back");
        assert_eq!(back.to_json(), json, "the same layout");
        assert_eq!(back.visible_panels(), dock.visible_panels());
        assert!(!back.visible(Panel::Explorer));
        assert_eq!(leaf_tabs(&back, Panel::Output), leaf_tabs(&dock, Panel::Output));
        assert!(matches!(&back.state[back.place(Panel::Assets).unwrap().0][back.place(Panel::Assets).unwrap().1], Node::Leaf(leaf) if leaf.active.0 == 1), "the open tab of a group is remembered");
    }

    #[test]
    fn old_odd_or_damaged_layouts_are_repaired_or_replaced_never_trusted() {
        let default = Dock::default_layout();
        let mut json = default.to_json();
        // A panel this version does not know, and one listed twice.
        let text = json.to_string().replacen("\"explorer\"", "\"explorer_from_the_future\"", 1).replacen("\"properties\"", "\"viewport\"", 1);
        json = serde_json::from_str(&text).unwrap();
        let repaired = Dock::from_json(&json).expect("still usable");
        assert!(!repaired.visible(Panel::Explorer), "an unknown panel is dropped");
        assert_eq!(repaired.state.iter_all_tabs().filter(|(_, t)| **t == Panel::Viewport).count(), 1, "a panel listed twice is kept once");
        assert!(repaired.visible(Panel::Output) && repaired.visible(Panel::Assets));

        for bad in [Json::Null, Json::String("nope".into()), serde_json::json!({}), serde_json::json!({"surfaces": 5}), serde_json::json!([1, 2, 3])] {
            assert!(Dock::from_json(&bad).is_none(), "{bad}");
            assert_eq!(Dock::from_saved(Some(&bad)).to_json(), default.to_json(), "falls back to the default");
        }
        // A layout with only unknown panels has nothing in it: default.
        let empty = serde_json::from_str::<Json>(&default.to_json().to_string().replace("\"viewport\"", "\"x1\"").replace("\"explorer\"", "\"x2\"").replace("\"properties\"", "\"x3\"").replace("\"output\"", "\"x4\"").replace("\"assets\"", "\"x5\"").replace("\"script_editor\"", "\"x6\"")).unwrap();
        assert!(Dock::from_json(&empty).is_none());
        assert_eq!(Dock::from_saved(None).to_json(), default.to_json());
    }

    #[test]
    fn every_panel_has_a_unique_key_and_the_table_matches_the_enum() {
        for (i, def) in PANELS.iter().enumerate() {
            assert_eq!(def.panel as usize, i, "{} is in the table at the position of its discriminant", def.key);
            assert_eq!(Panel::from_key(def.key), Some(def.panel));
        }
        let mut keys: Vec<&str> = PANELS.iter().map(|d| d.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), PANELS.len());
        assert_eq!(Panel::ALL.len() + 1, PANELS.len(), "every panel but the transient Game tab");
        assert!(!Panel::ALL.contains(&Panel::Game));
        assert!(Panel::from_key("nope").is_none());
    }
}
