use std::path::PathBuf;
use std::time::Duration;

use crate::commands;
use crate::mac::Mac;

#[derive(Debug, PartialEq)]
pub struct RunArgs {
    pub mac: Mac,
    pub lock_cmd: String,
    pub unlock_cmd: String,
    pub max_distance_cm: f64,
    pub max_missed: u32,
    pub scan_period: Duration,
    pub samples: usize,
    pub sample_sleep: Duration,
    pub max_attempts: usize,
    pub rssi_at_1m: f64,
    pub recv_timeout: Duration,
    pub dry_run: bool,
    pub once: bool,
    pub max_iterations: u32,
    pub verbose: bool,
    pub key_file: Option<PathBuf>,
    pub allow_insecure: bool,
    pub heartbeat_window: Duration,
    pub challenge_timeout: Duration,
    pub challenge_handle: u16,
    pub heartbeat_handle: u16,
    pub max_roundtrip: Duration,
    pub bridge_addr: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Help,
    Run(RunArgs),
    GenKey(PathBuf),
}

pub fn usage(prog: &str) -> String {
    format!(
        "Usage: {prog} <btaddr> [options]\n\
         \n\
         Polls the RSSI of a bluetooth device and locks/unlocks the screen\n\
         when it leaves/enters range.\n\
         \n\
         Options:\n\
         \x20 -h, --help              show this help and exit\n\
         \x20 --lock-cmd CMD          command run when the device goes away\n\
         \x20 --unlock-cmd CMD        command run when the device comes back\n\
         \x20 --max-distance-cm F     distance threshold in cm (default 90)\n\
         \x20 --max-missed N          consecutive far/away polls before locking (default 3)\n\
         \x20 --scan-period SECS      delay between polls in seconds (default 1.0)\n\
         \x20 --samples N             RSSI samples averaged per poll (default 7)\n\
         \x20 --sample-sleep-ms MS    delay between samples in ms (default 5)\n\
         \x20 --max-attempts N        max sampling attempts per poll (default 70)\n\
         \x20 --rssi-at-1m DBM        measured power at 1 meter (default -59)\n\
         \x20 --recv-timeout-ms MS    HCI receive timeout in ms (default 10000)\n\
         \x20 --dry-run               print actions instead of executing commands\n\
         \x20 --once                  run a single poll and exit\n\
         \x20 --max-iterations N      stop after N polls (0 = unlimited, default 0)\n\
         \x20 --quiet                 suppress informational output\n\
         \n\
         Secure heartbeat (default; required unless --allow-insecure):\n\
         \x20 --key-file PATH         file with 32-byte key as 64 hex chars (mode 0600)\n\
         \x20 --gen-key-file PATH     generate a new random key file and exit\n\
         \x20 --allow-insecure        allow legacy MAC+RSSI mode without a key (spoofable)\n\
         \x20 --heartbeat-window-ms MS  max accepted heartbeat timestamp age/skew (default 5000)\n\
         \x20 --challenge-timeout-ms MS  wait for the heartbeat reply per poll (default 2000)\n\
         \x20 --challenge-handle H    ATT handle of the challenge characteristic (default 0x0031)\n\
         \x20 --heartbeat-handle H    ATT handle of the heartbeat characteristic (default 0x0034)\n\
         \x20 --max-roundtrip-ms MS   reject heartbeats whose challenge round-trip exceeds MS (relay bound, default 250; 0 disables)\n\
         \x20 --bridge-addr HOST:PORT  use the phone-bridge TCP transport instead of direct BLE (requires --key-file)\n"
    )
}

fn defaults(mac: Mac) -> RunArgs {
    RunArgs {
        mac,
        lock_cmd: commands::DEFAULT_LOCK_CMD.to_string(),
        unlock_cmd: commands::DEFAULT_UNLOCK_CMD.to_string(),
        max_distance_cm: 90.0,
        max_missed: 3,
        scan_period: Duration::from_secs(1),
        samples: 7,
        sample_sleep: Duration::from_millis(5),
        max_attempts: 70,
        rssi_at_1m: -59.0,
        recv_timeout: Duration::from_secs(10),
        dry_run: false,
        once: false,
        max_iterations: 0,
        verbose: true,
        key_file: None,
        allow_insecure: false,
        heartbeat_window: Duration::from_millis(5000),
        challenge_timeout: Duration::from_millis(2000),
        challenge_handle: 0x0031,
        heartbeat_handle: 0x0034,
        max_roundtrip: Duration::from_millis(250),
        bridge_addr: None,
    }
}

