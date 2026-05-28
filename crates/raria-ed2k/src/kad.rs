//! eMule Kad routing, source search, keyword search, and publish ownership.

use crate::hash::Ed2kHash;
use crate::opcode::KadOpcode;
use crate::wire::{Cursor, ipv4_from_kad_contact};
use serde::{Deserialize, Serialize};

/// Number of routing buckets in eMule Kad's 128-bit ID space.
pub const KAD_ROUTING_BUCKETS: usize = 128;
/// Default useful routing bucket size.
pub const DEFAULT_KAD_BUCKET_SIZE: usize = 10;
/// Minimum seconds between empty-table bootstrap attempts.
pub const KAD_BOOTSTRAP_INTERVAL_SECONDS: u64 = 30;
/// Minimum seconds between routing refresh attempts.
pub const KAD_REFRESH_INTERVAL_SECONDS: u64 = 45;
/// Seconds after which a bucket or self lookup is considered stale.
pub const KAD_BUCKET_STALE_SECONDS: u64 = 900;
/// Confirmed contact failures tolerated before removal when no replacement exists.
pub const KAD_MAX_CONTACT_FAILURES: u8 = 20;

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
    /// Optional UDP key learned from a trusted Kad response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_key: Option<u32>,
    /// Whether this endpoint is verified for bootstrap use.
    pub verified: bool,
}

/// Kad contact validation error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KadContactValidationError {
    /// A contact must not point at the local Kad id.
    #[error("Kad contact id matches the local id")]
    SelfContact,
    /// A contact host must be useful for routing.
    #[error("Kad contact host is not routable")]
    InvalidHost,
    /// A contact UDP port must be non-zero.
    #[error("Kad contact UDP port is invalid")]
    InvalidUdpPort,
    /// A contact Kad protocol version is obsolete.
    #[error("Kad contact version is obsolete")]
    ObsoleteVersion,
}

/// Kad UDP endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KadEndpoint {
    /// Endpoint host.
    pub host: String,
    /// Endpoint UDP port.
    pub udp_port: u16,
}

impl KadEndpoint {
    /// Create a Kad UDP endpoint.
    pub fn new(host: impl Into<String>, udp_port: u16) -> Self {
        Self {
            host: host.into(),
            udp_port,
        }
    }
}

/// Runtime state of a Kad routing node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KadRoutingNodeState {
    /// Contact has been learned but not confirmed by direct traffic.
    Unconfirmed,
    /// Contact has been confirmed by direct traffic.
    Confirmed,
}

/// Kad routing node retained in a bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KadRoutingNode {
    /// Contact metadata.
    pub contact: KadContact,
    /// Confirmation state.
    pub state: KadRoutingNodeState,
    /// First observed timestamp in caller-owned seconds.
    pub first_seen_seconds: u64,
    /// Last observed timestamp in caller-owned seconds.
    pub last_seen_seconds: u64,
    /// Consecutive failures observed while no better replacement was available.
    pub fail_count: u8,
}

impl KadRoutingNode {
    fn new(contact: KadContact, state: KadRoutingNodeState, now_seconds: u64) -> Self {
        Self {
            contact,
            state,
            first_seen_seconds: now_seconds,
            last_seen_seconds: now_seconds,
            fail_count: 0,
        }
    }

    fn update(&mut self, contact: KadContact, state: KadRoutingNodeState, now_seconds: u64) {
        self.contact = contact;
        if state == KadRoutingNodeState::Confirmed {
            self.state = KadRoutingNodeState::Confirmed;
            self.fail_count = 0;
        }
        self.last_seen_seconds = now_seconds;
    }
}

/// Kad routing bucket with live contacts and bounded replacements.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KadRoutingBucket {
    /// Live contacts eligible for lookup responses.
    pub live: Vec<KadRoutingNode>,
    /// Replacement contacts retained when the live bucket is full.
    pub replacements: Vec<KadRoutingNode>,
    /// Last bucket refresh timestamp in caller-owned seconds.
    pub last_refresh_seconds: Option<u64>,
}

