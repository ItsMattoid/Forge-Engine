//! The Project Settings window (View > Project Settings, File > Project Settings, or its shortcut).
//!
//! The same layout as the Editor Settings window: a list of tabs on the left, grouped under dropdowns,
//! and the chosen tab on the right, scrolling inside a window that can be moved and resized but not docked,
//! and that opens where it was last left (see `window_memory`). What is
//! edited here belongs to the project and is saved in `Project.toml`: changes are written as soon as the
//! mouse and keyboard are idle (and when the window closes), so there is no Save button to forget.
//!
//! The pages: *General* (name, version, start scene, model scale), *Game Type* (single-player or multiplayer), then the game's
//! window: *Display*, *Graphics* and *Icon & Splash Screen*, and *Multiplayer*, which only a multiplayer game has.
//! Everything here is the project's own: the defaults of the built game. The editor's own preferences (its camera,
//! its viewport rendering) are in Editor Settings.

use bevy_egui::egui::{self, Color32, RichText};
use forge_engine::project::{
    launch::{self, Role},
    player_settings::PlayerSettings,
    slugify, DisplayMode, GameType, QualityPreset, ShadowQuality, WindowSettings,
};

use crate::{
    state::{project_tabs, Editor, LaunchPreview, ProjectTab},
    theme::SecondaryText,
};

/// Width of the tab list on the left.
const TAB_LIST_WIDTH: f32 = 150.0;

const ERROR: Color32 = Color32::from_rgb(235, 100, 100);
const NOTE: Color32 = Color32::from_rgb(235, 175, 70);

const WINDOW: super::window_memory::Spec = super::window_memory::Spec { key: "project_settings", title: "Project Settings", default_size: egui::vec2(760.0, 480.0), min_size: egui::vec2(460.0, 300.0) };

/// The Project Settings window, while it is open.
pub(super) fn project_settings_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.project_settings_ui.open {
        return;
    }
    if ed.project.is_none() {
        // (The project went away - another one was opened - with the window up.)
        ed.project_settings_ui.open = false;
        return;
    }
    let mut placed = ed.project_settings_ui.placed;
    let still_open = super::window_memory::show(ctx, ed, &WINDOW, &mut placed, project_settings_body);
    ed.project_settings_ui.placed = placed;
    if !still_open {
        ed.close_project_settings();
    }
}

/// What the Project Settings window shows: the tab list on the left and the chosen tab on the right.
pub(super) fn project_settings_body(ui: &mut egui::Ui, ed: &mut Editor) {
    let Some(game_type) = ed.project.as_ref().map(|p| p.manifest.game_type()) else {
        ui.secondary("Open or create a project to change its settings.");
        return;
    };
    // The pages that apply to this game type. (The Multiplayer page goes when the game becomes single-player: the
    // window moves to the page that decides it, so nothing is left showing settings the game does not have.)
    let groups = project_tabs(game_type);
    if !groups.iter().any(|(_, tabs)| tabs.contains(&ed.project_settings_ui.tab)) {
        ed.project_settings_ui.tab = ProjectTab::GameType;
    }
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        // Tabs, in groups that fold away.
        ui.allocate_ui_with_layout(egui::vec2(TAB_LIST_WIDTH, height), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(TAB_LIST_WIDTH);
            egui::ScrollArea::vertical().id_salt("project_settings_tabs").auto_shrink([false, false]).show(ui, |ui| {
                for (group, tabs) in &groups {
                    egui::CollapsingHeader::new(RichText::new(*group).strong()).id_salt(("project_settings_group", *group)).default_open(true).show(ui, |ui| {
                        for tab in *tabs {
                            if ui.selectable_label(ed.project_settings_ui.tab == *tab, tab.label()).clicked() {
                                ed.project_settings_ui.tab = *tab;
                            }
                        }
                    });
                }
            });
        });
        ui.separator();
        // The tab itself: as big as what is left of the panel, scrolling when the content is bigger. (It has
        // its own top-to-bottom layout: this row lays things out left to right.)
        let tab = ed.project_settings_ui.tab;
        let size = egui::vec2(ui.available_width(), height);
        ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::both().id_salt(("project_settings_tab", tab.label())).auto_shrink([false, false]).show(ui, |ui| {
                let changed = match tab {
                    ProjectTab::General => general_tab(ui, ed),
                    ProjectTab::GameType => game_type_tab(ui, ed),
                    ProjectTab::Display => display_tab(ui, ed),
                    ProjectTab::Graphics => graphics_tab(ui, ed),
                    ProjectTab::Launch => launch_tab(ui, ed),
                    ProjectTab::Multiplayer => multiplayer_tab(ui, ed),
                };
                if changed {
                    ed.project_settings_ui.pending_save = true;
                }
            });
        });
    });
}