impl RunArgs {
    fn validate(&self) -> Result<(), String> {
        if self.max_distance_cm <= 0.0 {
            return Err("--max-distance-cm must be positive".to_string());
        }
        if self.max_missed == 0 {
            return Err("--max-missed must be at least 1".to_string());
        }
        if self.samples == 0 {
            return Err("--samples must be at least 1".to_string());
        }
        if self.max_attempts == 0 {
            return Err("--max-attempts must be at least 1".to_string());
        }
        if self.recv_timeout.is_zero() {
            return Err("--recv-timeout-ms must be positive".to_string());
        }
        if self.heartbeat_window.is_zero() {
            return Err("--heartbeat-window-ms must be positive".to_string());
        }
        if self.challenge_timeout.is_zero() {
            return Err("--challenge-timeout-ms must be positive".to_string());
        }
        Ok(())
    }
}

fn value<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str, String> {
    *i += 1;
    if *i >= args.len() {
        return Err(format!("option {flag} requires a value"));
    }
    Ok(args[*i].as_str())
}

fn num_f64(args: &[String], i: &mut usize, flag: &str) -> Result<f64, String> {
    let v = value(args, i, flag)?;
    v.parse::<f64>()
        .map_err(|_| format!("option {flag} expects a number, got '{v}'"))
}

fn num_u32(args: &[String], i: &mut usize, flag: &str) -> Result<u32, String> {
    let v = value(args, i, flag)?;
    v.parse::<u32>()
        .map_err(|_| format!("option {flag} expects a positive integer, got '{v}'"))
}

fn num_usize(args: &[String], i: &mut usize, flag: &str) -> Result<usize, String> {
    let v = value(args, i, flag)?;
    v.parse::<usize>()
        .map_err(|_| format!("option {flag} expects a positive integer, got '{v}'"))
}

fn secs(args: &[String], i: &mut usize, flag: &str) -> Result<Duration, String> {
    let f = num_f64(args, i, flag)?;
    if f < 0.0 {
        return Err(format!("option {flag} must not be negative"));
    }
    Ok(Duration::from_secs_f64(f))
}

fn millis(args: &[String], i: &mut usize, flag: &str) -> Result<Duration, String> {
    let f = num_f64(args, i, flag)?;
    if f < 0.0 {
        return Err(format!("option {flag} must not be negative"));
    }
    Ok(Duration::from_millis(f as u64))
}

fn handle_num(args: &[String], i: &mut usize, flag: &str) -> Result<u16, String> {
    let v = value(args, i, flag)?;
    let parse_one = |s: &str| -> Result<u16, String> {
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u16::from_str_radix(hex, 16)
        } else {
            s.parse::<u16>()
        }
        .map_err(|_| format!("option {flag} expects a handle (decimal or 0x hex), got '{s}'"))
    };
    parse_one(v)
}

