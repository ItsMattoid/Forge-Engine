//! The executable that ships with every exported game.
//!
//! * `forge_player`                      – runs `game.pak` next to the executable
//! * `forge_player --pak <file>`         – runs a specific package
//! * `forge_player --project <folder>`   – dev mode: runs a project folder directly
//! * `forge_player --debug [overlays]`   – adds the debug overlays: a comma-separated list such as
//!   `fps,fps_graph,physics`, or `all` (default: the FPS and its graph). Ctrl+Shift+F5 toggles the FPS.
//! * `forge_player --bench <frames>`     – plays the game for that many frames (after a warm-up), prints how long
//!   they took and exits. V-Sync is off for the run.
//! * `forge_player --screenshot <png>`   – saves what the window shows (after the `--bench` frames, if given) and exits.
//! * `forge_player --wireframe`          – draws the edges of every mesh over the scene (where the graphics API can).
//! * `forge_player --wireframe-only`     – the same without the shaded scene (for looking at what culling keeps).

use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, Context, Result};
use forge_engine::{
    assets::{GameSource, Pak, PAK_NAME},
    bench::{self, BenchPlan},
    diag::DebugSettings,
    project::{player_settings::PlayerSettings, slugify, Project, ProjectManifest, LEGACY_MANIFEST, MANIFEST},
    render::WireframeView,
    runtime,
};

const USAGE: &str = "usage: forge_player [--pak <file> | --project <folder>] [--debug [overlays]] [--bench <frames>] [--screenshot <png>] [--wireframe | --wireframe-only]";

fn exe_dir() -> PathBuf {
    env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

/// What the command line asked for: the game, the debug overlays if `--debug` was given, a benchmark run
/// if `--bench` or `--screenshot` was, and how to draw wireframes.
type Launch = (GameSource, ProjectManifest, Option<DebugSettings>, Option<BenchPlan>, WireframeFlags);

/// `--wireframe` / `--wireframe-only`: draw the edges, and whether without the shaded scene.
#[derive(Clone, Copy, Default)]
struct WireframeFlags {
    enabled: bool,
    hide_solid: bool,
}

fn load() -> Result<Option<Launch>> {
    let mut project: Option<PathBuf> = None;
    let mut pak: Option<PathBuf> = None;
    let mut debug: Option<DebugSettings> = None;
    let mut frames: Option<u32> = None;
    let mut screenshot: Option<PathBuf> = None;
    let mut wireframe = WireframeFlags::default();

    let mut args = env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project = Some(args.next().context("--project needs a folder")?.into()),
            "--pak" => pak = Some(args.next().context("--pak needs a file")?.into()),
            "--debug" => {
                // An optional list follows (anything that is not another option).
                let list = args.next_if(|next| !next.starts_with("--")).unwrap_or_default();
                debug = Some(DebugSettings::from_list(&list).map_err(|e| anyhow::anyhow!("{e}"))?);
            }
            "--bench" => frames = Some(args.next().context("--bench needs a number of frames")?.parse().context("--bench needs a number of frames")?),
            "--screenshot" => screenshot = Some(args.next().context("--screenshot needs a file name")?.into()),
            "--wireframe" => wireframe = WireframeFlags { enabled: true, hide_solid: false },
            "--wireframe-only" => wireframe = WireframeFlags { enabled: true, hide_solid: true },
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => bail!("unknown argument '{other}'\n{USAGE}"),
        }
    }
    let bench = (frames.is_some() || screenshot.is_some()).then(|| BenchPlan { frames: frames.unwrap_or(0), screenshot });

    if let Some(dir) = project {
        let project = Project::open(&dir)?;
        return Ok(Some((GameSource::Dir(project.root.clone()), project.manifest, debug, bench, wireframe)));
    }

    let path = pak.unwrap_or_else(|| exe_dir().join(PAK_NAME));
    let pak = Pak::open(&path).with_context(|| format!("could not open the game package ({})", path.display()))?;
    let manifest = ProjectManifest::parse(pak.get(MANIFEST).or_else(|| pak.get(LEGACY_MANIFEST)).context("the package has no manifest")?)?;
    Ok(Some((GameSource::Pak(Arc::new(pak)), manifest, debug, bench, wireframe)))
}

/// The project's settings with the player's own choices over them (see project::player_settings). A benchmark
/// run measures the project as it ships, so it ignores them.
fn with_player_settings(mut manifest: ProjectManifest, benchmark: bool) -> ProjectManifest {
    if !benchmark {
        if let Some(settings) = PlayerSettings::load(&slugify(&manifest.project.name)) {
            settings.apply_to(&mut manifest.window);
        }
    }
    manifest
}

fn main() {
    match load() {
        Ok(Some((source, manifest, debug, plan, wireframe))) => {
            let manifest = with_player_settings(manifest, plan.is_some());
            let mut app = runtime::build_app_with_debug(source, manifest, debug);
            let mut view = app.world_mut().resource_mut::<WireframeView>();
            (view.enabled, view.hide_solid) = (wireframe.enabled, wireframe.hide_solid);
            if let Some(plan) = plan {
                bench::add_benchmark(&mut app, plan);
            }
            app.run();
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}
