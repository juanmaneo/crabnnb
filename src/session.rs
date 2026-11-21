use std::io;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::crypto;
use crate::device::BtOps;
use crate::heartbeat::{self, Heartbeat, HeartbeatError};
use crate::mac::Mac;

#[derive(Debug)]
pub enum ProbeError {
    Io(io::Error),
    NoResponse,
    Verify(HeartbeatError),
    TooSlow(Duration),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::Io(e) => write!(f, "transport error: {e}"),
            ProbeError::NoResponse => write!(f, "no heartbeat response from device"),
            ProbeError::Verify(e) => write!(f, "heartbeat verification failed: {e}"),
            ProbeError::TooSlow(rtt) => write!(
                f,
                "challenge round-trip {rtt:?} exceeds the configured latency limit"
            ),
        }
    }
}

impl std::error::Error for ProbeError {}

fn unix_millis(now: SystemTime) -> io::Result<u64> {
    now.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

#[derive(Debug, Clone, Copy)]
pub struct ProbeOk {
    pub heartbeat: Heartbeat,
    pub rtt: Duration,
}

pub struct SecureSession {
    key: [u8; crypto::KEY_LEN],
    aad: Vec<u8>,
    last_counter: u64,
    window: Duration,
    max_roundtrip: Duration,
}

impl SecureSession {
    pub fn new(
        key: [u8; crypto::KEY_LEN],
        watch: Mac,
        local: Mac,
        window: Duration,
        max_roundtrip: Duration,
    ) -> Self {
        SecureSession {
            key,
            aad: heartbeat::aad(&watch, &local),
            last_counter: 0,
            window,
            max_roundtrip,
        }
    }

    pub fn last_counter(&self) -> u64 {
        self.last_counter
    }

    pub fn set_max_roundtrip(&mut self, d: Duration) {
        self.max_roundtrip = d;
    }

    pub fn probe(&mut self, bt: &mut dyn BtOps) -> Result<ProbeOk, ProbeError> {
        let mut challenge_source = || crypto::random_vec(heartbeat::CHALLENGE_LEN);
        self.probe_with_challenge_source(bt, &mut challenge_source)
    }

    fn probe_with_challenge_source(
        &mut self,
        bt: &mut dyn BtOps,
        challenge_source: &mut dyn FnMut() -> io::Result<Vec<u8>>,
    ) -> Result<ProbeOk, ProbeError> {
        let mut now_source = SystemTime::now;
        self.probe_with_sources(bt, challenge_source, &mut now_source)
    }

    fn probe_with_sources(
        &mut self,
        bt: &mut dyn BtOps,
        challenge_source: &mut dyn FnMut() -> io::Result<Vec<u8>>,
        now_source: &mut dyn FnMut() -> SystemTime,
    ) -> Result<ProbeOk, ProbeError> {
        let challenge = challenge_source().map_err(ProbeError::Io)?;
        let t0 = Instant::now();
        let resp = bt
            .secure_probe(&challenge)
            .map_err(ProbeError::Io)?
            .ok_or(ProbeError::NoResponse)?;
        let rtt = t0.elapsed();
        let now_ms = unix_millis(now_source()).map_err(ProbeError::Io)?;
        let hb = heartbeat::open_heartbeat(
            &self.key,
            &self.aad,
            &resp,
            &challenge,
            self.last_counter,
            now_ms,
            self.window,
        )
        .map_err(ProbeError::Verify)?;
        if self.max_roundtrip > Duration::ZERO && rtt > self.max_roundtrip {
            return Err(ProbeError::TooSlow(rtt));
        }
        self.last_counter = hb.counter;
        Ok(ProbeOk { heartbeat: hb, rtt })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::NONCE_LEN;
    use std::collections::VecDeque;

    struct MockBt {
        key: [u8; crypto::KEY_LEN],
        watch: Mac,
        local: Mac,
        steps: VecDeque<MockStep>,
        probe_delay: Duration,
    }

    enum MockStep {
        Valid { counter: u64, ts_ms: u64 },
        Raw(Vec<u8>),
        None,
        IoError,
    }

    impl BtOps for MockBt {
        fn discover(&mut self) -> io::Result<usize> {
            Ok(1)
        }
        fn lookup_name(&mut self, _mac: &Mac) -> io::Result<Option<String>> {
            Ok(Some("watch".to_string()))
        }
        fn distance_approximation(&mut self) -> f64 {
            0.5
        }
        fn local_mac(&mut self) -> io::Result<Mac> {
            Ok(self.local)
        }
        fn secure_probe(&mut self, challenge: &[u8]) -> io::Result<Option<Vec<u8>>> {
            if self.probe_delay > Duration::ZERO {
                std::thread::sleep(self.probe_delay);
            }
            match self.steps.pop_front().unwrap_or(MockStep::None) {
                MockStep::Valid { counter, ts_ms } => {
                    let nonce: [u8; NONCE_LEN] = [0x77u8; NONCE_LEN];
                    let aad = heartbeat::aad(&self.watch, &self.local);
                    Ok(Some(heartbeat::seal_heartbeat(
                        &self.key, &aad, counter, ts_ms, challenge, &nonce,
                    )))
                }
                MockStep::Raw(b) => Ok(Some(b)),
                MockStep::None => Ok(None),
                MockStep::IoError => Err(io::Error::new(io::ErrorKind::BrokenPipe, "probe failed")),
            }
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn fixture(steps: Vec<MockStep>) -> (MockBt, SecureSession) {
        let key = [0x42u8; crypto::KEY_LEN];
        let watch = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        let local = Mac::parse("11:22:33:44:55:66").unwrap();
        let bt = MockBt {
            key,
            watch,
            local,
            steps: steps.into(),
            probe_delay: Duration::ZERO,
        };
        let session = SecureSession::new(
            key,
            watch,
            local,
            Duration::from_secs(5),
            Duration::from_millis(250),
        );
        (bt, session)
    }

    #[test]
    fn test_probe_io_error_and_pre_epoch_clock_error() {
        let (mut bt, mut session) = fixture(vec![]);
        let mut unavailable_random = || {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "random unavailable",
            ))
        };
        let result = session.probe_with_challenge_source(&mut bt, &mut unavailable_random);
        assert!(
            matches!(result, Err(ProbeError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied)
        );

        let (mut bt, mut session) = fixture(vec![MockStep::IoError]);
        assert!(matches!(
            session.probe(&mut bt),
            Err(ProbeError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));

        let before_epoch = UNIX_EPOCH - Duration::from_secs(1);
        assert!(unix_millis(before_epoch).is_err());

        let (mut bt, mut session) = fixture(vec![MockStep::Valid {
            counter: 1,
            ts_ms: now_ms(),
        }]);
        let mut challenge_source = || Ok(vec![0x33; heartbeat::CHALLENGE_LEN]);
        let mut clock_source = || before_epoch;
        let result = session.probe_with_sources(&mut bt, &mut challenge_source, &mut clock_source);
        assert!(
            matches!(result, Err(ProbeError::Io(error)) if error.kind() == io::ErrorKind::Other)
        );
    }

    #[test]
    fn test_probe_error_display_formats_all_variants() {
        let io_error = ProbeError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        assert_eq!(io_error.to_string(), "transport error: closed");
        assert_eq!(
            ProbeError::NoResponse.to_string(),
            "no heartbeat response from device"
        );
        assert!(ProbeError::Verify(HeartbeatError::Replay)
            .to_string()
            .contains("replayed heartbeat"));
        assert!(ProbeError::TooSlow(Duration::from_millis(1))
            .to_string()
            .contains("exceeds the configured latency limit"));
    }

    #[test]
    fn test_probe_valid_heartbeat() {
        let (mut bt, mut session) = fixture(vec![MockStep::Valid {
            counter: 1,
            ts_ms: now_ms(),
        }]);
        assert_eq!(bt.discover().unwrap(), 1);
        assert_eq!(
            bt.lookup_name(&Mac::parse("AA:BB:CC:DD:EE:FF").unwrap())
                .unwrap()
                .as_deref(),
            Some("watch")
        );
        assert_eq!(bt.distance_approximation(), 0.5);
        assert_eq!(
            bt.local_mac().unwrap(),
            Mac::parse("11:22:33:44:55:66").unwrap()
        );
        let ok = session.probe(&mut bt).unwrap();
        assert_eq!(ok.heartbeat.counter, 1);
        assert_eq!(session.last_counter(), 1);
    }

    #[test]
    fn test_probe_slow_roundtrip_rejected_as_relay() {
        let (mut bt, mut session) = fixture(vec![MockStep::Valid {
            counter: 1,
            ts_ms: now_ms(),
        }]);
        bt.probe_delay = Duration::from_millis(50);
        session.set_max_roundtrip(Duration::from_millis(5));
        assert!(matches!(
            session.probe(&mut bt),
            Err(ProbeError::TooSlow(rtt)) if rtt >= Duration::from_millis(50)
        ));
    }

    #[test]
    fn test_probe_roundtrip_bound_zero_disables_check() {
        let (mut bt, mut session) = fixture(vec![MockStep::Valid {
            counter: 1,
            ts_ms: now_ms(),
        }]);
        bt.probe_delay = Duration::from_millis(30);
        session.set_max_roundtrip(Duration::ZERO);
        assert!(session.probe(&mut bt).is_ok());
    }

    #[test]
    fn test_probe_counter_must_increase() {
        let (mut bt, mut session) = fixture(vec![
            MockStep::Valid {
                counter: 5,
                ts_ms: now_ms(),
            },
            MockStep::Valid {
                counter: 5,
                ts_ms: now_ms(),
            },
        ]);
        assert!(session.probe(&mut bt).is_ok());
        assert!(matches!(
            session.probe(&mut bt),
            Err(ProbeError::Verify(HeartbeatError::Replay))
        ));
    }

    #[test]
    fn test_probe_garbage_rejected() {
        let (mut bt, mut session) = fixture(vec![MockStep::Raw(vec![0u8; heartbeat::MESSAGE_LEN])]);
        assert!(matches!(
            session.probe(&mut bt),
            Err(ProbeError::Verify(HeartbeatError::InvalidTag))
        ));
    }

    #[test]
    fn test_probe_no_response() {
        let (mut bt, mut session) = fixture(vec![MockStep::None]);
        assert!(matches!(
            session.probe(&mut bt),
            Err(ProbeError::NoResponse)
        ));
    }

    #[test]
    fn test_probe_wrong_key_rejected() {
        let (mut bt, mut session) = fixture(vec![MockStep::Valid {
            counter: 1,
            ts_ms: now_ms(),
        }]);
        session.key = [0x99u8; crypto::KEY_LEN];
        assert!(matches!(
            session.probe(&mut bt),
            Err(ProbeError::Verify(HeartbeatError::InvalidTag))
        ));
    }
}