/// Serializable Kad routing table snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KadRoutingSnapshot {
    /// Local Kad id.
    pub self_id: Ed2kHash,
    /// Bucket size used by the snapshot.
    pub bucket_size: usize,
    /// Routing buckets.
    pub buckets: Vec<KadRoutingBucket>,
    /// Last bootstrap timestamp in caller-owned seconds.
    pub last_bootstrap_seconds: Option<u64>,
    /// Last routing refresh timestamp in caller-owned seconds.
    pub last_refresh_seconds: Option<u64>,
    /// Last self-lookup refresh timestamp in caller-owned seconds.
    pub last_self_refresh_seconds: Option<u64>,
}

/// Kad routing table restore error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KadRoutingError {
    /// A contact in the operation or snapshot is unusable.
    #[error("invalid Kad routing contact")]
    InvalidContact(#[from] KadContactValidationError),
    /// A routing snapshot is malformed.
    #[error("invalid Kad routing snapshot")]
    InvalidSnapshot,
}

/// Native eMule Kad routing table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KadRoutingTable {
    self_id: Ed2kHash,
    bucket_size: usize,
    buckets: Vec<KadRoutingBucket>,
    last_bootstrap_seconds: Option<u64>,
    last_refresh_seconds: Option<u64>,
    last_self_refresh_seconds: Option<u64>,
}

impl KadRoutingTable {
    /// Create an empty routing table.
    pub fn new(self_id: Ed2kHash, bucket_size: usize) -> Self {
        let bucket_size = bucket_size.max(1);
        Self {
            self_id,
            bucket_size,
            buckets: vec![KadRoutingBucket::default(); KAD_ROUTING_BUCKETS],
            last_bootstrap_seconds: None,
            last_refresh_seconds: None,
            last_self_refresh_seconds: None,
        }
    }

    /// Borrow the local Kad id.
    pub fn self_id(&self) -> &Ed2kHash {
        &self.self_id
    }

    /// Return the last bootstrap timestamp.
    pub fn last_bootstrap_seconds(&self) -> Option<u64> {
        self.last_bootstrap_seconds
    }

    /// Return the last self-refresh timestamp.
    pub fn last_self_refresh_seconds(&self) -> Option<u64> {
        self.last_self_refresh_seconds
    }

    /// Borrow the bucket that owns an id.
    pub fn bucket_for(&self, id: &Ed2kHash) -> Option<&KadRoutingBucket> {
        self.bucket_index(id)
            .and_then(|index| self.buckets.get(index))
    }

    /// Add an unconfirmed contact learned from another node.
    pub fn heard_about(
        &mut self,
        contact: KadContact,
        now_seconds: u64,
    ) -> Result<(), KadRoutingError> {
        self.insert(contact, KadRoutingNodeState::Unconfirmed, now_seconds)
    }

    /// Add or confirm a contact seen through direct traffic.
    pub fn node_seen(
        &mut self,
        contact: KadContact,
        now_seconds: u64,
    ) -> Result<(), KadRoutingError> {
        self.insert(contact, KadRoutingNodeState::Confirmed, now_seconds)
    }

    /// Record a node failure and promote a replacement when available.
    pub fn node_failed(&mut self, id: &Ed2kHash, now_seconds: u64) {
        let Some(bucket_index) = self.bucket_index(id) else {
            return;
        };
        let bucket = &mut self.buckets[bucket_index];

        if let Some(index) = bucket.live.iter().position(|node| node.contact.id == *id) {
            if bucket.live[index].state == KadRoutingNodeState::Unconfirmed {
                bucket.live.remove(index);
                return;
            }
            bucket.live[index].fail_count = bucket.live[index].fail_count.saturating_add(1);
            bucket.live[index].last_seen_seconds = now_seconds;
            if !bucket.replacements.is_empty()
                || bucket.live[index].fail_count >= KAD_MAX_CONTACT_FAILURES
            {
                bucket.live.remove(index);
                if let Some(mut replacement) = take_oldest_replacement(bucket) {
                    replacement.state = KadRoutingNodeState::Confirmed;
                    replacement.last_seen_seconds = now_seconds;
                    bucket.live.push(replacement);
                }
            }
            return;
        }

        if let Some(index) = bucket
            .replacements
            .iter()
            .position(|node| node.contact.id == *id)
        {
            bucket.replacements.remove(index);
        }
    }

