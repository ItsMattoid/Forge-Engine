//! Showing, hiding and focusing the editor's windows (the dockable panels), and saving the layout.
//!
//! The layout itself is [`crate::dock::Dock`]. These are the editor's operations on it: the View menu, the
//! window shortcuts, `open_script` (which brings the Script Editor forward) and the rest all go through here.
//! The Editor Settings and Project Settings windows are not part of it: they are windows of their own that
//! cannot be docked, and are opened and closed here too.
//!
//! While the dock is being drawn it is moved out of the editor (the panels being drawn need `&mut Editor`),
//! so a request made from inside a panel - double-clicking a script in the Assets tab opens the Script
//! Editor - is queued and applied right after the dock is drawn, in the same frame.

use super::Editor;
use crate::dock::{Dock, Panel};

/// A change to the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelOp {
    /// Show it (if hidden) and bring it to the front.
    Show(Panel),
    Hide(Panel),
    /// What a window shortcut does: show a hidden panel, focus a shown one that is not in use, hide the one in use.
    Toggle(Panel),
    /// The default layout again.
    Restore,
}

impl Editor {
    /// Whether a panel is shown. (Not meaningful from inside a panel being drawn: the layout is out then.)
    pub fn panel_visible(&self, p: Panel) -> bool {
        self.dock.visible(p)
    }

    pub fn show_panel(&mut self, p: Panel) {
        self.panel_op(PanelOp::Show(p));
    }

    pub fn hide_panel(&mut self, p: Panel) {
        self.panel_op(PanelOp::Hide(p));
    }

    pub fn toggle_panel(&mut self, p: Panel) {
        self.panel_op(PanelOp::Toggle(p));
    }

    pub fn restore_layout(&mut self) {
        self.panel_op(PanelOp::Restore);
    }

    fn panel_op(&mut self, op: PanelOp) {
        if self.dock_busy {
            self.panel_ops.push(op);
        } else {
            self.apply_panel_op(op);
        }
    }

    /// Applies the requests made while the dock was out of the editor.
    pub fn apply_panel_ops(&mut self) {
        for op in std::mem::take(&mut self.panel_ops) {
            self.apply_panel_op(op);
        }
    }

    fn apply_panel_op(&mut self, op: PanelOp) {
        match op {
            PanelOp::Show(p) => self.dock.show(p),
            PanelOp::Hide(p) => {
                self.dock.hide(p);
                // Without the Game tab the viewport is what is in front of its group.
                if p == Panel::Game {
                    self.dock.focus(Panel::Viewport);
                }
            }
            PanelOp::Toggle(p) => {
                self.dock.toggle(p);
            }
            PanelOp::Restore => {
                self.dock.restore_default();
                // A game that is playing keeps its tab.
                if self.playing() {
                    self.dock.show(Panel::Game);
                    if !self.game_view_active() {
                        self.dock.focus(Panel::Viewport);
                    }
                }
                self.info("Put the windows back in their default layout.");
            }
        }
    }

    // ---- the two settings windows -------------------------------------------------------------------

    pub fn editor_settings_open(&self) -> bool {
        self.settings_ui.open
    }

    pub fn project_settings_open(&self) -> bool {
        self.project_settings_ui.open
    }

    /// Opens the Editor Settings window, where it was last time.
    pub fn open_editor_settings(&mut self) {
        if !self.settings_ui.open {
            self.settings_ui.open = true;
            self.settings_ui.placed = false;
        }
    }

    pub fn close_editor_settings(&mut self) {
        self.settings_ui.open = false;
        // (A key that was being waited for is not any more: it would otherwise swallow the next key press.)
        self.settings_ui.capturing = None;
    }

    pub fn toggle_editor_settings(&mut self) {
        if self.settings_ui.open {
            self.close_editor_settings();
        } else {
            self.open_editor_settings();
        }
    }

    /// Opens the Project Settings window (there is nothing to show without a project).
    pub fn open_project_settings(&mut self) {
        if self.project.is_some() && !self.project_settings_ui.open {
            self.project_settings_ui.open = true;
            self.project_settings_ui.placed = false;
        }
    }

    pub fn close_project_settings(&mut self) {
        self.project_settings_ui.open = false;
        if self.project_settings_ui.pending_save {
            self.save_project_manifest();
        }
    }

