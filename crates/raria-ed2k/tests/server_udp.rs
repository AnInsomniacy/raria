use std::time::Duration;

use raria_ed2k::opcode::ServerOpcode;
use raria_ed2k::packet::Protocol;
use raria_ed2k::server::{
    ServerRequestCadence, ServerUdpState, ServerUdpStatus, build_global_get_sources_request,
    build_udp_status_request, parse_udp_found_sources_payloads,
};

#[test]
fn udp_status_request_and_reply_update_challenge_bound_metadata() {
    let request = build_udp_status_request(0x1122_3344);
    assert_eq!(request.protocol, Protocol::Edonkey);
    assert_eq!(
        request.opcode,
        u8::from(ServerOpcode::GlobalServerStatusRequest)
    );
    assert_eq!(request.payload, 0x1122_3344_u32.to_le_bytes());

    let mut state = ServerUdpState::new(0x1122_3344);
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x1122_3344_u32.to_le_bytes());
    payload.extend_from_slice(&42_u32.to_le_bytes());
    payload.extend_from_slice(&700_u32.to_le_bytes());
    payload.extend_from_slice(&9000_u32.to_le_bytes());
    payload.extend_from_slice(&100_u32.to_le_bytes());
    payload.extend_from_slice(&200_u32.to_le_bytes());
    payload.extend_from_slice(&0x0100_u32.to_le_bytes());
    payload.extend_from_slice(&7_u32.to_le_bytes());
    payload.extend_from_slice(&4675_u16.to_le_bytes());
    payload.extend_from_slice(&4665_u16.to_le_bytes());
    payload.extend_from_slice(&0xaabb_ccdd_u32.to_le_bytes());

    let status = state.apply_status_response(&payload).expect("status");

    assert_eq!(
        status,
        ServerUdpStatus {
            challenge: 0x1122_3344,
            users: 42,
            files: 700,
            max_users: Some(9000),
            soft_files: Some(100),
            hard_files: Some(200),
            udp_flags: Some(0x0100),
            low_id_users: Some(7),
            udp_obfuscation_port: Some(4675),
            tcp_obfuscation_port: Some(4665),
            udp_key: Some(0xaabb_ccdd)
        }
    );
    assert!(state.expected_challenge.is_none());
}

#[test]
fn udp_source_requests_use_hash_only_or_extended_hash_size_payloads() {
    let hash = [0x55; 16];
    let legacy = build_global_get_sources_request(hash, 700, false);
    assert_eq!(legacy.opcode, u8::from(ServerOpcode::GlobalGetSources));
    assert_eq!(legacy.payload, hash);

    let extended = build_global_get_sources_request(hash, u64::from(u32::MAX) + 5, true);
    assert_eq!(extended.opcode, u8::from(ServerOpcode::GlobalGetSources2));
    assert_eq!(&extended.payload[0..16], &hash);
    assert_eq!(
        u32::from_le_bytes(extended.payload[16..20].try_into().unwrap()),
        0
    );
    assert_eq!(
        u64::from_le_bytes(extended.payload[20..28].try_into().unwrap()),
        u64::from(u32::MAX) + 5
    );
}

#[test]
fn packed_udp_found_sources_keep_matching_sources_and_stop_at_bogus_tail() {
    let expected = [0x66; 16];
    let other = [0x77; 16];
    let mut payload = Vec::new();
    payload.extend_from_slice(&expected);
    payload.push(1);
    payload.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
    payload.extend_from_slice(&4662_u16.to_le_bytes());
    payload.push(0xe3);
    payload.push(ServerOpcode::GlobalFoundSources.into());
    payload.extend_from_slice(&other);
    payload.push(1);
    payload.extend_from_slice(&0x0506_0708_u32.to_le_bytes());
    payload.extend_from_slice(&4663_u16.to_le_bytes());
    payload.extend_from_slice(&[0xaa, 0xbb]);

    let sources = parse_udp_found_sources_payloads(&payload, expected).expect("sources");

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].client_id, 0x0102_0304);
    assert_eq!(sources[0].tcp_port, 4662);
}

#[test]
fn server_request_cadence_keeps_udp_polling_bounded() {
    let cadence = ServerRequestCadence::default();

    assert!(cadence.status_due(None, 100));
    assert!(!cadence.status_due(Some(90), 100));
    assert!(cadence.status_due(Some(0), 100));
    assert!(!cadence.source_due(Some(95), 100));
    assert!(cadence.source_due(Some(0), 100));
    assert_eq!(cadence.status_interval, Duration::from_secs(60));
}
