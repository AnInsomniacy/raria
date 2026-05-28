use std::time::Duration;

use raria_ed2k::opcode::ServerOpcode;
use raria_ed2k::packet::{PacketFrame, Protocol};
use raria_ed2k::server::{
    FoundSource, ServerCapabilities, ServerReachability, ServerRetryPolicy, ServerStatus,
    ServerTcpEvent, ServerTcpState, SourceObfuscation, build_get_sources_request,
    build_login_request, is_low_id,
};
use raria_ed2k::tag::{Tag, TagName, TagValue, encode_tag};

#[test]
fn login_request_uses_native_identity_and_server_capability_tags() {
    let frame = build_login_request([0x11; 16], 0, 4662, "raria", 0x3c, 0x0102_0304)
        .expect("login request");

    assert_eq!(frame.protocol, Protocol::Edonkey);
    assert_eq!(frame.opcode, u8::from(ServerOpcode::LoginRequest));
    assert_eq!(&frame.payload[0..16], &[0x11; 16]);
    assert_eq!(
        u32::from_le_bytes(frame.payload[16..20].try_into().unwrap()),
        0
    );
    assert_eq!(
        u16::from_le_bytes(frame.payload[20..22].try_into().unwrap()),
        4662
    );
    assert_eq!(
        u32::from_le_bytes(frame.payload[22..26].try_into().unwrap()),
        4
    );
    assert!(frame.payload.windows(5).any(|window| window == b"raria"));
    assert!(frame.payload.contains(&0x20));
}

#[test]
fn server_state_tracks_idchange_status_identity_and_messages() {
    let mut state = ServerTcpState::new("server.example", 4661);
    let mut idchange = Vec::new();
    idchange.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
    idchange.extend_from_slice(&ServerCapabilities::default_login().bits().to_le_bytes());
    idchange.extend_from_slice(&4661_u32.to_le_bytes());
    idchange.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
    idchange.extend_from_slice(&4665_u32.to_le_bytes());

    assert_eq!(
        state
            .apply_frame(&PacketFrame {
                protocol: Protocol::Edonkey,
                opcode: ServerOpcode::IdChange.into(),
                payload: idchange,
            })
            .expect("idchange"),
        ServerTcpEvent::IdChanged {
            client_id: 0x0102_0304,
            reachability: ServerReachability::HighId
        }
    );
    assert_eq!(state.reachability, Some(ServerReachability::HighId));
    assert_eq!(state.tcp_obfuscation_port, Some(4665));

    let status = state
        .apply_frame(&PacketFrame {
            protocol: Protocol::Edonkey,
            opcode: ServerOpcode::ServerStatus.into(),
            payload: [42_u32.to_le_bytes(), 700_u32.to_le_bytes()].concat(),
        })
        .expect("status");
    assert_eq!(
        status,
        ServerTcpEvent::Status(ServerStatus {
            users: 42,
            files: 700
        })
    );

    let mut ident = Vec::new();
    ident.extend_from_slice(&[0x22; 16]);
    ident.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
    ident.extend_from_slice(&4661_u16.to_le_bytes());
    ident.extend_from_slice(&2_u32.to_le_bytes());
    ident.extend_from_slice(
        &encode_tag(&Tag::new(
            TagName::Id(0x01),
            TagValue::String("Primary".into()),
        ))
        .unwrap(),
    );
    ident.extend_from_slice(
        &encode_tag(&Tag::new(
            TagName::Id(0x0b),
            TagValue::String("Stable server".into()),
        ))
        .unwrap(),
    );

    assert_eq!(
        state
            .apply_frame(&PacketFrame {
                protocol: Protocol::Edonkey,
                opcode: ServerOpcode::ServerIdentity.into(),
                payload: ident,
            })
            .expect("identity"),
        ServerTcpEvent::IdentityUpdated
    );
    assert_eq!(state.name.as_deref(), Some("Primary"));
    assert_eq!(state.description.as_deref(), Some("Stable server"));

    assert_eq!(
        state
            .apply_frame(&PacketFrame {
                protocol: Protocol::Edonkey,
                opcode: ServerOpcode::ServerMessage.into(),
                payload: [5_u16.to_le_bytes().as_slice(), b"hello"].concat(),
            })
            .expect("message"),
        ServerTcpEvent::Message("hello".into())
    );
}

#[test]
fn source_requests_and_found_sources_handle_large_files_and_obfuscation() {
    let hash = [0x33; 16];
    let small = build_get_sources_request(hash, 700, false).expect("small request");
    assert_eq!(small.opcode, u8::from(ServerOpcode::GetSources));
    assert_eq!(small.payload.len(), 20);
    assert_eq!(
        u32::from_le_bytes(small.payload[16..20].try_into().unwrap()),
        700
    );

    let large =
        build_get_sources_request(hash, u64::from(u32::MAX) + 1, true).expect("large request");
    assert_eq!(large.opcode, u8::from(ServerOpcode::GetSourcesObfuscated));
    assert_eq!(large.payload.len(), 28);
    assert_eq!(
        u32::from_le_bytes(large.payload[16..20].try_into().unwrap()),
        0
    );

    let mut payload = Vec::new();
    payload.extend_from_slice(&hash);
    payload.push(2);
    payload.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
    payload.extend_from_slice(&4662_u16.to_le_bytes());
    payload.push(0x08);
    payload.extend_from_slice(&[0x44; 16]);
    payload.extend_from_slice(&7_u32.to_le_bytes());
    payload.extend_from_slice(&4663_u16.to_le_bytes());
    payload.push(0);

    let mut state = ServerTcpState::new("server.example", 4661);
    let event = state
        .apply_frame(&PacketFrame {
            protocol: Protocol::Edonkey,
            opcode: ServerOpcode::FoundSourcesObfuscated.into(),
            payload,
        })
        .expect("sources");

    assert_eq!(
        event,
        ServerTcpEvent::FoundSources {
            file_hash: hash,
            sources: vec![
                FoundSource {
                    client_id: 0x0102_0304,
                    tcp_port: 4662,
                    obfuscation: Some(SourceObfuscation {
                        options: 0x08,
                        user_hash: Some([0x44; 16])
                    })
                },
                FoundSource {
                    client_id: 7,
                    tcp_port: 4663,
                    obfuscation: Some(SourceObfuscation {
                        options: 0,
                        user_hash: None
                    })
                }
            ]
        }
    );
    assert!(is_low_id(7));
    assert!(!is_low_id(0x0102_0304));
}

#[test]
fn retry_policy_uses_bounded_backoff_without_erasing_metadata() {
    let policy = ServerRetryPolicy::default();
    let first = policy.next_delay(1);
    let fourth = policy.next_delay(4);
    let capped = policy.next_delay(99);

    assert!(first < fourth);
    assert_eq!(capped, Duration::from_secs(15 * 60));
}
