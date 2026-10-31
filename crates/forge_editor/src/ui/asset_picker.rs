//! The window that chooses a file for a property or a material slot: every project file of the right kind, searchable, with
//! pictures for textures and a play button for sounds. Opened from the Properties panel (click a file field) and from the
//! Material Editor's texture slots.

use bevy_egui::egui::{self, vec2, Align2, Color32, FontId, Rect, Sense};
use forge_engine::project::{file_name, parent_of, AssetKind};

use super::assets::{field_thumb, kind_style};
use crate::{state::Editor, theme::SecondaryText};

/// How many rows are listed at most (the search narrows the rest).
const MAX_ROWS: usize = 400;

pub(super) fn asset_picker(ctx: &egui::Context, ed: &mut Editor) {
    let Some(mut picker) = ed.picker.take() else { return };
    let candidates = ed.pickable_assets(picker.kind, &picker.search);
    let mut chosen: Option<String> = None;
    let mut open = true;
    let kind = picker.kind;
    let mut window = egui::Window::new(picker.title.clone()).id(egui::Id::new("forge_asset_picker")).open(&mut open).collapsible(false).resizable(true).default_size([420.0, 460.0]).order(egui::Order::Foreground);
    if !picker.placed {
        window = window.anchor(Align2::CENTER_CENTER, [0.0, 0.0]);
    }
    window.show(ctx, |ui| {
        ui.horizontal(|ui| {
            let search = ui.add(egui::TextEdit::singleline(&mut picker.search).hint_text(format!("Search {} files", kind.label().to_lowercase())).desired_width(ui.available_width() - 70.0));
            if !picker.placed {
                search.request_focus();
            }
            if ui.button("Import...").on_hover_text("Copy files from your computer into the project").clicked() {
                if let Some(files) = rfd::FileDialog::new().set_title("Import files").pick_files() {
                    ed.asset_import(files);
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 30.0).show(ui, |ui| {
            let none = ui.selectable_label(picker.current.is_empty(), "(none)");
            if none.clicked() {
                chosen = Some(String::new());
            }
            if candidates.is_empty() {
                ui.add_space(6.0);
                ui.secondary(if picker.search.is_empty() { format!("The project has no {} files yet. Import some, or drag them into the Assets tab.", kind.label().to_lowercase()) } else { "Nothing matches.".to_owned() });
            }
            for path in candidates.iter().take(MAX_ROWS) {
                row(ui, ed, path, kind, path == &picker.current, &mut chosen);
            }
            if candidates.len() > MAX_ROWS {
                ui.secondary(format!("... {} more: type to narrow the list", candidates.len() - MAX_ROWS));
            }
        });
        ui.secondary(format!("{} file{}", candidates.len(), if candidates.len() == 1 { "" } else { "s" }));
    });
    picker.placed = true;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    if let Some(path) = chosen {
        let target = picker.target.clone();
        ed.apply_asset(&target, &path);
        ed.audio.stop_preview();
        return;
    }
    if open {
        ed.picker = Some(picker);
    } else {
        ed.audio.stop_preview();
    }
}

fn row(ui: &mut egui::Ui, ed: &mut Editor, path: &str, kind: AssetKind, current: bool, chosen: &mut Option<String>) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let painter = ui.painter_at(rect);
    if current {
        painter.rect_filled(rect, 3.0, ui.visuals().selection.bg_fill);
    } else if resp.hovered() {
        painter.rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let thumb = Rect::from_min_size(rect.min + vec2(3.0, 3.0), vec2(28.0, 28.0));
    painter.rect_filled(thumb, 3.0, ui.visuals().extreme_bg_color);
    let mut drew = false;
    if kind == AssetKind::Texture {
        if let Some(texture) = field_thumb(ui.ctx(), ed, path) {
            painter.image(texture.id(), thumb, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            drew = true;
        }
    }
    if !drew {
        let (glyph, color) = kind_style(kind);
        painter.rect_stroke(thumb, 3.0, egui::Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
        painter.text(thumb.center(), Align2::CENTER_CENTER, glyph, FontId::monospace(10.0), color);
    }
    let text = if current { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
    painter.text(egui::pos2(thumb.max.x + 8.0, rect.min.y + 3.0), Align2::LEFT_TOP, file_name(path), FontId::proportional(13.0), text);
    painter.text(egui::pos2(thumb.max.x + 8.0, rect.min.y + 19.0), Align2::LEFT_TOP, parent_of(path), FontId::proportional(10.5), crate::theme::secondary_color(ui));
    if kind == AssetKind::Audio {
        let playing = ed.audio.previewing(path);
        let button = Rect::from_min_size(egui::pos2(rect.max.x - 52.0, rect.min.y + 6.0), vec2(46.0, 22.0));
        if ui.put(button, egui::Button::new(if playing { "Stop" } else { "Play" }).small()).clicked() {
            ed.preview_sound(path);
            return;
        }
    }
    if resp.clicked() {
        *chosen = Some(path.to_owned());
    }
    resp.on_hover_text(path);
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;
    use forge_engine::project::Project;

    use super::*;
    use crate::state::PickTarget;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_picker_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Picker Test").expect("create project");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn draw(ctx: &egui::Context, ed: &mut Editor) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| asset_picker(ctx, ed));
    }

    #[test]
    fn the_picker_lists_the_files_of_its_kind_and_choosing_one_sets_the_property() {
        let s = Scratch::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 20, 20, 255])).save(s.0.join("Assets/Textures/red.png")).unwrap();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 20, 200, 255])).save(s.0.join("Assets/Textures/blue.png")).unwrap();
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.sync_assets(true);
        let all = ed.pickable_assets(AssetKind::Texture, "");
        assert_eq!(all, ["Assets/Textures/blue.png", "Assets/Textures/red.png"]);
        assert_eq!(ed.pickable_assets(AssetKind::Texture, "RED"), ["Assets/Textures/red.png"], "the search ignores case");
        assert_eq!(ed.pickable_assets(AssetKind::Material, "").len(), 5, "the five starter materials");
        assert!(ed.pickable_assets(AssetKind::Mesh, "").is_empty());

        let wall = { let dm = ed.dm.borrow(); dm.find_first_child(dm.workspace, "Wall", true).unwrap() };
        ed.open_picker(PickTarget::Property { targets: vec![wall], prop: "ColorMap" }, AssetKind::Texture, "Choose a texture", "");
        let ctx = egui::Context::default();
        draw(&ctx, &mut ed);
        draw(&ctx, &mut ed);
        assert!(ed.picker.is_some(), "it stays open until something is chosen");
        let target = ed.picker.as_ref().unwrap().target.clone();
        ed.apply_asset(&target, "Assets/Textures/red.png");
        assert_eq!(ed.dm.borrow().string(wall, "ColorMap"), "Assets/Textures/red.png");
        ed.apply_asset(&target, "");
        assert_eq!(ed.dm.borrow().string(wall, "ColorMap"), "");
    }
}