/// Writes the project settings that were changed to `Project.toml` once nothing is being dragged or typed
/// (a drag changes the value every frame). Closing the window writes them too (see `Editor::close_project_settings`).
pub(super) fn save_when_idle(ctx: &egui::Context, ed: &mut Editor) {
    if ed.project_settings_ui.pending_save && !(ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input()) {
        ed.save_project_manifest();
    }
}
fn assert_stacks_vertically(ui: &egui::Ui) {
    assert!(!ui.layout().main_dir().is_horizontal(), "the project settings tab content is laid out sideways instead of top to bottom");
}

/// A page's section title.
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(title).strong());
}

/// Says which part of the game a page's settings are for: the built game (and Play in Window), never the editor.
fn applies_to_the_game(ui: &mut egui::Ui) {
    ui.secondary("Applies to: the built game and Play in Window. Play in the editor's viewport and the viewport itself use the editor's own settings (Editor Settings > Rendering).");
}

fn general_tab(ui: &mut egui::Ui, ed: &mut Editor) -> bool {
    assert_stacks_vertically(ui);
    ui.heading("General");
    let scenes = ed.scenes.clone();
    let Some(project) = ed.project.as_mut() else { return false };
    let root = project.root.display().to_string();
    let manifest_file = project.manifest_file;
    let mut changed = false;
    let info = &mut project.manifest.project;
    let model_scale = &mut project.manifest.assets.mesh_unit_scale;
    egui::Grid::new("project_general").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Name");
        changed |= ui.add(egui::TextEdit::singleline(&mut info.name).desired_width(260.0)).on_hover_text("Shown in the editor, and the default window title and the name of the built game.").changed();
        ui.end_row();

        ui.label("Version");
        changed |= ui.add(egui::TextEdit::singleline(&mut info.version).desired_width(120.0)).on_hover_text("Your own version number for the game.").changed();
        ui.end_row();

        ui.label("Start scene");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("project_start_scene").selected_text(info.start_scene.clone()).width(260.0).show_ui(ui, |ui| {
                for s in &scenes {
                    changed |= ui.selectable_value(&mut info.start_scene, s.clone(), s).changed();
                }
            });
            if !scenes.contains(&info.start_scene) {
                ui.colored_label(ERROR, "missing").on_hover_text("This scene is not in the project. A built game would not start.");
            }
        });
        ui.end_row();

        ui.label("Model scale");
        changed |= ui.add(egui::DragValue::new(model_scale).speed(0.01).range(0.0001..=10_000.0).suffix(" studs per unit")).on_hover_text("How many studs one unit of a model file is. A model made 2 units tall is 2 x this many studs tall. Changing it re-reads every model; parts that already have a size keep it.").changed();
        ui.end_row();

        ui.label("Folder");
        ui.secondary(&root);
        ui.end_row();

        ui.label("Settings file");
        ui.secondary(manifest_file);
        ui.end_row();
    });
    ui.add_space(8.0);
    ui.secondary("The scene the game opens first. Any scene in the project can be chosen, wherever it is stored (or set it from the Assets tab: right-click a scene > Set as Start Scene).");
    changed
}

// ---- game type -----------------------------------------------------------------------------------------

fn game_type_description(kind: GameType) -> &'static str {
    match kind {
        GameType::SinglePlayer => "One player on one computer. The Multiplayer page of these settings and the Network Statistics debug overlay are hidden.",
        GameType::Multiplayer => "A game for several players. Forge has no networking yet, so for now this records what the game is meant to be; the Multiplayer page of these settings appears, and is where its options will go as the engine gets them.",
    }
}

