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
