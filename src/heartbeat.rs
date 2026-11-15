use std::time::Duration;

use crate::crypto::{self, KEY_LEN, NONCE_LEN};
use crate::mac::Mac;

pub const CHALLENGE_LEN: usize = 16;
pub const COUNTER_LEN: usize = 8;
pub const TIMESTAMP_LEN: usize = 8;
pub const PLAINTEXT_LEN: usize = COUNTER_LEN + TIMESTAMP_LEN + CHALLENGE_LEN;
pub const TAG_LEN: usize = 16;
pub const MESSAGE_LEN: usize = NONCE_LEN + PLAINTEXT_LEN + TAG_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heartbeat {
    pub counter: u64,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatError {
    Malformed,
    InvalidTag,
    ChallengeMismatch,
    Replay,
    Stale,
}

impl std::fmt::Display for HeartbeatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeartbeatError::Malformed => write!(f, "malformed heartbeat message"),
            HeartbeatError::InvalidTag => write!(
                f,
                "authentication tag mismatch (wrong key or tampered message)"
            ),
            HeartbeatError::ChallengeMismatch => {
                write!(f, "heartbeat does not echo the outstanding challenge")
            }
            HeartbeatError::Replay => write!(f, "counter not increasing (replayed heartbeat)"),
            HeartbeatError::Stale => write!(f, "heartbeat timestamp outside freshness window"),
        }
    }
}

impl std::error::Error for HeartbeatError {}

pub fn aad(watch: &Mac, local: &Mac) -> Vec<u8> {
    let mut v = Vec::with_capacity(12);
    v.extend_from_slice(&watch.bytes());
    v.extend_from_slice(&local.bytes());
    v
}

pub fn seal_heartbeat(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    counter: u64,
    timestamp_ms: u64,
    challenge: &[u8],
    nonce: &[u8; NONCE_LEN],
) -> Vec<u8> {
    let mut pt = Vec::with_capacity(PLAINTEXT_LEN);
    pt.extend_from_slice(&counter.to_le_bytes());
    pt.extend_from_slice(&timestamp_ms.to_le_bytes());
    pt.extend_from_slice(challenge);
    let ct = crypto::seal(key, nonce, aad, &pt);
    let mut msg = Vec::with_capacity(MESSAGE_LEN);
    msg.extend_from_slice(nonce);
    msg.extend_from_slice(&ct);
    msg
}

