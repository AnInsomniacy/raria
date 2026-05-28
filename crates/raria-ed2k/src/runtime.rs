//! ED2K daemon runtime context and scheduling ownership.

use anyhow::Context;
use raria_core::config::GlobalConfig;
use raria_core::native::TaskId;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

use crate::hash::Ed2kHash;
use crate::kad::{
    KadContact, KadFirewallObservation, KadFirewallState, KadRoutingTable, KadSearchEntry,
    KadTransactionPurpose, KadTransactionTable, build_kad_keyword_search_request,
    build_kad_publish_source_request, build_kad_source_search_request, dedupe_kad_search_entries,
    extract_kad_source_entries, parse_kad_search_result,
};
use crate::opcode::{KadOpcode, ServerOpcode};
use crate::packet::{
    PacketFrame, Protocol, decode_tcp_frame, decode_udp_datagram, encode_tcp_frame,
    encode_udp_datagram,
};
use crate::peer::PeerEndpoint;
use crate::server::{
    FoundSource, ServerTcpState, ServerUdpState, ServerUdpStatus, build_get_sources_request,
    build_global_get_sources_request, build_login_request, build_udp_status_request,
    parse_udp_found_sources_payloads,
};
use crate::source::SourceExchangeEntry;

/// Native ED2K runtime configuration projected from `raria.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kRuntimeConfig {
    /// Whether server source discovery is enabled.
    pub enable_servers: bool,
    /// Whether Kad source discovery is enabled.
    pub enable_kad: bool,
    /// Local ED2K TCP listen port.
    pub listen_tcp_port: u16,
    /// Local ED2K UDP listen port.
    pub listen_udp_port: u16,
    /// Whether the runtime should start from a firewalled assumption.
    pub assume_firewalled: bool,
    /// Maximum retained sources per task.
    pub max_sources_per_task: u32,
    /// Maximum upload slots for shared files.
    pub max_upload_slots: u16,
    /// Whether completed files should enter native sharing.
    pub share_completed: bool,
}

impl Ed2kRuntimeConfig {
    /// Project native ED2K settings from the global daemon config.
    pub fn from_global_config(config: &GlobalConfig) -> Self {
        Self {
            enable_servers: config.ed2k_enable_servers,
            enable_kad: config.ed2k_enable_kad,
            listen_tcp_port: config.ed2k_listen_tcp_port,
            listen_udp_port: config.ed2k_listen_udp_port,
            assume_firewalled: config.ed2k_assume_firewalled,
            max_sources_per_task: config.ed2k_max_sources_per_task,
            max_upload_slots: config.ed2k_max_upload_slots,
            share_completed: config.ed2k_share_completed,
        }
    }
}

/// ED2K runtime event class before projection into raria-native event names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ed2kRuntimeEventKind {
    /// Source discovery state changed.
    Source,
    /// Peer queue state changed.
    Queue,
    /// Kad discovery state changed.
    Kad,
    /// Transfer state changed.
    Transfer,
    /// Sharing state changed.
    Sharing,
    /// Upload state changed.
    Upload,
}

/// Compact ED2K runtime status emitted by the runtime scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kRuntimeStatus {
    /// Runtime event class.
    pub event_kind: Ed2kRuntimeEventKind,
    /// Status category exposed in native events.
    pub category: &'static str,
    /// Stable state name exposed in native events.
    pub state: &'static str,
    /// Optional concise status message.
    pub message: Option<&'static str>,
    /// Numeric status metrics.
    pub metrics: BTreeMap<String, u64>,
}

/// Native ED2K runtime context for one raria task.
#[derive(Debug, Clone)]
pub struct Ed2kRuntimeContext {
    task_id: TaskId,
    identity_profile_id: String,
    config: Ed2kRuntimeConfig,
    state: Ed2kRuntimeState,
    scheduler_ticks: u64,
    last_tick_elapsed: Duration,
}