fn game_type_tab(ui: &mut egui::Ui, ed: &mut Editor) -> bool {
    assert_stacks_vertically(ui);
    ui.heading("Game Type");
    let Some(project) = ed.project.as_mut() else { return false };
    let mut changed = false;
    ui.secondary("What kind of game this is. It decides which settings and tools are shown; it is saved in Project.toml and is the same the next time the project opens.");
    ui.add_space(6.0);
    for kind in GameType::ALL {
        changed |= ui.radio_value(&mut project.manifest.game.kind, kind, kind.label()).changed();
        ui.indent(("game_type", kind.label()), |ui| ui.secondary(game_type_description(kind)));
    }
    section(ui, "What the type changes");
    let multiplayer = project.manifest.game.kind.is_multiplayer();
    ui.horizontal(|ui| {
        ui.add_enabled(multiplayer, egui::Label::new("Multiplayer page"));
        ui.secondary(if multiplayer { "shown in this window" } else { "hidden: only a multiplayer game has one" });
    });
    ui.horizontal(|ui| {
        ui.add_enabled(multiplayer, egui::Label::new("Network Statistics overlay (Debug menu)"));
        ui.secondary(if multiplayer { "available" } else { "hidden" });
    });
    ui.add_space(8.0);
    ui.secondary("Switching the type never throws settings away: what was set for multiplayer stays in the project file while the game is single-player, and is there again when it is switched back.");
    changed
}

fn multiplayer_tab(ui: &mut egui::Ui, ed: &mut Editor) -> bool {
    assert_stacks_vertically(ui);
    ui.heading("Multiplayer");
    let Some(project) = ed.project.as_ref() else { return false };
    ui.secondary("Forge has no multiplayer networking yet: there is one local player, and no connection, server or session to configure. Nothing is offered here that the engine could not honour.");
    ui.add_space(6.0);
    ui.secondary("This page is where the options of a multiplayer game go - player slots, connection and session settings - as the engine gets them. The project is already marked as multiplayer, so they will be here, with your other settings, when they arrive.");
    let kept = &project.manifest.multiplayer.extra;
    if !kept.is_empty() {
        section(ui, "Kept from the project file");
        ui.secondary("[multiplayer] in Project.toml has these settings that this version of Forge does not use. They are kept as they are.");
        for (key, value) in kept {
            ui.horizontal(|ui| {
                ui.label(RichText::new(key).monospace());
                ui.secondary(value.to_string());
            });
        }
    }
    false
}

// ---- display and graphics -----------------------------------------------------------------------------

/// Puts the fields of `from` that `fields` names back to their starting values (and keeps the rest).
fn reset_window(window: &mut WindowSettings, fields: impl FnOnce(&mut WindowSettings, &WindowSettings)) {
    let defaults = WindowSettings::default();
    fields(window, &defaults);
}

fn display_tab(ui: &mut egui::Ui, ed: &mut Editor) -> bool {
    assert_stacks_vertically(ui);
    ui.heading("Display");
    let Some(project) = ed.project.as_mut() else { return false };
    let name = project.name().to_owned();
    let w = &mut project.manifest.window;
    let mut changed = false;
    applies_to_the_game(ui);
    ui.add_space(6.0);
    egui::Grid::new("project_display").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Window title");
        let mut title = w.title.clone().unwrap_or_default();
        if ui.add(egui::TextEdit::singleline(&mut title).hint_text(name).desired_width(260.0)).on_hover_text("Empty uses the project name.").changed() {
            w.title = (!title.trim().is_empty()).then_some(title);
            changed = true;
        }
        ui.end_row();

        ui.label("Display mode");
        let mut mode = w.display_mode;
        egui::ComboBox::from_id_salt("project_display_mode").selected_text(mode.label()).show_ui(ui, |ui| {
            for m in DisplayMode::ALL {
                ui.selectable_value(&mut mode, m, m.label());
            }
        });
        if mode != w.display_mode {
            w.set_display_mode(mode);
            changed = true;
        }
        ui.end_row();

        ui.label("Window size");
        ui.horizontal(|ui| {
            changed |= ui.add(egui::DragValue::new(&mut w.width).range(320..=7680).suffix(" px")).changed();
            ui.label("x");
            changed |= ui.add(egui::DragValue::new(&mut w.height).range(240..=4320).suffix(" px")).changed();
        });
        ui.end_row();

        ui.label("Resizable");
        changed |= ui.checkbox(&mut w.resizable, "The player can resize the window").changed();
        ui.end_row();
    });
    ui.add_space(8.0);
    if ui.add_enabled(w.title.is_some() || (w.width, w.height, w.display_mode, w.resizable) != (1280, 720, DisplayMode::Windowed, true), egui::Button::new("Reset display settings")).clicked() {
        reset_window(w, |w, d| {
            w.title = d.title.clone();
            (w.width, w.height, w.resizable) = (d.width, d.height, d.resizable);
            w.set_display_mode(d.display_mode);
        });
        changed = true;
    }
    ui.add_space(6.0);
    ui.secondary("The window size is the size of a windowed game, and what Borderless fullscreen and Fullscreen go back to. Both fullscreen modes use the screen's own resolution.");
    changed
}

