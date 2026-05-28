//! ED2K server TCP and UDP discovery ownership.

use crate::hash::Ed2kHash;
use crate::opcode::ServerOpcode;
use crate::packet::{PacketFrame, Protocol};
use crate::tag::{Tag, TagName, TagValue, decode_tag_prefix, encode_tag};
use crate::wire::{Cursor, ipv4_from_server_met};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const HIGHEST_LOW_ID: u32 = 16_777_216;
const CT_NAME: u8 = 0x01;
const CT_VERSION: u8 = 0x11;
const CT_SERVER_FLAGS: u8 = 0x20;
const CT_EMULE_VERSION: u8 = 0xfb;
const SRVCAP_ZLIB: u32 = 0x0001;
const SRVCAP_AUXPORT: u32 = 0x0004;
const SRVCAP_NEWTAGS: u32 = 0x0008;
const SRVCAP_UNICODE: u32 = 0x0010;
const SRVCAP_LARGEFILES: u32 = 0x0100;

/// Parsed ED2K server bootstrap metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kServerBootstrap {
    /// Server host or DNS name.
    pub host: String,
    /// Server TCP port.
    pub port: u16,
    /// Optional server name.
    pub name: Option<String>,
    /// Optional server description.
    pub description: Option<String>,
    /// Last known user count.
    pub users: Option<u32>,
    /// Last known file count.
    pub files: Option<u32>,
    /// Last known maximum users.
    pub max_users: Option<u32>,
    /// Last known soft file limit.
    pub soft_files: Option<u32>,
    /// Last known hard file limit.
    pub hard_files: Option<u32>,
    /// Last known UDP capability flags.
    pub udp_flags: Option<u32>,
    /// Last known LowID user count.
    pub low_id_users: Option<u32>,
    /// Last known UDP key.
    pub udp_key: Option<u32>,
    /// Optional TCP obfuscation port.
    pub tcp_obfuscation_port: Option<u16>,
    /// Optional UDP obfuscation port.
    pub udp_obfuscation_port: Option<u16>,
}

/// server.met parse error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServerMetError {
    /// The file is malformed or truncated.
    #[error("invalid server.met payload")]
    InvalidPayload,
}

/// Return whether an ED2K client ID is LowID-shaped.
pub fn is_low_id(id: u32) -> bool {
    id < HIGHEST_LOW_ID
}

/// Server capability bits retained by raria.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ServerCapabilities {
    bits: u32,
}

impl ServerCapabilities {
    /// Return the default login capability set for native raria ED2K.
    pub fn default_login() -> Self {
        Self {
            bits: SRVCAP_ZLIB
                | SRVCAP_AUXPORT
                | SRVCAP_NEWTAGS
                | SRVCAP_UNICODE
                | SRVCAP_LARGEFILES,
        }
    }

    /// Return the raw capability bits.
    pub fn bits(self) -> u32 {
        self.bits
    }

    /// Create capability metadata from server-provided raw bits.
    pub fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Return whether large-file source requests are supported.
    pub fn supports_large_files(self) -> bool {
        self.bits & SRVCAP_LARGEFILES != 0
    }
}

/// Server-assigned ED2K reachability state.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ServerReachability {
    /// Server assigned a HighID.
    HighId,
    /// Server assigned a LowID.
    LowId,
}

/// Server user and file counts.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ServerStatus {
    /// Current server user count.
    pub users: u32,
    /// Current server file count.
    pub files: u32,
}

/// Obfuscation metadata attached to a found source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceObfuscation {
    /// Source obfuscation option bits.
    pub options: u8,
    /// Optional source user hash.
    pub user_hash: Option<Ed2kHash>,
}

/// Source endpoint returned by a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundSource {
    /// ED2K client ID or LowID value.
    pub client_id: u32,
    /// Source TCP port.
    pub tcp_port: u16,
    /// Optional obfuscation metadata.
    pub obfuscation: Option<SourceObfuscation>,
}

