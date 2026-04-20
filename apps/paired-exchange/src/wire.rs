//! Wire framing for messages multiplexed through SwarmNL `SendRpc`.
//!
//! A single byte tag lets pairing traffic (`0x01`..`0x04`) and application
//! data traffic (`0x10`..`0x11`) share one RPC channel without registering a
//! second libp2p protocol. Bodies are fixed-length per variant; `decode`
//! verifies the length matches the tag and returns `Err` on any mismatch.
//! No variant can panic a decoder — malformed input is an ordinary error.
//!
//! Crypto primitive is HMAC-SHA256 over a fresh 16-byte nonce. See
//! `decisions.md` for why HMAC rather than a PAKE.

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::config::SECRET_LEN;

pub const NONCE_LEN: usize = 16;
pub const MAC_LEN: usize = 32;

/// Tag byte for [`WireMsg::Challenge`].
pub const TAG_CHALLENGE: u8 = 0x01;
/// Tag byte for [`WireMsg::Response`].
pub const TAG_RESPONSE: u8 = 0x02;
/// Tag byte for [`WireMsg::ResponseAndChallenge`].
pub const TAG_RESPONSE_AND_CHALLENGE: u8 = 0x03;
/// Tag byte for [`WireMsg::Ack`].
pub const TAG_ACK: u8 = 0x04;
/// Tag byte for [`WireMsg::DataPing`].
pub const TAG_DATA_PING: u8 = 0x10;
/// Tag byte for [`WireMsg::DataPong`].
pub const TAG_DATA_PONG: u8 = 0x11;

type HmacSha256 = Hmac<Sha256>;

/// Application-visible framed message. Every variant has a fixed body length;
/// the encoding is tag byte + body. Decoding rejects unknown tags and any
/// body whose length does not match the declared tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireMsg {
    Challenge([u8; NONCE_LEN]),
    Response([u8; MAC_LEN]),
    ResponseAndChallenge {
        mac: [u8; MAC_LEN],
        nonce: [u8; NONCE_LEN],
    },
    Ack,
    DataPing(u64),
    DataPong(u64),
}

impl WireMsg {
    /// Serialize to a tag-prefixed byte vector suitable for `AppData::SendRpc`.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            WireMsg::Challenge(nonce) => {
                let mut out = Vec::with_capacity(1 + NONCE_LEN);
                out.push(TAG_CHALLENGE);
                out.extend_from_slice(nonce);
                out
            }
            WireMsg::Response(mac) => {
                let mut out = Vec::with_capacity(1 + MAC_LEN);
                out.push(TAG_RESPONSE);
                out.extend_from_slice(mac);
                out
            }
            WireMsg::ResponseAndChallenge { mac, nonce } => {
                let mut out = Vec::with_capacity(1 + MAC_LEN + NONCE_LEN);
                out.push(TAG_RESPONSE_AND_CHALLENGE);
                out.extend_from_slice(mac);
                out.extend_from_slice(nonce);
                out
            }
            WireMsg::Ack => vec![TAG_ACK],
            WireMsg::DataPing(seq) => {
                let mut out = Vec::with_capacity(1 + 8);
                out.push(TAG_DATA_PING);
                out.extend_from_slice(&seq.to_le_bytes());
                out
            }
            WireMsg::DataPong(seq) => {
                let mut out = Vec::with_capacity(1 + 8);
                out.push(TAG_DATA_PONG);
                out.extend_from_slice(&seq.to_le_bytes());
                out
            }
        }
    }

    /// Parse a tag-prefixed byte slice. Never panics; unknown tags, wrong
    /// body length, and empty input all return `Err`.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or_else(|| anyhow!("wire: empty message"))?;
        match tag {
            TAG_CHALLENGE => {
                let nonce = fixed::<NONCE_LEN>(body, "Challenge")?;
                Ok(WireMsg::Challenge(nonce))
            }
            TAG_RESPONSE => {
                let mac = fixed::<MAC_LEN>(body, "Response")?;
                Ok(WireMsg::Response(mac))
            }
            TAG_RESPONSE_AND_CHALLENGE => {
                if body.len() != MAC_LEN + NONCE_LEN {
                    return Err(anyhow!(
                        "wire: ResponseAndChallenge body must be {} bytes; got {}",
                        MAC_LEN + NONCE_LEN,
                        body.len()
                    ));
                }
                let mut mac = [0u8; MAC_LEN];
                let mut nonce = [0u8; NONCE_LEN];
                mac.copy_from_slice(&body[..MAC_LEN]);
                nonce.copy_from_slice(&body[MAC_LEN..]);
                Ok(WireMsg::ResponseAndChallenge { mac, nonce })
            }
            TAG_ACK => {
                if !body.is_empty() {
                    return Err(anyhow!("wire: Ack body must be empty; got {}", body.len()));
                }
                Ok(WireMsg::Ack)
            }
            TAG_DATA_PING => {
                let seq = fixed::<8>(body, "DataPing")?;
                Ok(WireMsg::DataPing(u64::from_le_bytes(seq)))
            }
            TAG_DATA_PONG => {
                let seq = fixed::<8>(body, "DataPong")?;
                Ok(WireMsg::DataPong(u64::from_le_bytes(seq)))
            }
            other => Err(anyhow!("wire: unknown tag 0x{:02x}", other)),
        }
    }
}