impl Ed2kRuntimeContext {
    /// Create a runtime context for one native ED2K task.
    pub fn new(task_id: TaskId, config: Ed2kRuntimeConfig) -> Self {
        Self {
            task_id,
            identity_profile_id: "default".to_string(),
            config,
            state: Ed2kRuntimeState::default(),
            scheduler_ticks: 0,
            last_tick_elapsed: Duration::ZERO,
        }
    }

    /// Return the native task id owned by this runtime context.
    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }

    /// Return the native ED2K identity profile used by this runtime.
    pub fn identity_profile_id(&self) -> &str {
        &self.identity_profile_id
    }

    /// Return the projected runtime config.
    pub fn config(&self) -> &Ed2kRuntimeConfig {
        &self.config
    }

    /// Return the runtime-owned state snapshot.
    pub fn state(&self) -> &Ed2kRuntimeState {
        &self.state
    }

    /// Return startup statuses before any network loop runs.
    pub fn startup_statuses(&self) -> Vec<Ed2kRuntimeStatus> {
        vec![
            self.status(
                Ed2kRuntimeEventKind::Source,
                "source",
                if self.config.enable_servers || self.config.enable_kad {
                    "discovering"
                } else {
                    "disabled"
                },
                Some("ED2K runtime scheduler initialized"),
                [
                    ("knownSources", self.state.source.known_sources),
                    ("activeSources", self.state.source.active_sources),
                    ("schedulerTicks", self.scheduler_ticks),
                ],
            ),
            self.status(
                Ed2kRuntimeEventKind::Queue,
                "queue",
                "ready",
                None,
                [("waitingUploadPeers", self.state.queue.waiting_upload_peers)],
            ),
            self.status(
                Ed2kRuntimeEventKind::Kad,
                "kad",
                if self.config.enable_kad {
                    "discovering"
                } else {
                    "disabled"
                },
                None,
                [("knownKadContacts", self.state.kad.known_contacts)],
            ),
            self.status(
                Ed2kRuntimeEventKind::Sharing,
                "sharing",
                if self.config.share_completed {
                    "enabled"
                } else {
                    "disabled"
                },
                None,
                [("sharedFiles", self.state.sharing.shared_files)],
            ),
        ]
    }

    /// Advance bounded scheduler status for one elapsed timestamp.
    pub fn tick(&mut self, elapsed: Duration) -> Vec<Ed2kRuntimeStatus> {
        if elapsed == Duration::ZERO || elapsed <= self.last_tick_elapsed {
            return Vec::new();
        }
        self.last_tick_elapsed = elapsed;
        self.scheduler_ticks = self.scheduler_ticks.saturating_add(1);
        vec![
            self.status(
                Ed2kRuntimeEventKind::Source,
                "source",
                if self.config.enable_servers || self.config.enable_kad {
                    "discovering"
                } else {
                    "disabled"
                },
                None,
                [
                    ("knownSources", self.state.source.known_sources),
                    ("activeSources", self.state.source.active_sources),
                    ("schedulerTicks", self.scheduler_ticks),
                ],
            ),
            self.status(
                Ed2kRuntimeEventKind::Transfer,
                "transfer",
                "idle",
                None,
                [
                    ("activePeers", self.state.transfer.active_peers),
                    ("schedulerTicks", self.scheduler_ticks),
                ],
            ),
        ]
    }

    /// Record sources discovered by a live server runtime exchange.
    pub fn record_server_sources(&mut self, discovered: usize) -> Ed2kRuntimeStatus {
        self.state.source.known_sources = self
            .state
            .source
            .known_sources
            .saturating_add(discovered as u64)
            .min(u64::from(self.config.max_sources_per_task));
        self.status(
            Ed2kRuntimeEventKind::Source,
            "source",
            if self.state.source.known_sources > 0 {
                "discovered"
            } else {
                "discovering"
            },
            None,
            [
                ("knownSources", self.state.source.known_sources),
                ("activeSources", self.state.source.active_sources),
                ("schedulerTicks", self.scheduler_ticks),
            ],
        )
    }

    fn status<const N: usize>(
        &self,
        event_kind: Ed2kRuntimeEventKind,
        category: &'static str,
        state: &'static str,
        message: Option<&'static str>,
        metrics: [(&'static str, u64); N],
    ) -> Ed2kRuntimeStatus {
        Ed2kRuntimeStatus {
            event_kind,
            category,
            state,
            message,
            metrics: metrics
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        }
    }
}

/// Runtime-owned state for one ED2K task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kRuntimeState {
    /// Source discovery state.
    pub source: Ed2kSourceRuntimeState,
    /// Peer queue state.
    pub queue: Ed2kQueueRuntimeState,
    /// Kad discovery state.
    pub kad: Ed2kKadRuntimeState,
    /// Transfer worker state.
    pub transfer: Ed2kTransferRuntimeState,
    /// Sharing state.
    pub sharing: Ed2kSharingRuntimeState,
}

