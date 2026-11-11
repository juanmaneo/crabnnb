use std::io::Write;
use std::time::Duration;

use crate::cli::{self, Parsed, RunArgs};
use crate::clock::{Clock, SystemClock};
use crate::commands::{CmdRunner, DryRunner, ShellRunner};
use crate::device::{BleDevice, BtOps};
use crate::mac::Mac;
use crate::monitor::{Action, Monitor, Sample};
use crate::sampler::Sampler;
use crate::sys::Sys;
use crate::transport::HciTransport;

pub struct RunConfig {
    pub lock_cmd: String,
    pub unlock_cmd: String,
    pub max_distance_cm: f64,
    pub max_missed: u32,
    pub scan_period: Duration,
    pub dry_run: bool,
    pub once: bool,
    pub max_iterations: u32,
    pub verbose: bool,
}

pub fn run<B: BtOps + ?Sized>(
    bt: &mut B,
    mac: &Mac,
    cfg: &RunConfig,
    runner: &mut dyn CmdRunner,
    clock: &mut dyn Clock,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let found = match bt.discover() {
        Ok(n) => n,
        Err(e) => {
            let _ = writeln!(err, "ERROR: bluetooth discovery failed: {e}");
            return 2;
        }
    };
    if cfg.verbose {
        let _ = writeln!(out, "found {found} devices");
    }
    match bt.lookup_name(mac) {
        Err(e) => {
            let _ = writeln!(err, "ERROR: name lookup failed: {e}");
            return 3;
        }
        Ok(None) => {
            let _ = writeln!(err, "ERROR: Your bluetooth device is not active");
            return 3;
        }
        Ok(Some(name)) => {
            let _ = writeln!(out, "OK: Found your device {name}");
        }
    }
    let mut monitor = Monitor::new(cfg.max_missed, cfg.max_distance_cm);
    let mut iterations: u32 = 0;
    loop {
        if cfg.max_iterations > 0 && iterations >= cfg.max_iterations {
            break;
        }
        iterations += 1;
        let sample = match bt.lookup_name(mac) {
            Ok(Some(_)) => Sample::Distance(bt.distance_approximation()),
            Ok(None) | Err(_) => Sample::Away,
        };
        if cfg.verbose {
            match &sample {
                Sample::Distance(d) => {
                    let _ = writeln!(out, "Approximate distance: {:.1} cm", d * 100.0);
                }
                Sample::Away => {
                    let _ = writeln!(out, "DEVICE AWAY");
                }
            }
        }
        if let Some(action) = monitor.update(&sample) {
            let (cmd, label) = match action {
                Action::Lock => (&cfg.lock_cmd, "lock"),
                Action::Unlock => (&cfg.unlock_cmd, "unlock"),
            };
            let _ = writeln!(out, "running {label} command");
            if let Err(e) = runner.run(cmd) {
                let _ = writeln!(err, "ERROR: command failed: {e}");
            }
        }
        if cfg.once {
            break;
        }
        clock.sleep(cfg.scan_period);
    }
    0
}

pub fn run_cli<S: Sys>(
    prog: &str,
    args: &[String],
    sys: &S,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let parsed: RunArgs = match cli::parse(args) {
        Ok(Parsed::Help) => {
            let _ = write!(out, "{}", cli::usage(prog));
            return 0;
        }
        Ok(Parsed::Run(r)) => r,
        Err(msg) => {
            let _ = writeln!(err, "error: {msg}\n\n{}", cli::usage(prog));
            return 1;
        }
    };
    let transport = match HciTransport::open(sys) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(err, "ERROR: cannot open HCI socket: {e}");
            return 2;
        }
    };
    if let Err(e) = transport.set_recv_timeout(parsed.recv_timeout) {
        let _ = writeln!(err, "ERROR: cannot set receive timeout: {e}");
        return 2;
    }
    let mac = parsed.mac;
    let sampler = Sampler::with_params(
        parsed.rssi_at_1m,
        parsed.samples,
        parsed.sample_sleep,
        parsed.max_attempts,
    );
    let mut device = BleDevice::new(transport, mac, sampler);
    let cfg = RunConfig {
        lock_cmd: parsed.lock_cmd,
        unlock_cmd: parsed.unlock_cmd,
        max_distance_cm: parsed.max_distance_cm,
        max_missed: parsed.max_missed,
        scan_period: parsed.scan_period,
        dry_run: parsed.dry_run,
        once: parsed.once,
        max_iterations: parsed.max_iterations,
        verbose: parsed.verbose,
    };
    let mut dry = DryRunner::default();
    let mut shell = ShellRunner;
    let runner: &mut dyn CmdRunner = if parsed.dry_run {
        &mut dry
    } else {
        &mut shell
    };
    let mut clock = SystemClock;
    run(&mut device, &mac, &cfg, runner, &mut clock, out, err)
}
