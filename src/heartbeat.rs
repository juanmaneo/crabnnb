use std::time::Duration;

use crate::crypto::{self, KEY_LEN, NONCE_LEN};
use crate::mac::Mac;

pub const CHALLENGE_LEN: usize = 16;
pub const COUNTER_LEN: usize = 8;
pub const TIMESTAMP_LEN: usize = 8;
pub const PLAINTEXT_LEN: usize = COUNTER_LEN + TIMESTAMP_LEN + CHALLENGE_LEN;
pub const TAG_LEN: usize = 16;
pub const MESSAGE_LEN: usize = NONCE_LEN + PLAINTEXT_LEN + TAG_LEN;

pub const MODE_AEAD: u8 = 0x00;
pub const MODE_AES_CBC_HMAC: u8 = 0x01;
pub const MESSAGE_LEN_V2: usize = 1 + MESSAGE_LEN;
pub const BRIDGE_PLAINTEXT_LEN: usize = 4 + 4 + CHALLENGE_LEN;
pub const BRIDGE_MESSAGE_LEN: usize = 1 + 16 + 32 + 32;

pub fn mac_key(key: &[u8; KEY_LEN]) -> [u8; 32] {
    let mut v = Vec::with_capacity(11 + KEY_LEN);
    v.extend_from_slice(b"crabnnb-mac");
    v.extend_from_slice(key);
    crypto::sha256(&v)
}

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
    Stale { skew_ms: u64 },
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
            HeartbeatError::Stale { skew_ms } => write!(
                f,
                "heartbeat timestamp outside freshness window (skew {} s)",
                skew_ms / 1000
            ),
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

pub fn seal_heartbeat_v2(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    counter: u64,
    timestamp_ms: u64,
    challenge: &[u8],
    nonce: &[u8; NONCE_LEN],
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(MESSAGE_LEN_V2);
    msg.push(MODE_AEAD);
    msg.extend_from_slice(&seal_heartbeat(
        key,
        aad,
        counter,
        timestamp_ms,
        challenge,
        nonce,
    ));
    msg
}

