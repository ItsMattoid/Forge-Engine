//! The Material Editor window: the project's materials on the left, the selected one on the right.
//!
//! Every material is a `.mat` file. A change made here is written to the file within a moment (see
//! `Editor::flush_material`), and the viewport - which draws parts from the files - shows it at once, on every part that uses
//! the material. Materials can be created from the + New menu, duplicated, renamed (the file is renamed and the parts that
//! name it by path follow), deleted, and dragged from the list onto a part in the viewport or the Explorer, or onto a
//! `Material Asset` field in the Properties panel. The five starter materials are ordinary files: edit, replace or delete them.

use std::hash::{Hash, Hasher};

use bevy_egui::egui::{self, vec2, Color32, Sense};
use forge_engine::{
    assets::material::{AddressKind, AlphaKind, FilterKind, MaterialDef, MaterialKind, UvMode},
    project::{file_name, stem_of, AssetKind},
};

use super::asset_field::{asset_field, FieldAction};
use crate::{
    state::{AssetDrag, Editor, PickTarget},
    theme::SecondaryText,
};

const PREVIEW_EDGE: usize = 176;

pub(super) fn material_editor_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.material_editor.open {
        return;
    }
    let mut open = true;
    let mut window = egui::Window::new("Material Editor").id(egui::Id::new("forge_material_editor")).open(&mut open).resizable(true).collapsible(true).default_size([860.0, 640.0]);
    if !ed.material_editor.placed {
        window = window.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]);
    }
    window.show(ctx, |ui| {
        if ed.project.is_none() {
            ui.label("No project open.");
            return;
        }
        egui::Panel::left("material_list").resizable(true).default_size(220.0).size_range(150.0..=380.0).show(ui, |ui| list(ui, ed));
        egui::CentralPanel::default().show(ui, |ui| editor(ui, ed));
    });
    ed.material_editor.placed = true;
    // Writes happen as the mouse lets go of a slider, or every little while during the drag.
    let dragging = ctx.input(|i| i.pointer.any_down());
    ed.flush_material(dragging, false);
    if !open {
        ed.flush_material(false, true);
        ed.close_material_editor();
    }
}

// ---- the list ----------------------------------------------------------------------------------------

fn list(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.horizontal(|ui| {
        ui.menu_button("+ New", |ui| {
            for kind in MaterialKind::ALL {
                if ui.button(kind.label()).on_hover_text(kind.description()).clicked() {
                    ed.new_material(kind);
                    ui.close();
                }
            }
        });
        let has = ed.material_editor.path.is_some();
        if ui.add_enabled(has, egui::Button::new("Duplicate")).on_hover_text("Make a copy to change").clicked() {
            ed.duplicate_material();
        }
        if ui.add_enabled(has, egui::Button::new("Delete")).on_hover_text("Moves the file to .forge/trash; parts that use it go back to the engine's look for their Material").clicked() {
            ed.delete_material();
        }
    });
    ui.add(egui::TextEdit::singleline(&mut ed.material_editor.search).hint_text("Search").desired_width(ui.available_width()));
    ui.separator();
    let search = ed.material_editor.search.to_lowercase();
    let mut paths = ed.material_paths();
    paths.retain(|p| search.is_empty() || p.to_lowercase().contains(&search));
    if paths.is_empty() {
        ui.secondary(if search.is_empty() { "No materials yet. Use + New." } else { "Nothing matches." });
    }
    let current = ed.material_editor.path.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for path in paths {
            let selected = current.as_deref() == Some(path.as_str());
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            if selected {
                painter.rect_filled(rect, 3.0, ui.visuals().selection.bg_fill);
            } else if resp.hovered() {
                painter.rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
            }
            let swatch = egui::Rect::from_min_size(rect.min + vec2(3.0, 3.0), vec2(24.0, 24.0));
            match super::assets::material_swatch(ui.ctx(), ed, &path) {
                Some(texture) => {
                    painter.image(texture.id(), swatch, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    painter.rect_filled(swatch, 12.0, Color32::from_gray(90));
                }
            }
            let color = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
            let uses = ed.parts_using_material(&path);
            painter.text(egui::pos2(swatch.max.x + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, stem_of(file_name(&path)), egui::FontId::proportional(13.0), color);
            if uses > 0 {
                painter.text(egui::pos2(rect.max.x - 6.0, rect.center().y), egui::Align2::RIGHT_CENTER, format!("{uses}"), egui::FontId::proportional(11.0), crate::theme::secondary_color(ui));
            }
            if resp.clicked() {
                ed.select_material(&path);
            }
            resp.dnd_set_drag_payload(AssetDrag { paths: vec![path.clone()] });
            resp.on_hover_text(format!("{path}\nDrag onto a part to use it. Used by {uses} part(s) in this scene."));
        }
    });
}

