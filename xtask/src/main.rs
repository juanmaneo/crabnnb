use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::io::{self, Write};
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
    /// Generate Rust-derived Monkey C vectors for the Connect IQ simulator tests
    IqVectors,
    /// Remove generated root-level crabnnb-<pid> Unix socket files
    CleanLeftover,
}

fn main() -> Result<()> {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut output = io::stdout().lock();
    let mut run_vectors = || run_iq_vectors(&mut output);
    run_with_args(
        std::env::args_os(),
        project_root,
        &mut run_cargo_clean,
        &mut run_vectors,
    )
}

fn run_cargo_clean() -> Result<()> {
    run_cargo_clean_command(Command::new("cargo"))
}

fn run_iq_vectors(out: &mut dyn Write) -> Result<()> {
    cmd_iq_vectors(out)
}

fn run_cargo_clean_command(mut command: Command) -> Result<()> {
    let status = command
        .arg("clean")
        .status()
        .context("Failed to run cargo clean")?;
    anyhow::ensure!(status.success(), "cargo clean exited with status {status}");
    Ok(())
}

fn run_with_args<I, T>(
    args: I,
    project_root: &Path,
    run_cargo_clean: &mut dyn FnMut() -> Result<()>,
    run_vectors: &mut dyn FnMut() -> Result<()>,
) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    match cli.command {
        Commands::Clean => clean_at(project_root, run_cargo_clean)?,
        Commands::IqVectors => run_vectors()?,
        Commands::CleanLeftover => {
            clean_leftover_files_at(project_root)?;
        }
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

struct LeftoverEntry {
    path: std::path::PathBuf,
    name: std::ffi::OsString,
    file_type: std::io::Result<std::fs::FileType>,
}

fn clean_leftover_files_at(project_root: &Path) -> Result<usize> {
    let entries: Vec<std::io::Result<LeftoverEntry>> = std::fs::read_dir(project_root)
        .with_context(|| format!("Failed to read project root {}", project_root.display()))?
        .map(|entry| {
            entry.map(|entry| LeftoverEntry {
                path: entry.path(),
                name: entry.file_name(),
                file_type: entry.file_type(),
            })
        })
        .collect();
    clean_leftover_entries_at(project_root, entries, remove_path)
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

fn clean_leftover_entries_at(
    project_root: &Path,
    entries: Vec<std::io::Result<LeftoverEntry>>,
    remove_file: fn(&Path) -> std::io::Result<()>,
) -> Result<usize> {
    let mut removed = 0;

    for entry in entries {
        let entry = entry
            .with_context(|| format!("Failed to read an entry in {}", project_root.display()))?;
        let file_type = entry
            .file_type
            .with_context(|| format!("Failed to inspect {}", entry.path.display()))?;

        if is_socket(&file_type) && is_leftover_socket_name(&entry.name) {
            println!("Removing leftover socket: {}", entry.path.display());
            remove_file(&entry.path)
                .with_context(|| format!("Failed to remove {}", entry.path.display()))?;
            removed += 1;
        }
    }

    println!("Removed {removed} leftover socket(s).");
    Ok(removed)
}

#[cfg(unix)]
fn is_socket(file_type: &std::fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;
    file_type.is_socket()
}

#[cfg(not(unix))]
fn is_socket(_file_type: &std::fs::FileType) -> bool {
    false
}

fn is_leftover_socket_name(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix("crabnnb-"))
        .is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn iq_vectors_source() -> String {
    let key: [u8; 32] = std::array::from_fn(|i| i as u8);
    let challenge: [u8; 16] = std::array::from_fn(|i| 0xa0 + i as u8);
    let counter = 0x1234_5678_u32;
    let timestamp = 0x65ab_cdef_u32;
    let multi_block: Vec<u8> = (0..100).map(|i| i as u8).collect();
    let hmac_message = b"The quick brown fox jumps over the lazy dog";
    let long_hmac_key = vec![0xaa; 131];
    let aes_iv: [u8; 16] = std::array::from_fn(|i| 0x10 + i as u8);
    let aes_plaintext: Vec<u8> = (0..32).map(|i| i as u8).collect();

    let derived_mac_key = crabnnb::heartbeat::mac_key(&key);
    let mut heartbeat_plaintext = Vec::with_capacity(24);
    heartbeat_plaintext.extend_from_slice(&counter.to_le_bytes());
    heartbeat_plaintext.extend_from_slice(&timestamp.to_le_bytes());
    heartbeat_plaintext.extend_from_slice(&challenge);
    // The watch derives its deterministic IV from HMAC(macKey, plaintext).
    let heartbeat_iv: [u8; 16] =
        crabnnb::crypto::hmac_sha256(&derived_mac_key, &heartbeat_plaintext)[..16]
            .try_into()
            .expect("the IV prefix is 16 bytes");
    let heartbeat = crabnnb::heartbeat::seal_heartbeat_bridge(
        &key,
        counter,
        timestamp,
        &challenge,
        &heartbeat_iv,
    );

    let vectors = [
        ("KEY", crabnnb::crypto::to_hex(&key)),
        ("CHALLENGE", crabnnb::crypto::to_hex(&challenge)),
        ("COUNTER", format!("{counter:08x}")),
        ("TIMESTAMP", format!("{timestamp:08x}")),
        (
            "SHA256_EMPTY",
            crabnnb::crypto::to_hex(&crabnnb::crypto::sha256(b"")),
        ),
        (
            "SHA256_ABC",
            crabnnb::crypto::to_hex(&crabnnb::crypto::sha256(b"abc")),
        ),
        (
            "SHA256_MULTI_BLOCK",
            crabnnb::crypto::to_hex(&crabnnb::crypto::sha256(&multi_block)),
        ),
        (
            "HMAC_SHORT_KEY",
            crabnnb::crypto::to_hex(&crabnnb::crypto::hmac_sha256(b"key", hmac_message)),
        ),
        (
            "HMAC_LONG_KEY",
            crabnnb::crypto::to_hex(&crabnnb::crypto::hmac_sha256(&long_hmac_key, hmac_message)),
        ),
        ("AES_KEY", crabnnb::crypto::to_hex(&key)),
        ("AES_IV", crabnnb::crypto::to_hex(&aes_iv)),
        ("AES_PLAINTEXT", crabnnb::crypto::to_hex(&aes_plaintext)),
        (
            "AES_CIPHERTEXT",
            crabnnb::crypto::to_hex(&crabnnb::crypto::aes256_cbc_encrypt_raw(
                &key,
                &aes_iv,
                &aes_plaintext,
            )),
        ),
        ("HEARTBEAT", crabnnb::crypto::to_hex(&heartbeat)),
    ];

    let mut source =
        String::from("// Generated by `cargo xtask iq-vectors`; do not edit.\nclass IqVectors {\n");
    for (name, value) in vectors {
        source.push_str(&format!("    static const {name} = \"{value}\";\n"));
    }
    source.push_str("}\n");
    source
}

fn cmd_iq_vectors(out: &mut dyn Write) -> Result<()> {
    out.write_all(iq_vectors_source().as_bytes())
        .context("Failed to write Connect IQ vectors")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
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

    fn no_op() -> Result<()> {
        Ok(())
    }

    fn fail_remove(_path: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "injected remove error",
        ))
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::Other, "write failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
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

        run_with_args(
            ["xtask", "clean"],
            &temp.0,
            &mut || {
                called = true;
                Ok(())
            },
            &mut no_op,
        )
        .unwrap();

        assert!(called);
        assert!(!profraw.exists());
        assert!(!profdata.exists());
        assert!(other.exists());
    }

    #[cfg(unix)]
    #[test]
    fn clean_leftover_removes_only_matching_socket_files() {
        use std::os::unix::net::UnixListener;

        let temp = TempDir::new();
        let matching_socket = temp.0.join("crabnnb-132037");
        let nonmatching_socket = temp.0.join("crabnnb-12x");
        let regular_file = temp.0.join("crabnnb-456");
        let nested = temp.0.join("nested");
        fs::create_dir(&nested).unwrap();
        let nested_socket = nested.join("crabnnb-789");
        let _matching_listener = UnixListener::bind(&matching_socket).unwrap();
        let _nonmatching_listener = UnixListener::bind(&nonmatching_socket).unwrap();
        let _nested_listener = UnixListener::bind(&nested_socket).unwrap();
        fs::write(&regular_file, "keep").unwrap();

        let removed = clean_leftover_files_at(&temp.0).unwrap();

        assert_eq!(removed, 1);
        assert!(!matching_socket.exists());
        assert!(nonmatching_socket.exists());
        assert!(regular_file.exists());
        assert!(nested_socket.exists());
    }

    #[test]
    fn clean_leftover_reports_missing_project_root() {
        let temp = TempDir::new();
        let missing = temp.0.join("missing");

        let error = clean_leftover_files_at(&missing).unwrap_err();

        assert!(error.to_string().contains("Failed to read project root"));
    }

    #[test]
    fn clean_leftover_reports_directory_entry_errors() {
        let temp = TempDir::new();
        let entries = vec![Err(io::Error::new(
            io::ErrorKind::Other,
            "injected read error",
        ))];

        let error = clean_leftover_entries_at(&temp.0, entries, remove_path).unwrap_err();

        assert!(error.to_string().contains("Failed to read an entry"));
    }

    #[test]
    fn clean_leftover_reports_file_type_errors() {
        let temp = TempDir::new();
        let path = temp.0.join("crabnnb-132037");
        let entry = LeftoverEntry {
            path,
            name: std::ffi::OsString::from("crabnnb-132037"),
            file_type: Err(io::Error::new(
                io::ErrorKind::Other,
                "injected metadata error",
            )),
        };

        let error = clean_leftover_entries_at(&temp.0, vec![Ok(entry)], remove_path).unwrap_err();

        assert!(error.to_string().contains("Failed to inspect"));
    }

    #[cfg(unix)]
    #[test]
    fn clean_leftover_reports_socket_removal_errors() {
        use std::os::unix::net::UnixListener;

        let temp = TempDir::new();
        let path = temp.0.join("crabnnb-132037");
        let _listener = UnixListener::bind(&path).unwrap();
        let entry = LeftoverEntry {
            path: path.clone(),
            name: std::ffi::OsString::from("crabnnb-132037"),
            file_type: fs::symlink_metadata(&path).map(|metadata| metadata.file_type()),
        };

        let error = clean_leftover_entries_at(&temp.0, vec![Ok(entry)], fail_remove).unwrap_err();

        assert!(error.to_string().contains("Failed to remove"));
    }

    #[test]
    fn leftover_socket_name_requires_prefix_and_decimal_suffix() {
        assert!(is_leftover_socket_name(std::ffi::OsStr::new(
            "crabnnb-132037"
        )));
        assert!(!is_leftover_socket_name(std::ffi::OsStr::new("crabnnb-")));
        assert!(!is_leftover_socket_name(std::ffi::OsStr::new(
            "crabnnb-12x"
        )));
        assert!(!is_leftover_socket_name(std::ffi::OsStr::new(
            "other-132037"
        )));
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert!(!is_leftover_socket_name(std::ffi::OsStr::from_bytes(
                b"crabnnb-\xff",
            )));
        }
    }

    #[test]
    fn clean_leftover_dispatches_without_running_cargo_clean() {
        let temp = TempDir::new();

        run_with_args(["xtask", "clean-leftover"], &temp.0, &mut no_op, &mut no_op).unwrap();
    }

    #[test]
    fn clean_leftover_dispatch_propagates_cleanup_errors() {
        let temp = TempDir::new();
        let missing = temp.0.join("missing");

        let error = run_with_args(
            ["xtask", "clean-leftover"],
            &missing,
            &mut no_op,
            &mut no_op,
        )
        .unwrap_err();

        assert!(error.to_string().contains("Failed to read project root"));
    }

    #[test]
    fn no_op_callback_succeeds() {
        no_op().unwrap();
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
    fn cargo_clean_command_reports_nonzero_exit_status() {
        let error = run_cargo_clean_command(Command::new("false")).unwrap_err();
        assert!(error.to_string().contains("cargo clean exited with status"));
    }

    #[test]
    fn run_with_args_propagates_clean_failures() {
        let temp = TempDir::new();
        let error = run_with_args(
            ["xtask", "clean"],
            &temp.0,
            &mut || anyhow::bail!("injected clean failure"),
            &mut no_op,
        )
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

    #[test]
    fn iq_vectors_include_sha_hmac_aes_and_heartbeat_cases() {
        let source = iq_vectors_source();
        assert!(source.contains(
            "SHA256_EMPTY = \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\""
        ));
        assert!(source.contains(
            "SHA256_ABC = \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\""
        ));
        assert!(source.contains("HMAC_LONG_KEY = \""));
        assert!(source.contains("AES_CIPHERTEXT = \""));
        assert!(source.contains("HEARTBEAT = \""));
        assert_eq!(source.lines().count(), 17);
    }

    #[test]
    fn iq_vector_command_writes_the_generated_source() {
        let mut output = Vec::new();
        cmd_iq_vectors(&mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), iq_vectors_source());
    }

    #[test]
    fn iq_vector_command_reports_writer_failures() {
        let mut writer = FailingWriter;
        writer.flush().unwrap();
        let error = cmd_iq_vectors(&mut writer).unwrap_err();
        assert!(error
            .to_string()
            .contains("Failed to write Connect IQ vectors"));
    }

    #[test]
    fn iq_vectors_dispatches_to_the_generator() {
        let temp = TempDir::new();
        let mut output = Vec::new();
        run_with_args(["xtask", "iq-vectors"], &temp.0, &mut no_op, &mut || {
            run_iq_vectors(&mut output)
        })
        .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), iq_vectors_source());
    }

    #[test]
    fn run_with_args_propagates_vector_failures() {
        let temp = TempDir::new();
        let error = run_with_args(["xtask", "iq-vectors"], &temp.0, &mut no_op, &mut || {
            anyhow::bail!("injected vector failure")
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected vector failure"));
    }
}
