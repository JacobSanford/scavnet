mod audio;
mod range;
mod settings;
mod world;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use settings::Settings;
use world::World;

/// Simulated radio scanner that airs a git-tracked library of audio stories.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Settings file. Defaults to ./Settings.toml, or built-in defaults if that is missing.
    #[arg(long, global = true)]
    settings: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Load and validate the whole world, reporting every problem.
    Validate,
    /// Precompute the spectrum profile of every item into the cache directory.
    Cache,
}

fn main() -> ExitCode {
    match run_cli() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli() -> Result<ExitCode> {
    let cli = Cli::parse();
    let settings = Settings::load(cli.settings.as_deref())?;
    match cli.command.unwrap_or(Command::Validate) {
        Command::Validate => validate(&settings),
        Command::Cache => warm_cache(&settings),
    }
}

fn load_world(settings: &Settings) -> Result<World> {
    Ok(World::load(&settings.networks_path(), &settings.library_path())?)
}

fn validate(settings: &Settings) -> Result<ExitCode> {
    match load_world(settings) {
        Ok(world) => {
            println!("OK: {}", world::describe(&world));
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprint!("{e:#}");
            Ok(ExitCode::FAILURE)
        }
    }
}

fn warm_cache(settings: &Settings) -> Result<ExitCode> {
    let world = load_world(settings)?;
    let mut paths: Vec<_> = world.items().map(|i| i.path.as_path()).collect();
    paths.sort();
    paths.dedup();
    let started = std::time::Instant::now();
    for path in &paths {
        audio::spectrum::Spectrum::load_or_analyze(path, &settings.paths.cache_dir)?;
    }
    println!("Cached {} spectrum profiles in {:.2?}", paths.len(), started.elapsed());
    Ok(ExitCode::SUCCESS)
}