/// ED2K source discovery counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kSourceRuntimeState {
    /// Number of retained useful sources.
    pub known_sources: u64,
    /// Number of sources currently scheduled.
    pub active_sources: u64,
}

/// ED2K peer queue counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kQueueRuntimeState {
    /// Peers currently waiting for upload service.
    pub waiting_upload_peers: u64,
}

/// ED2K Kad counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kKadRuntimeState {
    /// Retained Kad contacts.
    pub known_contacts: u64,
}

/// ED2K transfer counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kTransferRuntimeState {
    /// Live peer transfer workers.
    pub active_peers: u64,
}

/// ED2K sharing counters owned by the runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed2kSharingRuntimeState {
    /// Files currently available through native ED2K sharing.
    pub shared_files: u64,
}

/// ED2K server endpoint used by live server runtime probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kServerEndpoint {
    /// Server host or address.
    pub host: String,
    /// TCP server port.
    pub tcp_port: u16,
    /// UDP server port.
    pub udp_port: u16,
}

impl Ed2kServerEndpoint {
    /// Create a server endpoint.
    pub fn new(host: impl Into<String>, tcp_port: u16, udp_port: u16) -> Self {
        Self {
            host: host.into(),
            tcp_port,
            udp_port,
        }
    }
}

/// Source-discovery query sent to ED2K servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kSourceQuery {
    /// Target file ED2K root hash.
    pub file_hash: Ed2kHash,
    /// Target file size.
    pub file_size: u64,
}

/// Live ED2K server runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kServerRuntimeConfig {
    /// Local ED2K client hash used for server login.
    pub client_hash: Ed2kHash,
    /// Local client id before a server assigns HighID or LowID.
    pub client_id: u32,
    /// Local TCP listen port advertised during server login.
    pub listen_tcp_port: u16,
    /// Native client nickname.
    pub nickname: String,
    /// Native client version advertised to servers.
    pub client_version: u32,
    /// eMule-compatible feature version advertised to servers.
    pub emule_version: u32,
    /// Maximum accepted packet payload size.
    pub max_packet_size: usize,
    /// Per-operation network timeout.
    pub io_timeout: Duration,
    /// Local UDP bind address.
    pub udp_bind_addr: String,
}

impl Default for Ed2kServerRuntimeConfig {
    fn default() -> Self {
        Self {
            client_hash: [0x11; 16],
            client_id: 0,
            listen_tcp_port: 4662,
            nickname: "raria".to_string(),
            client_version: 0x3c,
            emule_version: 0x0102_0304,
            max_packet_size: 1024 * 1024,
            io_timeout: Duration::from_secs(5),
            udp_bind_addr: "0.0.0.0:0".to_string(),
        }
    }
}

/// Live ED2K server runtime owner.
#[derive(Debug, Clone)]
pub struct Ed2kServerRuntime {
    config: Ed2kServerRuntimeConfig,
}

impl Ed2kServerRuntime {
    /// Create an ED2K server runtime.
    pub fn new(config: Ed2kServerRuntimeConfig) -> Self {
        Self { config }
    }

