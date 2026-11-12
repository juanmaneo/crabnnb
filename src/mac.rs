use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mac([u8; 6]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacError(pub String);

impl fmt::Display for MacError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MacError {}

impl Mac {
    pub fn parse(s: &str) -> Result<Mac, MacError> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 6 {
            return Err(MacError(
                "expected 6 colon-separated hex octets (AA:BB:CC:DD:EE:FF)".to_string(),
            ));
        }
        let mut out = [0u8; 6];
        for (i, p) in parts.iter().enumerate() {
            if p.len() != 2 {
                return Err(MacError(format!("octet {i} must be exactly 2 hex digits")));
            }
            out[i] = u8::from_str_radix(p, 16)
                .map_err(|_| MacError(format!("invalid hex digits in octet {i}")))?;
        }
        Ok(Mac(out))
    }

    pub fn bytes(&self) -> [u8; 6] {
        self.0
    }
}

impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0;
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            b[0], b[1], b[2], b[3], b[4], b[5]
        )
    }
}

impl FromStr for Mac {
    type Err = MacError;

    fn from_str(s: &str) -> Result<Mac, MacError> {
        Mac::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_mac() {
        let mac = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(mac.bytes(), [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_parse_lowercase() {
        let mac = Mac::parse("aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(mac.bytes(), [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_parse_wrong_part_count() {
        assert!(Mac::parse("AA:BB:CC:DD:EE").is_err());
        assert!(Mac::parse("AA:BB:CC:DD:EE:FF:00").is_err());
    }

    #[test]
    fn test_parse_invalid_hex() {
        assert!(Mac::parse("GG:BB:CC:DD:EE:FF").is_err());
    }

    #[test]
    fn test_parse_wrong_length_octet() {
        assert!(Mac::parse("A:BB:CC:DD:EE:FF").is_err());
        assert!(Mac::parse("AAB:BB:CC:DD:EE:FF").is_err());
    }

    #[test]
    fn test_display_format() {
        let mac = Mac::parse("01:02:03:04:05:06").unwrap();
        assert_eq!(format!("{mac}"), "01:02:03:04:05:06");
    }

    #[test]
    fn test_from_str() {
        let mac: Mac = "AA:BB:CC:DD:EE:FF".parse().unwrap();
        assert_eq!(mac.bytes(), [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_mac_error_display() {
        let err = MacError("test error".to_string());
        assert_eq!(format!("{err}"), "test error");
    }
}