pub fn seal_heartbeat_bridge(
    key: &[u8; KEY_LEN],
    counter: u32,
    timestamp_sec: u32,
    challenge: &[u8],
    iv: &[u8; 16],
) -> Vec<u8> {
    let mut pt = Vec::with_capacity(BRIDGE_PLAINTEXT_LEN);
    pt.extend_from_slice(&counter.to_le_bytes());
    pt.extend_from_slice(&timestamp_sec.to_le_bytes());
    pt.extend_from_slice(challenge);
    let ct = crypto::aes256_cbc_encrypt_raw(key, iv, &pt);
    let mut msg = Vec::with_capacity(BRIDGE_MESSAGE_LEN);
    msg.push(MODE_AES_CBC_HMAC);
    msg.extend_from_slice(iv);
    msg.extend_from_slice(&ct);
    let mac = crypto::hmac_sha256(&mac_key(key), &msg);
    msg.extend_from_slice(&mac);
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
    if expected_challenge.len() != CHALLENGE_LEN {
        return Err(HeartbeatError::Malformed);
    }
    let (counter, timestamp_ms, echo) = match msg.len() {
        MESSAGE_LEN => {
            let nonce: &[u8; NONCE_LEN] = msg[..NONCE_LEN].try_into().unwrap();
            let pt = crypto::open(key, nonce, aad, &msg[NONCE_LEN..])
                .map_err(|_| HeartbeatError::InvalidTag)?;
            let counter = u64::from_le_bytes(pt[..COUNTER_LEN].try_into().unwrap());
            let ts = u64::from_le_bytes(
                pt[COUNTER_LEN..COUNTER_LEN + TIMESTAMP_LEN]
                    .try_into()
                    .unwrap(),
            );
            (counter, ts, pt[COUNTER_LEN + TIMESTAMP_LEN..].to_vec())
        }
        MESSAGE_LEN_V2 if msg[0] == MODE_AEAD => {
            let nonce: &[u8; NONCE_LEN] = msg[1..1 + NONCE_LEN].try_into().unwrap();
            let pt = crypto::open(key, nonce, aad, &msg[1 + NONCE_LEN..])
                .map_err(|_| HeartbeatError::InvalidTag)?;
            let counter = u64::from_le_bytes(pt[..COUNTER_LEN].try_into().unwrap());
            let ts = u64::from_le_bytes(
                pt[COUNTER_LEN..COUNTER_LEN + TIMESTAMP_LEN]
                    .try_into()
                    .unwrap(),
            );
            (counter, ts, pt[COUNTER_LEN + TIMESTAMP_LEN..].to_vec())
        }
        BRIDGE_MESSAGE_LEN if msg[0] == MODE_AES_CBC_HMAC => {
            let iv: &[u8; 16] = msg[1..17].try_into().unwrap();
            let mac = &msg[BRIDGE_MESSAGE_LEN - 32..];
            let expected_mac = crypto::hmac_sha256(&mac_key(key), &msg[..BRIDGE_MESSAGE_LEN - 32]);
            if !crypto::constant_time_eq(mac, &expected_mac) {
                return Err(HeartbeatError::InvalidTag);
            }
            let pt = crypto::aes256_cbc_decrypt_raw(key, iv, &msg[17..BRIDGE_MESSAGE_LEN - 32])
                .map_err(|_| HeartbeatError::InvalidTag)?;
            if pt.len() != BRIDGE_PLAINTEXT_LEN {
                return Err(HeartbeatError::Malformed);
            }
            let counter = u32::from_le_bytes(pt[..4].try_into().unwrap()) as u64;
            let ts_sec = u32::from_le_bytes(pt[4..8].try_into().unwrap()) as u64;
            (counter, ts_sec * 1000, pt[8..].to_vec())
        }
        _ => return Err(HeartbeatError::Malformed),
    };
    if echo != expected_challenge {
        return Err(HeartbeatError::ChallengeMismatch);
    }
    if counter <= last_counter {
        return Err(HeartbeatError::Replay);
    }
    let window_ms = window.as_millis().min(u128::from(u64::MAX)) as u64;
    let skew_ms = now_ms.abs_diff(timestamp_ms);
    if skew_ms > window_ms {
        return Err(HeartbeatError::Stale { skew_ms });
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
    fn test_heartbeat_error_display() {
        assert_eq!(
            HeartbeatError::Malformed.to_string(),
            "malformed heartbeat message"
        );
        assert!(HeartbeatError::InvalidTag
            .to_string()
            .contains("authentication tag mismatch"));
        assert!(HeartbeatError::ChallengeMismatch
            .to_string()
            .contains("outstanding challenge"));
        assert!(HeartbeatError::Replay
            .to_string()
            .contains("replayed heartbeat"));
        assert!(HeartbeatError::Stale { skew_ms: 6_000 }
            .to_string()
            .contains("skew 6 s"));
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
        assert_eq!(err, HeartbeatError::Stale { skew_ms: 6_000 });
    }

    #[test]
    fn test_future_timestamp_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let now = 1_000_000u64;
        let msg = seal_heartbeat(&key(), &aad, 1, now + 6_000, &chal, &nonce());
        let err =
            open_heartbeat(&key(), &aad, &msg, &chal, 0, now, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, HeartbeatError::Stale { skew_ms: 6_000 });
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

    #[test]
    fn test_v2_mode_roundtrip() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let msg = seal_heartbeat_v2(&key(), &aad, 3, 1_700_000_000_000, &chal, &nonce());
        assert_eq!(msg.len(), MESSAGE_LEN_V2);
        assert_eq!(msg[0], MODE_AEAD);
        let hb = open_heartbeat(
            &key(),
            &aad,
            &msg,
            &chal,
            2,
            1_700_000_000_000,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(hb.counter, 3);
    }

    #[test]
    fn test_rejects_malformed_challenge_and_authenticated_bridge_payloads() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let nonce = nonce();
        let mut v2 = seal_heartbeat_v2(&key(), &aad, 1, 1_000, &chal, &nonce);
        let last = v2.len() - 1;
        v2[last] ^= 1;
        assert_eq!(
            open_heartbeat(&key(), &aad, &v2, &chal, 0, 1_000, Duration::from_secs(5)).unwrap_err(),
            HeartbeatError::InvalidTag
        );
        let valid = seal_heartbeat(&key(), &aad, 1, 1_000, &chal, &nonce);
        assert_eq!(
            open_heartbeat(&key(), &aad, &valid, &[], 0, 1_000, Duration::from_secs(5))
                .unwrap_err(),
            HeartbeatError::Malformed
        );

        let iv = [0x05u8; 16];
        let mut bad_padding = seal_heartbeat_bridge(&key(), 1, 1, &chal, &iv);
        bad_padding[17 + 15] ^= 1;
        let tag_start = bad_padding.len() - 32;
        let tag = crypto::hmac_sha256(&mac_key(&key()), &bad_padding[..tag_start]);
        bad_padding[tag_start..].copy_from_slice(&tag);
        assert_eq!(
            open_heartbeat(
                &key(),
                &aad,
                &bad_padding,
                &chal,
                0,
                1_000,
                Duration::from_secs(5)
            )
            .unwrap_err(),
            HeartbeatError::InvalidTag
        );

        let mut short_plaintext = vec![MODE_AES_CBC_HMAC];
        short_plaintext.extend_from_slice(&iv);
        short_plaintext.extend_from_slice(&crypto::aes256_cbc_encrypt_raw(&key(), &iv, &[0; 16]));
        let tag = crypto::hmac_sha256(&mac_key(&key()), &short_plaintext);
        short_plaintext.extend_from_slice(&tag);
        assert_eq!(short_plaintext.len(), BRIDGE_MESSAGE_LEN);
        assert_eq!(
            open_heartbeat(
                &key(),
                &aad,
                &short_plaintext,
                &chal,
                0,
                1_000,
                Duration::from_secs(5)
            )
            .unwrap_err(),
            HeartbeatError::Malformed
        );
    }

    #[test]
    fn test_bridge_mode_roundtrip() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let iv = [0x05u8; 16];
        let msg = seal_heartbeat_bridge(&key(), 9, 1_700_000_000, &chal, &iv);
        assert_eq!(msg.len(), BRIDGE_MESSAGE_LEN);
        assert_eq!(msg[0], MODE_AES_CBC_HMAC);
        let now_ms = 1_700_000_000_000u64;
        let hb =
            open_heartbeat(&key(), &aad, &msg, &chal, 8, now_ms, Duration::from_secs(5)).unwrap();
        assert_eq!(hb.counter, 9);
        assert_eq!(hb.timestamp_ms, 1_700_000_000_000);
    }

    #[test]
    fn test_bridge_wrong_key_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let msg = seal_heartbeat_bridge(&key(), 1, 1_000_000, &chal, &[0x05u8; 16]);
        let err = open_heartbeat(
            &[0x43u8; KEY_LEN],
            &aad,
            &msg,
            &chal,
            0,
            1_000_000_000,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::InvalidTag);
    }

    #[test]
    fn test_bridge_tampered_mac_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let mut msg = seal_heartbeat_bridge(&key(), 1, 1_000_000, &chal, &[0x05u8; 16]);
        let last = msg.len() - 1;
        msg[last] ^= 0x01;
        let err = open_heartbeat(
            &key(),
            &aad,
            &msg,
            &chal,
            0,
            1_000_000_000,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::InvalidTag);
    }

    #[test]
    fn test_bridge_tampered_ct_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let mut msg = seal_heartbeat_bridge(&key(), 1, 1_000_000, &chal, &[0x05u8; 16]);
        msg[20] ^= 0x01;
        let err = open_heartbeat(
            &key(),
            &aad,
            &msg,
            &chal,
            0,
            1_000_000_000,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::InvalidTag);
    }

    #[test]
    fn test_bridge_replay_and_stale_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let msg = seal_heartbeat_bridge(&key(), 7, 1_000_000, &chal, &[0x05u8; 16]);
        let err = open_heartbeat(
            &key(),
            &aad,
            &msg,
            &chal,
            7,
            1_000_000_000,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::Replay);
        let err = open_heartbeat(
            &key(),
            &aad,
            &msg,
            &chal,
            0,
            1_000_000_000 + 6_000,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::Stale { skew_ms: 6_000 });
    }

    #[test]
    fn test_bridge_wrong_challenge_rejected() {
        let aad = aad(&watch(), &local());
        let chal = challenge();
        let other: Vec<u8> = (200u8..200 + CHALLENGE_LEN as u8).collect();
        let msg = seal_heartbeat_bridge(&key(), 1, 1_000_000, &chal, &[0x05u8; 16]);
        let err = open_heartbeat(
            &key(),
            &aad,
            &msg,
            &other,
            0,
            1_000_000_000,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(err, HeartbeatError::ChallengeMismatch);
    }
}