    /// Run one TCP server login and source-discovery exchange.
    pub async fn query_tcp_sources(
        &self,
        endpoint: Ed2kServerEndpoint,
        query: Ed2kSourceQuery,
    ) -> anyhow::Result<Ed2kTcpServerReport> {
        let mut stream = tokio::time::timeout(
            self.config.io_timeout,
            TcpStream::connect((endpoint.host.as_str(), endpoint.tcp_port)),
        )
        .await
        .context("ED2K server TCP connect timed out")?
        .context("ED2K server TCP connect failed")?;
        let login = build_login_request(
            self.config.client_hash,
            self.config.client_id,
            self.config.listen_tcp_port,
            &self.config.nickname,
            self.config.client_version,
            self.config.emule_version,
        )?;
        write_tcp_frame(&mut stream, &login, self.config.max_packet_size).await?;
        let source_request = build_get_sources_request(query.file_hash, query.file_size, false)?;
        write_tcp_frame(&mut stream, &source_request, self.config.max_packet_size).await?;

        let mut state = ServerTcpState::new(endpoint.host, endpoint.tcp_port);
        let mut sources = Vec::new();
        for _ in 0..8 {
            let frame = read_tcp_frame(
                &mut stream,
                self.config.max_packet_size,
                self.config.io_timeout,
            )
            .await?;
            match state.apply_frame(&frame)? {
                crate::server::ServerTcpEvent::FoundSources {
                    file_hash,
                    sources: found,
                } if file_hash == query.file_hash => {
                    sources.extend(found);
                    break;
                }
                _ => {}
            }
        }
        Ok(Ed2kTcpServerReport { state, sources })
    }

    /// Run one UDP server status and source-discovery exchange.
    pub async fn query_udp_sources(
        &self,
        endpoint: Ed2kServerEndpoint,
        query: Ed2kSourceQuery,
        challenge: u32,
    ) -> anyhow::Result<Ed2kUdpServerReport> {
        let socket = UdpSocket::bind(&self.config.udp_bind_addr)
            .await
            .context("ED2K UDP bind failed")?;
        let server_addr = resolve_udp_endpoint(&endpoint).await?;
        let status_request = build_udp_status_request(challenge);
        write_udp_frame(
            &socket,
            server_addr,
            &status_request,
            self.config.max_packet_size,
        )
        .await?;
        let source_request =
            build_global_get_sources_request(query.file_hash, query.file_size, true);
        write_udp_frame(
            &socket,
            server_addr,
            &source_request,
            self.config.max_packet_size,
        )
        .await?;

        let mut state = ServerUdpState::new(challenge);
        let mut sources = Vec::new();
        for _ in 0..4 {
            let frame =
                read_udp_frame(&socket, self.config.max_packet_size, self.config.io_timeout)
                    .await?;
            match ServerOpcode::from_byte(frame.opcode) {
                Some(ServerOpcode::GlobalServerStatusResponse) => {
                    state.apply_status_response(&frame.payload)?;
                }
                Some(ServerOpcode::GlobalFoundSources) => {
                    sources.extend(parse_udp_found_sources_payloads(
                        &frame.payload,
                        query.file_hash,
                    )?);
                    break;
                }
                _ => {}
            }
        }
        Ok(Ed2kUdpServerReport {
            status: state.status,
            sources,
        })
    }
}

/// Result of one TCP ED2K server source-discovery exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kTcpServerReport {
    /// Final TCP server state.
    pub state: ServerTcpState,
    /// Sources discovered for the requested file.
    pub sources: Vec<FoundSource>,
}

/// Result of one UDP ED2K server source-discovery exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kUdpServerReport {
    /// Server UDP status when returned.
    pub status: Option<ServerUdpStatus>,
    /// Sources discovered for the requested file.
    pub sources: Vec<FoundSource>,
}

/// Live Kad runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kKadRuntimeConfig {
    /// Local Kad id.
    pub self_id: Ed2kHash,
    /// Useful routing bucket size.
    pub bucket_size: usize,
    /// Maximum accepted packet payload size.
    pub max_packet_size: usize,
    /// Per-operation network timeout.
    pub io_timeout: Duration,
    /// Local UDP bind address.
    pub udp_bind_addr: String,
    /// Whether to start from a firewalled assumption.
    pub assume_firewalled: bool,
}

