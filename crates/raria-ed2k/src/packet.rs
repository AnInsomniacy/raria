//! ED2K, eMule, and Kad packet framing.

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::io::{Read, Write};

/// ED2K-family protocol marker byte.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Protocol {
    /// Standard ED2K protocol marker.
    Edonkey = 0xe3,
    /// Zlib-packed standard ED2K protocol marker.
    Packed = 0xd4,
    /// eMule extension protocol marker.
    Emule = 0xc5,
    /// eMule Kad UDP protocol marker.
    Kad = 0xe4,
    /// Zlib-packed eMule Kad UDP protocol marker.
    KadPacked = 0xe5,
}

impl Protocol {
    /// Return a retained protocol marker from its wire byte.
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0xe3 => Self::Edonkey,
            0xd4 => Self::Packed,
            0xc5 => Self::Emule,
            0xe4 => Self::Kad,
            0xe5 => Self::KadPacked,
            _ => return None,
        })
    }

    fn is_tcp(self) -> bool {
        matches!(self, Self::Edonkey | Self::Packed | Self::Emule)
    }

    fn is_udp(self) -> bool {
        matches!(
            self,
            Self::Edonkey | Self::Packed | Self::Emule | Self::Kad | Self::KadPacked
        )
    }
}

impl From<Protocol> for u8 {
    fn from(value: Protocol) -> Self {
        value as u8
    }
}

/// Decoded ED2K-family packet frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketFrame {
    /// Protocol marker.
    pub protocol: Protocol,
    /// Opcode byte.
    pub opcode: u8,
    /// Opcode payload.
    pub payload: Vec<u8>,
}

/// Packet framing and compression error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PacketError {
    /// The frame is shorter than its fixed header or declared length.
    #[error("truncated ED2K packet")]
    Truncated,
    /// The packet length field is invalid.
    #[error("invalid ED2K packet length")]
    InvalidLength,
    /// The payload exceeds the caller-owned safety limit.
    #[error("ED2K packet payload too large: {size} > {max}")]
    PayloadTooLarge {
        /// Observed payload size.
        size: usize,
        /// Caller-owned maximum payload size.
        max: usize,
    },
    /// The protocol marker is unknown or deliberately unsupported.
    #[error("unsupported ED2K protocol marker: 0x{0:02x}")]
    UnsupportedProtocol(u8),
    /// The known protocol marker is invalid for this transport.
    #[error("ED2K protocol marker is invalid for this transport: {0:?}")]
    InvalidTransportProtocol(Protocol),
    /// Zlib compression or decompression failed.
    #[error("invalid ED2K compressed payload")]
    InvalidCompression,
}

/// Encode a TCP packet frame with the ED2K six-byte header.
pub fn encode_tcp_frame(frame: &PacketFrame, max_payload: usize) -> Result<Vec<u8>, PacketError> {
    if !frame.protocol.is_tcp() {
        return Err(PacketError::InvalidTransportProtocol(frame.protocol));
    }
    enforce_max(frame.payload.len(), max_payload)?;
    let length = frame
        .payload
        .len()
        .checked_add(1)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(PacketError::InvalidLength)?;
    let mut out = Vec::with_capacity(frame.payload.len() + 6);
    out.push(frame.protocol.into());
    out.extend_from_slice(&length.to_le_bytes());
    out.push(frame.opcode);
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

/// Decode a TCP packet frame with the ED2K six-byte header.
pub fn decode_tcp_frame(input: &[u8], max_payload: usize) -> Result<PacketFrame, PacketError> {
    if input.len() < 6 {
        return Err(PacketError::Truncated);
    }
    let protocol =
        Protocol::from_byte(input[0]).ok_or(PacketError::UnsupportedProtocol(input[0]))?;
    if !protocol.is_tcp() {
        return Err(PacketError::InvalidTransportProtocol(protocol));
    }
    let length = u32::from_le_bytes([input[1], input[2], input[3], input[4]]) as usize;
    if length == 0 {
        return Err(PacketError::InvalidLength);
    }
    let payload_len = length - 1;
    enforce_max(payload_len, max_payload)?;
    let expected = payload_len
        .checked_add(6)
        .ok_or(PacketError::InvalidLength)?;
    if input.len() != expected {
        return Err(PacketError::Truncated);
    }
    Ok(PacketFrame {
        protocol,
        opcode: input[5],
        payload: input[6..].to_vec(),
    })
}

/// Encode a UDP datagram with the ED2K two-byte datagram header.
pub fn encode_udp_datagram(
    frame: &PacketFrame,
    max_payload: usize,
) -> Result<Vec<u8>, PacketError> {
    if !frame.protocol.is_udp() {
        return Err(PacketError::InvalidTransportProtocol(frame.protocol));
    }
    enforce_max(frame.payload.len(), max_payload)?;
    let mut out = Vec::with_capacity(frame.payload.len() + 2);
    out.push(frame.protocol.into());
    out.push(frame.opcode);
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

/// Decode a UDP datagram with the ED2K two-byte datagram header.
pub fn decode_udp_datagram(input: &[u8], max_payload: usize) -> Result<PacketFrame, PacketError> {
    if input.len() < 2 {
        return Err(PacketError::Truncated);
    }
    let protocol =
        Protocol::from_byte(input[0]).ok_or(PacketError::UnsupportedProtocol(input[0]))?;
    if !protocol.is_udp() {
        return Err(PacketError::InvalidTransportProtocol(protocol));
    }
    let payload_len = input.len() - 2;
    enforce_max(payload_len, max_payload)?;
    Ok(PacketFrame {
        protocol,
        opcode: input[1],
        payload: input[2..].to_vec(),
    })
}

/// Compress a packet payload using the ED2K zlib wrapper.
pub fn pack_payload(payload: &[u8]) -> Result<Vec<u8>, PacketError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(payload)
        .map_err(|_| PacketError::InvalidCompression)?;
    encoder
        .finish()
        .map_err(|_| PacketError::InvalidCompression)
}

/// Decompress a packet payload using the ED2K zlib wrapper.
pub fn unpack_payload(payload: &[u8], max_output: usize) -> Result<Vec<u8>, PacketError> {
    let decoder = ZlibDecoder::new(payload);
    let mut out = Vec::new();
    let limit = u64::try_from(max_output)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(PacketError::InvalidLength)?;
    decoder
        .take(limit)
        .read_to_end(&mut out)
        .map_err(|_| PacketError::InvalidCompression)?;
    enforce_max(out.len(), max_output)?;
    Ok(out)
}

fn enforce_max(size: usize, max: usize) -> Result<(), PacketError> {
    if size > max {
        return Err(PacketError::PayloadTooLarge { size, max });
    }
    Ok(())
}