/// Server TCP state update produced by an incoming frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerTcpEvent {
    /// Server assigned a client ID.
    IdChanged {
        /// Assigned client ID.
        client_id: u32,
        /// Derived reachability state.
        reachability: ServerReachability,
    },
    /// Server status changed.
    Status(ServerStatus),
    /// Server identity metadata changed.
    IdentityUpdated,
    /// Server message.
    Message(String),
    /// Server returned sources for a file hash.
    FoundSources {
        /// File hash.
        file_hash: Ed2kHash,
        /// Useful sources from the payload.
        sources: Vec<FoundSource>,
    },
    /// Server rejected the last command.
    Rejected,
    /// Server callback failed.
    CallbackFailed,
    /// The frame was valid but does not change native state yet.
    Ignored,
}

/// Server TCP parser and state error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServerTcpError {
    /// Payload is malformed or truncated.
    #[error("invalid ED2K server TCP payload")]
    InvalidPayload,
    /// The frame uses a protocol marker not accepted on server TCP.
    #[error("invalid ED2K server TCP protocol")]
    InvalidProtocol,
    /// The opcode is not a retained server opcode.
    #[error("unsupported ED2K server TCP opcode: 0x{0:02x}")]
    UnsupportedOpcode(u8),
    /// A typed tag cannot be encoded.
    #[error("invalid ED2K server tag")]
    InvalidTag,
}

/// Native server TCP state retained by raria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTcpState {
    /// Server host.
    pub host: String,
    /// Server TCP port.
    pub port: u16,
    /// Assigned ED2K client ID.
    pub client_id: Option<u32>,
    /// Derived reachability.
    pub reachability: Option<ServerReachability>,
    /// Last server TCP capability bits.
    pub tcp_capabilities: Option<ServerCapabilities>,
    /// Server aux port from login response.
    pub aux_port: Option<u16>,
    /// Server-reported public IP if useful.
    pub reported_public_ip: Option<u32>,
    /// TCP obfuscation port advertised by the server.
    pub tcp_obfuscation_port: Option<u16>,
    /// Server name.
    pub name: Option<String>,
    /// Server description.
    pub description: Option<String>,
    /// Last known status.
    pub status: Option<ServerStatus>,
    /// Consecutive failure count.
    pub consecutive_failures: u32,
}