// ---- the selected material ---------------------------------------------------------------------------

fn editor(ui: &mut egui::Ui, ed: &mut Editor) {
    let Some(path) = ed.material_editor.path.clone() else {
        ui.add_space(8.0);
        ui.label("Choose a material on the left, or make a new one with + New.");
        ui.secondary("Materials are files in the project (Assets/Materials). A part uses one through its Material Asset property, by dragging it onto the part, or, for the engine's Material list (Plastic, Metal ...), through the file with the same name.");
        return;
    };
    let mut def = ed.material_editor.def.clone();

    // The header: name, where it is, who uses it.
    ui.horizontal(|ui| {
        match ed.material_editor.renaming.clone() {
            Some(mut typed) => {
                let edit = ui.add(egui::TextEdit::singleline(&mut typed).desired_width(180.0));
                if !edit.has_focus() && !edit.lost_focus() {
                    edit.request_focus();
                }
                if edit.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        ed.material_editor.renaming = None;
                    } else {
                        ed.material_editor.renaming = None;
                        ed.rename_material(&typed);
                    }
                } else {
                    ed.material_editor.renaming = Some(typed);
                }
            }
            None => {
                ui.heading(stem_of(file_name(&path)));
                if ui.small_button("Rename").clicked() {
                    ed.material_editor.renaming = Some(stem_of(file_name(&path)).to_owned());
                }
            }
        }
    });
    ui.secondary(&path);
    let uses = ed.parts_using_material(&path);
    let items = ed.enum_items_using(&path);
    let mut note = format!("{uses} part(s) in this scene use it");
    if !items.is_empty() {
        note.push_str(&format!("; it is the look of Material = {}", items.join(", ")));
    }
    ui.secondary(note);
    ui.add_space(4.0);

    // Preview and the buttons next to it.
    ui.horizontal(|ui| {
        preview(ui, ed, &def);
        ui.vertical(|ui| {
            egui::ComboBox::from_id_salt("material_kind").selected_text(def.kind.label()).show_ui(ui, |ui| {
                for kind in MaterialKind::ALL {
                    if ui.selectable_label(def.kind == kind, kind.label()).on_hover_text(kind.description()).clicked() {
                        def.kind = kind;
                    }
                }
            });
            ui.secondary(def.kind.description());
            ui.add_space(4.0);
            if ui.button("Use on selected parts").on_hover_text("Set the Material Asset of every selected part to this material").clicked() {
                ed.apply_material_to_selection();
            }
            if ui.button("Reset to the kind's defaults").on_hover_text("Keeps the texture maps, resets the numbers").clicked() {
                let maps = def.maps.clone();
                def = MaterialDef::of_kind(&def.name, def.kind);
                def.maps = maps;
            }
        });
    });
    for problem in &ed.material_editor.problems {
        ui.colored_label(Color32::from_rgb(235, 175, 70), problem);
    }
    ui.separator();

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::CollapsingHeader::new("Surface").default_open(true).show(ui, |ui| surface(ui, &mut def));
        egui::CollapsingHeader::new("Emission").default_open(def.emits()).show(ui, |ui| emission(ui, &mut def));
        egui::CollapsingHeader::new("Texture maps").default_open(true).show(ui, |ui| maps(ui, ed, &mut def));
        egui::CollapsingHeader::new("Texture coordinates").default_open(false).show(ui, |ui| coordinates(ui, &mut def));
        egui::CollapsingHeader::new("Texture sampling").default_open(false).show(ui, |ui| sampling(ui, &mut def));
        ui.add_space(8.0);
        ui.secondary("Colours are sRGB like a part's Color; a part's Color multiplies the material's colour and map, and its Transparency multiplies the opacity. A part's own Texture replaces the base-colour map.");
    });

    if def != ed.material_editor.def {
        ed.material_editor.def = def;
        ed.material_changed();
    }
}

