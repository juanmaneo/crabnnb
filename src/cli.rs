use std::time::Duration;

use crate::commands;
use crate::mac::Mac;

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
}

pub enum Parsed {
    Help,
    Run(RunArgs),
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
         \x20 --quiet                 suppress informational output\n"
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

pub fn parse(args: &[String]) -> Result<Parsed, String> {
    if args.is_empty() {
        return Err("missing bluetooth MAC address".to_string());
    }
    let first = args[0].as_str();
    if first == "-h" || first == "--help" {
        return Ok(Parsed::Help);
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
            other => return Err(format!("unknown option '{other}'")),
        }
        i += 1;
    }
    r.validate()?;
    Ok(Parsed::Run(r))
}