impl ServerTcpState {
    /// Create native server TCP state for an endpoint.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            client_id: None,
            reachability: None,
            tcp_capabilities: None,
            aux_port: None,
            reported_public_ip: None,
            tcp_obfuscation_port: None,
            name: None,
            description: None,
            status: None,
            consecutive_failures: 0,
        }
    }

    /// Apply one decoded server TCP frame to native state.
    pub fn apply_frame(&mut self, frame: &PacketFrame) -> Result<ServerTcpEvent, ServerTcpError> {
        if !matches!(frame.protocol, Protocol::Edonkey | Protocol::Packed) {
            return Err(ServerTcpError::InvalidProtocol);
        }
        let opcode = ServerOpcode::from_byte(frame.opcode)
            .ok_or(ServerTcpError::UnsupportedOpcode(frame.opcode))?;
        match opcode {
            ServerOpcode::IdChange => self.apply_id_change(&frame.payload),
            ServerOpcode::ServerStatus => self.apply_status(&frame.payload),
            ServerOpcode::ServerIdentity => self.apply_identity(&frame.payload),
            ServerOpcode::ServerMessage => parse_server_message(&frame.payload),
            ServerOpcode::FoundSources => parse_found_sources(&frame.payload, false),
            ServerOpcode::FoundSourcesObfuscated => parse_found_sources(&frame.payload, true),
            ServerOpcode::Reject => Ok(ServerTcpEvent::Rejected),
            ServerOpcode::CallbackFailed => Ok(ServerTcpEvent::CallbackFailed),
            _ => Ok(ServerTcpEvent::Ignored),
        }
    }

    fn apply_id_change(&mut self, payload: &[u8]) -> Result<ServerTcpEvent, ServerTcpError> {
        let mut cursor = Cursor::new(payload);
        let client_id = cursor.read_u32().ok_or(ServerTcpError::InvalidPayload)?;
        if let Some(bits) = cursor.read_u32() {
            self.tcp_capabilities = Some(ServerCapabilities::from_bits(bits));
        }
        if let Some(aux_port) = cursor
            .read_u32()
            .and_then(|value| u16::try_from(value).ok())
        {
            self.aux_port = Some(aux_port);
            self.port = aux_port;
        }
        if let Some(public_ip) = cursor.read_u32()
            && !is_low_id(public_ip)
        {
            self.reported_public_ip = Some(public_ip);
        }
        if let Some(port) = cursor
            .read_u32()
            .and_then(|value| u16::try_from(value).ok())
            && port != 0
        {
            self.tcp_obfuscation_port = Some(port);
        }
        let reachability = if is_low_id(client_id) {
            ServerReachability::LowId
        } else {
            ServerReachability::HighId
        };
        self.client_id = Some(client_id);
        self.reachability = Some(reachability);
        self.consecutive_failures = 0;
        Ok(ServerTcpEvent::IdChanged {
            client_id,
            reachability,
        })
    }

    fn apply_status(&mut self, payload: &[u8]) -> Result<ServerTcpEvent, ServerTcpError> {
        let mut cursor = Cursor::new(payload);
        let status = ServerStatus {
            users: cursor.read_u32().ok_or(ServerTcpError::InvalidPayload)?,
            files: cursor.read_u32().ok_or(ServerTcpError::InvalidPayload)?,
        };
        self.status = Some(status);
        Ok(ServerTcpEvent::Status(status))
    }

    fn apply_identity(&mut self, payload: &[u8]) -> Result<ServerTcpEvent, ServerTcpError> {
        let mut cursor = Cursor::new(payload);
        cursor.read_hash16().ok_or(ServerTcpError::InvalidPayload)?;
        cursor.read_u32().ok_or(ServerTcpError::InvalidPayload)?;
        cursor.read_u16().ok_or(ServerTcpError::InvalidPayload)?;
        let tag_count = cursor.read_u32().ok_or(ServerTcpError::InvalidPayload)?;
        for _ in 0..tag_count {
            let remaining = &payload[cursor.position()..];
            let (tag, consumed) =
                decode_tag_prefix(remaining).map_err(|_| ServerTcpError::InvalidPayload)?;
            apply_identity_tag(self, tag);
            cursor
                .read_exact(consumed)
                .ok_or(ServerTcpError::InvalidPayload)?;
        }
        Ok(ServerTcpEvent::IdentityUpdated)
    }
}

/// Server retry policy for bounded reconnect attempts.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ServerRetryPolicy {
    initial_delay: Duration,
    max_delay: Duration,
}

impl Default for ServerRetryPolicy {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(30),
            max_delay: Duration::from_secs(15 * 60),
        }
    }
}

impl ServerRetryPolicy {
    /// Return the next delay for a one-based consecutive failure count.
    pub fn next_delay(self, failures: u32) -> Duration {
        if failures == 0 {
            return Duration::ZERO;
        }
        let multiplier = 1_u32
            .checked_shl(failures.saturating_sub(1))
            .unwrap_or(u32::MAX);
        self.initial_delay
            .saturating_mul(multiplier)
            .min(self.max_delay)
    }
}

/// Build a native ED2K server login request frame.
pub fn build_login_request(
    client_hash: Ed2kHash,
    client_id: u32,
    listen_port: u16,
    nickname: &str,
    client_version: u32,
    emule_version: u32,
) -> Result<PacketFrame, ServerTcpError> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&client_hash);
    payload.extend_from_slice(&client_id.to_le_bytes());
    payload.extend_from_slice(&listen_port.to_le_bytes());
    let tags = [
        Tag::new(TagName::Id(CT_NAME), TagValue::String(nickname.to_owned())),
        Tag::new(TagName::Id(CT_VERSION), TagValue::UInt32(client_version)),
        Tag::new(
            TagName::Id(CT_SERVER_FLAGS),
            TagValue::UInt32(ServerCapabilities::default_login().bits()),
        ),
        Tag::new(
            TagName::Id(CT_EMULE_VERSION),
            TagValue::UInt32(emule_version),
        ),
    ];
    payload.extend_from_slice(
        &u32::try_from(tags.len())
            .map_err(|_| ServerTcpError::InvalidPayload)?
            .to_le_bytes(),
    );
    for tag in tags {
        payload.extend_from_slice(&encode_tag(&tag).map_err(|_| ServerTcpError::InvalidTag)?);
    }
    Ok(PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: ServerOpcode::LoginRequest.into(),
        payload,
    })
}