    /// Return closest live contacts sorted by XOR distance.
    pub fn find_closest(
        &self,
        target_id: &Ed2kHash,
        limit: usize,
        include_unconfirmed: bool,
    ) -> Vec<KadContact> {
        let mut contacts: Vec<&KadRoutingNode> = self
            .buckets
            .iter()
            .flat_map(|bucket| bucket.live.iter())
            .filter(|node| include_unconfirmed || node.state == KadRoutingNodeState::Confirmed)
            .collect();
        contacts.sort_by(|left, right| {
            xor_distance(&left.contact.id, target_id)
                .cmp(&xor_distance(&right.contact.id, target_id))
        });
        contacts
            .into_iter()
            .take(limit)
            .map(|node| node.contact.clone())
            .collect()
    }

    /// Return closest live contacts while excluding a requester id.
    pub fn find_closest_excluding(
        &self,
        target_id: &Ed2kHash,
        excluded_id: &Ed2kHash,
        limit: usize,
        include_unconfirmed: bool,
    ) -> Vec<KadContact> {
        self.find_closest(target_id, usize::MAX, include_unconfirmed)
            .into_iter()
            .filter(|contact| contact.id != *excluded_id)
            .take(limit)
            .collect()
    }

    /// Return whether bootstrap should be attempted now.
    pub fn needs_bootstrap(&self, now_seconds: u64) -> bool {
        let live_empty = self.buckets.iter().all(|bucket| bucket.live.is_empty());
        let replacements = self
            .buckets
            .iter()
            .map(|bucket| bucket.replacements.len())
            .sum::<usize>();
        live_empty
            && replacements < self.bucket_size
            && elapsed_at_least(
                self.last_bootstrap_seconds,
                now_seconds,
                KAD_BOOTSTRAP_INTERVAL_SECONDS,
            )
    }

    /// Record a bootstrap attempt.
    pub fn record_bootstrap(&mut self, now_seconds: u64) {
        self.last_bootstrap_seconds = Some(now_seconds);
    }

    /// Return whether the self id or target bucket needs refresh.
    pub fn needs_refresh(&self, target_id: &Ed2kHash, now_seconds: u64) -> bool {
        if !elapsed_at_least(
            self.last_refresh_seconds,
            now_seconds,
            KAD_REFRESH_INTERVAL_SECONDS,
        ) {
            return false;
        }
        if target_id == &self.self_id {
            return elapsed_at_least(
                self.last_self_refresh_seconds,
                now_seconds,
                KAD_BUCKET_STALE_SECONDS,
            );
        }
        let Some(index) = self.bucket_index(target_id) else {
            return false;
        };
        elapsed_at_least(
            self.buckets[index].last_refresh_seconds,
            now_seconds,
            KAD_BUCKET_STALE_SECONDS,
        )
    }

    /// Record a self-id or bucket refresh.
    pub fn record_refresh(&mut self, target_id: &Ed2kHash, now_seconds: u64) {
        self.last_refresh_seconds = Some(now_seconds);
        if target_id == &self.self_id {
            self.last_self_refresh_seconds = Some(now_seconds);
            return;
        }
        if let Some(index) = self.bucket_index(target_id) {
            self.buckets[index].last_refresh_seconds = Some(now_seconds);
        }
    }

    /// Return a serializable routing table snapshot.
    pub fn snapshot(&self) -> KadRoutingSnapshot {
        KadRoutingSnapshot {
            self_id: self.self_id,
            bucket_size: self.bucket_size,
            buckets: self.buckets.clone(),
            last_bootstrap_seconds: self.last_bootstrap_seconds,
            last_refresh_seconds: self.last_refresh_seconds,
            last_self_refresh_seconds: self.last_self_refresh_seconds,
        }
    }

