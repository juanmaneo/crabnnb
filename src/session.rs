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

    pub fn probe<B: BtOps + ?Sized>(&mut self, bt: &mut B) -> Result<ProbeOk, ProbeError> {
        let challenge = crypto::random_vec(heartbeat::CHALLENGE_LEN).map_err(ProbeError::Io)?;
        let t0 = Instant::now();
        let resp = bt
            .secure_probe(&challenge)
            .map_err(ProbeError::Io)?
            .ok_or(ProbeError::NoResponse)?;
        let rtt = t0.elapsed();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ProbeError::Io(io::Error::new(io::ErrorKind::Other, e)))?
            .as_millis() as u64;
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
    fn test_probe_valid_heartbeat() {
        let (mut bt, mut session) = fixture(vec![MockStep::Valid {
            counter: 1,
            ts_ms: now_ms(),
        }]);
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
        match session.probe(&mut bt) {
            Err(ProbeError::TooSlow(rtt)) => assert!(rtt >= Duration::from_millis(50)),
            other => panic!("expected TooSlow, got {other:?}"),
        }
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
        match session.probe(&mut bt) {
            Err(ProbeError::Verify(HeartbeatError::Replay)) => {}
            other => panic!("expected replay error, got {other:?}"),
        }
    }

    #[test]
    fn test_probe_garbage_rejected() {
        let (mut bt, mut session) = fixture(vec![MockStep::Raw(vec![0u8; heartbeat::MESSAGE_LEN])]);
        match session.probe(&mut bt) {
            Err(ProbeError::Verify(HeartbeatError::InvalidTag)) => {}
            other => panic!("expected invalid tag, got {other:?}"),
        }
    }

    #[test]
    fn test_probe_no_response() {
        let (mut bt, mut session) = fixture(vec![MockStep::None]);
        match session.probe(&mut bt) {
            Err(ProbeError::NoResponse) => {}
            other => panic!("expected no response, got {other:?}"),
        }
    }

    #[test]
    fn test_probe_wrong_key_rejected() {
        let (mut bt, mut session) = fixture(vec![MockStep::Valid {
            counter: 1,
            ts_ms: now_ms(),
        }]);
        session.key = [0x99u8; crypto::KEY_LEN];
        match session.probe(&mut bt) {
            Err(ProbeError::Verify(HeartbeatError::InvalidTag)) => {}
            other => panic!("expected invalid tag, got {other:?}"),
        }
    }
}
