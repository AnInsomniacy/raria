//! ED2K UDP server obfuscation.

use crate::packet::{PacketError, decode_udp_datagram};
use md5::{Digest, Md5};

const CRYPT_HEADER_WITHOUT_PADDING: usize = 8;
const MAGICVALUE_UDP_SYNC_SERVER: u32 = 0x13ef_24d5;
const MAGICVALUE_UDP_SERVERCLIENT: u8 = 0xa5;
const MAGICVALUE_UDP_CLIENTSERVER: u8 = 0x6b;
const OP_EDONKEYPROT: u8 = 0xe3;

/// Error returned by ED2K UDP obfuscation helpers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UdpObfuscationError {
    /// Datagram is too short or failed the magic check.
    #[error("invalid ED2K UDP obfuscation frame")]
    InvalidFrame,
    /// The decrypted payload is not a valid UDP datagram.
    #[error("invalid decrypted ED2K UDP datagram")]
    Packet(#[from] PacketError),
}

/// Encrypt a client-to-server UDP datagram with the server UDP key.
pub fn encrypt_server_request(
    datagram: &[u8],
    base_key: u32,
    random_key_part: u16,
    marker: u8,
) -> Vec<u8> {
    encrypt_server_datagram(
        datagram,
        base_key,
        MAGICVALUE_UDP_CLIENTSERVER,
        random_key_part,
        marker,
    )
}

/// Encrypt a server-to-client UDP datagram with the server UDP key.
pub fn encrypt_server_response(
    datagram: &[u8],
    base_key: u32,
    random_key_part: u16,
    marker: u8,
) -> Vec<u8> {
    encrypt_server_datagram(
        datagram,
        base_key,
        MAGICVALUE_UDP_SERVERCLIENT,
        random_key_part,
        marker,
    )
}

/// Decrypt a client-to-server obfuscated UDP datagram.
pub fn decrypt_server_request(
    datagram: &[u8],
    base_key: u32,
    max_packet_size: usize,
) -> Result<crate::packet::PacketFrame, UdpObfuscationError> {
    decrypt_server_datagram(
        datagram,
        base_key,
        MAGICVALUE_UDP_CLIENTSERVER,
        max_packet_size,
    )
}

fn encrypt_server_datagram(
    datagram: &[u8],
    base_key: u32,
    direction: u8,
    random_key_part: u16,
    marker: u8,
) -> Vec<u8> {
    let mut rc4 = Rc4::new(&server_key(base_key, direction, random_key_part));
    let first = if marker == OP_EDONKEYPROT {
        0x01
    } else {
        marker
    };
    let mut out = Vec::with_capacity(datagram.len() + CRYPT_HEADER_WITHOUT_PADDING);
    out.push(first);
    out.extend_from_slice(&random_key_part.to_le_bytes());
    out.extend_from_slice(&rc4.apply(&MAGICVALUE_UDP_SYNC_SERVER.to_be_bytes()));
    out.extend_from_slice(&rc4.apply(&[0]));
    out.extend_from_slice(&rc4.apply(datagram));
    out
}

/// Decrypt a server-to-client obfuscated UDP datagram.
pub fn decrypt_server_response(
    datagram: &[u8],
    base_key: u32,
    max_packet_size: usize,
) -> Result<crate::packet::PacketFrame, UdpObfuscationError> {
    decrypt_server_datagram(
        datagram,
        base_key,
        MAGICVALUE_UDP_SERVERCLIENT,
        max_packet_size,
    )
}