    pub fn toggle_project_settings(&mut self) {
        if self.project_settings_ui.open {
            self.close_project_settings();
        } else {
            self.open_project_settings();
        }
    }
    /// Remembers the layout in the editor settings when it changed (not while the mouse is down: dragging a
    /// divider or a window changes it every frame).
    pub fn save_layout_if_changed(&mut self, idle: bool, window_rect: &dyn Fn(egui_dock::SurfaceIndex) -> Option<bevy_egui::egui::Rect>) {
        if !idle || self.dock_busy {
            return;
        }
        let json = self.dock.to_json_with_windows(window_rect);
        let text = json.to_string();
        if text != self.layout_saved {
            self.settings.layout = Some(json);
            self.settings.touch();
            self.layout_saved = text;
        }
    }

    /// The layout the editor starts with: the saved one, or the default.
    pub(super) fn initial_dock(saved: Option<&serde_json::Value>) -> Dock {
        Dock::from_saved(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_made_while_the_dock_is_out_are_applied_afterwards_in_order() {
        let mut ed = Editor::new(None);
        assert!(ed.panel_visible(Panel::Explorer) && ed.panel_visible(Panel::Output));
        ed.dock_busy = true;
        ed.hide_panel(Panel::Explorer);
        ed.hide_panel(Panel::Output);
        ed.show_panel(Panel::Explorer);
        assert!(ed.panel_visible(Panel::Explorer) && ed.panel_visible(Panel::Output), "nothing happens while the dock is out");
        assert_eq!(ed.panel_ops.len(), 3);
        ed.dock_busy = false;
        ed.apply_panel_ops();
        assert!(ed.panel_visible(Panel::Explorer) && !ed.panel_visible(Panel::Output));
        assert!(ed.panel_ops.is_empty());
    }

    #[test]
    fn closing_the_editor_settings_ends_a_key_that_was_being_waited_for() {
        let mut ed = Editor::new(None);
        ed.open_editor_settings();
        assert!(ed.editor_settings_open() && !ed.settings_ui.placed);
        ed.settings_ui.begin_capture(crate::keymap::Action::Save, None);
        ed.close_editor_settings();
        assert!(ed.settings_ui.capturing.is_none() && !ed.editor_settings_open(), "a key no longer waited for");
        ed.open_editor_settings();
        ed.settings_ui.begin_capture(crate::keymap::Action::Save, None);
        ed.toggle_editor_settings();
        assert!(ed.settings_ui.capturing.is_none() && !ed.editor_settings_open());
    }

    #[test]
    fn the_settings_windows_are_not_part_of_the_layout() {
        let mut ed = Editor::new(None);
        let before = ed.dock.to_json();
        ed.open_editor_settings();
        assert_eq!(ed.dock.to_json(), before, "opening a settings window does not touch the dock");
        assert_eq!(ed.dock.state.surfaces_count(), 1, "and it is not a floating dock window");
        ed.project_settings_ui.open = false;
        ed.open_project_settings();
        assert!(!ed.project_settings_open(), "there is no project to have settings");
    }
    #[test]
    fn the_layout_is_written_to_the_settings_only_when_it_changed_and_the_mouse_is_up() {
        let mut ed = Editor::new(None);
        ed.save_layout_if_changed(true, &|_| None);
        let before = ed.settings.layout.clone();
        ed.save_layout_if_changed(true, &|_| None);
        assert_eq!(ed.settings.layout, before, "unchanged: nothing new");

        ed.hide_panel(Panel::Properties);
        ed.save_layout_if_changed(false, &|_| None);
        assert_eq!(ed.settings.layout, before, "mouse down (a drag): not yet");
        ed.save_layout_if_changed(true, &|_| None);
        assert_ne!(ed.settings.layout, before);
        let saved = ed.settings.layout.clone().unwrap();
        assert!(!Dock::from_json(&saved).unwrap().visible(Panel::Properties), "and what was saved has the change");
    }

    #[test]
    fn restoring_puts_the_default_back() {
        let mut ed = Editor::new(None);
        ed.hide_panel(Panel::Output);
        ed.hide_panel(Panel::Viewport);
        ed.restore_layout();
        assert_eq!(ed.dock.to_json(), Dock::default_layout().to_json());
    }
}
