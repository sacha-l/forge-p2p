//! CLI-supplied configuration — today, just the 32-byte shared secret used
//! to authenticate the pairing handshake.

use anyhow::{anyhow, Result};

/// Byte length of the shared secret used by the HMAC-based pairing handshake.
pub const SECRET_LEN: usize = 32;

/// Parse a hex-encoded shared secret into a fixed-size byte array.
///
/// Accepts 64 hex characters (upper or lower case). Leading/trailing whitespace
/// is trimmed so values piped in from files or `echo` don't trip an invisible
/// newline. Returns a plain `anyhow::Error` with a human-readable message on
/// any parse failure; never panics.
pub fn parse_secret(hex_str: &str) -> Result<[u8; SECRET_LEN]> {
    let trimmed = hex_str.trim();
    let bytes = hex::decode(trimmed).map_err(|e| anyhow!("invalid hex secret: {e}"))?;
    if bytes.len() != SECRET_LEN {
        return Err(anyhow!(
            "secret must be {} bytes ({} hex chars); got {} bytes",
            SECRET_LEN,
            SECRET_LEN * 2,
            bytes.len()
        ));
    }
    let mut arr = [0u8; SECRET_LEN];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

/// Process-wide configuration assembled from CLI flags and environment.
///
/// Intentionally held by value so callers can clone into per-task state.
#[derive(Debug, Clone)]
pub struct Config {
    pub secret: [u8; SECRET_LEN],
}

impl Config {
    /// Build a `Config` from a hex-encoded secret string.
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        Ok(Self {
            secret: parse_secret(hex_str)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_HEX: &str = "00112233445566778899aabbccddeeff\
                            00112233445566778899aabbccddeeff";

    #[test]
    fn parses_valid_hex() {
        let bytes = parse_secret(VALID_HEX).expect("valid hex should parse");
        assert_eq!(bytes.len(), SECRET_LEN);
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[1], 0x11);
        assert_eq!(bytes[31], 0xff);
    }

    #[test]
    fn accepts_uppercase_and_trims_whitespace() {
        let padded = format!("  {}\n", VALID_HEX.to_uppercase());
        let bytes = parse_secret(&padded).expect("uppercase + padding should parse");
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[31], 0xff);
    }

    #[test]
    fn rejects_non_hex() {
        let err = parse_secret("not hex!").unwrap_err().to_string();
        assert!(err.contains("invalid hex"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_odd_length() {
        let err = parse_secret("abc").unwrap_err().to_string();
        assert!(err.contains("invalid hex"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_wrong_byte_length() {
        let short = "00112233445566778899aabbccddeeff"; // 16 bytes
        let err = parse_secret(short).unwrap_err().to_string();
        assert!(err.contains("32 bytes"), "unexpected error: {err}");
    }

    #[test]
    fn config_from_hex_roundtrip() {
        let cfg = Config::from_hex(VALID_HEX).unwrap();
        assert_eq!(cfg.secret, parse_secret(VALID_HEX).unwrap());
    }
}
