use std::io::Write;
use std::time::Duration;

use crate::bridge::BridgeDevice;
use crate::cli::{self, Parsed, RunArgs};
use crate::clock::{Clock, SystemClock};
use crate::commands::{CmdRunner, DryRunner, ShellRunner};
use crate::crypto;
use crate::device::{BleDevice, BtOps};
use crate::mac::Mac;
use crate::monitor::{Action, Monitor, Sample};
use crate::sampler::Sampler;
use crate::session::SecureSession;
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
    mut session: Option<&mut SecureSession>,
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
            Ok(Some(_)) => match &mut session {
                Some(s) => match s.probe(bt) {
                    Ok(ok) => {
                        if cfg.verbose {
                            let _ = writeln!(
                                out,
                                "verified heartbeat counter={} ts_ms={} rtt={:?}",
                                ok.heartbeat.counter, ok.heartbeat.timestamp_ms, ok.rtt
                            );
                        }
                        Sample::Distance(bt.distance_approximation())
                    }
                    Err(e) => {
                        if cfg.verbose {
                            let _ = writeln!(out, "DEVICE UNVERIFIED: {e}");
                        }
                        Sample::Unverified
                    }
                },
                None => Sample::Distance(bt.distance_approximation()),
            },
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
                Sample::Unverified => {
                    let _ = writeln!(out, "DEVICE UNVERIFIED (no valid encrypted heartbeat)");
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
        Ok(Parsed::GenKey(path)) => {
            return match crypto::generate_key_file(&path) {
                Ok(()) => {
                    let _ = writeln!(
                        out,
                        "OK: wrote new 32-byte key to {} (mode 0600); use it with --key-file",
                        path.display()
                    );
                    0
                }
                Err(e) => {
                    let _ = writeln!(err, "ERROR: cannot write key file {}: {e}", path.display());
                    1
                }
            };
        }
        Ok(Parsed::Run(r)) => r,
        Err(msg) => {
            let _ = writeln!(err, "error: {msg}\n\n{}", cli::usage(prog));
            return 1;
        }
    };
    if parsed.key_file.is_none() && !parsed.allow_insecure {
        let _ = writeln!(
            err,
            "error: --key-file is required for secure operation.\n\
             Generate one with: {prog} --gen-key-file <path>\n\
             Or pass --allow-insecure to explicitly run in legacy MAC+RSSI mode (spoofable).\n\n{}",
            cli::usage(prog)
        );
        return 1;
    }
    let key = match &parsed.key_file {
        Some(p) => match crypto::load_key(p) {
            Ok(k) => Some(k),
            Err(e) => {
                let _ = writeln!(err, "ERROR: cannot load key file: {e}");
                return 1;
            }
        },
        None => None,
    };
    if parsed.bridge_addr.is_some() && key.is_none() {
        let _ = writeln!(
            err,
            "error: --bridge-addr requires --key-file (the phone bridge verifies encrypted heartbeats)"
        );
        return 1;
    }
    let mac = parsed.mac;
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
    let runner: &mut dyn CmdRunner = if parsed.dry_run { &mut dry } else { &mut shell };
    let mut clock = SystemClock;
    if let Some(addr) = &parsed.bridge_addr {
        let mut device = match BridgeDevice::connect(addr, parsed.challenge_timeout) {
            Ok(d) => d,
            Err(e) => {
                let _ = writeln!(err, "ERROR: cannot connect to phone bridge at {addr}: {e}");
                return 2;
            }
        };
        let k = key.unwrap();
        let local = match device.local_mac() {
            Ok(l) => l,
            Err(e) => {
                let _ = writeln!(err, "ERROR: cannot derive bridge identity: {e}");
                return 2;
            }
        };
        let mut session = Some(SecureSession::new(
            k,
            mac,
            local,
            parsed.heartbeat_window,
            parsed.max_roundtrip,
        ));
        return run(
            &mut device,
            &mac,
            &cfg,
            session.as_mut(),
            runner,
            &mut clock,
            out,
            err,
        );
    }
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
    let sampler = Sampler::with_params(
        parsed.rssi_at_1m,
        parsed.samples,
        parsed.sample_sleep,
        parsed.max_attempts,
    );
    let mut device = BleDevice::new(transport, mac, sampler);
    let mut session: Option<SecureSession> = if let Some(k) = key {
        device.set_secure(
            parsed.challenge_handle,
            parsed.heartbeat_handle,
            parsed.challenge_timeout,
        );
        match device.local_mac() {
            Ok(local) => Some(SecureSession::new(
                k,
                mac,
                local,
                parsed.heartbeat_window,
                parsed.max_roundtrip,
            )),
            Err(e) => {
                let _ = writeln!(err, "ERROR: cannot read local bluetooth address: {e}");
                return 2;
            }
        }
    } else {
        let _ = writeln!(
            err,
            "WARNING: --allow-insecure legacy mode: presence is MAC+RSSI only and can be spoofed (e.g. Flipper Zero)"
        );
        None
    };
    run(
        &mut device,
        &mac,
        &cfg,
        session.as_mut(),
        runner,
        &mut clock,
        out,
        err,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heartbeat;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;

    struct MockClock {
        slept: Arc<Mutex<Vec<Duration>>>,
    }

    impl Clock for MockClock {
        fn sleep(&mut self, d: Duration) {
            self.slept.lock().unwrap().push(d);
        }
    }

    enum ProbeStep {
        Valid { counter: u64, ts_ms: u64 },
        Raw(Vec<u8>),
        NoResponse,
    }

    struct MockBt {
        present: bool,
        distances: VecDeque<f64>,
        key: [u8; 32],
        watch: Mac,
        local: Mac,
        steps: VecDeque<ProbeStep>,
    }

    impl MockBt {
        fn distance(&mut self) -> f64 {
            self.distances.pop_front().unwrap_or(0.5)
        }
    }

    impl BtOps for MockBt {
        fn discover(&mut self) -> io::Result<usize> {
            Ok(1)
        }
        fn lookup_name(&mut self, _mac: &Mac) -> io::Result<Option<String>> {
            Ok(if self.present {
                Some("FR235".to_string())
            } else {
                None
            })
        }
        fn distance_approximation(&mut self) -> f64 {
            self.distance()
        }
        fn local_mac(&mut self) -> io::Result<Mac> {
            Ok(self.local)
        }
        fn secure_probe(&mut self, challenge: &[u8]) -> io::Result<Option<Vec<u8>>> {
            match self.steps.pop_front().unwrap_or(ProbeStep::NoResponse) {
                ProbeStep::Valid { counter, ts_ms } => {
                    let nonce: [u8; 12] = [0x42u8; 12];
                    let aad = heartbeat::aad(&self.watch, &self.local);
                    Ok(Some(heartbeat::seal_heartbeat(
                        &self.key, &aad, counter, ts_ms, challenge, &nonce,
                    )))
                }
                ProbeStep::Raw(b) => Ok(Some(b)),
                ProbeStep::NoResponse => Ok(None),
            }
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn test_cfg() -> RunConfig {
        RunConfig {
            lock_cmd: "lock-cmd".to_string(),
            unlock_cmd: "unlock-cmd".to_string(),
            max_distance_cm: 90.0,
            max_missed: 1,
            scan_period: Duration::from_millis(0),
            dry_run: true,
            once: false,
            max_iterations: 1,
            verbose: false,
        }
    }

    struct Fixture {
        bt: MockBt,
        watch: Mac,
        session: Option<SecureSession>,
    }

    fn fixture(steps: Vec<ProbeStep>, distances: Vec<f64>, secure: bool) -> Fixture {
        let key = [0x11u8; 32];
        let watch = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        let local = Mac::parse("11:22:33:44:55:66").unwrap();
        let bt = MockBt {
            present: true,
            distances: distances.into(),
            key,
            watch,
            local,
            steps: steps.into(),
        };
        let session = if secure {
            Some(SecureSession::new(
                key,
                watch,
                local,
                Duration::from_secs(5),
                Duration::from_millis(250),
            ))
        } else {
            None
        };
        Fixture { bt, watch, session }
    }

    fn run_once(fx: &mut Fixture, cfg: &RunConfig) -> Vec<String> {
        let mut runner = DryRunner::default();
        let slept = Arc::new(Mutex::new(Vec::new()));
        let mut clock = MockClock {
            slept: Arc::clone(&slept),
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &mut fx.bt,
            &fx.watch,
            cfg,
            fx.session.as_mut(),
            &mut runner,
            &mut clock,
            &mut out,
            &mut err,
        );
        assert_eq!(code, 0);
        let executed = runner.executed.borrow().clone();
        executed
    }

    #[test]
    fn secure_valid_heartbeat_near_unlocks() {
        let mut fx = fixture(
            vec![ProbeStep::Valid {
                counter: 1,
                ts_ms: now_ms(),
            }],
            vec![0.5],
            true,
        );
        let executed = run_once(&mut fx, &test_cfg());
        assert_eq!(executed, vec!["unlock-cmd".to_string()]);
    }

    #[test]
    fn secure_garbage_heartbeat_never_unlocks() {
        let mut fx = fixture(
            vec![ProbeStep::Raw(vec![0xaa; heartbeat::MESSAGE_LEN])],
            vec![0.5],
            true,
        );
        let executed = run_once(&mut fx, &test_cfg());
        assert!(executed.is_empty());
    }

    #[test]
    fn secure_no_response_never_unlocks() {
        let mut fx = fixture(vec![ProbeStep::NoResponse], vec![0.5], true);
        let executed = run_once(&mut fx, &test_cfg());
        assert!(executed.is_empty());
    }

    #[test]
    fn secure_replayed_counter_triggers_lock() {
        let mut cfg = test_cfg();
        cfg.max_iterations = 2;
        let mut fx = fixture(
            vec![
                ProbeStep::Valid {
                    counter: 1,
                    ts_ms: now_ms(),
                },
                ProbeStep::Valid {
                    counter: 1,
                    ts_ms: now_ms(),
                },
            ],
            vec![0.5, 0.5],
            true,
        );
        let executed = run_once(&mut fx, &cfg);
        assert_eq!(
            executed,
            vec!["unlock-cmd".to_string(), "lock-cmd".to_string()]
        );
    }

    #[test]
    fn secure_verified_but_far_locks() {
        let mut cfg = test_cfg();
        cfg.max_iterations = 2;
        let mut fx = fixture(
            vec![
                ProbeStep::Valid {
                    counter: 1,
                    ts_ms: now_ms(),
                },
                ProbeStep::Valid {
                    counter: 2,
                    ts_ms: now_ms(),
                },
            ],
            vec![0.5, 5.0],
            true,
        );
        let executed = run_once(&mut fx, &cfg);
        assert_eq!(
            executed,
            vec!["unlock-cmd".to_string(), "lock-cmd".to_string()]
        );
    }

    #[test]
    fn legacy_insecure_mode_unlocks_on_rssi_alone() {
        let mut fx = fixture(vec![], vec![0.5], false);
        let executed = run_once(&mut fx, &test_cfg());
        assert_eq!(executed, vec!["unlock-cmd".to_string()]);
    }

    #[test]
    fn bridge_valid_heartbeat_unlocks() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let key = [0x11u8; 32];
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut counter = 1u32;
            loop {
                let mut hdr = [0u8; 4];
                if s.read_exact(&mut hdr).is_err() {
                    break;
                }
                let len = u32::from_le_bytes(hdr) as usize;
                let mut chal = vec![0u8; len];
                if s.read_exact(&mut chal).is_err() {
                    break;
                }
                let ts = (now_ms() / 1000) as u32;
                let msg = heartbeat::seal_heartbeat_bridge(&key, counter, ts, &chal, &[0x09u8; 16]);
                counter += 1;
                if s.write_all(&(msg.len() as u32).to_le_bytes())
                    .and_then(|_| s.write_all(&msg))
                    .is_err()
                {
                    break;
                }
            }
        });
        let cfg = test_cfg();
        let mut device = BridgeDevice::connect(&addr, Duration::from_secs(2)).unwrap();
        let watch = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        let local = device.local_mac().unwrap();
        let mut session = Some(SecureSession::new(
            key,
            watch,
            local,
            Duration::from_secs(5),
            Duration::from_millis(500),
        ));
        let mut runner = DryRunner::default();
        let slept = Arc::new(Mutex::new(Vec::new()));
        let mut clock = MockClock {
            slept: Arc::clone(&slept),
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &mut device,
            &watch,
            &cfg,
            session.as_mut(),
            &mut runner,
            &mut clock,
            &mut out,
            &mut err,
        );
        assert_eq!(code, 0);
        assert_eq!(
            runner.executed.borrow().as_slice(),
            &["unlock-cmd".to_string()]
        );
        drop(server);
    }

    #[test]
    fn bridge_slow_relay_response_locks() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let key = [0x11u8; 32];
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            loop {
                let mut hdr = [0u8; 4];
                if s.read_exact(&mut hdr).is_err() {
                    break;
                }
                let len = u32::from_le_bytes(hdr) as usize;
                let mut chal = vec![0u8; len];
                if s.read_exact(&mut chal).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
                let ts = (now_ms() / 1000) as u32;
                let msg = heartbeat::seal_heartbeat_bridge(&key, 1, ts, &chal, &[0x09u8; 16]);
                if s.write_all(&(msg.len() as u32).to_le_bytes())
                    .and_then(|_| s.write_all(&msg))
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut cfg = test_cfg();
        cfg.max_iterations = 2;
        let mut device = BridgeDevice::connect(&addr, Duration::from_secs(2)).unwrap();
        let watch = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        let local = device.local_mac().unwrap();
        let mut session = Some(SecureSession::new(
            key,
            watch,
            local,
            Duration::from_secs(5),
            Duration::from_millis(5),
        ));
        let mut runner = DryRunner::default();
        let slept = Arc::new(Mutex::new(Vec::new()));
        let mut clock = MockClock {
            slept: Arc::clone(&slept),
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &mut device,
            &watch,
            &cfg,
            session.as_mut(),
            &mut runner,
            &mut clock,
            &mut out,
            &mut err,
        );
        assert_eq!(code, 0);
        assert!(runner.executed.borrow().is_empty());
        drop(server);
    }

    #[test]
    fn device_absent_exits_with_error() {
        let cfg = test_cfg();
        let mut fx = fixture(vec![], vec![], false);
        fx.bt.present = false;
        let mut runner = DryRunner::default();
        let slept = Arc::new(Mutex::new(Vec::new()));
        let mut clock = MockClock {
            slept: Arc::clone(&slept),
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &mut fx.bt,
            &fx.watch,
            &cfg,
            fx.session.as_mut(),
            &mut runner,
            &mut clock,
            &mut out,
            &mut err,
        );
        assert_eq!(code, 3);
        assert!(runner.executed.borrow().is_empty());
    }
}