impl Default for Ed2kKadRuntimeConfig {
    fn default() -> Self {
        Self {
            self_id: [0x22; 16],
            bucket_size: crate::kad::DEFAULT_KAD_BUCKET_SIZE,
            max_packet_size: 1024 * 1024,
            io_timeout: Duration::from_secs(5),
            udp_bind_addr: "0.0.0.0:0".to_string(),
            assume_firewalled: false,
        }
    }
}

/// Source lookup request for the Kad runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kKadSourceLookup {
    /// Target ED2K file id.
    pub target_id: Ed2kHash,
    /// Target file size.
    pub file_size: u64,
    /// Seed contacts used by this bounded lookup.
    pub seeds: Vec<KadContact>,
}

/// Keyword lookup request for the Kad runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kKeywordLookup {
    /// Kad keyword target id.
    pub target_id: Ed2kHash,
    /// Contacts used by this bounded lookup.
    pub contacts: Vec<KadContact>,
}

/// Source publish request for the Kad runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kKadSourcePublish {
    /// ED2K file id being published.
    pub file_id: Ed2kHash,
    /// File size.
    pub file_size: u64,
    /// Source endpoint to publish.
    pub source: PeerEndpoint,
    /// Source Kad id.
    pub source_id: Ed2kHash,
    /// Remote Kad contact that receives the publish.
    pub contact: KadContact,
    /// Whether sharing policy allows publishing.
    pub sharing_enabled: bool,
}

/// Kad source lookup result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kKadSourceLookupReport {
    /// Contacts confirmed by live UDP responses.
    pub confirmed_contacts: usize,
    /// Sources returned by Kad search responses.
    pub sources: Vec<SourceExchangeEntry>,
}

/// Kad keyword lookup result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kKeywordLookupReport {
    /// Deduplicated keyword result entries.
    pub results: Vec<KadSearchEntry>,
}

/// Kad source publish result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed2kKadSourcePublishReport {
    /// Whether a source publish payload was sent and acknowledged.
    pub published: bool,
    /// Whether UDP reachability was observed.
    pub udp_reachable: bool,
}

/// Live Kad UDP runtime owner.
#[derive(Debug, Clone)]
pub struct Ed2kKadRuntime {
    config: Ed2kKadRuntimeConfig,
    routing: KadRoutingTable,
    firewall: KadFirewallState,
    transactions: KadTransactionTable,
}

impl Ed2kKadRuntime {
    /// Create a Kad runtime.
    pub fn new(config: Ed2kKadRuntimeConfig) -> Self {
        Self {
            routing: KadRoutingTable::new(config.self_id, config.bucket_size),
            firewall: KadFirewallState::new(config.assume_firewalled),
            transactions: KadTransactionTable::default(),
            config,
        }
    }

    /// Borrow current routing table state.
    pub fn routing(&self) -> &KadRoutingTable {
        &self.routing
    }

    /// Borrow current firewall state.
    pub fn firewall(&self) -> &KadFirewallState {
        &self.firewall
    }