/// Build a server TCP source request frame.
pub fn build_get_sources_request(
    file_hash: Ed2kHash,
    file_size: u64,
    obfuscated: bool,
) -> Result<PacketFrame, ServerTcpError> {
    let mut payload = Vec::with_capacity(28);
    payload.extend_from_slice(&file_hash);
    if let Ok(size) = u32::try_from(file_size) {
        payload.extend_from_slice(&size.to_le_bytes());
    } else {
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.extend_from_slice(&file_size.to_le_bytes());
    }
    Ok(PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: if obfuscated {
            ServerOpcode::GetSourcesObfuscated.into()
        } else {
            ServerOpcode::GetSources.into()
        },
        payload,
    })
}

fn parse_server_message(payload: &[u8]) -> Result<ServerTcpEvent, ServerTcpError> {
    let mut cursor = Cursor::new(payload);
    let len = cursor.read_u16().ok_or(ServerTcpError::InvalidPayload)? as usize;
    let bytes = cursor
        .read_exact(len)
        .ok_or(ServerTcpError::InvalidPayload)?;
    Ok(ServerTcpEvent::Message(
        String::from_utf8_lossy(bytes).into_owned(),
    ))
}

fn parse_found_sources(payload: &[u8], obfuscated: bool) -> Result<ServerTcpEvent, ServerTcpError> {
    let mut cursor = Cursor::new(payload);
    let file_hash = cursor.read_hash16().ok_or(ServerTcpError::InvalidPayload)?;
    let count = cursor.read_u8().ok_or(ServerTcpError::InvalidPayload)?;
    let mut sources = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let client_id = cursor.read_u32().ok_or(ServerTcpError::InvalidPayload)?;
        let tcp_port = cursor.read_u16().ok_or(ServerTcpError::InvalidPayload)?;
        let obfuscation = if obfuscated {
            let options = cursor.read_u8().ok_or(ServerTcpError::InvalidPayload)?;
            let user_hash = if options & 0x08 != 0 {
                Some(cursor.read_hash16().ok_or(ServerTcpError::InvalidPayload)?)
            } else {
                None
            };
            Some(SourceObfuscation { options, user_hash })
        } else {
            None
        };
        sources.push(FoundSource {
            client_id,
            tcp_port,
            obfuscation,
        });
    }
    if !cursor.is_done() {
        return Err(ServerTcpError::InvalidPayload);
    }
    Ok(ServerTcpEvent::FoundSources { file_hash, sources })
}

fn apply_identity_tag(state: &mut ServerTcpState, tag: Tag) {
    match (tag.name, tag.value) {
        (TagName::Id(0x01), TagValue::String(value)) => state.name = Some(value),
        (TagName::Id(0x0b), TagValue::String(value)) => state.description = Some(value),
        _ => {}
    }
}

#[derive(Debug)]
struct MetTag {
    id: u8,
    name: String,
    value: MetTagValue,
}

#[derive(Debug)]
enum MetTagValue {
    String(String),
    UInt(u64),
    Other,
}

/// Parse useful ED2K server bootstrap entries from server.met bytes.
pub fn parse_server_met(payload: &[u8]) -> Result<Vec<Ed2kServerBootstrap>, ServerMetError> {
    let mut cursor = Cursor::new(payload);
    let header = cursor.read_u8().ok_or(ServerMetError::InvalidPayload)?;
    if !matches!(header, 0x0e | 0x0f | 0xe0) {
        return Err(ServerMetError::InvalidPayload);
    }
    let count = cursor.read_u32().ok_or(ServerMetError::InvalidPayload)?;
    let mut servers = Vec::new();
    for _ in 0..count {
        let ip = cursor.read_u32().ok_or(ServerMetError::InvalidPayload)?;
        let port = cursor.read_u16().ok_or(ServerMetError::InvalidPayload)?;
        let tag_count = cursor.read_u32().ok_or(ServerMetError::InvalidPayload)?;
        let mut server = Ed2kServerBootstrap {
            host: if ip == 0 {
                String::new()
            } else {
                ipv4_from_server_met(ip)
            },
            port,
            name: None,
            description: None,
            users: None,
            files: None,
            max_users: None,
            soft_files: None,
            hard_files: None,
            udp_flags: None,
            low_id_users: None,
            udp_key: None,
            tcp_obfuscation_port: None,
            udp_obfuscation_port: None,
        };

        for _ in 0..tag_count {
            apply_tag(&mut server, read_met_tag(&mut cursor)?);
        }

        if !server.host.is_empty() && server.port != 0 {
            servers.push(server);
        }
    }
    if !cursor.is_done() {
        return Err(ServerMetError::InvalidPayload);
    }
    Ok(servers)
}