    /// Restore a routing table from a native snapshot.
    pub fn restore(snapshot: KadRoutingSnapshot) -> Result<Self, KadRoutingError> {
        if snapshot.bucket_size == 0 || snapshot.buckets.len() != KAD_ROUTING_BUCKETS {
            return Err(KadRoutingError::InvalidSnapshot);
        }
        for bucket in &snapshot.buckets {
            for node in bucket.live.iter().chain(bucket.replacements.iter()) {
                validate_routing_contact(&node.contact, &snapshot.self_id)?;
            }
            if bucket.live.len() > snapshot.bucket_size
                || bucket.replacements.len() > snapshot.bucket_size
            {
                return Err(KadRoutingError::InvalidSnapshot);
            }
        }
        Ok(Self {
            self_id: snapshot.self_id,
            bucket_size: snapshot.bucket_size,
            buckets: snapshot.buckets,
            last_bootstrap_seconds: snapshot.last_bootstrap_seconds,
            last_refresh_seconds: snapshot.last_refresh_seconds,
            last_self_refresh_seconds: snapshot.last_self_refresh_seconds,
        })
    }

    fn insert(
        &mut self,
        contact: KadContact,
        state: KadRoutingNodeState,
        now_seconds: u64,
    ) -> Result<(), KadRoutingError> {
        validate_routing_contact(&contact, &self.self_id)?;
        let bucket_index = self
            .bucket_index(&contact.id)
            .ok_or(KadRoutingError::InvalidSnapshot)?;
        let bucket = &mut self.buckets[bucket_index];

        if update_existing(&mut bucket.live, &contact, state, now_seconds) {
            return Ok(());
        }
        if let Some(index) = equivalent_index(&bucket.replacements, &contact) {
            let mut node = bucket.replacements.remove(index);
            node.update(contact, state, now_seconds);
            if node.state == KadRoutingNodeState::Confirmed && bucket.live.len() < self.bucket_size
            {
                bucket.live.push(node);
            } else {
                bucket.replacements.push(node);
            }
            return Ok(());
        }

        let node = KadRoutingNode::new(contact, state, now_seconds);
        if bucket.live.len() < self.bucket_size {
            bucket.live.push(node);
        } else {
            bucket.replacements.push(node);
            if bucket.replacements.len() > self.bucket_size {
                bucket.replacements.remove(0);
            }
        }
        Ok(())
    }

    fn bucket_index(&self, id: &Ed2kHash) -> Option<usize> {
        bucket_index(&self.self_id, id)
    }
}

/// Kad transaction purpose retained for deterministic expiry and matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KadTransactionPurpose {
    /// Bootstrap transaction.
    Bootstrap,
    /// Hello transaction.
    Hello,
    /// Lookup transaction.
    Lookup,
    /// Search transaction.
    Search,
    /// Publish transaction.
    Publish,
    /// Firewall transaction.
    Firewall,
}

/// Pending Kad UDP transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KadTransaction {
    /// Remote endpoint.
    pub endpoint: KadEndpoint,
    /// Request opcode.
    pub opcode: KadOpcode,
    /// Optional target id.
    pub target_id: Option<Ed2kHash>,
    /// Transaction purpose.
    pub purpose: KadTransactionPurpose,
    /// Creation timestamp in caller-owned seconds.
    pub created_seconds: u64,
}

/// Deterministic Kad transaction table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KadTransactionTable {
    transactions: Vec<KadTransaction>,
}

impl KadTransactionTable {
    /// Add or replace a pending transaction for an endpoint, opcode, and target.
    pub fn add(
        &mut self,
        endpoint: KadEndpoint,
        opcode: KadOpcode,
        target_id: Option<Ed2kHash>,
        purpose: KadTransactionPurpose,
        created_seconds: u64,
    ) {
        if let Some(index) = self.transactions.iter().position(|transaction| {
            transaction.endpoint == endpoint
                && transaction.opcode == opcode
                && transaction.target_id == target_id
        }) {
            self.transactions.remove(index);
        }
        self.transactions.push(KadTransaction {
            endpoint,
            opcode,
            target_id,
            purpose,
            created_seconds,
        });
    }

    /// Complete a transaction by endpoint and opcode.
    pub fn complete(
        &mut self,
        endpoint: &KadEndpoint,
        opcode: KadOpcode,
    ) -> Option<KadTransaction> {
        self.transactions
            .iter()
            .position(|transaction| {
                transaction.endpoint == *endpoint && transaction.opcode == opcode
            })
            .map(|index| self.transactions.remove(index))
    }