    /// Run one bounded Kad source lookup over UDP.
    pub async fn lookup_sources(
        &mut self,
        lookup: Ed2kKadSourceLookup,
        now_seconds: u64,
    ) -> anyhow::Result<Ed2kKadSourceLookupReport> {
        let socket = self.bind_udp().await?;
        let mut confirmed_contacts = 0;
        let mut sources = Vec::new();
        for contact in lookup.seeds {
            let endpoint = crate::kad::KadEndpoint::new(contact.host.clone(), contact.udp_port);
            let addr = resolve_kad_contact(&contact).await?;
            let hello = PacketFrame {
                protocol: Protocol::Kad,
                opcode: KadOpcode::HelloRequestV2.into(),
                payload: self.config.self_id.to_vec(),
            };
            write_udp_frame(&socket, addr, &hello, self.config.max_packet_size).await?;
            self.transactions.add(
                endpoint.clone(),
                KadOpcode::HelloRequestV2,
                None,
                KadTransactionPurpose::Hello,
                now_seconds,
            );
            let Some(hello_response) = read_kad_udp_frame_optional(
                &socket,
                self.config.max_packet_size,
                self.config.io_timeout,
            )
            .await?
            else {
                self.expire_stale_transactions(now_seconds);
                self.routing.node_failed(&contact.id, now_seconds);
                continue;
            };
            if KadOpcode::from_byte(hello_response.opcode) != Some(KadOpcode::HelloResponseV2) {
                self.transactions
                    .complete(&endpoint, KadOpcode::HelloRequestV2);
                self.routing.node_failed(&contact.id, now_seconds);
                continue;
            }
            self.transactions
                .complete(&endpoint, KadOpcode::HelloRequestV2);
            self.routing.node_seen(contact.clone(), now_seconds)?;
            confirmed_contacts += 1;

            let search = PacketFrame {
                protocol: Protocol::Kad,
                opcode: KadOpcode::SearchSourceRequestV2.into(),
                payload: build_kad_source_search_request(lookup.target_id, 0, lookup.file_size),
            };
            write_udp_frame(&socket, addr, &search, self.config.max_packet_size).await?;
            self.transactions.add(
                endpoint.clone(),
                KadOpcode::SearchSourceRequestV2,
                Some(lookup.target_id),
                KadTransactionPurpose::Search,
                now_seconds,
            );
            let Some(search_response) = read_kad_udp_frame_optional(
                &socket,
                self.config.max_packet_size,
                self.config.io_timeout,
            )
            .await?
            else {
                self.expire_stale_transactions(now_seconds);
                self.routing.node_failed(&contact.id, now_seconds);
                continue;
            };
            if KadOpcode::from_byte(search_response.opcode) != Some(KadOpcode::SearchResponseV2) {
                self.transactions.complete_with_target(
                    &endpoint,
                    KadOpcode::SearchSourceRequestV2,
                    &lookup.target_id,
                );
                self.routing.node_failed(&contact.id, now_seconds);
                continue;
            }
            self.transactions.complete_with_target(
                &endpoint,
                KadOpcode::SearchSourceRequestV2,
                &lookup.target_id,
            );
            let result = parse_kad_search_result(&search_response.payload)?;
            if result.target_id == lookup.target_id {
                sources.extend(extract_kad_source_entries(&result));
            }
        }
        Ok(Ed2kKadSourceLookupReport {
            confirmed_contacts,
            sources,
        })
    }

    /// Run one bounded Kad keyword lookup over UDP.
    pub async fn lookup_keyword(
        &self,
        lookup: Ed2kKeywordLookup,
        now_seconds: u64,
    ) -> anyhow::Result<Ed2kKeywordLookupReport> {
        let socket = self.bind_udp().await?;
        let mut results = Vec::new();
        let mut transactions = KadTransactionTable::default();
        for contact in lookup.contacts {
            let endpoint = crate::kad::KadEndpoint::new(contact.host.clone(), contact.udp_port);
            let addr = resolve_kad_contact(&contact).await?;
            let search = PacketFrame {
                protocol: Protocol::Kad,
                opcode: KadOpcode::SearchKeyRequestV2.into(),
                payload: build_kad_keyword_search_request(lookup.target_id, 0),
            };
            write_udp_frame(&socket, addr, &search, self.config.max_packet_size).await?;
            transactions.add(
                endpoint.clone(),
                KadOpcode::SearchKeyRequestV2,
                Some(lookup.target_id),
                KadTransactionPurpose::Search,
                now_seconds,
            );
            let Some(response) = read_kad_udp_frame_optional(
                &socket,
                self.config.max_packet_size,
                self.config.io_timeout,
            )
            .await?
            else {
                transactions.expire(now_seconds.saturating_add(1), 1);
                continue;
            };
            if KadOpcode::from_byte(response.opcode) != Some(KadOpcode::SearchResponseV2) {
                transactions.complete_with_target(
                    &endpoint,
                    KadOpcode::SearchKeyRequestV2,
                    &lookup.target_id,
                );
                continue;
            }
            transactions.complete_with_target(
                &endpoint,
                KadOpcode::SearchKeyRequestV2,
                &lookup.target_id,
            );
            let result = parse_kad_search_result(&response.payload)?;
            if result.target_id == lookup.target_id {
                results.extend(result.entries);
            }
        }
        Ok(Ed2kKeywordLookupReport {
            results: dedupe_kad_search_entries(results),
        })
    }