pub fn parse(args: &[String]) -> Result<Parsed, String> {
    if args.is_empty() {
        return Err("missing bluetooth MAC address".to_string());
    }
    let first = args[0].as_str();
    if first == "-h" || first == "--help" {
        return Ok(Parsed::Help);
    }
    if first == "--gen-key-file" {
        let mut i = 0usize;
        let v = value(args, &mut i, first)?;
        return Ok(Parsed::GenKey(PathBuf::from(v)));
    }
    let mac = Mac::parse(first).map_err(|e| format!("invalid bluetooth address '{first}': {e}"))?;
    let mut r = defaults(mac);
    let mut i = 1usize;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--lock-cmd" => r.lock_cmd = value(args, &mut i, flag)?.to_string(),
            "--unlock-cmd" => r.unlock_cmd = value(args, &mut i, flag)?.to_string(),
            "--max-distance-cm" => r.max_distance_cm = num_f64(args, &mut i, flag)?,
            "--max-missed" => r.max_missed = num_u32(args, &mut i, flag)?,
            "--scan-period" => r.scan_period = secs(args, &mut i, flag)?,
            "--samples" => r.samples = num_usize(args, &mut i, flag)?,
            "--sample-sleep-ms" => r.sample_sleep = millis(args, &mut i, flag)?,
            "--max-attempts" => r.max_attempts = num_usize(args, &mut i, flag)?,
            "--rssi-at-1m" => r.rssi_at_1m = num_f64(args, &mut i, flag)?,
            "--recv-timeout-ms" => r.recv_timeout = millis(args, &mut i, flag)?,
            "--max-iterations" => r.max_iterations = num_u32(args, &mut i, flag)?,
            "--dry-run" => r.dry_run = true,
            "--once" => r.once = true,
            "--quiet" => r.verbose = false,
            "--key-file" => r.key_file = Some(PathBuf::from(value(args, &mut i, flag)?)),
            "--allow-insecure" => r.allow_insecure = true,
            "--heartbeat-window-ms" => r.heartbeat_window = millis(args, &mut i, flag)?,
            "--challenge-timeout-ms" => r.challenge_timeout = millis(args, &mut i, flag)?,
            "--challenge-handle" => r.challenge_handle = handle_num(args, &mut i, flag)?,
            "--heartbeat-handle" => r.heartbeat_handle = handle_num(args, &mut i, flag)?,
            "--max-roundtrip-ms" => r.max_roundtrip = millis(args, &mut i, flag)?,
            "--bridge-addr" => r.bridge_addr = Some(value(args, &mut i, flag)?.to_string()),
            other => return Err(format!("unknown option '{other}'")),
        }
        i += 1;
    }
    r.validate()?;
    Ok(Parsed::Run(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_parse_help() {
        assert_eq!(parse(&args(&["--help"])).unwrap(), Parsed::Help);
        assert_eq!(parse(&args(&["-h"])).unwrap(), Parsed::Help);
    }

    #[test]
    fn test_parse_empty_args() {
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn test_parse_invalid_mac() {
        let err = parse(&args(&["NOT_A_MAC"])).unwrap_err();
        assert!(err.contains("invalid bluetooth address"));
    }

    #[test]
    fn test_parse_defaults() {
        let parsed = parse(&args(&["AA:BB:CC:DD:EE:FF"])).unwrap();
        match parsed {
            Parsed::Run(r) => {
                assert_eq!(r.mac.bytes(), [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
                assert!((r.max_distance_cm - 90.0).abs() < 1e-10);
                assert_eq!(r.max_missed, 3);
                assert_eq!(r.scan_period, Duration::from_secs(1));
                assert_eq!(r.samples, 7);
                assert!((r.rssi_at_1m - (-59.0)).abs() < 1e-10);
                assert!(!r.dry_run);
                assert!(!r.once);
                assert!(r.verbose);
            }
            Parsed::Help | Parsed::GenKey(_) => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_all_options() {
        let parsed = parse(&args(&[
            "AA:BB:CC:DD:EE:FF",
            "--lock-cmd",
            "loginctl lock-session",
            "--unlock-cmd",
            "loginctl unlock-session",
            "--max-distance-cm",
            "150",
            "--max-missed",
            "5",
            "--scan-period",
            "2.5",
            "--samples",
            "10",
            "--sample-sleep-ms",
            "100",
            "--max-attempts",
            "20",
            "--rssi-at-1m",
            "-65",
            "--recv-timeout-ms",
            "5000",
            "--max-iterations",
            "100",
            "--dry-run",
            "--once",
            "--quiet",
        ]))
        .unwrap();
        match parsed {
            Parsed::Run(r) => {
                assert_eq!(r.lock_cmd, "loginctl lock-session");
                assert_eq!(r.unlock_cmd, "loginctl unlock-session");
                assert!((r.max_distance_cm - 150.0).abs() < 1e-10);
                assert_eq!(r.max_missed, 5);
                assert_eq!(r.scan_period, Duration::from_millis(2500));
                assert_eq!(r.samples, 10);
                assert_eq!(r.sample_sleep, Duration::from_millis(100));
                assert_eq!(r.max_attempts, 20);
                assert!((r.rssi_at_1m - (-65.0)).abs() < 1e-10);
                assert_eq!(r.recv_timeout, Duration::from_millis(5000));
                assert_eq!(r.max_iterations, 100);
                assert!(r.dry_run);
                assert!(r.once);
                assert!(!r.verbose);
            }
            Parsed::Help | Parsed::GenKey(_) => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_unknown_option() {
        let err = parse(&args(&["AA:BB:CC:DD:EE:FF", "--bogus"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn test_parse_missing_value() {
        let err = parse(&args(&["AA:BB:CC:DD:EE:FF", "--max-distance-cm"])).unwrap_err();
        assert!(err.contains("requires a value"));
    }

    #[test]
    fn test_parse_negative_distance_fails() {
        assert!(parse(&args(&["AA:BB:CC:DD:EE:FF", "--max-distance-cm", "-5"])).is_err());
    }

    #[test]
    fn test_usage_contains_prog() {
        let u = usage("crabnnb");
        assert!(u.contains("crabnnb <btaddr>"));
    }

    #[test]
    fn test_parse_secure_defaults() {
        let parsed = parse(&args(&["AA:BB:CC:DD:EE:FF"])).unwrap();
        match parsed {
            Parsed::Run(r) => {
                assert_eq!(r.key_file, None);
                assert!(!r.allow_insecure);
                assert_eq!(r.heartbeat_window, Duration::from_millis(5000));
                assert_eq!(r.challenge_timeout, Duration::from_millis(2000));
                assert_eq!(r.challenge_handle, 0x0031);
                assert_eq!(r.heartbeat_handle, 0x0034);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_secure_options() {
        let parsed = parse(&args(&[
            "AA:BB:CC:DD:EE:FF",
            "--key-file",
            "/run/crabnnb/key.hex",
            "--heartbeat-window-ms",
            "3000",
            "--challenge-timeout-ms",
            "1500",
            "--challenge-handle",
            "0x2a",
            "--heartbeat-handle",
            "45",
        ]))
        .unwrap();
        match parsed {
            Parsed::Run(r) => {
                assert_eq!(r.key_file, Some(PathBuf::from("/run/crabnnb/key.hex")));
                assert_eq!(r.heartbeat_window, Duration::from_millis(3000));
                assert_eq!(r.challenge_timeout, Duration::from_millis(1500));
                assert_eq!(r.challenge_handle, 0x2a);
                assert_eq!(r.heartbeat_handle, 45);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_allow_insecure() {
        let parsed = parse(&args(&["AA:BB:CC:DD:EE:FF", "--allow-insecure"])).unwrap();
        match parsed {
            Parsed::Run(r) => assert!(r.allow_insecure),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_gen_key_file() {
        let parsed = parse(&args(&["--gen-key-file", "/tmp/mykey.hex"])).unwrap();
        assert_eq!(parsed, Parsed::GenKey(PathBuf::from("/tmp/mykey.hex")));
        assert!(parse(&args(&["--gen-key-file"])).is_err());
    }

    #[test]
    fn test_parse_bad_handle() {
        let err = parse(&args(&["AA:BB:CC:DD:EE:FF", "--challenge-handle", "xyz"])).unwrap_err();
        assert!(err.contains("expects a handle"));
    }

    #[test]
    fn test_parse_zero_heartbeat_window_fails() {
        assert!(parse(&args(&["AA:BB:CC:DD:EE:FF", "--heartbeat-window-ms", "0"])).is_err());
    }

    #[test]
    fn test_usage_mentions_secure_options() {
        let u = usage("crabnnb");
        assert!(u.contains("--key-file"));
        assert!(u.contains("--gen-key-file"));
        assert!(u.contains("--allow-insecure"));
        assert!(u.contains("--heartbeat-handle"));
        assert!(u.contains("--max-roundtrip-ms"));
        assert!(u.contains("--bridge-addr"));
    }

    #[test]
    fn test_parse_bridge_and_roundtrip() {
        let parsed = parse(&args(&[
            "AA:BB:CC:DD:EE:FF",
            "--bridge-addr",
            "127.0.0.1:8765",
            "--max-roundtrip-ms",
            "300",
        ]))
        .unwrap();
        match parsed {
            Parsed::Run(r) => {
                assert_eq!(r.bridge_addr, Some("127.0.0.1:8765".to_string()));
                assert_eq!(r.max_roundtrip, Duration::from_millis(300));
            }
            _ => panic!("expected Run"),
        }
    }
}
