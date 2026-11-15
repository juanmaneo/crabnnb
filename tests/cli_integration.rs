use std::process::Command;

#[test]
fn test_help_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_crabnnb"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("crabnnb <btaddr>"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_no_args_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_crabnnb"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing bluetooth MAC address") || stderr.contains("Usage:"));
}

#[test]
fn test_invalid_mac_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_crabnnb"))
        .arg("INVALID")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid bluetooth address"));
}

#[test]
fn test_unknown_option_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_crabnnb"))
        .args(["AA:BB:CC:DD:EE:FF", "--bogus"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown option"));
}

#[test]
fn test_help_mentions_secure_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_crabnnb"))
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--key-file"));
    assert!(stdout.contains("--gen-key-file"));
    assert!(stdout.contains("--allow-insecure"));
    assert!(stdout.contains("--heartbeat-handle"));
}

#[test]
fn test_secure_mode_required_by_default() {
    let output = Command::new(env!("CARGO_BIN_EXE_crabnnb"))
        .args(["AA:BB:CC:DD:EE:FF", "--once", "--dry-run"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--key-file"));
}

#[test]
fn test_gen_key_file() {
    let dir = std::env::temp_dir().join(format!("crabnnb-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("key.hex");
    let output = Command::new(env!("CARGO_BIN_EXE_crabnnb"))
        .arg("--gen-key-file")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content.trim().len(), 64);
    assert!(content.trim().chars().all(|c| c.is_ascii_hexdigit()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let again = Command::new(env!("CARGO_BIN_EXE_crabnnb"))
        .arg("--gen-key-file")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!again.status.success());
    std::fs::remove_dir_all(&dir).ok();
}
