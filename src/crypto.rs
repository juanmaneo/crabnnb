use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use aes::Aes256;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type Aes256CbcEnc = cbc::Encryptor<Aes256>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;
type HmacSha256 = Hmac<Sha256>;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    InvalidTag,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::InvalidTag => write!(f, "authentication tag mismatch"),
        }
    }
}

impl std::error::Error for CryptoError {}

fn cipher(key: &[u8; KEY_LEN]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(key))
}

pub fn seal(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    cipher(key)
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("ChaCha20-Poly1305 encryption is infallible")
}

pub fn open(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    cipher(key)
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::InvalidTag)
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn aes256_cbc_encrypt_raw(key: &[u8; KEY_LEN], iv: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
    let mut buf = plaintext.to_vec();
    buf.resize(buf.len() + 16, 0);
    let out = Aes256CbcEnc::new(key.into(), iv.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
        .expect("buffer is large enough for padding");
    out.to_vec()
}

pub fn aes256_cbc_decrypt_raw(
    key: &[u8; KEY_LEN],
    iv: &[u8; 16],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut buf = ciphertext.to_vec();
    let out = Aes256CbcDec::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| CryptoError::InvalidTag)?;
    Ok(out.to_vec())
}

use cbc::cipher::block_padding::Pkcs7;

fn random_bytes_with(
    buf: &mut [u8],
    open: &mut dyn FnMut() -> io::Result<Box<dyn Read>>,
) -> io::Result<()> {
    let mut reader = open()?;
    reader.read_exact(buf)
}

pub fn random_bytes(buf: &mut [u8]) -> io::Result<()> {
    let mut open = || fs::File::open("/dev/urandom").map(|file| Box::new(file) as Box<dyn Read>);
    random_bytes_with(buf, &mut open)
}

fn random_vec_with(
    n: usize,
    fill: &mut dyn FnMut(&mut [u8]) -> io::Result<()>,
) -> io::Result<Vec<u8>> {
    let mut v = vec![0u8; n];
    fill(&mut v)?;
    Ok(v)
}

pub fn random_vec(n: usize) -> io::Result<Vec<u8>> {
    random_vec_with(n, &mut random_bytes)
}

pub fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn from_hex(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err("odd number of hex digits".to_string());
    }
    let bytes = cleaned.as_bytes();
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = hex_val(pair[0])?;
        let lo = hex_val(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_val(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("invalid hex digit '{}'", c as char)),
    }
}

pub fn load_key(path: &Path) -> io::Result<[u8; KEY_LEN]> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "key file {} must not be accessible by group/others (mode {mode:04o}, expected 0600)",
                    path.display()
                ),
            ));
        }
    }
    let text = fs::read_to_string(path)?;
    let bytes = from_hex(&text).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("key file {}: {e}", path.display()),
        )
    })?;
    let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "key file {} must contain exactly {KEY_LEN} bytes (64 hex chars)",
                path.display()
            ),
        )
    })?;
    Ok(key)
}

fn write_key(writer: &mut dyn Write, key: &[u8; KEY_LEN]) -> io::Result<()> {
    writer.write_all(to_hex(key).as_bytes())?;
    writer.write_all(b"\n")
}