    /// Run one bounded Kad source publish and firewall check over UDP.
    pub async fn publish_source(
        &mut self,
        publish: Ed2kKadSourcePublish,
        now_seconds: u64,
    ) -> anyhow::Result<Ed2kKadSourcePublishReport> {
        let Some(payload) = build_kad_publish_source_request(
            publish.file_id,
            publish.source,
            publish.source_id,
            publish.file_size,
            publish.sharing_enabled,
        )?
        else {
            return Ok(Ed2kKadSourcePublishReport {
                published: false,
                udp_reachable: self.firewall.udp_reachable(),
            });
        };
        let socket = self.bind_udp().await?;
        let endpoint =
            crate::kad::KadEndpoint::new(publish.contact.host.clone(), publish.contact.udp_port);
        let addr = resolve_kad_contact(&publish.contact).await?;
        let request = PacketFrame {
            protocol: Protocol::Kad,
            opcode: KadOpcode::PublishSourceRequestV2.into(),
            payload,
        };
        write_udp_frame(&socket, addr, &request, self.config.max_packet_size).await?;
        self.transactions.add(
            endpoint.clone(),
            KadOpcode::PublishSourceRequestV2,
            Some(publish.file_id),
            KadTransactionPurpose::Publish,
            now_seconds,
        );
        let publish_response = read_kad_udp_frame_optional(
            &socket,
            self.config.max_packet_size,
            self.config.io_timeout,
        )
        .await?;
        let mut published = false;
        if publish_response
            .as_ref()
            .and_then(|frame| KadOpcode::from_byte(frame.opcode))
            == Some(KadOpcode::PublishResponseV2)
        {
            self.transactions.complete_with_target(
                &endpoint,
                KadOpcode::PublishSourceRequestV2,
                &publish.file_id,
            );
            published = true;
        } else {
            self.expire_stale_transactions(now_seconds);
            self.routing.node_failed(&publish.contact.id, now_seconds);
        }

        let firewall_request = PacketFrame {
            protocol: Protocol::Kad,
            opcode: KadOpcode::FirewalledRequestV2.into(),
            payload: self.config.self_id.to_vec(),
        };
        write_udp_frame(
            &socket,
            addr,
            &firewall_request,
            self.config.max_packet_size,
        )
        .await?;
        self.firewall.record_check_started(now_seconds);
        self.transactions.add(
            endpoint.clone(),
            KadOpcode::FirewalledRequestV2,
            None,
            KadTransactionPurpose::Firewall,
            now_seconds,
        );
        let firewall_response = read_kad_udp_frame_optional(
            &socket,
            self.config.max_packet_size,
            self.config.io_timeout,
        )
        .await?;
        if firewall_response
            .as_ref()
            .and_then(|frame| KadOpcode::from_byte(frame.opcode))
            == Some(KadOpcode::FirewalledResponse)
        {
            self.transactions
                .complete(&endpoint, KadOpcode::FirewalledRequestV2);
            self.firewall
                .record_observation(KadFirewallObservation::UdpOpen, now_seconds);
        } else {
            self.expire_stale_transactions(now_seconds);
            self.firewall
                .record_observation(KadFirewallObservation::UdpFirewalled, now_seconds);
        }

        Ok(Ed2kKadSourcePublishReport {
            published,
            udp_reachable: self.firewall.udp_reachable(),
        })
    }

    async fn bind_udp(&self) -> anyhow::Result<UdpSocket> {
        UdpSocket::bind(&self.config.udp_bind_addr)
            .await
            .context("Kad UDP bind failed")
    }