    /// Complete a transaction by endpoint, opcode, and target id.
    pub fn complete_with_target(
        &mut self,
        endpoint: &KadEndpoint,
        opcode: KadOpcode,
        target_id: &Ed2kHash,
    ) -> Option<KadTransaction> {
        self.transactions
            .iter()
            .position(|transaction| {
                transaction.endpoint == *endpoint
                    && transaction.opcode == opcode
                    && transaction.target_id.as_ref() == Some(target_id)
            })
            .map(|index| self.transactions.remove(index))
    }

    /// Expire transactions older than the timeout.
    pub fn expire(&mut self, now_seconds: u64, timeout_seconds: u64) -> Vec<KadTransaction> {
        let mut expired = Vec::new();
        let mut index = 0;
        while index < self.transactions.len() {
            let age = now_seconds.saturating_sub(self.transactions[index].created_seconds);
            if age >= timeout_seconds {
                expired.push(self.transactions.remove(index));
            } else {
                index += 1;
            }
        }
        expired
    }
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
        udp_key: None,
        verified: true,
    })
}

/// Validate that a contact is usable for routing table state.
pub fn validate_routing_contact(
    contact: &KadContact,
    self_id: &Ed2kHash,
) -> Result<(), KadContactValidationError> {
    if contact.id == *self_id {
        return Err(KadContactValidationError::SelfContact);
    }
    if contact.host.is_empty() || contact.host == "0.0.0.0" {
        return Err(KadContactValidationError::InvalidHost);
    }
    if contact.udp_port == 0 {
        return Err(KadContactValidationError::InvalidUdpPort);
    }
    if contact.version <= 1 || (contact.udp_port == 53 && contact.version <= 5) {
        return Err(KadContactValidationError::ObsoleteVersion);
    }
    Ok(())
}

fn useful_contact(contact: &KadContact) -> bool {
    !contact.host.is_empty()
        && contact.host != "0.0.0.0"
        && contact.udp_port != 0
        && contact.version > 1
        && (contact.udp_port != 53 || contact.version > 5)
}

fn update_existing(
    nodes: &mut Vec<KadRoutingNode>,
    contact: &KadContact,
    state: KadRoutingNodeState,
    now_seconds: u64,
) -> bool {
    let Some(index) = equivalent_index(nodes, contact) else {
        return false;
    };
    let mut node = nodes.remove(index);
    node.update(contact.clone(), state, now_seconds);
    nodes.push(node);
    true
}

fn equivalent_index(nodes: &[KadRoutingNode], contact: &KadContact) -> Option<usize> {
    nodes
        .iter()
        .position(|node| contacts_equivalent(&node.contact, contact))
}

fn contacts_equivalent(left: &KadContact, right: &KadContact) -> bool {
    left.id == right.id || (left.host == right.host && left.udp_port == right.udp_port)
}

fn take_oldest_replacement(bucket: &mut KadRoutingBucket) -> Option<KadRoutingNode> {
    if bucket.replacements.is_empty() {
        None
    } else {
        Some(bucket.replacements.remove(0))
    }
}

fn elapsed_at_least(last: Option<u64>, now_seconds: u64, interval_seconds: u64) -> bool {
    match last {
        Some(last) => now_seconds.saturating_sub(last) >= interval_seconds,
        None => true,
    }
}

fn bucket_index(self_id: &Ed2kHash, id: &Ed2kHash) -> Option<usize> {
    let distance = xor_distance(self_id, id);
    if distance.iter().all(|byte| *byte == 0) {
        return None;
    }
    let leading_zeros = distance
        .iter()
        .take_while(|byte| **byte == 0)
        .count()
        .saturating_mul(8);
    let first_non_zero = distance.iter().copied().find(|byte| *byte != 0)?;
    Some((leading_zeros + first_non_zero.leading_zeros() as usize).min(KAD_ROUTING_BUCKETS - 1))
}

fn xor_distance(left: &Ed2kHash, right: &Ed2kHash) -> Ed2kHash {
    let mut distance = [0; 16];
    for (index, byte) in distance.iter_mut().enumerate() {
        *byte = left[index] ^ right[index];
    }
    distance
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
