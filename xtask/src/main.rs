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
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    run_with_args(std::env::args_os(), project_root, &mut run_cargo_clean)
}

fn run_cargo_clean() -> Result<()> {
    run_cargo_clean_command(Command::new("cargo"))
}

fn run_cargo_clean_command(mut command: Command) -> Result<()> {
    command
        .arg("clean")
        .status()
        .context("Failed to run cargo clean")?;
    Ok(())
}

fn run_with_args<I, T>(
    args: I,
    project_root: &Path,
    run_cargo_clean: &mut dyn FnMut() -> Result<()>,
) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    match cli.command {
        Commands::Clean => clean_at(project_root, run_cargo_clean)?,
    }
    Ok(())
}

fn clean_at(project_root: &Path, run_cargo_clean: &mut dyn FnMut() -> Result<()>) -> Result<()> {
    println!("Running cargo clean...");
    run_cargo_clean()?;

    println!("Cleaning additional files...");
    clean_profiling_files_at(project_root)?;

    println!("Clean completed successfully!");
    Ok(())
}

fn clean_profiling_files_at(project_root: &Path) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("crabnnb-xtask-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn clean_removes_profiling_files_only_after_cargo_clean_succeeds() {
        let temp = TempDir::new();
        let nested = temp.0.join("nested");
        fs::create_dir(&nested).unwrap();
        let profraw = nested.join("run.profraw");
        let profdata = temp.0.join("merged.profdata");
        let other = temp.0.join("keep.txt");
        fs::write(&profraw, []).unwrap();
        fs::write(&profdata, []).unwrap();
        fs::write(&other, []).unwrap();
        let mut called = false;

        run_with_args(["xtask", "clean"], &temp.0, &mut || {
            called = true;
            Ok(())
        })
        .unwrap();

        assert!(called);
        assert!(!profraw.exists());
        assert!(!profdata.exists());
        assert!(other.exists());
    }

    #[test]
    fn cargo_clean_command_accepts_a_successful_command() {
        run_cargo_clean_command(Command::new("true")).unwrap();
    }

    #[test]
    fn cargo_clean_command_reports_a_missing_program() {
        let error = run_cargo_clean_command(Command::new("/definitely/not/a/cargo")).unwrap_err();
        assert!(error.to_string().contains("Failed to run cargo clean"));
    }

    #[test]
    fn run_with_args_propagates_clean_failures() {
        let temp = TempDir::new();
        let error = run_with_args(["xtask", "clean"], &temp.0, &mut || {
            anyhow::bail!("injected clean failure")
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected clean failure"));
    }

    #[test]
    fn profiling_cleanup_ignores_walk_errors() {
        let temp = TempDir::new();
        let missing = temp.0.join("removed-before-walk");
        assert!(!missing.exists());
        clean_profiling_files_at(&missing).unwrap();
    }

    #[test]
    fn clean_returns_cargo_failure_without_removing_profiles() {
        let temp = TempDir::new();
        let profraw = temp.0.join("run.profraw");
        fs::write(&profraw, []).unwrap();

        let error = clean_at(&temp.0, &mut || anyhow::bail!("cargo failed")).unwrap_err();

        assert!(error.to_string().contains("cargo failed"));
        assert!(profraw.exists());
    }

    #[test]
    fn clean_reports_profiling_file_removal_failure() {
        let temp = TempDir::new();
        let profraw_dir = temp.0.join("not-a-file.profraw");
        fs::create_dir(&profraw_dir).unwrap();

        let error = clean_at(&temp.0, &mut || Ok(())).unwrap_err();

        assert!(error.to_string().contains("Failed to remove"));
    }
}
