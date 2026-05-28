//! Retained ED2K, eMule, and Kad opcode names.

/// Client/server ED2K TCP and UDP opcodes retained by raria.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum ServerOpcode {
    /// Server login request.
    LoginRequest = 0x01,
    /// Server rejection response.
    Reject = 0x05,
    /// Server-list request.
    GetServerList = 0x14,
    /// Shared-file offer.
    OfferFiles = 0x15,
    /// Server keyword search request.
    SearchRequest = 0x16,
    /// Disconnect request.
    Disconnect = 0x18,
    /// Source request.
    GetSources = 0x19,
    /// LowID callback request.
    CallbackRequest = 0x1c,
    /// Request for the next search result page.
    QueryMoreResults = 0x21,
    /// Obfuscation-aware source request.
    GetSourcesObfuscated = 0x23,
    /// Server-list response.
    ServerList = 0x32,
    /// Server search result response.
    SearchResult = 0x33,
    /// Server status response.
    ServerStatus = 0x34,
    /// Callback accepted response.
    CallbackRequested = 0x35,
    /// Callback failure response.
    CallbackFailed = 0x36,
    /// Server message response.
    ServerMessage = 0x38,
    /// Client ID change response.
    IdChange = 0x40,
    /// Server identity response.
    ServerIdentity = 0x41,
    /// Found sources response.
    FoundSources = 0x42,
    /// User-list response.
    UsersList = 0x43,
    /// Obfuscation-aware found sources response.
    FoundSourcesObfuscated = 0x44,
    /// Server UDP search request with tag set.
    GlobalSearchRequest3 = 0x90,
    /// Server UDP search request.
    GlobalSearchRequest2 = 0x92,
    /// Server UDP source request with file size.
    GlobalGetSources2 = 0x94,
    /// Server UDP status request.
    GlobalServerStatusRequest = 0x96,
    /// Server UDP status response.
    GlobalServerStatusResponse = 0x97,
    /// Server UDP legacy search request retained only for useful peers.
    GlobalSearchRequest = 0x98,
    /// Server UDP search response.
    GlobalSearchResponse = 0x99,
    /// Server UDP source request.
    GlobalGetSources = 0x9a,
    /// Server UDP found sources response.
    GlobalFoundSources = 0x9b,
    /// Server UDP callback request.
    GlobalCallbackRequest = 0x9c,
    /// Server UDP invalid LowID response.
    InvalidLowId = 0x9e,
    /// Server UDP peer-list request.
    ServerListRequest = 0xa0,
    /// Server UDP peer-list response.
    ServerListResponse = 0xa1,
    /// Server UDP description request.
    ServerDescriptionRequest = 0xa2,
    /// Server UDP description response.
    ServerDescriptionResponse = 0xa3,
    /// Server UDP peer-list request without endpoint.
    ServerListRequest2 = 0xa4,
}

impl ServerOpcode {
    /// Return a retained server opcode by wire value.
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Self::LoginRequest,
            0x05 => Self::Reject,
            0x14 => Self::GetServerList,
            0x15 => Self::OfferFiles,
            0x16 => Self::SearchRequest,
            0x18 => Self::Disconnect,
            0x19 => Self::GetSources,
            0x1c => Self::CallbackRequest,
            0x21 => Self::QueryMoreResults,
            0x23 => Self::GetSourcesObfuscated,
            0x32 => Self::ServerList,
            0x33 => Self::SearchResult,
            0x34 => Self::ServerStatus,
            0x35 => Self::CallbackRequested,
            0x36 => Self::CallbackFailed,
            0x38 => Self::ServerMessage,
            0x40 => Self::IdChange,
            0x41 => Self::ServerIdentity,
            0x42 => Self::FoundSources,
            0x43 => Self::UsersList,
            0x44 => Self::FoundSourcesObfuscated,
            0x90 => Self::GlobalSearchRequest3,
            0x92 => Self::GlobalSearchRequest2,
            0x94 => Self::GlobalGetSources2,
            0x96 => Self::GlobalServerStatusRequest,
            0x97 => Self::GlobalServerStatusResponse,
            0x98 => Self::GlobalSearchRequest,
            0x99 => Self::GlobalSearchResponse,
            0x9a => Self::GlobalGetSources,
            0x9b => Self::GlobalFoundSources,
            0x9c => Self::GlobalCallbackRequest,
            0x9e => Self::InvalidLowId,
            0xa0 => Self::ServerListRequest,
            0xa1 => Self::ServerListResponse,
            0xa2 => Self::ServerDescriptionRequest,
            0xa3 => Self::ServerDescriptionResponse,
            0xa4 => Self::ServerListRequest2,
            _ => return None,
        })
    }
}