fn graphics_tab(ui: &mut egui::Ui, ed: &mut Editor) -> bool {
    assert_stacks_vertically(ui);
    ui.heading("Graphics");
    let Some(project) = ed.project.as_mut() else { return false };
    let slug = slugify(project.name());
    let w = &mut project.manifest.window;
    let mut changed = false;
    applies_to_the_game(ui);

    section(ui, "Quality");
    egui::Grid::new("project_quality").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Quality preset");
        let current = w.quality();
        egui::ComboBox::from_id_salt("project_quality_preset").selected_text(current.label()).show_ui(ui, |ui| {
            for preset in QualityPreset::CHOICES {
                if ui.selectable_label(current == preset, preset.label()).clicked() && current != preset {
                    w.apply_quality(preset);
                    changed = true;
                }
            }
            ui.add_enabled(false, egui::Button::new("Custom").selected(current == QualityPreset::Custom)).on_disabled_hover_text("What the two settings below add up to when they are none of the presets");
        })
        .response
        .on_hover_text("A set of the anti-aliasing and shadow settings below. Changing either of them yourself makes it Custom.");
        ui.end_row();

        ui.label("Anti-aliasing (MSAA)");
        egui::ComboBox::from_id_salt("project_msaa").selected_text(if w.msaa == 0 { "Off".to_owned() } else { format!("{}x", w.msaa) }).show_ui(ui, |ui| {
            for samples in [0u8, 2, 4, 8] {
                changed |= ui.selectable_value(&mut w.msaa, samples, if samples == 0 { "Off".to_owned() } else { format!("{samples}x") }).changed();
            }
        });
        ui.end_row();

        ui.label("Shadows");
        egui::ComboBox::from_id_salt("project_shadows").selected_text(w.shadow_quality.label()).show_ui(ui, |ui| {
            for quality in ShadowQuality::ALL {
                changed |= ui.selectable_value(&mut w.shadow_quality, quality, quality.label()).changed();
            }
        })
        .response
        .on_hover_text("The detail of the shadows the sun and the lights cast (the size of their shadow maps). Off: nothing casts a shadow, whatever the lights ask for.");
        ui.end_row();
    });

    section(ui, "Frame rate");
    egui::Grid::new("project_frame_rate").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("V-Sync");
        changed |= ui.checkbox(&mut w.vsync, "Match the frame rate to the monitor").on_hover_text("Off lets the game run as fast as the computer can, which can show tearing.").changed();
        ui.end_row();

        ui.label("Frame-rate limit");
        ui.horizontal(|ui| {
            let mut limited = w.fps_limit > 0;
            if ui.checkbox(&mut limited, "Limit to").on_hover_text("The game waits between frames so it never goes faster than this (with or without V-Sync).").changed() {
                w.fps_limit = if limited { 144 } else { 0 };
                changed = true;
            }
            ui.add_enabled_ui(limited, |ui| {
                let mut limit = w.fps_limit.max(forge_engine::project::manifest::MIN_FPS_LIMIT);
                if ui.add(egui::DragValue::new(&mut limit).range(forge_engine::project::manifest::MIN_FPS_LIMIT..=forge_engine::project::manifest::MAX_FPS_LIMIT).speed(1.0).suffix(" FPS")).changed() {
                    w.fps_limit = limit;
                    changed = true;
                }
            });
            if !limited {
                ui.secondary("unlimited");
            }
        });
        ui.end_row();
    });

    section(ui, "Advanced");
    egui::Grid::new("project_graphics").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Occlusion culling");
        changed |= ui
            .checkbox(&mut w.occlusion_culling, "Skip what is hidden behind other objects")
            .on_hover_text("Needs graphics hardware that can build draw lists on the GPU (Vulkan, Metal). It only helps when the GPU is the bottleneck (a lot of hidden overdraw) and anti-aliasing is off; otherwise it makes frames slower, so it is off by default.")
            .changed();
        ui.end_row();
    });
    ui.add_space(8.0);
    let defaults = WindowSettings::default();
    let differs = (w.msaa, w.shadow_quality, w.vsync, w.fps_limit, w.occlusion_culling) != (defaults.msaa, defaults.shadow_quality, defaults.vsync, defaults.fps_limit, defaults.occlusion_culling);
    if ui.add_enabled(differs, egui::Button::new("Reset graphics settings")).clicked() {
        reset_window(w, |w, d| (w.msaa, w.shadow_quality, w.vsync, w.fps_limit, w.occlusion_culling) = (d.msaa, d.shadow_quality, d.vsync, d.fps_limit, d.occlusion_culling));
        changed = true;
    }

    section(ui, "Project defaults and player settings");
    ui.secondary("What is set on this page and on Display are the defaults the game ships with: what a player gets the first time. A player can change them for their own computer without the game being rebuilt, in a file of their own that wins over these, if it exists:");
    match PlayerSettings::path_for(&slug) {
        Some(path) => ui.label(RichText::new(path.display().to_string()).monospace()),
        None => ui.secondary("(this system has no settings folder)"),
    };
    ui.secondary("It can set: display_mode, width, height, vsync, fps_limit, quality (Low, Medium, High, Ultra), msaa and shadow_quality. Anything it leaves out stays as set here. The editor never reads it.");
    changed
}