fn decrypt_server_datagram(
    datagram: &[u8],
    base_key: u32,
    direction: u8,
    max_packet_size: usize,
) -> Result<crate::packet::PacketFrame, UdpObfuscationError> {
    if datagram.len() <= CRYPT_HEADER_WITHOUT_PADDING || datagram[0] == OP_EDONKEYPROT {
        return Err(UdpObfuscationError::InvalidFrame);
    }
    let random_key_part = u16::from_le_bytes([datagram[1], datagram[2]]);
    let mut rc4 = Rc4::new(&server_key(base_key, direction, random_key_part));
    let magic = rc4.apply(&datagram[3..7]);
    if u32::from_be_bytes([magic[0], magic[1], magic[2], magic[3]]) != MAGICVALUE_UDP_SYNC_SERVER {
        return Err(UdpObfuscationError::InvalidFrame);
    }
    let pad_len = rc4.apply(&datagram[7..8])[0] & 0x0f;
    let body_start = CRYPT_HEADER_WITHOUT_PADDING + usize::from(pad_len);
    if datagram.len() <= body_start {
        return Err(UdpObfuscationError::InvalidFrame);
    }
    if pad_len > 0 {
        let _ = rc4.apply(&datagram[CRYPT_HEADER_WITHOUT_PADDING..body_start]);
    }
    let plaintext = rc4.apply(&datagram[body_start..]);
    Ok(decode_udp_datagram(&plaintext, max_packet_size)?)
}

fn server_key(base_key: u32, direction: u8, random_key_part: u16) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(base_key.to_le_bytes());
    hasher.update([direction]);
    hasher.update(random_key_part.to_le_bytes());
    hasher.finalize().into()
}

struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    fn new(key: &[u8]) -> Self {
        let mut s = [0_u8; 256];
        for (index, value) in s.iter_mut().enumerate() {
            *value = index as u8;
        }
        let mut j = 0_u8;
        for i in 0..=255_u8 {
            let key_byte = key[usize::from(i) % key.len()];
            j = j.wrapping_add(s[usize::from(i)]).wrapping_add(key_byte);
            s.swap(usize::from(i), usize::from(j));
        }
        Self { s, i: 0, j: 0 }
    }

    fn apply(&mut self, input: &[u8]) -> Vec<u8> {
        input
            .iter()
            .map(|byte| {
                self.i = self.i.wrapping_add(1);
                self.j = self.j.wrapping_add(self.s[usize::from(self.i)]);
                self.s.swap(usize::from(self.i), usize::from(self.j));
                let k = self.s[usize::from(
                    self.s[usize::from(self.i)].wrapping_add(self.s[usize::from(self.j)]),
                )];
                byte ^ k
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opcode::ServerOpcode;
    use crate::packet::{PacketFrame, Protocol, encode_udp_datagram};

    #[test]
    fn server_request_obfuscation_hides_and_recovers_plain_datagram_with_matching_key() {
        let frame = PacketFrame {
            protocol: Protocol::Edonkey,
            opcode: ServerOpcode::GlobalSearchRequest.into(),
            payload: b"test".to_vec(),
        };
        let plain = encode_udp_datagram(&frame, 1024).expect("plain");

        let encrypted = encrypt_server_request(&plain, 0x1122_3344, 0x5566, 0xe3);

        assert_ne!(encrypted[0], 0xe3);
        assert_ne!(&encrypted[8..], &plain[..]);

        let decrypted =
            decrypt_with_direction(&encrypted, 0x1122_3344, MAGICVALUE_UDP_CLIENTSERVER)
                .expect("decrypt");
        assert_eq!(decrypted, plain);
    }

    fn decrypt_with_direction(
        datagram: &[u8],
        base_key: u32,
        direction: u8,
    ) -> Result<Vec<u8>, UdpObfuscationError> {
        if datagram.len() <= CRYPT_HEADER_WITHOUT_PADDING || datagram[0] == OP_EDONKEYPROT {
            return Err(UdpObfuscationError::InvalidFrame);
        }
        let random_key_part = u16::from_le_bytes([datagram[1], datagram[2]]);
        let mut rc4 = Rc4::new(&server_key(base_key, direction, random_key_part));
        let magic = rc4.apply(&datagram[3..7]);
        if u32::from_be_bytes([magic[0], magic[1], magic[2], magic[3]])
            != MAGICVALUE_UDP_SYNC_SERVER
        {
            return Err(UdpObfuscationError::InvalidFrame);
        }
        let pad_len = rc4.apply(&datagram[7..8])[0] & 0x0f;
        let body_start = CRYPT_HEADER_WITHOUT_PADDING + usize::from(pad_len);
        Ok(rc4.apply(&datagram[body_start..]))
    }
}