fn slider(ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, tip: &str) {
    ui.horizontal(|ui| {
        ui.label(label).on_hover_text(tip);
        ui.add(egui::Slider::new(value, range));
    });
}

fn colour(ui: &mut egui::Ui, label: &str, value: &mut [f32; 3], tip: &str) {
    ui.horizontal(|ui| {
        ui.label(label).on_hover_text(tip);
        let mut rgb = [value[0].clamp(0.0, 1.0), value[1].clamp(0.0, 1.0), value[2].clamp(0.0, 1.0)];
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            *value = rgb;
        }
    });
}

fn surface(ui: &mut egui::Ui, def: &mut MaterialDef) {
    colour(ui, "Colour", &mut def.base_color, "Multiplied with the part's Color and the colour map");
    slider(ui, "Opacity", &mut def.opacity, 0.0..=1.0, "1 is solid, lower is see-through; multiplied with the part's own opacity");
    slider(ui, "Roughness", &mut def.roughness, 0.0..=1.0, "0 is a mirror, 1 is chalk");
    slider(ui, "Metallic", &mut def.metallic, 0.0..=1.0, "0 is paint or plastic, 1 is bare metal");
    slider(ui, "Reflectance", &mut def.reflectance, 0.0..=1.0, "How strongly non-metals reflect (0.5 is ordinary)");
    ui.horizontal(|ui| {
        ui.label("Transparency mode").on_hover_text("Opaque ignores the alpha of the map; Blend is see-through; Mask is a cut-out (leaves, fences)");
        egui::ComboBox::from_id_salt("material_alpha").selected_text(alpha_label(def.alpha_mode)).show_ui(ui, |ui| {
            for mode in [AlphaKind::Opaque, AlphaKind::Blend, AlphaKind::Mask] {
                ui.selectable_value(&mut def.alpha_mode, mode, alpha_label(mode));
            }
        });
    });
    if def.alpha_mode == AlphaKind::Mask {
        slider(ui, "Cut-off", &mut def.alpha_cutoff, 0.0..=1.0, "Where the map's alpha is below this the surface is not drawn");
    }
    ui.checkbox(&mut def.double_sided, "Double sided").on_hover_text("Draw the back faces too (thin leaves, cloth)");
    if def.kind == MaterialKind::Glass {
        slider(ui, "Edge opacity", &mut def.edge_opacity, 0.0..=1.0, "Glass shows more of itself at a slant, like real glass");
    }
    slider(ui, "Refraction", &mut def.transmission, 0.0..=1.0, "Bends the view of what is behind the surface (costs a copy of the scene; 0 is off)");
    if def.transmission > 0.0 {
        slider(ui, "Index of refraction", &mut def.ior, 1.0..=3.0, "1.5 is glass, 1.33 water, 2.4 diamond");
    }
}

fn alpha_label(mode: AlphaKind) -> &'static str {
    match mode {
        AlphaKind::Opaque => "Opaque",
        AlphaKind::Blend => "Blend",
        AlphaKind::Mask => "Mask (cut-out)",
    }
}

fn emission(ui: &mut egui::Ui, def: &mut MaterialDef) {
    slider(ui, "Strength", &mut def.emission_strength, 0.0..=20.0, "0 gives off no light; Neon is around 3");
    colour(ui, "Colour", &mut def.emission_color, "The colour of the light given off");
    ui.checkbox(&mut def.emission_from_color, "Also use the part's colour").on_hover_text("Multiply the emission by the part's Color, so every part glows in its own colour");
}