// ---- icon and splash screen -------------------------------------------------------------------------------

/// The edge of the small pictures in the page.
const PREVIEW_EDGE: u32 = 128;

/// What is known about the picture the project uses for `role`: usable or why not, and a small picture of it. Read
/// again when the file changes; a few bytes of `stat` otherwise.
fn launch_preview(ctx: &egui::Context, ed: &mut Editor, role: Role, rel: &str) -> Option<(Result<launch::ImageInfo, String>, Option<egui::TextureHandle>)> {
    let project = ed.project.as_ref()?;
    let key = format!("{}:{rel}", role.label());
    let stamp = std::fs::metadata(project.root.join(rel)).ok().map(|m| (m.len(), m.modified().ok()));
    if let Some(known) = ed.project_settings_ui.launch_previews.get(&key) {
        if stamp == Some(known.stamp) {
            return Some((known.result.clone(), known.texture.clone()));
        }
    }
    let result = launch::check_project_file(project, rel, role);
    let texture = result.as_ref().ok().and_then(|_| super::assets::load_image(&project.root.join(rel), PREVIEW_EDGE)).map(|image| ctx.load_texture(format!("launch:{key}"), image, egui::TextureOptions::LINEAR));
    match stamp {
        Some(stamp) => {
            ed.project_settings_ui.launch_previews.insert(key, LaunchPreview { stamp, result: result.clone(), texture: texture.clone() });
        }
        None => {
            ed.project_settings_ui.launch_previews.remove(&key);
        }
    }
    Some((result, texture))
}

fn launch_tab(ui: &mut egui::Ui, ed: &mut Editor) -> bool {
    assert_stacks_vertically(ui);
    ui.heading("Icon & Splash Screen");
    ui.secondary("How the game presents itself when it starts. They are two separate pictures with different jobs, and both are files of the project (a picture from elsewhere is copied into Assets/Textures), so they are packed with the game and follow the file if it is moved or renamed in the Assets tab.");
    launch_section(ui, ed, Role::Icon, "Application icon", "A square PNG or JPEG, 256 x 256 or bigger. It is the picture of the game's window (the title bar and the task bar) when the game runs, from the editor with Play in Window or built. It is not written into the executable file itself.");
    launch_section(ui, ed, Role::Splash, "Splash screen", "Optional. A PNG or JPEG of any shape, shown over the whole window, on black, while the game starts: until the first scene is loaded and drawn, and for at least a moment.");
    false
}

