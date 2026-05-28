//! ED2K peer handshake, capability, queue, and request-state ownership.

use crate::opcode::ServerOpcode;
use crate::packet::{PacketFrame, Protocol};
use crate::server::is_low_id;

/// Endpoint accepted through a server-mediated LowID callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackEndpoint {
    /// ED2K client ID associated with the callback.
    pub client_id: u32,
    /// Callback IP as the ED2K wire integer.
    pub ip: u32,
    /// Callback TCP port.
    pub tcp_port: u16,
    /// Optional crypt option bits reported by the server.
    pub crypt_options: Option<u8>,
    /// Optional peer user hash reported by the server.
    pub user_hash: Option<[u8; 16]>,
}

impl CallbackEndpoint {
    /// Parse a server-mediated callback endpoint payload.
    pub fn parse_server_payload(
        client_id: u32,
        payload: &[u8],
    ) -> Result<Self, CallbackParseError> {
        if payload.len() != 6 && payload.len() < 23 {
            return Err(CallbackParseError::InvalidPayload);
        }
        let ip = u32::from_le_bytes(
            payload[0..4]
                .try_into()
                .map_err(|_| CallbackParseError::InvalidPayload)?,
        );
        let tcp_port = u16::from_le_bytes(
            payload[4..6]
                .try_into()
                .map_err(|_| CallbackParseError::InvalidPayload)?,
        );
        let (crypt_options, user_hash) = if payload.len() >= 23 {
            let mut hash = [0_u8; 16];
            hash.copy_from_slice(&payload[7..23]);
            (Some(payload[6]), Some(hash))
        } else {
            (None, None)
        };
        Ok(Self {
            client_id,
            ip,
            tcp_port,
            crypt_options,
            user_hash,
        })
    }
}

/// Server callback endpoint parse error.
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallbackParseError {
    /// Payload is malformed or truncated.
    #[error("invalid ED2K callback payload")]
    InvalidPayload,
}

/// LowID callback state.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LowIdCallbackState {
    /// Callback is not required for this peer.
    NotNeeded,
    /// LowID peer has not requested a callback yet.
    Needed,
    /// Callback request was sent to the server.
    Requested {
        /// Request timestamp in caller-owned monotonic seconds.
        requested_at: u64,
    },
    /// Server accepted the callback and returned a reachable endpoint.
    Accepted {
        /// Acceptance timestamp in caller-owned monotonic seconds.
        accepted_at: u64,
    },
    /// Server reported callback failure.
    Failed {
        /// Failure timestamp in caller-owned monotonic seconds.
        failed_at: u64,
    },
    /// Callback wait expired.
    TimedOut {
        /// Timeout timestamp in caller-owned monotonic seconds.
        timed_out_at: u64,
    },
    /// Peer cannot be reached through supported callback paths.
    Impossible,
    /// Callback path completed and no longer blocks scheduling.
    Completed {
        /// Completion timestamp in caller-owned monotonic seconds.
        completed_at: u64,
    },
}

/// Native peer scheduling state for reachability decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSchedulingState {
    /// ED2K client ID or LowID.
    pub client_id: u32,
    /// Last known TCP port.
    pub tcp_port: u16,
    /// Whether the peer has a LowID-shaped ID.
    pub low_id: bool,
    /// LowID callback state.
    pub callback_state: LowIdCallbackState,
    /// Last callback endpoint accepted by the server.
    pub callback_endpoint: Option<CallbackEndpoint>,
}

impl PeerSchedulingState {
    /// Create scheduling state from a server or peer source record.
    pub fn from_source(client_id: u32, tcp_port: u16) -> Self {
        let low_id = is_low_id(client_id);
        Self {
            client_id,
            tcp_port,
            low_id,
            callback_state: if low_id {
                LowIdCallbackState::Needed
            } else {
                LowIdCallbackState::NotNeeded
            },
            callback_endpoint: None,
        }
    }

    /// Return whether the peer can be scheduled for a direct TCP connection.
    pub fn can_connect_directly(&self, _now_seconds: u64) -> bool {
        if !self.low_id {
            return true;
        }
        matches!(
            self.callback_state,
            LowIdCallbackState::Accepted { .. } | LowIdCallbackState::Completed { .. }
        ) && self.callback_endpoint.is_some()
    }

    /// Build and mark a server-mediated callback request for a LowID peer.
    pub fn request_server_callback(&mut self, now_seconds: u64) -> Option<PacketFrame> {
        if !self.low_id || matches!(self.callback_state, LowIdCallbackState::Requested { .. }) {
            return None;
        }
        self.callback_state = LowIdCallbackState::Requested {
            requested_at: now_seconds,
        };
        Some(PacketFrame {
            protocol: Protocol::Edonkey,
            opcode: ServerOpcode::CallbackRequest.into(),
            payload: self.client_id.to_le_bytes().to_vec(),
        })
    }

    /// Accept a server-mediated callback endpoint.
    pub fn accept_server_callback(&mut self, endpoint: CallbackEndpoint, now_seconds: u64) {
        self.callback_endpoint = Some(endpoint);
        self.callback_state = LowIdCallbackState::Accepted {
            accepted_at: now_seconds,
        };
    }

    /// Mark server-mediated callback failure.
    pub fn fail_server_callback(&mut self, now_seconds: u64) {
        self.callback_endpoint = None;
        self.callback_state = LowIdCallbackState::Failed {
            failed_at: now_seconds,
        };
    }

    /// Expire a pending callback request when the wait exceeds `timeout_seconds`.
    pub fn expire_callback(&mut self, now_seconds: u64, timeout_seconds: u64) -> bool {
        let LowIdCallbackState::Requested { requested_at } = self.callback_state else {
            return false;
        };
        if now_seconds.saturating_sub(requested_at) <= timeout_seconds {
            return false;
        }
        self.callback_endpoint = None;
        self.callback_state = LowIdCallbackState::TimedOut {
            timed_out_at: now_seconds,
        };
        true
    }

    /// Mark the callback path completed.
    pub fn complete_callback(&mut self, now_seconds: u64) {
        self.callback_state = LowIdCallbackState::Completed {
            completed_at: now_seconds,
        };
    }
}

/// Server-mediated callback capability truth.
pub struct ServerMediatedCallback;

impl ServerMediatedCallback {
    /// Return whether direct UDP callback is implemented and advertised.
    pub fn supports_direct_udp_callback() -> bool {
        false
    }

    /// Return whether Kad buddy callback is implemented and advertised.
    pub fn supports_kad_buddy_callback() -> bool {
        false
    }

    /// Return whether required-crypt callback is implemented and advertised.
    pub fn supports_required_crypt_callback() -> bool {
        false
    }
}