fn maps(ui: &mut egui::Ui, ed: &mut Editor, def: &mut MaterialDef) {
    let slots: [(&'static str, &str, &str); 7] = [
        ("base_color", "Base colour", "The colour of the surface (sRGB); its alpha is the opacity"),
        ("normal", "Normal", "Bumps and dents without more triangles; lights correctly with every light"),
        ("roughness", "Roughness", "A grey picture: white is rough, black is shiny"),
        ("metallic", "Metallic", "A grey picture: white is metal, black is not"),
        ("metallic_roughness", "Roughness + metallic", "One picture with roughness in green and metallic in blue (glTF); used instead of the two above"),
        ("occlusion", "Ambient occlusion", "A grey picture that darkens creases"),
        ("emission", "Emission", "Which parts of the surface glow, and in what colour"),
    ];
    for (slot, label, tip) in slots {
        let current = match slot {
            "base_color" => def.maps.base_color.clone(),
            "normal" => def.maps.normal.clone(),
            "roughness" => def.maps.roughness.clone(),
            "metallic" => def.maps.metallic.clone(),
            "metallic_roughness" => def.maps.metallic_roughness.clone(),
            "occlusion" => def.maps.occlusion.clone(),
            _ => def.maps.emission.clone(),
        };
        let mut action = FieldAction::None;
        ui.horizontal(|ui| {
            ui.add_sized([120.0, 22.0], egui::Label::new(label)).on_hover_text(tip);
            action = asset_field(ui, ed, ("material_map", slot), AssetKind::Texture, &current, false);
        });
        let index = ["base_color", "normal", "roughness", "metallic", "metallic_roughness", "occlusion", "emission"].iter().position(|s| *s == slot).unwrap_or(0);
        match action {
            FieldAction::Set(p) => *def.maps.paths_mut()[index] = p,
            FieldAction::Clear => def.maps.paths_mut()[index].clear(),
            FieldAction::Pick => ed.open_picker(PickTarget::MaterialMap(slot), AssetKind::Texture, format!("Choose the {label} map"), &current),
            _ => {}
        }
    }
    if !def.maps.normal.is_empty() {
        slider(ui, "Normal strength", &mut def.normal_strength, 0.0..=4.0, "How deep the bumps look (1 as authored)");
        ui.checkbox(&mut def.flip_normal_y, "Flip the normal map's green channel").on_hover_text("For normal maps made for DirectX (Y down) instead of OpenGL / glTF (Y up)");
    }
    if !def.maps.occlusion.is_empty() {
        slider(ui, "Occlusion strength", &mut def.occlusion_strength, 0.0..=1.0, "How much the occlusion map darkens");
    }
}

fn coordinates(ui: &mut egui::Ui, def: &mut MaterialDef) {
    ui.horizontal(|ui| {
        ui.label("Mapping").on_hover_text("Auto: studs on the engine's shapes (a texture keeps its size on a part of any size), the model's own coordinates on a mesh");
        egui::ComboBox::from_id_salt("material_uv").selected_text(uv_label(def.uv.mode)).show_ui(ui, |ui| {
            for mode in [UvMode::Auto, UvMode::Mesh, UvMode::Studs] {
                ui.selectable_value(&mut def.uv.mode, mode, uv_label(mode));
            }
        });
    });
    if def.uv.mode != UvMode::Mesh {
        ui.horizontal(|ui| {
            ui.label("Studs per tile").on_hover_text("How many studs one repeat of the maps covers");
            ui.add(egui::DragValue::new(&mut def.uv.studs_per_tile[0]).speed(0.1).range(0.01..=1000.0).prefix("U "));
            ui.add(egui::DragValue::new(&mut def.uv.studs_per_tile[1]).speed(0.1).range(0.01..=1000.0).prefix("V "));
        });
    }
    ui.horizontal(|ui| {
        ui.label("Tiling");
        ui.add(egui::DragValue::new(&mut def.uv.tiling[0]).speed(0.05).prefix("U "));
        ui.add(egui::DragValue::new(&mut def.uv.tiling[1]).speed(0.05).prefix("V "));
    });
    ui.horizontal(|ui| {
        ui.label("Offset");
        ui.add(egui::DragValue::new(&mut def.uv.offset[0]).speed(0.01).prefix("U "));
        ui.add(egui::DragValue::new(&mut def.uv.offset[1]).speed(0.01).prefix("V "));
    });
    slider(ui, "Rotation", &mut def.uv.rotation, -180.0..=180.0, "Turns the maps, in degrees");
}

fn uv_label(mode: UvMode) -> &'static str {
    match mode {
        UvMode::Auto => "Auto",
        UvMode::Mesh => "Model's coordinates",
        UvMode::Studs => "Studs (projected)",
    }
}