fn launch_section(ui: &mut egui::Ui, ed: &mut Editor, role: Role, title: &str, help: &str) {
    section(ui, title);
    let current = ed.launch_image(role).map(str::to_owned);
    let preview = current.as_deref().and_then(|rel| launch_preview(ui.ctx(), ed, role, rel));
    ui.horizontal_top(|ui| {
        // The picture (or what stands in for it).
        let (rect, _) = ui.allocate_exact_size(egui::vec2(PREVIEW_EDGE as f32, PREVIEW_EDGE as f32 * if role == Role::Icon { 1.0 } else { 0.6 }), egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
        ui.painter().rect_stroke(rect, 4.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
        match preview.as_ref().and_then(|(_, texture)| texture.as_ref()) {
            Some(texture) => {
                let size = texture.size_vec2();
                let fit = (rect.width() / size.x).min(rect.height() / size.y).min(1.0);
                egui::Image::new(texture).fit_to_exact_size(size * fit).paint_at(ui, egui::Rect::from_center_size(rect.center(), size * fit));
            }
            None => {
                let text = if current.is_some() { "no picture" } else { "none" };
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(12.0), crate::theme::secondary_color(ui));
            }
        }
        ui.vertical(|ui| {
            match &current {
                Some(rel) => {
                    ui.label(RichText::new(rel).monospace());
                    if let Some((Ok(info), _)) = &preview {
                        ui.secondary(format!("{} x {} pixels", info.width, info.height));
                    }
                }
                None => {
                    ui.secondary(if role == Role::Icon { "None: the game uses the default icon." } else { "None: the game starts without a splash screen." });
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button(if current.is_some() { "Replace..." } else { "Select..." }).on_hover_text("Pick a PNG or JPEG file from anywhere on this computer").clicked() {
                    if let Some(file) = rfd::FileDialog::new().set_title(format!("Choose the game's {}", role.label())).add_filter("Image (PNG, JPEG)", &launch::IMAGE_EXTENSIONS).pick_file() {
                        ed.choose_launch_image(role, &file);
                    }
                }
                let images: Vec<String> = ed.asset_files_with_image_extension();
                ui.add_enabled_ui(!images.is_empty(), |ui| {
                    egui::ComboBox::from_id_salt(("launch_from_project", role.label())).selected_text("From the project").width(150.0).show_ui(ui, |ui| {
                        for image in &images {
                            if ui.selectable_label(current.as_deref() == Some(image.as_str()), image).clicked() {
                                ed.choose_launch_file(role, image);
                            }
                        }
                    });
                });
                if ui.add_enabled(current.is_some(), egui::Button::new("Clear")).clicked() {
                    ed.clear_launch_image(role);
                }
            });
        });
    });
    // What is wrong with the chosen picture, or what was refused just now.
    if let Some((Err(why), _)) = &preview {
        ui.colored_label(ERROR, why);
    } else if let Some((Ok(info), _)) = &preview {
        if let Some(advice) = launch::advice(*info, role) {
            ui.colored_label(NOTE, advice);
        }
    }
    if let Some((_, why)) = ed.project_settings_ui.launch_message.as_ref().filter(|(r, _)| *r == role) {
        ui.colored_label(ERROR, format!("Not changed: {why}"));
    }
    ui.secondary(help);
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
            let dir = std::env::temp_dir().join(format!("forge_projsettings_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Settings Test").expect("create project");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    /// One frame of the panel's contents, as the dock draws them.
    fn frame(ctx: &egui::Context, ed: &mut Editor) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| {
            egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| project_settings_body(ui, ed));
            save_when_idle(ctx, ed);
        });
    }

    /// A real PNG file of `w` x `h` pixels.
    fn write_png(path: &std::path::Path, w: u32, h: u32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(w, h, image::Rgba([30, 120, 200, 255])).save(path).unwrap();
    }

    #[test]
    fn every_tab_draws() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        let ctx = egui::Context::default();
        for (_, tabs) in crate::state::PROJECT_TABS {
            for tab in *tabs {
                ed.project_settings_ui.tab = *tab;
                // (The Multiplayer page is only there for a multiplayer game; the window moves off it otherwise.)
                ed.project.as_mut().unwrap().manifest.game.kind = if *tab == ProjectTab::Multiplayer { GameType::Multiplayer } else { GameType::SinglePlayer };
                for _ in 0..3 {
                    frame(&ctx, &mut ed);
                }
                assert_eq!(ed.project_settings_ui.tab, *tab);
            }
        }
    }

    #[test]
    fn changes_are_written_to_the_manifest_once_the_mouse_and_keyboard_are_idle() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        let ctx = egui::Context::default();
        frame(&ctx, &mut ed);
        ed.project.as_mut().unwrap().manifest.window.width = 999;
        ed.project.as_mut().unwrap().manifest.project.version = "9.9".into();
        ed.project_settings_ui.pending_save = true;
        frame(&ctx, &mut ed);
        assert!(!ed.project_settings_ui.pending_save);
        let saved = Project::open(&s.0).unwrap();
        assert_eq!((saved.manifest.window.width, saved.manifest.project.version.as_str()), (999, "9.9"));
        assert!(s.0.join("Project.toml").is_file());
    }

    #[test]
    fn closing_the_window_writes_what_is_still_waiting() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.open_project_settings();
        ed.project.as_mut().unwrap().manifest.project.version = "7.7".into();
        ed.project_settings_ui.pending_save = true;
        ed.close_project_settings();
        assert_eq!(Project::open(&s.0).unwrap().manifest.project.version, "7.7", "not lost by closing before the mouse was idle");
        assert!(!ed.project_settings_ui.pending_save);
    }

    #[test]
    fn the_shortcut_opens_and_closes_the_window_and_it_needs_a_project_to_show_anything() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.run_action(crate::keymap::Action::ProjectSettings);
        assert!(ed.project_settings_open());
        ed.run_action(crate::keymap::Action::ProjectSettings);
        assert!(!ed.project_settings_open());

        // Without a project the shortcut does nothing, and the body only says so.
        let mut empty = Editor::new(None);
        empty.run_action(crate::keymap::Action::ProjectSettings);
        assert!(!empty.project_settings_open());
        let ctx = egui::Context::default();
        frame(&ctx, &mut empty);
    }
    #[test]
    fn resetting_a_tab_only_touches_its_own_settings() {
        let mut w = WindowSettings { width: 500, height: 400, msaa: 8, occlusion_culling: true, title: Some("T".into()), ..WindowSettings::default() };
        reset_window(&mut w, |w, d| (w.msaa, w.occlusion_culling) = (d.msaa, d.occlusion_culling));
        assert_eq!((w.msaa, w.occlusion_culling, w.width, w.title.as_deref()), (4, false, 500, Some("T")));
    }

    // ---- game type ----

    #[test]
    fn the_multiplayer_page_is_only_there_for_a_multiplayer_game_and_its_settings_survive_switching() {
        let s = Scratch::new();
        // A project that already has multiplayer settings in its file.
        let mut project = Project::open(&s.0).unwrap();
        project.manifest.game.kind = GameType::Multiplayer;
        project.manifest.multiplayer.extra.insert("slots".into(), toml::Value::Integer(8));
        project.save_manifest().unwrap();

        let mut ed = Editor::new(Some(s.0.clone()));
        assert_eq!(ed.project.as_ref().unwrap().manifest.game_type(), GameType::Multiplayer, "restored when the project opens");
        let pages = |ed: &Editor| project_tabs(ed.project.as_ref().unwrap().manifest.game_type()).iter().flat_map(|(_, t)| t.iter().copied()).collect::<Vec<_>>();
        assert!(pages(&ed).contains(&ProjectTab::Multiplayer));

        // Single-player: the page is gone, and the window does not stay on it.
        ed.project_settings_ui.tab = ProjectTab::Multiplayer;
        ed.project.as_mut().unwrap().manifest.game.kind = GameType::SinglePlayer;
        ed.project_settings_ui.pending_save = true;
        let ctx = egui::Context::default();
        frame(&ctx, &mut ed);
        assert!(!pages(&ed).contains(&ProjectTab::Multiplayer));
        assert_eq!(ed.project_settings_ui.tab, ProjectTab::GameType, "moved to the page that decides it");
        assert!(!ed.project_settings_ui.pending_save, "and the change was written");
        let reopened = Project::open(&s.0).unwrap();
        assert_eq!(reopened.manifest.game_type(), GameType::SinglePlayer);
        assert_eq!(reopened.manifest.multiplayer.extra.get("slots"), Some(&toml::Value::Integer(8)), "nothing was lost by switching");

        // And back: it is all there.
        ed.project.as_mut().unwrap().manifest.game.kind = GameType::Multiplayer;
        ed.save_project_manifest();
        let again = Project::open(&s.0).unwrap();
        assert_eq!((again.manifest.game_type(), again.manifest.multiplayer.extra.len()), (GameType::Multiplayer, 1));
    }

    // ---- icon and splash screen ----

    #[test]
    fn a_chosen_picture_becomes_a_project_path_and_survives_closing_and_reopening() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        let elsewhere = std::env::temp_dir().join(format!("forge_launch_pick_{}", std::process::id()));
        std::fs::remove_dir_all(&elsewhere).ok();
        write_png(&elsewhere.join("My Icon.png"), 64, 64);
        write_png(&elsewhere.join("splash.png"), 320, 180);
        write_png(&s.0.join("Assets/Textures/inside.png"), 128, 128);

        // From outside the project: copied in, and the project's own path is what is kept (never the picked one).
        ed.choose_launch_image(Role::Icon, &elsewhere.join("My Icon.png"));
        assert_eq!(ed.launch_image(Role::Icon), Some("Assets/Textures/My Icon.png"));
        assert!(s.0.join("Assets/Textures/My Icon.png").is_file(), "the picture is in the project, so it is packed with the game");
        // From inside the project: used where it is.
        ed.choose_launch_image(Role::Splash, &s.0.join("Assets/Textures/inside.png"));
        assert_eq!(ed.launch_image(Role::Splash), Some("Assets/Textures/inside.png"));
        assert!(!s.0.join("Assets/Textures/inside2.png").exists(), "not copied a second time");
        // The two are separate settings: changing one leaves the other.
        ed.choose_launch_file(Role::Splash, "Assets/Textures/My Icon.png");
        assert_eq!((ed.launch_image(Role::Icon), ed.launch_image(Role::Splash)), (Some("Assets/Textures/My Icon.png"), Some("Assets/Textures/My Icon.png")));
        ed.choose_launch_image(Role::Splash, &elsewhere.join("splash.png"));
        assert_eq!(ed.launch_image(Role::Splash), Some("Assets/Textures/splash.png"));
        assert_eq!(ed.launch_image(Role::Icon), Some("Assets/Textures/My Icon.png"));

        // Written, and there again when the project is opened afresh.
        assert!(ed.project_settings_ui.pending_save);
        ed.save_project_manifest();
        let mut later = Editor::new(Some(s.0.clone()));
        assert_eq!((later.launch_image(Role::Icon), later.launch_image(Role::Splash)), (Some("Assets/Textures/My Icon.png"), Some("Assets/Textures/splash.png")));
        let text = std::fs::read_to_string(s.0.join("Project.toml")).unwrap();
        assert!(!text.contains("forge_launch_pick"), "no temporary path in the project file: {text}");

        // The page shows them (with their pictures) and clearing takes one away.
        later.project_settings_ui.tab = ProjectTab::Launch;
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut later);
        }
        let key = format!("{}:Assets/Textures/splash.png", Role::Splash.label());
        assert!(matches!(later.project_settings_ui.launch_previews.get(&key), Some(p) if p.result.is_ok() && p.texture.is_some()), "a preview was made");
        later.clear_launch_image(Role::Icon);
        assert_eq!((later.launch_image(Role::Icon), later.launch_image(Role::Splash)), (None, Some("Assets/Textures/splash.png")));
        std::fs::remove_dir_all(&elsewhere).ok();
    }

    #[test]
    fn a_picture_that_cannot_be_used_is_refused_with_the_reason_and_nothing_changes() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        let elsewhere = std::env::temp_dir().join(format!("forge_launch_bad_{}", std::process::id()));
        std::fs::remove_dir_all(&elsewhere).ok();
        write_png(&elsewhere.join("good.png"), 64, 64);
        std::fs::write(elsewhere.join("logo.bmp"), b"BM....").unwrap();
        std::fs::write(elsewhere.join("fake.png"), b"not really a png").unwrap();
        write_png(&elsewhere.join("tiny.png"), 8, 8);
        ed.choose_launch_image(Role::Icon, &elsewhere.join("good.png"));
        let chosen = ed.launch_image(Role::Icon).map(str::to_owned);
        assert!(chosen.is_some());

        for (file, expect) in [("logo.bmp", "PNG or JPEG"), ("fake.png", "not a valid"), ("tiny.png", "too small"), ("missing.png", "could not be read")] {
            ed.choose_launch_image(Role::Icon, &elsewhere.join(file));
            let (role, why) = ed.project_settings_ui.launch_message.clone().unwrap_or_else(|| panic!("{file} was accepted"));
            assert!(role == Role::Icon && why.contains(expect), "{file}: {why}");
            assert_eq!(ed.launch_image(Role::Icon).map(str::to_owned), chosen, "{file}: what was chosen before stays");
        }
        assert!(!s.0.join("Assets/Textures/logo.bmp").exists(), "a refused file is not copied into the project");
        // A file of the project that went missing, or is not an image, is shown as a problem rather than ignored.
        ed.choose_launch_file(Role::Splash, "Assets/Textures/nothing.png");
        assert!(ed.project_settings_ui.launch_message.as_ref().is_some_and(|(r, why)| *r == Role::Splash && why.contains("missing")));
        assert_eq!(ed.launch_image(Role::Splash), None);
        ed.project.as_mut().unwrap().manifest.launch.splash = Some("Assets/Textures/gone.png".into());
        ed.project_settings_ui.tab = ProjectTab::Launch;
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut ed);
        }
        let key = format!("{}:Assets/Textures/gone.png", Role::Splash.label());
        assert!(ed.project_settings_ui.launch_previews.get(&key).is_none(), "nothing is remembered about a file that is not there");
        let report = forge_engine::project::export::validate(ed.project.as_ref().unwrap());
        assert!(report.iter().any(|d| d.severity == forge_engine::project::export::Severity::Error && d.message.contains("gone.png") && d.message.contains("missing")), "a build says so: {report:?}");
        std::fs::remove_dir_all(&elsewhere).ok();
    }
}
