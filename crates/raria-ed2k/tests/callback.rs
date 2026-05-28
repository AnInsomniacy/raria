use raria_ed2k::opcode::ServerOpcode;
use raria_ed2k::packet::Protocol;
use raria_ed2k::peer::{
    CallbackEndpoint, LowIdCallbackState, PeerSchedulingState, ServerMediatedCallback,
};

#[test]
fn high_id_sources_are_schedulable_without_callback() {
    let peer = PeerSchedulingState::from_source(0x0102_0304, 4662);

    assert!(peer.can_connect_directly(100));
    assert_eq!(peer.callback_state, LowIdCallbackState::NotNeeded);
}

#[test]
fn low_id_sources_wait_for_server_callback_before_connecting() {
    let mut peer = PeerSchedulingState::from_source(7, 4662);

    assert!(!peer.can_connect_directly(100));
    let request = peer.request_server_callback(100).expect("callback request");
    assert_eq!(request.protocol, Protocol::Edonkey);
    assert_eq!(request.opcode, u8::from(ServerOpcode::CallbackRequest));
    assert_eq!(request.payload, 7_u32.to_le_bytes());
    assert_eq!(
        peer.callback_state,
        LowIdCallbackState::Requested { requested_at: 100 }
    );

    peer.accept_server_callback(
        CallbackEndpoint::parse_server_payload(7, &[4, 3, 2, 1, 0x36, 0x12]).unwrap(),
        110,
    );

    assert!(peer.can_connect_directly(111));
    assert_eq!(
        peer.callback_state,
        LowIdCallbackState::Accepted { accepted_at: 110 }
    );
}

#[test]
fn callback_endpoint_parses_server_payload_metadata() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
    payload.extend_from_slice(&4662_u16.to_le_bytes());
    payload.push(0x08);
    payload.extend_from_slice(&[0xaa; 16]);

    let endpoint = CallbackEndpoint::parse_server_payload(7, &payload).unwrap();

    assert_eq!(endpoint.client_id, 7);
    assert_eq!(endpoint.ip, 0x0102_0304);
    assert_eq!(endpoint.tcp_port, 4662);
    assert_eq!(endpoint.crypt_options, Some(0x08));
    assert_eq!(endpoint.user_hash, Some([0xaa; 16]));
}

#[test]
fn callback_failure_and_timeout_isolate_unreachable_low_id_sources() {
    let mut peer = PeerSchedulingState::from_source(9, 4662);
    peer.request_server_callback(10).expect("callback request");

    assert!(!peer.expire_callback(39, 30));
    assert!(peer.expire_callback(41, 30));
    assert_eq!(
        peer.callback_state,
        LowIdCallbackState::TimedOut { timed_out_at: 41 }
    );
    assert!(!peer.can_connect_directly(42));

    peer.fail_server_callback(50);
    assert_eq!(
        peer.callback_state,
        LowIdCallbackState::Failed { failed_at: 50 }
    );
    assert!(!peer.can_connect_directly(51));
}

#[test]
fn unsupported_callback_modes_stay_unadvertised() {
    assert!(!ServerMediatedCallback::supports_direct_udp_callback());
    assert!(!ServerMediatedCallback::supports_kad_buddy_callback());
    assert!(!ServerMediatedCallback::supports_required_crypt_callback());
}