fn sampling(ui: &mut egui::Ui, def: &mut MaterialDef) {
    ui.horizontal(|ui| {
        ui.label("Filter");
        egui::ComboBox::from_id_salt("material_filter").selected_text(if def.sampler.filter == FilterKind::Linear { "Smooth" } else { "Pixelated" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut def.sampler.filter, FilterKind::Linear, "Smooth (mip-maps)");
            ui.selectable_value(&mut def.sampler.filter, FilterKind::Nearest, "Pixelated (nearest)");
        });
    });
    ui.horizontal(|ui| {
        ui.label("Wrapping");
        egui::ComboBox::from_id_salt("material_wrap").selected_text(match def.sampler.address {
            AddressKind::Repeat => "Repeat",
            AddressKind::Clamp => "Clamp",
            AddressKind::Mirror => "Mirror",
        }).show_ui(ui, |ui| {
            ui.selectable_value(&mut def.sampler.address, AddressKind::Repeat, "Repeat");
            ui.selectable_value(&mut def.sampler.address, AddressKind::Clamp, "Clamp to edge");
            ui.selectable_value(&mut def.sampler.address, AddressKind::Mirror, "Mirror");
        });
    });
    if def.sampler.filter == FilterKind::Linear {
        let mut aniso = i32::from(def.sampler.anisotropy);
        ui.horizontal(|ui| {
            ui.label("Anisotropy").on_hover_text("Keeps textures sharp on surfaces seen at a slant (1 is off)");
            if ui.add(egui::Slider::new(&mut aniso, 1..=16)).changed() {
                def.sampler.anisotropy = aniso as u8;
            }
        });
    }
}

// ---- the picture -------------------------------------------------------------------------------------

