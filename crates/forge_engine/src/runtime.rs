//! The game runtime: loads scenes into the data model, drives the Luau host and
//! mirrors the result into Bevy.
//!
//! Physics steps at a fixed rate in `FixedUpdate` (before `Update`): `physics_pre_step` brings the world up to
//! date with the data model, `bevy_box3d` steps it, `physics_post_step` writes the results back (see `physics`).
//!
//! Frame order (all in `Update`, chained):
//! 1. `load_pending_scene` – replace the data model's contents when a scene was requested
//! 2. `run_frame`          – input and scripts (see `scripting`)
//! 3. `render_sync`        – mirror changed instances into Bevy entities
//! 4. `apply_requests`     – quit / scene switch / mouse capture requested by scripts

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use bevy::{
    asset::RenderAssetUsages,
    input::mouse::{MouseMotion, MouseWheel},
    log::LogPlugin,
    light::{DirectionalLightShadowMap, PointLightShadowMap},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::UiTargetCamera,
    window::{CursorGrabMode, CursorOptions, MonitorSelection, PresentMode, PrimaryWindow, VideoModeSelection, WindowMode},
    winit::WinitWindows,
};

use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use crate::{
    assets::{clean_path, info::AssetInfo, GameSource},
    audio::{AudioEngine, SoundMirror},
    compat::{self, ExitWriter},
    diag::{self, DebugSettings, DiagPlugin, Metrics, Overlay},
    dm::{file::PlaceFile, DataModel},
    physics::{Box3dSet, Box3dStats, Box3dWorld, ForgePhysicsPlugin},
    project::{
        launch::{self, Role},
        DisplayMode, ProjectManifest,
    },
    render::{self, msaa_from_samples, AssetCache, Culling, Mode, PartMaterialPlugin, SpawnParams, SyncOptions},
    scripting::{FrameInput, ScriptHost},
};

#[derive(Resource, Clone)]
pub struct Game {
    pub source: GameSource,
    pub manifest: ProjectManifest,
}

#[derive(Resource)]
struct PendingScene(Option<String>);

pub struct ForgeRuntimePlugin {
    pub source: GameSource,
    pub manifest: ProjectManifest,
}

impl Plugin for ForgeRuntimePlugin {
    fn build(&self, app: &mut App) {
        let dm = Rc::new(RefCell::new(DataModel::new()));
        // What the data model can ask about the project's files (a model's size, a sound's length).
        let info = Rc::new(AssetInfo::new(self.source.clone()));
        info.set_unit_scale(self.manifest.assets.mesh_scale());
        dm.borrow_mut().set_asset_info(Some(info));
        let host = ScriptHost::new(self.source.clone(), dm).expect("failed to start the Luau VM");
        app.insert_resource(Game { source: self.source.clone(), manifest: self.manifest.clone() })
            .insert_resource(PendingScene(Some(self.manifest.project.start_scene.clone())))
            .init_resource::<AssetCache>()
            // How parts are drawn: one material per surface kind, colour per instance (see `render::part_material`).
            .add_plugins(PartMaterialPlugin)
            // The physics world and its fixed-rate step; the two systems keep the data model and the world in step.
            .add_plugins(ForgePhysicsPlugin)
            .insert_non_send(host)
            // Sound: the mixer and the mirror that plays the scene's Sound objects.
            .insert_non_send(AudioEngine::new())
            .insert_non_send(SoundMirror::new())
            .add_systems(FixedUpdate, (physics_pre_step.in_set(Box3dSet::Sync), physics_post_step.after(Box3dSet::Step)))
            .add_systems(Update, (load_pending_scene, run_frame, audio_sync, render_sync, apply_requests).chain());
        app.world_mut().resource_mut::<Culling>().occlusion = self.manifest.window.occlusion_culling;
        app.world_mut().resource_mut::<AssetCache>().mesh_scale = self.manifest.assets.mesh_scale();


        // The graphics settings of the project (or of the player, see `project::player_settings`).
        let w = &self.manifest.window;
        let shadow_size = w.shadow_quality.map_size();
        if shadow_size > 0 {
            app.insert_resource(DirectionalLightShadowMap { size: shadow_size }).insert_resource(PointLightShadowMap { size: shadow_size / 2 });
        } else {
            app.add_systems(Update, disable_shadows.after(render_sync));
        }
        if w.fps_limit > 0 {
            app.insert_resource(FrameLimit(Duration::from_secs_f64(1.0 / f64::from(w.fps_limit)))).add_systems(Last, limit_frame_rate);
        }
        // How the game presents itself while it starts (see `project::launch`).
        if self.manifest.launch.icon.is_some() {
            app.add_systems(Update, apply_window_icon);
        }
        if self.manifest.launch.splash.is_some() {
            app.add_systems(Startup, show_splash).add_systems(Update, hide_splash.after(render_sync));
        }
    }
}