    fn expire_stale_transactions(&mut self, now_seconds: u64) {
        let timeout_seconds = self.config.io_timeout.as_secs().max(1);
        self.transactions
            .expire(now_seconds.saturating_add(timeout_seconds), timeout_seconds);
    }
}

async fn write_tcp_frame(
    stream: &mut TcpStream,
    frame: &PacketFrame,
    max_packet_size: usize,
) -> anyhow::Result<()> {
    let bytes = encode_tcp_frame(frame, max_packet_size)?;
    stream
        .write_all(&bytes)
        .await
        .context("ED2K TCP write failed")
}

async fn read_tcp_frame(
    stream: &mut TcpStream,
    max_packet_size: usize,
    timeout: Duration,
) -> anyhow::Result<PacketFrame> {
    let mut header = [0_u8; 6];
    tokio::time::timeout(timeout, stream.read_exact(&mut header))
        .await
        .context("ED2K TCP read timed out")?
        .context("ED2K TCP header read failed")?;
    let length = u32::from_le_bytes(header[1..5].try_into()?) as usize;
    anyhow::ensure!(length > 0, "invalid ED2K TCP packet length");
    let payload_len = length - 1;
    let mut payload = vec![0_u8; payload_len];
    tokio::time::timeout(timeout, stream.read_exact(&mut payload))
        .await
        .context("ED2K TCP read timed out")?
        .context("ED2K TCP payload read failed")?;
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&payload);
    Ok(decode_tcp_frame(&bytes, max_packet_size)?)
}

async fn write_udp_frame(
    socket: &UdpSocket,
    server_addr: SocketAddr,
    frame: &PacketFrame,
    max_packet_size: usize,
) -> anyhow::Result<()> {
    let bytes = encode_udp_datagram(frame, max_packet_size)?;
    socket
        .send_to(&bytes, server_addr)
        .await
        .context("ED2K UDP send failed")?;
    Ok(())
}

async fn read_udp_frame(
    socket: &UdpSocket,
    max_packet_size: usize,
    timeout: Duration,
) -> anyhow::Result<PacketFrame> {
    let mut buffer = vec![0_u8; max_packet_size + 2];
    let (len, _) = tokio::time::timeout(timeout, socket.recv_from(&mut buffer))
        .await
        .context("ED2K UDP read timed out")?
        .context("ED2K UDP read failed")?;
    Ok(decode_udp_datagram(&buffer[..len], max_packet_size)?)
}

async fn read_kad_udp_frame_optional(
    socket: &UdpSocket,
    max_packet_size: usize,
    timeout: Duration,
) -> anyhow::Result<Option<PacketFrame>> {
    let mut buffer = vec![0_u8; max_packet_size + 2];
    let Some(result) = tokio::time::timeout(timeout, socket.recv_from(&mut buffer))
        .await
        .ok()
    else {
        return Ok(None);
    };
    let (len, _) = result.context("Kad UDP read failed")?;
    let frame = decode_udp_datagram(&buffer[..len], max_packet_size)?;
    anyhow::ensure!(
        matches!(frame.protocol, Protocol::Kad | Protocol::KadPacked),
        "unexpected Kad UDP protocol"
    );
    Ok(Some(frame))
}

async fn resolve_udp_endpoint(endpoint: &Ed2kServerEndpoint) -> anyhow::Result<SocketAddr> {
    let mut addrs = tokio::net::lookup_host((endpoint.host.as_str(), endpoint.udp_port))
        .await
        .context("ED2K UDP host resolution failed")?;
    addrs
        .next()
        .context("ED2K UDP host resolution returned no addresses")
}

async fn resolve_kad_contact(contact: &KadContact) -> anyhow::Result<SocketAddr> {
    let mut addrs = tokio::net::lookup_host((contact.host.as_str(), contact.udp_port))
        .await
        .context("Kad UDP host resolution failed")?;
    addrs
        .next()
        .context("Kad UDP host resolution returned no addresses")
}
