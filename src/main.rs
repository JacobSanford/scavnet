mod audio;
mod engine;
#[cfg(feature = "generate")]
mod generate;
mod range;
mod settings;
mod ui;
mod world;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use audio::loader::ThreadLoader;
use audio::output::{AudioOut, NullOut, RodioOut};
use engine::Engine;
use settings::Settings;
use world::progress::Progress;
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
    /// Run the scanner (default).
    Run,
    /// Load and validate the whole world, reporting every problem.
    Validate,
    /// Precompute the spectrum profile of every item into the cache directory.
    Cache,
    /// Author a conversation with OpenAI into a set (needs OPENAI_API_KEY).
    #[cfg(feature = "generate")]
    Generate(generate::GenerateArgs),
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
    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run(settings),
        Command::Validate => validate(&settings),
        Command::Cache => warm_cache(&settings),
        #[cfg(feature = "generate")]
        Command::Generate(args) => generate::run(&settings, args).map(|()| ExitCode::SUCCESS),
    }
}

fn load_world(settings: &Settings) -> Result<World> {
    Ok(World::load(&settings.networks_path(), &settings.library_path())?)
}

fn run(settings: Settings) -> Result<ExitCode> {
    // Everything that can fail on bad input fails here, before the terminal is taken over.
    let world = load_world(&settings)?;
    let progress_path = settings.paths.state_dir.join("progress.yaml");
    let mut warnings = Vec::new();
    let mut progress = Progress::load(&progress_path).unwrap_or_else(|e| {
        warnings.push(format!("Ignoring unreadable progress, starting fresh: {e:#}"));
        Progress::default()
    });
    progress.prune(&world);
    let loader = ThreadLoader::spawn(settings.paths.cache_dir.clone())?;
    let audio: Box<dyn AudioOut> = match RodioOut::open() {
        Ok(out) => Box::new(out),
        Err(e) => {
            warnings.push(format!("No audio output ({e}); running silent."));
            Box::new(NullOut::default())
        }
    };
    let redraw_hz = settings.display.redraw_hz;
    let mut engine = Engine::new(
        world,
        settings,
        progress,
        Some(progress_path),
        Box::new(loader),
        audio,
        std::time::Instant::now(),
    );
    for w in warnings {
        engine.log_info(w);
    }
    ui::run(&mut engine, redraw_hz)?;
    Ok(ExitCode::SUCCESS)
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