// ---- graphics settings ---------------------------------------------------------------------------------

/// `ShadowQuality::Off`: no light casts a shadow, whatever it asks for. (Runs after the scene was mirrored into Bevy,
/// every frame, and only writes when a light was about to cast one, so it costs nothing in a game without shadows.)
fn disable_shadows(mut sun: Query<&mut DirectionalLight>, mut points: Query<&mut PointLight>, mut spots: Query<&mut SpotLight>) {
    for mut light in &mut sun {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
    for mut light in &mut points {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
    for mut light in &mut spots {
        if light.shadow_maps_enabled {
            light.shadow_maps_enabled = false;
        }
    }
}

/// The time one frame may take under the project's frame-rate limit.
#[derive(Resource)]
struct FrameLimit(Duration);

/// The frame limiter: runs at the very end of the frame and waits until one budget after the previous frame ended
/// (sleeping most of the time, since a sleep wakes up late, and spinning the rest). A frame that ran long is not
/// made up for.
fn limit_frame_rate(limit: Res<FrameLimit>, mut previous_end: Local<Option<Instant>>) {
    const SLACK: Duration = Duration::from_micros(1500);
    if let Some(end) = *previous_end {
        let target = end + limit.0;
        if let Some(left) = target.checked_duration_since(Instant::now()) {
            if left > SLACK {
                std::thread::sleep(left - SLACK);
            }
        }
        while Instant::now() < target {
            std::hint::spin_loop();
        }
    }
    *previous_end = Some(Instant::now());
}

// ---- icon and splash screen ----------------------------------------------------------------------------

/// Gives the game's window the icon the project chose. Waits for the window to exist (the first frames), tries once,
/// and says in the log why when the file cannot be used - the game starts with the default icon then.
fn apply_window_icon(game: Res<Game>, windows: NonSend<WinitWindows>, primary: Query<Entity, With<PrimaryWindow>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Ok(entity) = primary.single() else { return };
    let Some(window) = windows.get_window(entity) else { return };
    *done = true;
    let Some(rel) = game.manifest.launch.icon.as_deref() else { return };
    let icon = game
        .source
        .read(rel)
        .map_err(|e| format!("{e:#}"))
        .and_then(|bytes| {
            launch::check_image(rel, &bytes, Role::Icon)?;
            launch::decode_icon(&bytes)
        })
        .and_then(|(w, h, rgba)| winit::window::Icon::from_rgba(rgba, w, h).map_err(|e| e.to_string()));
    match icon {
        Ok(icon) => window.set_window_icon(Some(icon)),
        Err(e) => warn!("the game's icon '{rel}' could not be used: {e}"),
    }
}

/// The splash screen is up: the picture on black, over the whole window, until the game has had time to load.
#[derive(Resource)]
struct Splash {
    started: Instant,
    /// Frames drawn since the start scene was loaded.
    frames: u32,
}

#[derive(Component)]
struct SplashPart;

/// The splash screen stays at least this long (so it can be read, and does not flash) ...
const SPLASH_MIN: Duration = Duration::from_millis(1500);
/// ... and until the scene has been drawn for this many frames (the first ones are the slow ones).
const SPLASH_FRAMES: u32 = 10;

fn show_splash(mut commands: Commands, game: Res<Game>, mut images: ResMut<Assets<Image>>) {
    let Some(rel) = game.manifest.launch.splash.as_deref() else { return };
    let decoded = game.source.read(rel).map_err(|e| format!("{e:#}")).and_then(|bytes| {
        launch::check_image(rel, &bytes, Role::Splash)?;
        launch::decode_rgba(&bytes)
    });
    let (w, h, rgba) = match decoded {
        Ok(image) => image,
        Err(e) => return warn!("the splash screen '{rel}' could not be used: {e}"),
    };
    let image = images.add(Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD));
    // Its own camera, drawn after the game's and clearing to black, so nothing of the game shows behind it.
    let camera = commands.spawn((Camera2d, Camera { order: 1000, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() }, SplashPart)).id();
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::BLACK),
            UiTargetCamera(camera),
            GlobalZIndex(i32::MAX),
            SplashPart,
        ))
        .with_children(|root| {
            root.spawn((ImageNode::new(image), Node { height: Val::Percent(70.0), aspect_ratio: Some(w as f32 / h as f32), ..default() }));
        });
    commands.insert_resource(Splash { started: Instant::now(), frames: 0 });
}

