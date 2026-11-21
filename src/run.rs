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

fn run_bridge(
    device: &mut dyn BtOps,
    key: [u8; crypto::KEY_LEN],
    watch: Mac,
    cfg: &RunConfig,
    heartbeat_window: Duration,
    max_roundtrip: Duration,
    runner: &mut dyn CmdRunner,
    clock: &mut dyn Clock,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let local = match device.local_mac() {
        Ok(local) => local,
        Err(e) => {
            let _ = writeln!(err, "ERROR: cannot derive bridge identity: {e}");
            return 2;
        }
    };
    let mut session = Some(SecureSession::new(
        key,
        watch,
        local,
        heartbeat_window,
        max_roundtrip,
    ));
    run(
        device,
        &watch,
        cfg,
        session.as_mut(),
        runner,
        clock,
        out,
        err,
    )
}

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

pub fn run(
    bt: &mut dyn BtOps,
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
        return run_bridge(
            &mut device,
            key.unwrap(),
            mac,
            &cfg,
            parsed.heartbeat_window,
            parsed.max_roundtrip,
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
    use crate::hci;
    use crate::heartbeat;
    use crate::sys::LinuxSys;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;

    /// Linux `SO_LINGER` option name (not defined in `hci`, only used by tests
    /// to force an RST on close when exercising fake-bridge error paths).
    const SO_LINGER: i32 = 13;

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
        fail_discover: bool,
        lookup_error_on_call: Option<usize>,
        lookup_calls: usize,
        distances: VecDeque<f64>,
        key: [u8; 32],
        watch: Mac,
        local: Mac,
        fail_local_mac: bool,
        steps: VecDeque<ProbeStep>,
    }

    impl MockBt {
        fn distance(&mut self) -> f64 {
            self.distances.pop_front().unwrap_or(0.5)
        }
    }

    impl BtOps for MockBt {
        fn discover(&mut self) -> io::Result<usize> {
            if self.fail_discover {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "scan denied",
                ))
            } else {
                Ok(1)
            }
        }
        fn lookup_name(&mut self, _mac: &Mac) -> io::Result<Option<String>> {
            self.lookup_calls += 1;
            if self.lookup_error_on_call == Some(self.lookup_calls) {
                return Err(io::Error::new(io::ErrorKind::NotFound, "lookup failed"));
            }
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
            if self.fail_local_mac {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "identity unavailable",
                ))
            } else {
                Ok(self.local)
            }
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
            fail_discover: false,
            lookup_error_on_call: None,
            lookup_calls: 0,
            distances: distances.into(),
            key,
            watch,
            local,
            fail_local_mac: false,
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

    #[test]
    fn bridge_run_reports_local_identity_failure() {
        let mut f = fixture(vec![], vec![], false);
        f.bt.fail_local_mac = true;
        let mut runner = DryRunner::default();
        let mut clock = MockClock {
            slept: Arc::new(Mutex::new(Vec::new())),
        };
        let mut out = Vec::new();
        let mut err = Vec::new();

        assert_eq!(
            run_bridge(
                &mut f.bt,
                [0x11; crypto::KEY_LEN],
                f.watch,
                &test_cfg(),
                Duration::from_secs(5),
                Duration::from_millis(250),
                &mut runner,
                &mut clock,
                &mut out,
                &mut err,
            ),
            2
        );
        assert!(out.is_empty());
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "ERROR: cannot derive bridge identity: identity unavailable\n"
        );
    }

    fn run_capture(
        fx: &mut Fixture,
        cfg: &RunConfig,
        runner: &mut dyn CmdRunner,
        out: &mut Vec<u8>,
        err: &mut Vec<u8>,
    ) -> i32 {
        let slept = Arc::new(Mutex::new(Vec::new()));
        let mut clock = MockClock {
            slept: Arc::clone(&slept),
        };
        run(
            &mut fx.bt,
            &fx.watch,
            cfg,
            fx.session.as_mut(),
            runner,
            &mut clock,
            out,
            err,
        )
    }

    fn run_once(fx: &mut Fixture, cfg: &RunConfig) -> Vec<String> {
        let mut runner = DryRunner::default();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_capture(fx, cfg, &mut runner, &mut out, &mut err);
        assert_eq!(code, 0);
        let executed = runner.executed.borrow().clone();
        executed
    }

    struct FailingRunner;

    impl CmdRunner for FailingRunner {
        fn run(&mut self, _: &str) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "runner denied",
            ))
        }
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
    fn run_reports_discovery_and_initial_lookup_errors() {
        let cfg = test_cfg();
        let mut fx = fixture(vec![], vec![], false);
        fx.bt.fail_discover = true;
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run_capture(&mut fx, &cfg, &mut DryRunner::default(), &mut out, &mut err,),
            2
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("bluetooth discovery failed"));

        let mut fx = fixture(vec![], vec![], false);
        fx.bt.lookup_error_on_call = Some(1);
        let mut err = Vec::new();
        assert_eq!(
            run_capture(
                &mut fx,
                &cfg,
                &mut DryRunner::default(),
                &mut Vec::new(),
                &mut err,
            ),
            3
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("name lookup failed"));
    }

    #[test]
    fn run_verbose_paths_report_distance_unverified_away_and_command_errors() {
        let mut cfg = test_cfg();
        cfg.verbose = true;
        cfg.once = true;

        let mut fx = fixture(vec![], vec![0.5], false);
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run_capture(&mut fx, &cfg, &mut FailingRunner, &mut out, &mut err,),
            0
        );
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("Approximate distance:"));
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("command failed: runner denied"));

        let mut fx = fixture(vec![ProbeStep::NoResponse], vec![], true);
        let mut out = Vec::new();
        assert_eq!(
            run_capture(
                &mut fx,
                &cfg,
                &mut DryRunner::default(),
                &mut out,
                &mut Vec::new(),
            ),
            0
        );
        let output = String::from_utf8(out).unwrap();
        assert!(output.contains("DEVICE UNVERIFIED: no heartbeat response"));
        assert!(output.contains("DEVICE UNVERIFIED (no valid encrypted heartbeat)"));

        let mut fx = fixture(vec![], vec![], false);
        fx.bt.lookup_error_on_call = Some(2);
        let mut out = Vec::new();
        assert_eq!(
            run_capture(
                &mut fx,
                &cfg,
                &mut DryRunner::default(),
                &mut out,
                &mut Vec::new(),
            ),
            0
        );
        assert!(String::from_utf8(out).unwrap().contains("DEVICE AWAY"));
    }

    #[test]
    fn run_verbose_secure_success_reports_verified_heartbeat() {
        let mut fx = fixture(
            vec![ProbeStep::Valid {
                counter: 1,
                ts_ms: now_ms(),
            }],
            vec![0.5],
            true,
        );
        let mut cfg = test_cfg();
        cfg.verbose = true;
        cfg.once = true;
        let mut out = Vec::new();
        assert_eq!(
            run_capture(
                &mut fx,
                &cfg,
                &mut DryRunner::default(),
                &mut out,
                &mut Vec::new(),
            ),
            0
        );
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("verified heartbeat counter=1"));
    }

    struct FakeBridge {
        key: [u8; 32],
        counter: u32,
        delay: Option<Duration>,
    }

    impl FakeBridge {
        /// Serves sealed heartbeats on `stream` until the peer disconnects or
        /// an I/O error occurs on either side of the conversation.
        fn serve(mut self, mut stream: std::net::TcpStream) {
            use std::io::{Read, Write};
            loop {
                let mut hdr = [0u8; 4];
                if stream.read_exact(&mut hdr).is_err() {
                    break;
                }
                let len = u32::from_le_bytes(hdr) as usize;
                let mut chal = vec![0u8; len];
                if stream.read_exact(&mut chal).is_err() {
                    break;
                }
                if let Some(delay) = self.delay {
                    std::thread::sleep(delay);
                }
                let ts = (now_ms() / 1000) as u32;
                let msg = heartbeat::seal_heartbeat_bridge(
                    &self.key,
                    self.counter,
                    ts,
                    &chal,
                    &[0x09u8; 16],
                );
                self.counter += 1;
                if stream
                    .write_all(&(msg.len() as u32).to_le_bytes())
                    .and_then(|_| stream.write_all(&msg))
                    .is_err()
                {
                    break;
                }
            }
        }
    }

    fn spawn_fake_bridge(
        listener: std::net::TcpListener,
        key: [u8; 32],
        counter: u32,
        delay: Option<Duration>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            FakeBridge {
                key,
                counter,
                delay,
            }
            .serve(stream);
        })
    }

    #[test]
    fn bridge_valid_heartbeat_unlocks() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let key = [0x11u8; 32];
        let server = spawn_fake_bridge(listener, key, 1, None);
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
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let key = [0x11u8; 32];
        let server = spawn_fake_bridge(listener, key, 1, Some(Duration::from_millis(50)));
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
    fn fake_bridge_serve_breaks_on_truncated_challenge_and_failed_response_write() {
        use std::io::Write;
        use std::net::{TcpListener, TcpStream};
        use std::os::fd::AsRawFd;

        // Peer announces a challenge body it never finishes sending: the fake
        // bridge must drop the stream instead of blocking forever.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            FakeBridge {
                key: [0x11; 32],
                counter: 1,
                delay: None,
            }
            .serve(stream);
        });
        let mut client = TcpStream::connect(&addr).unwrap();
        client.write_all(&16u32.to_le_bytes()).unwrap();
        client.write_all(&[0u8; 5]).unwrap();
        client.flush().unwrap();
        drop(client);
        server.join().unwrap();

        // Peer sends a complete challenge, then aborts the connection (RST on
        // close via SO_LINGER 0) while the server is delayed, so its response
        // write fails and it must stop serving.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            FakeBridge {
                key: [0x11; 32],
                counter: 1,
                delay: Some(Duration::from_millis(100)),
            }
            .serve(stream);
        });
        let mut client = TcpStream::connect(&addr).unwrap();
        LinuxSys
            .setsockopt(
                client.as_raw_fd(),
                hci::SOL_SOCKET,
                SO_LINGER,
                &[1u8, 0, 0, 0, 0, 0, 0, 0],
            )
            .unwrap();
        client.write_all(&16u32.to_le_bytes()).unwrap();
        client.write_all(&[0u8; 16]).unwrap();
        client.flush().unwrap();
        // Give the server time to read the full challenge before the RST, so
        // the failure happens on its response write rather than its read.
        std::thread::sleep(Duration::from_millis(50));
        drop(client);
        server.join().unwrap();
    }

    #[test]
    fn mock_bt_local_mac_reports_configured_address() {
        let mut f = fixture(vec![], vec![], false);
        assert_eq!(f.bt.local_mac().unwrap(), f.bt.local);
    }

    struct CliSys {
        fail_socket: bool,
        fail_timeout: bool,
        local_mac_response: bool,
        local_mac_returned: std::cell::Cell<bool>,
    }

    impl Default for CliSys {
        fn default() -> Self {
            Self {
                fail_socket: false,
                fail_timeout: false,
                local_mac_response: false,
                local_mac_returned: std::cell::Cell::new(false),
            }
        }
    }

    impl Sys for CliSys {
        fn socket(&self, _: i32, _: i32, _: i32) -> io::Result<i32> {
            if self.fail_socket {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "socket denied",
                ))
            } else {
                Ok(10)
            }
        }
        fn socketpair(&self, _: i32, _: i32, _: i32, _: &mut [i32; 2]) -> io::Result<()> {
            Ok(())
        }
        fn bind(&self, _: i32, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
        fn listen(&self, _: i32, _: i32) -> io::Result<()> {
            Ok(())
        }
        fn connect(&self, _: i32, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
        fn accept(&self, _: i32) -> io::Result<i32> {
            Ok(11)
        }
        fn read(&self, _: i32, buf: &mut [u8]) -> io::Result<usize> {
            if self.local_mac_response && !self.local_mac_returned.replace(true) {
                let opcode = crate::hci::OPCODE_READ_BD_ADDR.to_le_bytes();
                let event = [
                    crate::hci::HCI_EVENT_PKT,
                    crate::hci::EVT_CMD_COMPLETE,
                    10,
                    1,
                    opcode[0],
                    opcode[1],
                    0,
                    1,
                    2,
                    3,
                    4,
                    5,
                    6,
                ];
                buf[..event.len()].copy_from_slice(&event);
                Ok(event.len())
            } else {
                Err(io::Error::new(io::ErrorKind::WouldBlock, "no event"))
            }
        }
        fn write(&self, _: i32, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }
        fn close(&self, _: i32) -> io::Result<()> {
            Ok(())
        }
        fn ioctl(&self, _: i32, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::NotFound, "no connection"))
        }
        fn setsockopt(&self, _: i32, _: i32, _: i32, _: &[u8]) -> io::Result<()> {
            if self.fail_timeout {
                Err(io::Error::new(io::ErrorKind::InvalidInput, "bad timeout"))
            } else {
                Ok(())
            }
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    #[test]
    fn cli_sys_unused_trait_methods_return_defaults() {
        let sys = CliSys::default();
        let mut fds = [0; 2];
        sys.socketpair(1, 1, 0, &mut fds).unwrap();
        sys.listen(10, 1).unwrap();
        sys.connect(10, &[]).unwrap();
        assert_eq!(sys.accept(10).unwrap(), 11);
        let mut arg = [0u8; 16];
        assert_eq!(
            sys.ioctl(10, 0, &mut arg).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn run_cli_handles_help_parse_and_secure_mode_errors() {
        let sys = CliSys::default();
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run_cli("crabnnb", &args(&["--help"]), &sys, &mut out, &mut err),
            0
        );
        assert!(String::from_utf8(out).unwrap().contains("Usage:"));

        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run_cli("crabnnb", &args(&["bad-mac"]), &sys, &mut out, &mut err),
            1
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("invalid bluetooth address"));

        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&["AA:BB:CC:DD:EE:FF"]),
                &sys,
                &mut Vec::new(),
                &mut err
            ),
            1
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("--key-file is required"));

        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&["AA:BB:CC:DD:EE:FF", "--key-file", "/missing/crabnnb-key"]),
                &sys,
                &mut Vec::new(),
                &mut err,
            ),
            1
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("cannot load key file"));
    }

    #[test]
    fn run_cli_handles_key_generation_and_existing_file_errors() {
        let path = std::env::temp_dir().join(format!("crabnnb-run-key-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let sys = CliSys::default();
        let mut out = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&["--gen-key-file", path.to_str().unwrap()]),
                &sys,
                &mut out,
                &mut Vec::new(),
            ),
            0
        );
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("wrote new 32-byte key"));
        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&["--gen-key-file", path.to_str().unwrap()]),
                &sys,
                &mut Vec::new(),
                &mut err,
            ),
            1
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("cannot write key file"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn run_cli_exercises_bridge_and_hci_startup_failures() {
        let sys = CliSys {
            fail_socket: true,
            ..CliSys::default()
        };
        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&["AA:BB:CC:DD:EE:FF", "--allow-insecure"]),
                &sys,
                &mut Vec::new(),
                &mut err,
            ),
            2
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("cannot open HCI socket"));

        let sys = CliSys {
            fail_timeout: true,
            ..CliSys::default()
        };
        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&["AA:BB:CC:DD:EE:FF", "--allow-insecure"]),
                &sys,
                &mut Vec::new(),
                &mut err,
            ),
            2
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("cannot set receive timeout"));

        let sys = CliSys::default();
        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&[
                    "AA:BB:CC:DD:EE:FF",
                    "--allow-insecure",
                    "--bridge-addr",
                    "127.0.0.1:0",
                ]),
                &sys,
                &mut Vec::new(),
                &mut err,
            ),
            1
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("requires --key-file"));
    }

    #[test]
    fn run_cli_creates_secure_session_from_local_hci_address() {
        let key_path = std::env::temp_dir().join(format!(
            "crabnnb-run-local-address-key-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&key_path);
        crate::crypto::generate_key_file(&key_path).unwrap();

        let sys = CliSys {
            local_mac_response: true,
            ..CliSys::default()
        };
        let mut err = Vec::new();
        let result = run_cli(
            "crabnnb",
            &args(&[
                "AA:BB:CC:DD:EE:FF",
                "--key-file",
                key_path.to_str().unwrap(),
                "--once",
            ]),
            &sys,
            &mut Vec::new(),
            &mut err,
        );
        assert_eq!(result, 3);
        assert!(!String::from_utf8(err)
            .unwrap()
            .contains("cannot read local bluetooth address"));
        std::fs::remove_file(key_path).unwrap();
    }

    #[test]
    fn run_cli_connects_to_bridge_and_verifies_heartbeat() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let key_path =
            std::env::temp_dir().join(format!("crabnnb-run-bridge-key-{}", std::process::id()));
        let _ = std::fs::remove_file(&key_path);
        crate::crypto::generate_key_file(&key_path).unwrap();
        let key = crate::crypto::load_key(&key_path).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut header = [0u8; 4];
            stream.read_exact(&mut header).unwrap();
            let challenge_len = u32::from_le_bytes(header) as usize;
            let mut challenge = vec![0; challenge_len];
            stream.read_exact(&mut challenge).unwrap();
            let response = heartbeat::seal_heartbeat_bridge(
                &key,
                1,
                (now_ms() / 1000) as u32,
                &challenge,
                &[0x53; 16],
            );
            stream
                .write_all(&(response.len() as u32).to_le_bytes())
                .unwrap();
            stream.write_all(&response).unwrap();
        });

        let sys = CliSys::default();
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&[
                    "AA:BB:CC:DD:EE:FF",
                    "--key-file",
                    key_path.to_str().unwrap(),
                    "--bridge-addr",
                    &addr,
                    "--once",
                    "--dry-run",
                ]),
                &sys,
                &mut out,
                &mut err,
            ),
            0
        );
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("running unlock command"));
        assert!(err.is_empty());
        server.join().unwrap();

        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&[
                    "AA:BB:CC:DD:EE:FF",
                    "--key-file",
                    key_path.to_str().unwrap(),
                    "--bridge-addr",
                    "127.0.0.1:0",
                ]),
                &sys,
                &mut Vec::new(),
                &mut err,
            ),
            2
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("cannot connect to phone bridge"));
        std::fs::remove_file(key_path).unwrap();
    }

    #[test]
    fn run_cli_warns_in_insecure_mode_and_reports_missing_local_address() {
        let sys = CliSys::default();
        let mut out = Vec::new();
        let mut warning = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&["AA:BB:CC:DD:EE:FF", "--allow-insecure"]),
                &sys,
                &mut out,
                &mut warning,
            ),
            3
        );
        let output = String::from_utf8(out).unwrap();
        assert!(String::from_utf8(warning)
            .unwrap()
            .contains("WARNING: --allow-insecure"));
        assert!(output.contains("found 0 devices"));

        let path =
            std::env::temp_dir().join(format!("crabnnb-run-secure-key-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        crate::crypto::generate_key_file(&path).unwrap();
        let mut err = Vec::new();
        assert_eq!(
            run_cli(
                "crabnnb",
                &args(&["AA:BB:CC:DD:EE:FF", "--key-file", path.to_str().unwrap(),]),
                &sys,
                &mut Vec::new(),
                &mut err,
            ),
            2
        );
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("cannot read local bluetooth address"));
        std::fs::remove_file(path).unwrap();
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