fn fixed<const N: usize>(body: &[u8], variant: &'static str) -> Result<[u8; N]> {
    if body.len() != N {
        return Err(anyhow!(
            "wire: {variant} body must be {N} bytes; got {}",
            body.len()
        ));
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(body);
    Ok(arr)
}

/// HMAC-SHA256 of `nonce` under the shared secret. Deterministic and
/// constant-time comparable by both peers.
pub fn hmac_nonce(secret: &[u8; SECRET_LEN], nonce: &[u8; NONCE_LEN]) -> [u8; MAC_LEN] {
    // `Hmac::new_from_slice` only fails on keys the chosen MAC cannot accept;
    // SHA-256 accepts every byte length, so this unwrap is unreachable.
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(nonce);
    let out = mac.finalize().into_bytes();
    let mut arr = [0u8; MAC_LEN];
    arr.copy_from_slice(&out);
    arr
}

/// Constant-time comparison of two MAC arrays. Avoids timing side-channels
/// when verifying a challenge response.
pub fn mac_eq(a: &[u8; MAC_LEN], b: &[u8; MAC_LEN]) -> bool {
    let mut diff = 0u8;
    for i in 0..MAC_LEN {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(m: WireMsg) {
        let encoded = m.encode();
        let decoded = WireMsg::decode(&encoded).expect("roundtrip decode");
        assert_eq!(m, decoded);
    }

    #[test]
    fn roundtrip_all_variants() {
        roundtrip(WireMsg::Challenge([7u8; NONCE_LEN]));
        roundtrip(WireMsg::Response([9u8; MAC_LEN]));
        roundtrip(WireMsg::ResponseAndChallenge {
            mac: [3u8; MAC_LEN],
            nonce: [4u8; NONCE_LEN],
        });
        roundtrip(WireMsg::Ack);
        roundtrip(WireMsg::DataPing(0));
        roundtrip(WireMsg::DataPing(u64::MAX));
        roundtrip(WireMsg::DataPong(42));
    }

    #[test]
    fn rejects_empty_input() {
        assert!(WireMsg::decode(&[]).is_err());
    }

    #[test]
    fn rejects_unknown_tag() {
        assert!(WireMsg::decode(&[0xfe]).is_err());
        assert!(WireMsg::decode(&[0xfe, 1, 2, 3]).is_err());
    }

    #[test]
    fn rejects_short_bodies() {
        assert!(WireMsg::decode(&[TAG_CHALLENGE]).is_err());
        assert!(WireMsg::decode(&[TAG_CHALLENGE, 1, 2, 3]).is_err());
        assert!(WireMsg::decode(&[TAG_RESPONSE, 1, 2]).is_err());
        assert!(WireMsg::decode(&[TAG_DATA_PING, 0, 0, 0]).is_err());
    }

    #[test]
    fn rejects_long_bodies() {
        let mut bad = vec![TAG_CHALLENGE];
        bad.extend_from_slice(&[0u8; NONCE_LEN + 1]);
        assert!(WireMsg::decode(&bad).is_err());

        assert!(WireMsg::decode(&[TAG_ACK, 0]).is_err());
    }

    #[test]
    fn rejects_wrong_length_response_and_challenge() {
        let mut bad = vec![TAG_RESPONSE_AND_CHALLENGE];
        bad.extend_from_slice(&[0u8; MAC_LEN + NONCE_LEN - 1]);
        assert!(WireMsg::decode(&bad).is_err());
    }

    #[test]
    fn hmac_is_deterministic_and_verifies() {
        let secret = [0x5au8; SECRET_LEN];
        let nonce = [0xa5u8; NONCE_LEN];
        let a = hmac_nonce(&secret, &nonce);
        let b = hmac_nonce(&secret, &nonce);
        assert_eq!(a, b);
        assert!(mac_eq(&a, &b));
        assert_eq!(a.len(), MAC_LEN);
    }

    #[test]
    fn hmac_differs_with_different_secret_or_nonce() {
        let s1 = [0x01u8; SECRET_LEN];
        let s2 = [0x02u8; SECRET_LEN];
        let n1 = [0x10u8; NONCE_LEN];
        let n2 = [0x11u8; NONCE_LEN];

        let a = hmac_nonce(&s1, &n1);
        let b = hmac_nonce(&s2, &n1);
        let c = hmac_nonce(&s1, &n2);

        assert!(!mac_eq(&a, &b), "different secrets must produce different MACs");
        assert!(!mac_eq(&a, &c), "different nonces must produce different MACs");
    }

    #[test]
    fn hmac_matches_known_rfc4231_vector() {
        // RFC 4231 test case 1: key=0x0b*20, data="Hi There" → expected MAC.
        // We can't call hmac_nonce directly (fixed key/nonce sizes), so
        // re-use HmacSha256 for the vector check.
        let key = [0x0bu8; 20];
        let mut mac = HmacSha256::new_from_slice(&key).unwrap();
        mac.update(b"Hi There");
        let out = mac.finalize().into_bytes();
        let expected =
            hex::decode("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
                .unwrap();
        assert_eq!(out.as_slice(), expected.as_slice());
    }
}