fn hide_splash(mut commands: Commands, pending: Res<PendingScene>, splash: Option<ResMut<Splash>>, parts: Query<Entity, With<SplashPart>>) {
    let Some(mut splash) = splash else { return };
    if pending.0.is_some() {
        return; // the start scene is not loaded yet
    }
    splash.frames += 1;
    if splash.frames >= SPLASH_FRAMES && splash.started.elapsed() >= SPLASH_MIN {
        for part in &parts {
            commands.entity(part).despawn();
        }
        commands.remove_resource::<Splash>();
    }
}

/// The debug overlays of a running game (`forge_player --debug`): the same Debug menu, panels and
/// shortcut (`Ctrl+Shift+F5` toggles the FPS) as the editor's, drawn over the game window. Games that
/// are not started with `--debug` do not even load the UI library for it.
#[derive(Resource)]
pub struct RuntimeDebug {
    pub settings: DebugSettings,
}

/// Adds the debug overlays to a game app. Call after [`ForgeRuntimePlugin`] and the default plugins.
pub fn add_debug_overlays(app: &mut App, settings: DebugSettings) {
    app.add_plugins(EguiPlugin::default())
        .add_plugins(DiagPlugin { gpu_timing: true })
        .insert_resource(RuntimeDebug { settings })
        .add_systems(Update, feed_debug.after(render_sync))
        .add_systems(EguiPrimaryContextPass, draw_debug);
}

/// Tells the collectors what the settings ask for and hands them what the script host did.
fn feed_debug(host: NonSend<ScriptHost>, debug: Res<RuntimeDebug>, mut metrics: ResMut<Metrics>) {
    metrics.apply(&debug.settings);
    let wants = metrics.wants;
    host.set_profiling(wants.script_costs);
    if std::mem::take(&mut metrics.clear_requested) {
        host.clear_profile();
    }
    if metrics.timings.enabled {
        let (scripts_ms, physics_ms) = host.frame_ms();
        metrics.record_script_frame(scripts_ms, physics_ms);
    }
    if metrics.sampling {
        if wants.instances {
            let dm = host.dm();
            let dm = dm.borrow();
            metrics.instances = Some((dm.instance_count() as u32, dm.component_count("ScriptRunner") as u32));
        }
        metrics.scripts = wants.scripts.then(|| host.profile());
        metrics.physics = wants.physics.then(|| host.physics_stats());
    }
}

/// The only keyboard shortcut a game has: Ctrl+Shift+F5 toggles the FPS (the editor makes all of them rebindable).
fn draw_debug(mut contexts: EguiContexts, mut debug: ResMut<RuntimeDebug>, mut metrics: ResMut<Metrics>, keys: Res<ButtonInput<KeyCode>>) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if ctrl && shift && keys.just_pressed(KeyCode::F5) {
        debug.settings.toggle(Overlay::Fps);
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let viewport = ctx.content_rect();
    let hint = |o: Overlay| (o == Overlay::Fps).then(|| "Ctrl+Shift+F5".to_owned());
    if diag::ui::floating_menu(ctx, viewport, &mut debug.settings, &hint).clear_data {
        metrics.clear();
    }
    diag::ui::overlays(ctx, viewport, &mut debug.settings, &metrics);
}

/// Builds a ready-to-run app for a game (window, renderer, runtime plugin).
pub fn build_app(source: GameSource, manifest: ProjectManifest) -> App {
    build_app_with_debug(source, manifest, None)
}

/// Like [`build_app`], with the debug overlays (the Debug menu, `Ctrl+Shift+F5` for the FPS) when `debug` is given.
pub fn build_app_with_debug(source: GameSource, manifest: ProjectManifest, debug: Option<DebugSettings>) -> App {
    let w = &manifest.window;
    let window = Window {
        title: manifest.window_title().to_owned(),
        resolution: (w.width, w.height).into(),
        present_mode: if w.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync },
        resizable: w.resizable,
        mode: match w.display_mode {
            DisplayMode::Windowed => WindowMode::Windowed,
            DisplayMode::Borderless => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
            DisplayMode::Fullscreen => WindowMode::Fullscreen(MonitorSelection::Primary, VideoModeSelection::Current),
        },
        ..default()
    };

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin { primary_window: Some(window), ..default() })
            // (The engine mixes its own sounds, see udio; Bevy''s audio plugin would only open a second output.)
            .disable::<bevy::audio::AudioPlugin>()
            .set(LogPlugin { filter: "wgpu=error,naga=warn,bevy_render=info,bevy_app=info".into(), custom_layer: diag::log::layer, ..default() }),
    )
    .add_plugins(ForgeRuntimePlugin { source, manifest });
    if let Some(settings) = debug {
        add_debug_overlays(&mut app, settings);
    }
    app
}

