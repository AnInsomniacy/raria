//! eMule Kad routing, source search, keyword search, and publish ownership.

use crate::hash::Ed2kHash;
use crate::wire::{Cursor, ipv4_from_kad_contact};
use serde::{Deserialize, Serialize};

/// Parsed Kad nodes.dat bootstrap state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodesDat {
    /// nodes.dat format version, or zero for count-first legacy files.
    pub version: u32,
    /// Bootstrap edition marker used by version 3 bootstrap files.
    pub bootstrap_edition: u32,
    /// Useful Kad contacts retained for native bootstrap.
    pub contacts: Vec<KadContact>,
}

/// Useful eMule Kad contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KadContact {
    /// Kad node id.
    pub id: Ed2kHash,
    /// Contact IPv4 host.
    pub host: String,
    /// Contact UDP port.
    pub udp_port: u16,
    /// Contact TCP port.
    pub tcp_port: u16,
    /// Kad protocol version.
    pub version: u8,
    /// Whether this endpoint is verified for bootstrap use.
    pub verified: bool,
}

/// nodes.dat parse error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NodesDatError {
    /// The file is malformed or truncated.
    #[error("invalid nodes.dat payload")]
    InvalidPayload,
}

/// Parse useful Kad contacts from nodes.dat bytes.
pub fn parse_nodes_dat(payload: &[u8]) -> Result<NodesDat, NodesDatError> {
    let mut cursor = Cursor::new(payload);
    let mut count = cursor.read_u32().ok_or(NodesDatError::InvalidPayload)?;
    let mut version = 0_u32;
    let mut bootstrap_edition = 0_u32;

    if count == 0 {
        version = cursor.read_u32().ok_or(NodesDatError::InvalidPayload)?;
        if !(1..=3).contains(&version) {
            return Err(NodesDatError::InvalidPayload);
        }
        if version >= 3 {
            bootstrap_edition = cursor.read_u32().ok_or(NodesDatError::InvalidPayload)?;
        }
        count = cursor.read_u32().ok_or(NodesDatError::InvalidPayload)?;
    }

    let has_verified_data = version >= 2 && bootstrap_edition == 0;
    let min_entry_size = if has_verified_data { 34 } else { 25 };
    if cursor.remaining() < count as usize * min_entry_size {
        return Err(NodesDatError::InvalidPayload);
    }

    let mut contacts = Vec::new();
    let mut any_verified = false;
    for _ in 0..count {
        let mut contact = read_contact(&mut cursor)?;
        if has_verified_data {
            cursor.read_u64().ok_or(NodesDatError::InvalidPayload)?;
            contact.verified = cursor.read_u8().ok_or(NodesDatError::InvalidPayload)? != 0;
        } else {
            contact.verified = true;
        }
        if !useful_contact(&contact) {
            continue;
        }
        any_verified = any_verified || contact.verified;
        contacts.push(contact);
    }

    if !cursor.is_done() {
        return Err(NodesDatError::InvalidPayload);
    }
    if !has_verified_data || !any_verified {
        for contact in &mut contacts {
            contact.verified = true;
        }
    }

    Ok(NodesDat {
        version,
        bootstrap_edition,
        contacts,
    })
}

fn read_contact(cursor: &mut Cursor<'_>) -> Result<KadContact, NodesDatError> {
    let id = cursor.read_hash16().ok_or(NodesDatError::InvalidPayload)?;
    let ip = cursor.read_u32().ok_or(NodesDatError::InvalidPayload)?;
    let udp_port = cursor.read_u16().ok_or(NodesDatError::InvalidPayload)?;
    let tcp_port = cursor.read_u16().ok_or(NodesDatError::InvalidPayload)?;
    let version = cursor.read_u8().ok_or(NodesDatError::InvalidPayload)?;
    Ok(KadContact {
        id,
        host: ipv4_from_kad_contact(ip),
        udp_port,
        tcp_port,
        version,
        verified: true,
    })
}

fn useful_contact(contact: &KadContact) -> bool {
    contact.host != "0.0.0.0"
        && contact.udp_port != 0
        && contact.version > 1
        && (contact.udp_port != 53 || contact.version > 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_le(value: u16) -> [u8; 2] {
        value.to_le_bytes()
    }

    fn u32_le(value: u32) -> [u8; 4] {
        value.to_le_bytes()
    }

    fn contact(id: [u8; 16], ip: [u8; 4], udp: u16, tcp: u16, version: u8) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&id);
        data.extend_from_slice(&[ip[3], ip[2], ip[1], ip[0]]);
        data.extend_from_slice(&u16_le(udp));
        data.extend_from_slice(&u16_le(tcp));
        data.push(version);
        data
    }

    #[test]
    fn parses_bootstrap_nodes_dat_and_filters_unusable_contacts() {
        let valid_id = [0x23; 16];
        let invalid_id = [0x44; 16];
        let mut data = Vec::new();
        data.extend_from_slice(&u32_le(0));
        data.extend_from_slice(&u32_le(3));
        data.extend_from_slice(&u32_le(1));
        data.extend_from_slice(&u32_le(3));
        data.extend_from_slice(&contact(valid_id, [203, 0, 113, 1], 4672, 4662, 8));
        data.extend_from_slice(&contact(invalid_id, [0, 0, 0, 0], 4672, 4662, 8));
        data.extend_from_slice(&contact(invalid_id, [203, 0, 113, 2], 53, 4662, 5));

        let nodes = parse_nodes_dat(&data).expect("nodes.dat");

        assert_eq!(nodes.version, 3);
        assert_eq!(nodes.bootstrap_edition, 1);
        assert_eq!(nodes.contacts.len(), 1);
        assert_eq!(nodes.contacts[0].id, valid_id);
        assert_eq!(nodes.contacts[0].host, "203.0.113.1");
        assert_eq!(nodes.contacts[0].udp_port, 4672);
        assert_eq!(nodes.contacts[0].tcp_port, 4662);
        assert_eq!(nodes.contacts[0].version, 8);
        assert!(nodes.contacts[0].verified);
    }

    #[test]
    fn parses_versioned_nodes_dat_verified_state() {
        let valid_id = [0x11; 16];
        let invalid_id = [0x22; 16];
        let mut data = Vec::new();
        data.extend_from_slice(&u32_le(0));
        data.extend_from_slice(&u32_le(2));
        data.extend_from_slice(&u32_le(2));
        data.extend_from_slice(&contact(valid_id, [1, 159, 24, 5], 4672, 4662, 8));
        data.extend_from_slice(&0_u64.to_le_bytes());
        data.push(0);
        data.extend_from_slice(&contact(invalid_id, [0, 0, 0, 0], 4672, 4662, 8));
        data.extend_from_slice(&0_u64.to_le_bytes());
        data.push(1);

        let nodes = parse_nodes_dat(&data).expect("nodes.dat");

        assert_eq!(nodes.version, 2);
        assert_eq!(nodes.contacts.len(), 1);
        assert!(nodes.contacts.iter().all(|contact| contact.verified));
        assert_eq!(nodes.contacts[0].host, "1.159.24.5");
    }

    #[test]
    fn rejects_truncated_nodes_dat() {
        let mut data = Vec::new();
        data.extend_from_slice(&u32_le(0));
        data.extend_from_slice(&u32_le(2));
        data.extend_from_slice(&u32_le(u32::MAX));

        assert!(parse_nodes_dat(&data).is_err());
    }
}
