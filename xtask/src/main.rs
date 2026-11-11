use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Custom build commands for crabnnb", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Clean build artifacts and profiling files
    Clean,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Clean => clean()?,
    }

    Ok(())
}

fn clean() -> Result<()> {
    // Run cargo clean
    println!("Running cargo clean...");
    Command::new("cargo")
        .arg("clean")
        .status()
        .context("Failed to run cargo clean")?;

    // Clean additional files
    println!("Cleaning additional files...");
    clean_profiling_files()?;

    println!("Clean completed successfully!");
    Ok(())
}

fn clean_profiling_files() -> Result<()> {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    // Clean profiling files in the main project
    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext == "profraw" || ext == "profdata" {
                println!("Removing: {}", path.display());
                std::fs::remove_file(path)
                    .with_context(|| format!("Failed to remove {}", path.display()))?;
            }
        }
    }

    Ok(())
}