/// Merge incoming bootstrap metadata without erasing useful existing fields.
pub fn merge_server_bootstrap(
    existing: &mut Vec<Ed2kServerBootstrap>,
    incoming: Vec<Ed2kServerBootstrap>,
) {
    for server in incoming {
        if let Some(current) = existing
            .iter_mut()
            .find(|candidate| candidate.host == server.host && candidate.port == server.port)
        {
            merge_one(current, server);
        } else {
            existing.push(server);
        }
    }
}

fn merge_one(current: &mut Ed2kServerBootstrap, incoming: Ed2kServerBootstrap) {
    replace_if_some(&mut current.name, incoming.name);
    replace_if_some(&mut current.description, incoming.description);
    replace_if_some(&mut current.users, incoming.users);
    replace_if_some(&mut current.files, incoming.files);
    replace_if_some(&mut current.max_users, incoming.max_users);
    replace_if_some(&mut current.soft_files, incoming.soft_files);
    replace_if_some(&mut current.hard_files, incoming.hard_files);
    replace_if_some(&mut current.udp_flags, incoming.udp_flags);
    replace_if_some(&mut current.low_id_users, incoming.low_id_users);
    replace_if_some(&mut current.udp_key, incoming.udp_key);
    replace_if_some(
        &mut current.tcp_obfuscation_port,
        incoming.tcp_obfuscation_port,
    );
    replace_if_some(
        &mut current.udp_obfuscation_port,
        incoming.udp_obfuscation_port,
    );
}

fn replace_if_some<T>(current: &mut Option<T>, incoming: Option<T>) {
    if incoming.is_some() {
        *current = incoming;
    }
}

fn apply_tag(server: &mut Ed2kServerBootstrap, tag: MetTag) {
    let id = tag.id;
    let name = tag.name;
    match tag.value {
        MetTagValue::String(value) => match_tag_string(server, id, &name, value),
        MetTagValue::UInt(value) => match_tag_uint(server, id, &name, value),
        MetTagValue::Other => {}
    }
}

fn match_tag_string(server: &mut Ed2kServerBootstrap, id: u8, name: &str, value: String) {
    if tag_matches(id, name, 0x01, "name") {
        server.name = Some(value);
    } else if tag_matches(id, name, 0x0b, "description") {
        server.description = Some(value);
    } else if tag_matches(id, name, 0x85, "dynip") && server.host.is_empty() {
        server.host = value;
    }
}

fn match_tag_uint(server: &mut Ed2kServerBootstrap, id: u8, name: &str, value: u64) {
    let Ok(value32) = u32::try_from(value) else {
        return;
    };
    if name == "users" {
        server.users = Some(value32);
    } else if name == "files" {
        server.files = Some(value32);
    } else if tag_matches(id, name, 0x87, "maxusers") {
        server.max_users = Some(value32);
    } else if tag_matches(id, name, 0x88, "softfiles") {
        server.soft_files = Some(value32);
    } else if tag_matches(id, name, 0x89, "hardfiles") {
        server.hard_files = Some(value32);
    } else if tag_matches(id, name, 0x92, "udpflags") {
        server.udp_flags = Some(value32);
    } else if tag_matches(id, name, 0x94, "lowusers") {
        server.low_id_users = Some(value32);
    } else if tag_matches(id, name, 0x95, "udpkey") {
        server.udp_key = Some(value32);
    } else if tag_matches(id, name, 0x97, "tcpportobfuscation") {
        if let Ok(port) = u16::try_from(value) {
            server.tcp_obfuscation_port = Some(port);
        }
    } else if tag_matches(id, name, 0x98, "udpportobfuscation") {
        if let Ok(port) = u16::try_from(value) {
            server.udp_obfuscation_port = Some(port);
        }
    }
}