fn load_pending_scene(game: Res<Game>, mut pending: ResMut<PendingScene>, mut host: NonSendMut<ScriptHost>) {
    let Some(path) = pending.0.take() else { return };
    let path = game.source.resolve_scene(&path);
    let loaded: anyhow::Result<PlaceFile> =
        clean_path(&path).and_then(|p| game.source.read(&p)).and_then(|bytes| Ok(serde_json::from_slice::<PlaceFile>(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes))?));
    let place = match loaded {
        Ok(p) => p,
        Err(e) => {
            error!("could not load scene '{path}': {e:#}");
            return;
        }
    };
    match host.load_place(&place) {
        Ok(()) => info!("loaded scene '{path}'"),
        Err(e) => error!("could not load scene '{path}': {e:#}"),
    }
}

/// Before each physics step: edits made to the data model since the last one are applied to the world.
fn physics_pre_step(host: NonSend<ScriptHost>, physics: NonSend<Box3dWorld>, time: Res<Time<Fixed>>) {
    host.physics_pre_step(physics.world(), time.delta_secs());
}

/// After each physics step: where things moved to goes back into the data model, touches become events.
fn physics_post_step(host: NonSend<ScriptHost>, physics: NonSend<Box3dWorld>, stats: Res<Box3dStats>) {
    host.physics_post_step(physics.world(), stats.step_ms as f32);
}

#[allow(clippy::too_many_arguments)]
fn run_frame(
    mut host: NonSendMut<ScriptHost>,
    physics: NonSend<Box3dWorld>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let mouse_delta = motion.read().fold(Vec2::ZERO, |acc, e| acc + e.delta);
    let wheel = wheel.read().map(|e| e.y).sum::<f32>();
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    if let Ok(w) = windows.single() {
        host.set_viewport(Vec2::new(w.width(), w.height()));
    }
    host.frame_with(&FrameInput { dt: time.delta_secs(), keys: &keys, mouse: &mouse, mouse_delta, cursor, wheel }, Some(physics.world()));
}

/// Plays the scene''s sounds (see udio::sync).
fn audio_sync(host: NonSend<ScriptHost>, mut engine: NonSendMut<AudioEngine>, mut mirror: NonSendMut<SoundMirror>, time: Res<Time>) {
    let dm = host.dm();
    let mut dm = dm.borrow_mut();
    engine.update(time.delta_secs());
    mirror.update(&mut dm, &mut engine, time.delta_secs(), true);
}

fn render_sync(host: NonSend<ScriptHost>, game: Res<Game>, time: Res<Time>, mut commands: Commands, mut spawn: SpawnParams, mut metrics: Option<ResMut<Metrics>>) {
    let dm = host.dm();
    let mut dm = dm.borrow_mut();
    // (The camera is where the workspace's current camera is: effects that face it use it.)
    let camera = dm.reference(dm.workspace, "CurrentCamera").and_then(|c| dm.world_cframe(c)).map(|(p, _)| p);
    let opts = SyncOptions { mode: Mode::Game, msaa: msaa_from_samples(game.manifest.window.msaa), dt: time.delta_secs(), camera };
    diag::timed(metrics.as_deref_mut(), diag::timing::SCENE_SYNC, || render::sync(&mut dm, &mut commands, &mut spawn, opts, &game.source));
}

fn apply_requests(host: NonSend<ScriptHost>, mut pending: ResMut<PendingScene>, mut windows: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>, mut exit: ExitWriter) {
    let requests = host.take_requests();
    if requests.quit {
        compat::request_exit(&mut exit);
    }
    if let Some(place) = requests.place {
        pending.0 = Some(place);
    }

    // `UserInputService.MouseBehavior` decides whether the mouse is captured.
    let locked = matches!(host.mouse_behavior().as_str(), "LockCenter" | "LockCurrentPosition");
    if let Ok((w, mut cursor)) = windows.single_mut() {
        let want = locked && w.focused;
        let mode = if want { CursorGrabMode::Locked } else { CursorGrabMode::None };
        if cursor.grab_mode != mode {
            cursor.grab_mode = mode;
        }
        if cursor.visible == want {
            cursor.visible = !want;
        }
    }
}