pub fn open_heartbeat(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    msg: &[u8],
    expected_challenge: &[u8],
    last_counter: u64,
    now_ms: u64,
    window: Duration,
) -> Result<Heartbeat, HeartbeatError> {
    if msg.len() != MESSAGE_LEN || expected_challenge.len() != CHALLENGE_LEN {
        return Err(HeartbeatError::Malformed);
    }
    let nonce: &[u8; NONCE_LEN] = msg[..NONCE_LEN].try_into().unwrap();
    let ct = &msg[NONCE_LEN..];
    let pt = crypto::open(key, nonce, aad, ct).map_err(|_| HeartbeatError::InvalidTag)?;
    let counter = u64::from_le_bytes(pt[..COUNTER_LEN].try_into().unwrap());
    let timestamp_ms = u64::from_le_bytes(
        pt[COUNTER_LEN..COUNTER_LEN + TIMESTAMP_LEN]
            .try_into()
            .unwrap(),
    );
    let echo = &pt[COUNTER_LEN + TIMESTAMP_LEN..];
    if echo != expected_challenge {
        return Err(HeartbeatError::ChallengeMismatch);
    }
    if counter <= last_counter {
        return Err(HeartbeatError::Replay);
    }
    let window_ms = window.as_millis().min(u128::from(u64::MAX)) as u64;
    if now_ms.abs_diff(timestamp_ms) > window_ms {
        return Err(HeartbeatError::Stale);
    }
    Ok(Heartbeat {
        counter,
        timestamp_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; KEY_LEN] {
        [0x42u8; KEY_LEN]
    }

    fn watch() -> Mac {
        Mac::parse("AA:BB:CC:DD:EE:FF").unwrap()
    }

    fn local() -> Mac {
        Mac::parse("11:22:33:44:55:66").unwrap()
    }

    fn challenge() -> Vec<u8> {
        (0u8..CHALLENGE_LEN as u8).collect()
    }

    fn nonce() -> [u8; NONCE_LEN] {
        [0x09u8; NONCE_LEN]
    }

    #[test]
    fn test_message_len() {
        assert_eq!(MESSAGE_LEN, 12 + 32 + 16);
    }

    #[test]
    fn test_roundtrip() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let msg = seal_heartbeat(&key(), &aad, 7, 1_700_000_000_000, &chal, &nonce());
        assert_eq!(msg.len(), MESSAGE_LEN);
        let hb = open_heartbeat(
            &key(),
            &aad,
            &msg,
            &chal,
            6,
            1_700_000_000_000,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(hb.counter, 7);
        assert_eq!(hb.timestamp_ms, 1_700_000_000_000);
    }

    #[test]
    fn test_wrong_key_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let msg = seal_heartbeat(&key(), &aad, 1, 1000, &chal, &nonce());
        let err = open_heartbeat(
            &[0x43u8; KEY_LEN],
            &aad,
            &msg,
            &chal,
            0,
            1000,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::InvalidTag);
    }

    #[test]
    fn test_tampered_message_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let mut msg = seal_heartbeat(&key(), &aad, 1, 1000, &chal, &nonce());
        msg[MESSAGE_LEN - 1] ^= 0x80;
        let err =
            open_heartbeat(&key(), &aad, &msg, &chal, 0, 1000, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, HeartbeatError::InvalidTag);
    }

    #[test]
    fn test_wrong_aad_rejected() {
        let aad_bytes = aad(&watch(), &local());
        let other = aad(&local(), &watch());
        let chal = challenge();
        let msg = seal_heartbeat(&key(), &aad_bytes, 1, 1000, &chal, &nonce());
        let err = open_heartbeat(&key(), &other, &msg, &chal, 0, 1000, Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(err, HeartbeatError::InvalidTag);
    }

    #[test]
    fn test_wrong_challenge_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let other_chal: Vec<u8> = (100u8..100 + CHALLENGE_LEN as u8).collect();
        let msg = seal_heartbeat(&key(), &aad, 1, 1000, &chal, &nonce());
        let err = open_heartbeat(
            &key(),
            &aad,
            &msg,
            &other_chal,
            0,
            1000,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::ChallengeMismatch);
    }

    #[test]
    fn test_replay_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let msg = seal_heartbeat(&key(), &aad, 5, 1000, &chal, &nonce());
        let err =
            open_heartbeat(&key(), &aad, &msg, &chal, 5, 1000, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, HeartbeatError::Replay);
        let err =
            open_heartbeat(&key(), &aad, &msg, &chal, 9, 1000, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, HeartbeatError::Replay);
    }

    #[test]
    fn test_stale_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let now = 1_000_000u64;
        let msg = seal_heartbeat(&key(), &aad, 1, now - 6_000, &chal, &nonce());
        let err =
            open_heartbeat(&key(), &aad, &msg, &chal, 0, now, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, HeartbeatError::Stale);
    }

    #[test]
    fn test_future_timestamp_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let now = 1_000_000u64;
        let msg = seal_heartbeat(&key(), &aad, 1, now + 6_000, &chal, &nonce());
        let err =
            open_heartbeat(&key(), &aad, &msg, &chal, 0, now, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, HeartbeatError::Stale);
    }

    #[test]
    fn test_window_boundary_accepted() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let now = 1_000_000u64;
        let msg = seal_heartbeat(&key(), &aad, 1, now - 5_000, &chal, &nonce());
        let hb = open_heartbeat(&key(), &aad, &msg, &chal, 0, now, Duration::from_secs(5)).unwrap();
        assert_eq!(hb.counter, 1);
    }

    #[test]
    fn test_malformed_length_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let err = open_heartbeat(
            &key(),
            &aad,
            &[0u8; 10],
            &chal,
            0,
            0,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::Malformed);
    }
}