fn preview(ui: &mut egui::Ui, ed: &mut Editor, def: &MaterialDef) {
    // The base-colour map, decoded small for the shading (read once per file).
    let map_path = def.maps.base_color.clone();
    let map = if map_path.is_empty() {
        None
    } else {
        if !ed.material_editor.preview_textures.contains_key(&map_path) {
            let image = ed.project.as_ref().and_then(|p| p.abs(&map_path).ok()).and_then(|f| image::open(f).ok()).map(|i| std::sync::Arc::new(i.thumbnail(128, 128).to_rgba8()));
            ed.material_editor.preview_textures.insert(map_path.clone(), image);
        }
        ed.material_editor.preview_textures.get(&map_path).cloned().flatten()
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    def.to_json().hash(&mut hasher);
    map.as_ref().map(|m| (m.width(), m.height(), m.as_raw().len())).hash(&mut hasher);
    let key = hasher.finish();
    let stale = ed.material_editor.preview.as_ref().is_none_or(|(k, _)| *k != key);
    if stale {
        let image = super::material_preview::render(def, map.as_deref(), PREVIEW_EDGE);
        let handle = ui.ctx().load_texture("material_preview", image, egui::TextureOptions::LINEAR);
        ed.material_editor.preview = Some((key, handle));
    }
    if let Some((_, texture)) = &ed.material_editor.preview {
        ui.image((texture.id(), vec2(PREVIEW_EDGE as f32, PREVIEW_EDGE as f32)));
    }
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;
    use forge_engine::project::Project;

    use super::*;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_material_editor_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Material Editor Test").expect("create project");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn draw(ctx: &egui::Context, ed: &mut Editor) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| material_editor_window(ctx, ed));
    }

    fn read(s: &Scratch, path: &str) -> MaterialDef {
        MaterialDef::parse(&std::fs::read(s.0.join(path)).unwrap()).unwrap().0
    }

    #[test]
    fn a_new_project_has_the_five_starter_materials_as_files_and_the_window_draws_them() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.sync_assets(true);
        let names: Vec<String> = ed.material_paths().iter().map(|p| stem_of(file_name(p)).to_owned()).collect();
        assert_eq!(names, ["Glass", "Metal", "Neon", "Plastic", "SmoothPlastic"]);
        ed.open_material_editor(None);
        assert_eq!(ed.material_editor.path.as_deref(), Some("Assets/Materials/Glass.mat"));
        let ctx = egui::Context::default();
        draw(&ctx, &mut ed);
        draw(&ctx, &mut ed);
        assert_eq!(read(&s, "Assets/Materials/Neon.mat").kind, MaterialKind::Neon);
        assert!(read(&s, "Assets/Materials/Metal.mat").metallic > 0.9);
    }

    #[test]
    fn editing_a_material_writes_its_file_and_it_survives_a_reopen() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.sync_assets(true);
        ed.open_material_editor(Some("Assets/Materials/Plastic.mat"));
        ed.material_editor.def.roughness = 0.123;
        ed.material_editor.def.maps.normal = "Assets/Textures/n.png".into();
        ed.material_changed();
        assert!(ed.material_editor.pending);
        assert_eq!(read(&s, "Assets/Materials/Plastic.mat").roughness, 0.8, "not written yet");
        ed.flush_material(false, false);
        let on_disk = read(&s, "Assets/Materials/Plastic.mat");
        assert_eq!((on_disk.roughness, on_disk.maps.normal.as_str()), (0.123, "Assets/Textures/n.png"));
        assert!(ed.material_editor.problems.iter().any(|p| p.contains("normal map") || p.contains("not in the project")), "{:?}", ed.material_editor.problems);

        // A drag in progress writes at most now and then.
        ed.material_editor.def.roughness = 0.2;
        ed.material_changed();
        ed.flush_material(true, false);
        assert_eq!(read(&s, "Assets/Materials/Plastic.mat").roughness, 0.123, "just saved: the drag waits");
        ed.flush_material(false, false);
        assert_eq!(read(&s, "Assets/Materials/Plastic.mat").roughness, 0.2, "let go: written at once");

        let mut again = Editor::new(Some(s.0.clone()));
        again.sync_assets(true);
        again.open_material_editor(Some("Assets/Materials/Plastic.mat"));
        assert_eq!(again.material_editor.def.roughness, 0.2);
    }

    #[test]
    fn materials_can_be_created_duplicated_renamed_and_deleted_without_touching_the_engine() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.sync_assets(true);
        let made = ed.new_material(MaterialKind::Glass).expect("created");
        assert_eq!(made, "Assets/Materials/New Glass.mat");
        assert_eq!(read(&s, &made).kind, MaterialKind::Glass);
        ed.rename_material("Window");
        assert!(s.0.join("Assets/Materials/Window.mat").is_file() && !s.0.join(&made).exists());
        ed.duplicate_material();
        assert!(s.0.join("Assets/Materials/Window Copy.mat").is_file());
        assert_eq!(ed.material_editor.path.as_deref(), Some("Assets/Materials/Window Copy.mat"));
        ed.delete_material();
        let crate::state::Dialog::DeleteAssets { paths, .. } = &ed.dialog else { panic!("it asks first") };
        let paths = paths.clone();
        ed.asset_delete(&paths);
        assert!(!s.0.join("Assets/Materials/Window Copy.mat").exists());
        // A starter material can be deleted and replaced like any other file.
        ed.asset_delete(&["Assets/Materials/Plastic.mat".to_owned()]);
        assert!(!s.0.join("Assets/Materials/Plastic.mat").exists());
        assert!(ed.material_paths().iter().all(|p| p != "Assets/Materials/Plastic.mat"), "deleted from the list too (SmoothPlastic is another material)");
    }

    #[test]
    fn a_material_is_assigned_to_parts_by_the_editor_the_picker_and_drag_and_drop() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.sync_assets(true);
        let wall = { let dm = ed.dm.borrow(); dm.find_first_child(dm.workspace, "Wall", true).unwrap() };
        ed.select_only(Some(wall));
        ed.open_material_editor(Some("Assets/Materials/Metal.mat"));
        ed.apply_material_to_selection();
        assert_eq!(ed.dm.borrow().string(wall, "MaterialVariant"), "Assets/Materials/Metal.mat");
        assert_eq!(ed.parts_using_material("Assets/Materials/Metal.mat"), 2, "the Wall by path, and the Spinner whose Material is Metal");
        // Dragging a material file onto a part is a plan the editor understands.
        let step = { let dm = ed.dm.borrow(); dm.find_first_child(dm.workspace, "Step 1", true).unwrap() };
        assert!(matches!(ed.plan_asset("Assets/Materials/Neon.mat", Some(step), None, true), Ok(crate::state::AssetPlan::AssignMaterial { target, .. }) if target == step));
        ed.drop_assets(&["Assets/Materials/Neon.mat".to_owned()], Some(step), None);
        assert_eq!(ed.dm.borrow().string(step, "MaterialVariant"), "Assets/Materials/Neon.mat");
        // ... and double-clicking one opens the editor.
        assert!(matches!(ed.plan_asset("Assets/Materials/Neon.mat", None, None, false), Ok(crate::state::AssetPlan::OpenMaterial(_))));
        // A material cannot be dropped on empty space.
        assert!(ed.plan_asset("Assets/Materials/Neon.mat", None, None, true).is_err());
    }
}