impl From<ServerOpcode> for u8 {
    fn from(value: ServerOpcode) -> Self {
        value as u8
    }
}

/// Client/client ED2K and eMule opcodes retained by raria.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum PeerOpcode {
    /// Peer hello.
    Hello = 0x01,
    /// Normal part payload.
    SendingPart = 0x46,
    /// Normal part request.
    RequestParts = 0x47,
    /// Requested file not found.
    FileRequestNoFile = 0x48,
    /// Peer download end notification.
    EndOfDownload = 0x49,
    /// Shared-file list request.
    AskSharedFiles = 0x4a,
    /// Shared-file list response.
    AskSharedFilesAnswer = 0x4b,
    /// Peer hello response.
    HelloAnswer = 0x4c,
    /// Client ID change.
    ChangeClientId = 0x4d,
    /// Requested file selection.
    SetRequestedFileId = 0x4f,
    /// File part availability response.
    FileStatus = 0x50,
    /// Hashset request.
    HashsetRequest = 0x51,
    /// Hashset response.
    HashsetAnswer = 0x52,
    /// Upload queue request.
    StartUploadRequest = 0x54,
    /// Upload accepted response.
    AcceptUploadRequest = 0x55,
    /// Transfer cancel.
    CancelTransfer = 0x56,
    /// Peer has no requested parts.
    OutOfPartRequests = 0x57,
    /// Filename request.
    RequestFileName = 0x58,
    /// Filename response.
    RequestFileNameAnswer = 0x59,
    /// Queue rank response.
    QueueRank = 0x5c,
    /// Compressed part payload.
    CompressedPart = 0x40,
    /// eMule queue ranking response.
    QueueRanking = 0x60,
    /// File description.
    FileDescription = 0x61,
    /// Source Exchange request.
    RequestSources = 0x81,
    /// Source Exchange response.
    AnswerSources = 0x82,
    /// Source Exchange v2 request.
    RequestSources2 = 0x83,
    /// Source Exchange v2 response.
    AnswerSources2 = 0x84,
    /// LowID callback.
    Callback = 0x99,
    /// Server-mediated callback request.
    ReaskCallbackTcp = 0x9a,
    /// AICH recovery request.
    AichRequest = 0x9b,
    /// AICH recovery response.
    AichAnswer = 0x9c,
    /// AICH file hash response.
    AichFileHashAnswer = 0x9d,
    /// AICH file hash request.
    AichFileHashRequest = 0x9e,
    /// Compressed I64 part payload.
    CompressedPartI64 = 0xa1,
    /// I64 part payload.
    SendingPartI64 = 0xa2,
    /// I64 part request.
    RequestPartsI64 = 0xa3,
    /// UDP queue reask.
    ReaskFilePing = 0x90,
    /// UDP queue rank response.
    ReaskAck = 0x91,
    /// UDP file-not-found response.
    FileNotFound = 0x92,
    /// UDP queue-full response.
    QueueFull = 0x93,
    /// UDP callback reask.
    ReaskCallbackUdp = 0x94,
}

impl PeerOpcode {
    /// Return a retained peer opcode by wire value.
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Self::Hello,
            0x40 => Self::CompressedPart,
            0x46 => Self::SendingPart,
            0x47 => Self::RequestParts,
            0x48 => Self::FileRequestNoFile,
            0x49 => Self::EndOfDownload,
            0x4a => Self::AskSharedFiles,
            0x4b => Self::AskSharedFilesAnswer,
            0x4c => Self::HelloAnswer,
            0x4d => Self::ChangeClientId,
            0x4f => Self::SetRequestedFileId,
            0x50 => Self::FileStatus,
            0x51 => Self::HashsetRequest,
            0x52 => Self::HashsetAnswer,
            0x54 => Self::StartUploadRequest,
            0x55 => Self::AcceptUploadRequest,
            0x56 => Self::CancelTransfer,
            0x57 => Self::OutOfPartRequests,
            0x58 => Self::RequestFileName,
            0x59 => Self::RequestFileNameAnswer,
            0x5c => Self::QueueRank,
            0x60 => Self::QueueRanking,
            0x61 => Self::FileDescription,
            0x81 => Self::RequestSources,
            0x82 => Self::AnswerSources,
            0x83 => Self::RequestSources2,
            0x84 => Self::AnswerSources2,
            0x90 => Self::ReaskFilePing,
            0x91 => Self::ReaskAck,
            0x92 => Self::FileNotFound,
            0x93 => Self::QueueFull,
            0x94 => Self::ReaskCallbackUdp,
            0x99 => Self::Callback,
            0x9a => Self::ReaskCallbackTcp,
            0x9b => Self::AichRequest,
            0x9c => Self::AichAnswer,
            0x9d => Self::AichFileHashAnswer,
            0x9e => Self::AichFileHashRequest,
            0xa1 => Self::CompressedPartI64,
            0xa2 => Self::SendingPartI64,
            0xa3 => Self::RequestPartsI64,
            _ => return None,
        })
    }
}