fn generate_key_file_with_random(
    path: &Path,
    fill: &mut dyn FnMut(&mut [u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut key = [0u8; KEY_LEN];
    fill(&mut key)?;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    write_key(&mut f, &key)
}

pub fn generate_key_file(path: &Path) -> io::Result<()> {
    generate_key_file_with_random(path, &mut random_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rfc8439_key() -> [u8; KEY_LEN] {
        let mut k = [0u8; KEY_LEN];
        for (i, byte) in k.iter_mut().enumerate() {
            *byte = 0x80 + i as u8;
        }
        k
    }

    #[test]
    fn test_rfc8439_aead_vector() {
        let key = rfc8439_key();
        let nonce: [u8; NONCE_LEN] = [
            0x07, 0x00, 0x00, 0x00, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
        ];
        let aad: [u8; 12] = [
            0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
        ];
        let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let expected_ct: [u8; 114] = [
            0xd3, 0x1a, 0x8d, 0x34, 0x64, 0x8e, 0x60, 0xdb, 0x7b, 0x86, 0xaf, 0xbc, 0x53, 0xef,
            0x7e, 0xc2, 0xa4, 0xad, 0xed, 0x51, 0x29, 0x6e, 0x08, 0xfe, 0xa9, 0xe2, 0xb5, 0xa7,
            0x36, 0xee, 0x62, 0xd6, 0x3d, 0xbe, 0xa4, 0x5e, 0x8c, 0xa9, 0x67, 0x12, 0x82, 0xfa,
            0xfb, 0x69, 0xda, 0x92, 0x72, 0x8b, 0x1a, 0x71, 0xde, 0x0a, 0x9e, 0x06, 0x0b, 0x29,
            0x05, 0xd6, 0xa5, 0xb6, 0x7e, 0xcd, 0x3b, 0x36, 0x92, 0xdd, 0xbd, 0x7f, 0x2d, 0x77,
            0x8b, 0x8c, 0x98, 0x03, 0xae, 0xe3, 0x28, 0x09, 0x1b, 0x58, 0xfa, 0xb3, 0x24, 0xe4,
            0xfa, 0xd6, 0x75, 0x94, 0x55, 0x85, 0x80, 0x8b, 0x48, 0x31, 0xd7, 0xbc, 0x3f, 0xf4,
            0xde, 0xf0, 0x8e, 0x4b, 0x7a, 0x9d, 0xe5, 0x76, 0xd2, 0x65, 0x86, 0xce, 0xc6, 0x4b,
            0x61, 0x16,
        ];
        let tag: [u8; 16] = [
            0x1a, 0xe1, 0x0b, 0x59, 0x4f, 0x09, 0xe2, 0x6a, 0x7e, 0x90, 0x2e, 0xcb, 0xd0, 0x60,
            0x06, 0x91,
        ];
        let mut expected = expected_ct.to_vec();
        expected.extend_from_slice(&tag);
        let sealed = seal(&key, &nonce, &aad, pt);
        assert_eq!(sealed, expected);
        let opened = open(&key, &nonce, &aad, &sealed).unwrap();
        assert_eq!(opened, pt);
    }

    #[test]
    fn test_random_io_errors_are_propagated() {
        let mut open_error_source = || -> io::Result<Box<dyn Read>> {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "entropy source unavailable",
            ))
        };
        let open_error = random_bytes_with(&mut [0u8; 4], &mut open_error_source).unwrap_err();
        assert_eq!(open_error.kind(), io::ErrorKind::NotFound);

        let mut short_read_source =
            || Ok(Box::new(std::io::Cursor::new(vec![1u8, 2])) as Box<dyn Read>);
        let read_error = random_bytes_with(&mut [0u8; 4], &mut short_read_source).unwrap_err();
        assert_eq!(read_error.kind(), io::ErrorKind::UnexpectedEof);

        let mut fill_error =
            |_: &mut [u8]| Err(io::Error::new(io::ErrorKind::PermissionDenied, "blocked"));
        let vector_error = random_vec_with(4, &mut fill_error).unwrap_err();
        assert_eq!(vector_error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_generate_key_file_propagates_entropy_and_write_errors() {
        use std::io::Write;

        let dir = std::env::temp_dir().join(format!(
            "crabnnb-crypto-test-write-error-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key.hex");
        let mut failed_entropy =
            |_: &mut [u8]| Err(io::Error::new(io::ErrorKind::Other, "entropy failed"));
        let entropy_error = generate_key_file_with_random(&path, &mut failed_entropy).unwrap_err();
        assert_eq!(entropy_error.kind(), io::ErrorKind::Other);
        assert!(!path.exists());

        struct FailingWriter(usize);
        impl Write for FailingWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0 == 0 {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "writer failed"));
                }
                let written = bytes.len().min(self.0);
                self.0 -= written;
                Ok(written)
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        FailingWriter(0).flush().unwrap();
        assert_eq!(
            write_key(&mut FailingWriter(0), &[0x42; KEY_LEN])
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            write_key(&mut FailingWriter(KEY_LEN * 2), &[0x42; KEY_LEN])
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_roundtrip() {
        let key = [0x42u8; KEY_LEN];
        let nonce = [0x07u8; NONCE_LEN];
        let sealed = seal(&key, &nonce, b"aad", b"hello");
        assert_eq!(open(&key, &nonce, b"aad", &sealed).unwrap(), b"hello");
    }

    #[test]
    fn test_wrong_key_fails() {
        let key = [0x42u8; KEY_LEN];
        let other = [0x43u8; KEY_LEN];
        let nonce = [0x07u8; NONCE_LEN];
        let sealed = seal(&key, &nonce, b"aad", b"hello");
        assert_eq!(
            open(&other, &nonce, b"aad", &sealed),
            Err(CryptoError::InvalidTag)
        );
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key = [0x42u8; KEY_LEN];
        let nonce = [0x07u8; NONCE_LEN];
        let mut sealed = seal(&key, &nonce, b"aad", b"hello");
        sealed[3] ^= 0x01;
        assert_eq!(
            open(&key, &nonce, b"aad", &sealed),
            Err(CryptoError::InvalidTag)
        );
    }

    #[test]
    fn test_wrong_aad_fails() {
        let key = [0x42u8; KEY_LEN];
        let nonce = [0x07u8; NONCE_LEN];
        let sealed = seal(&key, &nonce, b"aad-1", b"hello");
        assert_eq!(
            open(&key, &nonce, b"aad-2", &sealed),
            Err(CryptoError::InvalidTag)
        );
    }

    #[test]
    fn test_hmac_sha256_rfc4231_case1() {
        let key = [0x0bu8; 20];
        let mac = hmac_sha256(&key, b"Hi There");
        let expected: [u8; 32] = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        assert_eq!(mac, expected);
    }

    #[test]
    fn test_aes256_cbc_nist_vector() {
        let key: [u8; 32] = [
            0x60, 0x3d, 0xeb, 0x10, 0x15, 0xca, 0x71, 0xbe, 0x2b, 0x73, 0xae, 0xf0, 0x85, 0x7d,
            0x77, 0x81, 0x1f, 0x35, 0x2c, 0x07, 0x3b, 0x61, 0x08, 0xd7, 0x2d, 0x98, 0x10, 0xa3,
            0x09, 0x14, 0xdf, 0xf4,
        ];
        let iv: [u8; 16] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let pt = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a,
        ];
        let ct = aes256_cbc_encrypt_raw(&key, &iv, &pt);
        let nist_ct: [u8; 16] = [
            0xf5, 0x8c, 0x4c, 0x04, 0xd6, 0xe5, 0xf1, 0xba, 0x77, 0x9e, 0xab, 0xfb, 0x5f, 0x7b,
            0xfb, 0xd6,
        ];
        assert_eq!(&ct[..16], &nist_ct);
        assert_eq!(ct.len(), 32);
        assert_eq!(aes256_cbc_decrypt_raw(&key, &iv, &ct).unwrap(), pt);
    }

    #[test]
    fn test_aes_cbc_tamper_fails() {
        let key = [0x42u8; 32];
        let iv = [0x07u8; 16];
        let mut ct = aes256_cbc_encrypt_raw(&key, &iv, b"hello");
        ct[3] ^= 0x01;
        assert!(aes256_cbc_decrypt_raw(&key, &iv, &ct).is_err());
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn test_hex_roundtrip() {
        let bytes = [0x00u8, 0xab, 0xff, 0x10];
        assert_eq!(to_hex(&bytes), "00abff10");
        assert_eq!(from_hex("00abff10").unwrap(), bytes);
        assert_eq!(from_hex("00 ab ff 10\n").unwrap(), bytes);
        assert_eq!(from_hex("ABcd").unwrap(), [0xab, 0xcd]);
        assert!(from_hex("0").is_err());
        assert!(from_hex("zz").is_err());
        assert!(from_hex("0z").is_err());
    }

    #[test]
    fn test_crypto_error_display_and_invalid_key_file_contents() {
        assert_eq!(
            CryptoError::InvalidTag.to_string(),
            "authentication tag mismatch"
        );

        let dir = std::env::temp_dir().join(format!(
            "crabnnb-crypto-test-invalid-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key.hex");
        fs::write(&path, "not hex").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let error = load_key(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("key file"));

        let directory_error = load_key(&dir).unwrap_err();
        assert!(matches!(
            directory_error.kind(),
            io::ErrorKind::IsADirectory | io::ErrorKind::PermissionDenied
        ));
        fs::write(&path, [0xff]).unwrap();
        assert_eq!(
            load_key(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_generate_and_load_key_file() {
        let dir = std::env::temp_dir().join(format!("crabnnb-crypto-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key.hex");
        generate_key_file(&path).unwrap();
        let meta = fs::metadata(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.trim().len(), 64);
        let key = load_key(&path).unwrap();
        assert_eq!(to_hex(&key), text.trim());
        assert!(generate_key_file(&path).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_key_rejects_loose_permissions() {
        let dir =
            std::env::temp_dir().join(format!("crabnnb-crypto-test-perm-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key.hex");
        fs::write(&path, to_hex(&[0x42u8; KEY_LEN])).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(load_key(&path).is_err());
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(load_key(&path).is_ok());
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_key_rejects_wrong_length() {
        let dir =
            std::env::temp_dir().join(format!("crabnnb-crypto-test-len-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key.hex");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&path, "aabb").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        #[cfg(not(unix))]
        fs::write(&path, "aabb").unwrap();
        assert!(load_key(&path).is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