fn tag_matches(tag_id: u8, tag_name: &str, id: u8, name: &str) -> bool {
    tag_id == id || tag_name == name
}

fn read_met_tag(cursor: &mut Cursor<'_>) -> Result<MetTag, ServerMetError> {
    let raw_type = cursor.read_u8().ok_or(ServerMetError::InvalidPayload)?;
    let tag_type = raw_type & 0x7f;
    let (id, name) = if raw_type & 0x80 != 0 {
        (
            cursor.read_u8().ok_or(ServerMetError::InvalidPayload)?,
            String::new(),
        )
    } else {
        let size = cursor.read_u16().ok_or(ServerMetError::InvalidPayload)? as usize;
        let bytes = cursor
            .read_exact(size)
            .ok_or(ServerMetError::InvalidPayload)?;
        let name = String::from_utf8_lossy(bytes).into_owned();
        let id = if bytes.len() == 1 { bytes[0] } else { 0 };
        (id, name)
    };

    let value = match tag_type {
        0x02 => {
            let size = cursor.read_u16().ok_or(ServerMetError::InvalidPayload)? as usize;
            let bytes = cursor
                .read_exact(size)
                .ok_or(ServerMetError::InvalidPayload)?;
            MetTagValue::String(String::from_utf8_lossy(bytes).into_owned())
        }
        0x03 => MetTagValue::UInt(u64::from(
            cursor.read_u32().ok_or(ServerMetError::InvalidPayload)?,
        )),
        0x08 => MetTagValue::UInt(u64::from(
            cursor.read_u16().ok_or(ServerMetError::InvalidPayload)?,
        )),
        0x09 => MetTagValue::UInt(u64::from(
            cursor.read_u8().ok_or(ServerMetError::InvalidPayload)?,
        )),
        0x0b => MetTagValue::UInt(cursor.read_u64().ok_or(ServerMetError::InvalidPayload)?),
        0x11..=0x20 => {
            let size = usize::from(tag_type - 0x11 + 1);
            let bytes = cursor
                .read_exact(size)
                .ok_or(ServerMetError::InvalidPayload)?;
            MetTagValue::String(String::from_utf8_lossy(bytes).into_owned())
        }
        0x01 => {
            cursor
                .read_exact(16)
                .ok_or(ServerMetError::InvalidPayload)?;
            MetTagValue::Other
        }
        0x07 => {
            let size = cursor.read_u32().ok_or(ServerMetError::InvalidPayload)? as usize;
            cursor
                .read_exact(size)
                .ok_or(ServerMetError::InvalidPayload)?;
            MetTagValue::Other
        }
        _ => return Err(ServerMetError::InvalidPayload),
    };

    Ok(MetTag { id, name, value })
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

    fn string_tag(id: u8, value: &str) -> Vec<u8> {
        let mut tag = vec![0x80 | 0x02, id];
        tag.extend_from_slice(&u16_le(value.len() as u16));
        tag.extend_from_slice(value.as_bytes());
        tag
    }

    fn u32_tag(id: u8, value: u32) -> Vec<u8> {
        let mut tag = vec![0x80 | 0x03, id];
        tag.extend_from_slice(&u32_le(value));
        tag
    }

    fn u16_tag(id: u8, value: u16) -> Vec<u8> {
        let mut tag = vec![0x80 | 0x08, id];
        tag.extend_from_slice(&u16_le(value));
        tag
    }

    #[test]
    fn parses_server_met_entries_with_useful_metadata() {
        let mut data = Vec::new();
        data.push(0x0e);
        data.extend_from_slice(&u32_le(1));
        data.extend_from_slice(&u32_le(0x04030201));
        data.extend_from_slice(&u16_le(4661));
        data.extend_from_slice(&u32_le(10));
        data.extend_from_slice(&string_tag(0x01, "Peer Server"));
        data.extend_from_slice(&string_tag(0x0b, "Primary ED2K server"));
        data.extend_from_slice(&u32_tag(0x87, 9000));
        data.extend_from_slice(&u32_tag(0x88, 100));
        data.extend_from_slice(&u32_tag(0x89, 200));
        data.extend_from_slice(&u32_tag(0x92, 0x01020304));
        data.extend_from_slice(&u32_tag(0x94, 77));
        data.extend_from_slice(&u32_tag(0x95, 0x11223344));
        data.extend_from_slice(&u16_tag(0x97, 4665));
        data.extend_from_slice(&u16_tag(0x98, 4675));

        let servers = parse_server_met(&data).expect("server.met");

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].host, "1.2.3.4");
        assert_eq!(servers[0].port, 4661);
        assert_eq!(servers[0].name, Some("Peer Server".into()));
        assert_eq!(servers[0].description, Some("Primary ED2K server".into()));
        assert_eq!(servers[0].max_users, Some(9000));
        assert_eq!(servers[0].soft_files, Some(100));
        assert_eq!(servers[0].hard_files, Some(200));
        assert_eq!(servers[0].udp_flags, Some(0x01020304));
        assert_eq!(servers[0].low_id_users, Some(77));
        assert_eq!(servers[0].udp_key, Some(0x11223344));
        assert_eq!(servers[0].tcp_obfuscation_port, Some(4665));
        assert_eq!(servers[0].udp_obfuscation_port, Some(4675));
    }

    #[test]
    fn parses_server_met_dynip_and_rejects_bad_inputs() {
        let mut data = Vec::new();
        data.push(0xe0);
        data.extend_from_slice(&u32_le(1));
        data.extend_from_slice(&u32_le(0));
        data.extend_from_slice(&u16_le(4661));
        data.extend_from_slice(&u32_le(2));
        data.extend_from_slice(&string_tag(0x85, "peer.example.org"));
        data.extend_from_slice(&string_tag(0x01, "Hostname Server"));

        let servers = parse_server_met(&data).expect("server.met");

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].host, "peer.example.org");
        assert_eq!(servers[0].name, Some("Hostname Server".into()));
        assert!(parse_server_met(&[]).is_err());
        assert!(parse_server_met(&[0xff, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn merges_server_bootstrap_without_erasing_existing_metadata() {
        let mut existing = vec![Ed2kServerBootstrap {
            host: "1.2.3.4".into(),
            port: 4661,
            name: Some("Known".into()),
            description: Some("Existing description".into()),
            users: Some(10),
            files: None,
            max_users: None,
            soft_files: None,
            hard_files: None,
            udp_flags: Some(7),
            low_id_users: None,
            udp_key: None,
            tcp_obfuscation_port: None,
            udp_obfuscation_port: None,
        }];
        let incoming = vec![
            Ed2kServerBootstrap {
                host: "1.2.3.4".into(),
                port: 4661,
                name: None,
                description: None,
                users: None,
                files: Some(200),
                max_users: Some(9000),
                soft_files: None,
                hard_files: None,
                udp_flags: None,
                low_id_users: None,
                udp_key: Some(0x11223344),
                tcp_obfuscation_port: None,
                udp_obfuscation_port: None,
            },
            Ed2kServerBootstrap {
                host: "5.6.7.8".into(),
                port: 4661,
                name: Some("New".into()),
                description: None,
                users: None,
                files: None,
                max_users: None,
                soft_files: None,
                hard_files: None,
                udp_flags: None,
                low_id_users: None,
                udp_key: None,
                tcp_obfuscation_port: None,
                udp_obfuscation_port: None,
            },
        ];

        merge_server_bootstrap(&mut existing, incoming);

        assert_eq!(existing.len(), 2);
        assert_eq!(existing[0].name, Some("Known".into()));
        assert_eq!(existing[0].description, Some("Existing description".into()));
        assert_eq!(existing[0].users, Some(10));
        assert_eq!(existing[0].files, Some(200));
        assert_eq!(existing[0].max_users, Some(9000));
        assert_eq!(existing[0].udp_flags, Some(7));
        assert_eq!(existing[0].udp_key, Some(0x11223344));
        assert_eq!(existing[1].host, "5.6.7.8");
    }
}
