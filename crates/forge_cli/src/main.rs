use std::{
    path::PathBuf,
    process::{Command, ExitCode},
};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use forge_engine::project::{
    export::{self, BuildOptions, Severity},
    launcher, Project,
};

#[derive(Parser)]
#[command(name = "forge", version, about = "Forge game engine: create, check, run and build projects")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new project from a template
    New {
        /// Folder to create the project in
        path: PathBuf,
        /// Game name (defaults to the folder name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Validate scenes and scripts without running the game
    Check { project: PathBuf },
    /// Run a project directly from its folder (dev mode)
    Run { project: PathBuf },
    /// Open the editor, optionally on a project
    Edit { project: Option<PathBuf> },
    /// Package a project as a standalone game
    Build {
        project: PathBuf,
        /// Output directory (default: <project>/build)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Player executable to bundle (default: the one next to this binary).
        /// Use this to ship a build made with `--target` or `--features jit`.
        #[arg(long)]
        player: Option<PathBuf>,
    },
}

fn run() -> Result<ExitCode> {
    match Cli::parse().command {
        Cmd::New { path, name } => {
            let name = name.unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "My Game".into()));
            let project = Project::create(&path, &name)?;
            println!("Created project '{}' in {}", project.name(), path.display());
            println!("  forge edit {}    # open the editor", path.display());
            println!("  forge run {}     # play it", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Check { project } => {
            let project = Project::open(&project)?;
            let diagnostics = export::validate(&project);
            for d in &diagnostics {
                println!("{}: {}", if d.severity == Severity::Error { "error" } else { "warning" }, d.message);
            }
            if export::has_errors(&diagnostics) {
                Ok(ExitCode::FAILURE)
            } else {
                println!("OK ({} scenes, {} scripts)", project.scenes().len(), project.scripts().len());
                Ok(ExitCode::SUCCESS)
            }
        }
        Cmd::Run { project } => {
            let project = Project::open(&project)?;
            let status = launcher::spawn_player(&project.root, false)?.wait()?;
            Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
        }
        Cmd::Edit { project } => {
            let editor = launcher::editor_path()?;
            let mut cmd = Command::new(editor);
            if let Some(p) = project {
                cmd.arg(p);
            }
            let status = cmd.status().context("starting the editor")?;
            Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
        }
        Cmd::Build { project, out, player } => {
            let project = Project::open(&project)?;
            let player = match player {
                Some(p) => p,
                None => launcher::player_path()?,
            };
            let out_dir = out.unwrap_or_else(|| export::default_out_dir(&project.root));
            let report = match export::build(&project, &BuildOptions { out_dir, player }, &mut |line| println!("{line}")) {
                Ok(r) => r,
                Err(e) => bail!("{e:#}"),
            };
            println!("\nBuild complete: {}", report.dir.display());
            println!("  run it with: {}", report.exe.display());
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