impl From<PeerOpcode> for u8 {
    fn from(value: PeerOpcode) -> Self {
        value as u8
    }
}

/// eMule Kad UDP opcodes retained by raria.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum KadOpcode {
    /// Kad v2 bootstrap request.
    BootstrapRequestV2 = 0x01,
    /// Kad v2 bootstrap response.
    BootstrapResponseV2 = 0x09,
    /// Kad v2 hello request.
    HelloRequestV2 = 0x11,
    /// Kad v2 hello response.
    HelloResponseV2 = 0x19,
    /// Kad v2 lookup request.
    RequestV2 = 0x21,
    /// Kad v2 hello acknowledgement.
    HelloResponseAckV2 = 0x22,
    /// Kad v2 lookup response.
    ResponseV2 = 0x29,
    /// Kad v2 keyword search request.
    SearchKeyRequestV2 = 0x33,
    /// Kad v2 source search request.
    SearchSourceRequestV2 = 0x34,
    /// Kad v2 notes search request.
    SearchNotesRequestV2 = 0x35,
    /// Kad v2 search response.
    SearchResponseV2 = 0x3b,
    /// Kad v2 keyword publish request.
    PublishKeyRequestV2 = 0x43,
    /// Kad v2 source publish request.
    PublishSourceRequestV2 = 0x44,
    /// Kad v2 notes publish request.
    PublishNotesRequestV2 = 0x45,
    /// Kad v2 publish response.
    PublishResponseV2 = 0x4b,
    /// Kad v2 publish acknowledgement.
    PublishResponseAckV2 = 0x4c,
    /// Kad firewall request.
    FirewalledRequest = 0x50,
    /// Kad find-buddy request.
    FindBuddyRequest = 0x51,
    /// Kad callback request.
    CallbackRequest = 0x52,
    /// Kad v2 firewall request.
    FirewalledRequestV2 = 0x53,
    /// Kad firewall response.
    FirewalledResponse = 0x58,
    /// Kad firewall acknowledgement.
    FirewalledAckResponse = 0x59,
    /// Kad find-buddy response.
    FindBuddyResponse = 0x5a,
    /// Kad v2 ping.
    PingV2 = 0x60,
    /// Kad v2 pong.
    PongV2 = 0x61,
    /// Kad v2 UDP firewall probe.
    FirewallUdpV2 = 0x62,
}

impl KadOpcode {
    /// Return a retained Kad opcode by wire value.
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Self::BootstrapRequestV2,
            0x09 => Self::BootstrapResponseV2,
            0x11 => Self::HelloRequestV2,
            0x19 => Self::HelloResponseV2,
            0x21 => Self::RequestV2,
            0x22 => Self::HelloResponseAckV2,
            0x29 => Self::ResponseV2,
            0x33 => Self::SearchKeyRequestV2,
            0x34 => Self::SearchSourceRequestV2,
            0x35 => Self::SearchNotesRequestV2,
            0x3b => Self::SearchResponseV2,
            0x43 => Self::PublishKeyRequestV2,
            0x44 => Self::PublishSourceRequestV2,
            0x45 => Self::PublishNotesRequestV2,
            0x4b => Self::PublishResponseV2,
            0x4c => Self::PublishResponseAckV2,
            0x50 => Self::FirewalledRequest,
            0x51 => Self::FindBuddyRequest,
            0x52 => Self::CallbackRequest,
            0x53 => Self::FirewalledRequestV2,
            0x58 => Self::FirewalledResponse,
            0x59 => Self::FirewalledAckResponse,
            0x5a => Self::FindBuddyResponse,
            0x60 => Self::PingV2,
            0x61 => Self::PongV2,
            0x62 => Self::FirewallUdpV2,
            _ => return None,
        })
    }
}

impl From<KadOpcode> for u8 {
    fn from(value: KadOpcode) -> Self {
        value as u8
    }
}
