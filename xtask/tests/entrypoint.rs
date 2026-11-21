use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "crabnnb-xtask-empty-path-{}-{id}",
            std::process::id()
        ));
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
fn iq_vectors_entrypoint_writes_generated_source() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("iq-vectors")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(
        "SHA256_EMPTY = \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\""
    ));
    assert!(stdout.contains("HEARTBEAT = \""));
}

#[test]
fn clean_entrypoint_reports_missing_cargo_before_touching_profiles() {
    let empty_path = TempDir::new();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("clean")
        .env("PATH", &empty_path.0)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("Running cargo clean..."));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("Failed to run cargo clean"));
}
